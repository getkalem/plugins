//! The plugin's state machine (phase 0) driven as Kalem drives the
//! component: its git runs run for real in repositories the tests make,
//! its questions are answered from a script, and what it shows is
//! recorded. Git is isolated from the user's configuration.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kalem_plugin_git::app::{
    Answer, App, COMMANDS, COMMIT_KIND, Doc, Effect, Input, Level, MAX_RUNS, PanelEvent,
    STATUS_KIND, Settings,
};
use kalem_plugin_git::git::cmd::GitCommand;
use kalem_plugin_git::gutter::Mark;
use kalem_plugin_git::native::NativeRunner;
use kalem_plugin_git::panel::Node;
use kalem_plugin_git::refresh::Runner;

fn isolated(git: &mut NativeRunner) {
    git.env = [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_AUTHOR_NAME", "Ayşe Test"),
        ("GIT_AUTHOR_EMAIL", "ayse@example.com"),
        ("GIT_COMMITTER_NAME", "Ayşe Test"),
        ("GIT_COMMITTER_EMAIL", "ayse@example.com"),
        ("GIT_AUTHOR_DATE", "2026-10-07T12:00:00+03:00"),
        ("GIT_COMMITTER_DATE", "2026-10-07T12:00:00+03:00"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
}

/// The plugin, a repository, and what the plugin did.
struct Kalem {
    app: App,
    git: NativeRunner,
    dir: PathBuf,
    answers: VecDeque<Answer>,
    asked: Vec<String>,
    notes: Vec<(String, Level)>,
    status: Option<String>,
    panel: Option<Node>,
    shown: bool,
    commands: Vec<(String, String)>,
    set: Vec<(String, String)>,
    /// The documents: their text and cursor, by repository (the status)
    /// or by `repository#hash` (a commit's view).
    docs: BTreeMap<String, (String, usize)>,
    /// The repositories whose status was shown, in order.
    showed: Vec<String>,
    /// The texts the prompts offered.
    offered: Vec<Option<String>>,
    /// The marks beside the lines, by file.
    gutters: BTreeMap<String, Vec<(u32, Mark)>>,
    /// A path whose git runs are held back, as a slow answer is.
    hold: Option<String>,
    /// The runs held back: their number, folder and command.
    held: Vec<(u64, Option<String>, GitCommand)>,
    /// The plugin's programs Kalem runs at a time, refusing the next.
    kalem_runs: usize,
    /// The runs started and not ended, and the most there were.
    runs: usize,
    most_runs: usize,
    /// An argument of the runs whose output is cut short, as Kalem cuts
    /// a program's past 16 MB.
    cut: Option<String>,
}

impl Drop for Kalem {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.dir.parent().unwrap());
    }
}

impl Kalem {
    fn new(name: &str) -> Kalem {
        static N: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "kalem-git-app-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("repo")).unwrap();
        let dir = base.join("repo").canonicalize().unwrap();
        let mut git = NativeRunner::new(&dir);
        isolated(&mut git);
        let mut k = Kalem {
            app: App::new(Settings::default()),
            git,
            dir,
            answers: VecDeque::new(),
            asked: Vec::new(),
            notes: Vec::new(),
            status: None,
            panel: None,
            shown: false,
            commands: Vec::new(),
            set: Vec::new(),
            docs: BTreeMap::new(),
            showed: Vec::new(),
            offered: Vec::new(),
            gutters: BTreeMap::new(),
            hold: None,
            held: Vec::new(),
            kalem_runs: 16,
            runs: 0,
            most_runs: 0,
            cut: None,
        };
        k.git_ok(&["init", "-q", "-b", "main"]);
        k
    }

