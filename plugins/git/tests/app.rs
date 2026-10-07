//! The plugin's state machine (phase 0) driven as Kalem drives the
//! component: its git runs run for real in repositories the tests make,
//! its questions are answered from a script, and what it shows is
//! recorded. Git is isolated from the user's configuration.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kalem_plugin_git::app::{Answer, App, Doc, Effect, Input, Level, PanelEvent, Settings};
use kalem_plugin_git::git::cmd::GitCommand;
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
            for e in self.app.handle(i) {
                match e {
                    Effect::Run {
                        token,
                        cwd,
                        command,
                    } => {
                        self.git.cwd = cwd.map_or_else(|| self.dir.clone(), PathBuf::from);
                        let result = self.git.run(&command);
                        queue.push_back(Input::Done { token, result });
                    }
                    Effect::Confirm { token, message } => {
                        self.asked.push(message);
                        let answer = self.answers.pop_front().expect("an answer to a confirm");
                        queue.push_back(Input::Answer { token, answer });
                    }
                    Effect::Prompt { token, title } => {
                        self.asked.push(title);
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
                    Effect::ShowPanel => self.shown = true,
                    Effect::RunCommand { id, args } => self.commands.push((id, args)),
                    Effect::SetSetting { key, value } => self.set.push((key, value)),
                }
            }
        }
        assert!(self.answers.is_empty(), "answers left: {:?}", self.answers);
    }

    fn command(&mut self, id: &str, file: Option<&str>) {
        let doc = file.map(|f| Doc {
            path: Some(self.path(f)),
            line: 1,
            text: None,
        });
        self.feed(Input::Command { id: id.into(), doc });
    }

    fn texts(&self) -> Vec<String> {
        self.panel.as_ref().map(Node::texts).unwrap_or_default()
    }

    fn last_note(&self) -> &str {
        self.notes.last().map_or("", |n| n.0.as_str())
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
    // SPC g g shows the panel.
    k.command("git.status", Some("a.txt"));
    assert!(k.shown);
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
            text: None,
        }),
    });
    assert_eq!(
        k.notes.last().unwrap(),
        &("Not in a git repository".to_string(), Level::Error)
    );
}
