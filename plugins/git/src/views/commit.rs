//! A commit as a document (DESIGN.md, 2.4): `git show`'s header and stat,
//! then its diff as git prints it, every file and hunk a region, so that
//! Enter on a line opens the file there.

use crate::content::{Color, Content, Style};
use crate::git::diff::Diff;
use crate::model::{Folds, Section};
use crate::target::{file_key, hunk_key};
use crate::views::{ViewOptions, lossy_line};

/// The document of a commit, from `git show`'s output read by
/// [`crate::git::diff::parse`].
pub fn render(diff: &Diff, folds: &Folds, opts: &ViewOptions) -> Content {
    let _ = opts;
    let mut c = Content::new();
    let mut first_message_line = true;
    for l in &diff.preamble {
        header_line(&mut c, &lossy_line(l), &mut first_message_line);
    }
    for f in &diff.files {
        let key = file_key(Section::Shown, f.path());
        let folded = folds.folded(&key);
        c.open(&key, true, folded);
        let header: &[Vec<u8>] = if folded { &f.header[..1] } else { &f.header };
        for l in header {
            c.line(&lossy_line(l), Style::Normal);
        }
        if !folded {
            if f.binary && f.hunks.is_empty() {
                c.line("Binary file changed", Style::Muted);
            }
            for (i, h) in f.hunks.iter().enumerate() {
                let hk = hunk_key(Section::Shown, i, f.path());
                let hfolded = folds.folded(&hk);
                c.open(&hk, true, hfolded);
                c.line(&lossy_line(&h.shown_header()), Style::Normal);
                if !hfolded {
                    for l in &h.lines {
                        c.line(&lossy_line(&l.raw()), Style::Normal);
                    }
                }
                c.close();
            }
        }
        c.close();
    }
    c
}

/// A line of `git show`'s header and stat, styled as magit shows them:
/// the `commit` line in yellow, the labels of the author, the committer
/// and the dates muted, the message's first line strong, a file's counts
/// in the stat green and red, the stat's summary muted.
fn header_line(c: &mut Content, line: &str, first_message_line: &mut bool) {
    const LABELS: [&str; 5] = ["Author:", "AuthorDate:", "Commit:", "CommitDate:", "Merge:"];
    if line.starts_with("commit ") {
        c.line(line, Style::Color(Color::Yellow, true));
    } else if let Some(label) = LABELS.iter().find(|l| line.starts_with(*l)) {
        c.push(label, Style::Muted);
        c.line(&line[label.len()..], Style::Normal);
    } else if line.starts_with("    ") && !line.trim().is_empty() && *first_message_line {
        *first_message_line = false;
        c.line(line, Style::Strong);
    } else if let Some((path, counts)) = line.split_once(" | ") {
        // ` src/lib.rs | 12 ++++---`: the pluses and minuses in color.
        c.push(path, Style::Normal);
        c.push(" | ", Style::Normal);
        let marks = counts.trim_end();
        let digits = marks.trim_end_matches(['+', '-']);
        c.push(digits, Style::Normal);
        let rest = &marks[digits.len()..];
        let pluses = rest.trim_end_matches('-');
        c.push(pluses, Style::Color(Color::Green, false));
        c.push(&rest[pluses.len()..], Style::Color(Color::Red, false));
        c.line(&counts[marks.len()..], Style::Normal);
    } else if line.contains(" changed") && (line.contains("file") || line.contains("files")) {
        c.line(line, Style::Muted);
    } else {
        c.line(line, Style::Normal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff;
    use crate::target::{self, Target};

    #[test]
    fn header_then_files_unfolded() {
        let out = "commit abc\nAuthor: A <a@x>\n\n    Subject\n\n x | 1 +\n\ndiff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1 +1,2 @@\n a\n+b\n";
        let d = diff::parse(out.as_bytes()).unwrap();
        let c = render(&d, &Folds::default(), &ViewOptions::default());
        assert_eq!(c.text, out);
        // The header's styles: the commit line, the author's label, the
        // subject, the stat's plus.
        let style_of = |s: &str| c.style_at(c.text.find(s).unwrap());
        assert_eq!(style_of("commit abc"), Style::Color(Color::Yellow, true));
        assert_eq!(style_of("Author:"), Style::Muted);
        assert_eq!(style_of("A <a@x>"), Style::Normal);
        assert_eq!(style_of("Subject"), Style::Strong);
        assert_eq!(style_of("+\n\ndiff"), Style::Color(Color::Green, false));
        let b = c.text.find("+b").unwrap();
        assert_eq!(
            target::at(&c, b),
            Some(Target::Hunk {
                section: Section::Shown,
                path: "x".into(),
                hunk: 0
            })
        );
        assert_eq!(target::line_in_hunk(&c, b), Some(1));
        let mut folds = Folds::default();
        folds.set(&file_key(Section::Shown, "x"), true);
        let c = render(&d, &folds, &ViewOptions::default());
        assert!(c.text.ends_with("\ndiff --git a/x b/x\n"));
    }
}
