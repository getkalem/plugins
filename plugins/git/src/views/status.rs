//! The status document (DESIGN.md, 2.1 and appendix D): lazygit's panels
//! as tabs of one document (Files, Local branches, Commits, Stash) under a
//! header of the repository's status, in lazygit's colors; and magit's
//! diffs: Tab on a file shows its hunks under it.
//!
//! A hunk's lines start at the first column as `git diff` prints them, so
//! that Kalem's `diff` highlighter colors them; every other line starts
//! with a space, a code or a mark, so the highlighter leaves it alone.

use std::collections::BTreeMap;

use crate::content::{Color, Content, Style};
use crate::git::diff::{FileDiff, LineKind};
use crate::git::log::Commit;
use crate::git::refs::Branch;
use crate::git::status::{Entry, Kind, State};
use crate::model::{Folds, Repo, Section};
use crate::target::{hunk_key, path_key};
use crate::views::{Glyphs, ViewOptions, lossy_line};

/// A tab of the status, as lazygit's panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// The changed files.
    #[default]
    Files,
    /// The local branches.
    Branches,
    /// The last commits.
    Commits,
    /// The stashes.
    Stash,
}

impl Tab {
    /// Every tab, in order: `1` to `4` show them.
    pub const ALL: [Tab; 4] = [Tab::Files, Tab::Branches, Tab::Commits, Tab::Stash];

    /// Its name in region keys and commands.
    pub fn key(self) -> &'static str {
        match self {
            Tab::Files => "files",
            Tab::Branches => "branches",
            Tab::Commits => "commits",
            Tab::Stash => "stash",
        }
    }

    /// The tab named `key`.
    pub fn from_key(key: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.key() == key)
    }

    /// Its title.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Files => "Files",
            Tab::Branches => "Local branches",
            Tab::Commits => "Commits",
            Tab::Stash => "Stash",
        }
    }

    /// The next tab, or the previous one, round.
    pub fn step(self, next: bool) -> Tab {
        let i = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0);
        let n = Tab::ALL.len();
        Tab::ALL[if next { (i + 1) % n } else { (i + n - 1) % n }]
    }

    /// The keys the first line names in it.
    fn keys(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Tab::Files => &[
                ("Tab", "diff"),
                ("Enter", "open"),
                ("s", "stage"),
                ("u", "unstage"),
                ("a", "all"),
                ("d", "discard"),
                ("c c", "commit"),
                ("P p", "push"),
                ("p", "pull"),
            ],
            Tab::Branches => &[
                ("Enter", "switch"),
                ("b c", "new branch"),
                ("P p", "push"),
                ("p", "pull"),
            ],
            Tab::Commits => &[
                ("Enter", "details"),
                ("c a", "amend"),
                ("c w", "reword"),
                ("P p", "push"),
            ],
            Tab::Stash => &[("Enter", "apply, pop, drop"), ("Z z", "stash")],
        }
    }
}

/// The keys every tab names after its own.
pub const COMMON_KEYS: &[(&str, &str)] = &[("1-4 [ ]", "tabs"), ("?", "menu"), ("q", "close")];

/// The first line: tab `tab`'s keys, then the common ones.
pub fn help_line(glyphs: Glyphs, tab: Tab) -> String {
    let sep = match glyphs {
        Glyphs::Unicode => " · ",
        Glyphs::Ascii => " | ",
    };
    tab.keys()
        .iter()
        .chain(COMMON_KEYS)
        .map(|(k, w)| format!("{k} {w}"))
        .collect::<Vec<_>>()
        .join(sep)
}

/// The local branches listed at most; `b b` offers them all.
pub const BRANCHES_SHOWN: usize = 50;

/// The width of the rule under the tabs.
pub const WIDTH: usize = 60;

const RED: Style = Style::Color(Color::Red, false);
const GREEN: Style = Style::Color(Color::Green, false);
const YELLOW: Style = Style::Color(Color::Yellow, false);
const MAGENTA: Style = Style::Color(Color::Magenta, false);
const CYAN: Style = Style::Color(Color::Cyan, false);

