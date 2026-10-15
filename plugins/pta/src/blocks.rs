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
    })
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
