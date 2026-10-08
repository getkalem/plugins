//! The status document (DESIGN.md, 2.1 and appendix D): the keys, the
//! head lines, then the sections of files with their diffs under them, the
//! branches, the commits and the stashes, written as text with regions:
//! magit's status in lazygit's order of panels.
//!
//! A hunk's lines start at the first column as `git diff` prints them, so
//! that Kalem's `diff` highlighter colors them; every other line is
//! indented or starts with a word, so the highlighter leaves it alone.

use crate::content::{Content, Style};
use crate::git::diff::{FileDiff, LineKind};
use crate::git::log::Commit;
use crate::git::refs::Branch;
use crate::git::status::{Entry, Kind, State};
use crate::model::{Folds, Repo, Section};
use crate::target::{file_key, hunk_key};
use crate::views::{ViewOptions, lossy_line};

/// The first line's keys, so that nothing has to be known beforehand.
pub const HELP: &[(&str, &str)] = &[
    ("Tab", "fold"),
    ("Enter", "open"),
    ("s", "stage"),
    ("u", "unstage"),
    ("x", "discard"),
    ("c c", "commit"),
    ("P p", "push"),
    ("F p", "pull"),
    ("b b", "branch"),
    ("?", "all"),
    ("q", "close"),
];

/// The first line: [`HELP`]'s keys.
pub fn help_line(glyphs: crate::views::Glyphs) -> String {
    let sep = match glyphs {
        crate::views::Glyphs::Unicode => " · ",
        crate::views::Glyphs::Ascii => " | ",
    };
    HELP.iter()
        .map(|(k, w)| format!("{k} {w}"))
        .collect::<Vec<_>>()
        .join(sep)
}

/// The local branches listed at most; `b b` offers them all.
pub const BRANCHES_SHOWN: usize = 10;

/// The status of `repo` as the folds have it.
pub fn render(repo: &Repo, folds: &Folds, opts: &ViewOptions) -> Content {
    let mut c = Content::new();
    c.open("help", false, false);
    c.line(&help_line(opts.glyphs), Style::Muted);
    c.close();
    c.newline();
    head_lines(&mut c, repo, opts);
    c.newline();
    for section in [
        Section::Unmerged,
        Section::Untracked,
        Section::Unstaged,
        Section::Staged,
    ] {
        let entries = repo.entries(section);
        if entries.is_empty() {
            continue;
        }
        let key = format!("section:{}", section.key());
        let folded = folds.folded(&key);
        c.open(&key, true, folded);
        heading(&mut c, opts, folded, title(section), entries.len());
        if !folded {
            for e in entries {
                file(&mut c, repo, folds, opts, section, e);
            }
        }
        c.close();
        c.newline();
    }
    branches(&mut c, repo, folds, opts);
    let upstream = repo.status.upstream.as_deref().unwrap_or("the upstream");
    let commit_sections = [
        (
            Section::Unpushed,
            format!("Unpushed to {upstream}"),
            &repo.unpushed,
            repo.status.ahead,
        ),
        (
            Section::Unpulled,
            format!("Unpulled from {upstream}"),
            &repo.unpulled,
            repo.status.behind,
        ),
    ];
    for (section, title, commits, total) in commit_sections {
        if !commits.is_empty() {
            commit_section(
                &mut c,
                folds,
                opts,
                section,
                &title,
                commits,
                total as usize,
            );
        }
    }
    if repo.shows_recent() {
        let n = repo.recent.len();
        commit_section(
            &mut c,
            folds,
            opts,
            Section::Recent,
            "Recent commits",
            &repo.recent,
            n,
        );
    }
    if !repo.stashes.is_empty() {
        let key = "section:stashes";
        let folded = folds.folded(key);
        c.open(key, true, folded);
        heading(&mut c, opts, folded, "Stashes", repo.stashes.len());
        if !folded {
            for s in &repo.stashes {
                c.open(format!("stash:{}", s.index), false, false);
                c.push("  ", Style::Normal);
                c.push(&s.name(), Style::Code);
                c.push("  ", Style::Normal);
                c.line(&s.message, Style::Normal);
                c.close();
            }
        }
        c.close();
        c.newline();
    }
    // The last blank line goes: the document ends with its last section.
    if c.text.ends_with("\n\n") {
        c.text.pop();
    }
    c
}

/// The local branches, the current one marked, the latest commit first.
fn branches(c: &mut Content, repo: &Repo, folds: &Folds, opts: &ViewOptions) {
    let mut local = repo.local_branches();
    if local.is_empty() {
        return;
    }
    // The current one first, as lazygit lists them.
    local.sort_by_key(|b| !b.current);
    let key = "section:branches";
    let folded = folds.folded(key);
    c.open(key, true, folded);
    heading(c, opts, folded, "Branches", local.len());
    if !folded {
        let width = local
            .iter()
            .take(BRANCHES_SHOWN)
            .map(|b| b.name.chars().count())
            .max()
            .unwrap_or(0);
        for b in local.iter().take(BRANCHES_SHOWN) {
            branch_line(c, b, width, opts);
        }
        if local.len() > BRANCHES_SHOWN {
            c.line(
                &format!(
                    "  … and {} more: b b offers them all",
                    local.len() - BRANCHES_SHOWN
                ),
                Style::Muted,
            );
        }
    }
    c.close();
    c.newline();
}