    fn git_ok(&mut self, args: &[&str]) -> String {
        self.git.cwd = self.dir.clone();
        let out = self
            .git
            .run(&GitCommand::new(args.iter().copied()))
            .unwrap();
        assert!(out.success(), "git {args:?}: {}", out.message());
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn write(&self, path: &str, text: &str) {
        let p = self.dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn path(&self, file: &str) -> String {
        self.dir.join(file).display().to_string()
    }

    fn commit_all(&mut self, message: &str) {
        self.git_ok(&["add", "-A"]);
        self.git_ok(&["commit", "-q", "-m", message]);
    }

    /// Hands `input` to the plugin, and everything that follows from it.
    fn feed(&mut self, input: Input) {
        let mut queue = VecDeque::from([input]);
        while let Some(i) = queue.pop_front() {
            if matches!(i, Input::Done { .. }) {
                self.runs -= 1;
            }
            for e in self.app.handle(i) {
                match e {
                    Effect::Run {
                        token,
                        cwd,
                        command,
                    } => {
                        if self.runs >= self.kalem_runs {
                            let error = format!(
                                "{} programs of the plugin are running already",
                                self.kalem_runs
                            );
                            queue.push_back(Input::Refused { token, error });
                            continue;
                        }
                        self.runs += 1;
                        self.most_runs = self.most_runs.max(self.runs);
                        if self.hold.as_ref().is_some_and(|h| command.args.contains(h)) {
                            self.held.push((token, cwd, command));
                            continue;
                        }
                        self.git.cwd = cwd.map_or_else(|| self.dir.clone(), PathBuf::from);
                        let mut result = self.git.run(&command);
                        if let Ok(o) = &mut result
                            && self.cut.as_ref().is_some_and(|c| command.args.contains(c))
                        {
                            o.stdout.truncate(o.stdout.len() / 2);
                            o.truncated = true;
                        }
                        queue.push_back(Input::Done { token, result });
                    }
                    Effect::Confirm { token, message } => {
                        self.asked.push(message);
                        let answer = self.answers.pop_front().expect("an answer to a confirm");
                        queue.push_back(Input::Answer { token, answer });
                    }
                    Effect::Prompt {
                        token,
                        title,
                        value,
                    } => {
                        self.asked.push(title);
                        self.offered.push(value);
                        let answer = self.answers.pop_front().expect("an answer to a prompt");
                        queue.push_back(Input::Answer { token, answer });
                    }
                    Effect::Pick {
                        token,
                        title,
                        items,
                    } => {
                        self.asked.push(format!(
                            "{title}: {}",
                            items
                                .iter()
                                .map(|(l, _)| l.as_str())
                                .collect::<Vec<_>>()
                                .join(" | ")
                        ));
                        let answer = self.answers.pop_front().expect("an answer to a pick");
                        queue.push_back(Input::Answer { token, answer });
                    }
                    Effect::Notify(m, l) => self.notes.push((m, l)),
                    Effect::Status(s) => self.status = s,
                    Effect::Panel(p) => self.panel = Some(p),
                    Effect::RemovePanel => self.panel = None,
                    Effect::ShowPanel => self.shown = true,
                    Effect::RunCommand { id, args } => self.commands.push((id, args)),
                    Effect::SetSetting { key, value } => self.set.push((key, value)),
                    Effect::Document {
                        root,
                        commit,
                        text,
                        cursor,
                        show,
                        ..
                    } => {
                        let root = match commit {
                            Some(hash) => format!("{root}#{hash}"),
                            None => root,
                        };
                        if show {
                            self.showed.push(root.clone());
                        } else if !self.docs.contains_key(&root) {
                            continue;
                        }
                        // Without a cursor, the editor keeps its line.
                        let at = match (cursor, self.docs.get(&root)) {
                            (Some(c), _) => c,
                            (None, Some((old, c))) => {
                                let line = old[..*c].matches('\n').count();
                                text.split_inclusive('\n').take(line).map(str::len).sum()
                            }
                            (None, None) => 0,
                        };
                        let at = at.min(text.len());
                        self.docs.insert(root, (text, at));
                    }
                    Effect::CloseDocument { root, commit } => {
                        let key = match commit {
                            Some(hash) => format!("{root}#{hash}"),
                            None => root,
                        };
                        self.docs.remove(&key);
                    }
                    Effect::Gutter { path, marks } => {
                        if marks.is_empty() {
                            self.gutters.remove(&path);
                        } else {
                            self.gutters.insert(path, marks);
                        }
                    }
                    Effect::ClearGutters => self.gutters.clear(),
                }
            }
        }
        assert!(self.answers.is_empty(), "answers left: {:?}", self.answers);
    }

    /// The message asked for as one line (the setting `commit_message =
    /// "prompt"`), as the tests of that way write it.
    fn prompt_messages(&mut self) {
        self.feed(Input::Settings(Settings {
            message_document: false,
            ..Settings::default()
        }));
    }

    /// Runs the git runs held back, and hands their ends to the plugin.
    fn release(&mut self) {
        for (token, cwd, command) in std::mem::take(&mut self.held) {
            self.git.cwd = cwd.map_or_else(|| self.dir.clone(), PathBuf::from);
            let result = self.git.run(&command);
            self.feed(Input::Done { token, result });
        }
    }

    fn command(&mut self, id: &str, file: Option<&str>) {
        let doc = file.map(|f| Doc {
            path: Some(self.path(f)),
            line: 1,
            ..Doc::default()
        });
        self.feed(Input::Command { id: id.into(), doc });
    }

    fn root(&self) -> String {
        self.dir.display().to_string()
    }

    /// The status document's text.
    fn doc_text(&self) -> String {
        self.docs
            .get(&self.root())
            .map(|d| d.0.clone())
            .unwrap_or_default()
    }

    /// The line of the status document the cursor is on.
    fn cursor_line(&self) -> String {
        let (text, at) = &self.docs[&self.root()];
        let start = text[..*at].rfind('\n').map_or(0, |i| i + 1);
        text[start..].lines().next().unwrap_or_default().to_string()
    }

    /// Command `id` run in the status document, from `anchor` to `head`.
    fn in_doc(&mut self, id: &str, anchor: usize, head: usize) {
        let doc = Doc {
            status: Some(self.root()),
            anchor,
            head,
            ..Doc::default()
        };
        if let Some(d) = self.docs.get_mut(&self.root()) {
            d.1 = head;
        }
        self.feed(Input::Command {
            id: id.into(),
            doc: Some(doc),
        });
    }

    /// Command `id` run with the cursor at the start of the first line of
    /// the status document holding `line`.
    fn on(&mut self, id: &str, line: &str) {
        let text = self.doc_text();
        let at = text
            .find(line)
            .unwrap_or_else(|| panic!("{line:?} is not in\n{text}"));
        let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        self.in_doc(id, start, start);
    }

    /// Command `id` run where the cursor is.
    fn here(&mut self, id: &str) {
        let at = self.docs[&self.root()].1;
        self.in_doc(id, at, at);
    }

    /// The key of the commit view open, if one is.
    fn commit_key(&self) -> Option<String> {
        self.docs.keys().find(|k| k.contains('#')).cloned()
    }

    /// The commit view's text.
    fn commit_text(&self) -> String {
        self.commit_key()
            .and_then(|k| self.docs.get(&k))
            .map(|d| d.0.clone())
            .unwrap_or_default()
    }

    /// Command `id` run in the commit view, with the cursor at the start
    /// of the first line holding `line`.
    fn on_commit(&mut self, id: &str, line: &str) {
        let key = self.commit_key().expect("a commit view open");
        let (root, hash) = key.split_once('#').unwrap();
        let text = self.docs[&key].0.clone();
        let at = text
            .find(line)
            .unwrap_or_else(|| panic!("{line:?} is not in\n{text}"));
        let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let doc = Doc {
            commit: Some((root.to_string(), hash.to_string())),
            anchor: start,
            head: start,
            ..Doc::default()
        };
        if let Some(d) = self.docs.get_mut(&key) {
            d.1 = start;
        }
        self.feed(Input::Command {
            id: id.into(),
            doc: Some(doc),
        });
    }

    /// Command `id` run in the view written under `key` (a commit's or a
    /// log's), with the cursor at the start of the first line holding
    /// `line`.
    fn in_view(&mut self, key: &str, id: &str, line: &str) {
        let (root, view) = key.split_once('#').unwrap();
        let text = self.docs[key].0.clone();
        let at = text
            .find(line)
            .unwrap_or_else(|| panic!("{line:?} is not in\n{text}"));
        let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let doc = Doc {
            commit: Some((root.to_string(), view.to_string())),
            anchor: start,
            head: start,
            ..Doc::default()
        };
        self.feed(Input::Command {
            id: id.into(),
            doc: Some(doc),
        });
    }

    fn texts(&self) -> Vec<String> {
        self.panel.as_ref().map(Node::texts).unwrap_or_default()
    }

    fn last_note(&self) -> &str {
        self.notes.last().map_or("", |n| n.0.as_str())
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join(path)).unwrap()
    }

    fn index(&mut self, path: &str) -> String {
        self.git_ok(&["show", &format!(":{path}")])
    }
}

#[test]
fn a_document_opened_shows_its_repository() {
    let mut k = Kalem::new("open");
    k.write("a.txt", "one\ntwo\n");
    k.commit_all("First");
    k.write("a.txt", "one\nTWO\n");
    k.write("new.txt", "new\n");
    let path = k.path("a.txt");
    k.feed(Input::Opened(path));
    assert_eq!(k.status.as_deref(), Some("⎇ main •2"));
    let texts = k.texts();
    assert_eq!(texts[0], "main");
    assert!(texts.iter().any(|t| t.ends_with("  First")), "{texts:?}");
    for t in [
        "(Refresh)",
        "(Commit)",
        "Unstaged (1)",
        "[ ] ",
        "a.txt — modified  +1 −1",
        "Untracked (1)",
        "new.txt",
    ] {
        assert!(texts.iter().any(|x| x == t), "{t} in {texts:?}");
    }
    assert!(k.notes.is_empty(), "{:?}", k.notes);
    // SPC g g shows the status document.
    k.command("git.status", Some("a.txt"));
    assert_eq!(k.showed, [k.root()]);
    let text = k.doc_text();
    assert!(
        text.starts_with("Tab diff · Enter open · s stage"),
        "{text}"
    );
    assert!(
        text.contains("\n  M a.txt   +1 −1\n ?? new.txt\n"),
        "{text}"
    );
}

/// A document outside every repository leaves none current: the status
/// bar named the one before.
#[test]
fn a_document_outside_every_repository_leaves_none_current() {
    let mut k = Kalem::new("outside");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    let path = k.path("a.txt");
    k.feed(Input::Opened(path.clone()));
    assert_eq!(k.status.as_deref(), Some("⎇ main"));
    let elsewhere = std::env::temp_dir().join(format!("kalem-git-loose-{}", std::process::id()));
    std::fs::create_dir_all(&elsewhere).unwrap();
    let loose = elsewhere.join("loose.txt");
    std::fs::write(&loose, "x\n").unwrap();
    k.feed(Input::Opened(loose.to_string_lossy().into_owned()));
    assert_eq!(k.status, None);
    assert!(k.notes.is_empty(), "{:?}", k.notes);
    // Back in the repository, its branch again.
    k.feed(Input::Opened(path));
    assert_eq!(k.status.as_deref(), Some("⎇ main"));
    let _ = std::fs::remove_dir_all(&elsewhere);
}

#[test]
fn the_leader_stages_unstages_reverts_and_deletes_the_file() {
    let mut k = Kalem::new("leader");
    k.write("dir/a.txt", "one\n");
    k.commit_all("First");
    k.write("dir/a.txt", "two\n");
    k.command("git.stageFile", Some("dir/a.txt"));
    assert_eq!(k.last_note(), "Staged dir/a.txt");
    assert_eq!(k.index("dir/a.txt"), "two\n");
    assert!(k.texts().iter().any(|t| t == "Staged (1)"));
    assert!(k.texts().iter().any(|t| t == "[x] "));
    // Again: nothing more to stage.
    k.command("git.stageFile", Some("dir/a.txt"));
    assert_eq!(
        k.notes.last().unwrap(),
        &("dir/a.txt has nothing to stage".to_string(), Level::Warning)
    );
    k.command("git.unstageFile", Some("dir/a.txt"));
    assert_eq!(k.index("dir/a.txt"), "one\n");
    // Revert asks first; no keeps the file.
    k.answers.push_back(Answer::Confirmed(false));
    k.command("git.revertFile", Some("dir/a.txt"));
    assert!(k.asked.last().unwrap().starts_with("Revert dir/a.txt?"));
    assert_eq!(
        std::fs::read_to_string(k.dir.join("dir/a.txt")).unwrap(),
        "two\n"
    );
    k.answers.push_back(Answer::Confirmed(true));
    k.command("git.revertFile", Some("dir/a.txt"));
    assert_eq!(
        std::fs::read_to_string(k.dir.join("dir/a.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(k.status.as_deref(), Some("⎇ main"));
    // Delete an untracked file, after asking.
    k.write("dir/junk.txt", "j\n");
    k.answers.push_back(Answer::Confirmed(true));
    k.command("git.deleteFile", Some("dir/junk.txt"));
    assert!(k.asked.last().unwrap().contains("cannot bring it back"));
    assert!(!k.dir.join("dir/junk.txt").exists());
}

#[test]
fn the_panel_stages_by_its_boxes_and_opens_files() {
    let mut k = Kalem::new("panel");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    k.write("a.txt", "two\n");
    k.write("b.txt", "b\n");
    let path = k.path("a.txt");
    k.feed(Input::Opened(path));
    let keys = k.panel.as_ref().unwrap().keys();
    assert!(
        keys.contains(&"stage:unstaged:a.txt".to_string()),
        "{keys:?}"
    );
    k.feed(Input::Panel {
        key: "stage:untracked:b.txt".into(),
        event: PanelEvent::Toggled(true),
    });
    assert_eq!(k.index("b.txt"), "b\n");
    k.feed(Input::Panel {
        key: "stage:staged:b.txt".into(),
        event: PanelEvent::Toggled(false),
    });
    assert!(k.git_ok(&["ls-files", "b.txt"]).is_empty());
    k.feed(Input::Panel {
        key: "open:unstaged:a.txt".into(),
        event: PanelEvent::Clicked,
    });
    let (id, args) = k.commands.last().unwrap();
    assert_eq!(id, "file.open");
    assert_eq!(args, &format!("{{\"path\":\"{}\"}}", k.path("a.txt")));
    // A section folds.
    k.feed(Input::Panel {
        key: "section:untracked".into(),
        event: PanelEvent::Expanded(false),
    });
    assert!(!k.texts().iter().any(|t| t == "b.txt"), "{:?}", k.texts());
}

#[test]
fn commit_asks_for_its_message() {
    let mut k = Kalem::new("commit");
    k.prompt_messages();
    k.write("a.txt", "one\n");
    // Nothing staged: said, not asked.
    k.command("git.commit", Some("a.txt"));
    assert_eq!(k.notes.last().unwrap().1, Level::Warning);
    k.command("git.stageFile", Some("a.txt"));
    k.answers.push_back(Answer::Text(Some("Add a".into())));
    k.command("git.commit", Some("a.txt"));
    assert_eq!(k.asked.last().unwrap(), "Commit 1 staged file: the message");
    assert_eq!(k.last_note(), "Committed");
    assert_eq!(k.git_ok(&["log", "-1", "--format=%s"]).trim(), "Add a");
    // Cancelled: nothing happens.
    k.write("a.txt", "two\n");
    k.command("git.stageFile", Some("a.txt"));
    k.answers.push_back(Answer::Text(None));
    k.command("git.commit", Some("a.txt"));
    assert_eq!(k.git_ok(&["rev-list", "--count", "HEAD"]).trim(), "1");
}

/// A line starting with `#` is the message's own: "#123 Fix the crash"
/// was refused as empty, and rewording dropped the body's "# Why" and
/// "#42 closes it".
#[test]
fn a_message_keeps_its_lines_starting_with_a_hash() {
    let mut k = Kalem::new("hash");
    k.prompt_messages();
    k.write("a.txt", "one\n");
    k.command("git.stageFile", Some("a.txt"));
    k.answers
        .push_back(Answer::Text(Some("#123 Fix the crash".into())));
    k.command("git.commit", Some("a.txt"));
    assert_eq!(k.last_note(), "Committed");
    assert_eq!(
        k.git_ok(&["log", "-1", "--format=%s"]).trim(),
        "#123 Fix the crash"
    );
    k.write("a.txt", "two\n");
    k.git_ok(&[
        "commit",
        "-q",
        "-a",
        "-m",
        "Second\n\n# Why\nBecause.\n\n#42 closes it",
    ]);
    k.answers
        .push_back(Answer::Text(Some("Second, reworded".into())));
    k.command("git.reword", Some("a.txt"));
    assert_eq!(k.offered.last().unwrap().as_deref(), Some("Second"));
    assert_eq!(k.last_note(), "Reworded the last commit");
    assert_eq!(
        k.git_ok(&["log", "-1", "--format=%B"]),
        "Second, reworded\n\n# Why\nBecause.\n\n#42 closes it\n\n"
    );
}

/// The message as a document (DESIGN.md, G7): `c c` opens the
/// repository's COMMIT_EDITMSG filled as git fills it; saving commits with
/// what it says, its `#` lines left out, and closes it; closing it unsaved
/// cancels; a commit git refuses leaves it open to fix; Amend starts from
/// the last message.
#[test]
fn the_message_is_written_as_a_document() {
    let mut k = Kalem::new("message");
    k.write("a.txt", "one\n");
    k.command("git.stageFile", Some("a.txt"));
    k.command("git.commit", Some("a.txt"));
    let (path, text) = k.app.message_fill().expect("the message's first text");
    assert!(path.ends_with("/.git/COMMIT_EDITMSG"), "{path}");
    assert!(
        text.starts_with("\n# Please enter the commit message"),
        "{text}"
    );
    assert!(text.contains("#\tnew file:   a.txt\n"), "{text}");
    let ids: Vec<&str> = k.commands.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids[ids.len() - 2..], ["file.open", "git.writeMessage"]);
    // Written and saved: committed, the comments left out, the document
    // closed.
    std::fs::write(&path, format!("Add a\n\nBecause.\n{text}")).unwrap();
    k.commands.clear();
    k.feed(Input::Saved(path.clone()));
    assert_eq!(k.last_note(), "Committed");
    assert_eq!(
        k.git_ok(&["log", "-1", "--format=%B"]),
        "Add a\n\nBecause.\n\n"
    );
    let ids: Vec<&str> = k.commands.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["file.open", "file.close"]);
    // Closed unsaved: cancelled.
    k.write("a.txt", "two\n");
    k.command("git.stageFile", Some("a.txt"));
    k.command("git.commit", Some("a.txt"));
    k.feed(Input::Closed(path.clone()));
    assert!(
        k.last_note().starts_with("The commit is cancelled"),
        "{}",
        k.last_note()
    );
    assert_eq!(k.git_ok(&["rev-list", "--count", "HEAD"]).trim(), "1");
    // Saved without a message: git refuses, the document stays to fix.
    k.command("git.commit", Some("a.txt"));
    let (_, text) = k.app.message_fill().unwrap();
    std::fs::write(&path, &text).unwrap();
    k.feed(Input::Saved(path.clone()));
    assert!(
        k.last_note().starts_with("Not committed"),
        "{}",
        k.last_note()
    );
    std::fs::write(&path, format!("Second\n{text}")).unwrap();
    k.feed(Input::Saved(path.clone()));
    assert_eq!(k.last_note(), "Committed");
    // Amend: the last message first.
    k.command("git.amend", Some("a.txt"));
    let (_, text) = k.app.message_fill().unwrap();
    assert!(text.starts_with("Second\n\n# Please"), "{text}");
}

/// The log as a document (DESIGN.md, 2.4): SPC g L in a file shows the
/// file's log, one commit a line; Right shows a commit's message and stat
/// under its line; Enter shows its view, and SPC g L there the log from
/// it; q closes.
#[test]
fn the_log_as_a_document() {
    let mut k = Kalem::new("log");
    k.write("a.txt", "one\n");
    k.commit_all("First, of a");
    k.write("b.txt", "b\n");
    k.commit_all("Second, of b");
    k.write("a.txt", "two\n");
    k.commit_all("Third, of a");
    k.command("git.log", Some("a.txt"));
    let key = format!("{}#log:a.txt", k.root());
    let text = k.docs[&key].0.clone();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].contains("Third, of a"), "{text}");
    assert!(lines[1].contains("First, of a"), "{text}");
    // Right: the commit's message and stat under its line.
    k.in_view(&key, "git.unfold", "Third");
    let text = k.docs[&key].0.clone();
    assert!(text.contains("a.txt | 2 +-"), "{text}");
    // Enter: the commit's view.
    k.in_view(&key, "git.visit", "First");
    let first = k
        .git_ok(&["rev-list", "--max-parents=0", "HEAD"])
        .trim()
        .to_string();
    let view = format!("{}#{first}", k.root());
    assert!(k.docs.contains_key(&view), "{:?}", k.docs.keys());
    // SPC g L in it: the log from that commit.
    k.in_view(&view, "git.log", "First");
    let from = format!("{}#log:{first}", k.root());
    assert_eq!(k.docs[&from].0.lines().count(), 1, "{}", k.docs[&from].0);
    // q closes the log.
    k.in_view(&key, "git.close", "Third");
    assert!(!k.docs.contains_key(&key));
}

