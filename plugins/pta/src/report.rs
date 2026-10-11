//! The balance (PT7a): the tool's balance report of the main journal as
//! the document "Balance: …", an account tree folded and unfolded by
//! account. Two sections: assets, liabilities and equity as their
//! balances at the period's end, the other accounts (income, expenses,
//! budgets) as their changes within it. **The tool computes every
//! amount**, each account's with its subaccounts' and each section's
//! total: hledger's and Ledger's balance reports (`--format` with the
//! tree's depth and the totals), Beancount's `bean-query` at each level
//! of the tree (`root(account, n)`); the plugin only reads them, writes
//! them as the tool wrote them (its decimal mark, digit groups and
//! commodity side), and lays them out.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::Dialect;
use crate::write::Date;

/// An account's line of a report, or a total (no account).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    /// Its full name; empty for a total.
    pub account: String,
    /// Its amounts, one per commodity, as the tool wrote them.
    pub amounts: Vec<String>,
}

/// A period of the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    All,
    ThisMonth,
    LastMonth,
    ThisQuarter,
    ThisYear,
    LastYear,
}

impl Period {
    pub const ALL: [Period; 6] = [
        Period::ThisMonth,
        Period::LastMonth,
        Period::ThisQuarter,
        Period::ThisYear,
        Period::LastYear,
        Period::All,
    ];

    /// The period named `name` as the pick names it (`this month`, `last
    /// year`, `all dates`; `all` too), for a command's arguments.
    pub fn named(name: &str) -> Option<Period> {
        let n = name.trim().to_lowercase();
        if n == "all" {
            return Some(Period::All);
        }
        Period::ALL.into_iter().find(|p| p.name() == n)
    }

    /// Its name in the pick and the header.
    pub fn name(self) -> &'static str {
        match self {
            Period::All => "all dates",
            Period::ThisMonth => "this month",
            Period::LastMonth => "last month",
            Period::ThisQuarter => "this quarter",
            Period::ThisYear => "this year",
            Period::LastYear => "last year",
        }
    }

    /// Its first day and the day after its last; none for all dates.
    pub fn bounds(self, today: Date) -> Option<(Date, Date)> {
        let month = |y: i32, m: u32| Date { y, m, d: 1 };
        let next = |d: Date| {
            if d.m == 12 {
                month(d.y + 1, 1)
            } else {
                month(d.y, d.m + 1)
            }
        };
        let this = month(today.y, today.m);
        Some(match self {
            Period::All => return None,
            Period::ThisMonth => (this, next(this)),
            Period::LastMonth => (this.plus(-1).with_day(1), this),
            Period::ThisQuarter => {
                let q = month(today.y, (today.m - 1) / 3 * 3 + 1);
                (q, next(next(next(q))))
            }
            Period::ThisYear => (month(today.y, 1), month(today.y + 1, 1)),
            Period::LastYear => (month(today.y - 1, 1), month(today.y, 1)),
        })
    }
}

impl Date {
    fn with_day(self, d: u32) -> Date {
        Date { d, ..self }
    }
}

/// The report's two sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Assets, liabilities and equity: balances at the period's end.
    Sheet,
    /// The others: changes within the period.
    Flow,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Sheet => "Assets, liabilities and equity",
            Section::Flow => "Income, expenses and the others",
        }
    }
}

/// The hledger and Ledger formats: an account's depth (hledger's tree
/// indent) or full name, a bar, its total; the total's line with no
/// account.
const HLEDGER_FORMAT: &str = "%2(depth_spacer)%(account)|%(total)";
const LEDGER_FORMAT: &str = "%(account)|%(scrub(display_total))\\n";

/// Beancount's root accounts of the sheet's section.
const SHEET_ROOTS: &str = "Assets|Liabilities|Equity";

