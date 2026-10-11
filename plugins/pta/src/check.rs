//! The tool's check (PT5): the command each tool is run with on the main
//! journal, what it says when the journal fails (read from its standard
//! error, each tool's own form), and the document "Check: …" written
//! from it, an entry per problem with its place, its message and the
//! lines the tool quoted.
//!
//! hledger stops at the first problem (`hledger check --strict`, and the
//! checks the setting `checks` adds); Ledger (`--pedantic --explicit`)
//! and Beancount (`bean-check`) report all of them, which are sorted by
//! file and line.

use std::ops::Range;

use crate::Dialect;

/// The arguments that check `main` with `tool` (after its program).
pub fn command(tool: Dialect, main: &str, checks: &[String]) -> Vec<String> {
    match tool {
        Dialect::Hledger => {
            let mut a = vec![
                "-f".to_string(),
                main.to_string(),
                "check".to_string(),
                "--strict".to_string(),
            ];
            a.extend(
                checks
                    .iter()
                    .filter(|c| !c.is_empty() && c.chars().all(|c| c.is_ascii_alphanumeric()))
                    .cloned(),
            );
            a
        }
        // The balance report goes to standard output, unread; the
        // problems to standard error.
        Dialect::Ledger => vec![
            "-f".to_string(),
            main.to_string(),
            "--pedantic".to_string(),
            "--explicit".to_string(),
            "balance".to_string(),
        ],
        Dialect::Beancount => vec![main.to_string()],
    }
}

/// The check as the user reads it: the program and its arguments, the
/// journal by its name.
pub fn said(tool: Dialect, main: &str, checks: &[String]) -> String {
    let name = crate::journal::name(main);
    let args: Vec<String> = command(tool, main, checks)
        .into_iter()
        .map(|a| if a == main { name.to_string() } else { a })
        .collect();
    format!("{} {}", tool.checker(), args.join(" "))
}

/// A problem the tool found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Problem {
    /// The file, as the tool names it.
    pub file: Option<String>,
    /// Its line, from 1.
    pub line: Option<u32>,
    /// Its column, from 1, when the tool says.
    pub column: Option<u32>,
    /// What is wrong, a line or more.
    pub message: String,
    /// The journal's lines the tool quoted.
    pub excerpt: Vec<String>,
}

/// The problems in what `tool` said (its standard error) when the check
/// failed (`status` not 0); none when it passed.
pub fn problems(tool: Dialect, status: Option<i32>, said: &str) -> Vec<Problem> {
    if status == Some(0) {
        return Vec::new();
    }
    let mut out = match tool {
        Dialect::Hledger => hledger(said),
        Dialect::Ledger => ledger(said),
        Dialect::Beancount => beancount(said),
    };
    if out.is_empty() {
        // A failure the forms above do not know: said whole.
        let message = said.trim();
        out.push(Problem {
            message: if message.is_empty() {
                format!("{} failed, and said nothing", tool.checker())
            } else {
                message.to_string()
            },
            ..Problem::default()
        });
    }
    out.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    out
}

/// `PATH:LINE` or `PATH:LINE:COLUMN`, with a range after either
/// (`13:49-61`, `5-7`), and a colon at the end.
fn place(s: &str) -> Option<(String, u32, Option<u32>)> {
    let s = s.strip_suffix(':')?;
    let mut parts: Vec<&str> = s.rsplitn(3, ':').collect();
    parts.reverse();
    let number = |p: &str| p.split('-').next().and_then(|n| n.parse::<u32>().ok());
    match parts.as_slice() {
        [file, line, column] if number(line).is_some() && number(column).is_some() => {
            Some((file.to_string(), number(line)?, number(column)))
        }
        [a, b, line] if number(line).is_some() => Some((format!("{a}:{b}"), number(line)?, None)),
        [file, line] if number(line).is_some() => Some((file.to_string(), number(line)?, None)),
        _ => None,
    }
}

