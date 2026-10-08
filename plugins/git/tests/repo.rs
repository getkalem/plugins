//! The plugin's library against a real git, in repositories the tests
//! make (DESIGN.md, section 10): the refresh, the status document, the
//! actions, and patches of hunks and lines checked against what git does
//! with them. Git is isolated from the user's configuration.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kalem_plugin_git::actions::{self, Act, Settings};
use kalem_plugin_git::git::cmd::{self, CommitKind, GitCommand};
use kalem_plugin_git::git::diff::{self, FileDiff, LineKind};
use kalem_plugin_git::git::refs::Operation;
use kalem_plugin_git::git::{blame, log};
use kalem_plugin_git::model::{Folds, Repo, Section};
use kalem_plugin_git::native::{self, NativeRunner};
use kalem_plugin_git::patch::{self, Mode, Pick};
use kalem_plugin_git::refresh::{self, Options, Runner};
use kalem_plugin_git::target::{self, Target, file_key};
use kalem_plugin_git::views::{self, ViewOptions};

struct T {
    dir: PathBuf,
    git: NativeRunner,
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl T {
    fn new(name: &str) -> T {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "kalem-git-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let mut git = NativeRunner::new(&dir);
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
        let mut t = T { dir, git };
        t.ok(&["init", "-q", "-b", "main"]);
        t
    }

    fn ok(&mut self, args: &[&str]) -> Vec<u8> {
        let out = self
            .git
            .run(&GitCommand::new(args.iter().copied()))
            .unwrap();
        assert!(out.success(), "git {args:?}: {}", out.message());
        out.stdout
    }

    fn write(&self, path: &str, text: &str) {
        let p = self.dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join(path)).unwrap()
    }

    fn commit_all(&mut self, message: &str) {
        self.ok(&["add", "-A"]);
        self.ok(&["commit", "-q", "-m", message]);
    }

    fn index(&mut self, path: &str) -> String {
        String::from_utf8(self.ok(&["show", &format!(":{path}")])).unwrap()
    }

    fn repo(&mut self) -> Repo {
        let root = self.dir.display().to_string();
        refresh::load(&mut self.git, &root, 1, Options::default()).unwrap()
    }

    /// The repository with these files' diffs loaded.
    fn repo_with(&mut self, files: &[(Section, &str)]) -> (Repo, Folds) {
        let mut repo = self.repo();
        let mut folds = Folds::default();
        for (s, p) in files {
            folds.set(&file_key(*s, p), false);
        }
        refresh::load_diffs(&mut self.git, &mut repo, &folds, 3).unwrap();
        (repo, folds)
    }