/// The status of `repo` with tab `tab` shown, as the folds have it.
pub fn render(repo: &Repo, folds: &Folds, opts: &ViewOptions, tab: Tab) -> Content {
    let mut c = Content::new();
    c.open("help", false, false);
    c.line(&help_line(opts.glyphs, tab), Style::Muted);
    c.close();
    c.newline();
    status_lines(&mut c, repo, opts);
    c.newline();
    tabs(&mut c, repo, opts, tab);
    c.open("content", false, false);
    match tab {
        Tab::Files => files(&mut c, repo, folds, opts),
        Tab::Branches => branches(&mut c, repo, opts),
        Tab::Commits => commits(&mut c, repo, opts),
        Tab::Stash => stashes(&mut c, repo),
    }
    c.close();
    c
}

/// How many things tab `tab` lists.
fn count(repo: &Repo, tab: Tab) -> usize {
    match tab {
        Tab::Files => repo.changed().len(),
        Tab::Branches => repo.local_branches().len(),
        Tab::Commits => repo.recent.len(),
        Tab::Stash => repo.stashes.len(),
    }
}

/// The tabs' line, the one shown green, and a rule under it.
fn tabs(c: &mut Content, repo: &Repo, opts: &ViewOptions, shown: Tab) {
    let (sep, bar) = match opts.glyphs {
        Glyphs::Unicode => (" │ ", "─"),
        Glyphs::Ascii => (" | ", "-"),
    };
    c.push(" ", Style::Normal);
    for (i, t) in Tab::ALL.into_iter().enumerate() {
        if i > 0 {
            c.push(sep, Style::Muted);
        }
        c.open(format!("tab:{}", t.key()), false, false);
        c.push(&format!("{} ", i + 1), Style::Muted);
        if t == shown {
            c.push(t.title(), Style::Color(Color::Green, true));
        } else {
            c.push(t.title(), Style::Normal);
        }
        c.push(&format!(" {}", count(repo, t)), Style::Muted);
        c.close();
    }
    c.newline();
    c.line(&bar.repeat(WIDTH), Style::Muted);
}

/// The repository and its branch, how far from its upstream, the
/// nearest tag and the operation under way.
fn status_lines(c: &mut Content, repo: &Repo, opts: &ViewOptions) {
    let s = &repo.status;
    c.open("head", false, false);
    c.push(" ", Style::Normal);
    match (&s.upstream, s.ahead, s.behind) {
        (Some(_), 0, 0) => {
            let ok = match opts.glyphs {
                Glyphs::Unicode => "✓",
                Glyphs::Ascii => "=",
            };
            c.push(ok, GREEN);
            c.push(" ", Style::Normal);
        }
        (Some(_), a, b) => {
            c.push(&opts.glyphs.ahead_behind(a, b), YELLOW);
            c.push(" ", Style::Normal);
        }
        (None, _, _) => {}
    }
    c.push(&repo_name(&repo.root), Style::Normal);
    c.push(&format!(" {} ", opts.glyphs.arrow()), Style::Muted);
    match (&s.branch, &repo.head) {
        (Some(b), _) => c.push(b, Style::Strong),
        (None, Some(h)) => {
            c.push("(detached) ", RED);
            c.push(&h.short, Style::Code);
        }
        (None, None) => c.push("(detached)", RED),
    }
    match &repo.head {
        Some(h) => {
            c.push("  ", Style::Normal);
            c.line(&h.subject, Style::Normal);
        }
        None if s.unborn() => c.line("  (no commit yet)", Style::Muted),
        None => c.newline(),
    }
    c.close();
    if let Some(u) = &s.upstream {
        c.open("upstream", false, false);
        c.push(" ", Style::Normal);
        c.push(u, Style::Muted);
        c.line("  (Enter: push, pull, fetch)", Style::Muted);
        c.close();
    }
    if let Some((tag, n)) = &repo.tag {
        c.open("tag", false, false);
        c.push(" ", Style::Normal);
        c.push(tag, Style::Color(Color::Magenta, true));
        let since = match n {
            0 => " (at HEAD)".to_string(),
            1 => " (1 commit ago)".to_string(),
            n => format!(" ({n} commits ago)"),
        };
        c.line(&since, Style::Muted);
        c.close();
    }
    if let Some(op) = repo.operation {
        c.open("operation", false, false);
        let unmerged = repo.entries(Section::Unmerged).len();
        c.push(" ", Style::Normal);
        c.push(op.word(), RED);
        if unmerged > 0 {
            c.line(&format!(" ({unmerged} unmerged)"), Style::Muted);
        } else {
            c.newline();
        }
        c.close();
    }
}

/// The last part of a repository's root: its name.
pub fn repo_name(root: &str) -> String {
    root.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(root)
        .to_string()
}

