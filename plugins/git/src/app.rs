//! The plugin as Kalem runs it, phase 0 (DESIGN.md, section 8): the status
//! bar item, the Git panel, the leader's keys on the file being edited,
//! commit, fetch, pull and push, a line's blame.
//!
//! [`App`] is a state machine: it takes an [`Input`] (a command with the
//! document it runs in, how a git run ended, the user's answer, a click in
//! the panel, an event) and gives [`Effect`]s (run git, notify, set the
//! status bar item or the panel, ask). It knows nothing of Kalem's types,
//! so its tests drive it with a real git and scripted answers; the
//! component (`component.rs`) turns its effects into `kalem_plugin` calls
//! and its callbacks back into inputs.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::actions::{self, Act};
use crate::git::blame;
use crate::git::cmd::{self, CommitKind, GitCommand, PullMode, Push, Untracked};
use crate::git::status::Kind;
use crate::model::{Repo, Section};
use crate::panel::{Node, TextStyle};
use crate::refresh::{self, Output, Part, Refresh};
use crate::target::Target;
use crate::views::Glyphs;

/// The panel's ID.
pub const PANEL: &str = "git.panel";

/// The status bar item's ID.
pub const STATUS: &str = "git.status";

/// A command of the plugin: its ID, title, default keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    /// `git.NAME`.
    pub id: &'static str,
    /// For the palette and the menus.
    pub title: &'static str,
    /// Its default keys, as `keymap.json` writes them.
    pub keys: &'static [&'static str],
}

/// The commands of phase 0, with Doom Emacs's keys (DESIGN.md, 5.2).
pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        id: "git.status",
        title: "Git: Status",
        keys: &["space g g", "ctrl+shift+g"],
    },
    CommandInfo {
        id: "git.stageFile",
        title: "Git: Stage File",
        keys: &["space g shift+s"],
    },
    CommandInfo {
        id: "git.unstageFile",
        title: "Git: Unstage File",
        keys: &["space g shift+u"],
    },
    CommandInfo {
        id: "git.revertFile",
        title: "Git: Revert File",
        keys: &["space g shift+r"],
    },
    CommandInfo {
        id: "git.deleteFile",
        title: "Git: Delete File",
        keys: &["space g shift+d"],
    },
    CommandInfo {
        id: "git.commit",
        title: "Git: Commit",
        keys: &["space g c c"],
    },
    CommandInfo {
        id: "git.fetch",
        title: "Git: Fetch",
        keys: &["space g shift+f"],
    },
    CommandInfo {
        id: "git.pull",
        title: "Git: Pull",
        keys: &[],
    },
    CommandInfo {
        id: "git.push",
        title: "Git: Push",
        keys: &[],
    },
    CommandInfo {
        id: "git.blameLine",
        title: "Git: Blame This Line",
        keys: &["space g shift+b"],
    },
    CommandInfo {
        id: "git.refresh",
        title: "Git: Refresh",
        keys: &[],
    },
    CommandInfo {
        id: "git.fileMenu",
        title: "Git: This File",
        keys: &["space g ."],
    },
    CommandInfo {
        id: "git.dispatch",
        title: "Git: Commands",
        keys: &["space g /"],
    },
];

/// What the plugin's settings say (DESIGN.md, section 6), as phase 0 uses
/// them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Reverting asks first.
    pub confirm_discard: bool,
    /// How Pull pulls.
    pub pull: PullMode,
    /// Fetch prunes.
    pub fetch_prune: bool,
    /// Which untracked files are listed.
    pub untracked: Untracked,
    /// Commits are signed off.
    pub signoff: bool,
    /// The marks.
    pub glyphs: Glyphs,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            confirm_discard: true,
            pull: PullMode::Config,
            fetch_prune: true,
            untracked: Untracked::Normal,
            signoff: false,
            glyphs: Glyphs::Unicode,
        }
    }
}

impl Settings {
    /// The settings from the plugin's own, as JSON text by key; a value
    /// missing or of another type is the default.
    pub fn read(own: impl Fn(&str) -> Option<String>) -> Settings {
        let d = Settings::default();
        let boolean = |k: &str, d: bool| own(k).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
        let text = |k: &str| own(k).map(|v| v.trim().trim_matches('"').to_string());
        Settings {
            confirm_discard: boolean("confirm_discard", d.confirm_discard),
            pull: text("pull")
                .and_then(|v| PullMode::parse(&v))
                .unwrap_or(d.pull),
            fetch_prune: boolean("fetch_prune", d.fetch_prune),
            untracked: text("untracked")
                .and_then(|v| Untracked::parse(&v))
                .unwrap_or(d.untracked),
            signoff: boolean("signoff", d.signoff),
            glyphs: text("glyphs")
                .and_then(|v| Glyphs::parse(&v))
                .unwrap_or(d.glyphs),
        }
    }
}

/// How much a notification matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// News.
    Info,
    /// Something to look at.
    Warning,
    /// Something failed.
    Error,
}

/// The user's answer to a question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Yes or no; cancelled is no.
    Confirmed(bool),
    /// A line of text; `None` when cancelled.
    Text(Option<String>),
    /// The entries chosen, by index; none when cancelled.
    Picked(Vec<u32>),
}

/// What the user did to a widget of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelEvent {
    /// A button or an entry clicked.
    Clicked,
    /// A box ticked (`true`) or not.
    Toggled(bool),
    /// An entry's children shown (`true`) or hidden.
    Expanded(bool),
}

/// The document a command runs in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Doc {
    /// Its file, absolute.
    pub path: Option<String>,
    /// The cursor's line, from 1.
    pub line: u32,
    /// Its text, for a command that reads it (a line's blame).
    pub text: Option<String>,
}

