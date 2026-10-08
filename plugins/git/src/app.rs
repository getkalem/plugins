//! The plugin as Kalem runs it (DESIGN.md, section 8): the status bar
//! item, the Git panel, the leader's keys on the file being edited,
//! commit, fetch, pull and push, a line's blame (phase 0); the status as a
//! document of Kalem's, its sections, files and hunks folded with Tab and
//! the arrows, opened with Enter, acted on with magit's keys (phase 1,
//! `document.rs`).
//!
//! [`App`] is a state machine: it takes an [`Input`] (a command with the
//! document it runs in, how a git run ended, the user's answer, a click in
//! the panel, an event) and gives [`Effect`]s (run git, notify, set the
//! status bar item or the panel, ask). It knows nothing of Kalem's types,
//! so its tests drive it with a real git and scripted answers; the
//! component (`component.rs`) turns its effects into `kalem_plugin` calls
//! and its callbacks back into inputs.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod document;

use crate::actions::{self, Act};
use crate::content::Content;
use crate::git::blame;
use crate::git::cmd::{self, CommitKind, GitCommand, PullMode, Push, Untracked};
use crate::git::status::Kind;
use crate::model::{Folds, Repo, Section};
use crate::panel::{Node, TextStyle};
use crate::refresh::{self, Output, Part, Refresh};
use crate::target::Target;
use crate::views::Glyphs;

pub use document::Keep;

/// The panel's ID.
pub const PANEL: &str = "git.panel";

/// The status bar item's ID.
pub const STATUS: &str = "git.status";

/// The status document's ID (`documents`), one a repository.
pub const STATUS_DOC: &str = "git.status";

/// The status document's kind: the text type its keys and its own
/// commands are scoped to.
pub const STATUS_KIND: &str = "git-status";

/// The when-clause of the status document's keys: in it, in Vim's command
/// mode or without Vim, so that its letters do not type (it is read-only)
/// and come before Vim's.
pub const DOC_WHEN: &str = "textType == git-status && (vimCommand || !vimActive)";

/// Escape's: with a selection Escape clears it first (Vim leaves visual
/// mode).
pub const DOC_ESCAPE_WHEN: &str =
    "textType == git-status && !hasSelection && (vimCommand || !vimActive)";

/// A command of the plugin: its ID, title, keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    /// `git.NAME`.
    pub id: &'static str,
    /// For the palette and the menus.
    pub title: &'static str,
    /// Its keys everywhere, as `keymap.json` writes them: Doom's under the
    /// leader (`space g g`, bound in Vim's command mode) and others.
    pub keys: &'static [&'static str],
    /// Its keys in the status document (magit's and lazygit's), bound with
    /// [`DOC_WHEN`].
    pub doc_keys: &'static [&'static str],
    /// It acts in the status document only, and the palette offers it
    /// there only.
    pub in_status_only: bool,
}

impl CommandInfo {
    const fn new(id: &'static str, title: &'static str) -> CommandInfo {
        CommandInfo {
            id,
            title,
            keys: &[],
            doc_keys: &[],
            in_status_only: false,
        }
    }

    const fn keys(mut self, keys: &'static [&'static str]) -> CommandInfo {
        self.keys = keys;
        self
    }

    const fn doc(mut self, keys: &'static [&'static str]) -> CommandInfo {
        self.doc_keys = keys;
        self
    }

    const fn status_only(mut self) -> CommandInfo {
        self.in_status_only = true;
        self
    }
}

/// The command `id`.
pub fn command_info(id: &str) -> Option<&'static CommandInfo> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// The when-clause of a leader key: Vim's command mode, as Kalem's own
/// leader keys (`keymaps/vim.json`). Without it a key starting with Space
/// was active in insert mode too, and Space typed no space.
pub const LEADER_WHEN: &str = "vimCommand";

/// A command's keys split into its leader keys (`space g g`), bound with
/// [`LEADER_WHEN`], and the others (`ctrl+shift+g`), its default keys.
pub fn split_leader_keys<'a>(keys: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    keys.iter().partition(|k| {
        let first = k.split(' ').next().unwrap_or_default();
        first == "space" || first.eq_ignore_ascii_case("leader")
    })
}