/// A bare repository beside the test's, its `origin`.
fn with_origin(k: &mut Kalem) -> String {
    let remote = k
        .dir
        .parent()
        .unwrap()
        .join("origin.git")
        .display()
        .to_string();
    k.git_ok(&["init", "-q", "--bare", "-b", "main", &remote]);
    k.git_ok(&["remote", "add", "origin", &remote]);
    remote
}

#[test]
fn push_sets_the_upstream_and_pull_asks_how_once() {
    let mut k = Kalem::new("push");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    let remote = with_origin(&mut k);
    // No upstream: the remotes are offered.
    k.answers.push_back(Answer::Picked(vec![0]));
    k.command("git.push", Some("a.txt"));
    assert_eq!(
        k.asked.last().unwrap(),
        "The branch has no upstream: Push to origin/main and set it as the upstream"
    );
    assert_eq!(k.last_note(), "Pushed to origin/main, now its upstream");
    assert_eq!(k.status.as_deref(), Some("⎇ main"));
    assert!(
        k.texts().iter().any(|t| t == "main → origin/main"),
        "{:?}",
        k.texts()
    );

    // Another clone pushes a commit; this one commits its own.
    let other = k.dir.parent().unwrap().join("other");
    let mut g = NativeRunner::new(k.dir.parent().unwrap());
    isolated(&mut g);
    let out = g
        .run(&GitCommand::new([
            "clone",
            "-q",
            &remote,
            &other.display().to_string(),
        ]))
        .unwrap();
    assert!(out.success(), "{}", out.message());
    std::fs::write(other.join("b.txt"), "theirs\n").unwrap();
    g.cwd = other.clone();
    for args in [
        &["add", "-A"][..],
        &["commit", "-q", "-m", "Theirs"],
        &["push", "-q"],
    ] {
        let out = g.run(&GitCommand::new(args.iter().copied())).unwrap();
        assert!(out.success(), "{}", out.message());
    }
    k.write("a.txt", "ours\n");
    k.commit_all("Ours");
    k.command("git.fetch", Some("a.txt"));
    assert_eq!(k.last_note(), "Fetched");
    assert_eq!(k.status.as_deref(), Some("⎇ main ↑1 ↓1"));

    // Behind: pull first; the branches diverged, so how is asked, and
    // remembered; then the push.
    k.answers.push_back(Answer::Picked(vec![0]));
    k.answers.push_back(Answer::Picked(vec![0]));
    k.command("git.push", Some("a.txt"));
    assert!(k.asked[k.asked.len() - 2].starts_with("origin/main has 1 commit this branch lacks"));
    assert!(k.asked.last().unwrap().starts_with("The branches diverged"));
    assert_eq!(k.set, [("pull".to_string(), "\"rebase\"".to_string())]);
    assert_eq!(k.last_note(), "Pushed");
    assert_eq!(k.status.as_deref(), Some("⎇ main"));
    let subjects = k.git_ok(&["log", "--format=%s"]);
    assert_eq!(subjects, "Ours\nTheirs\nFirst\n");
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn a_lines_blame_reads_the_unsaved_text() {
    let mut k = Kalem::new("blame");
    k.write("a.txt", "one\ntwo\n");
    k.commit_all("First");
    let doc = |line: u32| Doc {
        path: Some(k.path("a.txt")),
        line,
        text: Some("one\nTWO, not saved\n".into()),
        ..Doc::default()
    };
    let (one, two) = (doc(1), doc(2));
    k.feed(Input::Command {
        id: "git.blameLine".into(),
        doc: Some(one),
    });
    let note = k.last_note().to_string();
    assert!(
        note.ends_with(" · Ayşe Test · 2026-10-07 · First"),
        "{note}"
    );
    k.feed(Input::Command {
        id: "git.blameLine".into(),
        doc: Some(two),
    });
    assert_eq!(k.last_note(), "Line 2 is not committed yet");
}

#[test]
fn the_command_lists_run_what_is_chosen() {
    let mut k = Kalem::new("dispatch");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    k.write("a.txt", "two\n");
    // SPC g /: every command; the second is Stage File.
    k.answers.push_back(Answer::Picked(vec![1]));
    k.command("git.dispatch", Some("a.txt"));
    assert!(
        k.asked
            .last()
            .unwrap()
            .starts_with("Git: Status | Stage File | ")
    );
    assert_eq!(k.index("a.txt"), "two\n");
    // SPC g .: this file's; cancelling does nothing.
    k.answers.push_back(Answer::Picked(vec![]));
    k.command("git.fileMenu", Some("a.txt"));
    assert!(
        k.asked
            .last()
            .unwrap()
            .starts_with("This file: Stage File | Unstage File")
    );
}

#[test]
fn outside_a_repository() {
    let mut k = Kalem::new("outside");
    let elsewhere = k.dir.parent().unwrap().join("plain");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("x.txt"), "x").unwrap();
    let x = elsewhere.join("x.txt").display().to_string();
    // Opening says nothing; a command says why.
    k.feed(Input::Opened(x.clone()));
    assert!(k.notes.is_empty());
    k.feed(Input::Command {
        id: "git.stageFile".into(),
        doc: Some(Doc {
            path: Some(x),
            line: 1,
            ..Doc::default()
        }),
    });
    assert_eq!(
        k.notes.last().unwrap(),
        &("Not in a git repository".to_string(), Level::Error)
    );
}

