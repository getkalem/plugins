//! Aligning amounts (PT4, ledger-mode's auto-align): the postings of a
//! transaction, or of the lines selected, get their amounts' last digits
//! at one column. Only the blanks between a posting's account and its
//! amount change: the account, the amount as written (its sign, decimal
//! mark, digit groups, commodity and side), a cost, a lot, a balance
//! assertion and a comment stay byte for byte, and a posting without an
//! amount is left alone.
//!
//! The amount's anchor is its last digit (`45.000,00` of
//! `45.000,00 TRY = 93.749,60 TRY`, `400.00` of `$-400.00`, `42.35` of
//! Beancount's `800.00 + 42.35 TRY`), the closing parenthesis of a value
//! expression (`(-$132.00)`), and, for a posting with a balance assertion
//! alone, the assertion's amount. Columns count characters.

use std::ops::Range;

use crate::Dialect;

/// The fewest blanks between an account and its amount: two, as hledger
/// and Ledger require (one ends an account name in neither).
const MIN_GAP: usize = 2;

/// A posting line's parts, in bytes of the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posting {
    /// The blanks between the account and the amount.
    pub gap: Range<usize>,
    /// Characters from the line's start to the gap: the indent, the
    /// status, the account.
    pub before: usize,
    /// Characters from the gap's end to the amount's anchor, the anchor
    /// included.
    pub to_anchor: usize,
}

/// Whether `c` is a blank.
fn blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// The parts of `line` (without its line feed) when it is a posting with
/// an amount or a balance assertion.
pub fn posting(line: &str, dialect: Dialect) -> Option<Posting> {
    let indent = line.len() - line.trim_start_matches(blank).len();
    if indent == 0 {
        return None;
    }
    let mut at = indent;
    let rest = &line[at..];
    // A comment line, Beancount's metadata (`key: value`, a lowercase
    // key), or nothing.
    match rest.chars().next() {
        None | Some(';' | '#' | '%' | '|') => return None,
        _ => {}
    }
    if dialect == Dialect::Beancount && is_metadata(rest) {
        return None;
    }
    // A posting's status (`*`, `!`; Beancount's other flags).
    let flag = |c: char| match dialect {
        Dialect::Beancount => "*!&#?%PSTCURM".contains(c),
        _ => c == '*' || c == '!',
    };
    let mut chars = rest.chars();
    if let (Some(c), Some(next)) = (chars.next(), chars.next())
        && flag(c)
        && blank(next)
    {
        at += 1;
        at += line[at..].len() - line[at..].trim_start_matches(blank).len();
    }
    // The account: up to two blanks or a tab (one blank in Beancount),
    // a comment, or the end.
    let account_end = account_end(line, at, dialect)?;
    if account_end == at {
        return None;
    }
    let gap_end = account_end + line[account_end..].len()
        - line[account_end..].trim_start_matches(blank).len();
    let amount = &line[gap_end..];
    if gap_end == account_end || amount.is_empty() || amount.starts_with(';') {
        return None;
    }
    let anchor = anchor(amount)?;
    Some(Posting {
        gap: account_end..gap_end,
        before: line[..account_end].chars().count(),
        to_anchor: amount[..anchor].chars().count(),
    })
}

fn is_metadata(rest: &str) -> bool {
    let key: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    key.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && rest[key.len()..].starts_with(':')
}

/// Where the account starting at `from` ends.
fn account_end(line: &str, from: usize, dialect: Dialect) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'\t' => return Some(i),
            b' ' if dialect == Dialect::Beancount => return Some(i),
            b' ' if bytes.get(i + 1).is_none_or(|b| *b == b' ' || *b == b'\t') => {
                return Some(i);
            }
            b';' => return Some(i),
            _ => i += 1,
        }
    }
    Some(i)
}

/// Whether `c` may be part of a commodity's symbol written bare: neither
/// a digit, a blank, a sign nor what starts another part of the posting.
fn commodity_char(c: char) -> bool {
    !(c.is_ascii_digit() || blank(c) || "-+.,@*;=(){}[]<>|~#!&^/%:\"'`".contains(c))
}