fn branch_line(c: &mut Content, b: &Branch, width: usize, opts: &ViewOptions) {
    c.open(format!("branch:{}", b.name), false, false);
    if b.current {
        c.push("  * ", Style::Strong);
        c.push(&format!("{:<width$}", b.name), Style::Strong);
    } else {
        c.push("    ", Style::Normal);
        c.push(&format!("{:<width$}", b.name), Style::Normal);
    }
    if let Some(u) = &b.upstream {
        c.push("  ", Style::Normal);
        c.push(u, Style::Muted);
        if b.gone {
            c.push(" (gone)", Style::Error);
        } else if b.ahead > 0 || b.behind > 0 {
            c.push(" ", Style::Normal);
            c.push(&opts.glyphs.ahead_behind(b.ahead, b.behind), Style::Muted);
        }
    }
    c.newline();
    c.close();
}

/// A section of files' title.
fn title(section: Section) -> &'static str {
    match section {
        Section::Unmerged => "Unmerged",
        Section::Untracked => "Untracked files",
        Section::Unstaged => "Unstaged changes",
        Section::Staged => "Staged changes",
        _ => "",
    }
}

fn heading(c: &mut Content, opts: &ViewOptions, folded: bool, title: &str, count: usize) {
    c.push(opts.glyphs.fold(folded), Style::Muted);
    c.push(" ", Style::Normal);
    c.push(title, Style::Heading);
    c.line(&format!(" ({count})"), Style::Muted);
}

fn head_lines(c: &mut Content, repo: &Repo, opts: &ViewOptions) {
    let s = &repo.status;
    c.open("head", false, false);
    c.push("Head:      ", Style::Muted);
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
            c.push("   ", Style::Normal);
            c.line(&h.subject, Style::Normal);
        }
        None if s.unborn() => c.line("   (no commit yet)", Style::Muted),
        None => c.newline(),
    }
    c.close();
    if let Some(u) = &s.upstream {
        c.open("upstream", false, false);
        c.push("Upstream:  ", Style::Muted);
        c.push(u, Style::Strong);
        if s.ahead > 0 || s.behind > 0 {
            c.push("   ", Style::Normal);
            c.push(&opts.glyphs.ahead_behind(s.ahead, s.behind), Style::Muted);
        }
        c.newline();
        c.close();
    }
    if let Some((tag, n)) = &repo.tag {
        c.open("tag", false, false);
        c.push("Tags:      ", Style::Muted);
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
        let tail = if unmerged > 0 {
            format!(" ({unmerged} unmerged)")
        } else {
            String::new()
        };
        c.push(op.word(), Style::Error);
        c.line(&tail, Style::Muted);
        c.close();
    }
}