#[test]
fn the_status_document_folds_opens_and_acts_with_its_keys() {
    let mut k = Kalem::new("doc");
    k.prompt_messages();
    let long: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    k.write("a.txt", &long);
    k.write("b.txt", "bee\n");
    k.commit_all("First");
    k.write(
        "a.txt",
        &long
            .replace("line 2\n", "line TWO\n")
            .replace("line 29\n", "line 29!\n"),
    );
    k.write("new.txt", "new\n");
    k.command("git.status", Some("a.txt"));
    let text = k.doc_text();
    for part in [
        "\n repo → main  First\n",
        "\n 1 Files 2 │ 2 Local branches 1 │ 3 Commits 1 │ 4 Stash 0\n",
        "\n  M a.txt   +2 −2\n ?? new.txt\n",
    ] {
        assert!(text.contains(part), "{part:?} in\n{text}");
    }
    // The other tabs: `2`, `3`, `]` and `[`, Enter on a tab's name.
    k.on("git.tabBranches", "repo → main");
    assert!(k.doc_text().ends_with("\n * main\n"), "{}", k.doc_text());
    assert_eq!(
        k.cursor_line(),
        " * main",
        "the cursor on the tab's first line"
    );
    k.here("git.nextTab");
    assert!(k.doc_text().ends_with(" First\n"), "{}", k.doc_text());
    k.here("git.previousTab");
    k.here("git.previousTab");
    assert!(k.doc_text().contains("?? new.txt"), "{}", k.doc_text());
    let text = k.doc_text();
    let at = text.find("4 Stash").unwrap();
    k.in_doc("git.visit", at, at);
    assert!(
        k.doc_text().ends_with("Z z stashes the changes\n"),
        "{}",
        k.doc_text()
    );
    k.on("git.tabFiles", "repo → main");

    // Tab unfolds the file: its two hunks, read then.
    k.on("git.toggle", " M a.txt");
    let text = k.doc_text();
    assert!(
        text.contains("  M a.txt   +2 −2\n@@ -1,5 +1,5 @@"),
        "{text}"
    );
    assert!(text.contains("\n-line 2\n+line TWO\n"), "{text}");
    assert!(text.contains("\n+line 29!\n"), "{text}");
    assert_eq!(k.cursor_line(), "  M a.txt   +2 −2");
    // Right, on it unfolded: into it. Left folds the hunk, Left again goes
    // to the file, Left again folds it.
    k.here("git.unfold");
    assert!(k.cursor_line().starts_with("@@ -1,5 +1,5 @@"));
    k.here("git.fold");
    assert!(k.cursor_line().starts_with("@@ -1,5 +1,5 @@"));
    assert!(!k.doc_text().contains("+line TWO"));
    k.here("git.fold");
    assert_eq!(k.cursor_line(), "  M a.txt   +2 −2");
    k.here("git.toggle");
    assert!(!k.doc_text().contains("@@ -1,5"));
    k.here("git.toggle");
    // The hunk folded stays so; Tab on it unfolds it.
    assert!(!k.doc_text().contains("+line TWO"));
    k.on("git.toggle", "@@ -1,5 +1,5 @@");
    assert!(k.doc_text().contains("+line TWO"));

    // Enter on a diff line opens the file there; on the file, the file.
    k.on("git.visit", "+line 29!");
    let a = k.path("a.txt");
    assert_eq!(
        k.commands.last().unwrap(),
        &(
            "file.open".to_string(),
            format!("{{\"path\":\"{a}\",\"line\":29}}")
        )
    );
    k.on("git.visit", " M a.txt");
    assert_eq!(
        k.commands.last().unwrap(),
        &("file.open".to_string(), format!("{{\"path\":\"{a}\"}}"))
    );

    // `s` on the second hunk stages it alone: the file has both, each
    // shown under it.
    k.on("git.stage", "+line 29!");
    assert_eq!(k.last_note(), "Staged a hunk of a.txt");
    assert!(k.index("a.txt").contains("line 29!\n"));
    assert!(!k.index("a.txt").contains("line TWO"));
    let text = k.doc_text();
    assert!(
        text.contains("\n MM a.txt   +2 −2\n  unstaged:\n@@"),
        "{text}"
    );
    assert!(text.contains("\n  staged:\n@@"), "{text}");
    // A line selected alone: `s` stages that line.
    let text = k.doc_text();
    let plus = text.find("+line TWO").unwrap();
    k.in_doc("git.stage", plus, plus + "+line TWO".len());
    assert!(
        k.index("a.txt").contains("line TWO\n"),
        "{}",
        k.index("a.txt")
    );
    assert!(
        k.index("a.txt").contains("line 2\n"),
        "{}",
        k.index("a.txt")
    );

    // `u` on the file unstages what is staged; `a` stages everything,
    // the untracked file too, and `a` again unstages everything.
    k.on("git.unstage", " MM a.txt");
    assert_eq!(k.index("a.txt"), long);
    k.on("git.stageEverything", "1 Files");
    assert_eq!(k.last_note(), "Staged everything");
    assert!(k.doc_text().contains("\n M  a.txt"), "{}", k.doc_text());
    assert!(k.doc_text().contains("\n A  new.txt"), "{}", k.doc_text());
    k.on("git.stageEverything", "1 Files");
    assert_eq!(k.last_note(), "Unstaged everything");
    assert!(k.doc_text().contains("\n ?? new.txt"), "{}", k.doc_text());

    // `d` on the untracked file: lazygit's choice, Cancel last.
    k.answers.push_back(Answer::Picked(vec![0]));
    k.on("git.discard", "?? new.txt");
    assert_eq!(
        k.asked.last().unwrap(),
        "new.txt: Discard all changes: delete the file | Cancel"
    );
    assert!(!k.dir.join("new.txt").exists());
    // On a file staged and not: all of it, or its unstaged changes only.
    k.git_ok(&["add", "a.txt"]);
    k.write("a.txt", &long.replace("line 5\n", "line FIVE\n"));
    k.on("git.refresh", "1 Files");
    k.answers.push_back(Answer::Picked(vec![1]));
    k.on("git.discard", "MM a.txt");
    assert_eq!(
        k.asked.last().unwrap(),
        "a.txt: Discard all changes | Discard unstaged changes | Cancel"
    );
    assert!(!k.read("a.txt").contains("FIVE"));
    assert!(k.index("a.txt").contains("line TWO"));
    // Cancel does nothing.
    k.answers.push_back(Answer::Picked(vec![1]));
    k.on("git.discard", "M  a.txt");
    assert!(k.index("a.txt").contains("line TWO"));

    // `c c` commits; the status follows.
    k.answers.push_back(Answer::Text(Some("Second".into())));
    k.on("git.commit", "repo → main");
    assert_eq!(k.last_note(), "Committed");
    assert!(
        k.doc_text().contains(" repo → main  Second"),
        "{}",
        k.doc_text()
    );
    assert!(k.doc_text().contains("Nothing changed"), "{}", k.doc_text());

    // `q` closes it.
    k.on("git.close", "repo → main");
    assert!(k.docs.is_empty());
}

/// `d` on a folded file staged and not offers to discard all its changes:
/// the choice was there only once its staged diff was read (Tab), else
/// only "Discard unstaged changes" was. It goes back to HEAD, a rename to
/// its old path.
#[test]
fn discard_all_on_a_folded_file() {
    let mut k = Kalem::new("discard-all");
    let long: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    k.write("a.txt", "one\n");
    k.write("old.txt", &long);
    k.commit_all("First");
    k.write("a.txt", "two\n");
    k.git_ok(&["add", "a.txt"]);
    k.write("a.txt", "three\n");
    k.command("git.status", Some("a.txt"));
    k.answers.push_back(Answer::Picked(vec![0]));
    k.on("git.discard", "MM a.txt");
    assert_eq!(
        k.asked.last().unwrap(),
        "a.txt: Discard all changes | Discard unstaged changes | Cancel"
    );
    assert_eq!(k.last_note(), "Reverted a.txt");
    assert_eq!(k.read("a.txt"), "one\n");
    assert_eq!(k.index("a.txt"), "one\n");
    // A rename staged, then edited.
    k.git_ok(&["mv", "old.txt", "new.txt"]);
    k.write("new.txt", &long.replace("line 5\n", "line FIVE\n"));
    k.on("git.refresh", "1 Files");
    k.answers.push_back(Answer::Picked(vec![0]));
    k.on("git.discard", "old.txt → new.txt");
    assert_eq!(
        k.asked.last().unwrap(),
        "new.txt: Discard all changes | Discard unstaged changes | Cancel"
    );
    assert!(!k.dir.join("new.txt").exists());
    assert_eq!(k.read("old.txt"), long);
    assert_eq!(k.git_ok(&["status", "--porcelain"]), "");
}

/// A file in conflict staged with its conflict markers left asks first:
/// `s` (the panel's box, `a`, `S` too) staged the markers without a word,
/// and `c c` then committed them.
#[test]
fn staging_a_conflict_with_its_markers_asks_first() {
    let mut k = Kalem::new("markers");
    for f in ["f.txt", "g.txt", "h.txt"] {
        k.write(f, "base\n");
    }
    k.commit_all("Base");
    k.git_ok(&["switch", "-q", "-c", "other"]);
    k.write("f.txt", "theirs\n");
    k.write("g.txt", "theirs\n");
    k.commit_all("Theirs");
    k.git_ok(&["switch", "-q", "main"]);
    k.write("f.txt", "ours\n");
    k.write("g.txt", "ours\n");
    k.commit_all("Ours");
    let out = k
        .git
        .run(&GitCommand::new(["merge", "--no-edit", "other"]))
        .unwrap();
    assert!(!out.success());
    k.write("h.txt", "changed\n");
    let conflicts = |k: &mut Kalem| k.git_ok(&["diff", "--name-only", "--diff-filter=U"]);
    k.command("git.status", Some("f.txt"));
    // `s`, `S` and `a` ask; no stages nothing.
    k.answers.push_back(Answer::Confirmed(false));
    k.on("git.stage", "UU f.txt");
    assert_eq!(
        k.asked.last().unwrap(),
        "f.txt still has conflict markers (<<<<<<<): stage it anyway?"
    );
    for id in ["git.stageAll", "git.stageEverything"] {
        k.answers.push_back(Answer::Confirmed(false));
        k.on(id, "1 Files");
        assert_eq!(
            k.asked.last().unwrap(),
            "2 files still have conflict markers (<<<<<<<): stage them anyway?",
            "{id}"
        );
    }
    assert_eq!(conflicts(&mut k), "f.txt\ng.txt\n");
    assert_eq!(k.index("h.txt"), "base\n");
    // Resolved, it is staged without a question.
    k.write("f.txt", "ours and theirs\n");
    let asked = k.asked.len();
    k.on("git.stage", "UU f.txt");
    assert_eq!(k.asked.len(), asked, "{:?}", k.asked);
    assert_eq!(k.index("f.txt"), "ours and theirs\n");
    // The panel's box asks too; yes stages the file as it is.
    k.answers.push_back(Answer::Confirmed(true));
    k.feed(Input::Panel {
        key: "stage:unmerged:g.txt".into(),
        event: PanelEvent::Toggled(true),
    });
    assert_eq!(
        k.asked.last().unwrap(),
        "g.txt still has conflict markers (<<<<<<<): stage it anyway?"
    );
    assert!(k.index("g.txt").contains("<<<<<<<"));
    assert_eq!(conflicts(&mut k), "");
}

