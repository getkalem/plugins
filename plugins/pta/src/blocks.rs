//! Journal blocks in Org, run (PT11b): an `hledger`, `ledger` or
//! `beancount` source block's text handed to its tool on standard input
//! (`hledger -f -`, `ledger -f -`, `bean-query /dev/stdin`) with the
//! block's `:cmdline` (ob-ledger's header argument; `:cmd` too), the
//! output written under the block as Org's `#+RESULTS:`, in fixed-width
//! lines, a block's results run again replaced: the plugin's own command.
//! Kalem's Babel runs the same blocks through the plugin's executor (plugin
//! API 0.2.21, K6), with [`tool_command`].

use std::ops::Range;

use crate::Dialect;

/// A journal source block of an Org document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub dialect: Dialect,
    /// The `#+begin_src` line's start.
    pub start: usize,
    /// Its text: the lines between `#+begin_src` and `#+end_src`.
    pub body: Range<usize>,
    /// Where the results go: after the `#+end_src` line, or over the
    /// results already there.
    pub results: Range<usize>,
    /// Its `:cmdline` (or `:cmd`), empty without.
    pub cmdline: String,
    /// Its indentation kept (the `-i` switch).
    pub preserve: bool,
}

/// The lines of `text`: each one's start and end (its line feed left
/// out).
fn lines(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut s = 0;
    for l in text.split_inclusive('\n') {
        out.push(s..s + l.trim_end_matches(['\n', '\r']).len());
        s += l.len();
    }
    out
}

/// A header argument's value: from after `:name ` to the next ` :word`.
fn header_arg(args: &str, name: &str) -> Option<String> {
    let key = format!(":{name}");
    let words: Vec<&str> = args.split_whitespace().collect();
    let i = words.iter().position(|w| w.eq_ignore_ascii_case(&key))?;
    let value: Vec<&str> = words[i + 1..]
        .iter()
        .take_while(|w| !(w.starts_with(':') && w.len() > 1))
        .copied()
        .collect();
    Some(value.join(" "))
}