    fn act(&mut self, act: Act, targets: &[Target], repo: &Repo) -> Result<String, String> {
        let plan = actions::plan(act, targets, repo, Settings::default())?;
        native::run_plan(&mut self.git, &plan.commands)?;
        Ok(plan.done)
    }
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

#[test]
fn the_status_of_a_working_repository() {
    let mut t = T::new("status");
    t.write("a.txt", &lines(5));
    t.write("dir/b c.txt", "b\n");
    t.write("old.txt", &lines(20));
    t.commit_all("First commit");
    t.write("a.txt", &lines(5).replace("line 3", "LINE 3"));
    t.write("new:file.md", "new\n");
    t.ok(&["add", "new:file.md"]);
    t.ok(&["mv", "old.txt", "renamed.txt"]);
    t.write("untracked/é.txt", "u\n");
    let (repo, folds) = t.repo_with(&[(Section::Unstaged, "a.txt")]);
    assert_eq!(repo.status.branch.as_deref(), Some("main"));
    assert_eq!(repo.head.as_ref().unwrap().subject, "First commit");
    assert!(repo.shows_recent());
    let c = views::status::render(&repo, &folds, &ViewOptions::default());
    let expected = "\
Tab fold · Enter open · s stage · u unstage · x discard · c c commit · P p push · F p pull · b b branch · ? all · q close

Head:      main   First commit

▾ Untracked files (1)
▸ untracked/

▾ Unstaged changes (1)
▾ modified     a.txt   +1 −1
@@ -1,5 +1,5 @@
 line 1
 line 2
-line 3
+LINE 3
 line 4
 line 5

▾ Staged changes (2)
▸ new file     new:file.md   +1 −0
▸ renamed      old.txt → renamed.txt   +0 −0

▾ Branches (1)
  * main

▾ Recent commits (1)
";
    assert!(c.text.starts_with(expected), "{}", c.text);
    assert!(c.text.contains("  First commit\n"), "{}", c.text);
    // The cursor on the removed line names its hunk, and the line in it.
    let at = c.text.find("-line 3").unwrap();
    assert_eq!(
        target::at(&c, at),
        Some(Target::Hunk {
            section: Section::Unstaged,
            path: "a.txt".into(),
            hunk: 0
        })
    );
    let hunk = &repo.diff(Section::Unstaged, "a.txt").unwrap().hunks[0];
    let i = target::line_in_hunk(&c, at).unwrap();
    assert_eq!(hunk.new_line_of(i), 3);
}

#[test]
fn stashes_and_ahead_of_the_upstream() {
    let mut t = T::new("upstream");
    t.write("a", "1\n");
    t.commit_all("One");
    // A bare clone as the remote; HEAD one commit ahead of it.
    let remote = t.dir.join("../").join(format!(
        "{}-remote.git",
        t.dir.file_name().unwrap().to_string_lossy()
    ));
    let remote = remote.display().to_string();
    t.ok(&["clone", "-q", "--bare", ".", &remote]);
    t.ok(&["remote", "add", "origin", &remote]);
    t.ok(&["fetch", "-q", "origin"]);
    t.ok(&["branch", "-q", "--set-upstream-to=origin/main"]);
    t.write("a", "2\n");
    t.commit_all("Two");
    t.write("a", "3\n");
    t.ok(&["stash", "push", "-q", "-m", "three"]);
    t.ok(&["tag", "v1", "HEAD~1"]);
    let repo = t.repo();
    assert_eq!(repo.status.upstream.as_deref(), Some("origin/main"));
    assert_eq!((repo.status.ahead, repo.status.behind), (1, 0));
    assert_eq!(repo.unpushed.len(), 1);
    assert_eq!(repo.stashes.len(), 1);
    assert_eq!(repo.tag, Some(("v1".into(), 1)));
    let c = views::status::render(&repo, &Folds::default(), &ViewOptions::default());
    assert!(
        c.text
            .contains("Upstream:  origin/main   ↑1\nTags:      v1 (1 commit ago)\n"),
        "{}",
        c.text
    );
    assert!(
        c.text
            .contains("▾ Stashes (1)\n  stash@{0}  On main: three\n"),
        "{}",
        c.text
    );
    assert!(
        c.text.contains("▾ Unpushed to origin/main (1)\n"),
        "{}",
        c.text
    );
    let _ = std::fs::remove_dir_all(&remote);
}

#[test]
fn staging_and_unstaging_a_hunk() {
    let mut t = T::new("hunk");
    let base = lines(40);
    t.write("f.txt", &base);
    t.commit_all("Base");
    let changed = base
        .replace("line 2\n", "line two\n")
        .replace("line 30\n", "line thirty\n");
    t.write("f.txt", &changed);
    let (repo, _) = t.repo_with(&[(Section::Unstaged, "f.txt")]);
    assert_eq!(
        repo.diff(Section::Unstaged, "f.txt").unwrap().hunks.len(),
        2
    );
    let second = Target::Hunk {
        section: Section::Unstaged,
        path: "f.txt".into(),
        hunk: 1,
    };
    assert_eq!(
        t.act(Act::Stage, &[second], &repo).unwrap(),
        "Staged a hunk of f.txt"
    );
    assert_eq!(t.index("f.txt"), base.replace("line 30\n", "line thirty\n"));
    // Unstaged again from the staged diff: the index is HEAD's again, the
    // working tree untouched.
    let (repo, _) = t.repo_with(&[(Section::Staged, "f.txt")]);
    let staged = Target::Hunk {
        section: Section::Staged,
        path: "f.txt".into(),
        hunk: 0,
    };
    t.act(Act::Unstage, &[staged], &repo).unwrap();
    assert_eq!(t.index("f.txt"), base);
    assert_eq!(t.read("f.txt"), changed);
    // Discarding the first hunk leaves the second in the working tree.
    let (repo, _) = t.repo_with(&[(Section::Unstaged, "f.txt")]);
    let first = Target::Hunk {
        section: Section::Unstaged,
        path: "f.txt".into(),
        hunk: 0,
    };
    t.act(Act::Discard, &[first], &repo).unwrap();
    assert_eq!(t.read("f.txt"), base.replace("line 30\n", "line thirty\n"));
}

#[test]
fn whole_files_new_deleted_renamed_and_untracked() {
    let mut t = T::new("files");
    t.write("keep.txt", "k\n");
    t.write("gone.txt", "g\n");
    t.write("move.txt", &lines(10));
    t.commit_all("Base");
    t.ok(&["rm", "-q", "gone.txt"]);
    t.ok(&["mv", "move.txt", "moved.txt"]);
    t.write("fresh.txt", "f\n");
    let repo = t.repo();
    let staged: Vec<&str> = repo
        .entries(Section::Staged)
        .iter()
        .map(|e| e.path.as_str())
        .collect();
    assert_eq!(staged, ["gone.txt", "moved.txt"]);
    // Unstaging the rename unstages both its paths.
    let moved = Target::File {
        section: Section::Staged,
        path: "moved.txt".into(),
    };
    t.act(Act::Unstage, &[moved], &repo).unwrap();
    let repo = t.repo();
    assert!(repo.entry(Section::Staged, "moved.txt").is_none());
    assert!(repo.entry(Section::Unstaged, "move.txt").is_some());
    // An untracked file's diff loads, as a new file.
    let (repo, _) = t.repo_with(&[(Section::Untracked, "fresh.txt")]);
    let d = repo.diff(Section::Untracked, "fresh.txt").unwrap();
    assert!(d.new_file && d.hunks.len() == 1);
    // Its only hunk stages as the whole file.
    let hunk = Target::Hunk {
        section: Section::Untracked,
        path: "fresh.txt".into(),
        hunk: 0,
    };
    t.act(Act::Stage, &[hunk], &repo).unwrap();
    assert_eq!(t.index("fresh.txt"), "f\n");
    // Deleting an untracked file.
    t.write("junk.txt", "j\n");
    let repo = t.repo();
    let junk = Target::File {
        section: Section::Untracked,
        path: "junk.txt".into(),
    };
    let plan = actions::plan(
        Act::Delete,
        std::slice::from_ref(&junk),
        &repo,
        Settings::default(),
    )
    .unwrap();
    assert!(plan.confirm.is_some());
    t.act(Act::Delete, &[junk], &repo).unwrap();
    assert!(!t.dir.join("junk.txt").exists());
}

#[test]
fn reverting_staged_changes_keeps_the_unstaged_ones() {
    let mut t = T::new("revert-staged");
    let base = lines(30);
    t.write("f", &base);
    t.commit_all("Base");
    let staged = base.replace("line 3\n", "line THREE\n");
    t.write("f", &staged);
    t.ok(&["add", "f"]);
    let both = staged.replace("line 25\n", "line 25 and more\n");
    t.write("f", &both);
    let (repo, _) = t.repo_with(&[(Section::Staged, "f")]);
    let f = Target::File {
        section: Section::Staged,
        path: "f".into(),
    };
    t.act(Act::Discard, &[f], &repo).unwrap();
    assert_eq!(t.index("f"), base);
    assert_eq!(t.read("f"), base.replace("line 25\n", "line 25 and more\n"));
}

#[test]
fn a_repository_without_a_commit() {
    let mut t = T::new("unborn");
    t.write("first.txt", "1\n");
    t.ok(&["add", "first.txt"]);
    let repo = t.repo();
    assert!(repo.unborn());
    assert!(repo.head.is_none());
    let c = views::status::render(&repo, &Folds::default(), &ViewOptions::default());
    assert!(
        c.text.contains("\n\nHead:      main   (no commit yet)\n"),
        "{}",
        c.text
    );
    t.act(Act::Unstage, &[Target::Section(Section::Staged)], &repo)
        .unwrap();
    let repo = t.repo();
    assert!(repo.entries(Section::Staged).is_empty());
    assert_eq!(repo.entries(Section::Untracked).len(), 1);
}

#[test]
fn committing_amending_and_the_log() {
    let mut t = T::new("commit");
    t.write("a", "a\n");
    t.ok(&["add", "a"]);
    let repo = t.repo();
    let plan = actions::commit(
        "First\n\nBody line.\n# a comment\n",
        &CommitKind::New,
        false,
        &repo,
    )
    .unwrap();
    native::run_plan(&mut t.git, &plan.commands).unwrap();
    let message = String::from_utf8(t.ok(&["log", "-1", "--format=%B"])).unwrap();
    assert_eq!(message.trim_end(), "First\n\nBody line.");
    // Nothing staged: refused before git is asked.
    let repo = t.repo();
    assert!(actions::commit("Second\n", &CommitKind::New, false, &repo).is_err());
    t.write("a", "b\n");
    t.ok(&["add", "a"]);
    let repo = t.repo();
    let plan = actions::commit("", &CommitKind::Extend, false, &repo).unwrap();
    native::run_plan(&mut t.git, &plan.commands).unwrap();
    let out = t
        .git
        .run(&cmd::log(&cmd::LogSpec {
            count: 10,
            ..Default::default()
        }))
        .unwrap();
    let commits = log::commits(&out.stdout).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "First");
    assert_eq!(commits[0].author, "Ayşe Test");
    assert_eq!(commits[0].refs, ["HEAD -> main"]);
    // The commit as a document: its header, then its diff.
    let out = t.git.run(&cmd::show(&commits[0].hash, 3)).unwrap();
    let d = diff::parse(&out.stdout).unwrap();
    assert_eq!(d.files.len(), 1);
    let c = views::commit::render(&d, &Folds::default(), &ViewOptions::default());
    assert!(
        c.text.starts_with(&format!("commit {}\n", commits[0].hash)),
        "{}",
        c.text
    );
    assert!(c.text.ends_with("+b\n"), "{}", c.text);
}