/// What the plugin hears.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// One of [`COMMANDS`] runs in `doc`.
    Command {
        /// Its ID.
        id: String,
        /// The document it runs in, when it runs in one.
        doc: Option<Doc>,
    },
    /// Git run `token` ended, or could not start.
    Done {
        /// The run, as [`Effect::Run`] named it.
        token: u64,
        /// How it ended.
        result: Result<Output, String>,
    },
    /// The user answered question `token`.
    Answer {
        /// The question.
        token: u64,
        /// The answer.
        answer: Answer,
    },
    /// The user acted on widget `key` of the panel.
    Panel {
        /// The widget.
        key: String,
        /// What they did.
        event: PanelEvent,
    },
    /// A document opened, by its file.
    Opened(String),
    /// A document saved, by its file.
    Saved(String),
    /// A file of a project changed on disk.
    Changed(String),
    /// The plugin's settings changed.
    Settings(Settings),
}

/// What the plugin does.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Runs git in `cwd` (a project's folder; the first project's when
    /// none); its end comes back as [`Input::Done`] with `token`.
    Run {
        /// Its number.
        token: u64,
        /// Its folder.
        cwd: Option<String>,
        /// The command.
        command: GitCommand,
    },
    /// Shows a message.
    Notify(String, Level),
    /// Sets the status bar item; `None` takes it away.
    Status(Option<String>),
    /// Replaces the panel's content.
    Panel(Node),
    /// Shows the panel.
    ShowPanel,
    /// Asks yes or no; the answer comes back with `token`.
    Confirm {
        /// The question's number.
        token: u64,
        /// The question.
        message: String,
    },
    /// Asks for a line of text.
    Prompt {
        /// The question's number.
        token: u64,
        /// What is asked.
        title: String,
    },
    /// Offers a list to choose from.
    Pick {
        /// The question's number.
        token: u64,
        /// The list's title.
        title: String,
        /// The entries: a label and a second line.
        items: Vec<(String, Option<String>)>,
    },
    /// Runs one of Kalem's commands with its arguments as JSON.
    RunCommand {
        /// The command.
        id: String,
        /// Its arguments.
        args: String,
    },
    /// Sets one of the plugin's own settings to a JSON value.
    SetSetting {
        /// The key.
        key: String,
        /// The value.
        value: String,
    },
}

/// What to do once the repository is found and its status read.
#[derive(Debug, Clone, PartialEq)]
enum After {
    /// Nothing more: the status bar item and the panel follow.
    Nothing,
    /// An action on a file, by its path in the repository.
    Act(Act, String),
    /// Ask for a message and commit.
    Commit,
    /// Fetch.
    Fetch,
    /// Pull.
    Pull,
    /// Push.
    Push,
    /// Show the commit of a line.
    Blame {
        /// The file in the repository.
        path: String,
        /// The line, from 1.
        line: u32,
        /// The document's text.
        text: Option<String>,
    },
    /// Offer the file's commands.
    FileMenu(Doc),
}

/// What a run is for.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    /// Finding the repository of a folder.
    Locate {
        folder: Option<String>,
        file: Option<String>,
        after: After,
    },
    /// A part of a refresh.
    Refresh {
        root: String,
        generation: u64,
        part: Part,
    },
    /// A command of a plan; the rest follow it.
    Plan {
        root: String,
        rest: VecDeque<GitCommand>,
        done: String,
    },
    /// Fetch, pull or push.
    Remote { root: String, op: RemoteOp },
    /// The remotes, for a push without an upstream.
    Remotes { root: String, branch: String },
    /// A line's blame.
    Blame { line: u32 },
}

/// A network operation.
#[derive(Debug, Clone, PartialEq)]
enum RemoteOp {
    Fetch,
    Pull(PullMode),
    /// A pull before a push.
    PullThenPush,
    Push(Push),
}

/// What an answer is for.
#[derive(Debug, Clone, PartialEq)]
enum Question {
    /// Run the plan when yes.
    Plan {
        root: String,
        commands: Vec<GitCommand>,
        done: String,
    },
    /// The commit message.
    Message { root: String },
    /// One of these commands.
    Commands {
        ids: Vec<&'static str>,
        doc: Option<Doc>,
    },
    /// How to pull, remembered.
    PullHow { root: String, then_push: bool },
    /// Where to push a branch without an upstream.
    PushTo {
        root: String,
        branch: String,
        remotes: Vec<String>,
    },
    /// What to do when the upstream has commits the branch lacks.
    PushBehind { root: String },
    /// Force with lease, when yes.
    Force { root: String },
}

/// A repository and the refresh of its status.
#[derive(Debug, Default)]
struct RepoState {
    repo: Option<Repo>,
    refresh: Option<Refresh>,
    generation: u64,
    /// What waits for the refresh running.
    running: Vec<After>,
    /// What waits for the next one.
    waiting: Vec<After>,
}

/// The plugin's state.
#[derive(Debug, Default)]
pub struct App {
    settings: Settings,
    next: u64,
    /// Folders found, by their path: the repository's root and the
    /// folder's place in it.
    folders: BTreeMap<String, (String, String)>,
    repos: BTreeMap<String, RepoState>,
    /// The repository of the last document opened, saved or acted on.
    current: Option<String>,
    pending: BTreeMap<u64, Pending>,
    questions: BTreeMap<u64, Question>,
    /// The panel's sections folded.
    folded: BTreeSet<Section>,
    /// The network operation under way, for the panel and the status bar.
    busy: Option<String>,
    /// The last failure, shown in the panel until the next success.
    error: Option<String>,
}