/// A folder of the Files tab's tree: its folders and its files.
#[derive(Default)]
struct Folder<'a> {
    folders: BTreeMap<String, Folder<'a>>,
    files: Vec<&'a Entry>,
}

impl<'a> Folder<'a> {
    fn add(&mut self, parts: &[&str], e: &'a Entry) {
        match parts {
            [] | [_] => self.files.push(e),
            [dir, rest @ ..] => self
                .folders
                .entry((*dir).to_string())
                .or_default()
                .add(rest, e),
        }
    }

    /// Whether something in it is not staged: lazygit shows it red.
    fn unstaged(&self) -> bool {
        self.files.iter().any(|e| not_staged(e)) || self.folders.values().any(Folder::unstaged)
    }
}

/// A path with changes not staged, untracked or in conflict.
fn not_staged(e: &Entry) -> bool {
    e.unstaged() || matches!(e.kind, Kind::Untracked | Kind::Unmerged(_))
}

/// The changed paths as a tree of folders, as lazygit shows them: a
/// folder holding one folder and no file is written with it
/// (`crates/kalem-core/src`).
fn files(c: &mut Content, repo: &Repo, folds: &Folds, opts: &ViewOptions) {
    let changed = repo.changed();
    if changed.is_empty() {
        c.line(" Nothing changed: the working tree is clean", Style::Muted);
        return;
    }
    let mut root = Folder::default();
    for e in &changed {
        let parts: Vec<&str> = e.path.trim_end_matches('/').split('/').collect();
        root.add(&parts, e);
    }
    folder(c, repo, folds, opts, &root, "", 0);
}

fn folder(
    c: &mut Content,
    repo: &Repo,
    folds: &Folds,
    opts: &ViewOptions,
    f: &Folder<'_>,
    prefix: &str,
    depth: usize,
) {
    for (name, sub) in &f.folders {
        // A chain of folders each holding one folder: one line.
        let mut name = name.clone();
        let mut sub = sub;
        while sub.files.is_empty() && sub.folders.len() == 1 {
            let (n, s) = sub.folders.iter().next().expect("one folder");
            name = format!("{name}/{n}");
            sub = s;
        }
        let path = format!("{prefix}{name}");
        let key = format!("dir:{path}");
        let folded = folds.folded(&key);
        c.open(&key, true, folded);
        c.push(&" ".repeat(1 + depth * 2), Style::Normal);
        let mark = match (opts.glyphs, folded) {
            (Glyphs::Unicode, false) => "▼ ",
            (Glyphs::Unicode, true) => "▶ ",
            (Glyphs::Ascii, false) => "v ",
            (Glyphs::Ascii, true) => "> ",
        };
        c.push(mark, Style::Muted);
        c.line(&name, if sub.unstaged() { RED } else { GREEN });
        if !folded {
            folder(c, repo, folds, opts, sub, &format!("{path}/"), depth + 1);
        }
        c.close();
    }
    for e in &f.files {
        file(c, repo, folds, opts, e, depth);
    }
}

fn file(c: &mut Content, repo: &Repo, folds: &Folds, opts: &ViewOptions, e: &Entry, depth: usize) {
    let key = path_key(&e.path);
    let folded = folds.folded(&key);
    c.open(&key, true, folded);
    c.push(&" ".repeat(1 + depth * 2), Style::Normal);
    // lazygit's colors: what is staged green, what is not red.
    let code = e.code();
    match e.kind {
        Kind::Tracked => {
            let mut letters = code.chars();
            let (x, y) = (
                letters.next().unwrap_or(' ').to_string(),
                letters.next().unwrap_or(' ').to_string(),
            );
            c.push(
                &x,
                if e.index == State::Unmodified {
                    Style::Normal
                } else {
                    GREEN
                },
            );
            c.push(
                &y,
                if e.worktree == State::Unmodified {
                    Style::Normal
                } else {
                    RED
                },
            );
        }
        _ => c.push(&code, RED),
    }
    c.push(" ", Style::Normal);
    let name_style = if not_staged(e) { RED } else { GREEN };
    let name = e
        .path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(&e.path);
    let name = if e.path.ends_with('/') {
        format!("{name}/")
    } else {
        name.to_string()
    };
    match &e.from {
        Some(from) => {
            c.push(from, name_style);
            c.push(&format!(" {} ", opts.glyphs.arrow()), Style::Muted);
            c.push(&name, name_style);
        }
        None => c.push(&name, name_style),
    }
    let (mut added, mut removed, mut any) = (0, 0, false);
    for s in [Section::Unstaged, Section::Staged] {
        if let Some((a, r)) = repo.counts.get(&(s, e.path.clone())) {
            added += a;
            removed += r;
            any = true;
        }
    }
    if any {
        c.push(
            &format!("   {}", opts.glyphs.counts(added, removed)),
            Style::Muted,
        );
    }
    c.newline();
    if !folded {
        let sections = repo.sections_of(&e.path);
        for s in &sections {
            if sections.len() > 1 {
                let what = if *s == Section::Staged {
                    "staged"
                } else {
                    "unstaged"
                };
                c.line(&format!("{}  {what}:", " ".repeat(depth * 2)), Style::Muted);
            }
            match repo.diff(*s, &e.path) {
                None => c.line("  Loading…", Style::Muted),
                Some(d) => diff_lines(c, folds, opts, *s, &e.path, d),
            }
        }
    }
    c.close();
}