#[test]
fn blame_and_the_revisions_of_a_file() {
    let mut t = T::new("blame");
    t.write("f", "one\ntwo\n");
    t.commit_all("One");
    t.write("f", "one\nTWO\nthree\n");
    t.commit_all("Two");
    t.write("f", "one\nTWO\nthree\nfour\n");
    let out = t.git.run(&cmd::blame(None, "f")).unwrap();
    assert!(out.success(), "{}", out.message());
    let b = blame::parse(&out.stdout).unwrap();
    assert_eq!(b.lines.len(), 4);
    assert_eq!(b.commits[&b.lines[0].hash].summary, "One");
    assert_eq!(b.commits[&b.lines[1].hash].summary, "Two");
    assert_eq!(b.lines[3].hash, blame::UNCOMMITTED);
    let c = views::blame::render(&b, &ViewOptions::default());
    assert_eq!(c.text.lines().count(), 4);
    assert!(c.text.lines().nth(3).unwrap().starts_with("not committed"));
    let out = t.git.run(&cmd::file_revisions("f")).unwrap();
    let revs = log::parse_revisions(&out.stdout);
    assert_eq!(
        revs.iter().map(|r| r.subject.as_str()).collect::<Vec<_>>(),
        ["Two", "One"]
    );
    let out = t.git.run(&cmd::show_file(&revs[1].hash, "f")).unwrap();
    assert_eq!(out.stdout, b"one\ntwo\n");
}