/// A key pressed after a refresh, before the status is written again,
/// acts on what the status shows: it took the hunk's number from the text
/// and its patch from the diffs read again, and `x` on the second hunk
/// shown discarded a change the user had not seen yet.
#[test]
fn a_key_before_the_status_is_written_again_acts_on_what_it_shows() {
    let mut k = Kalem::new("shown");
    let long: String = (1..=40).map(|n| format!("line {n}\n")).collect();
    k.write("a.txt", &long);
    k.write("b.txt", "bee\n");
    k.commit_all("First");
    let shown = long
        .replace("line 2\n", "line TWO\n")
        .replace("line 30\n", "line 30!\n");
    k.write("a.txt", &shown);
    k.write("b.txt", "BEE\n");
    k.command("git.status", Some("a.txt"));
    k.on("git.toggle", " M a.txt");
    k.on("git.toggle", " M b.txt");
    assert!(k.doc_text().contains("\n+line 30!\n"), "{}", k.doc_text());
    // Line 15 changed and saved: the status is read again, and written
    // once b.txt's diff is, which is slow.
    let saved = shown.replace("line 15\n", "line 15, not seen\n");
    k.write("a.txt", &saved);
    k.hold = Some("b.txt".into());
    let a = k.path("a.txt");
    k.feed(Input::Saved(a));
    assert!(!k.held.is_empty());
    assert!(!k.doc_text().contains("not seen"), "{}", k.doc_text());
    // `x` on the hunk of line 30, the second shown.
    k.answers.push_back(Answer::Picked(vec![0]));
    k.on("git.discard", "+line 30!");
    assert_eq!(k.asked.last().unwrap(), "a.txt: Discard this hunk | Cancel");
    assert_eq!(k.read("a.txt"), saved.replace("line 30!\n", "line 30\n"));
    k.hold = None;
    k.release();
    assert!(
        k.doc_text().contains("+line 15, not seen"),
        "{}",
        k.doc_text()
    );
    assert!(!k.doc_text().contains("+line 30!"), "{}", k.doc_text());
}

/// Kalem runs 16 programs of a plugin at a time and refuses the next,
/// which the plugin took for an empty answer: Shift+Tab over many files
/// showed most without their diffs, the marks of the files open went, a
/// refresh refused dropped the repository. It runs 12 at a time, a
/// refused run waiting for another to end; the marks of the files open
/// are read in one run, a closed file's no more.
#[test]
fn runs_wait_their_turn() {
    let mut k = Kalem::new("runs");
    let names: Vec<String> = (0..20).map(|i| format!("f{i:02}.txt")).collect();
    for n in &names {
        k.write(n, "one\ntwo\n");
    }
    k.commit_all("First");
    for n in &names {
        k.write(n, "one\nTWO\n");
        k.git_ok(&["add", n]);
        k.write(n, "one\nTWO\nthree\n");
    }
    for n in &names {
        let p = k.path(n);
        k.feed(Input::Opened(p));
    }
    assert_eq!(k.gutters.len(), 20);
    k.command("git.status", Some("f00.txt"));
    k.on("git.cycle", "1 Files");
    let text = k.doc_text();
    assert_eq!(text.matches("\n+TWO\n").count(), 20, "{text}");
    assert_eq!(text.matches("\n+three\n").count(), 20, "{text}");
    assert!(k.most_runs <= MAX_RUNS, "{}", k.most_runs);
    // Kalem taking fewer: the runs it refuses wait for the others.
    k.kalem_runs = 3;
    k.on("git.refresh", "1 Files");
    let text = k.doc_text();
    assert_eq!(text.matches("\n+three\n").count(), 20, "{text}");
    assert_eq!(k.status.as_deref(), Some("⎇ main •20"));
    assert_eq!(k.gutters.len(), 20);
    assert!(k.notes.is_empty(), "{:?}", k.notes);
    // One run marks the files open; a file closed is not among them.
    let p = k.path("f00.txt");
    k.feed(Input::Closed(p));
    let before = k.git.log.runs().map(|r| r.number).max().unwrap_or(0);
    let p = k.path("f01.txt");
    k.feed(Input::Saved(p));
    let marks: Vec<String> = k
        .git
        .log
        .runs()
        .filter(|r| r.number > before && r.command.contains(" -U0 "))
        .map(|r| r.command.clone())
        .collect();
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert!(
        !marks[0].contains("f00.txt") && marks[0].contains(" f01.txt f02.txt "),
        "{}",
        marks[0]
    );
}

/// A big file of a commit starts folded, saying how big: a commit of
/// 500,000 lines took 119 ms to write each time a key moved in its view.
/// An answer cut at Kalem's 16 MB says so: read, a diff cut short failed
/// as "a hunk shorter than its header".
#[test]
fn big_diffs_and_answers_cut_short() {
    let mut k = Kalem::new("big");
    k.write("small.txt", "one\n");
    k.commit_all("First");
    let big: String = (1..=10_001).map(|n| format!("line {n}\n")).collect();
    k.write("big.txt", &big);
    k.write("small.txt", "two\n");
    k.commit_all("Big");
    k.command("git.status", Some("small.txt"));
    k.on("git.tabCommits", "repo → main");
    let short = k
        .git_ok(&["rev-parse", "--short", "HEAD"])
        .trim()
        .to_string();
    k.on("git.visit", &short);
    let text = k.commit_text();
    assert!(
        text.contains("\ndiff --git a/big.txt b/big.txt\n  10,001 changed lines: Tab shows them\n"),
        "{text}"
    );
    assert!(!text.contains("+line 5000\n"), "{text}");
    assert!(text.ends_with("-one\n+two\n"), "{text}");
    k.on_commit("git.toggle", "diff --git a/big.txt");
    assert!(k.commit_text().contains("\n+line 5000\n"));
    k.on_commit("git.toggle", "diff --git a/big.txt");
    assert!(!k.commit_text().contains("\n+line 5000\n"));
    k.on_commit("git.close", "commit ");
    // A file's diff cut short.
    k.write("small.txt", "three\n");
    k.cut = Some("small.txt".into());
    k.on("git.refresh", "repo → main");
    k.on("git.tabFiles", "repo → main");
    k.on("git.toggle", " M small.txt");
    assert!(
        k.doc_text().contains(
            "\n  Too large to show: git's answer passed 16 MB, all Kalem keeps of a program's output\n"
        ),
        "{}",
        k.doc_text()
    );
    // A commit's.
    k.cut = Some("--format=fuller".into());
    k.on("git.tabCommits", "repo → main");
    k.on("git.visit", &short);
    assert!(k.commit_key().is_none());
    assert_eq!(
        k.notes.last().unwrap(),
        &(
            "The commit is too large to show: git's answer passed 16 MB, all Kalem keeps of a program's output"
                .to_string(),
            Level::Error
        )
    );
}

/// A merge, a rebase or a cherry-pick stopped on a conflict goes on from
/// the status: `r r` (`m m`, `A A`) continues, `r s` skips, `r a` aborts,
/// and Enter on its line offers them. Enter only said to use git, and
/// `GIT_EDITOR=false` made a continue fail where git wants its editor on
/// the message it made.
#[test]
fn a_merge_or_a_rebase_goes_on_from_the_status() {
    let mut k = Kalem::new("sequence");
    k.write("f.txt", "base\n");
    k.commit_all("Base");
    for b in ["a", "b", "c"] {
        k.git_ok(&["switch", "-q", "-c", b, "main"]);
        k.write("f.txt", &format!("{b}\n"));
        k.commit_all(&format!("Branch {b}"));
    }
    k.git_ok(&["switch", "-q", "main"]);
    k.write("f.txt", "ours\n");
    k.commit_all("Ours");
    let fails = |k: &mut Kalem, args: &[&str]| {
        let out = k.git.run(&GitCommand::new(args.iter().copied())).unwrap();
        assert!(!out.success(), "{args:?}");
    };
    fails(&mut k, &["merge", "--no-edit", "a"]);
    k.command("git.status", Some("f.txt"));
    assert!(
        k.doc_text()
            .contains("\n Merging (1 unmerged)  (Enter: continue, abort)\n"),
        "{}",
        k.doc_text()
    );
    // In conflict still: continuing says so.
    k.on("git.continue", " Merging");
    assert_eq!(
        k.last_note(),
        "1 file is still in conflict: resolve and stage it (s), then continue"
    );
    // Resolved and staged, Enter on its line: the merge is committed, its
    // message the one git made.
    k.write("f.txt", "both\n");
    k.git_ok(&["add", "f.txt"]);
    k.on("git.refresh", " Merging");
    k.answers.push_back(Answer::Picked(vec![0]));
    k.on("git.visit", " Merging");
    assert_eq!(
        k.asked.last().unwrap(),
        "Merging: Continue the merge | Abort the merge…"
    );
    assert_eq!(k.last_note(), "Continued the merge");
    assert_eq!(
        k.git_ok(&["log", "-1", "--format=%s"]).trim(),
        "Merge branch 'a'"
    );
    assert!(!k.doc_text().contains("Merging"), "{}", k.doc_text());
    // A rebase stopped on a conflict: aborted after asking.
    k.git_ok(&["switch", "-q", "b"]);
    fails(&mut k, &["rebase", "main"]);
    k.on("git.refresh", "repo → ");
    k.answers.push_back(Answer::Confirmed(true));
    k.on("git.abort", " Rebasing");
    assert!(k.asked.last().unwrap().starts_with("Abort the rebase?"));
    assert_eq!(k.last_note(), "Aborted the rebase");
    assert_eq!(k.git_ok(&["branch", "--show-current"]).trim(), "b");
    assert_eq!(k.read("f.txt"), "b\n");
    // Again, resolved: continued to its end.
    fails(&mut k, &["rebase", "main"]);
    k.write("f.txt", "both and b\n");
    k.git_ok(&["add", "f.txt"]);
    k.on("git.refresh", "repo → ");
    k.on("git.continue", " Rebasing");
    assert_eq!(k.last_note(), "Continued the rebase");
    assert_eq!(k.git_ok(&["branch", "--show-current"]).trim(), "b");
    assert_eq!(
        k.git_ok(&["log", "-2", "--format=%s"]),
        "Branch b\nMerge branch 'a'\n"
    );
    // A cherry-pick stopped: its commit skipped.
    k.git_ok(&["switch", "-q", "main"]);
    fails(&mut k, &["cherry-pick", "c"]);
    k.on("git.refresh", "repo → ");
    k.answers.push_back(Answer::Confirmed(true));
    k.on("git.skip", " Cherry-picking");
    assert_eq!(k.last_note(), "Skipped a commit of the cherry-pick");
    assert_eq!(k.read("f.txt"), "both\n");
    assert!(!k.doc_text().contains("Cherry-picking"), "{}", k.doc_text());
}

