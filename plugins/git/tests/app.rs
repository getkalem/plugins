//! The plugin's state machine (phase 0) driven as Kalem drives the
//! component: its git runs run for real in repositories the tests make,
//! its questions are answered from a script, and what it shows is
//! recorded. Git is isolated from the user's configuration.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kalem_plugin_git::app::{Answer, App, Doc, Effect, Input, Level, PanelEvent, Settings};
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
    /// The status documents: their text and cursor, by repository.
    docs: BTreeMap<String, (String, usize)>,
    /// The repositories whose status was shown, in order.
    showed: Vec<String>,
    /// The texts the prompts offered.
    offered: Vec<Option<String>>,
    /// The marks beside the lines, by file.
    gutters: BTreeMap<String, Vec<(u32, Mark)>>,
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
                    Effect::ShowPanel => self.shown = true,
                    Effect::RunCommand { id, args } => self.commands.push((id, args)),
                    Effect::SetSetting { key, value } => self.set.push((key, value)),
                    Effect::Document {
                        root,
                        text,
                        cursor,
                        show,
                        ..
                    } => {
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
                    Effect::CloseDocument { root } => {
                        self.docs.remove(&root);
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
        "\n─ Status ─",
        "\n repo → main  First\n",
        "\n─ Files ─",
        "\n  M a.txt   +2 −2\n ?? new.txt\n",
        "\n─ Local branches ─",
        "\n * main\n",
        "\n─ Commits ─",
        " First\n",
        "\n─ Stash ─",
    ] {
        assert!(text.contains(part), "{part:?} in\n{text}");
    }

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
    k.on("git.stageEverything", "─ Files");
    assert_eq!(k.last_note(), "Staged everything");
    assert!(k.doc_text().contains("\n M  a.txt"), "{}", k.doc_text());
    assert!(k.doc_text().contains("\n A  new.txt"), "{}", k.doc_text());
    k.on("git.stageEverything", "─ Files");
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
    k.on("git.refresh", "─ Files");
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

#[test]
fn branches_stashes_and_amends_from_the_status() {
    let mut k = Kalem::new("branches");
    k.write("a.txt", "one\n");
    k.commit_all("First\n\nThe body stays.");
    k.git_ok(&["branch", "feature"]);
    k.command("git.status", Some("a.txt"));
    assert!(
        k.doc_text().contains("─ Local branches ─"),
        "{}",
        k.doc_text()
    );

    // Enter on a branch switches to it.
    k.on("git.visit", "   feature");
    assert_eq!(k.last_note(), "Switched to feature");
    assert!(k.doc_text().contains(" repo → feature"), "{}", k.doc_text());
    assert!(k.doc_text().contains("\n * feature"), "{}", k.doc_text());

    // `b c` makes one; `b b` offers the others.
    k.answers.push_back(Answer::Text(Some("topic".into())));
    k.on("git.newBranch", "repo → ");
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