#[test]
fn a_merge_in_conflict() {
    let mut t = T::new("conflict");
    t.write("f", "base\n");
    t.commit_all("Base");
    t.ok(&["switch", "-q", "-c", "other"]);
    t.write("f", "theirs\n");
    t.commit_all("Theirs");
    t.ok(&["switch", "-q", "main"]);
    t.write("f", "ours\n");
    t.commit_all("Ours");
    let out = t
        .git
        .run(&GitCommand::new(["merge", "--no-edit", "other"]))
        .unwrap();
    assert!(!out.success());
    let repo = t.repo();
    assert_eq!(repo.operation, Some(Operation::Merge));
    assert_eq!(repo.entries(Section::Unmerged).len(), 1);
    let c = views::status::render(&repo, &Folds::default(), &ViewOptions::default());
    assert!(c.text.contains("\nMerging (1 unmerged)\n"), "{}", c.text);
    assert!(c.text.contains("▸ both modified  f\n"), "{}", c.text);
    // A commit is refused while files are in conflict.
    assert!(actions::commit("Merge\n", &CommitKind::New, false, &repo).is_err());
}

#[test]
fn the_process_log_keeps_what_ran() {
    let mut t = T::new("process");
    t.write("a", "a\n");
    let _ = t.repo();
    let runs: Vec<String> = t.git.log.runs().map(|r| r.command.clone()).collect();
    assert!(
        runs.iter()
            .any(|r| r.starts_with("git status --porcelain=v2")),
        "{runs:?}"
    );
    let c = views::process::render(&t.git.log, &Folds::default(), &ViewOptions::default());
    assert!(
        c.text.contains(
            "git status --porcelain=v2 --branch --show-stash -z --untracked-files=normal"
        )
    );
}

// ---------------------------------------------------------------------
// Patches of lines, checked against git with random edits and random
// selections (DESIGN.md 3.6): what the index or the working tree holds
// after a patch is what the selection says it should.