/// hledger's one problem: `hledger: Error: PATH:LINE[:COL]:`, the lines
/// it quotes (`13 |     …`), a blank line, the message.
fn hledger(said: &str) -> Vec<Problem> {
    let Some(start) = said.find("Error: ") else {
        return Vec::new();
    };
    let rest = &said[start + "Error: ".len()..];
    let mut lines = rest.lines();
    let first = lines.next().unwrap_or_default();
    let Some((file, line, column)) = place(first.trim()) else {
        return vec![Problem {
            message: rest.trim().to_string(),
            ..Problem::default()
        }];
    };
    let mut excerpt = Vec::new();
    let mut message = Vec::new();
    let mut in_excerpt = true;
    for l in lines {
        if in_excerpt {
            let quoted = l
                .trim_start()
                .split_once('|')
                .is_some_and(|(n, _)| n.trim().chars().all(|c| c.is_ascii_digit()));
            if quoted {
                excerpt.push(l.trim_end().to_string());
                continue;
            }
            if l.trim().is_empty() && !excerpt.is_empty() && l.is_empty() {
                in_excerpt = false;
                continue;
            }
            if l.trim().is_empty() {
                continue;
            }
            in_excerpt = false;
        }
        message.push(l.trim_end());
    }
    while message.last().is_some_and(|l| l.trim().is_empty()) {
        message.pop();
    }
    vec![Problem {
        file: Some(file),
        line: Some(line),
        column,
        message: message.join("\n"),
        excerpt,
    }]
}

/// Ledger's problems, each from `While parsing file "PATH", line N:` to
/// its `Error: …` line: the quoted lines (`> …`, or indented under
/// "While parsing posting:") and the explanation between.
fn ledger(said: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut current: Option<Problem> = None;
    let mut explanation: Vec<String> = Vec::new();
    for l in said.lines() {
        if let Some(rest) = l.strip_prefix("While parsing file \"") {
            let (file, after) = rest.split_once('"').unwrap_or((rest, ""));
            let line = after
                .trim_start_matches(", line ")
                .trim_end_matches(':')
                .parse::<u32>()
                .ok();
            if current.is_none() {
                current = Some(Problem {
                    file: Some(file.to_string()),
                    line,
                    ..Problem::default()
                });
                explanation.clear();
            }
            continue;
        }
        if let Some(message) = l.strip_prefix("Error: ") {
            let mut p = current.take().unwrap_or_default();
            let mut m = vec![message.to_string()];
            m.append(&mut explanation);
            p.message = m.join("\n");
            out.push(p);
            continue;
        }
        let Some(p) = current.as_mut() else {
            continue;
        };
        if l.starts_with("While ") || l.trim().is_empty() {
            continue;
        }
        if let Some(q) = l.strip_prefix("> ") {
            p.excerpt.push(q.to_string());
        } else if l.starts_with("  ") && explanation.is_empty() {
            p.excerpt.push(l.trim_start().to_string());
        } else {
            explanation.push(l.trim().to_string());
        }
    }
    out
}

/// bean-check's problems: `PATH:LINE: message`, then the entry it
/// quotes, indented, between blank lines.
fn beancount(said: &str) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    for l in said.lines() {
        if !l.starts_with(' ')
            && !l.trim().is_empty()
            && let Some((head, message)) = l.split_once(": ")
            && let Some((file, line, column)) = place(&format!("{head}:"))
        {
            out.push(Problem {
                file: Some(file),
                line: Some(line),
                column,
                message: message.trim().to_string(),
                excerpt: Vec::new(),
            });
            continue;
        }
        if let Some(p) = out.last_mut() {
            if l.trim().is_empty() {
                continue;
            }
            if l.starts_with(' ') {
                p.excerpt.push(l.trim_start().to_string());
            } else {
                p.message.push('\n');
                p.message.push_str(l.trim());
            }
        } else if !l.trim().is_empty() {
            out.push(Problem {
                message: l.trim().to_string(),
                ..Problem::default()
            });
        }
    }
    out
}

/// A color of the document's text, as `styled-documents` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// The header.
    Header,
    /// A problem's place.
    Place,
    /// The lines quoted.
    Muted,
    /// The check passed.
    Passed,
}

/// The document "Check: …": its text, its styled stretches, and for
/// each problem the bytes of its entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub text: String,
    pub styles: Vec<(Range<usize>, Color)>,
    pub entries: Vec<(Range<usize>, Problem)>,
}

/// `file` as the document shows it: beside the main journal by its name,
/// elsewhere whole (macOS's `/private/tmp` taken as `/tmp`).
fn shown(file: &str, main: &str) -> String {
    let plain = |p: &str| {
        p.strip_prefix("/private")
            .filter(|r| r.starts_with("/tmp/") || r.starts_with("/var/"))
            .unwrap_or(p)
            .to_string()
    };
    let (file, dir) = (plain(file), plain(crate::journal::folder(main)));
    match file.strip_prefix(&format!("{dir}/")) {
        Some(rel) if !dir.is_empty() => rel.to_string(),
        _ => file,
    }
}

