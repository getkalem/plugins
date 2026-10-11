//! The other reports (PT7c): the tool's own reports beyond the Balance
//! and the Register, one pick (`SPC m r`, "Report…"), each shown as the
//! tool prints it. hledger's cash flow, income statement, budget,
//! forecast, activity, statistics and lists; Ledger's income statement,
//! budget, forecast, cleared report, statistics and lists; Beancount's
//! through `bean-query`, the journal's saved queries (its `query`
//! directives, which `bean-query`'s `.run` lists) first. The setting
//! `reports` pins reports of the user's own, `NAME: ARGUMENTS`, first
//! of all.

use crate::Dialect;
use crate::write::Date;

/// A report: its title, the tool's arguments after the journal's, and
/// how its output reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub title: String,
    pub args: Vec<String>,
    /// Its output is journal syntax (`print`): highlighted as the
    /// journal.
    pub journal: bool,
    /// It takes the period's dates (`-b`, `-e`).
    pub dated: bool,
    /// Asked as CSV, for a table ([`as_table`]).
    pub csv: bool,
}

impl Report {
    fn of(title: &str, args: &[&str], dated: bool) -> Report {
        Report {
            title: title.into(),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            journal: args.first() == Some(&"print") || args.first() == Some(&"PRINT"),
            dated,
            csv: false,
        }
    }
}

/// Beancount's income statement, as `bean-query` asks it.
const BEAN_INCOME: &str = "SELECT account, sum(position) AS amount WHERE account ~ '^(Income|Expenses)' GROUP BY account ORDER BY account";
const BEAN_HOLDINGS: &str = "SELECT account, units(sum(position)) AS units, cost(sum(position)) AS cost, value(sum(position)) AS value WHERE account ~ '^(Assets|Liabilities)' GROUP BY account ORDER BY account";
const BEAN_MONTHLY: &str = "SELECT year, month, root(account, 2) AS account, sum(position) AS amount WHERE account ~ '^Expenses' GROUP BY year, month, account ORDER BY year, month, account";

/// The tool's reports, in the pick's order. Ledger's forecast runs from
/// `today` to six months on.
pub fn reports(tool: Dialect, today: Date) -> Vec<Report> {
    match tool {
        Dialect::Hledger => vec![
            Report::of("Cash Flow", &["cashflow"], true),
            Report::of("Income Statement", &["incomestatement"], true),
            Report::of("Budget, Monthly", &["balance", "--budget", "-M"], true),
            Report::of("Forecast, Monthly", &["balance", "-M", "--forecast"], true),
            Report::of("Activity, Monthly", &["activity", "-M"], true),
            Report::of("Statistics", &["stats"], true),
            Report::of("Accounts", &["accounts", "--tree"], false),
            Report::of("Commodities", &["commodities"], false),
            Report::of("Payees", &["payees"], false),
            Report::of("Tags", &["tags"], false),
            // The costs' prices too, as Ledger lists them.
            Report::of("Prices", &["prices", "--infer-market-prices"], true),
            Report::of("Transactions", &["print"], true),
        ],
        Dialect::Ledger => {
            let end = format!("d<[{}]", today.plus(183).written('/'));
            let begin = today.written('/');
            vec![
                Report::of(
                    "Income Statement",
                    &["balance", "^Income", "^Expenses"],
                    true,
                ),
                Report::of("Budget", &["budget"], true),
                Report::of(
                    "Forecast",
                    &["register", "--forecast", &end, "-b", &begin],
                    false,
                ),
                Report::of("Cleared", &["cleared"], true),
                Report::of("Statistics", &["stats"], true),
                Report::of("Accounts", &["accounts"], false),
                Report::of("Commodities", &["commodities"], false),
                Report::of("Payees", &["payees"], false),
                Report::of("Tags", &["tags"], false),
                Report::of("Prices", &["prices"], true),
                Report::of("Transactions", &["print"], true),
            ]
        }
        Dialect::Beancount => vec![
            Report::of("Income Statement", &[BEAN_INCOME], false),
            Report::of("Holdings at Cost and Value", &[BEAN_HOLDINGS], false),
            Report::of("Expenses, Monthly", &[BEAN_MONTHLY], false),
            Report::of("Balances", &["BALANCES"], false),
            Report::of(
                "Commodities",
                &["SELECT DISTINCT currency ORDER BY currency"],
                false,
            ),
            Report::of("Payees", &["SELECT DISTINCT payee ORDER BY payee"], false),
            Report::of(
                "Prices",
                &["SELECT date, currency, amount FROM #prices ORDER BY date"],
                false,
            ),
            Report::of("Transactions", &["PRINT"], false),
        ],
    }
}