/// The byte offset in `amount` just after its anchor.
fn anchor(amount: &str) -> Option<usize> {
    let b = amount.as_bytes();
    let mut i = 0;
    let skip_blanks = |i: &mut usize| {
        while *i < b.len() && (b[*i] == b' ' || b[*i] == b'\t') {
            *i += 1;
        }
    };
    // A balance assertion alone: its amount.
    if b.first() == Some(&b'=') {
        while i < b.len() && (b[i] == b'=' || b[i] == b'*') {
            i += 1;
        }
        skip_blanks(&mut i);
        return anchor(&amount[i..]).map(|a| a + i);
    }
    // A value expression in parentheses: its closing one.
    if b.first() == Some(&b'(') {
        let mut depth = 0;
        for (j, c) in amount.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j + 1);
                    }
                }
                _ => {}
            }
        }
        return None;
    }
    // A sign, a commodity on the left, blanks.
    while i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    skip_blanks(&mut i);
    if amount[i..].starts_with('"') {
        i += 1 + amount[i + 1..].find('"')? + 1;
    } else {
        let symbol: usize = amount[i..]
            .chars()
            .take_while(|c| commodity_char(*c))
            .map(char::len_utf8)
            .sum();
        i += symbol;
    }
    skip_blanks(&mut i);
    // The number, or Beancount's arithmetic (`800.00 + 42.35`, `*-1`):
    // digits, their groups and decimal mark, signs and operators, up to
    // what is neither; the anchor after the last digit.
    let mut last = None;
    while i < b.len() {
        let c = b[i];
        let digit_next = b.get(i + 1).is_some_and(u8::is_ascii_digit);
        let digit_before = last == Some(i);
        match c {
            b'0'..=b'9' => {
                i += 1;
                last = Some(i);
            }
            // A decimal mark or a digit group's mark between digits; a
            // decimal mark ending a number (`1.`), or starting one (`.5`).
            b'.' | b',' | b'\'' | b'_' if digit_next || digit_before => i += 1,
            // A blank as a digit group's mark (hledger), between digits.
            b' ' if digit_before && digit_next => i += 1,
            b'-' | b'+' | b'*' | b'/' | b' ' | b'\t' => i += 1,
            // An exponent.
            b'e' | b'E' if digit_before && digit_next => i += 1,
            _ => {
                // A thin or no-break space as a group's mark.
                let rest = &amount[i..];
                match rest.chars().next() {
                    Some(c @ ('\u{2009}' | '\u{202F}' | '\u{A0}'))
                        if digit_before
                            && rest[c.len_utf8()..].starts_with(|d: char| d.is_ascii_digit()) =>
                    {
                        i += c.len_utf8();
                    }
                    _ => break,
                }
            }
        }
    }
    last
}

/// The gap that puts `p`'s anchor at `column` (from 1), two blanks at the
/// least.
fn gap(p: &Posting, column: usize) -> usize {
    column
        .saturating_sub(1 + p.before + p.to_anchor.saturating_sub(1))
        .max(MIN_GAP)
}

/// The column (from 1) of the anchors of `postings` when they are put as
/// near their accounts as the longest allows: `amount_column` 0.
pub fn tightest(postings: &[Posting]) -> usize {
    postings
        .iter()
        .map(|p| p.before + MIN_GAP + p.to_anchor)
        .max()
        .unwrap_or(0)
}

/// The lines of `text`, each with its start in bytes and without its
/// line feed (nor a carriage return before it).
fn lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    for l in text.split_inclusive('\n') {
        out.push((start, l.trim_end_matches('\n').trim_end_matches('\r')));
        start += l.len();
    }
    out
}

/// Whether a line starts a transaction or a directive: neither indented
/// nor blank.
fn top(line: &str) -> bool {
    line.chars().next().is_some_and(|c| !blank(c))
}