/// The report of `problems` found by `check` (as [`said`] puts it) in
/// `main`.
pub fn report(check: &str, main: &str, problems: &[Problem]) -> Report {
    let mut r = Report::default();
    let n = problems.len();
    let header = match n {
        0 => format!("✓ {}: no problems", crate::journal::name(main)),
        1 => format!("{}: 1 problem", crate::journal::name(main)),
        _ => format!("{}: {n} problems", crate::journal::name(main)),
    };
    r.styles.push((
        0..header.len(),
        if n == 0 { Color::Passed } else { Color::Header },
    ));
    r.text.push_str(&header);
    r.text.push('\n');
    let said = format!("{check}\n");
    let s = r.text.len();
    r.text.push_str(&said);
    r.styles.push((s..s + said.len() - 1, Color::Muted));
    for p in problems {
        r.text.push('\n');
        let start = r.text.len();
        let place = match (&p.file, p.line, p.column) {
            (Some(f), Some(l), Some(c)) => format!("{}:{l}:{c}", shown(f, main)),
            (Some(f), Some(l), None) => format!("{}:{l}", shown(f, main)),
            (Some(f), None, _) => shown(f, main),
            (None, ..) => String::new(),
        };
        if !place.is_empty() {
            r.text.push_str(&place);
            r.styles.push((start..start + place.len(), Color::Place));
            r.text.push('\n');
        }
        for m in p.message.lines() {
            r.text.push_str("  ");
            r.text.push_str(m);
            r.text.push('\n');
        }
        for q in &p.excerpt {
            let s = r.text.len();
            r.text.push_str("    ");
            r.text.push_str(q);
            r.styles.push((s..r.text.len(), Color::Muted));
            r.text.push('\n');
        }
        r.entries.push((start..r.text.len(), p.clone()));
    }
    r
}