fn parent(path: &str) -> (String, String) {
    match path.rfind(['/', '\\']) {
        Some(0) => ("/".into(), path[1..].into()),
        Some(i) => (path[..i].into(), path[i + 1..].into()),
        None => (".".into(), path.into()),
    }
}

impl App {
    /// The plugin with its settings.
    pub fn new(settings: Settings) -> App {
        App {
            settings,
            ..App::default()
        }
    }

    /// The repository shown, when one is.
    pub fn current(&self) -> Option<&Repo> {
        self.repos.get(self.current.as_ref()?)?.repo.as_ref()
    }

    fn token(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn run(
        &mut self,
        out: &mut Vec<Effect>,
        cwd: Option<String>,
        command: GitCommand,
        why: Pending,
    ) {
        let token = self.token();
        self.pending.insert(token, why);
        out.push(Effect::Run {
            token,
            cwd,
            command,
        });
    }

    fn ask(&mut self, q: Question) -> u64 {
        let token = self.token();
        self.questions.insert(token, q);
        token
    }

    fn fail(&mut self, out: &mut Vec<Effect>, message: String) {
        self.error = Some(message.clone());
        out.push(Effect::Notify(message, Level::Error));
        self.show(out);
    }

    /// Handles `input`; what follows from it.
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        let mut out = Vec::new();
        match input {
            Input::Command { id, doc } => self.command(&mut out, &id, doc),
            Input::Done { token, result } => {
                if let Some(p) = self.pending.remove(&token) {
                    self.done(&mut out, p, result);
                }
            }
            Input::Answer { token, answer } => {
                if let Some(q) = self.questions.remove(&token) {
                    self.answer(&mut out, q, answer);
                }
            }
            Input::Panel { key, event } => self.panel_event(&mut out, &key, &event),
            Input::Opened(path) | Input::Saved(path) => {
                let (folder, _) = parent(&path);
                self.locate(&mut out, Some(folder), None, After::Nothing, true);
            }
            Input::Changed(path) => {
                if let Some(root) = self.current.clone()
                    && path.starts_with(&root)
                    && !path[root.len()..].starts_with("/.git/")
                {
                    self.refresh(&mut out, &root, After::Nothing);
                }
            }
            Input::Settings(s) => {
                self.settings = s;
                self.show(&mut out);
            }
        }
        out
    }

    fn command(&mut self, out: &mut Vec<Effect>, id: &str, doc: Option<Doc>) {
        let path = doc.as_ref().and_then(|d| d.path.clone());
        let (folder, file) = match &path {
            Some(p) => {
                let (f, n) = parent(p);
                (Some(f), Some(n))
            }
            None => (None, None),
        };
        let needs_file = |out: &mut Vec<Effect>| {
            out.push(Effect::Notify(
                "This command acts on a file: open one of the repository first".into(),
                Level::Warning,
            ));
        };
        let after = match id {
            "git.status" => {
                out.push(Effect::ShowPanel);
                After::Nothing
            }
            "git.refresh" => After::Nothing,
            "git.stageFile" | "git.unstageFile" | "git.revertFile" | "git.deleteFile" => {
                if file.is_none() {
                    return needs_file(out);
                }
                let act = match id {
                    "git.stageFile" => Act::Stage,
                    "git.unstageFile" => Act::Unstage,
                    "git.revertFile" => Act::Discard,
                    _ => Act::Delete,
                };
                After::Act(act, String::new())
            }
            "git.commit" => After::Commit,
            "git.fetch" => After::Fetch,
            "git.pull" => After::Pull,
            "git.push" => After::Push,
            "git.blameLine" => {
                if file.is_none() {
                    return needs_file(out);
                }
                let d = doc.clone().unwrap_or_default();
                After::Blame {
                    path: String::new(),
                    line: d.line.max(1),
                    text: d.text,
                }
            }
            "git.fileMenu" => {
                if file.is_none() {
                    return needs_file(out);
                }
                After::FileMenu(doc.clone().unwrap_or_default())
            }
            "git.dispatch" => {
                let ids: Vec<&'static str> = COMMANDS
                    .iter()
                    .map(|c| c.id)
                    .filter(|i| *i != "git.dispatch")
                    .collect();
                let items = ids
                    .iter()
                    .map(|i| {
                        let c = COMMANDS.iter().find(|c| c.id == *i).unwrap_or(&COMMANDS[0]);
                        (
                            c.title.trim_start_matches("Git: ").to_string(),
                            keys_hint(c.keys),
                        )
                    })
                    .collect();
                let token = self.ask(Question::Commands { ids, doc });
                out.push(Effect::Pick {
                    token,
                    title: "Git".into(),
                    items,
                });
                return;
            }
            other => {
                out.push(Effect::Notify(format!("No command {other}"), Level::Error));
                return;
            }
        };
        let folder = folder.or_else(|| self.current.clone());
        self.locate(out, folder, file, after, true);
    }

    /// Finds the repository of `folder` (the first project's when none),
    /// then refreshes it and does `after`, with `file` (a name in the
    /// folder) made a path in the repository.
    fn locate(
        &mut self,
        out: &mut Vec<Effect>,
        folder: Option<String>,
        file: Option<String>,
        after: After,
        make_current: bool,
    ) {
        if let Some(f) = &folder
            && let Some((root, prefix)) = self.folders.get(f).cloned()
        {
            let after = with_path(after, &prefix, file.as_deref());
            if make_current {
                self.current = Some(root.clone());
            }
            return self.refresh(out, &root, after);
        }
        self.run(
            out,
            folder.clone(),
            cmd::locate(),
            Pending::Locate {
                folder,
                file,
                after,
            },
        );
    }