/// The lines (by index) of the transaction holding line `n`: its header
/// and the indented lines under it; none when `n` is outside one.
fn transaction(lines: &[(usize, &str)], n: usize) -> Option<Range<usize>> {
    let mut start = n;
    loop {
        let l = lines.get(start)?.1;
        if l.trim().is_empty() {
            return None;
        }
        if top(l) {
            break;
        }
        start = start.checked_sub(1)?;
    }
    let mut end = start + 1;
    while end < lines.len() && !top(lines[end].1) && !lines[end].1.trim().is_empty() {
        end += 1;
    }
    Some(start..end)
}

/// The edits that align the postings of `text` touched by `selection`
/// (bytes): the transaction holding the cursor when nothing is selected,
/// else every transaction the selection reaches. Each edit replaces a
/// gap with blanks; a gap already right gives none. `column` is
/// `amount_column`: 0 aligns each transaction as near its accounts as its
/// longest posting allows.
pub fn edits(
    text: &str,
    selection: Range<usize>,
    dialect: Dialect,
    column: usize,
) -> Vec<(Range<usize>, String)> {
    let lines = lines(text);
    let line_of = |at: usize| lines.iter().rposition(|(s, _)| *s <= at).unwrap_or(0);
    let (first, last) = (line_of(selection.start), line_of(selection.end));
    let mut blocks: Vec<Range<usize>> = Vec::new();
    for n in first..=last {
        // A selection's postings outside a transaction's header (a
        // periodic or an automated one has its own header) are aligned
        // by their own block too.
        let block = transaction(&lines, n).unwrap_or(n..n + 1);
        if blocks.last() != Some(&block) && !blocks.iter().any(|b| b.contains(&n)) {
            blocks.push(block);
        }
    }
    let mut out = Vec::new();
    for block in blocks {
        let postings: Vec<(usize, Posting)> = block
            .filter_map(|n| posting(lines[n].1, dialect).map(|p| (n, p)))
            .collect();
        let at = if column == 0 {
            tightest(&postings.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>())
        } else {
            column
        };
        for (n, p) in postings {
            let want = gap(&p, at);
            let line = lines[n].1;
            if line[p.gap.clone()] != *" ".repeat(want) {
                let s = lines[n].0;
                out.push((s + p.gap.start..s + p.gap.end, " ".repeat(want)));
            }
        }
    }
    out
}

/// The settings of alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// `amount_column`: where the amounts' last digits go, from 1; 0 for
    /// as near the accounts as the longest posting allows.
    pub column: usize,
    /// `align_on_enter`: Enter on a posting aligns it first.
    pub on_enter: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            column: 52,
            on_enter: true,
        }
    }
}

impl Settings {
    /// The settings through `own` (the plugin's own setting as JSON).
    pub fn read(own: impl Fn(&str) -> Option<String>) -> Settings {
        let json =
            |k: &str| own(k).and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok());
        let d = Settings::default();
        Settings {
            column: json("amount_column")
                .and_then(|v| v.as_u64())
                .map_or(d.column, |c| c.min(400) as usize),
            on_enter: json("align_on_enter")
                .and_then(|v| v.as_bool())
                .unwrap_or(d.on_enter),
        }
    }
}

/// The edits of [`edits`] on the cursor's line only (Enter's), the
/// column still the transaction's.
pub fn edits_on_line(
    text: &str,
    cursor: usize,
    dialect: Dialect,
    column: usize,
) -> Vec<(Range<usize>, String)> {
    let start = text[..cursor.min(text.len())]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    edits(text, cursor..cursor, dialect, column)
        .into_iter()
        .filter(|(r, _)| r.start >= start && r.end <= end)
        .collect()
}