/// Back in the status from another document, it is read again: a commit
/// made in a terminal showed only after `SPC g g`.
#[test]
fn the_status_is_read_again_when_the_cursor_comes_back() {
    let mut k = Kalem::new("returned");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    k.write("a.txt", "two\n");
    k.command("git.status", Some("a.txt"));
    assert!(k.doc_text().contains("\n  M a.txt"), "{}", k.doc_text());
    k.commit_all("Made in a terminal");
    let root = k.root();
    k.feed(Input::Returned(root));
    let text = k.doc_text();
    assert!(
        text.contains(" repo → main  Made in a terminal\n"),
        "{text}"
    );
    assert!(text.contains("Nothing changed"), "{text}");
}

/// Tab on an untracked folder lists its files: `git diff --no-index`
/// takes no folder, and the status said "No change to show".
#[test]
fn tab_on_an_untracked_folder_lists_its_files() {
    let mut k = Kalem::new("folder");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    k.write("new/x.txt", "x\n");
    k.write("new/deep/y.txt", "y\n");
    k.command("git.status", Some("a.txt"));
    k.on("git.toggle", "?? new/");
    let text = k.doc_text();
    assert!(
        text.contains(" ?? new/\n  new/deep/y.txt\n  new/x.txt\n"),
        "{text}"
    );
    assert!(!text.contains("No change to show"), "{text}");
}

#[test]
fn a_new_branch_starts_where_asked() {
    let mut k = Kalem::new("branch-start");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    k.git_ok(&["branch", "old"]);
    k.write("a.txt", "two\n");
    k.commit_all("Second");
    k.command("git.status", Some("a.txt"));
    // Where HEAD is, then `old`: picked, the branch is made there.
    k.answers.push_back(Answer::Picked(vec![1]));
    k.answers.push_back(Answer::Text(Some("fix".into())));
    k.on("git.newBranch", "repo → ");
    assert!(
        k.asked.iter().any(|a| a.ends_with(" | old")),
        "{:?}",
        k.asked
    );
    assert_eq!(
        k.asked.last().map(String::as_str),
        Some("A new branch from old, switched to: its name")
    );
    assert_eq!(
        k.last_note(),
        "Made the branch fix from old, and switched to it"
    );
    assert_eq!(
        k.git_ok(&["rev-parse", "fix"]),
        k.git_ok(&["rev-parse", "old"])
    );
    assert_eq!(k.git_ok(&["branch", "--show-current"]).trim(), "fix");
}

#[test]
fn branches_stashes_and_amends_from_the_status() {
    let mut k = Kalem::new("branches");
    k.prompt_messages();
    k.write("a.txt", "one\n");
    k.commit_all("First\n\nThe body stays.");
    k.git_ok(&["branch", "feature"]);
    k.command("git.status", Some("a.txt"));
    assert!(
        k.doc_text().contains("2 Local branches 2"),
        "{}",
        k.doc_text()
    );

    // Enter on a branch, in its tab, switches to it.
    k.on("git.tabBranches", "repo → ");
    k.on("git.visit", "   feature");
    assert_eq!(k.last_note(), "Switched to feature");
    assert!(k.doc_text().contains(" repo → feature"), "{}", k.doc_text());
    assert!(k.doc_text().contains("\n * feature"), "{}", k.doc_text());

    // `b c` asks where the branch starts, where HEAD is first, then its
    // name; `b b` offers the others.
    k.answers.push_back(Answer::Picked(vec![0]));
    k.answers.push_back(Answer::Text(Some("topic".into())));
    k.on("git.newBranch", "repo → ");
    assert!(
        k.asked
            .iter()
            .any(|a| a.starts_with("A new branch starting at: feature | ")),
        "{:?}",
        k.asked
    );
    assert_eq!(k.last_note(), "Made the branch topic, and switched to it");
    k.answers.push_back(Answer::Picked(vec![1]));
    k.on("git.switchBranch", "repo → ");
    assert!(
        k.asked
            .last()
            .unwrap()
            .starts_with("Switch to the branch: "),
        "{:?}",
        k.asked
    );
    assert!(k.doc_text().contains(" repo → "), "{}", k.doc_text());

    // `Z z` stashes; Enter on the stash pops it.
    k.write("a.txt", "two\n");
    k.on("git.stash", "repo → ");
    assert_eq!(k.last_note(), "Stashed the changes");
    k.on("git.tabStash", "repo → ");
    assert!(k.doc_text().contains("\n stash@{0}: "), "{}", k.doc_text());
    k.answers.push_back(Answer::Picked(vec![1]));
    k.on("git.visit", "stash@{0}");
    assert_eq!(k.last_note(), "Applied and dropped stash@{0}");
    assert_eq!(
        std::fs::read_to_string(k.dir.join("a.txt")).unwrap(),
        "two\n"
    );

    // `c a` amends with what is staged, the first line offered, the body
    // kept.
    k.git_ok(&["add", "a.txt"]);
    k.answers
        .push_back(Answer::Text(Some("First, amended".into())));
    k.on("git.amend", "repo → ");
    assert_eq!(k.offered.last().unwrap().as_deref(), Some("First"));
    assert_eq!(k.last_note(), "Amended the last commit");
    assert_eq!(
        k.git_ok(&["log", "-1", "--format=%B"]),
        "First, amended\n\nThe body stays.\n\n"
    );
    assert_eq!(k.git_ok(&["show", "HEAD:a.txt"]), "two\n");
}

#[test]
fn the_changed_lines_of_an_open_file_are_marked() {
    let mut k = Kalem::new("gutter");
    k.write("a.txt", "one\ntwo\nthree\nfour\n");
    k.commit_all("First");
    k.write("a.txt", "zero\none\nTWO\nfour\n");
    let a = k.path("a.txt");
    k.feed(Input::Opened(a.clone()));
    assert_eq!(
        k.gutters[&a],
        [(1, Mark::Added), (3, Mark::Changed), (3, Mark::Removed),]
    );
    // Staged, nothing differs from the index any more.
    k.command("git.stageFile", Some("a.txt"));
    assert!(!k.gutters.contains_key(&a), "{:?}", k.gutters);
    // Edited and saved again: marked again.
    k.write("a.txt", "zero\none\nTWO\nfour\nfive\n");
    k.feed(Input::Saved(a.clone()));
    assert_eq!(k.gutters[&a], [(5, Mark::Added)]);
    // The setting off: every mark goes.
    k.feed(Input::Settings(Settings {
        gutter: false,
        ..Settings::default()
    }));
    assert!(k.gutters.is_empty());
}

#[test]
fn enter_on_a_commit_shows_it_as_magit_does() {
    let mut k = Kalem::new("commit-view");
    k.write("a.txt", "one\ntwo\n");
    k.commit_all("First");
    k.write("a.txt", "one\nTWO\n");
    k.commit_all("Second line changed");
    k.command("git.status", Some("a.txt"));
    k.on("git.tabCommits", "repo → main");
    assert!(
        k.doc_text().contains("Second line changed"),
        "{}",
        k.doc_text()
    );
    // Enter on the commit's line (the head line names the subject too):
    // its header, stat and diff.
    let short = k
        .git_ok(&["rev-parse", "--short", "HEAD"])
        .trim()
        .to_string();
    k.on("git.visit", &short);
    let text = k.commit_text();
    assert!(text.starts_with("commit "), "{text}");
    assert!(
        text.contains("Author:     Ayşe Test <ayse@example.com>"),
        "{text}"
    );
    assert!(text.contains("\n    Second line changed\n"), "{text}");
    assert!(text.contains(" a.txt | 2 +-\n"), "{text}");
    assert!(
        text.contains("\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n"),
        "{text}"
    );
    let key = k.commit_key().unwrap();
    assert_eq!(k.showed.last(), Some(&key));
    // Enter on the added line opens the file at it.
    k.on_commit("git.visit", "+TWO");
    let (id, args) = k.commands.last().cloned().unwrap();
    assert_eq!(id, "file.open");
    assert!(
        args.contains("a.txt") && args.ends_with("\"line\":2}"),
        "{args}"
    );
    // Left folds the file, Right unfolds it, the cursor kept on it.
    k.on_commit("git.fold", "diff --git");
    assert!(!k.commit_text().contains("+TWO"), "{}", k.commit_text());
    assert!(
        k.commit_text().contains("diff --git"),
        "{}",
        k.commit_text()
    );
    k.on_commit("git.unfold", "diff --git");
    assert!(k.commit_text().contains("+TWO"), "{}", k.commit_text());
    // Nothing is staged from it.
    k.on_commit("git.stage", "+TWO");
    assert!(
        k.last_note().starts_with("Nothing is staged from a commit"),
        "{}",
        k.last_note()
    );
    // Escape closes it; the status stays.
    k.on_commit("git.close", "commit ");
    assert!(k.commit_key().is_none());
    assert!(k.doc_text().contains("Second line changed"));
}