/// The tool's run for `section` of `main` within `bounds`, `depth`
/// levels deep (0 for all); for Beancount, the query of tree level
/// `level` (from 1), or of the total with `level` 0.
pub fn command(
    tool: Dialect,
    main: &str,
    section: Section,
    bounds: Option<(Date, Date)>,
    depth: usize,
    level: usize,
) -> (&'static str, Vec<String>) {
    let mut a: Vec<String> = Vec::new();
    let dates = |a: &mut Vec<String>| {
        if let Some((begin, end)) = bounds {
            if section == Section::Flow {
                a.extend(["-b".into(), begin.written('-')]);
            }
            a.extend(["-e".into(), end.written('-')]);
        }
    };
    match tool {
        Dialect::Hledger => {
            a.extend([
                "-f".into(),
                main.into(),
                "balance".into(),
                "--tree".into(),
                "--no-elide".into(),
            ]);
            a.extend(["--format".into(), HLEDGER_FORMAT.into()]);
            dates(&mut a);
            if depth > 0 {
                a.push(format!("--depth={depth}"));
            }
            a.push(match section {
                Section::Sheet => "type:ALE".into(),
                Section::Flow => "not:type:ALE".into(),
            });
            ("hledger", a)
        }
        Dialect::Ledger => {
            a.extend(["-f".into(), main.into(), "balance".into()]);
            a.extend(["--balance-format".into(), LEDGER_FORMAT.into()]);
            dates(&mut a);
            if depth > 0 {
                a.push(format!("--depth={depth}"));
            }
            match section {
                Section::Sheet => {
                    a.extend(["^Assets".into(), "^Liabilities".into(), "^Equity".into()])
                }
                Section::Flow => a.extend([
                    "not".into(),
                    "(".into(),
                    "^Assets".into(),
                    "or".into(),
                    "^Liabilities".into(),
                    "or".into(),
                    "^Equity".into(),
                    ")".into(),
                ]),
            }
            ("ledger", a)
        }
        Dialect::Beancount => {
            let mut wh = vec![match section {
                Section::Sheet => format!("account ~ '^({SHEET_ROOTS})(:|$)'"),
                Section::Flow => format!("NOT account ~ '^({SHEET_ROOTS})(:|$)'"),
            }];
            if let Some((begin, end)) = bounds {
                if section == Section::Flow {
                    wh.push(format!("date >= {}", begin.written('-')));
                }
                wh.push(format!("date < {}", end.written('-')));
            }
            let wh = wh.join(" AND ");
            let q = if level == 0 {
                format!("SELECT units(sum(position)) AS b WHERE {wh}")
            } else {
                format!(
                    "SELECT root(account, {level}) AS a, units(sum(position)) AS b WHERE {wh} GROUP BY a ORDER BY a"
                )
            };
            (
                "bean-query",
                vec!["-f".into(), "csv".into(), main.into(), q],
            )
        }
    }
}

/// A balance's or register's run made to give market values (PT10):
/// hledger's `-V` (its prices, and the costs of the journal's
/// transactions as prices: `--infer-market-prices`), Ledger's `-V`,
/// Beancount's `value()` of the sums and positions.
pub fn value_args(tool: Dialect, args: &mut [String]) -> Vec<String> {
    match tool {
        Dialect::Hledger => {
            let mut a = args.to_vec();
            a.extend(["-V".into(), "--infer-market-prices".into()]);
            a
        }
        Dialect::Ledger => {
            let mut a = args.to_vec();
            a.push("-V".into());
            a
        }
        Dialect::Beancount => args
            .iter()
            .map(|a| {
                a.replace("units(sum(position))", "value(sum(position))")
                    .replace("position, balance,", "value(position), value(balance),")
            })
            .collect(),
    }
}

/// hledger's rows: `INDENT name|amount`, more amounts on the lines
/// after; the total after a line of dashes, with no name. Full names
/// from the indents (two blanks a level).
pub fn hledger_rows(out: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with("---") {
            continue;
        }
        let Some(bar) = line.rfind('|') else {
            if let Some(r) = rows.last_mut() {
                r.amounts.push(line.trim().to_string());
            }
            continue;
        };
        let (left, amount) = (&line[..bar], line[bar + 1..].trim());
        let name = left.trim_start();
        let account = if name.is_empty() {
            String::new()
        } else {
            let depth = (left.len() - name.len()) / 2;
            path.truncate(depth);
            path.push(name.to_string());
            path.join(":")
        };
        rows.push(Row {
            account,
            amounts: amounts_of(amount),
        });
    }
    rows
}

/// Ledger's rows: `Full:Name|amount`, more amounts on the lines after;
/// the total with no name.
pub fn ledger_rows(out: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match line.rfind('|') {
            Some(bar) => rows.push(Row {
                account: line[..bar].trim().to_string(),
                amounts: amounts_of(line[bar + 1..].trim()),
            }),
            None => {
                if let Some(r) = rows.last_mut() {
                    r.amounts.push(line.trim().to_string());
                }
            }
        }
    }
    rows
}

/// `bean-query -f csv`'s rows of one level (`a,b`), or the total (`b`):
/// the inventory's commodities, padded and empty ones left out.
pub fn beancount_rows(csv: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for line in csv.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let (account, inventory) = match line.split_once(",\"") {
            Some((a, rest)) => (a.trim().trim_matches('"'), rest.trim_end_matches('"')),
            None if line.starts_with('"') => ("", line.trim_matches('"')),
            None => match line.split_once(',') {
                Some((a, b)) => (a.trim(), b),
                None => ("", line),
            },
        };
        rows.push(Row {
            account: account.to_string(),
            amounts: inventory
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        });
    }
    rows
}