fn diff_lines(
    c: &mut Content,
    folds: &Folds,
    opts: &ViewOptions,
    section: Section,
    path: &str,
    d: &FileDiff,
) {
    let lines: usize = d.hunks.iter().map(|h| h.lines.len()).sum();
    if d.binary {
        c.line("  Binary file changed", Style::Muted);
        return;
    }
    if lines > opts.big_diff {
        c.open(format!("big:{}:{path}", section.key()), false, false);
        c.line(
            &format!("  {} changed lines: too many to show here", group(lines)),
            Style::Muted,
        );
        c.close();
        return;
    }
    if d.hunks.is_empty() {
        let what = if d.mode_changed {
            "  Its mode changed"
        } else if d.renamed {
            "  Renamed, its content unchanged"
        } else {
            "  No change to show"
        };
        c.line(what, Style::Muted);
        return;
    }
    for (i, h) in d.hunks.iter().enumerate() {
        let key = hunk_key(section, i, path);
        let folded = folds.folded(&key);
        c.open(&key, true, folded);
        c.line(&lossy_line(&h.shown_header()), Style::Normal);
        if !folded {
            for l in &h.lines {
                let style = if l.kind == LineKind::NoNewline {
                    Style::Muted
                } else {
                    Style::Normal
                };
                c.line(&lossy_line(&l.raw()), style);
            }
        }
        c.close();
    }
}

/// The local branches, the current one first and marked, how far each is
/// from its upstream.
fn branches(c: &mut Content, repo: &Repo, opts: &ViewOptions) {
    let mut local = repo.local_branches();
    if local.is_empty() {
        c.line(" No branch yet", Style::Muted);
        return;
    }
    // The current one first, as lazygit lists them.
    local.sort_by_key(|b| !b.current);
    for b in local.iter().take(BRANCHES_SHOWN) {
        branch_line(c, b, opts);
    }
    if local.len() > BRANCHES_SHOWN {
        c.line(
            &format!(
                "   … and {} more: b b offers them all",
                local.len() - BRANCHES_SHOWN
            ),
            Style::Muted,
        );
    }
}

fn branch_line(c: &mut Content, b: &Branch, opts: &ViewOptions) {
    c.open(format!("branch:{}", b.name), false, false);
    if b.current {
        c.push(" * ", GREEN);
        c.push(&b.name, Style::Color(Color::Green, true));
    } else {
        c.push("   ", Style::Normal);
        c.push(&b.name, Style::Normal);
    }
    if b.upstream.is_some() {
        if b.gone {
            c.push(" (upstream gone)", RED);
        } else if b.ahead > 0 || b.behind > 0 {
            c.push(" ", Style::Normal);
            c.push(&opts.glyphs.ahead_behind(b.ahead, b.behind), YELLOW);
        } else if opts.glyphs == Glyphs::Unicode {
            c.push(" ✓", GREEN);
        }
    }
    c.newline();
    c.close();
}

/// The last commits, HEAD's first: hash (red when not pushed), author's
/// initials, tags, subject.
fn commits(c: &mut Content, repo: &Repo, opts: &ViewOptions) {
    if repo.recent.is_empty() {
        c.line(" No commit yet", Style::Muted);
    }
    for commit in &repo.recent {
        let unpushed = repo.unpushed.iter().any(|u| u.hash == commit.hash);
        commit_line(c, commit, unpushed, opts);
    }
    if !repo.unpulled.is_empty() {
        c.line(
            &format!(
                " {} to pull from {}: p",
                repo.unpulled.len(),
                repo.status.upstream.as_deref().unwrap_or("the upstream")
            ),
            Style::Muted,
        );
    }
}