/// Whether when-clause `when` holds in a document of `text_type` with
/// `flags` set, as Kalem's `WhenClause` reads the plugin's clauses:
/// `textType == X`, flags, `!`, `&&`, `||` and parentheses.
fn holds(when: &str, text_type: &str, flags: &[&str]) -> bool {
    struct Reader<'a> {
        tokens: Vec<&'a str>,
        at: usize,
        text_type: &'a str,
        flags: &'a [&'a str],
    }
    impl<'a> Reader<'a> {
        fn next(&mut self) -> &'a str {
            self.at += 1;
            self.tokens.get(self.at - 1).copied().unwrap_or_default()
        }
        fn peek(&self) -> &'a str {
            self.tokens.get(self.at).copied().unwrap_or_default()
        }
        fn or(&mut self) -> bool {
            let mut v = self.and();
            while self.peek() == "||" {
                self.next();
                v |= self.and();
            }
            v
        }
        fn and(&mut self) -> bool {
            let mut v = self.not();
            while self.peek() == "&&" {
                self.next();
                v &= self.not();
            }
            v
        }
        fn not(&mut self) -> bool {
            match self.next() {
                "!" => !self.not(),
                "(" => {
                    let v = self.or();
                    assert_eq!(self.next(), ")");
                    v
                }
                "textType" => {
                    assert_eq!(self.next(), "==");
                    self.next() == self.text_type
                }
                flag => self.flags.contains(&flag),
            }
        }
    }
    assert!(!when.contains("!="), "{when}");
    let spaced = when
        .replace('(', " ( ")
        .replace(')', " ) ")
        .replace('!', " ! ");
    let mut r = Reader {
        tokens: spaced.split_whitespace().collect(),
        at: 0,
        text_type,
        flags,
    };
    let v = r.or();
    assert_eq!(r.at, r.tokens.len(), "{when}");
    v
}

/// The commands Kalem runs for `keys` in a document of `text_type` with
/// `flags`: the bindings the component hands its keymap, each binding's
/// when-clause ANDed with its command's scope, as Kalem's keymap does.
fn resolve(keys: &str, text_type: &str, flags: &[&str]) -> Vec<&'static str> {
    COMMANDS
        .iter()
        .filter(|c| c.scope().is_none_or(|kinds| kinds.contains(&text_type)))
        .filter(|c| {
            c.bindings()
                .iter()
                .any(|(k, when)| *k == keys && holds(when, text_type, flags))
        })
        .map(|c| c.id)
        .collect()
}

/// Every key the plugin binds, once.
fn bound_keys() -> Vec<&'static str> {
    let mut keys: Vec<&str> = COMMANDS
        .iter()
        .flat_map(|c| c.bindings())
        .map(|(k, _)| k)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// The keys of a commit view reached it: Kalem ANDs a command's scope with
/// its keys' when-clauses, and the folds' commands were scoped to the
/// status alone, so their keys, bound in both documents, ran in neither
/// view of a commit.
#[test]
fn every_key_runs_its_command_where_kalem_looks_it_up() {
    const VIM: &[&str] = &["vimCommand", "vimActive"];
    const WORD: &[&str] = &[];
    let (leader, doc): (Vec<&str>, Vec<&str>) = bound_keys()
        .into_iter()
        .partition(|k| k.starts_with("space "));
    for flags in [VIM, WORD] {
        // A commit view: the movement, the folds, Enter and closing; the
        // letters are Vim's.
        let found: Vec<(&str, &str)> = doc
            .iter()
            .flat_map(|k| {
                resolve(k, COMMIT_KIND, flags)
                    .into_iter()
                    .map(move |c| (*k, c))
            })
            .collect();
        assert_eq!(
            found,
            [
                ("alt+down", "git.next"),
                ("alt+enter", "git.dispatch"),
                ("alt+up", "git.previous"),
                ("enter", "git.visit"),
                ("escape", "git.close"),
                ("left", "git.fold"),
                ("q", "git.close"),
                ("right", "git.unfold"),
                ("tab", "git.toggle"),
            ],
            "with {flags:?}"
        );
        // The leader's keys, in Vim's command mode.
        for k in &leader {
            let runs = !resolve(k, COMMIT_KIND, flags).is_empty();
            assert_eq!(runs, flags == VIM, "`{k}` with {flags:?}");
        }
        // The status: each document key runs its own command, and only it.
        for c in COMMANDS {
            for k in c.doc_keys {
                assert_eq!(
                    resolve(k, STATUS_KIND, flags),
                    [c.id],
                    "`{k}` in the status"
                );
            }
        }
    }
    // In a file only the leader's keys act, in Vim's command mode.
    for k in bound_keys() {
        let leader = k.starts_with("space ");
        assert_eq!(
            !resolve(k, "org", VIM).is_empty(),
            leader,
            "`{k}` in a file"
        );
        assert!(resolve(k, "org", WORD).is_empty(), "`{k}` in a file");
    }
    // Escape clears a selection first; in insert mode the letters type.
    let selected = &["vimCommand", "vimActive", "hasSelection"];
    assert!(resolve("escape", COMMIT_KIND, selected).is_empty());
    assert!(resolve("q", COMMIT_KIND, &["vimActive"]).is_empty());
}

#[test]
fn the_keys_of_a_commit_view_act_in_it_and_the_repositorys_commands_go_on() {
    let mut k = Kalem::new("commit-view-keys");
    k.write("a.txt", "one\ntwo\n");
    k.write("b.txt", "x\n");
    k.commit_all("First");
    k.write("a.txt", "one\nTWO\n");
    k.write("b.txt", "y\n");
    k.commit_all("Both changed");
    k.command("git.status", Some("a.txt"));
    k.on("git.tabCommits", "repo → main");
    let short = k
        .git_ok(&["rev-parse", "--short", "HEAD"])
        .trim()
        .to_string();
    k.on("git.visit", &short);
    assert!(k.commit_text().contains("diff --git a/b.txt"));
    // Alt+Down steps from a file to its hunk, then to the next file.
    k.on_commit("git.next", "diff --git a/a.txt");
    let (text, at) = k.docs[&k.commit_key().unwrap()].clone();
    assert!(text[at..].starts_with("@@ -1,2 +1,2 @@"), "{}", &text[at..]);
    k.on_commit("git.next", "@@ -1,2 +1,2 @@");
    let (text, at) = k.docs[&k.commit_key().unwrap()].clone();
    assert!(
        text[at..].starts_with("diff --git a/b.txt"),
        "{}",
        &text[at..]
    );
    // Each of its keys Kalem runs in a commit view acts there.
    let mut ids: Vec<&str> = bound_keys()
        .into_iter()
        .filter(|key| !key.starts_with("space "))
        .flat_map(|key| resolve(key, COMMIT_KIND, &["vimCommand", "vimActive"]))
        .filter(|id| *id != "git.close")
        .collect();
    ids.dedup();
    for id in ids {
        let notes = k.notes.len();
        // Alt+Enter's menu, cancelled.
        if id == "git.dispatch" {
            k.answers.push_back(Answer::Picked(Vec::new()));
        }
        k.on_commit(id, "diff --git a/a.txt");
        let refused = k.notes[notes..].iter().any(|(n, _)| {
            n.starts_with("Nothing is staged from a commit") || n.starts_with("This acts in")
        });
        assert!(!refused, "{id}: {:?}", &k.notes[notes..]);
        assert!(k.commit_key().is_some(), "{id} closed the view");
    }
    // The commands of every text type act on the commit's repository: a
    // new branch, not "Nothing is staged"; a file's needs a file.
    k.answers.push_back(Answer::Text(Some("topic".into())));
    k.on_commit("git.newBranch", "commit ");
    assert_eq!(k.last_note(), "Made the branch topic, and switched to it");
    assert_eq!(k.git_ok(&["branch", "--show-current"]).trim(), "topic");
    k.on_commit("git.blameLine", "commit ");
    assert!(
        k.last_note().contains("acts on a file"),
        "{}",
        k.last_note()
    );
    // The status's own are not the view's.
    k.on_commit("git.stage", "commit ");
    assert!(
        k.last_note().starts_with("Nothing is staged from a commit"),
        "{}",
        k.last_note()
    );
    k.on_commit("git.close", "commit ");
    assert!(k.commit_key().is_none());
}

/// The full hash of `rev`, and its short one as the plugin writes it.
fn hashes(k: &mut Kalem, rev: &str) -> (String, String) {
    let full = k.git_ok(&["rev-parse", rev]).trim().to_string();
    let short = full[..7].to_string();
    (full, short)
}

/// A commit's menu (DESIGN.md, 2.4): Alt+Enter (and `SPC g /`) reaches it
/// in a log, a commit's view and the status. In the log it lists what can
/// be done with the commit under the cursor, then the repository's
/// commands; Cherry-pick makes the commit's changes a commit of the
/// current branch, and one stopped on a conflict shows the status saying
/// so, from where it goes on or is aborted.
#[test]
fn a_commits_menu_cherry_picks_and_a_conflict_shows_in_the_status() {
    let vim = &["vimCommand", "vimActive"];
    for kind in [STATUS_KIND, COMMIT_KIND] {
        assert_eq!(resolve("alt+enter", kind, vim), ["git.dispatch"], "{kind}");
        assert_eq!(resolve("alt+enter", kind, &[]), ["git.dispatch"], "{kind}");
        assert_eq!(resolve("space g /", kind, vim), ["git.dispatch"], "{kind}");
    }
    // `?` stays Vim's search in a commit's view and a log.
    assert!(resolve("?", COMMIT_KIND, vim).is_empty());
    let mut k = Kalem::new("cherry-pick");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    k.git_ok(&["switch", "-q", "-c", "other"]);
    k.write("b.txt", "b\n");
    k.commit_all("Add b");
    k.write("a.txt", "theirs\n");
    k.commit_all("Theirs");
    let (_, add_b) = hashes(&mut k, "HEAD~1");
    // The branch's log, read on `other`; then main moves on.
    k.command("git.log", None);
    let log = format!("{}#log:HEAD", k.root());
    k.git_ok(&["switch", "-q", "main"]);
    k.write("a.txt", "ours\n");
    k.commit_all("Ours");
    k.command("git.refresh", None);
    // Alt+Enter on `Add b`: its entries, then the repository's commands.
    // Cherry-pick.
    k.answers.push_back(Answer::Picked(vec![2]));
    k.in_view(&log, "git.dispatch", "Add b");
    let menu = k.asked.last().unwrap().clone();
    assert!(
        menu.starts_with(&format!(
            "Git: Show {add_b} | Check out {add_b}, HEAD detached there | Cherry-pick {add_b} onto main | Revert {add_b}… | Reset main to {add_b} (soft, mixed, hard)… | Fix up {add_b} with what is staged | Show or hide its message and files | Status | Commit | "
        )),
        "{menu}"
    );
    assert_eq!(k.last_note(), format!("Cherry-picked {add_b} onto main"));
    assert_eq!(k.git_ok(&["log", "-1", "--format=%s"]).trim(), "Add b");
    assert_eq!(k.read("b.txt"), "b\n");
    // `Theirs` changes the line `Ours` changed: the cherry-pick stops, and
    // the status, shown, says so.
    k.answers.push_back(Answer::Picked(vec![2]));
    k.in_view(&log, "git.dispatch", "Theirs");
    assert!(
        k.last_note().starts_with("The cherry-pick stopped: "),
        "{}",
        k.last_note()
    );
    assert_eq!(k.notes.last().unwrap().1, Level::Warning);
    assert_eq!(k.showed.last(), Some(&k.root()));
    assert!(
        k.doc_text()
            .contains("\n Cherry-picking (1 unmerged)  (Enter: continue, skip, abort)\n"),
        "{}",
        k.doc_text()
    );
    // Another meanwhile is refused before git is asked.
    k.in_view(&log, "git.cherryPick", "Add b");
    assert!(
        k.last_note().starts_with("A cherry-pick is under way"),
        "{}",
        k.last_note()
    );
    // Aborted from the status.
    k.answers.push_back(Answer::Confirmed(true));
    k.on("git.abort", " Cherry-picking");
    assert_eq!(k.last_note(), "Aborted the cherry-pick");
    assert_eq!(k.read("a.txt"), "ours\n");
    assert_eq!(k.git_ok(&["log", "-1", "--format=%s"]).trim(), "Add b");
}