/// A small, seeded generator: the same cases on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// `base`'s lines edited at random: lines replaced, removed and added.
fn edit(rng: &mut Rng, base: &[String], round: usize) -> Vec<String> {
    let mut out = Vec::new();
    for (i, l) in base.iter().enumerate() {
        match rng.below(12) {
            0 => {}
            1 => out.push(format!("changed {i} in {round}")),
            2 => {
                out.push(l.clone());
                out.push(format!("added after {i} in {round}"));
            }
            _ => out.push(l.clone()),
        }
    }
    if rng.chance(30) {
        out.insert(0, format!("added first in {round}"));
    }
    out
}

/// A random selection: for each hunk, all of it, some lines, or none.
fn select(rng: &mut Rng, d: &FileDiff) -> Vec<(usize, Pick)> {
    let mut picks = Vec::new();
    for (i, h) in d.hunks.iter().enumerate() {
        match rng.below(4) {
            0 => {}
            1 => picks.push((i, Pick::All)),
            _ => {
                let lines: Vec<usize> = (0..h.lines.len()).filter(|_| rng.chance(50)).collect();
                picks.push((i, Pick::Lines(lines)));
            }
        }
    }
    picks
}

fn chosen(picks: &[(usize, Pick)], hunk: usize, line: usize) -> bool {
    picks.iter().any(|(h, p)| {
        *h == hunk
            && match p {
                Pick::All => true,
                Pick::Lines(v) => v.contains(&line),
            }
    })
}