/// A commit's line: its short hash, its author's initials, its tags and
/// its subject.
pub fn commit_line(c: &mut Content, commit: &Commit, unpushed: bool, opts: &ViewOptions) {
    c.open(format!("commit:{}", commit.hash), false, false);
    let mark = match (unpushed, opts.glyphs) {
        (false, _) => " ",
        (true, Glyphs::Unicode) => "↑",
        (true, Glyphs::Ascii) => "^",
    };
    c.push(&format!(" {mark}"), RED);
    c.push(&commit.short, if unpushed { RED } else { GREEN });
    c.push(" ", Style::Normal);
    c.push(&initials(&commit.author), MAGENTA);
    let tags: Vec<&str> = commit
        .refs
        .iter()
        .filter_map(|r| r.strip_prefix("tag: "))
        .collect();
    if !tags.is_empty() {
        c.push(" ", Style::Normal);
        c.push(&tags.join(" "), Style::Color(Color::Magenta, true));
    }
    c.push(" ", Style::Normal);
    c.line(&commit.subject, Style::Normal);
    c.close();
}

/// An author's initials, as lazygit shows them: `Ayşe Test` is `AT`.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let first = |w: &str| w.chars().next().map(|c| c.to_uppercase().to_string());
    match words.as_slice() {
        [] => "??".into(),
        [one] => one.chars().take(2).collect::<String>(),
        [a, .., b] => format!(
            "{}{}",
            first(a).unwrap_or_default(),
            first(b).unwrap_or_default()
        ),
    }
}

/// The stashes.
fn stashes(c: &mut Content, repo: &Repo) {
    if repo.stashes.is_empty() {
        c.line(" No stash: Z z stashes the changes", Style::Muted);
    }
    for s in &repo.stashes {
        c.open(format!("stash:{}", s.index), false, false);
        c.push(" ", Style::Normal);
        c.push(&s.name(), CYAN);
        c.push(": ", Style::Muted);
        c.line(&s.message, Style::Normal);
        c.close();
    }
}

