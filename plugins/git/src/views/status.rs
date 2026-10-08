//! The status document (DESIGN.md, 2.1 and appendix D): lazygit's panels
//! in one document, one under the other (Status, Files, Local branches,
//! Commits, Stash), each under a title line with its count, and magit's
//! diffs: Tab on a file shows its hunks under it.
//!
//! A hunk's lines start at the first column as `git diff` prints them, so
//! that Kalem's `diff` highlighter colors them; every other line starts
//! with a space, a code or a mark, so the highlighter leaves it alone.

use std::collections::BTreeMap;

use crate::content::{Content, Style};
use crate::git::diff::{FileDiff, LineKind};
use crate::git::log::Commit;
use crate::git::refs::Branch;
use crate::git::status::{Entry, Kind};
use crate::model::{Folds, Repo, Section};
use crate::target::{hunk_key, path_key};
use crate::views::{Glyphs, ViewOptions, lossy_line};

/// The first line's keys, so that nothing has to be known beforehand.
pub const HELP: &[(&str, &str)] = &[
    ("Tab", "diff"),
    ("Enter", "open"),
    ("s", "stage"),
    ("u", "unstage"),
    ("a", "all"),
    ("d", "discard"),
    ("c c", "commit"),
    ("P p", "push"),
    ("p", "pull"),
    ("b b", "branch"),
    ("?", "menu"),
    ("q", "close"),
];

/// The first line: [`HELP`]'s keys.
pub fn help_line(glyphs: Glyphs) -> String {
    let sep = match glyphs {
        Glyphs::Unicode => " · ",
        Glyphs::Ascii => " | ",
    };
    HELP.iter()
        .map(|(k, w)| format!("{k} {w}"))
        .collect::<Vec<_>>()
        .join(sep)
}

/// The local branches listed at most; `b b` offers them all.
pub const BRANCHES_SHOWN: usize = 10;

/// The width of a panel's title line.
pub const WIDTH: usize = 60;

/// The status of `repo` as the folds have it.
pub fn render(repo: &Repo, folds: &Folds, opts: &ViewOptions) -> Content {
    let mut c = Content::new();
    c.open("help", false, false);
    c.line(&help_line(opts.glyphs), Style::Muted);
    c.close();
    c.newline();
    status_panel(&mut c, repo, opts);
    files_panel(&mut c, repo, folds, opts);
    branches_panel(&mut c, repo, folds, opts);
    commits_panel(&mut c, repo, folds, opts);
    stash_panel(&mut c, repo, folds, opts);
    // The last blank line goes: the document ends with its last panel.
    if c.text.ends_with("\n\n") {
        c.text.pop();
    }
    c
}

/// A panel's title line: `─ Files ───────── 3 ─`, `…` after the title
/// when the panel is folded; no count for none.
fn title(c: &mut Content, opts: &ViewOptions, name: &str, count: Option<usize>, folded: bool) {
    let (bar, more) = match opts.glyphs {
        Glyphs::Unicode => ("─", " …"),
        Glyphs::Ascii => ("-", " ..."),
    };
    let name = if folded {
        format!("{name}{more}")
    } else {
        name.to_string()
    };
    c.push(&format!("{bar} "), Style::Muted);
    c.push(&name, Style::Heading);
    let count = count.map(|n| format!(" {n} {bar}")).unwrap_or_default();
    // `─ NAME ` and ` COUNT ─` around the rule.
    let used = 2 + name.chars().count() + 1 + count.chars().count();
    let fill = WIDTH.saturating_sub(used).max(3);
    c.push(&format!(" {}", bar.repeat(fill)), Style::Muted);
    c.line(&count, Style::Muted);
}

/// Opens panel `section` with its title; whether its lines follow.
fn panel(
    c: &mut Content,
    folds: &Folds,
    opts: &ViewOptions,
    section: Section,
    name: &str,
    count: usize,
) -> bool {
    let key = format!("section:{}", section.key());
    let folded = folds.folded(&key);
    c.open(&key, true, folded);
    title(c, opts, name, Some(count), folded);
    !folded
}