fn amounts_of(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        Vec::new()
    } else {
        vec![s.to_string()]
    }
}

/// The rows of every level Beancount gave (each level's from its own
/// query), each account once, parents before children.
pub fn beancount_tree(levels: &[Vec<Row>]) -> Vec<Row> {
    let mut seen = BTreeSet::new();
    let mut out: Vec<Row> = Vec::new();
    for level in levels {
        for r in level {
            if !r.account.is_empty() && seen.insert(r.account.clone()) {
                out.push(r.clone());
            }
        }
    }
    out.sort_by(|a, b| a.account.split(':').cmp(b.account.split(':')));
    out
}

/// A section as the tool gave it: its accounts, and its total.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Part {
    pub rows: Vec<Row>,
    pub total: Vec<String>,
}

impl Part {
    /// The rows a tool gave, its total (the row without an account) taken
    /// apart.
    pub fn of(rows: Vec<Row>) -> Part {
        let (total, rows): (Vec<Row>, Vec<Row>) =
            rows.into_iter().partition(|r| r.account.is_empty());
        Part {
            rows,
            total: total
                .into_iter()
                .next_back()
                .map(|r| r.amounts)
                .unwrap_or_default(),
        }
    }
}

/// A line's style in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Header,
    Hint,
    Section,
    Mark,
    Negative,
    Total,
    /// To look at: a posting the statement does not have.
    Warning,
}

/// The balance document: its text, styles, and the account of each line
/// (none on other lines), and which accounts have children.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub text: String,
    pub styles: Vec<(Range<usize>, Style)>,
    pub lines: Vec<Option<String>>,
    pub parents: BTreeSet<String>,
}

/// Whether an amount, as written, is below zero: a minus before its
/// number (`-1.120,00 TRY`, `$-400.00`, `-$5`).
pub fn negative(amount: &str) -> bool {
    let b = amount.as_bytes();
    b.iter().enumerate().any(|(i, c)| {
        *c == b'-'
            && b.get(i + 1)
                .is_some_and(|n| n.is_ascii_digit() || !n.is_ascii())
    }) || amount.trim_start().starts_with("-$")
}

/// The column the amounts end at: the longest name with its indent and
/// mark, and the longest amount, apart by two.
const MIN_COLUMN: usize = 40;

/// Lays out `parts` (the sections in order) with `folded` accounts'
/// children hidden: a header, then each section's title, its accounts as
/// a tree (`▾` unfolded, `▸` folded, two blanks a level; `glyphs`'
/// marks), their amounts right-aligned (one line per commodity), and its
/// total.
pub fn layout(
    header: &str,
    hint: &str,
    parts: &[(Section, String, Part)],
    folded: &BTreeSet<String>,
    glyphs: crate::Glyphs,
) -> Document {
    let mut doc = Document::default();
    let push = |doc: &mut Document, s: &str, style: Option<Style>, account: Option<String>| {
        let start = doc.text.len();
        doc.text.push_str(s);
        if let Some(st) = style {
            doc.styles.push((start..doc.text.len(), st));
        }
        doc.text.push('\n');
        doc.lines.push(account);
    };
    push(&mut doc, header, Some(Style::Header), None);
    push(&mut doc, hint, Some(Style::Hint), None);
    // The widths: every section's, so that all amounts end at one column.
    let mut name_width = 0;
    let mut amount_width = 0;
    for (_, _, part) in parts {
        for r in &part.rows {
            let depth = depth_of(&part.rows, &r.account);
            name_width =
                name_width.max(2 * depth + 2 + shown_name(&part.rows, &r.account).chars().count());
            for a in &r.amounts {
                amount_width = amount_width.max(a.chars().count());
            }
        }
        for a in &part.total {
            amount_width = amount_width.max(a.chars().count());
        }
    }
    let column = (name_width + 2 + amount_width).max(MIN_COLUMN);
    for (section, title, part) in parts {
        push(&mut doc, "", None, None);
        let _ = section;
        push(&mut doc, title, Some(Style::Section), None);
        if part.rows.is_empty() {
            push(&mut doc, "  (no accounts)", Some(Style::Hint), None);
            continue;
        }
        let parents: BTreeSet<String> = part
            .rows
            .iter()
            .filter_map(|r| parent_of(&part.rows, &r.account))
            .collect();
        doc.parents.extend(parents.iter().cloned());
        for r in &part.rows {
            // Hidden under a folded ancestor.
            if ancestors(&part.rows, &r.account)
                .iter()
                .any(|a| folded.contains(a))
            {
                continue;
            }
            let depth = depth_of(&part.rows, &r.account);
            let mark = if parents.contains(&r.account) {
                if folded.contains(&r.account) {
                    glyphs.folded()
                } else {
                    glyphs.open()
                }
            } else {
                "  "
            };
            let name = format!(
                "{}{mark}{}",
                "  ".repeat(depth),
                shown_name(&part.rows, &r.account)
            );
            let mut amounts = r.amounts.iter();
            let first = amounts.next().map(String::as_str).unwrap_or("");
            let line_start = doc.text.len();
            let pad = column
                .saturating_sub(name.chars().count() + first.chars().count())
                .max(2);
            let line = format!("{name}{}{first}", " ".repeat(pad));
            let mark_at = line_start + 2 * depth;
            doc.styles
                .push((mark_at..mark_at + mark.len(), Style::Mark));
            if negative(first) {
                let s = line_start + line.len() - first.len();
                doc.styles
                    .push((s..line_start + line.len(), Style::Negative));
            }
            push(&mut doc, &line, None, Some(r.account.clone()));
            for a in amounts {
                let s = doc.text.len();
                let line = format!(
                    "{}{a}",
                    " ".repeat(column.saturating_sub(a.chars().count()))
                );
                if negative(a) {
                    doc.styles
                        .push((s + line.len() - a.len()..s + line.len(), Style::Negative));
                }
                push(&mut doc, &line, None, Some(r.account.clone()));
            }
        }
        let rule = format!(
            "{}{}",
            " ".repeat(column.saturating_sub(amount_width)),
            "-".repeat(amount_width)
        );
        push(&mut doc, &rule, Some(Style::Hint), None);
        for a in &part.total {
            let line = format!(
                "{}{a}",
                " ".repeat(column.saturating_sub(a.chars().count()))
            );
            push(&mut doc, &line, Some(Style::Total), None);
        }
    }
    doc
}

