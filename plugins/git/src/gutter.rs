//! The marks beside the lines of a file (DESIGN.md, 2.8): what changed
//! since the index (or HEAD), read from `git diff -U0`, as Kalem's
//! `decorations` draws them in the gutter.

use crate::git::cmd::GitCommand;
use crate::git::diff::FileDiff;

/// What a line changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    /// The line is new.
    Added,
    /// The line changed.
    Changed,
    /// Lines were removed after it (after line 0: before the first).
    Removed,
}

/// The diff of file `path` (in the repository) without context: against
/// the index, or against HEAD (the setting `gutter_base = "head"`).
pub fn command(path: &str, head: bool) -> GitCommand {
    let mut args = vec![
        "diff".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "-U0".to_string(),
    ];
    if head {
        args.push("HEAD".into());
    }
    args.push("--".into());
    args.push(path.to_string());
    GitCommand::read(args)
}

/// The marks of a file's diff without context, lines from 1: a hunk's
/// lines are changed as far as it replaced lines, added past them, and a
/// hunk that took lines away marks the line before them.
pub fn marks(d: &FileDiff) -> Vec<(u32, Mark)> {
    let mut v = Vec::new();
    if d.binary {
        return v;
    }
    for h in &d.hunks {
        let (old, new) = (h.old_count, h.new_count);
        if new == 0 {
            // `-U0` names the line before the removed ones.
            v.push((h.new_start, Mark::Removed));
            continue;
        }
        for i in 0..new {
            let kind = if i < old { Mark::Changed } else { Mark::Added };
            v.push((h.new_start + i, kind));
        }
        if old > new {
            v.push((h.new_start + new - 1, Mark::Removed));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff;

    fn file(text: &str) -> FileDiff {
        diff::parse(text.as_bytes()).unwrap().files.remove(0)
    }

    #[test]
    fn hunks_become_marks() {
        let d = file(
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n\
             @@ -0,0 +1,2 @@\n+new\n+lines\n\
             @@ -5 +7 @@\n-old\n+changed\n\
             @@ -9,2 +10,3 @@\n-x\n-y\n+X\n+Y\n+Z\n\
             @@ -20,3 +21 @@\n-p\n-q\n-r\n+P\n\
             @@ -30,2 +29,0 @@\n-gone\n-too\n",
        );
        assert_eq!(
            marks(&d),
            [
                (1, Mark::Added),
                (2, Mark::Added),
                (7, Mark::Changed),
                (10, Mark::Changed),
                (11, Mark::Changed),
                (12, Mark::Added),
                (21, Mark::Changed),
                (21, Mark::Removed),
                (29, Mark::Removed),
            ]
        );
        let top = file("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +0,0 @@\n-first\n");
        assert_eq!(marks(&top), [(0, Mark::Removed)]);
    }
}
