//! A commit as a document (DESIGN.md, 2.4): `git show`'s header and stat,
//! then its diff as git prints it, every file and hunk a region, so that
//! Enter on a line opens the file there.

use crate::content::{Content, Style};
use crate::git::diff::Diff;
use crate::model::{Folds, Section};
use crate::target::{file_key, hunk_key};
use crate::views::{ViewOptions, lossy_line};

/// The document of a commit, from `git show`'s output read by
/// [`crate::git::diff::parse`].
pub fn render(diff: &Diff, folds: &Folds, opts: &ViewOptions) -> Content {
    let _ = opts;
    let mut c = Content::new();
    for l in &diff.preamble {
        c.line(&lossy_line(l), Style::Normal);
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