/// The commands, with Doom Emacs's keys (DESIGN.md, 5.2) and, in the
/// status document, magit's and the arrows (5.1).
pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo::new("git.status", "Git: Status").keys(&["space g g", "ctrl+shift+g"]),
    CommandInfo::new("git.stageFile", "Git: Stage File").keys(&["space g shift+s"]),
    CommandInfo::new("git.unstageFile", "Git: Unstage File").keys(&["space g shift+u"]),
    CommandInfo::new("git.revertFile", "Git: Revert File").keys(&["space g shift+r"]),
    CommandInfo::new("git.deleteFile", "Git: Delete File").keys(&["space g shift+d"]),
    CommandInfo::new("git.commit", "Git: Commit")
        .keys(&["space g c c"])
        .doc(&["c c"]),
    CommandInfo::new("git.amend", "Git: Amend Last Commit")
        .keys(&["space g c a"])
        .doc(&["c a"]),
    CommandInfo::new("git.extend", "Git: Extend Last Commit")
        .keys(&["space g c e"])
        .doc(&["c e"]),
    CommandInfo::new("git.reword", "Git: Reword Last Commit")
        .keys(&["space g c w"])
        .doc(&["c w"]),
    CommandInfo::new("git.fetch", "Git: Fetch")
        .keys(&["space g shift+f"])
        .doc(&["f p", "f u", "f a"]),
    CommandInfo::new("git.pull", "Git: Pull").doc(&["p", "shift+f p", "shift+f u"]),
    CommandInfo::new("git.push", "Git: Push").doc(&["shift+p p", "shift+p u"]),
    CommandInfo::new("git.switchBranch", "Git: Switch Branch").doc(&["b b"]),
    CommandInfo::new("git.newBranch", "Git: New Branch").doc(&["b c"]),
    CommandInfo::new("git.stash", "Git: Stash Changes").doc(&["shift+z z"]),
    CommandInfo::new("git.blameLine", "Git: Blame This Line").keys(&["space g shift+b"]),
    CommandInfo::new("git.refresh", "Git: Refresh"),
    CommandInfo::new("git.fileMenu", "Git: This File").keys(&["space g ."]),
    CommandInfo::new("git.dispatch", "Git: Commands")
        .keys(&["space g /"])
        .doc(&["alt+enter", "?"]),
    // The status document's own.
    CommandInfo::new("git.visit", "Git: Open")
        .doc(&["enter"])
        .status_only(),
    CommandInfo::new("git.toggle", "Git: Fold or Unfold")
        .doc(&["tab"])
        .status_only(),
    CommandInfo::new("git.cycle", "Git: Fold Everything, Step by Step")
        .doc(&["shift+tab"])
        .status_only(),
    CommandInfo::new("git.unfold", "Git: Unfold")
        .doc(&["right"])
        .status_only(),
    CommandInfo::new("git.fold", "Git: Fold, or Go to the Parent")
        .doc(&["left"])
        .status_only(),
    CommandInfo::new("git.next", "Git: Next Item")
        .doc(&["alt+down"])
        .status_only(),
    CommandInfo::new("git.previous", "Git: Previous Item")
        .doc(&["alt+up"])
        .status_only(),
    CommandInfo::new("git.stage", "Git: Stage")
        .doc(&["s"])
        .status_only(),
    CommandInfo::new("git.unstage", "Git: Unstage")
        .doc(&["u"])
        .status_only(),
    CommandInfo::new("git.discard", "Git: Discard")
        .doc(&["d", "x"])
        .status_only(),
    CommandInfo::new("git.stageEverything", "Git: Stage or Unstage Everything")
        .doc(&["a"])
        .status_only(),
    CommandInfo::new("git.stageAll", "Git: Stage All Changes")
        .doc(&["shift+s"])
        .status_only(),
    CommandInfo::new("git.unstageAll", "Git: Unstage Everything")
        .doc(&["shift+u"])
        .status_only(),
    CommandInfo::new("git.tabFiles", "Git: Files Tab")
        .doc(&["1"])
        .status_only(),
    CommandInfo::new("git.tabBranches", "Git: Local Branches Tab")
        .doc(&["2"])
        .status_only(),
    CommandInfo::new("git.tabCommits", "Git: Commits Tab")
        .doc(&["3"])
        .status_only(),
    CommandInfo::new("git.tabStash", "Git: Stash Tab")
        .doc(&["4"])
        .status_only(),
    CommandInfo::new("git.nextTab", "Git: Next Tab")
        .doc(&["]"])
        .status_only(),
    CommandInfo::new("git.previousTab", "Git: Previous Tab")
        .doc(&["["])
        .status_only(),
    CommandInfo::new("git.close", "Git: Close the Status")
        .doc(&["escape", "q"])
        .status_only(),
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
    /// Marks beside the changed lines of files.
    pub gutter: bool,
    /// The marks compare with HEAD rather than the index.
    pub gutter_head: bool,
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
            gutter: true,
            gutter_head: false,
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
            gutter: boolean("gutter", d.gutter),
            gutter_head: text("gutter_base").is_some_and(|v| v == "head"),
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
    /// The repository whose status document it is, by its root.
    pub status: Option<String>,
    /// The selection's other end, in bytes; the cursor's when nothing is
    /// selected.
    pub anchor: usize,
    /// The cursor, in bytes.
    pub head: usize,
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
    /// The user closed the status document of this repository (its write
    /// was refused).
    DocumentClosed(String),
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
        /// The text offered, to edit.
        value: Option<String>,
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
    /// Writes the status document of repository `root`.
    Document {
        /// The repository.
        root: String,
        /// The document's title.
        title: String,
        /// Its text.
        text: String,
        /// Where the cursor goes, in bytes; else it stays on its line.
        cursor: Option<usize>,
        /// Opened, or shown, rather than written where it is open.
        show: bool,
        /// The styles of its text (lazygit's colors).
        styles: Vec<crate::content::Styled>,
    },
    /// Closes the status document of repository `root`.
    CloseDocument {
        /// The repository.
        root: String,
    },
    /// Sets the marks beside the lines of a file; none takes them away.
    Gutter {
        /// The file, absolute.
        path: String,
        /// Its lines (from 1) and what changed there.
        marks: Vec<(u32, crate::gutter::Mark)>,
    },
    /// Takes the marks away from every file.
    ClearGutters,
}