    fn refresh(&mut self, out: &mut Vec<Effect>, root: &str, after: After) {
        let state = self.repos.entry(root.to_string()).or_default();
        if state.refresh.is_some() {
            state.waiting.push(after);
            return;
        }
        state.generation += 1;
        let generation = state.generation;
        state.running.push(after);
        let options = refresh::Options {
            untracked: self.settings.untracked,
            ..refresh::Options::default()
        };
        let (r, commands) = Refresh::start(root, generation, options);
        state.refresh = Some(r);
        for (part, command) in commands {
            self.run(
                out,
                Some(root.to_string()),
                command,
                Pending::Refresh {
                    root: root.to_string(),
                    generation,
                    part,
                },
            );
        }
    }

    fn done(&mut self, out: &mut Vec<Effect>, pending: Pending, result: Result<Output, String>) {
        match pending {
            Pending::Locate {
                folder,
                file,
                after,
            } => {
                let located = match &result {
                    Ok(o) if o.success() => {
                        let text = String::from_utf8_lossy(&o.stdout).into_owned();
                        let mut lines = text.lines();
                        let root = lines.next().unwrap_or_default().trim_end().to_string();
                        let prefix = lines.next().unwrap_or_default().trim_end().to_string();
                        (!root.is_empty()).then_some((root, prefix))
                    }
                    _ => None,
                };
                let Some((root, prefix)) = located else {
                    // A document outside every repository says nothing;
                    // a command says why it cannot act.
                    if after != After::Nothing {
                        let why = match &result {
                            Err(e)
                                if e.contains("outside the projects")
                                    || e.contains("No project") =>
                            {
                                format!(
                                    "{} is outside Kalem's projects: add the repository as a project (SPC p a) to use git in it",
                                    folder.as_deref().unwrap_or("This folder")
                                )
                            }
                            Err(e) => e.clone(),
                            Ok(o) if o.message().contains("not a git repository") => {
                                "Not in a git repository".into()
                            }
                            Ok(o) => o.message(),
                        };
                        self.fail(out, why);
                    }
                    return;
                };
                if let Some(f) = folder {
                    self.folders.insert(f, (root.clone(), prefix.clone()));
                }
                self.current = Some(root.clone());
                let after = with_path(after, &prefix, file.as_deref());
                self.refresh(out, &root, after);
            }
            Pending::Refresh {
                root,
                generation,
                part,
            } => {
                let (next, finished) = {
                    let Some(state) = self.repos.get_mut(&root) else {
                        return;
                    };
                    if state.generation != generation {
                        return;
                    }
                    let Some(r) = state.refresh.as_mut() else {
                        return;
                    };
                    let next = r.feed(part, result);
                    (next, r.done())
                };
                for (p, c) in next {
                    self.run(
                        out,
                        Some(root.clone()),
                        c,
                        Pending::Refresh {
                            root: root.clone(),
                            generation,
                            part: p,
                        },
                    );
                }
                if finished {
                    self.refreshed(out, &root);
                }
            }
            Pending::Plan {
                root,
                mut rest,
                done,
            } => match result {
                Ok(o) if o.success() => {
                    if let Some(next) = rest.pop_front() {
                        self.run(
                            out,
                            Some(root.clone()),
                            next,
                            Pending::Plan { root, rest, done },
                        );
                    } else {
                        self.error = None;
                        out.push(Effect::Notify(done, Level::Info));
                        self.refresh(out, &root, After::Nothing);
                    }
                }
                other => {
                    self.fail(out, message(&other));
                    self.refresh(out, &root, After::Nothing);
                }
            },
            Pending::Remote { root, op } => self.remote_done(out, root, op, result),
            Pending::Remotes { root, branch } => {
                let remotes: Vec<String> = match &result {
                    Ok(o) if o.success() => String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect(),
                    _ => Vec::new(),
                };
                if remotes.is_empty() {
                    return self.fail(
                        out,
                        "The repository has no remote to push to: add one with git remote add"
                            .into(),
                    );
                }
                let mut remotes = remotes;
                // origin first, as git's own default.
                remotes.sort_by_key(|r| r != "origin");
                let items = remotes
                    .iter()
                    .map(|r| {
                        (
                            format!("Push to {r}/{branch} and set it as the upstream"),
                            None,
                        )
                    })
                    .collect();
                let token = self.ask(Question::PushTo {
                    root,
                    branch,
                    remotes,
                });
                out.push(Effect::Pick {
                    token,
                    title: "The branch has no upstream".into(),
                    items,
                });
            }
            Pending::Blame { line } => match result {
                Ok(o) if o.success() => match blame::parse(&o.stdout) {
                    Ok(b) => {
                        let Some(l) = b.lines.first() else {
                            return out.push(Effect::Notify(
                                format!("Line {line}: no blame"),
                                Level::Warning,
                            ));
                        };
                        let text = if l.hash == blame::UNCOMMITTED {
                            format!("Line {line} is not committed yet")
                        } else {
                            let c = b.commits.get(&l.hash).cloned().unwrap_or_default();
                            format!(
                                "{} · {} · {} · {}",
                                l.hash.get(..7).unwrap_or(&l.hash),
                                c.author,
                                c.day(),
                                c.summary
                            )
                        };
                        out.push(Effect::Notify(text, Level::Info));
                    }
                    Err(e) => self.fail(out, e),
                },
                other => self.fail(out, message(&other)),
            },
        }
    }