/// The status bar item's mark for `problems`: ✓, or their count.
pub fn mark(problems: usize) -> String {
    match problems {
        0 => "✓".to_string(),
        1 => "1 problem".to_string(),
        n => format!("{n} problems"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HLEDGER: &str = "hledger: Error: /private/tmp/m/hledger/errors.journal:13:49:\n   | 2026-02-02 * Market | an assertion off by one lira\n   |     expenses:food:market      100,00 TRY\n13 |     assets:bank:ziraat       -100,00 TRY = 49.901,00 TRY\n   |                                          ^^^^^^^^^^^^^^^\n\nBalance assertion failed in assets:bank:ziraat\nIn commodity TRY at this point, excluding subaccounts, ignoring costs,\nthe asserted balance is:        49.901,00 TRY\nbut the calculated balance is:  49.900,00 TRY\n(difference: 1,00 TRY)\n";

    const HLEDGER_ORDER: &str = "hledger: Error: /tmp/m/two.journal:8:\n4 | 2026-03-02 * Shop\n  |     expenses:food:market       10,00 TRY\n \n8 | 2026-03-01 * Undeclared\n  | ^^^^^^^^^^\n\nOrdered dates checking is enabled, and this transaction's\ndate (2026-03-01) is out of order with the previous transaction.\n";

    const LEDGER: &str = "While parsing file \"/private/tmp/m/two.ledger\", line 7:\nWhile balancing transaction from \"/private/tmp/m/two.ledger\", lines 5-7:\n> 2026/03/01 * One\n>     Expenses:Software  $49.00\n>     Assets:Checking  $-48.00\nUnbalanced remainder is:\n               $1.00\nAmount to balance against:\n              $49.00\nError: Transaction does not balance\nWhile parsing file \"/private/tmp/m/two.ledger\", line 10:\nWhile parsing posting:\n  Expenses:Unknown  $5.00\n\nError: Unknown account 'Expenses:Unknown'\n";

    const BEANCOUNT: &str = "/private/tmp/m/two.beancount:12: Balance failed for 'Assets:Cash': expected 5.00 TRY != accumulated -9.00 TRY (14.00 too little)\n\n   2026-01-17 balance Assets:Cash                                     5.00 TRY\n\n\n/private/tmp/m/two.beancount:4: Invalid reference to unknown account 'Expenses:Food'\n\n   2026-01-15 * \"A\" \"unknown\"\n     Expenses:Food   10.00 TRY\n     Assets:Cash    -10.00 TRY\n\n\n";

    #[test]
    fn the_commands() {
        let checks = vec!["ordereddates".to_string(), "rm -rf".to_string()];
        assert_eq!(
            command(Dialect::Hledger, "/j/2026.journal", &checks),
            ["-f", "/j/2026.journal", "check", "--strict", "ordereddates"]
        );
        assert_eq!(
            said(Dialect::Ledger, "/j/b.ledger", &[]),
            "ledger -f b.ledger --pedantic --explicit balance"
        );
        assert_eq!(
            said(Dialect::Beancount, "/j/h.beancount", &[]),
            "bean-check h.beancount"
        );
    }

    #[test]
    fn hledgers_problem() {
        let p = problems(Dialect::Hledger, Some(1), HLEDGER);
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0].file.as_deref(),
            Some("/private/tmp/m/hledger/errors.journal")
        );
        assert_eq!((p[0].line, p[0].column), (Some(13), Some(49)));
        assert!(
            p[0].message
                .starts_with("Balance assertion failed in assets:bank:ziraat\n")
        );
        assert!(p[0].message.ends_with("(difference: 1,00 TRY)"));
        assert_eq!(p[0].excerpt.len(), 4);
        assert!(p[0].excerpt[2].starts_with("13 |"));
        let p = problems(Dialect::Hledger, Some(1), HLEDGER_ORDER);
        assert_eq!((p[0].line, p[0].column), (Some(8), None));
        assert!(p[0].message.starts_with("Ordered dates checking"));
        assert_eq!(p[0].excerpt.len(), 4);
        // No place: the message whole.
        let p = problems(
            Dialect::Hledger,
            Some(1),
            "hledger: Error: data file \"/x.journal\" was not found.\nPlease create it first.\n",
        );
        assert_eq!(p[0].file, None);
        assert!(p[0].message.starts_with("data file"));
    }

    #[test]
    fn ledgers_problems() {
        let p = problems(Dialect::Ledger, Some(2), LEDGER);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].line, Some(7));
        assert!(
            p[0].message
                .starts_with("Transaction does not balance\nUnbalanced remainder is:")
        );
        assert_eq!(p[0].excerpt.len(), 3);
        assert_eq!(p[1].line, Some(10));
        assert_eq!(p[1].message, "Unknown account 'Expenses:Unknown'");
        assert_eq!(p[1].excerpt, ["Expenses:Unknown  $5.00"]);
    }

    #[test]
    fn beancounts_problems_in_line_order() {
        let p = problems(Dialect::Beancount, Some(1), BEANCOUNT);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].line, Some(4));
        assert_eq!(
            p[0].message,
            "Invalid reference to unknown account 'Expenses:Food'"
        );
        assert_eq!(p[0].excerpt.len(), 3);
        assert_eq!(p[1].line, Some(12));
        assert!(problems(Dialect::Beancount, Some(0), "").is_empty());
        // A failure in no known form: said whole.
        let p = problems(Dialect::Ledger, Some(1), "Segmentation fault\n");
        assert_eq!(p[0].message, "Segmentation fault");
    }

    #[test]
    fn the_report() {
        let p = problems(Dialect::Hledger, Some(1), HLEDGER);
        let main = "/tmp/m/hledger/2026.journal";
        let r = report(&said(Dialect::Hledger, main, &[]), main, &p);
        assert!(r.text.starts_with("2026.journal: 1 problem\nhledger -f 2026.journal check --strict\n\nerrors.journal:13:49\n  Balance assertion failed"), "{}", r.text);
        assert!(r.text.contains("\n    13 |     assets:bank:ziraat"));
        assert_eq!(r.entries.len(), 1);
        let (range, problem) = &r.entries[0];
        assert!(r.text[range.clone()].starts_with("errors.journal:13:49\n"));
        assert_eq!(problem.line, Some(13));
        let ok = report("bean-check h.beancount", "/j/h.beancount", &[]);
        assert_eq!(
            ok.text,
            "✓ h.beancount: no problems\nbean-check h.beancount\n"
        );
        assert_eq!(ok.styles[0].1, Color::Passed);
        assert_eq!(mark(0), "✓");
        assert_eq!(mark(3), "3 problems");
    }
}