/// What a forward patch of `picks` makes of the diff's old side: removed
/// lines chosen go, added lines chosen come.
fn expected_forward(old: &[String], d: &FileDiff, picks: &[(usize, Pick)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0usize; // lines of `old` copied so far
    for (hi, h) in d.hunks.iter().enumerate() {
        // An empty side's start names the line before it.
        let start = if h.old_count == 0 {
            h.old_start as usize
        } else {
            h.old_start as usize - 1
        };
        out.extend_from_slice(&old[at..start]);
        at = start;
        for (li, l) in h.lines.iter().enumerate() {
            let text = String::from_utf8(l.text.clone()).unwrap();
            match l.kind {
                LineKind::Context => {
                    out.push(text);
                    at += 1;
                }
                LineKind::Removed => {
                    if !chosen(picks, hi, li) {
                        out.push(text);
                    }
                    at += 1;
                }
                LineKind::Added if chosen(picks, hi, li) => out.push(text),
                LineKind::Added => {}
                _ => {}
            }
        }
    }
    out.extend_from_slice(&old[at..]);
    out
}

/// What a reverse patch of `picks` makes of the diff's new side: added
/// lines chosen go, removed lines chosen come back.
fn expected_reverse(new: &[String], d: &FileDiff, picks: &[(usize, Pick)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0usize;
    for (hi, h) in d.hunks.iter().enumerate() {
        let start = if h.new_count == 0 {
            h.new_start as usize
        } else {
            h.new_start as usize - 1
        };
        out.extend_from_slice(&new[at..start]);
        at = start;
        for (li, l) in h.lines.iter().enumerate() {
            let text = String::from_utf8(l.text.clone()).unwrap();
            match l.kind {
                LineKind::Context => {
                    out.push(text);
                    at += 1;
                }
                LineKind::Added => {
                    if !chosen(picks, hi, li) {
                        out.push(text);
                    }
                    at += 1;
                }
                LineKind::Removed if chosen(picks, hi, li) => out.push(text),
                LineKind::Removed => {}
                _ => {}
            }
        }
    }
    out.extend_from_slice(&new[at..]);
    out
}

fn split(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

fn join(lines: &[String]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

fn file_diff(t: &mut T, staged: bool, context: u32) -> Option<FileDiff> {
    let out = t.git.run(&cmd::diff("f", None, staged, context)).unwrap();
    assert!(out.success(), "{}", out.message());
    diff::parse(&out.stdout).unwrap().files.into_iter().next()
}

fn apply(t: &mut T, patch: Vec<u8>, to: cmd::ApplyTo, reverse: bool, context: u32) {
    let c = cmd::apply(patch.clone(), to, reverse, context == 0);
    let out = t.git.run(&c).unwrap();
    assert!(
        out.success(),
        "git apply refused: {}\n{}",
        out.message(),
        String::from_utf8_lossy(&patch)
    );
}

#[test]
fn random_lines_stage_unstage_and_discard_as_selected() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..40 {
        let context = [3, 1, 0][round % 3];
        let mut t = T::new("lines");
        let base: Vec<String> = (0..40).map(|i| format!("base {i}")).collect();
        t.write("f", &join(&base));
        t.commit_all("Base");
        let work = edit(&mut rng, &base, round);
        t.write("f", &join(&work));
        let Some(unstaged) = file_diff(&mut t, false, context) else {
            continue;
        };
        // Stage a random selection.
        let picks = select(&mut rng, &unstaged);
        let index = match patch::build(&unstaged, &picks, Mode::Forward) {
            Ok(p) => {
                apply(&mut t, p, cmd::ApplyTo::Index, false, context);
                expected_forward(&base, &unstaged, &picks)
            }
            Err(patch::Refusal::NothingSelected) => base.clone(),
            Err(e) => panic!("round {round}: {e}"),
        };
        assert_eq!(split(&t.index("f")), index, "round {round}: staging");
        assert_eq!(
            split(&t.read("f")),
            work,
            "round {round}: the working tree moved"
        );
        // Unstage a random selection of what is staged.
        if let Some(staged) = file_diff(&mut t, true, context) {
            let picks = select(&mut rng, &staged);
            let after = match patch::build(&staged, &picks, Mode::Reverse) {
                Ok(p) => {
                    apply(&mut t, p, cmd::ApplyTo::Index, true, context);
                    expected_reverse(&index, &staged, &picks)
                }
                Err(patch::Refusal::NothingSelected) => index.clone(),
                Err(e) => panic!("round {round}: {e}"),
            };
            assert_eq!(split(&t.index("f")), after, "round {round}: unstaging");
        }
        // Discard a random selection of what is not staged.
        let index_now = split(&t.index("f"));
        if let Some(unstaged) = file_diff(&mut t, false, context) {
            let picks = select(&mut rng, &unstaged);
            let after = match patch::build(&unstaged, &picks, Mode::Reverse) {
                Ok(p) => {
                    apply(&mut t, p, cmd::ApplyTo::WorkTree, true, context);
                    expected_reverse(&work, &unstaged, &picks)
                }
                Err(patch::Refusal::NothingSelected) => work.clone(),
                Err(e) => panic!("round {round}: {e}"),
            };
            assert_eq!(split(&t.read("f")), after, "round {round}: discarding");
            assert_eq!(
                split(&t.index("f")),
                index_now,
                "round {round}: the index moved"
            );
        }
    }
}

#[test]
fn lines_of_a_file_without_a_final_newline() {
    let mut t = T::new("no-newline");
    t.write("f", "a\nb");
    t.commit_all("Base");
    t.write("f", "a\nc");
    let d = file_diff(&mut t, false, 3).unwrap();
    // Stage the removal of "b" alone: the index ends after "a".
    let p = patch::build(&d, &[(0, Pick::Lines(vec![1]))], Mode::Forward).unwrap();
    apply(&mut t, p, cmd::ApplyTo::Index, false, 3);
    assert_eq!(t.index("f"), "a\n");
    // Stage the rest: the index is the working tree.
    let d = file_diff(&mut t, false, 3).unwrap();
    let p = patch::build(&d, &[(0, Pick::All)], Mode::Forward).unwrap();
    apply(&mut t, p, cmd::ApplyTo::Index, false, 3);
    assert_eq!(t.index("f"), "a\nc");
}

#[test]
fn carriage_returns_and_latin1_survive_a_line_patch() {
    let mut t = T::new("bytes");
    std::fs::write(t.dir.join("f"), b"one\r\ntwo\r\n\xe9t\xe9\r\n").unwrap();
    t.commit_all("Base");
    std::fs::write(t.dir.join("f"), b"one\r\nTWO\r\n\xe9t\xe9!\r\n").unwrap();
    let d = file_diff(&mut t, false, 3).unwrap();
    let h = &d.hunks[0];
    // Stage the second change alone (its removed and added lines).
    let picks: Vec<usize> = h
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.text.starts_with(b"\xe9"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(picks.len(), 2);
    let p = patch::build(&d, &[(0, Pick::Lines(picks))], Mode::Forward).unwrap();
    apply(&mut t, p, cmd::ApplyTo::Index, false, 3);
    let index = t.ok(&["show", ":f"]);
    assert_eq!(index, b"one\r\ntwo\r\n\xe9t\xe9!\r\n");
}