    fn refreshed(&mut self, out: &mut Vec<Effect>, root: &str) {
        let (result, afters, waiting) = {
            let Some(state) = self.repos.get_mut(root) else {
                return;
            };
            let Some(r) = state.refresh.take() else {
                return;
            };
            (
                r.finish(),
                std::mem::take(&mut state.running),
                std::mem::take(&mut state.waiting),
            )
        };
        match result {
            Ok(repo) => {
                if let Some(s) = self.repos.get_mut(root) {
                    s.repo = Some(repo);
                }
            }
            Err(e) => {
                if let Some(s) = self.repos.get_mut(root) {
                    s.repo = None;
                }
                if afters.iter().chain(&waiting).any(|a| *a != After::Nothing) {
                    self.fail(out, e);
                } else {
                    self.show(out);
                }
                return;
            }
        }
        // What came during this refresh waits for the next one.
        for w in waiting {
            self.refresh(out, root, w);
        }
        self.show(out);
        for a in afters {
            self.after(out, root, a);
        }
    }

    /// Does `after` in `root`, its status fresh.
    fn after(&mut self, out: &mut Vec<Effect>, root: &str, after: After) {
        let Some(repo) = self.repos.get(root).and_then(|s| s.repo.clone()) else {
            return;
        };
        match after {
            After::Nothing => {}
            After::Act(act, path) => self.act(out, root, &repo, act, &path),
            After::Commit => {
                if repo.entries(Section::Staged).is_empty() {
                    out.push(Effect::Notify(
                        "Nothing is staged: stage changes first (SPC g S)".into(),
                        Level::Warning,
                    ));
                    return;
                }
                let n = repo.entries(Section::Staged).len();
                let token = self.ask(Question::Message {
                    root: root.to_string(),
                });
                out.push(Effect::Prompt {
                    token,
                    title: format!(
                        "Commit {} staged file{}: the message",
                        n,
                        if n == 1 { "" } else { "s" }
                    ),
                });
            }
            After::Fetch => self.remote(out, root, RemoteOp::Fetch),
            After::Pull => self.remote(out, root, RemoteOp::Pull(self.settings.pull)),
            After::Push => self.push(out, root, &repo),
            After::Blame { path, line, text } => {
                let c = cmd::blame_line(&path, line, text.map(String::into_bytes));
                self.run(out, Some(root.to_string()), c, Pending::Blame { line });
            }
            After::FileMenu(doc) => {
                let ids = vec![
                    "git.stageFile",
                    "git.unstageFile",
                    "git.revertFile",
                    "git.deleteFile",
                    "git.blameLine",
                    "git.status",
                ];
                let items = ids
                    .iter()
                    .map(|i| {
                        let c = COMMANDS.iter().find(|c| c.id == *i).unwrap_or(&COMMANDS[0]);
                        (
                            c.title.trim_start_matches("Git: ").to_string(),
                            keys_hint(c.keys),
                        )
                    })
                    .collect();
                let token = self.ask(Question::Commands {
                    ids,
                    doc: Some(doc),
                });
                out.push(Effect::Pick {
                    token,
                    title: "This file".into(),
                    items,
                });
            }
        }
    }

    fn act(&mut self, out: &mut Vec<Effect>, root: &str, repo: &Repo, act: Act, path: &str) {
        let entry = |s: Section| repo.entry(s, path).is_some();
        let unmerged = entry(Section::Unmerged);
        let untracked = entry(Section::Untracked)
            || repo
                .entries(Section::Untracked)
                .iter()
                .any(|e| e.path.ends_with('/') && path.starts_with(&e.path));
        let section = match act {
            Act::Stage if unmerged => Some(Section::Unmerged),
            Act::Stage if untracked => Some(Section::Untracked),
            Act::Stage if entry(Section::Unstaged) => Some(Section::Unstaged),
            Act::Unstage if entry(Section::Staged) => Some(Section::Staged),
            Act::Discard if unmerged => None,
            Act::Discard if untracked => Some(Section::Untracked),
            Act::Discard if entry(Section::Unstaged) => Some(Section::Unstaged),
            Act::Discard if entry(Section::Staged) => Some(Section::Staged),
            Act::Delete if untracked => Some(Section::Untracked),
            Act::Delete => Some(Section::Unstaged),
            _ => None,
        };
        let Some(section) = section else {
            let why = match act {
                Act::Stage => format!("{path} has nothing to stage"),
                Act::Unstage => format!("{path} has nothing staged"),
                Act::Discard if unmerged => {
                    format!("{path} is in conflict: resolve it in the file")
                }
                _ => format!("{path} has no changes"),
            };
            out.push(Effect::Notify(why, Level::Warning));
            return;
        };
        let settings = actions::Settings {
            confirm_discard: self.settings.confirm_discard,
            zero_context: false,
        };
        let target = Target::File {
            section,
            path: path.to_string(),
        };
        match actions::plan(act, &[target], repo, settings) {
            Ok(plan) => self.start_plan(out, root, plan),
            Err(e) => out.push(Effect::Notify(e, Level::Warning)),
        }
    }

    fn start_plan(&mut self, out: &mut Vec<Effect>, root: &str, plan: actions::Plan) {
        if let Some(message) = plan.confirm {
            let token = self.ask(Question::Plan {
                root: root.to_string(),
                commands: plan.commands,
                done: plan.done,
            });
            out.push(Effect::Confirm { token, message });
            return;
        }
        self.run_plan(out, root, plan.commands, plan.done);
    }

    fn run_plan(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        commands: Vec<GitCommand>,
        done: String,
    ) {
        let mut rest: VecDeque<GitCommand> = commands.into();
        let Some(first) = rest.pop_front() else {
            return;
        };
        self.run(
            out,
            Some(root.to_string()),
            first,
            Pending::Plan {
                root: root.to_string(),
                rest,
                done,
            },
        );
    }