/// The account's nearest ancestor among `rows` (Ledger shows a parent
/// with one child as one line: `Budget:Travel`).
fn parent_of(rows: &[Row], account: &str) -> Option<String> {
    let mut a = account;
    while let Some((p, _)) = a.rsplit_once(':') {
        if rows.iter().any(|r| r.account == p) {
            return Some(p.to_string());
        }
        a = p;
    }
    None
}

fn ancestors(rows: &[Row], account: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut a = account.to_string();
    while let Some(p) = parent_of(rows, &a) {
        out.push(p.clone());
        a = p;
    }
    out
}

fn depth_of(rows: &[Row], account: &str) -> usize {
    ancestors(rows, account).len()
}

/// The name shown: below its parent's.
fn shown_name<'a>(rows: &[Row], account: &'a str) -> &'a str {
    match parent_of(rows, account) {
        Some(p) => &account[p.len() + 1..],
        None => account,
    }
}

/// The register (PT7b): one line per posting of an account (its
/// subaccounts' too), with the tool's running total.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Posting {
    pub date: String,
    /// `*`, `!`, or a blank.
    pub status: char,
    pub description: String,
    pub amount: String,
    pub total: String,
    /// Its file and line, where the tool says.
    pub place: Option<(String, u32)>,
    /// Its transaction's number, hledger's, for its place
    /// ([`hledger_place`]).
    pub transaction: Option<usize>,
}

/// `account` and its subaccounts, as a regular expression.
pub(crate) fn account_pattern(account: &str) -> String {
    let mut p = String::from("^");
    for c in account.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            p.push('\\');
        }
        p.push(c);
    }
    p.push_str("(:|$)");
    p
}

/// `account` and its subaccounts as Ledger's query terms: its query
/// language takes parentheses for grouping, so the two alternatives are
/// two terms, `^A$ or ^A:`.
pub(crate) fn ledger_pattern(account: &str) -> Vec<String> {
    let p = account_pattern(account);
    let base = p.trim_end_matches("(:|$)");
    vec![format!("{base}$"), "or".into(), format!("{base}:")]
}

/// Ledger's register format: fields apart by U+001F, which no payee
/// holds (`|` may).
pub(crate) const LEDGER_REGISTER: &str = "%(format_date(date, \"%Y-%m-%d\"))\u{1f}%(state)\u{1f}%(payee)\u{1f}%(scrub(display_amount))\u{1f}%(scrub(display_total))\u{1f}%(filename)\u{1f}%(beg_line)\\n";