/// What to do once the repository is found and its status read.
#[derive(Debug, Clone, PartialEq)]
enum After {
    /// Nothing more: the status bar item and the panel follow.
    Nothing,
    /// An action on a file, by its path in the repository.
    Act(Act, String),
    /// Show the status document.
    Show,
    /// Ask for a message and commit, or amend.
    Commit(CommitKind),
    /// Offer the branches to switch to.
    SwitchBranch,
    /// Ask for a new branch's name.
    NewBranch,
    /// Stash the changes.
    Stash,
    /// Mark the changed lines of a file, by its path in the repository,
    /// from now on.
    Marks(String),
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
    /// A file's diff, for the status document.
    Diff {
        root: String,
        generation: u64,
        section: Section,
        path: String,
    },
    /// The last commit's message, to amend or reword it.
    LastMessage { root: String, kind: CommitKind },
    /// A file's changes, for the marks beside its lines.
    Marks { root: String, path: String },
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

impl After {
    /// Nothing the user asked for: a failure says nothing.
    fn quiet(&self) -> bool {
        matches!(self, After::Nothing | After::Marks(_))
    }
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
    /// The commit message's first line; `rest` is what follows it, kept
    /// (an amended commit's body).
    Message {
        root: String,
        kind: CommitKind,
        rest: String,
    },
    /// The branch to switch to: local ones by name, remote ones as
    /// `origin/name`.
    Branch {
        root: String,
        branches: Vec<(String, bool)>,
    },
    /// A new branch's name.
    NewBranch { root: String },
    /// What to do with a stash.
    Stash { root: String, index: u32 },
    /// Which discard to run, as `d` offers them; past the plans, Cancel.
    Discard {
        root: String,
        plans: Vec<actions::Plan>,
    },
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
    /// What the status document folds.
    folds: Folds,
    /// The status document as last written.
    content: Option<Content>,
    /// The status document is open.
    shown: bool,
    /// Where its cursor goes at its next writing.
    keep: Option<Keep>,
    /// Shift+Tab's step.
    cycle: u8,
    /// The status document's tab shown.
    tab: crate::views::status::Tab,
    /// The diffs being read for it.
    loading: BTreeSet<(Section, String)>,
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
    /// The files opened, whose changed lines are marked: by repository,
    /// their paths in it.
    marked: BTreeMap<String, BTreeSet<String>>,
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
                let (folder, name) = parent(&path);
                let after = if self.settings.gutter {
                    After::Marks(String::new())
                } else {
                    After::Nothing
                };
                self.locate(&mut out, Some(folder), Some(name), after, true);
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
                let marks = (self.settings.gutter, self.settings.gutter_head);
                self.settings = s;
                if !self.settings.gutter && marks.0 {
                    out.push(Effect::ClearGutters);
                } else if self.settings.gutter && marks != (s.gutter, s.gutter_head) {
                    let roots: Vec<String> = self.marked.keys().cloned().collect();
                    for root in roots {
                        self.mark_files(&mut out, &root);
                    }
                }
                self.show(&mut out);
                let shown: Vec<String> = self
                    .repos
                    .iter()
                    .filter(|(_, s)| s.shown)
                    .map(|(r, _)| r.clone())
                    .collect();
                for root in shown {
                    self.write_status(&mut out, &root, false);
                }
            }
            Input::DocumentClosed(root) => {
                if let Some(s) = self.repos.get_mut(&root) {
                    s.shown = false;
                    s.content = None;
                    s.keep = None;
                }
            }
        }
        out
    }

    fn command(&mut self, out: &mut Vec<Effect>, id: &str, doc: Option<Doc>) {
        // In the status document: what is under the cursor.
        if let Some(d) = &doc
            && let Some(root) = d.status.clone()
            && self.status_command(out, &root, id, d)
        {
            return;
        }
        if command_info(id).is_some_and(|c| c.in_status_only) {
            out.push(Effect::Notify(
                "This acts in the status document: open it with SPC g g".into(),
                Level::Warning,
            ));
            return;
        }
        let status_root = doc.as_ref().and_then(|d| d.status.clone());
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
            "git.status" => After::Show,
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
            "git.commit" => After::Commit(CommitKind::New),
            "git.amend" => After::Commit(CommitKind::Amend),
            "git.extend" => After::Commit(CommitKind::Extend),
            "git.reword" => After::Commit(CommitKind::Reword),
            "git.switchBranch" => After::SwitchBranch,
            "git.newBranch" => After::NewBranch,
            "git.stash" => After::Stash,
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
                    .filter(|c| !c.in_status_only && c.id != "git.dispatch")
                    .map(|c| c.id)
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
        let folder = folder.or(status_root).or_else(|| self.current.clone());
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
                    if !after.quiet() {
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
            Pending::Diff {
                root,
                generation,
                section,
                path,
            } => self.diff_done(out, &root, generation, section, &path, result),
            Pending::LastMessage { root, kind } => match result {
                Ok(o) if o.success() => {
                    let text = String::from_utf8_lossy(&o.stdout).into_owned();
                    let (first, rest) = text
                        .trim_end()
                        .split_once('\n')
                        .unwrap_or((text.trim_end(), ""));
                    let token = self.ask(Question::Message {
                        root,
                        kind: kind.clone(),
                        rest: rest.trim_start_matches('\n').to_string(),
                    });
                    out.push(Effect::Prompt {
                        token,
                        title: match kind {
                            CommitKind::Reword => "Reword the last commit: its first line".into(),
                            _ => "Amend the last commit with what is staged: its first line".into(),
                        },
                        value: Some(first.to_string()),
                    });
                }
                other => self.fail(out, message(&other)),
            },
            Pending::Marks { root, path } => {
                let marks = match &result {
                    Ok(o) if o.success() => crate::git::diff::parse(&o.stdout)
                        .ok()
                        .and_then(|d| d.files.into_iter().next())
                        .map(|f| crate::gutter::marks(&f))
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                out.push(Effect::Gutter {
                    path: format!("{}/{}", root.trim_end_matches('/'), path),
                    marks,
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
                    s.loading.clear();
                }
            }
            Err(e) => {
                if let Some(s) = self.repos.get_mut(root) {
                    s.repo = None;
                }
                if afters.iter().chain(&waiting).any(|a| !a.quiet()) {
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
        // The files opened in it: their changed lines marked anew.
        for a in &afters {
            if let After::Marks(path) = a
                && !path.is_empty()
            {
                self.marked
                    .entry(root.to_string())
                    .or_default()
                    .insert(path.clone());
            }
        }
        if self.settings.gutter {
            self.mark_files(out, root);
        }
        // The status document, open, is written again (once: Show writes
        // it too).
        let shown = self.repos.get(root).is_some_and(|s| s.shown);
        if shown && !afters.contains(&After::Show) {
            self.write_status(out, root, false);
        }
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
            After::Show => self.write_status(out, root, true),
            // Done by `refreshed`.
            After::Marks(_) => {}
            After::Commit(kind) => self.commit(out, root, &repo, kind),
            After::SwitchBranch => {
                let mut branches: Vec<(String, bool)> = repo
                    .local_branches()
                    .into_iter()
                    .filter(|b| !b.current)
                    .map(|b| (b.name.clone(), false))
                    .collect();
                // A remote's branch with no local branch of its name: a new
                // local branch tracking it.
                for b in repo.branches.iter().filter(|b| b.remote) {
                    let short = b.name.split_once('/').map_or(b.name.as_str(), |(_, n)| n);
                    if !repo.branches.iter().any(|l| !l.remote && l.name == short) {
                        branches.push((b.name.clone(), true));
                    }
                }
                if branches.is_empty() {
                    out.push(Effect::Notify(
                        "No other branch: make one with b c (Git: New Branch)".into(),
                        Level::Info,
                    ));
                    return;
                }
                let items = branches
                    .iter()
                    .map(|(name, remote)| {
                        let detail = remote.then(|| "A new local branch tracking it".to_string());
                        (name.clone(), detail)
                    })
                    .collect();
                let token = self.ask(Question::Branch {
                    root: root.to_string(),
                    branches,
                });
                out.push(Effect::Pick {
                    token,
                    title: "Switch to the branch".into(),
                    items,
                });
            }
            After::NewBranch => {
                let token = self.ask(Question::NewBranch {
                    root: root.to_string(),
                });
                let from = repo.status.branch.as_deref().unwrap_or("HEAD");
                out.push(Effect::Prompt {
                    token,
                    title: format!("A new branch from {from}, switched to: its name"),
                    value: None,
                });
            }
            After::Stash => {
                let changed = repo
                    .status
                    .entries
                    .iter()
                    .any(|e| e.staged() || e.unstaged());
                if !changed {
                    out.push(Effect::Notify(
                        "No change of a tracked file to stash".into(),
                        Level::Info,
                    ));
                    return;
                }
                self.run_plan(
                    out,
                    root,
                    vec![cmd::stash_push()],
                    "Stashed the changes".into(),
                );
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

    /// Reads the changes of the files of `root` opened, for the marks
    /// beside their lines.
    fn mark_files(&mut self, out: &mut Vec<Effect>, root: &str) {
        let paths: Vec<String> = self
            .marked
            .get(root)
            .map(|p| p.iter().cloned().collect())
            .unwrap_or_default();
        for path in paths {
            let command = crate::gutter::command(&path, self.settings.gutter_head);
            self.run(
                out,
                Some(root.to_string()),
                command,
                Pending::Marks {
                    root: root.to_string(),
                    path,
                },
            );
        }
    }

    /// A commit of kind `kind`: its message asked for (an amended
    /// commit's first line offered), or none for an extension.
    fn commit(&mut self, out: &mut Vec<Effect>, root: &str, repo: &Repo, kind: CommitKind) {
        let staged = repo.entries(Section::Staged).len();
        match kind {
            CommitKind::New => {
                if staged == 0 {
                    out.push(Effect::Notify(
                        "Nothing is staged: stage changes first (s in the status, SPC g S in a file)"
                            .into(),
                        Level::Warning,
                    ));
                    return;
                }
                let token = self.ask(Question::Message {
                    root: root.to_string(),
                    kind,
                    rest: String::new(),
                });
                out.push(Effect::Prompt {
                    token,
                    title: format!(
                        "Commit {} staged file{}: the message",
                        staged,
                        if staged == 1 { "" } else { "s" }
                    ),
                    value: None,
                });
            }
            CommitKind::Extend if staged == 0 => out.push(Effect::Notify(
                "Nothing is staged to add to the last commit".into(),
                Level::Warning,
            )),
            CommitKind::Amend | CommitKind::Reword if repo.unborn() => out.push(Effect::Notify(
                "There is no commit to amend yet".into(),
                Level::Warning,
            )),
            CommitKind::Amend | CommitKind::Reword => self.run(
                out,
                Some(root.to_string()),
                cmd::last_message(),
                Pending::LastMessage {
                    root: root.to_string(),
                    kind,
                },
            ),
            kind => match actions::commit("", &kind, self.settings.signoff, repo) {
                Ok(plan) => self.start_plan(out, root, plan),
                Err(e) => out.push(Effect::Notify(e, Level::Warning)),
            },
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
            (Question::Message { root, kind, rest }, Answer::Text(Some(text))) => {
                let Some(repo) = self.repos.get(&root).and_then(|s| s.repo.clone()) else {
                    return;
                };
                let message = if rest.trim().is_empty() {
                    format!("{text}\n")
                } else {
                    format!("{text}\n\n{}\n", rest.trim_end())
                };
                match actions::commit(&message, &kind, self.settings.signoff, &repo) {
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
            (Question::Branch { root, branches }, Answer::Picked(p)) => {
                let Some((name, remote)) = p.first().and_then(|i| branches.get(*i as usize)) else {
                    return;
                };
                let (command, done) = if *remote {
                    let local = name.split_once('/').map_or(name.as_str(), |(_, n)| n);
                    (
                        cmd::switch_create(local, Some(name)),
                        format!("Switched to {local}, a new branch tracking {name}"),
                    )
                } else {
                    (cmd::switch(name), format!("Switched to {name}"))
                };
                self.run_plan(out, &root, vec![command], done);
            }
            (Question::NewBranch { root }, Answer::Text(Some(name))) => {
                let name = name.trim();
                if name.is_empty() {
                    return;
                }
                self.run_plan(
                    out,
                    &root,
                    vec![cmd::switch_create(name, None)],
                    format!("Made the branch {name}, and switched to it"),
                );
            }
            (Question::Stash { root, index }, Answer::Picked(p)) => {
                let name = format!("stash@{{{index}}}");
                let plan = match p.first() {
                    Some(0) => actions::Plan {
                        confirm: None,
                        commands: vec![cmd::stash(cmd::StashOp::Apply, index)],
                        done: format!("Applied {name}"),
                    },
                    Some(1) => actions::Plan {
                        confirm: None,
                        commands: vec![cmd::stash(cmd::StashOp::Pop, index)],
                        done: format!("Applied and dropped {name}"),
                    },
                    Some(2) => actions::Plan {
                        confirm: Some(format!(
                            "Drop {name}? Its changes are in no commit, and are lost."
                        )),
                        commands: vec![cmd::stash(cmd::StashOp::Drop, index)],
                        done: format!("Dropped {name}"),
                    },
                    _ => return,
                };
                self.start_plan(out, &root, plan);
            }
            (Question::Discard { root, mut plans }, Answer::Picked(p)) => {
                match p.first().map(|i| *i as usize) {
                    Some(i) if i < plans.len() => {
                        let plan = plans.swap_remove(i);
                        self.run_plan(out, &root, plan.commands, plan.done);
                    }
                    _ => {
                        if let Some(s) = self.repos.get_mut(&root) {
                            s.keep = None;
                        }
                    }
                }
            }
            (Question::Plan { root, .. } | Question::Discard { root, .. }, _) => {
                // Not done: the cursor stays where it is.
                if let Some(s) = self.repos.get_mut(&root) {
                    s.keep = None;
                }
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
            ("commit", PanelEvent::Clicked) => {
                self.refresh(out, &root, After::Commit(CommitKind::New))
            }
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
                "Open a file of a git repository",
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
        // Two rows of two: one row of four ran past a terminal's side
        // panel (some 36 columns), Push cut off.
        nodes.push(Node::Row(vec![
            button("refresh", "Refresh"),
            button("commit", "Commit"),
        ]));
        nodes.push(Node::Row(vec![
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
            nodes.push(Node::label("Clean: nothing to commit", TextStyle::Muted));
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
        After::Marks(_) => After::Marks(path),
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
    fn leader_keys_are_bound_apart() {
        let (leader, other) = split_leader_keys(&["space g g", "ctrl+shift+g", "leader g s"]);
        assert_eq!(leader, ["space g g", "leader g s"]);
        assert_eq!(other, ["ctrl+shift+g"]);
        // Every command's Space keys are leader keys; none other starts
        // with a plain key that types text.
        for c in COMMANDS {
            let (_, other) = split_leader_keys(c.keys);
            for k in other {
                let first = k.split(' ').next().unwrap_or_default();
                assert!(
                    first.contains('+'),
                    "{} binds `{k}`, a key that types",
                    c.id
                );
            }
        }
    }

    #[test]
    fn every_command_is_named_after_the_plugin() {
        for c in COMMANDS {
            assert!(c.id.starts_with("git."), "{}", c.id);
            assert!(c.title.starts_with("Git: "), "{}", c.title);
        }
    }

    /// The columns a node takes in the terminal: a button as `[ Label ]`,
    /// a row's widgets two columns apart.
    fn columns(n: &Node) -> usize {
        match n {
            Node::Label { text, .. } => text.chars().count(),
            Node::Button { label, .. } => label.chars().count() + 4,
            Node::Row(c) => c.iter().map(columns).sum::<usize>() + 2 * c.len().saturating_sub(1),
            _ => 0,
        }
    }

    #[test]
    fn the_panel_fits_a_terminals_side_panel() {
        // A terminal's side panel leaves some 34 columns for its widgets.
        const WIDE: usize = 32;
        let mut app = App::new(Settings::default());
        let Node::Column(empty) = app.panel() else {
            panic!("a column")
        };
        assert!(empty.iter().all(|n| columns(n) <= WIDE), "{empty:?}");
        let status = crate::git::status::parse(b"# branch.oid abc\0# branch.head main\0").unwrap();
        app.repos.insert(
            "/r".into(),
            RepoState {
                repo: Some(Repo {
                    root: "/r".into(),
                    status,
                    ..Repo::default()
                }),
                ..RepoState::default()
            },
        );
        app.current = Some("/r".into());
        let Node::Column(nodes) = app.panel() else {
            panic!("a column")
        };
        let keys: Vec<String> = nodes.iter().flat_map(Node::keys).collect();
        assert_eq!(keys, ["refresh", "commit", "pull", "push"]);
        for n in &nodes {
            assert!(columns(n) <= WIDE, "{n:?} takes {} columns", columns(n));
        }
        assert!(
            app.panel()
                .texts()
                .contains(&"Clean: nothing to commit".to_string())
        );
    }

    #[test]
    fn a_command_on_a_file_finds_its_repository_first() {
        let mut app = App::new(Settings::default());
        let out = app.handle(Input::Command {
            id: "git.stageFile".into(),
            doc: Some(Doc {
                path: Some("/r/src/lib.rs".into()),
                line: 1,
                ..Doc::default()
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