fn file(
    c: &mut Content,
    repo: &Repo,
    folds: &Folds,
    opts: &ViewOptions,
    section: Section,
    e: &Entry,
) {
    let key = file_key(section, &e.path);
    let folded = folds.folded(&key);
    c.open(&key, true, folded);
    c.push(opts.glyphs.fold(folded), Style::Muted);
    c.push(" ", Style::Normal);
    let word = match (section, e.kind) {
        (_, Kind::Unmerged(conflict)) => {
            c.push(&format!("{:<15}", conflict.words()), Style::Error);
            None
        }
        (Section::Untracked, _) => None,
        (Section::Staged, _) => Some(e.index),
        _ => Some(e.worktree),
    };
    if let Some(state) = word {
        let w = if e.submodule && state == State::Modified {
            "submodule"
        } else {
            state.word()
        };
        c.push(&format!("{w:<13}"), Style::Muted);
    }
    match (&e.from, section) {
        (Some(from), Section::Staged) => {
            c.push(from, Style::Normal);
            c.push(&format!(" {} ", opts.glyphs.arrow()), Style::Muted);
            c.push(&e.path, Style::Normal);
        }
        _ => c.push(&e.path, Style::Normal),
    }
    if let Some((added, removed)) = repo.counts.get(&(section, e.path.clone())) {
        c.push(
            &format!("   {}", opts.glyphs.counts(*added, *removed)),
            Style::Muted,
        );
    }
    c.newline();
    if !folded {
        match repo.diff(section, &e.path) {
            None => c.line("  Loading…", Style::Muted),
            Some(d) => diff_lines(c, folds, opts, section, &e.path, d),
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
            &format!(
                "  {} changed lines: Enter opens the diff as a document",
                group(lines)
            ),
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

fn commit_section(
    c: &mut Content,
    folds: &Folds,
    opts: &ViewOptions,
    section: Section,
    title: &str,
    commits: &[Commit],
    total: usize,
) {
    let key = format!("section:{}", section.key());
    let folded = folds.folded(&key);
    c.open(&key, true, folded);
    heading(c, opts, folded, title, total);
    if !folded {
        for commit in commits {
            commit_line(c, commit);
        }
        if total > commits.len() {
            c.line(
                &format!("  … and {} more", total - commits.len()),
                Style::Muted,
            );
        }
    }
    c.close();
    c.newline();
}

/// A commit's line: its short hash and subject, its references after.
pub fn commit_line(c: &mut Content, commit: &Commit) {
    c.open(format!("commit:{}", commit.hash), false, false);
    c.push("  ", Style::Normal);
    c.push(&commit.short, Style::Code);
    c.push("  ", Style::Normal);
    c.push(&commit.subject, Style::Normal);
    if !commit.refs.is_empty() {
        c.push(&format!("  ({})", commit.refs.join(", ")), Style::Strong);
    }
    c.newline();
    c.close();
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
    use crate::views::Glyphs;

    fn repo() -> Repo {
        let z = |parts: &[&str]| -> Vec<u8> {
            parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
        };
        let s = status::parse(&z(&[
            "# branch.oid abc",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +1 -0",
            "1 .M N... 100644 100644 100644 a a src/lib.rs",
            "1 A. N... 000000 100644 100644 0 b new.md",
            "? notes.txt",
        ]))
        .unwrap();
        let mut r = Repo {
            root: "/r".into(),
            status: s,
            head: Some(Commit {
                hash: "abc".into(),
                short: "abc".into(),
                subject: "Fix it".into(),
                ..Commit::default()
            }),
            ..Repo::default()
        };
        r.unpushed = vec![r.head.clone().unwrap()];
        r.branches = crate::git::refs::parse_branches(
            b"refs/heads/main\0main\0origin/main\0[ahead 1]\0*\nrefs/heads/feature/long\0feature/long\0\0\0 \nrefs/remotes/origin/main\0origin/main\0\0\0 \n",
        );
        r.counts
            .insert((Section::Unstaged, "src/lib.rs".into()), (1, 1));
        r.diffs.insert(
            (Section::Unstaged, "src/lib.rs".into()),
            diff::parse(b"diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n")
                .unwrap()
                .files
                .remove(0),
        );
        r
    }

    #[test]
    fn the_whole_status() {
        let mut folds = Folds::default();
        folds.set(&file_key(Section::Unstaged, "src/lib.rs"), false);
        let c = render(&repo(), &folds, &ViewOptions::default());
        let expected = "\
Tab fold · Enter open · s stage · u unstage · x discard · c c commit · P p push · F p pull · b b branch · ? all · q close

Head:      main   Fix it
Upstream:  origin/main   ↑1

▾ Untracked files (1)
▸ notes.txt

▾ Unstaged changes (1)
▾ modified     src/lib.rs   +1 −1
@@ -1,2 +1,2 @@
 a
-b
+B

▾ Staged changes (1)
▸ new file     new.md

▾ Branches (2)
  * main          origin/main ↑1
    feature/long

▾ Unpushed to origin/main (1)
  abc  Fix it
";
        assert_eq!(c.text, expected);
        let b = c.text.find("-b").unwrap();
        assert_eq!(
            target::at(&c, b),
            Some(Target::Hunk {
                section: Section::Unstaged,
                path: "src/lib.rs".into(),
                hunk: 0
            })
        );
        assert_eq!(
            target::at(&c, c.text.find("abc").unwrap()),
            Some(Target::Commit("abc".into()))
        );
        assert_eq!(c.style_at(c.text.find("main").unwrap()), Style::Strong);
        assert_eq!(
            target::at(&c, c.text.find("feature/long").unwrap()),
            Some(Target::Branch("feature/long".into()))
        );
        assert_eq!(target::at(&c, 0), Some(Target::Help));
    }

    #[test]
    fn ascii_glyphs_folded_sections_and_loading() {
        let mut folds = Folds::default();
        folds.set("section:untracked", true);
        folds.set(&file_key(Section::Staged, "new.md"), false);
        let opts = ViewOptions {
            glyphs: Glyphs::Ascii,
            ..ViewOptions::default()
        };
        let c = render(&repo(), &folds, &opts);
        assert!(
            c.text.contains("Upstream:  origin/main   +1 ahead\n"),
            "{}",
            c.text
        );
        assert!(
            c.text.contains("> Untracked files (1)\n\nv Unstaged"),
            "{}",
            c.text
        );
        assert!(
            c.text.contains("v new file     new.md\n  Loading…\n"),
            "{}",
            c.text
        );
    }

    #[test]
    fn large_numbers() {
        assert_eq!(group(12), "12");
        assert_eq!(group(1234567), "1,234,567");
    }
}