/// The tool's register of `account` in `main` within `bounds`.
pub fn register_command(
    tool: Dialect,
    main: &str,
    account: &str,
    bounds: Option<(Date, Date)>,
) -> (&'static str, Vec<String>) {
    let pattern = account_pattern(account);
    let mut a: Vec<String> = Vec::new();
    // Within a period the running total is still the account's balance,
    // the postings before it counted: hledger's `-H`, Ledger's postings
    // shown by date (`-d`) rather than taken (`-b`), Beancount's opening
    // balances (`FROM OPEN ON`).
    match tool {
        Dialect::Hledger => {
            a.extend([
                "-f".into(),
                main.into(),
                "register".into(),
                "-O".into(),
                "json".into(),
            ]);
            if let Some((begin, end)) = bounds {
                a.extend([
                    "-H".into(),
                    "-b".into(),
                    begin.written('-'),
                    "-e".into(),
                    end.written('-'),
                ]);
            }
            a.push(format!("acct:{pattern}"));
            ("hledger", a)
        }
        Dialect::Ledger => {
            a.extend(["-f".into(), main.into(), "register".into()]);
            a.extend(["--register-format".into(), LEDGER_REGISTER.into()]);
            if let Some((begin, end)) = bounds {
                a.extend([
                    "-d".into(),
                    format!("d>=[{}] & d<[{}]", begin.written('/'), end.written('/')),
                ]);
            }
            a.extend(ledger_pattern(account));
            ("ledger", a)
        }
        Dialect::Beancount => {
            let from = bounds.map_or(String::new(), |(begin, end)| {
                format!(
                    "FROM OPEN ON {} CLOSE ON {} ",
                    begin.written('-'),
                    end.written('-')
                )
            });
            let q = format!(
                "SELECT date, flag, payee, narration, position, balance, filename, lineno {from}WHERE account ~ '{}'",
                pattern.replace('\'', "\\'")
            );
            (
                "bean-query",
                vec!["-f".into(), "csv".into(), main.into(), q],
            )
        }
    }
}

/// The fields of a CSV line: quoted ones with `""` for a quote.
fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    out.push(field);
    out
}

/// hledger's CSV register: `txnidx,date,code,description,account,amount,
/// total`; no status nor place (the transaction's number finds it). The
/// plugin reads its JSON instead (`crate::reconcile::hledger_register`),
/// which has the status and the amounts' style; this stays for a
/// register written as CSV.
pub fn hledger_register(csv: &str) -> Vec<Posting> {
    csv.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f = csv_fields(l);
            let get = |i: usize| f.get(i).cloned().unwrap_or_default();
            Posting {
                date: get(1),
                status: ' ',
                description: get(3),
                amount: get(5),
                total: get(6),
                place: None,
                transaction: get(0).parse().ok(),
            }
        })
        .collect()
}

/// Ledger's register in [`LEDGER_REGISTER`]'s form; a total of several
/// commodities runs over lines, joined.
pub fn ledger_register(out: &str) -> Vec<Posting> {
    let mut rows = Vec::new();
    let mut buf = String::new();
    for line in out.lines() {
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
        if buf.matches('\u{1f}').count() < 6 {
            continue;
        }
        let f: Vec<&str> = buf.split('\u{1f}').collect();
        let join = |s: &str| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        };
        rows.push(Posting {
            date: f[0].trim().to_string(),
            status: match f[1].trim() {
                "1" => '*',
                "2" => '!',
                _ => ' ',
            },
            description: f[2].trim().to_string(),
            amount: join(f[3]),
            total: join(f[4]),
            place: f[6].trim().parse().ok().map(|l| (f[5].to_string(), l)),
            transaction: None,
        });
        buf.clear();
    }
    rows
}

/// bean-query's CSV register: `date,flag,payee,narration,position,
/// balance,filename,lineno`.
pub fn beancount_register(csv: &str) -> Vec<Posting> {
    csv.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f = csv_fields(l);
            let get = |i: usize| f.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
            let (payee, narration) = (get(2), get(3));
            Posting {
                date: get(0),
                status: match get(1).as_str() {
                    "*" => '*',
                    "!" => '!',
                    _ => ' ',
                },
                description: match (payee.is_empty(), narration.is_empty()) {
                    (false, false) => format!("{payee} | {narration}"),
                    (false, true) => payee,
                    _ => narration,
                },
                amount: get(4),
                total: get(5)
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", "),
                place: get(7).parse().ok().map(|l| (get(6), l)),
                transaction: None,
            }
        })
        .collect()
}

/// The file and line of hledger's transaction number `n`, from `hledger
/// print -O json`.
pub fn hledger_place(print_json: &str, n: usize) -> Option<(String, u32)> {
    let all: serde_json::Value = serde_json::from_str(print_json).ok()?;
    let t = all
        .as_array()?
        .iter()
        .find(|t| t["tindex"].as_u64() == Some(n as u64))?;
    let pos = &t["tsourcepos"][0];
    Some((
        pos["sourceName"].as_str()?.to_string(),
        pos["sourceLine"].as_u64()? as u32,
    ))
}