/// `text` with `edits` applied (ascending, apart).
pub fn apply(text: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (r, s) in edits {
        out.push_str(&text[at..r.start]);
        out.push_str(s);
        at = r.end;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Dialect::*;

    /// The column (from 1) of the anchor of `line` after aligning it.
    fn anchor_column(line: &str, d: Dialect) -> usize {
        let p = posting(line, d).unwrap_or_else(|| panic!("a posting: {line:?}"));
        p.before + (p.gap.end - p.gap.start) + p.to_anchor
    }

    #[test]
    fn the_anchor_of_each_kind_of_amount() {
        let cases: &[(&str, Dialect, &str)] = &[
            (
                "    assets:bank:ziraat  45.000,00 TRY = 93.749,60 TRY",
                Hledger,
                "45.000,00",
            ),
            ("    expenses:food:market  842,35 TRY", Hledger, "842,35"),
            ("    assets:cash  200,00 EUR @ 36,50 TRY", Hledger, "200,00"),
            (
                "    assets:bank:ziraat  -7.300,00 TRY",
                Hledger,
                "-7.300,00",
            ),
            (
                "    assets:broker:index fund  10,00 \"FUND A\" @ 350,00 TRY",
                Hledger,
                "10,00",
            ),
            (
                "    assets:bank:ziraat  == 63.697,25 TRY",
                Hledger,
                "== 63.697,25",
            ),
            ("    (budget:food)  *-1", Hledger, "*-1"),
            ("    assets:x  1 000,50 TRY", Hledger, "1 000,50"),
            ("    [Liabilities:Tax]  $-400.00", Ledger, "$-400.00"),
            ("    Expenses:Travel  EUR 120.00", Ledger, "EUR 120.00"),
            ("    chk  (-$132.00)", Ledger, "(-$132.00)"),
            ("    Expenses:Software  $49.00  ; :tools:", Ledger, "$49.00"),
            (
                "    Assets:Fund  10 AAPL {$30} [2025/12/01] @ $35",
                Ledger,
                "10",
            ),
            (
                "  Expenses:Food:Market  800.00 + 42.35 TRY",
                Beancount,
                "800.00 + 42.35",
            ),
            (
                "  Assets:Broker:FUND  -4 FUND {350.00 TRY} @ 380.00 TRY",
                Beancount,
                "-4",
            ),
            ("  ! Assets:Cash  200.00 EUR", Beancount, "200.00"),
        ];
        for (line, d, anchored) in cases {
            let p = posting(line, *d).unwrap_or_else(|| panic!("{line}"));
            let amount = &line[p.gap.end..];
            let n: usize = anchored.chars().count();
            assert_eq!(p.to_anchor, n, "{line}: {amount:?}");
            assert!(amount.starts_with(anchored), "{line}");
        }
    }

    #[test]
    fn lines_that_are_no_postings_with_an_amount() {
        for (line, d) in [
            ("2026-01-02 * Ziraat | salary", Hledger),
            ("    income:salary", Hledger),
            ("    assets:bank:ziraat  ; a comment", Hledger),
            ("    ; a comment line", Hledger),
            ("  receipt: \"P-0001\"", Beancount),
            ("    shared-with: \"Ayşe\"", Beancount),
            ("  Liabilities:Card", Beancount),
            ("", Hledger),
        ] {
            assert_eq!(posting(line, d), None, "{line:?}");
        }
        // An account with a space, ended by two.
        let p = posting("    assets:broker:index fund   10,00 TRY", Hledger).unwrap();
        assert_eq!(p.before, "    assets:broker:index fund".len());
    }

    const TRANSACTION: &str = "2026-01-15 * Broker | units  ; a comment\n    assets:broker:index fund      10,00 \"FUND A\" @ 350,00 TRY  ; lot one\n    assets:bank:ziraat  -3.500,00 TRY = 60.197,25 TRY\n    expenses:fees\t1,00 TRY\n    assets:cash\n";

    #[test]
    fn a_transaction_aligned_at_the_column() {
        let e = edits(TRANSACTION, 50..50, Hledger, 52);
        let out = apply(TRANSACTION, &e);
        for line in out.lines().skip(1).take(3) {
            assert_eq!(anchor_column(line, Hledger), 52, "{line}");
        }
        // Only the gaps changed: the rest byte for byte, the posting
        // without an amount untouched.
        let squeeze = |s: &str| {
            s.lines()
                .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
        };
        assert_eq!(squeeze(&out), squeeze(TRANSACTION));
        assert_eq!(out.lines().next(), TRANSACTION.lines().next());
        assert!(out.ends_with("    assets:cash\n"));
        assert!(out.contains("@ 350,00 TRY  ; lot one\n"));
        assert!(out.contains("-3.500,00 TRY = 60.197,25 TRY\n"));
        // Aligned again: nothing to do.
        assert!(edits(&out, 50..50, Hledger, 52).is_empty());
    }

    #[test]
    fn as_near_as_the_longest_allows() {
        let e = edits(TRANSACTION, 0..0, Hledger, 0);
        let out = apply(TRANSACTION, &e);
        let cols: Vec<usize> = out
            .lines()
            .filter_map(|l| posting(l, Hledger).map(|_| anchor_column(l, Hledger)))
            .collect();
        assert!(cols.windows(2).all(|w| w[0] == w[1]), "{cols:?}");
        // The longest account and its amount two blanks apart.
        assert!(out.contains("index fund  10,00"), "{out}");
        assert!(edits(&out, 0..0, Hledger, 0).is_empty());
    }

    #[test]
    fn too_long_for_the_column() {
        let t = "2026-01-01 x\n    expenses:a:very:long:account:name:indeed  1.000,00 TRY\n";
        let out = apply(t, &edits(t, 20..20, Hledger, 20));
        assert!(out.contains("indeed  1.000,00"), "{out}");
    }

    #[test]
    fn the_transaction_at_the_cursor_only() {
        let t = "2026-01-01 a\n    x  1 TRY\n    y\n\n2026-01-02 b\n    x  2 TRY\n    y\n";
        let second = t.find("2026-01-02").unwrap();
        let e = edits(t, second + 3..second + 3, Hledger, 20);
        assert_eq!(e.len(), 1);
        assert!(e[0].0.start > second);
        // On a blank line: nothing.
        let blank = t.find("\n\n").unwrap() + 1;
        assert!(edits(t, blank..blank, Hledger, 20).is_empty());
        // A selection over both: both.
        assert_eq!(edits(t, 0..t.len(), Hledger, 20).len(), 2);
    }

    #[test]
    fn beancount_and_its_metadata() {
        let t = "2026-01-02 * \"Ziraat\" \"Salary\"\n  receipt: \"P-0001\"\n  Assets:Bank:Ziraat 45000.00 TRY\n    note: \"x\"\n  Income:Salary   -45000.00 TRY\n";
        let out = apply(t, &edits(t, 0..0, Beancount, 50));
        assert!(out.contains("  receipt: \"P-0001\"\n"));
        assert!(out.contains("    note: \"x\"\n"));
        for l in out.lines().filter(|l| l.contains("TRY")) {
            assert_eq!(anchor_column(l, Beancount), 50, "{l}");
        }
    }

    #[test]
    fn enter_aligns_its_line_only() {
        let t = "2026-01-01 x\n    a  1 TRY\n    bb  2 TRY\n";
        let at = t.find("bb").unwrap() + 9;
        let e = edits_on_line(t, at, Hledger, 20);
        assert_eq!(e.len(), 1);
        assert!(t[e[0].0.clone()].chars().all(char::is_whitespace));
        assert!(e[0].0.start > t.find("bb").unwrap());
    }

    #[test]
    fn the_settings_read() {
        assert_eq!(Settings::read(|_| None), Settings::default());
        let s = Settings::read(|k| match k {
            "amount_column" => Some("0".into()),
            "align_on_enter" => Some("false".into()),
            _ => None,
        });
        assert_eq!(
            s,
            Settings {
                column: 0,
                on_enter: false
            }
        );
    }

    #[test]
    fn wide_characters_count_as_one() {
        let t = "2026-01-01 x\n    gider:çay  5 TRY\n    gider:yemek  10 TRY\n";
        let out = apply(t, &edits(t, 0..0, Hledger, 30));
        for l in out.lines().skip(1) {
            assert_eq!(anchor_column(l, Hledger), 30, "{l}");
        }
    }
}