/// `12345` as `12,345`.
pub fn group(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{diff, status};
    use crate::target::{self, Target};

    fn repo() -> Repo {
        let z = |parts: &[&str]| -> Vec<u8> {
            parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
        };
        let s = status::parse(&z(&[
            "# branch.oid abc",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +1 -0",
            "1 .M N... 100644 100644 100644 a a crates/core/src/lib.rs",
            "1 MM N... 100644 100644 100644 a b README.md",
            "1 A. N... 000000 100644 100644 0 b new.md",
            "? notes.txt",
        ]))
        .unwrap();
        let head = Commit {
            hash: "abc".into(),
            short: "abc1234".into(),
            author: "Ayşe Test".into(),
            subject: "Fix it".into(),
            refs: vec!["HEAD -> main".into(), "tag: v0.1".into()],
            ..Commit::default()
        };
        let mut r = Repo {
            root: "/home/a/org".into(),
            status: s,
            head: Some(head.clone()),
            ..Repo::default()
        };
        r.recent = vec![
            head.clone(),
            Commit {
                hash: "def".into(),
                short: "def5678".into(),
                author: "Ayşe Test".into(),
                subject: "First".into(),
                ..Commit::default()
            },
        ];
        r.unpushed = vec![head];
        r.counts
            .insert((Section::Unstaged, "crates/core/src/lib.rs".into()), (1, 1));
        let d = |p: &str, from: &str, to: &str| {
            diff::parse(
                format!(
                    "diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1,2 +1,2 @@\n a\n-{from}\n+{to}\n"
                )
                .as_bytes(),
            )
            .unwrap()
            .files
            .remove(0)
        };
        r.diffs.insert(
            (Section::Unstaged, "README.md".into()),
            d("README.md", "b", "B"),
        );
        r.diffs.insert(
            (Section::Staged, "README.md".into()),
            d("README.md", "x", "X"),
        );
        r.branches = crate::git::refs::parse_branches(
            b"refs/heads/feature\0feature\0\0\0 \nrefs/heads/main\0main\0origin/main\0[ahead 1]\0*\n",
        );
        r
    }

    #[test]
    fn lazygits_tabs_and_magits_diff() {
        let mut folds = Folds::default();
        folds.set(&path_key("README.md"), false);
        let c = render(&repo(), &folds, &ViewOptions::default(), Tab::Files);
        let expected = "\
Tab diff · Enter open · s stage · u unstage · a all · d discard · c c commit · P p push · p pull · 1-4 [ ] tabs · ? menu · q close

 ↑1 org → main  Fix it
 origin/main  (Enter: push, pull, fetch)

 1 Files 4 │ 2 Local branches 2 │ 3 Commits 2 │ 4 Stash 0
────────────────────────────────────────────────────────────
 ▼ crates/core/src
    M lib.rs   +1 −1
 MM README.md
  unstaged:
@@ -1,2 +1,2 @@
 a
-b
+B
  staged:
@@ -1,2 +1,2 @@
 a
-x
+X
 A  new.md
 ?? notes.txt
";
        assert_eq!(c.text, expected);
        // The cursor on a line names what it stands for.
        let at = |s: &str| target::at(&c, c.text.find(s).unwrap());
        assert_eq!(at("MM README"), Some(Target::Path("README.md".into())));
        assert_eq!(
            at("lib.rs"),
            Some(Target::Path("crates/core/src/lib.rs".into()))
        );
        assert_eq!(at("▼ crates"), Some(Target::Dir("crates/core/src".into())));
        assert_eq!(
            at("-x"),
            Some(Target::Hunk {
                section: Section::Staged,
                path: "README.md".into(),
                hunk: 0
            })
        );
        assert_eq!(at("Commits 2"), Some(Target::Tab("commits".into())));
        // lazygit's colors: not staged red, staged green, the tab shown
        // green and bold.
        let style = |s: &str| c.style_at(c.text.find(s).unwrap());
        assert_eq!(style("lib.rs"), RED);
        assert_eq!(style("new.md"), GREEN);
        assert_eq!(style("Files 4"), Style::Color(Color::Green, true));
        assert_eq!(style("Commits 2"), Style::Normal);
    }

    #[test]
    fn the_other_tabs() {
        let folds = Folds::default();
        let opts = ViewOptions::default();
        let tab = |t| render(&repo(), &folds, &opts, t).text;
        let branches = tab(Tab::Branches);
        assert!(
            branches.ends_with("\n * main ↑1\n   feature\n"),
            "{branches}"
        );
        assert!(
            branches.starts_with("Enter switch · b c new branch"),
            "{branches}"
        );
        let commits = tab(Tab::Commits);
        assert!(
            commits.ends_with("\n ↑abc1234 AT v0.1 Fix it\n  def5678 AT First\n"),
            "{commits}"
        );
        let c = render(&repo(), &folds, &opts, Tab::Commits);
        let style = |s: &str| c.style_at(c.text.find(s).unwrap());
        assert_eq!(style("abc1234"), RED, "not pushed");
        assert_eq!(style("def5678"), GREEN);
        assert_eq!(style("v0.1"), Style::Color(Color::Magenta, true));
        let stash = tab(Tab::Stash);
        assert!(
            stash.ends_with(" No stash: Z z stashes the changes\n"),
            "{stash}"
        );
        // Round the tabs.
        assert_eq!(Tab::Stash.step(true), Tab::Files);
        assert_eq!(Tab::Files.step(false), Tab::Stash);
    }

    #[test]
    fn folded_folders_and_ascii() {
        let mut folds = Folds::default();
        folds.set("dir:crates/core/src", true);
        let opts = ViewOptions {
            glyphs: Glyphs::Ascii,
            ..ViewOptions::default()
        };
        let c = render(&repo(), &folds, &opts, Tab::Files);
        assert!(
            c.text.contains("\n > crates/core/src\n MM README.md\n"),
            "{}",
            c.text
        );
        assert!(
            c.text.contains("\n +1 ahead org -> main  Fix it\n"),
            "{}",
            c.text
        );
        assert!(
            c.text.contains(" 1 Files 4 | 2 Local branches 2 |"),
            "{}",
            c.text
        );
    }

    #[test]
    fn initials_and_large_numbers() {
        assert_eq!(initials("Ayşe Test"), "AT");
        assert_eq!(initials("Mehmet Ali Sekercioglu"), "MS");
        assert_eq!(initials("bot"), "bo");
        assert_eq!(group(12), "12");
        assert_eq!(group(1234567), "1,234,567");
    }
}
