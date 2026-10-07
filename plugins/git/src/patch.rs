//! Patches for a hunk, or for some lines of hunks (DESIGN.md, 3.6), as
//! magit's `magit-apply` and lazygit's `patch` package make them.
//!
//! A patch is made from the diff the status shows and applied by `git
//! apply` (see [`crate::git::cmd::apply`]):
//!
//! - **Forward**, to stage: from the unstaged diff (index → working tree),
//!   applied to the index. Selected lines are kept; an unselected removed
//!   line stays in the index, so it becomes context; an unselected added
//!   line is not staged, so it is dropped.
//! - **Reverse**, to unstage or discard: from the staged diff (HEAD →
//!   index) or the unstaged one, applied with `--reverse` to the index or
//!   the working tree, whose content is the diff's new side. Selected
//!   lines are kept; an unselected added line stays where it is, so it
//!   becomes context; an unselected removed line is not brought back, so
//!   it is dropped.
//!
//! The file's header is kept as git printed it; each hunk's `@@` line is
//! counted again. A `\ No newline at end of file` line goes with the line
//! before it.

use crate::git::diff::{FileDiff, Hunk, Line, LineKind};

/// Which way a patch will be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Applied as it is: staging.
    Forward,
    /// Applied with `--reverse`: unstaging, discarding.
    Reverse,
}

/// What of a hunk goes into a patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The whole hunk.
    All,
    /// These lines of it, by index in [`Hunk::lines`].
    Lines(Vec<usize>),
}

/// Why no patch could be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The selection holds no added or removed line.
    NothingSelected,
    /// The file is new, deleted, binary, a merge's combined diff, or has
    /// no hunks: it is staged, unstaged or discarded whole.
    WholeFileOnly,
    /// A hunk index past the file's hunks.
    NoSuchHunk(usize),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NothingSelected => f.write_str("the selection holds no change"),
            Refusal::WholeFileOnly => {
                f.write_str("this file is staged, unstaged or discarded whole, not by hunk")
            }
            Refusal::NoSuchHunk(i) => write!(f, "the file has no hunk {}", i + 1),
        }
    }
}

/// Whether a file's changes can be taken a hunk or a line at a time.
pub fn partial_allowed(file: &FileDiff) -> bool {
    !(file.new_file || file.deleted || file.binary || file.combined || file.hunks.is_empty())
}

/// A patch of the whole file: every hunk, as git printed them.
pub fn file_patch(file: &FileDiff) -> Vec<u8> {
    let mut out = header(file);
    for h in &file.hunks {
        push_line(&mut out, &h.header_line());
        for l in &h.lines {
            push_line(&mut out, &l.raw());
        }
    }
    out
}

/// A patch of some hunks of `file`, or of some lines of them; hunks named
/// in any order, each once.
pub fn build(file: &FileDiff, picks: &[(usize, Pick)], mode: Mode) -> Result<Vec<u8>, Refusal> {
    if !partial_allowed(file) {
        return Err(Refusal::WholeFileOnly);
    }
    let mut picks: Vec<&(usize, Pick)> = picks.iter().collect();
    picks.sort_by_key(|(i, _)| *i);
    let mut hunks = Vec::new();
    for (i, pick) in picks {
        let h = file.hunks.get(*i).ok_or(Refusal::NoSuchHunk(*i))?;
        if let Some(h) = select(h, pick, mode) {
            hunks.push(h);
        }
    }
    if hunks.is_empty() {
        return Err(Refusal::NothingSelected);
    }
    // The side the patch is located by keeps its numbers; the other side
    // moves by what the patch's earlier hunks add or remove.
    let mut delta: i64 = 0;
    for h in &mut hunks {
        match mode {
            Mode::Forward => h.new_start = shift(h.old_start, h.old_count, h.new_count, delta),
            Mode::Reverse => h.old_start = shift(h.new_start, h.new_count, h.old_count, -delta),
        }
        delta += i64::from(h.new_count) - i64::from(h.old_count);
    }
    let mut out = header(file);
    for h in &hunks {
        push_line(&mut out, &h.header_line());
        for l in &h.lines {
            push_line(&mut out, &l.raw());
        }
    }
    Ok(out)
}

/// The start on the moving side: `start` on the fixed side, moved by
/// `delta`. An empty side's start names the line before it.
fn shift(start: u32, count: u32, other_count: u32, delta: i64) -> u32 {
    let fixed = i64::from(start) + i64::from(count == 0);
    let moved = fixed + delta - i64::from(other_count == 0);
    u32::try_from(moved.max(0)).unwrap_or(0)
}