/// The journal's saved queries, by the names `bean-query`'s `.run`
/// lists, as reports.
pub fn saved_queries(listed: &str) -> Vec<Report> {
    listed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains(char::is_whitespace))
        .map(|name| Report {
            title: format!("Query: {name}"),
            args: vec![format!(".run {name}")],
            journal: false,
            dated: false,
            csv: false,
        })
        .collect()
}

/// `bean-query`'s listing of the saved queries.
pub fn saved_queries_args(main: &str) -> Vec<String> {
    vec![main.into(), ".run".into()]
}

/// The words of `s` as a shell splits them: blanks between, quotes
/// (`'…'`, `"…"`) keeping blanks, a backslash keeping the next.
pub fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    word.push(n);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// The setting `reports`' entries, `NAME: ARGUMENTS` (`Food by month:
/// balance expenses:food -M`; for Beancount, a query: `Food:
/// SELECT …`), as reports: hledger's and Ledger's arguments split as a
/// shell splits them, dated unless they give dates themselves;
/// Beancount's query whole.
pub fn pinned(tool: Dialect, entries: &[String]) -> Vec<Report> {
    entries
        .iter()
        .filter_map(|e| {
            let (name, args) = e.split_once(": ")?;
            let (name, args) = (name.trim(), args.trim());
            if name.is_empty() || args.is_empty() {
                return None;
            }
            let args = if tool == Dialect::Beancount {
                vec![args.to_string()]
            } else {
                words(args)
            };
            let dates = ["-b", "-e", "-p", "--begin", "--end", "--period"];
            let dated = tool != Dialect::Beancount
                && !args.iter().any(|a| {
                    dates
                        .iter()
                        .any(|d| a == d || a.starts_with(&format!("{d}=")))
                });
            Some(Report {
                title: name.to_string(),
                journal: matches!(args.first().map(String::as_str), Some("print" | "PRINT")),
                args,
                dated,
                csv: false,
            })
        })
        .collect()
}

/// The program and arguments of `r` on `main`, within `bounds` when it
/// takes dates.
pub fn command(
    tool: Dialect,
    main: &str,
    r: &Report,
    bounds: Option<(Date, Date)>,
) -> (&'static str, Vec<String>) {
    match tool {
        Dialect::Beancount => {
            let mut a = if r.csv {
                vec!["-f".to_string(), "csv".to_string()]
            } else {
                Vec::new()
            };
            a.push(main.to_string());
            a.extend(r.args.iter().cloned());
            ("bean-query", a)
        }
        _ => {
            let mut a = vec!["-f".to_string(), main.to_string()];
            a.extend(r.args.iter().cloned());
            if r.dated
                && let Some((begin, end)) = bounds
            {
                let sep = if tool == Dialect::Ledger { '/' } else { '-' };
                a.extend([
                    "-b".into(),
                    begin.written(sep),
                    "-e".into(),
                    end.written(sep),
                ]);
            }
            (tool.checker(), a)
        }
    }
}

/// Ledger's balance as CSV: each account and its amounts, the lots'
/// annotations scrubbed.
const LEDGER_CSV: &str = "%(quoted(account)),%(quoted(scrub(display_total)))\n";