fn end_panel(c: &mut Content) {
    c.close();
    c.newline();
}

/// The repository's name and its branch, how far from its upstream, the
/// nearest tag and the operation under way.
fn status_panel(c: &mut Content, repo: &Repo, opts: &ViewOptions) {
    let s = &repo.status;
    c.open("section:status", false, false);
    title(c, opts, "Status", None, false);
    c.open("head", false, false);
    let sync = match (&s.upstream, s.ahead, s.behind) {
        (Some(_), 0, 0) => match opts.glyphs {
            Glyphs::Unicode => "✓".to_string(),
            Glyphs::Ascii => "=".to_string(),
        },
        (Some(_), a, b) => opts.glyphs.ahead_behind(a, b),
        (None, _, _) => String::new(),
    };
    c.push(" ", Style::Normal);
    if !sync.is_empty() {
        c.push(&sync, Style::Strong);
        c.push(" ", Style::Normal);
    }
    c.push(&repo_name(&repo.root), Style::Normal);
    c.push(&format!(" {} ", opts.glyphs.arrow()), Style::Muted);
    match (&s.branch, &repo.head) {
        (Some(b), _) => c.push(b, Style::Strong),
        (None, Some(h)) => {
            c.push("(detached) ", Style::Error);
            c.push(&h.short, Style::Code);
        }
        (None, None) => c.push("(detached)", Style::Error),
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
        c.push(tag, Style::Strong);
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
        c.push(op.word(), Style::Error);
        if unmerged > 0 {
            c.line(&format!(" ({unmerged} unmerged)"), Style::Muted);
        } else {
            c.newline();
        }
        c.close();
    }
    c.close();
    c.newline();
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

/// A folder of the Files panel's tree: its folders and its files.
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
}

/// The changed paths as a tree of folders, as lazygit shows them: a
/// folder holding one folder and no file is written with it
/// (`crates/kalem-core/src`).
fn files_panel(c: &mut Content, repo: &Repo, folds: &Folds, opts: &ViewOptions) {
    let changed = repo.changed();
    if !panel(c, folds, opts, Section::Files, "Files", changed.len()) {
        return end_panel(c);
    }
    if changed.is_empty() {
        c.line(" Nothing changed: the working tree is clean", Style::Muted);
        return end_panel(c);
    }
    let mut root = Folder::default();
    for e in &changed {
        let parts: Vec<&str> = e.path.trim_end_matches('/').split('/').collect();
        root.add(&parts, e);
    }
    folder(c, repo, folds, opts, &root, "", 0);
    end_panel(c);
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
        c.line(&name, Style::Strong);
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
    let style = match e.kind {
        Kind::Unmerged(_) => Style::Error,
        _ if e.staged() && !e.unstaged() => Style::Strong,
        _ => Style::Normal,
    };
    c.push(&e.code(), style);
    c.push(" ", Style::Normal);
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
            c.push(from, Style::Normal);
            c.push(&format!(" {} ", opts.glyphs.arrow()), Style::Muted);
            c.push(&name, style);
        }
        None => c.push(&name, style),
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
fn branches_panel(c: &mut Content, repo: &Repo, folds: &Folds, opts: &ViewOptions) {
    let mut local = repo.local_branches();
    // The current one first, as lazygit lists them.
    local.sort_by_key(|b| !b.current);
    if !panel(
        c,
        folds,
        opts,
        Section::Branches,
        "Local branches",
        local.len(),
    ) {
        return end_panel(c);
    }
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
    end_panel(c);
}