    fn remote(&mut self, out: &mut Vec<Effect>, root: &str, op: RemoteOp) {
        let (busy, command) = match &op {
            RemoteOp::Fetch => ("Fetching…", cmd::fetch(self.settings.fetch_prune)),
            RemoteOp::Pull(mode) => ("Pulling…", cmd::pull(*mode)),
            RemoteOp::PullThenPush => ("Pulling…", cmd::pull(self.settings.pull)),
            RemoteOp::Push(how) => ("Pushing…", cmd::push(how)),
        };
        self.busy = Some(busy.to_string());
        self.show(out);
        self.run(
            out,
            Some(root.to_string()),
            command,
            Pending::Remote {
                root: root.to_string(),
                op,
            },
        );
    }

    fn push(&mut self, out: &mut Vec<Effect>, root: &str, repo: &Repo) {
        let s = &repo.status;
        let Some(branch) = s.branch.clone() else {
            return self.fail(out, "HEAD is detached: switch to a branch to push".into());
        };
        if repo.unborn() {
            return self.fail(out, "There is no commit to push yet".into());
        }
        if s.upstream.is_none() {
            self.run(
                out,
                Some(root.to_string()),
                cmd::remotes(),
                Pending::Remotes {
                    root: root.to_string(),
                    branch,
                },
            );
            return;
        }
        if s.behind > 0 {
            let token = self.ask(Question::PushBehind {
                root: root.to_string(),
            });
            out.push(Effect::Pick {
                token,
                title: format!(
                    "{} has {} commit{} this branch lacks",
                    s.upstream.as_deref().unwrap_or("The upstream"),
                    s.behind,
                    if s.behind == 1 { "" } else { "s" }
                ),
                items: vec![
                    ("Pull first, then push".into(), None),
                    (
                        "Force with lease…".into(),
                        Some("Replaces the upstream's commits: they are lost there".into()),
                    ),
                ],
            });
            return;
        }
        self.remote(out, root, RemoteOp::Push(Push::Plain));
    }

    fn remote_done(
        &mut self,
        out: &mut Vec<Effect>,
        root: String,
        op: RemoteOp,
        result: Result<Output, String>,
    ) {
        self.busy = None;
        let ok = matches!(&result, Ok(o) if o.success());
        if ok {
            self.error = None;
            let done = match &op {
                RemoteOp::Fetch => "Fetched".to_string(),
                RemoteOp::Pull(_) | RemoteOp::PullThenPush => {
                    let text = result
                        .as_ref()
                        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        .unwrap_or_default();
                    if text.contains("Already up to date") {
                        "Already up to date".to_string()
                    } else {
                        "Pulled".to_string()
                    }
                }
                RemoteOp::Push(Push::SetUpstream { remote, branch }) => {
                    format!("Pushed to {remote}/{branch}, now its upstream")
                }
                RemoteOp::Push(_) => "Pushed".to_string(),
            };
            out.push(Effect::Notify(done, Level::Info));
            if op == RemoteOp::PullThenPush {
                self.remote(out, &root, RemoteOp::Push(Push::Plain));
                return;
            }
            self.refresh(out, &root, After::Nothing);
            return;
        }
        let why = message(&result);
        let divergent = why.contains("divergent branches") || why.contains("reconcile");
        if matches!(op, RemoteOp::Pull(_) | RemoteOp::PullThenPush) && divergent {
            let token = self.ask(Question::PullHow {
                root: root.clone(),
                then_push: op == RemoteOp::PullThenPush,
            });
            out.push(Effect::Pick {
                token,
                title: "The branches diverged: how should Pull bring them together? (remembered)"
                    .into(),
                items: vec![
                    (
                        "Rebase".into(),
                        Some("This branch's commits on top of the upstream's".into()),
                    ),
                    ("Merge".into(), Some("A merge commit joins the two".into())),
                    (
                        "Fast-forward only".into(),
                        Some("Refuse when they diverge".into()),
                    ),
                ],
            });
            self.show(out);
            return;
        }
        self.fail(out, why);
        self.refresh(out, &root, After::Nothing);
    }

    fn answer(&mut self, out: &mut Vec<Effect>, q: Question, a: Answer) {
        match (q, a) {
            (
                Question::Plan {
                    root,
                    commands,
                    done,
                },
                Answer::Confirmed(true),
            ) => {
                self.run_plan(out, &root, commands, done);
            }
            (Question::Message { root }, Answer::Text(Some(text))) => {
                let Some(repo) = self.repos.get(&root).and_then(|s| s.repo.clone()) else {
                    return;
                };
                match actions::commit(
                    &format!("{text}\n"),
                    &CommitKind::New,
                    self.settings.signoff,
                    &repo,
                ) {
                    Ok(plan) => self.start_plan(out, &root, plan),
                    Err(e) => out.push(Effect::Notify(e, Level::Warning)),
                }
            }
            (Question::Commands { ids, doc }, Answer::Picked(p)) => {
                if let Some(id) = p.first().and_then(|i| ids.get(*i as usize)) {
                    let id = id.to_string();
                    self.command(out, &id, doc);
                }
            }
            (Question::PullHow { root, then_push }, Answer::Picked(p)) => {
                let mode = match p.first() {
                    Some(0) => PullMode::Rebase,
                    Some(1) => PullMode::Merge,
                    Some(2) => PullMode::FfOnly,
                    _ => return,
                };
                let value = match mode {
                    PullMode::Rebase => "rebase",
                    PullMode::Merge => "merge",
                    _ => "ff-only",
                };
                out.push(Effect::SetSetting {
                    key: "pull".into(),
                    value: format!("\"{value}\""),
                });
                self.settings.pull = mode;
                let op = if then_push {
                    RemoteOp::PullThenPush
                } else {
                    RemoteOp::Pull(mode)
                };
                self.remote(out, &root, op);
            }
            (
                Question::PushTo {
                    root,
                    branch,
                    remotes,
                },
                Answer::Picked(p),
            ) => {
                if let Some(remote) = p.first().and_then(|i| remotes.get(*i as usize)) {
                    let how = Push::SetUpstream {
                        remote: remote.clone(),
                        branch,
                    };
                    self.remote(out, &root, RemoteOp::Push(how));
                }
            }
            (Question::PushBehind { root }, Answer::Picked(p)) => match p.first() {
                Some(0) => self.remote(out, &root, RemoteOp::PullThenPush),
                Some(1) => {
                    let token = self.ask(Question::Force { root });
                    out.push(Effect::Confirm {
                        token,
                        message: "Push with force-with-lease? The upstream's commits this branch lacks are lost there.".into(),
                    });
                }
                _ => {}
            },
            (Question::Force { root }, Answer::Confirmed(true)) => {
                self.remote(out, &root, RemoteOp::Push(Push::ForceWithLease));
            }
            // Cancelled.
            _ => {}
        }
    }