/// `r` asked for CSV, where the tool writes it as a table: hledger's
/// balance reports, register and `print` with `-O csv`; Ledger's balance
/// through a CSV format, its register and `print` as its `csv` command;
/// Beancount's queries with `-f csv` (not `PRINT`). `None` for a report
/// that is no table (statistics, the lists, Ledger's budget).
pub fn as_table(tool: Dialect, r: &Report) -> Option<Report> {
    let first = r.args.first()?.as_str();
    let mut t = r.clone();
    t.csv = true;
    t.journal = false;
    match tool {
        Dialect::Hledger => {
            const TABLES: &[&str] = &[
                "balance",
                "bal",
                "incomestatement",
                "is",
                "cashflow",
                "cf",
                "balancesheet",
                "bs",
                "balancesheetequity",
                "bse",
                "register",
                "reg",
                "aregister",
                "areg",
                "print",
            ];
            if !TABLES.contains(&first) {
                return None;
            }
            t.args.extend(["-O".into(), "csv".into()]);
        }
        Dialect::Ledger => match first {
            "balance" | "bal" => t.args.extend([
                "--flat".into(),
                "--no-total".into(),
                "--balance-format".into(),
                LEDGER_CSV.into(),
            ]),
            "register" | "reg" | "print" => t.args[0] = "csv".into(),
            "csv" => {}
            _ => return None,
        },
        Dialect::Beancount => {
            if first.trim().eq_ignore_ascii_case("print") {
                return None;
            }
        }
    }
    Some(t)
}

/// The header Ledger does not write for a table of `r` (its balance's,
/// its `csv` command's).
pub fn header(tool: Dialect, r: &Report) -> Option<&'static str> {
    if tool != Dialect::Ledger {
        return None;
    }
    match r.args.first()?.as_str() {
        "balance" | "bal" => Some("\"Account\",\"Amount\"\n"),
        "csv" => Some(
            "\"Date\",\"Code\",\"Payee\",\"Account\",\"Commodity\",\"Amount\",\"Status\",\"Note\"\n",
        ),
        _ => None,
    }
}

/// The rows of CSV `text`: fields split at commas outside quotes, `""`
/// a quote, a line feed inside quotes kept.
pub fn csv_rows(text: &str) -> Vec<Vec<String>> {
    csv_rows_with(text, ',')
}

/// The rows of `text` with fields split at `sep` (`;` and tabs too, as
/// bank statements have them).
pub fn csv_rows_with(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, c) if c == sep => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// A table's markup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markup {
    Org,
    Markdown,
}

