//! A blamed file as a document (DESIGN.md, 2.5): each line after a column
//! naming the commit that last changed it, the column written once per run
//! of lines from one commit. Line N of the document is line N of the file.

use crate::content::{Content, Style};
use crate::git::blame::{Blame, UNCOMMITTED};
use crate::views::{ViewOptions, lossy_line};

/// The author column's width.
const AUTHOR: usize = 10;

/// The blame document.
pub fn render(blame: &Blame, opts: &ViewOptions) -> Content {
    let bar = match opts.glyphs {
        crate::views::Glyphs::Unicode => "│",
        crate::views::Glyphs::Ascii => "|",
    };
    let blank = " ".repeat(7 + 1 + 10 + 1 + AUTHOR);
    let mut c = Content::new();
    let mut previous: Option<&str> = None;
    for line in &blame.lines {
        let first = previous != Some(line.hash.as_str());
        if first {
            if previous.is_some() {
                c.close();
            }
            c.open(format!("commit:{}", line.hash), false, false);
        }
        previous = Some(&line.hash);
        if !first {
            c.push(&blank, Style::Normal);
        } else if line.hash == UNCOMMITTED {
            c.push(
                &format!("{:<w$}", "not committed", w = blank.len()),
                Style::Muted,
            );
        } else {
            let info = blame.commits.get(&line.hash).cloned().unwrap_or_default();
            c.push(line.hash.get(..7).unwrap_or(&line.hash), Style::Code);
            c.push(" ", Style::Normal);
            c.push(&info.day(), Style::Muted);
            c.push(" ", Style::Normal);
            let author: String = info.author.chars().take(AUTHOR).collect();
            c.push(&format!("{author:<AUTHOR$}"), Style::Muted);
        }
        c.push(&format!(" {bar} "), Style::Muted);
        c.line(&lossy_line(&line.text), Style::Normal);
    }
    if previous.is_some() {
        c.close();
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::blame;
    use crate::target::{self, Target};

    #[test]
    fn runs_of_lines_name_their_commit_once() {
        let out = "\
a1b2c3d4e5 1 1 2
author Ayşe Yılmazoğlu
author-time 0
author-tz +0000
summary S
filename f
\tone
a1b2c3d4e5 2 2
filename f
\ttwo
0000000000000000000000000000000000000000 3 3 1
author Not Committed Yet
author-time 0
author-tz +0000
summary S
filename f
\tthree
";
        let b = blame::parse(out.as_bytes()).unwrap();
        let c = render(&b, &ViewOptions::default());
        let lines: Vec<&str> = c.text.lines().collect();
        assert_eq!(lines[0], "a1b2c3d 1970-01-01 Ayşe Yılma │ one");
        assert_eq!(lines[1], format!("{:29} │ two", ""));
        assert_eq!(lines[2], format!("{:29} │ three", "not committed"));
        let two = c.text.find("two").unwrap();
        assert_eq!(
            target::at(&c, two),
            Some(Target::Commit("a1b2c3d4e5".into()))
        );
        assert_eq!(c.line_of(two), 1);
    }
}