fn branch_line(c: &mut Content, b: &Branch, opts: &ViewOptions) {
    c.open(format!("branch:{}", b.name), false, false);
    if b.current {
        c.push(" * ", Style::Strong);
        c.push(&b.name, Style::Strong);
    } else {
        c.push("   ", Style::Normal);
        c.push(&b.name, Style::Normal);
    }
    if b.upstream.is_some() {
        if b.gone {
            c.push(" (upstream gone)", Style::Error);
        } else if b.ahead > 0 || b.behind > 0 {
            c.push(" ", Style::Normal);
            c.push(&opts.glyphs.ahead_behind(b.ahead, b.behind), Style::Muted);
        } else if opts.glyphs == Glyphs::Unicode {
            c.push(" ✓", Style::Muted);
        }
    }
    c.newline();
    c.close();
}

/// The last commits, HEAD's first: hash, author's initials, tags,
/// subject; the ones not pushed marked `↑`.
fn commits_panel(c: &mut Content, repo: &Repo, folds: &Folds, opts: &ViewOptions) {
    if !panel(
        c,
        folds,
        opts,
        Section::Recent,
        "Commits",
        repo.recent.len(),
    ) {
        return end_panel(c);
    }
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
                " {} to pull from {}: F p",
                repo.unpulled.len(),
                repo.status.upstream.as_deref().unwrap_or("the upstream")
            ),
            Style::Muted,
        );
    }
    end_panel(c);
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
    c.push(&format!(" {mark}"), Style::Strong);
    c.push(&commit.short, Style::Code);
    c.push(" ", Style::Normal);
    c.push(&initials(&commit.author), Style::Muted);
    let tags: Vec<&str> = commit
        .refs
        .iter()
        .filter_map(|r| r.strip_prefix("tag: "))
        .collect();
    if !tags.is_empty() {
        c.push(" ", Style::Normal);
        c.push(&tags.join(" "), Style::Strong);
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
fn stash_panel(c: &mut Content, repo: &Repo, folds: &Folds, opts: &ViewOptions) {
    if !panel(
        c,
        folds,
        opts,
        Section::Stashes,
        "Stash",
        repo.stashes.len(),
    ) {
        return end_panel(c);
    }
    for s in &repo.stashes {
        c.open(format!("stash:{}", s.index), false, false);
        c.push(" ", Style::Normal);
        c.push(&s.name(), Style::Code);
        c.push(": ", Style::Muted);
        c.line(&s.message, Style::Normal);
        c.close();
    }
    end_panel(c);
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
    fn lazygits_panels_and_magits_diff() {
        let mut folds = Folds::default();
        folds.set(&path_key("README.md"), false);
        let c = render(&repo(), &folds, &ViewOptions::default());
        let expected = "\
Tab diff · Enter open · s stage · u unstage · a all · d discard · c c commit · P p push · p pull · b b branch · ? menu · q close

─ Status ───────────────────────────────────────────────────
 ↑1 org → main  Fix it
 origin/main  (Enter: push, pull, fetch)

─ Files ──────────────────────────────────────────────── 4 ─
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

─ Local branches ─────────────────────────────────────── 2 ─
 * main ↑1
   feature

─ Commits ────────────────────────────────────────────── 2 ─
 ↑abc1234 AT v0.1 Fix it
  def5678 AT First

─ Stash ──────────────────────────────────────────────── 0 ─
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
        assert_eq!(at("* main"), Some(Target::Branch("main".into())));
        assert_eq!(at("def5678"), Some(Target::Commit("def".into())));
        assert_eq!(at("─ Files"), Some(Target::Section(Section::Files)));
    }

    #[test]
    fn folded_panels_folders_and_ascii() {
        let mut folds = Folds::default();
        folds.set("section:branches", true);
        folds.set("dir:crates/core/src", true);
        let opts = ViewOptions {
            glyphs: Glyphs::Ascii,
            ..ViewOptions::default()
        };
        let c = render(&repo(), &folds, &opts);
        assert!(
            c.text.contains("\n > crates/core/src\n MM README.md\n"),
            "{}",
            c.text
        );
        assert!(c.text.contains("- Local branches ... ---"), "{}", c.text);
        assert!(!c.text.contains("feature"), "{}", c.text);
        assert!(
            c.text.contains("\n +1 ahead org -> main  Fix it\n"),
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