/// The journal block holding byte `at` of the Org text `text`, if any.
pub fn block_at(text: &str, at: usize) -> Option<Block> {
    let ls = lines(text);
    let line = ls.iter().position(|r| r.start <= at && at <= r.end)?;
    let begin = (0..=line).rev().find(|&n| {
        text[ls[n].clone()]
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("#+begin_src")
    })?;
    let end = (begin + 1..ls.len()).find(|&n| {
        text[ls[n].clone()]
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("#+end_src")
    })?;
    if line > end {
        return None;
    }
    let header = text[ls[begin].clone()].trim_start();
    let rest = header["#+begin_src".len()..].trim_start();
    let (language, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let dialect = Dialect::of_language(&language.to_ascii_lowercase())?;
    let cmdline = header_arg(args, "cmdline")
        .or_else(|| header_arg(args, "cmd"))
        .unwrap_or_default();
    // The switches come before the header arguments.
    let preserve = args
        .split_whitespace()
        .take_while(|w| !w.starts_with(':'))
        .any(|w| w == "-i");
    let body = if begin + 1 < end {
        ls[begin + 1].start..ls[end - 1].end
    } else {
        ls[end].start..ls[end].start
    };
    // After `#+end_src`: blank lines, then results already written (a
    // `#+RESULTS:` line and its fixed-width lines, or its example
    // block), replaced.
    let after = ls.get(end + 1).map_or(text.len(), |r| r.start);
    let mut n = end + 1;
    while n < ls.len() && text[ls[n].clone()].trim().is_empty() {
        n += 1;
    }
    let results = if n < ls.len()
        && text[ls[n].clone()]
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("#+results")
    {
        let mut last = n;
        let mut m = n + 1;
        if m < ls.len()
            && text[ls[m].clone()]
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("#+begin_example")
        {
            while m < ls.len()
                && !text[ls[m].clone()]
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("#+end_example")
            {
                m += 1;
            }
            last = m.min(ls.len() - 1);
        } else {
            while m < ls.len() && text[ls[m].clone()].trim_start().starts_with(':') {
                last = m;
                m += 1;
            }
        }
        let end_of = ls.get(last + 1).map_or(text.len(), |r| r.start);
        after..end_of
    } else {
        after..after
    };
    Some(Block {
        dialect,
        start: ls[begin].start,
        body,
        results,
        cmdline,
        preserve,
    })
}

/// The block's code as Kalem's Babel gives it to an executor, and so to
/// the tool: Org's escapes taken off (`,*` and `,#+` at a line's start,
/// after its indentation: `org-unescape-code-in-string`), then, unless
/// the block keeps its indentation (`-i`), the common indentation of its
/// lines (Org's content indent of two blanks), as
/// `org-remove-indentation` does; ending with a line feed.
pub fn code(text: &str, b: &Block) -> String {
    let body = text[b.body.clone()].replace("\r\n", "\n");
    let mut out = String::with_capacity(body.len() + 1);
    for line in body.split_inclusive('\n') {
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let rest = &line[indent..];
        let commas = rest.len() - rest.trim_start_matches(',').len();
        let after = &rest[commas..];
        if commas > 0 && (after.starts_with('*') || after.starts_with("#+")) {
            out.push_str(&line[..indent + commas - 1]);
            out.push_str(after);
        } else {
            out.push_str(line);
        }
    }
    let mut out = if b.preserve {
        out
    } else {
        remove_indentation(&out)
    };
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// `org-remove-indentation`: the smallest indentation of the lines not
/// blank taken off each, a tab counting to the next multiple of 8 (one
/// across the cut stays); blank lines lose their blanks; nothing where a
/// line has none.
fn remove_indentation(s: &str) -> String {
    let column_of = |l: &str| {
        let mut w = 0;
        for c in l.chars() {
            match c {
                ' ' => w += 1,
                '\t' => w = (w / 8 + 1) * 8,
                _ => break,
            }
        }
        w
    };
    let Some(n) = s
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .map(column_of)
        .min()
    else {
        return s.to_string();
    };
    if n == 0 {
        return s.to_string();
    }
    let lines: Vec<String> = s
        .split('\n')
        .map(|l| {
            if l.trim().is_empty() {
                return String::new();
            }
            let mut col = 0;
            let mut rest = l;
            while col < n {
                match rest.chars().next() {
                    Some(' ') => {
                        col += 1;
                        rest = &rest[1..];
                    }
                    Some('\t') if (col / 8 + 1) * 8 <= n => {
                        col = (col / 8 + 1) * 8;
                        rest = &rest[1..];
                    }
                    _ => break,
                }
            }
            rest.to_string()
        })
        .collect();
    lines.join("\n")
}

/// The tool's run of a block: program and arguments, the block's text
/// on standard input.
pub fn command(b: &Block) -> (&'static str, Vec<String>) {
    tool_command(b.dialect, &b.cmdline)
}

/// The tool of `dialect` run over a journal on its standard input with
/// `cmdline` (ob-ledger's `:cmdline`): the balance report without one,
/// `BALANCES` for Beancount.
pub fn tool_command(dialect: Dialect, cmdline: &str) -> (&'static str, Vec<String>) {
    let words = crate::other::words(cmdline);
    match dialect {
        Dialect::Hledger => {
            let mut a = vec!["-f".to_string(), "-".to_string()];
            if words.is_empty() {
                a.push("balance".into());
            }
            a.extend(words);
            ("hledger", a)
        }
        Dialect::Ledger => {
            let mut a = vec!["-f".to_string(), "-".to_string()];
            if words.is_empty() {
                a.push("balance".into());
            }
            a.extend(words);
            ("ledger", a)
        }
        Dialect::Beancount => {
            let query = if cmdline.trim().is_empty() {
                "BALANCES".to_string()
            } else {
                cmdline.trim().trim_matches('"').to_string()
            };
            ("bean-query", vec!["/dev/stdin".into(), query])
        }
    }
}

/// The results as Org writes them: `#+RESULTS:` and the output's lines
/// in fixed width (`: `).
pub fn results(output: &str) -> String {
    let mut out = String::from("#+RESULTS:\n");
    for l in output.trim_end_matches('\n').lines() {
        if l.trim().is_empty() {
            out.push_str(":\n");
        } else {
            out.push_str(": ");
            out.push_str(l.trim_end());
            out.push('\n');
        }
    }
    out
}

/// The edit that writes `output` as `b`'s results: after a blank line
/// below the block, over results already there.
pub fn write(text: &str, b: &Block, output: &str) -> (Range<usize>, String) {
    let mut ins = String::new();
    if b.results.start == text.len() && !text.ends_with('\n') {
        ins.push('\n');
    }
    ins.push('\n');
    ins.push_str(&results(output));
    // A blank line before what follows, where none is.
    let rest = &text[b.results.end..];
    if !rest.is_empty() && !rest.starts_with('\n') && !rest.starts_with("\r\n") {
        ins.push('\n');
    }
    // The blank lines between the block and old results go too.
    (b.results.clone(), ins)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "* Money\n#+begin_src hledger :cmdline bal -M :results output\n2026-01-01 Shop\n    expenses:food  10 TRY\n    assets:cash\n#+end_src\n\nMore text.\n";

    #[test]
    fn a_block_found() {
        let b = block_at(ORG, ORG.find("Shop").unwrap()).expect("a block");
        assert_eq!(b.dialect, Dialect::Hledger);
        assert_eq!(b.cmdline, "bal -M");
        assert!(
            ORG[b.body.clone()].starts_with("2026-01-01 Shop")
                && ORG[b.body.clone()].ends_with("assets:cash")
        );
        assert_eq!(command(&b).1, ["-f", "-", "bal", "-M"]);
        assert_eq!(
            tool_command(Dialect::Ledger, ""),
            ("ledger", vec!["-f".into(), "-".into(), "balance".into()])
        );
        assert_eq!(
            tool_command(Dialect::Beancount, "SELECT account"),
            (
                "bean-query",
                vec!["/dev/stdin".into(), "SELECT account".into()]
            )
        );
        assert!(block_at(ORG, ORG.find("More").unwrap()).is_none());
        assert!(block_at("#+begin_src python\nx\n#+end_src\n", 20).is_none());
    }

    #[test]
    fn the_code_given_to_the_tool() {
        // Indented under its heading, a journal's own heading (a comment
        // to the tools) escaped with a comma.
        let org = "* Money\n  #+begin_src ledger :cmdline bal\n  ,* January\n  2026/01/01 Shop\n      Expenses:Food  10 EUR\n      Assets:Cash\n\n  2026/01/02 Kiosk\n      Expenses:Food  1 EUR\n      Assets:Cash\n  #+end_src\n";
        let b = block_at(org, org.find("Shop").unwrap()).expect("a block");
        assert!(!b.preserve);
        assert_eq!(
            code(org, &b),
            "* January\n2026/01/01 Shop\n    Expenses:Food  10 EUR\n    Assets:Cash\n\n2026/01/02 Kiosk\n    Expenses:Food  1 EUR\n    Assets:Cash\n"
        );
        // `,#+` too; `-i` keeps the indentation, not the escapes.
        let kept = "#+begin_src hledger -i :cmdline bal\n  ,#+ note\n  2026-01-01 Shop\n      a  1\n      b\n#+end_src\n";
        let b = block_at(kept, 40).expect("a block");
        assert!(b.preserve);
        assert_eq!(
            code(kept, &b),
            "  #+ note\n  2026-01-01 Shop\n      a  1\n      b\n"
        );
        // Not indented: as written.
        let b = block_at(ORG, ORG.find("Shop").unwrap()).unwrap();
        assert_eq!(code(ORG, &b), format!("{}\n", &ORG[b.body.clone()]));
        assert_eq!(remove_indentation("\t\tx\n\ty"), "\tx\ny");
    }

    #[test]
    fn results_written_and_replaced() {
        let b = block_at(ORG, ORG.find("Shop").unwrap()).unwrap();
        let (r, ins) = write(ORG, &b, "  10 TRY  expenses:food\n\n");
        let once = format!("{}{}{}", &ORG[..r.start], ins, &ORG[r.end..]);
        assert!(
            once.contains("#+end_src\n\n#+RESULTS:\n:   10 TRY  expenses:food\n\nMore text."),
            "{once:?}"
        );
        // Run again: the results replaced, not added.
        let b = block_at(&once, once.find("Shop").unwrap()).unwrap();
        let (r, ins) = write(&once, &b, "  20 TRY  expenses:food\n");
        let twice = format!("{}{}{}", &once[..r.start], ins, &once[r.end..]);
        assert_eq!(twice.matches("#+RESULTS:").count(), 1, "{twice:?}");
        assert!(
            twice.contains(":   20 TRY") && !twice.contains("10 TRY  expenses:food\n\n"),
            "{twice:?}"
        );
        assert!(twice.ends_with("More text.\n"));
    }
}