    fn panel_event(&mut self, out: &mut Vec<Effect>, key: &str, event: &PanelEvent) {
        let Some(root) = self.current.clone() else {
            return;
        };
        match (key, event) {
            ("refresh", PanelEvent::Clicked) => self.refresh(out, &root, After::Nothing),
            ("commit", PanelEvent::Clicked) => self.refresh(out, &root, After::Commit),
            ("pull", PanelEvent::Clicked) => self.refresh(out, &root, After::Pull),
            ("push", PanelEvent::Clicked) => self.refresh(out, &root, After::Push),
            (_, PanelEvent::Expanded(open)) if key.starts_with("section:") => {
                if let Some(s) = Section::from_key(&key["section:".len()..]) {
                    if *open {
                        self.folded.remove(&s);
                    } else {
                        self.folded.insert(s);
                    }
                    self.show(out);
                }
            }
            (_, PanelEvent::Toggled(on)) if key.starts_with("stage:") => {
                let mut f = key["stage:".len()..].splitn(2, ':');
                let (Some(s), Some(path)) = (f.next(), f.next()) else {
                    return;
                };
                let act = if *on { Act::Stage } else { Act::Unstage };
                let _ = s;
                self.refresh(out, &root, After::Act(act, path.to_string()));
            }
            (_, PanelEvent::Clicked) if key.starts_with("open:") => {
                let Some((_, path)) = key["open:".len()..].split_once(':') else {
                    return;
                };
                let full = format!("{}/{}", root.trim_end_matches('/'), path);
                out.push(Effect::RunCommand {
                    id: "file.open".into(),
                    args: json_object(&[("path", &full)]),
                });
            }
            _ => {}
        }
    }

    /// The status bar item and the panel, as the current repository is.
    fn show(&mut self, out: &mut Vec<Effect>) {
        out.push(Effect::Status(self.status_text()));
        out.push(Effect::Panel(self.panel()));
    }

    /// The status bar's text: the branch, ahead and behind, the changed
    /// files.
    pub fn status_text(&self) -> Option<String> {
        let repo = self.current()?;
        let s = &repo.status;
        let g = self.settings.glyphs;
        let branch = s
            .branch
            .clone()
            .or_else(|| s.oid.as_ref().map(|o| o.chars().take(7).collect()))
            .unwrap_or_else(|| "?".into());
        let mut text = match g {
            Glyphs::Unicode => format!("⎇ {branch}"),
            Glyphs::Ascii => format!("git {branch}"),
        };
        let ab = g.ahead_behind(s.ahead, s.behind);
        if !ab.is_empty() {
            text.push(' ');
            text.push_str(&ab);
        }
        let changed = s.entries.iter().filter(|e| e.kind != Kind::Ignored).count();
        if changed > 0 {
            text.push_str(&match g {
                Glyphs::Unicode => format!(" •{changed}"),
                Glyphs::Ascii => format!(" *{changed}"),
            });
        }
        if let Some(b) = &self.busy {
            text.push_str(&format!(" {b}"));
        }
        Some(text)
    }