/// The hunk as the patch carries it; none when it changes nothing.
fn select(h: &Hunk, pick: &Pick, mode: Mode) -> Option<Hunk> {
    let chosen = |i: usize| match pick {
        Pick::All => true,
        Pick::Lines(v) => v.contains(&i),
    };
    let mut lines: Vec<Line> = Vec::with_capacity(h.lines.len());
    let mut kept_previous = false;
    let mut changes = 0;
    for (i, l) in h.lines.iter().enumerate() {
        let kind = match (l.kind, chosen(i), mode) {
            (LineKind::NoNewline, _, _) => {
                if kept_previous {
                    lines.push(l.clone());
                }
                continue;
            }
            (LineKind::Context, _, _) => Some(LineKind::Context),
            (k @ (LineKind::Added | LineKind::Removed), true, _) => {
                changes += 1;
                Some(k)
            }
            (LineKind::Removed, false, Mode::Forward) => Some(LineKind::Context),
            (LineKind::Added, false, Mode::Forward) => None,
            (LineKind::Added, false, Mode::Reverse) => Some(LineKind::Context),
            (LineKind::Removed, false, Mode::Reverse) => None,
            (LineKind::Raw, _, _) => return None,
        };
        kept_previous = kind.is_some();
        if let Some(kind) = kind {
            lines.push(Line {
                kind,
                text: l.text.clone(),
            });
        }
    }
    if changes == 0 {
        return None;
    }
    let old_count = lines
        .iter()
        .filter(|l| matches!(l.kind, LineKind::Context | LineKind::Removed))
        .count();
    let new_count = lines
        .iter()
        .filter(|l| matches!(l.kind, LineKind::Context | LineKind::Added))
        .count();
    Some(Hunk {
        old_start: h.old_start,
        old_count: u32::try_from(old_count).unwrap_or(u32::MAX),
        new_start: h.new_start,
        new_count: u32::try_from(new_count).unwrap_or(u32::MAX),
        heading: h.heading.clone(),
        printed: Vec::new(),
        lines,
    })
}

fn header(file: &FileDiff) -> Vec<u8> {
    let mut out = Vec::new();
    for l in &file.header {
        push_line(&mut out, l);
    }
    out
}

fn push_line(out: &mut Vec<u8>, line: &[u8]) {
    out.extend_from_slice(line);
    out.push(b'\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff;

    const DIFF: &str = "\
diff --git a/f b/f
index 1..2 100644
--- a/f
+++ b/f
@@ -1,5 +1,5 @@
 a
-b
-c
+B
+C
 d
 e
@@ -20,3 +20,4 @@ fn x
 t
+u
 v
 w
";

    fn file() -> FileDiff {
        diff::parse(DIFF.as_bytes()).unwrap().files.remove(0)
    }

    fn text(v: &[u8]) -> &str {
        std::str::from_utf8(v).unwrap()
    }

    #[test]
    fn a_whole_hunk() {
        let p = build(&file(), &[(1, Pick::All)], Mode::Forward).unwrap();
        assert_eq!(
            text(&p),
            "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -20,3 +20,4 @@ fn x\n t\n+u\n v\n w\n"
        );
    }

    #[test]
    fn lines_forward_turn_unselected_removals_into_context() {
        // Stage "-b" and "+B" only: c stays in the index, C is not staged.
        let p = build(&file(), &[(0, Pick::Lines(vec![1, 3]))], Mode::Forward).unwrap();
        assert!(
            text(&p).ends_with("@@ -1,5 +1,5 @@\n a\n-b\n c\n+B\n d\n e\n"),
            "{}",
            text(&p)
        );
    }

    #[test]
    fn lines_reverse_turn_unselected_additions_into_context() {
        // Unstage "+C" only: B stays staged, so it is context; b is not
        // brought back, so it is dropped.
        let p = build(&file(), &[(0, Pick::Lines(vec![4]))], Mode::Reverse).unwrap();
        assert!(
            text(&p).ends_with("@@ -1,4 +1,5 @@\n a\n B\n+C\n d\n e\n"),
            "{}",
            text(&p)
        );
    }

    #[test]
    fn later_hunks_move_by_the_earlier_ones() {
        // The first hunk staged with one line fewer on the new side: the
        // second's new start moves up by one.
        let p = build(
            &file(),
            &[(1, Pick::All), (0, Pick::Lines(vec![1]))],
            Mode::Forward,
        )
        .unwrap();
        let t = text(&p);
        assert!(t.contains("@@ -1,5 +1,4 @@\n a\n-b\n c\n d\n e\n"), "{t}");
        assert!(t.contains("@@ -20,3 +19,4 @@ fn x\n"), "{t}");
    }

    #[test]
    fn nothing_selected_and_whole_files() {
        assert_eq!(
            build(&file(), &[(0, Pick::Lines(vec![0, 5]))], Mode::Forward),
            Err(Refusal::NothingSelected)
        );
        assert_eq!(
            build(&file(), &[(7, Pick::All)], Mode::Forward),
            Err(Refusal::NoSuchHunk(7))
        );
        let new = diff::parse(b"diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1 @@\n+x\n")
            .unwrap()
            .files
            .remove(0);
        assert_eq!(
            build(&new, &[(0, Pick::All)], Mode::Forward),
            Err(Refusal::WholeFileOnly)
        );
        assert!(text(&file_patch(&new)).ends_with("@@ -0,0 +1,1 @@\n+x\n"));
    }

    #[test]
    fn no_newline_goes_with_its_line() {
        let d = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n";
        let f = diff::parse(d.as_bytes()).unwrap().files.remove(0);
        // Stage the removal alone: the addition and its marker stay out.
        let p = build(&f, &[(0, Pick::Lines(vec![1]))], Mode::Forward).unwrap();
        assert!(
            text(&p).ends_with("@@ -1,2 +1,1 @@\n a\n-b\n\\ No newline at end of file\n"),
            "{}",
            text(&p)
        );
        // Unstage the addition alone: the removal and its marker stay out.
        let p = build(&f, &[(0, Pick::Lines(vec![3]))], Mode::Reverse).unwrap();
        assert!(
            text(&p).ends_with("@@ -1,1 +1,2 @@\n a\n+c\n\\ No newline at end of file\n"),
            "{}",
            text(&p)
        );
    }
}