/// The table of CSV `text` in `markup`, its columns aligned: the first
/// row its header, under a rule. A first row of one title (hledger's
/// "Income Statement 2026-01") is returned apart, as the caption. A
/// field's line feeds become `, ` (Ledger's amounts of several
/// commodities), its `|` the markup's escape.
pub fn table(text: &str, markup: Markup) -> (Option<String>, String) {
    let clean = |f: &str| {
        let f = f.trim().replace('\n', ", ");
        match markup {
            Markup::Org => f.replace('|', "\\vert{}"),
            Markup::Markdown => f.replace('|', "\\|"),
        }
    };
    let mut rows: Vec<Vec<String>> = csv_rows(text)
        .into_iter()
        .map(|r| r.iter().map(|f| clean(f)).collect())
        .collect();
    let mut caption = None;
    if rows.len() > 1 && rows[0].len() > 1 && rows[0][1..].iter().all(String::is_empty) {
        caption = Some(rows.remove(0).remove(0));
    }
    let n = rows.iter().map(Vec::len).max().unwrap_or(0);
    if n == 0 {
        return (caption, String::new());
    }
    let mut widths = vec![1; n];
    for r in &rows {
        for (i, f) in r.iter().enumerate() {
            widths[i] = widths[i].max(f.chars().count());
        }
    }
    let line = |r: &[String]| {
        let cells: Vec<String> = (0..n)
            .map(|i| {
                let f = r.get(i).map_or("", String::as_str);
                format!("{f}{}", " ".repeat(widths[i] - f.chars().count()))
            })
            .collect();
        format!("| {} |\n", cells.join(" | "))
    };
    let rule = {
        let dashes: Vec<String> = widths.iter().map(|w| "-".repeat(w + 2)).collect();
        match markup {
            Markup::Org => format!("|{}|\n", dashes.join("+")),
            Markup::Markdown => format!("|{}|\n", dashes.join("|")),
        }
    };
    let mut out = String::new();
    for (i, r) in rows.iter().enumerate() {
        out.push_str(&line(r));
        if i == 0 {
            out.push_str(&rule);
        }
    }
    (caption, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: Date = Date {
        y: 2026,
        m: 10,
        d: 11,
    };

    #[test]
    fn each_tools_reports() {
        for tool in Dialect::ALL {
            let r = reports(tool, TODAY);
            assert!(r.len() >= 8, "{tool:?}");
            assert_eq!(r.iter().filter(|r| r.journal).count(), 1, "{tool:?}");
        }
        let l = reports(Dialect::Ledger, TODAY);
        let f = l.iter().find(|r| r.title == "Forecast").unwrap();
        assert_eq!(
            f.args,
            [
                "register",
                "--forecast",
                "d<[2027/04/12]",
                "-b",
                "2026/10/11"
            ]
        );
    }

    #[test]
    fn a_command() {
        let h = &reports(Dialect::Hledger, TODAY)[0];
        let jan = Some((
            Date {
                y: 2026,
                m: 1,
                d: 1,
            },
            Date {
                y: 2026,
                m: 2,
                d: 1,
            },
        ));
        assert_eq!(
            command(Dialect::Hledger, "/j/2026.journal", h, jan).1,
            [
                "-f",
                "/j/2026.journal",
                "cashflow",
                "-b",
                "2026-01-01",
                "-e",
                "2026-02-01"
            ]
        );
        let b = &reports(Dialect::Beancount, TODAY)[3];
        assert_eq!(
            command(Dialect::Beancount, "/j/h.beancount", b, jan),
            (
                "bean-query",
                vec!["/j/h.beancount".into(), "BALANCES".into()]
            )
        );
    }

    #[test]
    fn pinned_reports() {
        let p = pinned(
            Dialect::Hledger,
            &[
                "Food by month: balance 'expenses:food' -M".into(),
                "Q1: balance -p 2026Q1".into(),
                "Journal of rent: print expenses:rent".into(),
                "no colon".into(),
            ],
        );
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].args, ["balance", "expenses:food", "-M"]);
        assert!(p[0].dated && !p[1].dated && p[2].journal);
        let b = pinned(
            Dialect::Beancount,
            &["Cash: SELECT account WHERE account ~ 'Cash'".into()],
        );
        assert_eq!(b[0].args, ["SELECT account WHERE account ~ 'Cash'"]);
        assert_eq!(words(r#"a "b c" 'd e'f g\ h"#), ["a", "b c", "d ef", "g h"]);
        assert_eq!(
            saved_queries("cash\nfood-by-month\n")
                .iter()
                .map(|r| r.args[0].as_str())
                .collect::<Vec<_>>(),
            [".run cash", ".run food-by-month"]
        );
    }

    #[test]
    fn tables() {
        let h = reports(Dialect::Hledger, TODAY);
        let is = as_table(Dialect::Hledger, &h[1]).expect("a table");
        assert_eq!(is.args, ["incomestatement", "-O", "csv"]);
        assert!(as_table(Dialect::Hledger, &h[5]).is_none(), "stats");
        let l = reports(Dialect::Ledger, TODAY);
        let f = as_table(Dialect::Ledger, &l[2]).expect("the forecast");
        assert_eq!(f.args[0], "csv");
        assert_eq!(
            header(Dialect::Ledger, &f).map(|h| h.starts_with("\"Date\"")),
            Some(true)
        );
        let b = reports(Dialect::Beancount, TODAY);
        let q = as_table(Dialect::Beancount, &b[0]).expect("a query");
        assert_eq!(
            command(Dialect::Beancount, "/j/h.beancount", &q, None).1[..3],
            ["-f", "csv", "/j/h.beancount"]
        );
        assert!(
            as_table(Dialect::Beancount, b.last().unwrap()).is_none(),
            "PRINT"
        );

        let csv = "\"Income Statement 2026-01\",\"\"\n\"Account\",\"2026-01\"\n\"income:salary\",\"45000,00 TRY\"\n\"Expenses\",\"$49.00\nEUR 120.00\"\n\"a|b\",\"1\"\n";
        let (caption, org) = table(csv, Markup::Org);
        assert_eq!(caption.as_deref(), Some("Income Statement 2026-01"));
        assert_eq!(
            org,
            "| Account       | 2026-01            |\n\
             |---------------+--------------------|\n\
             | income:salary | 45000,00 TRY       |\n\
             | Expenses      | $49.00, EUR 120.00 |\n\
             | a\\vert{}b     | 1                  |\n"
        );
        let (_, md) = table("a,b\n1,2\n", Markup::Markdown);
        assert_eq!(md, "| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert_eq!(csv_rows("\"x \"\"y\"\"\",z"), [["x \"y\"", "z"]]);
    }
}