/// The most lines a register shows: the last ones.
pub const REGISTER_MAX: usize = 2000;

/// The register document: a header, a hint, a line per posting (the
/// last [`REGISTER_MAX`]), its date, status, description (cut to the
/// widest that fits), amount and running total right-aligned; each
/// line's posting's index in `rows` (as text) in `lines`.
pub fn register_layout(header: &str, hint: &str, rows: &[Posting]) -> Document {
    let mut doc = Document::default();
    let push = |doc: &mut Document, s: String, style: Option<Style>, at: Option<String>| {
        let start = doc.text.len();
        doc.text.push_str(&s);
        if let Some(st) = style {
            doc.styles.push((start..doc.text.len(), st));
        }
        doc.text.push('\n');
        doc.lines.push(at);
    };
    push(&mut doc, header.to_string(), Some(Style::Header), None);
    push(&mut doc, hint.to_string(), Some(Style::Hint), None);
    push(&mut doc, String::new(), None, None);
    if rows.is_empty() {
        push(&mut doc, "(no postings)".into(), Some(Style::Hint), None);
        return doc;
    }
    let skipped = rows.len().saturating_sub(REGISTER_MAX);
    if skipped > 0 {
        push(
            &mut doc,
            format!("… {skipped} earlier postings: a shorter period (p) shows them"),
            Some(Style::Hint),
            None,
        );
    }
    let shown = &rows[skipped..];
    let width = |s: &str| s.chars().count();
    let desc = shown
        .iter()
        .map(|r| width(&r.description))
        .max()
        .unwrap_or(0)
        .clamp(12, 40);
    let amount = shown.iter().map(|r| width(&r.amount)).max().unwrap_or(0);
    let total = shown.iter().map(|r| width(&r.total)).max().unwrap_or(0);
    for (i, r) in shown.iter().enumerate() {
        let d: String = if width(&r.description) > desc {
            let mut t: String = r.description.chars().take(desc - 1).collect();
            t.push('…');
            t
        } else {
            r.description.clone()
        };
        let head = format!(
            "{} {} {d}{}",
            r.date,
            r.status,
            " ".repeat(desc - width(&d))
        );
        let a = format!("{}{}", " ".repeat(amount - width(&r.amount)), r.amount);
        let t = format!("{}{}", " ".repeat(total - width(&r.total)), r.total);
        let line = format!("{head}  {a}  {t}");
        let start = doc.text.len();
        if negative(&r.amount) {
            let s = start + head.len() + 2 + a.len() - r.amount.len();
            doc.styles.push((s..s + r.amount.len(), Style::Negative));
        }
        if negative(&r.total) {
            let s = start + line.len() - r.total.len();
            doc.styles.push((s..s + r.total.len(), Style::Negative));
        }
        push(&mut doc, line, None, Some((skipped + i).to_string()));
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    const HLEDGER: &str = "assets|    200,00 EUR\n10,00 \"FUND A\"\n 65.197,25 TRY\n  cash|  200,00 EUR\n1.500,00 TRY\n  bank|63.697,25 TRY\n    ziraat|63.697,25 TRY\n  broker|10,00 \"FUND A\"\n    index fund|10,00 \"FUND A\"\nliabilities|-1.120,00 TRY\n  card|-1.120,00 TRY\n-----------------------------\n|200,00 EUR\n 64.077,25 TRY\n";

    const LEDGER: &str = "Assets|$12,219.00\nAssets:Checking|$11,619.00\nAssets:Petty|$600.00\nBudget:Travel|$-132.00\nExpenses|$49.00\nEUR 120.00\nExpenses:Software|$49.00\nExpenses:Travel|EUR 120.00\n|$-1,951.00\nEUR 120.00\n";

    #[test]
    fn hledgers_rows() {
        let p = Part::of(hledger_rows(HLEDGER));
        let names: Vec<&str> = p.rows.iter().map(|r| r.account.as_str()).collect();
        assert_eq!(
            names,
            [
                "assets",
                "assets:cash",
                "assets:bank",
                "assets:bank:ziraat",
                "assets:broker",
                "assets:broker:index fund",
                "liabilities",
                "liabilities:card"
            ]
        );
        assert_eq!(
            p.rows[0].amounts,
            ["200,00 EUR", "10,00 \"FUND A\"", "65.197,25 TRY"]
        );
        assert_eq!(p.total, ["200,00 EUR", "64.077,25 TRY"]);
    }

    #[test]
    fn ledgers_rows() {
        let p = Part::of(ledger_rows(LEDGER));
        assert_eq!(p.rows.len(), 7);
        assert_eq!(p.rows[4].amounts, ["$49.00", "EUR 120.00"]);
        assert_eq!(p.total, ["$-1,951.00", "EUR 120.00"]);
        // A parent with one child, one line: shown whole at the top.
        assert_eq!(parent_of(&p.rows, "Budget:Travel"), None);
        assert_eq!(shown_name(&p.rows, "Budget:Travel"), "Budget:Travel");
        assert_eq!(shown_name(&p.rows, "Assets:Petty"), "Petty");
    }

    #[test]
    fn beancounts_levels() {
        let one = beancount_rows(
            "a,b\nAssets,\" 200.00 EUR, 6 FUND, 84877.65 TRY\"\nEquity,\"           ,       ,-50000.00 TRY\"\n",
        );
        assert_eq!(one[0].amounts, ["200.00 EUR", "6 FUND", "84877.65 TRY"]);
        assert_eq!(one[1].amounts, ["-50000.00 TRY"]);
        let two = beancount_rows(
            "a,b\nAssets:Cash,\" 200.00 EUR,        ,             \"\nAssets:Bank,\"           ,        , 76857.65 TRY\"\nEquity,\"           ,       ,-50000.00 TRY\"\n",
        );
        let tree = beancount_tree(&[one, two]);
        let names: Vec<&str> = tree.iter().map(|r| r.account.as_str()).collect();
        assert_eq!(names, ["Assets", "Assets:Bank", "Assets:Cash", "Equity"]);
        let total = beancount_rows("b\n-43157.65 TRY\n");
        assert_eq!(total[0].account, "");
        assert_eq!(total[0].amounts, ["-43157.65 TRY"]);
    }

    #[test]
    fn periods() {
        let today = Date {
            y: 2026,
            m: 10,
            d: 11,
        };
        let d = |y, m, d| Date { y, m, d };
        assert_eq!(
            Period::ThisMonth.bounds(today),
            Some((d(2026, 10, 1), d(2026, 11, 1)))
        );
        assert_eq!(
            Period::LastMonth.bounds(today),
            Some((d(2026, 9, 1), d(2026, 10, 1)))
        );
        assert_eq!(
            Period::ThisQuarter.bounds(today),
            Some((d(2026, 10, 1), d(2027, 1, 1)))
        );
        assert_eq!(
            Period::LastYear.bounds(today),
            Some((d(2025, 1, 1), d(2026, 1, 1)))
        );
        let jan = Date {
            y: 2026,
            m: 1,
            d: 5,
        };
        assert_eq!(
            Period::LastMonth.bounds(jan),
            Some((d(2025, 12, 1), d(2026, 1, 1)))
        );
        assert_eq!(Period::All.bounds(today), None);
    }

    #[test]
    fn the_commands() {
        let b = Period::ThisYear.bounds(Date {
            y: 2026,
            m: 10,
            d: 11,
        });
        let (p, a) = command(Dialect::Hledger, "/j/m.journal", Section::Flow, b, 2, 0);
        assert_eq!(p, "hledger");
        assert!(
            a.ends_with(&[
                "-b".into(),
                "2026-01-01".into(),
                "-e".into(),
                "2027-01-01".into(),
                "--depth=2".into(),
                "not:type:ALE".into()
            ]),
            "{a:?}"
        );
        let (_, a) = command(Dialect::Ledger, "/j/b.ledger", Section::Sheet, b, 0, 0);
        assert!(a.contains(&"-e".to_string()) && !a.contains(&"-b".to_string()));
        let (p, a) = command(Dialect::Beancount, "/j/h.beancount", Section::Flow, b, 0, 2);
        assert_eq!(p, "bean-query");
        assert!(
            a[3].contains("root(account, 2)") && a[3].contains("date >= 2026-01-01"),
            "{}",
            a[3]
        );
    }

    #[test]
    fn valued() {
        let (_, mut a) = command(Dialect::Hledger, "/j", Section::Sheet, None, 0, 0);
        assert!(
            value_args(Dialect::Hledger, &mut a)
                .ends_with(&["-V".into(), "--infer-market-prices".into()])
        );
        let (_, mut a) = command(Dialect::Beancount, "/j", Section::Sheet, None, 0, 1);
        assert!(value_args(Dialect::Beancount, &mut a)[3].contains("value(sum(position))"));
        let (_, mut a) = register_command(Dialect::Beancount, "/j", "Assets", None);
        assert!(
            value_args(Dialect::Beancount, &mut a)[3].contains("value(position), value(balance),")
        );
    }

    #[test]
    fn the_layout() {
        let part = Part::of(hledger_rows(HLEDGER));
        let none = BTreeSet::new();
        let d = layout(
            "Balance",
            "keys",
            &[(Section::Sheet, "Assets".into(), part.clone())],
            &none,
            crate::Glyphs::Unicode,
        );
        let text = &d.text;
        assert!(text.contains("\n▾ assets "), "{text}");
        assert!(text.contains("\n  ▾ bank "));
        assert!(text.contains("\n      ziraat "));
        // Every amount ends at one column.
        let ends: BTreeSet<usize> = text
            .lines()
            .filter(|l| l.ends_with("TRY") || l.ends_with("EUR"))
            .map(|l| l.chars().count())
            .collect();
        assert_eq!(ends.len(), 1, "{text}");
        // A line's account, for Enter and folding.
        let n = text.lines().position(|l| l.contains("ziraat")).unwrap();
        assert_eq!(d.lines[n].as_deref(), Some("assets:bank:ziraat"));
        assert!(d.parents.contains("assets:bank") && !d.parents.contains("assets:cash"));
        // Folded: its children hidden, its mark ▸.
        let folded: BTreeSet<String> = ["assets:bank".to_string()].into();
        let d = layout(
            "Balance",
            "keys",
            &[(Section::Sheet, "Assets".into(), part.clone())],
            &folded,
            crate::Glyphs::Unicode,
        );
        assert!(d.text.contains("▸ bank") && !d.text.contains("ziraat"));
        let ascii = layout(
            "Balance",
            "keys",
            &[(Section::Sheet, "Assets".into(), part)],
            &folded,
            crate::Glyphs::Ascii,
        );
        assert!(ascii.text.contains("\nv assets ") && ascii.text.contains("> bank"));
        assert!(ascii.text.is_ascii() || !ascii.text.contains(['▸', '▾']));
        // Below zero: red.
        assert!(negative("-1.120,00 TRY") && negative("$-400.00") && !negative("1.120,00 TRY"));
        assert!(
            d.styles
                .iter()
                .any(|(r, s)| *s == Style::Negative && d.text[r.clone()] == *"-1.120,00 TRY")
        );
    }

    #[test]
    fn registers() {
        let h = hledger_register(
            "\"txnidx\",\"date\",\"code\",\"description\",\"account\",\"amount\",\"total\"\n\"3\",\"2026-01-02\",\"1001\",\"Ziraat | salary\",\"assets:bank:ziraat\",\"45000,00 TRY\",\"93749,60 TRY\"\n\"5\",\"2026-01-03\",\"\",\"A \"\"quoted\"\" one\",\"x\",\"-842,35 TRY\",\"92907,25 TRY\"\n",
        );
        assert_eq!(h.len(), 2);
        assert_eq!(
            (h[0].transaction, h[0].amount.as_str()),
            (Some(3), "45000,00 TRY")
        );
        assert_eq!(h[1].description, "A \"quoted\" one");
        let l = ledger_register(
            "2026-01-09\u{1f}2\u{1f}Software\u{1f}$49.00\u{1f}$49.00\u{1f}/j/b.ledger\u{1f}41\n2026-01-20\u{1f}1\u{1f}Client A | consulting\u{1f}EUR 120.00\u{1f}$49.00\nEUR 120.00\u{1f}/j/b.ledger\u{1f}52\n",
        );
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].status, '!');
        assert_eq!(l[1].description, "Client A | consulting");
        assert_eq!(l[1].total, "$49.00, EUR 120.00");
        assert_eq!(l[1].place, Some(("/j/b.ledger".into(), 52)));
        let b = beancount_register(
            "date,flag,payee,narration,position,balance,filename,lineno\n2026-01-02,*,Ziraat,\"Salary, January\", 45000.00 TRY, 95000.00 TRY,/j/h.beancount,24\n",
        );
        assert_eq!(b[0].description, "Ziraat | Salary, January");
        assert_eq!(b[0].amount, "45000.00 TRY");
        assert_eq!(b[0].place, Some(("/j/h.beancount".into(), 24)));
        let json = r#"[{"tindex": 3, "tsourcepos": [{"sourceName": "/j/2026.journal", "sourceLine": 17, "sourceColumn": 1}]}]"#;
        assert_eq!(hledger_place(json, 3), Some(("/j/2026.journal".into(), 17)));
        assert_eq!(hledger_place(json, 4), None);
        assert_eq!(account_pattern("assets:bank"), "^assets:bank(:|$)");
        assert_eq!(account_pattern("a.b$"), "^a\\.b\\$(:|$)");
        let d = register_layout("R", "keys", &l);
        let lines: Vec<&str> = d.text.lines().collect();
        assert!(lines[3].starts_with("2026-01-09 ! Software"), "{}", d.text);
        assert_eq!(
            lines[3].chars().count(),
            lines[4].chars().count(),
            "{}",
            d.text
        );
        assert_eq!(d.lines[4].as_deref(), Some("1"));
    }
}