/// The rest of a commit's menu, from the status's Commits tab and from a
/// commit's view: Revert asks first; Reset offers soft, mixed and hard,
/// hard asking first and saying what is lost; Fixup needs something
/// staged; Check out detaches HEAD, git refusing when a change would be
/// lost.
#[test]
fn a_commits_menu_reverts_resets_fixes_up_and_checks_out() {
    let mut k = Kalem::new("commit-menu");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    k.write("b.txt", "b\n");
    k.commit_all("Add b");
    let (first, first_short) = hashes(&mut k, "HEAD~1");
    let (_, add_b) = hashes(&mut k, "HEAD");
    k.command("git.status", Some("a.txt"));
    k.on("git.tabCommits", "repo → main");
    // Not on a commit: said.
    k.on("git.revertCommit", "repo → main");
    assert!(
        k.last_note().starts_with("No commit here"),
        "{}",
        k.last_note()
    );
    // The status's menu on a commit lists its entries first. Revert asks,
    // and no leaves the branch as it is.
    k.answers.push_back(Answer::Picked(vec![3]));
    k.answers.push_back(Answer::Confirmed(false));
    k.on("git.dispatch", &add_b);
    let menu = &k.asked[k.asked.len() - 2];
    assert!(
        menu.starts_with(&format!(
            "Git: Show {add_b} | Check out {add_b}, HEAD detached there | Cherry-pick {add_b} onto main | Revert {add_b}… | "
        )),
        "{menu}"
    );
    assert_eq!(
        k.asked.last().unwrap(),
        &format!("Revert {add_b} (Add b)? A new commit on main undoes its changes.")
    );
    assert_eq!(k.git_ok(&["rev-list", "--count", "HEAD"]).trim(), "2");
    k.answers.push_back(Answer::Picked(vec![3]));
    k.answers.push_back(Answer::Confirmed(true));
    k.on("git.dispatch", &add_b);
    assert_eq!(k.last_note(), format!("Reverted {add_b} on main"));
    assert_eq!(
        k.git_ok(&["log", "-1", "--format=%s"]).trim(),
        "Revert \"Add b\""
    );
    assert!(!k.dir.join("b.txt").exists());
    // Fix up: with nothing staged, said; staged, a `fixup!` commit.
    k.on("git.fixup", &first_short);
    assert_eq!(
        k.last_note(),
        format!(
            "Nothing is staged to fix up {first_short} with: stage changes first (s in the status, SPC g S in a file)"
        )
    );
    k.write("a.txt", "one, fixed\n");
    k.git_ok(&["add", "a.txt"]);
    k.on("git.fixup", &first_short);
    assert_eq!(
        k.last_note(),
        format!("Made a fixup commit for {first_short}")
    );
    assert_eq!(
        k.git_ok(&["log", "-1", "--format=%s"]).trim(),
        "fixup! First"
    );
    // Reset soft: the branch at First, what came after it staged.
    k.answers.push_back(Answer::Picked(vec![0]));
    k.on("git.resetHere", &first_short);
    assert_eq!(
        k.asked.last().unwrap(),
        &format!("Reset main to {first_short} (First): Soft | Mixed | Hard…")
    );
    assert_eq!(
        k.last_note(),
        format!("Reset main to {first_short}, the files and what is staged kept")
    );
    assert_eq!(k.git_ok(&["rev-parse", "HEAD"]).trim(), first);
    assert_eq!(k.git_ok(&["diff", "--cached", "--name-only"]), "a.txt\n");
    // In First's view, its menu has no Show, and the log from it. Mixed:
    // nothing staged, the file kept.
    k.on("git.visit", &first_short);
    k.answers.push_back(Answer::Picked(vec![3]));
    k.answers.push_back(Answer::Picked(vec![1]));
    k.on_commit("git.dispatch", "commit ");
    let menu = &k.asked[k.asked.len() - 2];
    assert!(
        menu.starts_with(&format!(
            "Git: Check out {first_short}, HEAD detached there | "
        )),
        "{menu}"
    );
    assert!(
        menu.contains(&format!(" | The log from {first_short} | Status | ")),
        "{menu}"
    );
    assert_eq!(k.git_ok(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(k.read("a.txt"), "one, fixed\n");
    // Hard asks first, saying what is lost; no keeps the change.
    k.answers.push_back(Answer::Picked(vec![2]));
    k.answers.push_back(Answer::Confirmed(false));
    k.on_commit("git.resetHere", "commit ");
    assert_eq!(
        k.asked.last().unwrap(),
        &format!(
            "Reset main hard to {first_short}? Uncommitted changes are lost: the files become as {first_short} has them."
        )
    );
    assert_eq!(k.read("a.txt"), "one, fixed\n");
    k.answers.push_back(Answer::Picked(vec![2]));
    k.answers.push_back(Answer::Confirmed(true));
    k.on_commit("git.resetHere", "commit ");
    assert_eq!(k.last_note(), format!("Reset main hard to {first_short}"));
    assert_eq!(k.read("a.txt"), "one\n");
    // Check out: a change it would lose, git refuses, and it is said.
    k.write("a.txt", "two\n");
    k.commit_all("Second");
    let (second, _) = hashes(&mut k, "HEAD");
    k.write("a.txt", "mine\n");
    k.on_commit("git.checkoutCommit", "commit ");
    assert_eq!(k.notes.last().unwrap().1, Level::Error, "{:?}", k.notes);
    assert_eq!(k.git_ok(&["branch", "--show-current"]).trim(), "main");
    // Without it, HEAD is detached at First, the branch left at Second.
    k.write("a.txt", "two\n");
    k.answers.push_back(Answer::Picked(vec![0]));
    k.on_commit("git.dispatch", "commit ");
    assert_eq!(
        k.last_note(),
        format!(
            "Checked out {first_short}: HEAD is detached there (b b switches back to a branch)"
        )
    );
    assert_eq!(k.git_ok(&["rev-parse", "HEAD"]).trim(), first);
    assert_eq!(k.git_ok(&["branch", "--show-current"]).trim(), "");
    assert_eq!(k.git_ok(&["rev-parse", "main"]).trim(), second);
    assert_eq!(k.read("a.txt"), "one\n");
}

#[test]
fn context_lines_shape_the_diffs_and_none_still_stages() {
    let mut k = Kalem::new("context");
    let long: String = (1..=12).map(|i| format!("line {i}\n")).collect();
    k.write("a.txt", &long);
    k.commit_all("First");
    let changed = long
        .replace("line 3\n", "line THREE\n")
        .replace("line 9\n", "line NINE\n");
    k.write("a.txt", &changed);
    k.feed(Input::Settings(Settings {
        context_lines: 0,
        ..Settings::default()
    }));
    k.command("git.status", Some("a.txt"));
    k.on("git.toggle", " M a.txt");
    let text = k.doc_text();
    assert!(
        text.contains(
            "\n@@ -3 +3 @@ line 2\n-line 3\n+line THREE\n@@ -9 +9 @@ line 8\n-line 9\n+line NINE\n"
        ),
        "{text}"
    );
    // A hunk and a line are staged without context.
    k.on("git.stage", "+line NINE");
    assert_eq!(k.last_note(), "Staged a hunk of a.txt");
    assert!(k.index("a.txt").contains("\nline NINE\n"));
    let text = k.doc_text();
    let plus = text.find("+line THREE").unwrap();
    k.in_doc("git.stage", plus, plus + "+line THREE".len());
    assert!(
        k.index("a.txt").contains("\nline 3\nline THREE\nline 4\n"),
        "{}",
        k.index("a.txt")
    );
    // Three again: the diffs are read again, with context.
    k.feed(Input::Settings(Settings::default()));
    let text = k.doc_text();
    assert!(
        text.contains("\n line 2\n-line 3\n line THREE\n line 4\n"),
        "{text}"
    );
}

#[test]
fn the_panel_setting_takes_the_panel_away_and_back() {
    let mut k = Kalem::new("panel");
    k.write("a.txt", "one\n");
    k.commit_all("First");
    let path = k.path("a.txt");
    k.feed(Input::Opened(path.clone()));
    assert!(k.panel.is_some());
    k.feed(Input::Settings(Settings {
        panel: false,
        ..Settings::default()
    }));
    assert!(k.panel.is_none());
    // A save leaves it away; the status bar follows.
    k.write("a.txt", "two\n");
    k.feed(Input::Saved(path));
    assert!(k.panel.is_none());
    assert_eq!(k.status.as_deref(), Some("⎇ main •1"));
    k.feed(Input::Settings(Settings::default()));
    assert!(
        k.texts().iter().any(|t| t == "a.txt — modified  +1 −1"),
        "{:?}",
        k.texts()
    );
}