    /// The Git panel (DESIGN.md, 2.9).
    pub fn panel(&self) -> Node {
        let mut nodes = Vec::new();
        let Some(repo) = self.current() else {
            nodes.push(Node::label(
                "Open a file of a git repository: its status shows here.",
                TextStyle::Muted,
            ));
            return Node::Column(nodes);
        };
        let s = &repo.status;
        let mut head = s.branch.clone().unwrap_or_else(|| "(detached)".into());
        if let Some(u) = &s.upstream {
            head.push_str(&format!(" → {u}"));
            let ab = self.settings.glyphs.ahead_behind(s.ahead, s.behind);
            if !ab.is_empty() {
                head.push_str(&format!(" {ab}"));
            }
        }
        nodes.push(Node::label(head, TextStyle::Strong));
        if let Some(h) = &repo.head {
            nodes.push(Node::label(
                format!("{}  {}", h.short, h.subject),
                TextStyle::Muted,
            ));
        }
        let button = |key: &str, label: &str| Node::Button {
            key: key.into(),
            label: label.into(),
        };
        nodes.push(Node::Row(vec![
            button("refresh", "Refresh"),
            button("commit", "Commit"),
            button("pull", "Pull"),
            button("push", "Push"),
        ]));
        if let Some(b) = &self.busy {
            nodes.push(Node::Progress {
                value: None,
                label: Some(b.clone()),
            });
        }
        if let Some(e) = &self.error {
            nodes.push(Node::label(e.clone(), TextStyle::Error));
        }
        let mut any = false;
        for (section, title) in [
            (Section::Unmerged, "Unmerged"),
            (Section::Staged, "Staged"),
            (Section::Unstaged, "Unstaged"),
            (Section::Untracked, "Untracked"),
        ] {
            let entries = repo.entries(section);
            if entries.is_empty() {
                continue;
            }
            any = true;
            let open = !self.folded.contains(&section);
            let mut children = Vec::new();
            for e in &entries {
                let counts = repo
                    .counts
                    .get(&(section, e.path.clone()))
                    .map(|(a, r)| self.settings.glyphs.counts(*a, *r));
                let what = match (section, e.kind) {
                    (_, Kind::Unmerged(c)) => Some(c.words().to_string()),
                    (Section::Untracked, _) => None,
                    (Section::Staged, _) => Some(e.index.word().to_string()),
                    _ => Some(e.worktree.word().to_string()),
                };
                let detail = match (what, counts) {
                    (Some(w), Some(c)) => Some(format!("{w}  {c}")),
                    (Some(w), None) => Some(w),
                    (None, c) => c,
                };
                children.push(Node::Row(vec![
                    Node::Checkbox {
                        key: format!("stage:{}:{}", section.key(), e.path),
                        label: String::new(),
                        checked: section == Section::Staged,
                    },
                    Node::Item {
                        key: format!("open:{}:{}", section.key(), e.path),
                        label: e.path.clone(),
                        detail,
                        expanded: None,
                        children: Vec::new(),
                    },
                ]));
            }
            nodes.push(Node::Item {
                key: format!("section:{}", section.key()),
                label: format!("{title} ({})", entries.len()),
                detail: None,
                expanded: Some(open),
                children: if open { children } else { Vec::new() },
            });
        }
        if !any {
            nodes.push(Node::label(
                "Nothing to commit: the working tree is clean",
                TextStyle::Muted,
            ));
        }
        Node::Column(nodes)
    }
}

/// `after` with its file made a path in the repository.
fn with_path(after: After, prefix: &str, file: Option<&str>) -> After {
    let Some(file) = file else {
        return after;
    };
    let path = format!("{prefix}{file}");
    match after {
        After::Act(act, _) => After::Act(act, path),
        After::Blame { line, text, .. } => After::Blame { path, line, text },
        other => other,
    }
}

/// The message of a failed run.
fn message(result: &Result<Output, String>) -> String {
    match result {
        Ok(o) => o.message(),
        Err(e) => e.clone(),
    }
}

/// The keys of a command as the palette shows them: `SPC g S`.
fn keys_hint(keys: &[&str]) -> Option<String> {
    let k = keys.first()?;
    Some(
        k.split(' ')
            .map(|p| match p.strip_prefix("shift+") {
                Some(c) => c.to_uppercase(),
                None if p == "space" => "SPC".into(),
                None => p.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// A JSON object of strings.
fn json_object(fields: &[(&str, &str)]) -> String {
    let esc = |s: &str| {
        let mut o = String::new();
        for c in s.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\t' => o.push_str("\\t"),
                c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                c => o.push(c),
            }
        }
        o
    };
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("\"{}\":\"{}\"", esc(k), esc(v)))
        .collect();
    format!("{{{}}}", body.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_as_the_palette_shows_them() {
        assert_eq!(keys_hint(&["space g shift+s"]).as_deref(), Some("SPC g S"));
        assert_eq!(keys_hint(&["space g c c"]).as_deref(), Some("SPC g c c"));
        assert_eq!(keys_hint(&[]), None);
    }

    #[test]
    fn json_escapes() {
        assert_eq!(json_object(&[("path", "a\"b\\c")]), r#"{"path":"a\"b\\c"}"#);
    }

    #[test]
    fn settings_from_json() {
        let s = Settings::read(|k| match k {
            "confirm_discard" => Some("false".into()),
            "pull" => Some("\"rebase\"".into()),
            "glyphs" => Some("\"ascii\"".into()),
            "untracked" => Some("42".into()),
            _ => None,
        });
        assert!(!s.confirm_discard);
        assert_eq!(s.pull, PullMode::Rebase);
        assert_eq!(s.glyphs, Glyphs::Ascii);
        assert_eq!(s.untracked, Untracked::Normal);
    }

    #[test]
    fn every_command_is_named_after_the_plugin() {
        for c in COMMANDS {
            assert!(c.id.starts_with("git."), "{}", c.id);
            assert!(c.title.starts_with("Git: "), "{}", c.title);
        }
    }

    #[test]
    fn a_command_on_a_file_finds_its_repository_first() {
        let mut app = App::new(Settings::default());
        let out = app.handle(Input::Command {
            id: "git.stageFile".into(),
            doc: Some(Doc {
                path: Some("/r/src/lib.rs".into()),
                line: 1,
                text: None,
            }),
        });
        let [Effect::Run { cwd, command, .. }] = &out[..] else {
            panic!("{out:?}")
        };
        assert_eq!(cwd.as_deref(), Some("/r/src"));
        assert_eq!(
            command.args,
            ["rev-parse", "--show-toplevel", "--show-prefix"]
        );
        // Without a document, a file command says why it cannot act.
        let out = app.handle(Input::Command {
            id: "git.stageFile".into(),
            doc: None,
        });
        assert!(matches!(&out[..], [Effect::Notify(_, Level::Warning)]));
    }
}
