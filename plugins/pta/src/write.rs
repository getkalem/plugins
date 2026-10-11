//! Writing transactions (PT6): the transactions of a journal's text, and
//! the edits that toggle their status, copy one at today's date, move a
//! date by a day, sort them by date, and place a new one; the dates the
//! user types (hledger's smart dates, resolved here), and the draft a
//! tool gives turned into the new transaction. Each edit is the least
//! that does it: the bytes around stay as they are.

use std::ops::Range;

use crate::Dialect;

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

impl Date {
    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    pub fn days(self) -> i64 {
        let y = i64::from(self.y) - i64::from(self.m <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.m);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.d) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01.
    pub fn from_days(days: i64) -> Date {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
        Date { y, m, d }
    }

    /// The date `n` days later (earlier when negative).
    pub fn plus(self, n: i64) -> Date {
        Date::from_days(self.days() + n)
    }

    /// The day of the week, 0 for Monday.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday.
        (self.days() + 3).rem_euclid(7) as u32
    }

    /// The date of `ms` milliseconds since 1970 (a local time).
    pub fn of_millis(ms: i64) -> Date {
        Date::from_days(ms.div_euclid(86_400_000))
    }

    fn valid(self) -> bool {
        (1..=12).contains(&self.m) && self.d >= 1 && Date::from_days(self.days()) == self
    }

    /// Written with `sep` between its parts: `2026-01-05`.
    pub fn written(self, sep: char) -> String {
        format!("{:04}{sep}{:02}{sep}{:02}", self.y, self.m, self.d)
    }
}

/// A date written at the start of `s`: `2026-01-05`, `2026/1/5`,
/// `2026.01.05`; the date, its length and its separator.
pub fn date_at(s: &str) -> Option<(Date, usize, char)> {
    let b = s.as_bytes();
    let digits = |from: usize, max: usize| {
        let n = b[from..]
            .iter()
            .take(max)
            .take_while(|c| c.is_ascii_digit())
            .count();
        (n > 0).then(|| (s[from..from + n].parse::<u32>().ok(), from + n))
    };
    let (Some(y), at) = digits(0, 4)? else {
        return None;
    };
    if at != 4 {
        return None;
    }
    let sep = *b.get(at)? as char;
    if !matches!(sep, '-' | '/' | '.') {
        return None;
    }
    let (Some(m), at) = digits(at + 1, 2)? else {
        return None;
    };
    if b.get(at) != Some(&(sep as u8)) {
        return None;
    }
    let (Some(d), at) = digits(at + 1, 2)? else {
        return None;
    };
    let date = Date { y: y as i32, m, d };
    date.valid().then_some((date, at, sep))
}

/// A transaction in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    /// Its lines: the header and the indented lines under it, the last
    /// line feed included.
    pub range: Range<usize>,
    /// The comment lines right above it (none blank between), with it
    /// when it is moved.
    pub with_comments: Range<usize>,
    /// Its date and where it is written.
    pub date: Date,
    pub date_range: Range<usize>,
    /// The date's separator.
    pub sep: char,
    /// Where its status goes: after the date (and a second date) and one
    /// blank; and the status written there, with the blank after it.
    pub status_at: usize,
    pub status: Option<Range<usize>>,
}

/// The lines of `text`: start and end (the line feed left out).
fn lines(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut s = 0;
    for l in text.split_inclusive('\n') {
        let end = s + l.trim_end_matches('\n').trim_end_matches('\r').len();
        out.push(s..end);
        s += l.len();
    }
    out
}

fn indented(l: &str) -> bool {
    l.starts_with(' ') || l.starts_with('\t')
}

fn comment_line(l: &str, dialect: Dialect) -> bool {
    let t = l.trim_start();
    match dialect {
        Dialect::Beancount => t.starts_with(';'),
        _ => t.starts_with(';') || (!indented(l) && (l.starts_with('#') || l.starts_with('*'))),
    }
}

/// Beancount's transaction flags, `txn` among them.
fn beancount_flag(rest: &str) -> Option<usize> {
    if rest.starts_with("txn") && rest[3..].starts_with([' ', '\t']) {
        return Some(3);
    }
    let c = rest.chars().next()?;
    ("*!&#?%PSTCURM".contains(c) && rest[1..].starts_with([' ', '\t']).then_some(()).is_some())
        .then_some(1)
}

/// The transactions of `text`, in order; `comment … end comment` blocks
/// skipped.
pub fn transactions(text: &str, dialect: Dialect) -> Vec<Transaction> {
    let ls = lines(text);
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_block = false;
    while i < ls.len() {
        let line = &text[ls[i].clone()];
        if dialect != Dialect::Beancount {
            if line.starts_with("comment") || line.starts_with("test") {
                in_block = true;
            } else if line.starts_with("end comment") || line.starts_with("end test") {
                in_block = false;
                i += 1;
                continue;
            }
        }
        if in_block {
            i += 1;
            continue;
        }
        let Some((date, len, sep)) = date_at(line) else {
            i += 1;
            continue;
        };
        let start = ls[i].start;
        let mut at = len;
        // A second date (`=2026-01-07`), hledger's and Ledger's.
        if dialect != Dialect::Beancount
            && line[at..].starts_with('=')
            && let Some((_, l2, _)) = date_at(&line[at + 1..])
        {
            at += 1 + l2;
        }
        let blanks = line[at..].len() - line[at..].trim_start_matches([' ', '\t']).len();
        if blanks == 0 && at < line.len() {
            i += 1;
            continue;
        }
        let after = at + blanks;
        let rest = &line[after..];
        let status = match dialect {
            Dialect::Beancount => {
                let Some(n) = beancount_flag(rest) else {
                    // A dated directive (`open`, `balance`…): no
                    // transaction.
                    i += 1;
                    continue;
                };
                let b = rest[n..].len() - rest[n..].trim_start_matches([' ', '\t']).len();
                Some(start + after..start + after + n + b)
            }
            _ => match rest.chars().next() {
                Some('*' | '!') if rest[1..].starts_with([' ', '\t']) || rest.len() == 1 => {
                    let b = rest[1..].len() - rest[1..].trim_start_matches([' ', '\t']).len();
                    Some(start + after..start + after + 1 + b)
                }
                _ => None,
            },
        };
        let mut j = i + 1;
        while j < ls.len()
            && indented(&text[ls[j].clone()])
            && !text[ls[j].clone()].trim().is_empty()
        {
            j += 1;
        }
        let end = if j < ls.len() {
            ls[j].start
        } else {
            text.len()
        };
        let mut c = i;
        while c > 0
            && comment_line(&text[ls[c - 1].clone()], dialect)
            && !indented(&text[ls[c - 1].clone()])
        {
            c -= 1;
        }
        out.push(Transaction {
            range: start..end,
            with_comments: ls[c].start..end,
            date,
            date_range: start..start + len,
            sep,
            status_at: start + after,
            status,
        });
        i = j;
    }
    out
}

/// The transaction holding byte `at`, by its lines.
pub fn at(txs: &[Transaction], at: usize) -> Option<&Transaction> {
    txs.iter()
        .find(|t| t.range.contains(&at) || (t.range.end == at && at > t.range.start))
}

/// What deleting the transaction at `at` takes: its comment lines
/// above it, its lines, and one blank line after it (else before it).
pub fn deletion(text: &str, dialect: Dialect, at: usize) -> Option<Range<usize>> {
    let txs = transactions(text, dialect);
    let t = self::at(&txs, at)?;
    let (start, end) = (t.with_comments.start, t.range.end);
    let blank = |s: &str| s.strip_suffix('\n').is_some_and(|l| l.trim().is_empty());
    let next = text[end..].split_inclusive('\n').next().unwrap_or("");
    if blank(next) {
        return Some(start..end + next.len());
    }
    let before = text[..start].trim_end_matches(['\n']);
    let line = text[..start].len() - before.len();
    if line >= 2 {
        // The blank line before it, its own line feed kept on the line
        // above.
        return Some(start - (line - 1)..end);
    }
    Some(start..end)
}

/// A status, as the cycle goes: unmarked, pending, cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unmarked,
    Pending,
    Cleared,
}

impl Status {
    fn next(self) -> Status {
        match self {
            Status::Unmarked => Status::Pending,
            Status::Pending => Status::Cleared,
            Status::Cleared => Status::Unmarked,
        }
    }

    fn of(mark: &str) -> Status {
        match mark.trim() {
            "!" => Status::Pending,
            "*" => Status::Cleared,
            _ => Status::Unmarked,
        }
    }

    /// The mark, with the blank after it; Beancount's unmarked
    /// transaction is `txn`.
    fn written(self, dialect: Dialect) -> &'static str {
        match (self, dialect) {
            (Status::Pending, _) => "! ",
            (Status::Cleared, _) => "* ",
            (Status::Unmarked, Dialect::Beancount) => "txn ",
            (Status::Unmarked, _) => "",
        }
    }
}

fn transaction_status(text: &str, t: &Transaction) -> Status {
    t.status
        .clone()
        .map_or(Status::Unmarked, |r| Status::of(&text[r]))
}

/// The posting line holding `at` and its status's place: after the
/// indent; the status written there, with its blank.
fn posting_status(
    text: &str,
    at: usize,
    dialect: Dialect,
) -> Option<(usize, Option<Range<usize>>)> {
    let ls = lines(text);
    let r = ls.iter().find(|r| r.start <= at && at <= r.end)?.clone();
    let line = &text[r.clone()];
    if !indented(line) || comment_line(line, dialect) || line.trim().is_empty() {
        return None;
    }
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = &line[indent..];
    // Beancount's metadata is no posting.
    if dialect == Dialect::Beancount && rest.starts_with(|c: char| c.is_ascii_lowercase()) {
        return None;
    }
    let mark = match rest.chars().next() {
        Some('*' | '!') if rest[1..].starts_with([' ', '\t']) => {
            let b = rest[1..].len() - rest[1..].trim_start_matches([' ', '\t']).len();
            Some(r.start + indent..r.start + indent + 1 + b)
        }
        _ => None,
    };
    Some((r.start + indent, mark))
}

/// Toggle Status: the transactions selected, or the one at the cursor,
/// or, with `postings`, the posting at the cursor, a step along the
/// cycle; the transactions selected all to the first one's next.
pub fn toggle(
    text: &str,
    dialect: Dialect,
    selection: Range<usize>,
    postings: bool,
) -> Vec<(Range<usize>, String)> {
    let txs = transactions(text, dialect);
    if postings
        && selection.is_empty()
        && let Some((at, mark)) = posting_status(text, selection.start, dialect)
    {
        let now = mark
            .clone()
            .map_or(Status::Unmarked, |r| Status::of(&text[r]));
        // A posting is never `txn`.
        let next = match now.next() {
            Status::Unmarked => "",
            s => s.written(dialect),
        };
        return vec![(mark.unwrap_or(at..at), next.to_string())];
    }
    let chosen: Vec<&Transaction> = if selection.is_empty() {
        at(&txs, selection.start).into_iter().collect()
    } else {
        txs.iter()
            .filter(|t| t.range.start < selection.end && selection.start < t.range.end)
            .collect()
    };
    let Some(first) = chosen.first() else {
        return Vec::new();
    };
    let next = transaction_status(text, first).next();
    chosen
        .iter()
        .filter(|t| transaction_status(text, t) != next)
        .map(|t| {
            let r = t.status.clone().unwrap_or(t.status_at..t.status_at);
            (r, next.written(dialect).to_string())
        })
        .collect()
}

/// The start of the next transaction after the one holding `at` (or
/// after `at`), or of the previous one.
pub fn neighbour(text: &str, dialect: Dialect, at: usize, forward: bool) -> Option<usize> {
    let txs = transactions(text, dialect);
    if forward {
        txs.iter().map(|t| t.range.start).find(|s| *s > at)
    } else {
        let here = self::at(&txs, at).map(|t| t.range.start);
        txs.iter()
            .map(|t| t.range.start)
            .rfind(|s| *s < at && Some(*s) != here)
    }
}

/// Where a transaction dated `date` goes in `text`: after the last
/// transaction dated on or before it, else before the first transaction
/// (with its comments), else at the end; at the end with `at_end`.
pub fn place(text: &str, dialect: Dialect, date: Date, at_end: bool) -> usize {
    let txs = transactions(text, dialect);
    if at_end || txs.is_empty() {
        return text.len();
    }
    match txs.iter().rposition(|t| t.date <= date) {
        Some(i) => txs[i].range.end,
        None => txs[0].with_comments.start,
    }
}

/// The text inserted at `at` to hold `transaction` (its lines, ending
/// with a line feed): a blank line before and after it where its
/// neighbours have text, a line feed first where the text does not end
/// with one. The transaction's start in what is inserted.
pub fn insertion(text: &str, at: usize, transaction: &str) -> (String, usize) {
    let mut s = String::new();
    let before = &text[..at];
    if !before.is_empty() && !before.ends_with('\n') {
        s.push('\n');
    }
    if !before.is_empty() && !before.ends_with("\n\n") {
        s.push('\n');
    }
    let start = s.len();
    s.push_str(transaction);
    if !transaction.ends_with('\n') {
        s.push('\n');
    }
    let after = &text[at..];
    if !after.is_empty() && !after.starts_with('\n') {
        s.push('\n');
    }
    (s, start)
}

/// Copy Transaction: the transaction holding `at` again, dated `today`
/// (as it writes its dates), placed by its date (or at the end); the
/// insertion's place and text, and the copy's start in the text.
pub fn copy(
    text: &str,
    dialect: Dialect,
    at: usize,
    today: Date,
    at_end: bool,
) -> Option<(usize, String, usize)> {
    let txs = transactions(text, dialect);
    let t = self::at(&txs, at)?;
    let body = &text[t.range.clone()];
    let rel = t.date_range.start - t.range.start..t.date_range.end - t.range.start;
    let copied = format!(
        "{}{}{}",
        &body[..rel.start],
        today.written(t.sep),
        &body[rel.end..]
    );
    let place = place(text, dialect, today, at_end);
    let (ins, start) = insertion(text, place, &copied);
    Some((place, ins, start))
}

/// The date of the transaction holding `at` moved by `days`: the edit.
pub fn shift_date(
    text: &str,
    dialect: Dialect,
    at: usize,
    days: i64,
) -> Option<(Range<usize>, String)> {
    let txs = transactions(text, dialect);
    let t = self::at(&txs, at)?;
    Some((t.date_range.clone(), t.date.plus(days).written(t.sep)))
}

/// Directives that change what the lines after them mean: sorting
/// across one could move a transaction out of its reach.
fn scoping(line: &str, dialect: Dialect) -> bool {
    let word = line.split_whitespace().next().unwrap_or("");
    let word = word.trim_start_matches(['!', '@']);
    match dialect {
        Dialect::Beancount => matches!(
            word,
            "pushtag" | "poptag" | "pushmeta" | "popmeta" | "include" | "option"
        ),
        _ => matches!(
            word,
            "apply"
                | "end"
                | "alias"
                | "Y"
                | "year"
                | "D"
                | "decimal-mark"
                | "include"
                | "bucket"
                | "A"
                | "fixed"
                | "endfixed"
                | "comment"
                | "test"
        ),
    }
}

/// Sort Transactions: those selected (or of the whole text), by date and
/// then as they were, each with the comment lines above it; what lies
/// between them stays where it is. Refused where a directive between
/// them (`apply account`, `alias`, `include`, `pushtag`…) would change
/// what one means. The edit, or none when they are in order.
pub fn sort(
    text: &str,
    dialect: Dialect,
    selection: Range<usize>,
) -> Result<Option<(Range<usize>, String)>, String> {
    // A last line without its line feed is sorted as if it had one.
    let owned;
    let (text, added) = if !text.is_empty() && !text.ends_with('\n') {
        owned = format!("{text}\n");
        (owned.as_str(), true)
    } else {
        (text, false)
    };
    let all = transactions(text, dialect);
    let chosen: Vec<&Transaction> = if selection.is_empty() {
        all.iter().collect()
    } else {
        all.iter()
            .filter(|t| t.range.start < selection.end && selection.start < t.range.end)
            .collect()
    };
    if chosen.len() < 2 {
        return Ok(None);
    }
    let from = chosen[0].with_comments.start;
    let to = chosen[chosen.len() - 1].range.end;
    // What lies between: gaps that stay.
    for (a, b) in chosen.iter().zip(chosen.iter().skip(1)) {
        let between = &text[a.range.end..b.with_comments.start];
        if let Some(l) = between.lines().find(|l| scoping(l, dialect)) {
            return Err(format!(
                "Not sorted: \"{}\" between the transactions would apply to others after sorting",
                l.trim()
            ));
        }
    }
    let mut order: Vec<usize> = (0..chosen.len()).collect();
    order.sort_by_key(|&i| (chosen[i].date, i));
    if order.iter().enumerate().all(|(i, &j)| i == j) {
        return Ok(None);
    }
    let mut out = String::new();
    for (slot, &i) in order.iter().enumerate() {
        if slot > 0 {
            out.push_str(&text[chosen[slot - 1].range.end..chosen[slot].with_comments.start]);
        }
        out.push_str(&text[chosen[i].with_comments.clone()]);
    }
    let mut to = to;
    if added && to == text.len() {
        // The line feed added at the end: not the file's.
        to -= 1;
        out.pop();
    }
    Ok(Some((from..to, out)))
}

/// The date the user typed, hledger's smart dates resolved against
/// `today`: `today`, `yesterday`, `tomorrow`, `+3`, `-1` (days), `5`
/// (this month's day), `10/5` or `10-5` (this year's), a full date, a
/// weekday (`friday`, `fri`: the last on or before today; `last fri`
/// before today; `next fri` after it); none for anything else.
pub fn smart_date(input: &str, today: Date) -> Option<Date> {
    let s = input.trim().to_ascii_lowercase();
    match s.as_str() {
        "" | "today" | "." => return Some(today),
        "yesterday" => return Some(today.plus(-1)),
        "tomorrow" => return Some(today.plus(1)),
        _ => {}
    }
    if let Some(n) = s.strip_prefix('+').and_then(|n| n.parse::<i64>().ok()) {
        return Some(today.plus(n));
    }
    if let Some(n) = s.strip_prefix('-').and_then(|n| n.parse::<i64>().ok()) {
        return Some(today.plus(-n));
    }
    if let Some((d, len, _)) = date_at(&s)
        && len == s.len()
    {
        return Some(d);
    }
    let parts: Vec<&str> = s.split(['/', '-', '.']).collect();
    let num = |p: &str| p.parse::<u32>().ok();
    match parts.as_slice() {
        [d] if num(d).is_some() => {
            let date = Date {
                d: num(d)?,
                ..today
            };
            return date.valid().then_some(date);
        }
        [m, d] if num(m).is_some() && num(d).is_some() => {
            let date = Date {
                y: today.y,
                m: num(m)?,
                d: num(d)?,
            };
            return date.valid().then_some(date);
        }
        _ => {}
    }
    let (which, day) = match s.split_once(' ') {
        Some((w @ ("last" | "next" | "this"), d)) => (w, d.trim()),
        _ => ("", s.as_str()),
    };
    const DAYS: [&str; 7] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    let wd = DAYS
        .iter()
        .position(|d| day.len() >= 3 && d.starts_with(day))? as i64;
    let now = i64::from(today.weekday());
    let back = (now - wd).rem_euclid(7);
    Some(match which {
        "next" => today.plus(if back == 0 { 7 } else { 7 - back }),
        "last" => today.plus(-(if back == 0 { 7 } else { back })),
        _ => today.plus(-back),
    })
}

/// The new transaction from the draft a tool gave for the payee (its
/// last transaction, as `hledger print --match`, `ledger xact` or
/// `bean-query`'s `PRINT` write it), dated `date` with `sep`: its first
/// transaction, without a code, a status (Beancount's flag kept), a
/// second date, or balance assertions; or, with no draft, the payee and
/// one posting line to fill. The text, and where its first amount (or
/// the posting's account) is in it.
pub fn new_transaction(
    dialect: Dialect,
    date: Date,
    sep: char,
    payee: &str,
    draft: Option<&str>,
) -> (String, usize) {
    let dated = date.written(sep);
    let draft = draft.and_then(|d| {
        let txs = transactions(d, dialect);
        txs.last().map(|t| d[t.range.clone()].to_string())
    });
    let Some(draft) = draft else {
        let head = match dialect {
            Dialect::Beancount => format!("{dated} * \"{}\" \"\"\n", payee.replace('"', "'")),
            _ => format!("{dated} {payee}\n"),
        };
        let indent = if dialect == Dialect::Beancount {
            "  "
        } else {
            "    "
        };
        let text = format!("{head}{indent}\n");
        let at = head.len() + indent.len();
        return (text, at);
    };
    let mut out = String::new();
    let mut first_amount = None;
    for (n, line) in draft.lines().enumerate() {
        if n == 0 {
            let (_, len, _) = date_at(line).unwrap_or((date, 0, sep));
            let mut rest = &line[len..];
            if rest.starts_with('=')
                && let Some((_, l2, _)) = date_at(&rest[1..])
            {
                rest = &rest[1 + l2..];
            }
            let mut rest = rest.trim_start();
            if dialect != Dialect::Beancount {
                if let Some(r) = rest.strip_prefix(['*', '!']) {
                    rest = r.trim_start();
                }
                if rest.starts_with('(')
                    && let Some(close) = rest.find(')')
                {
                    rest = rest[close + 1..].trim_start();
                }
            }
            out.push_str(&dated);
            out.push(' ');
            out.push_str(rest.trim_end());
            out.push('\n');
            continue;
        }
        // A posting's balance assertion: the old balance, not the new.
        let mut line = line.trim_end().to_string();
        if dialect != Dialect::Beancount
            && let Some(p) = crate::align::posting(&line, dialect)
        {
            let amount_at = p.gap.end;
            let tail = &line[amount_at..];
            if let Some(eq) = tail
                .find(" =")
                .or_else(|| tail.starts_with('=').then_some(0))
            {
                let cut = amount_at + eq;
                let comment = line[cut..].find(';').map(|c| line[cut + c..].to_string());
                line.truncate(cut);
                let trimmed = line.trim_end().len();
                line.truncate(trimmed);
                if let Some(c) = comment {
                    line.push_str("  ");
                    line.push_str(&c);
                }
            }
        }
        if first_amount.is_none()
            && let Some(p) = crate::align::posting(&line, dialect)
        {
            first_amount = Some(out.len() + p.gap.end);
        }
        out.push_str(&line);
        out.push('\n');
    }
    let at = first_amount.unwrap_or(out.find('\n').map_or(out.len(), |i| i + 1));
    (out, at)
}

/// The account of the posting on the line holding `at`: after its
/// indent and status, up to two blanks, a tab or a comment (one blank
/// in Beancount), a virtual posting's parentheses or brackets taken
/// off; none on another line.
pub fn account_at(text: &str, at: usize, dialect: Dialect) -> Option<String> {
    let start = text[..at.min(text.len())].rfind('\n').map_or(0, |i| i + 1);
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line = &text[start..end];
    if !indented(line) || comment_line(line, dialect) {
        return None;
    }
    let mut rest = line.trim_start();
    if dialect == Dialect::Beancount && rest.starts_with(|c: char| c.is_ascii_lowercase()) {
        return None;
    }
    if let Some(r) = rest.strip_prefix(['*', '!'])
        && r.starts_with([' ', '\t'])
    {
        rest = r.trim_start();
    }
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        let two = rest[i..].starts_with("  ");
        if c == '\t' || c == ';' || two || (dialect == Dialect::Beancount && c == ' ') {
            end = i;
            break;
        }
    }
    let account = rest[..end]
        .trim()
        .trim_start_matches(['(', '['])
        .trim_end_matches([')', ']']);
    (!account.is_empty()).then(|| account.to_string())
}

/// The separator the text writes its dates with: its first
/// transaction's, else the dialect's own.
pub fn separator(text: &str, dialect: Dialect) -> char {
    transactions(text, dialect)
        .first()
        .map_or(if dialect == Dialect::Ledger { '/' } else { '-' }, |t| {
            t.sep
        })
}

/// The payees of `text` as the tools write them in headers, the last
/// written first: hledger's and Ledger's description up to `|`,
/// Beancount's first quoted string when there are two.
pub fn payees_in(text: &str, dialect: Dialect) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in transactions(text, dialect).iter().rev() {
        let header = text[t.range.clone()].lines().next().unwrap_or("");
        let rest = &header[t.status.as_ref().map_or(t.status_at, |s| s.end) - t.range.start..];
        let payee = match dialect {
            Dialect::Beancount => {
                let quoted: Vec<&str> = rest.split('"').skip(1).step_by(2).collect();
                if quoted.len() >= 2 {
                    quoted[0].to_string()
                } else {
                    String::new()
                }
            }
            _ => {
                let mut r = rest.trim_start();
                if r.starts_with('(')
                    && let Some(c) = r.find(')')
                {
                    r = r[c + 1..].trim_start();
                }
                let r = r.split(';').next().unwrap_or("");
                r.split('|').next().unwrap_or("").trim().to_string()
            }
        };
        if !payee.is_empty() && !out.contains(&payee) {
            out.push(payee);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Dialect::*;

    const D: fn(i32, u32, u32) -> Date = |y, m, d| Date { y, m, d };

    fn apply(text: &str, edits: &[(Range<usize>, String)]) -> String {
        crate::align::apply(text, edits)
    }

    #[test]
    fn dates() {
        assert_eq!(D(1970, 1, 1).days(), 0);
        assert_eq!(D(2026, 10, 11).weekday(), 6, "a Sunday");
        assert_eq!(D(2026, 1, 31).plus(1), D(2026, 2, 1));
        assert_eq!(D(2024, 2, 28).plus(1), D(2024, 2, 29));
        assert_eq!(D(2026, 1, 1).plus(-1), D(2025, 12, 31));
        assert_eq!(date_at("2026/1/5 x"), Some((D(2026, 1, 5), 8, '/')));
        assert_eq!(date_at("2026-02-30"), None);
        assert_eq!(D(2026, 1, 5).written('/'), "2026/01/05");
        assert_eq!(Date::of_millis(1_791_676_800_000), D(2026, 10, 11));
    }

    #[test]
    fn smart_dates() {
        let today = D(2026, 10, 11); // a Sunday
        for (typed, want) in [
            ("", today),
            ("yesterday", D(2026, 10, 10)),
            ("tomorrow", D(2026, 10, 12)),
            ("-3", D(2026, 10, 8)),
            ("+1", D(2026, 10, 12)),
            ("5", D(2026, 10, 5)),
            ("10/5", D(2026, 10, 5)),
            ("1-31", D(2026, 1, 31)),
            ("2025-12-31", D(2025, 12, 31)),
            ("friday", D(2026, 10, 9)),
            ("fri", D(2026, 10, 9)),
            ("sun", today),
            ("last sunday", D(2026, 10, 4)),
            ("next mon", D(2026, 10, 12)),
            ("next sunday", D(2026, 10, 18)),
        ] {
            assert_eq!(smart_date(typed, today), Some(want), "{typed}");
        }
        for bad in ["2/30", "someday", "fr"] {
            assert_eq!(smart_date(bad, today), None, "{bad}");
        }
    }

    const H: &str = "; header comment\n\n; groceries\n2026-01-03 ! Market | groceries  ; food:\n    expenses:food  842,35 TRY\n    assets:bank\n\n2026-01-02 * (1001) Ziraat | salary\n    assets:bank  45.000,00 TRY = 93.749,60 TRY\n    income:salary\n\napply account Assets\n2026-01-01 Opening\n    x  1 TRY\n    y\nend apply account\n";

    #[test]
    fn the_transactions() {
        let txs = transactions(H, Hledger);
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].date, D(2026, 1, 3));
        assert_eq!(&H[txs[0].with_comments.clone()][..12], "; groceries\n");
        assert_eq!(&H[txs[0].status.clone().unwrap()], "! ");
        assert_eq!(txs[2].status, None);
        assert!(H[txs[1].range.clone()].ends_with("income:salary\n"));
        let b = "2025-01-01 open Assets:Cash TRY\n  note: \"x\"\n\n2026-01-15 txn \"Market\" \"Groceries\"\n  Expenses:Food  1 TRY\n  Assets:Cash\n";
        let txs = transactions(b, Beancount);
        assert_eq!(txs.len(), 1, "a directive is no transaction");
        assert_eq!(&b[txs[0].status.clone().unwrap()], "txn ");
    }

    #[test]
    fn toggling_a_status() {
        let at = H.find("Ziraat").unwrap();
        let once = apply(H, &toggle(H, Hledger, at..at, false));
        assert!(once.contains("2026-01-02 (1001) Ziraat"), "{once}");
        let twice = apply(&once, &toggle(&once, Hledger, at..at, false));
        assert!(twice.contains("2026-01-02 ! (1001) Ziraat"));
        let at = H.find("Opening").unwrap();
        let p = apply(H, &toggle(H, Hledger, at..at, false));
        assert!(p.contains("2026-01-01 ! Opening"));
        // A posting.
        let at = H.find("expenses:food").unwrap() + 2;
        let p = apply(H, &toggle(H, Hledger, at..at, true));
        assert!(p.contains("    ! expenses:food  842,35"), "{p}");
        let pp = apply(&p, &toggle(&p, Hledger, at..at, true));
        assert!(pp.contains("    * expenses:food  842,35"));
        let ppp = apply(&pp, &toggle(&pp, Hledger, at..at, true));
        assert_eq!(ppp, H);
        // The selection's all to the first's next.
        let all = apply(H, &toggle(H, Hledger, 0..H.len(), false));
        assert!(
            all.contains("2026-01-03 * Market")
                && all.contains("2026-01-02 * (1001)")
                && all.contains("2026-01-01 * Opening"),
            "{all}"
        );
        // Beancount: txn, then !, then *, then txn.
        let b = "2026-01-15 txn \"M\" \"G\"\n  A:B  1 TRY\n  C:D\n";
        let s1 = apply(b, &toggle(b, Beancount, 3..3, false));
        assert!(s1.starts_with("2026-01-15 ! \"M\""), "{s1}");
        let s3 = apply(&apply(&s1, &toggle(&s1, Beancount, 3..3, false)), &[]);
        let s3 = apply(&s3, &toggle(&s3, Beancount, 3..3, false));
        assert_eq!(s3, b);
    }

    #[test]
    fn moving_between_transactions() {
        let first = H.find("2026-01-03").unwrap();
        let second = H.find("2026-01-02").unwrap();
        assert_eq!(neighbour(H, Hledger, 0, true), Some(first));
        assert_eq!(neighbour(H, Hledger, first + 5, true), Some(second));
        assert_eq!(neighbour(H, Hledger, second + 30, false), Some(first));
        assert_eq!(neighbour(H, Hledger, first + 3, false), None);
    }

    #[test]
    fn copying_and_dates() {
        let t = "2026-01-02 * (1001) Ziraat | salary\n    assets:bank  45.000,00 TRY\n    income:salary\n\n2026-01-20 Rent\n    expenses:rent  15.000,00 TRY\n    assets:bank\n";
        let at = t.find("Ziraat").unwrap();
        // Copied at the 10th: after the 2nd, before the 20th.
        let (place, ins, start) = copy(t, Hledger, at, D(2026, 1, 10), false).unwrap();
        assert_eq!(&ins[start..start + 10], "2026-01-10");
        let out = format!("{}{}{}", &t[..place], ins, &t[place..]);
        assert!(out.contains("income:salary\n\n2026-01-10 * (1001) Ziraat | salary\n    assets:bank  45.000,00 TRY\n    income:salary\n\n2026-01-20 Rent"), "{out}");
        // At the end, with `insert_at` end.
        let (place, _, _) = copy(t, Hledger, at, D(2026, 1, 10), true).unwrap();
        assert_eq!(place, t.len());
        let e = shift_date(t, Hledger, at, 30).unwrap();
        assert_eq!(e.1, "2026-02-01");
        assert_eq!(&t[e.0], "2026-01-02");
    }

    #[test]
    fn sorting() {
        // Refused across `apply account`.
        assert!(
            sort(H, Hledger, 0..0)
                .unwrap_err()
                .contains("apply account")
        );
        // The first two alone: sorted, each with its comment.
        let sel = 0..H.find("apply").unwrap();
        let (r, s) = sort(H, Hledger, sel).unwrap().unwrap();
        let out = format!("{}{}{}", &H[..r.start], s, &H[r.end..]);
        assert!(
            out.starts_with("; header comment\n\n2026-01-02 * (1001) Ziraat"),
            "{out}"
        );
        assert!(
            out.contains("income:salary\n\n; groceries\n2026-01-03 ! Market"),
            "{out}"
        );
        assert_eq!(out.len(), H.len());
        // Sorted again: nothing.
        let sel = 0..out.find("apply").unwrap();
        assert_eq!(sort(&out, Hledger, sel).unwrap(), None);
        // A last line without its line feed.
        let t = "2026-01-02 b\n    x  1\n    y\n\n2026-01-01 a\n    x  1\n    y";
        let (r, s) = sort(t, Hledger, 0..0).unwrap().unwrap();
        let out = format!("{}{}{}", &t[..r.start], s, &t[r.end..]);
        assert_eq!(
            out,
            "2026-01-01 a\n    x  1\n    y\n\n2026-01-02 b\n    x  1\n    y"
        );
    }

    #[test]
    fn placing_a_new_transaction() {
        assert_eq!(
            place(H, Hledger, D(2026, 1, 2), false),
            // After the last in the file dated on or before it: in `H`,
            // out of order, the 1st's, not the 2nd's.
            transactions(H, Hledger)[2].range.end
        );
        let t = "2026-01-05 a\n    x  1\n    y\n";
        assert_eq!(place(t, Hledger, D(2026, 1, 1), false), 0);
        let (ins, start) = insertion(t, t.len(), "2026-01-06 b\n    x\n");
        assert_eq!(ins, "\n2026-01-06 b\n    x\n");
        assert_eq!(start, 1);
        let (ins, _) = insertion(t, 0, "2026-01-01 z\n    x\n");
        assert_eq!(ins, "2026-01-01 z\n    x\n\n");
    }

    #[test]
    fn a_new_transaction_from_a_draft() {
        let draft = "2026-01-02 * (1001) Ziraat | salary  ; payroll:january\n    assets:bank:ziraat    45.000,00 TRY = 93.749,60 TRY\n    income:salary\n\n";
        let (t, at) = new_transaction(Hledger, D(2026, 2, 2), '-', "Ziraat", Some(draft));
        assert_eq!(
            t,
            "2026-02-02 Ziraat | salary  ; payroll:january\n    assets:bank:ziraat    45.000,00 TRY\n    income:salary\n"
        );
        assert_eq!(&t[at..at + 9], "45.000,00");
        // No draft: the payee and a posting to fill.
        let (t, at) = new_transaction(Ledger, D(2026, 2, 2), '/', "Client B", None);
        assert_eq!(t, "2026/02/02 Client B\n    \n");
        assert_eq!(at, t.len() - 1);
        let (t, _) = new_transaction(Beancount, D(2026, 2, 2), '-', "Market", None);
        assert_eq!(t, "2026-02-02 * \"Market\" \"\"\n  \n");
        // bean-query's PRINT: the last of several, its flag kept.
        let b = "2026-01-03 * \"Market\" \"one\"\n  Expenses:Food  1.00 TRY\n  Assets:Cash\n\n2026-01-15 ! \"Market\" \"two\"\n  Expenses:Food  842.35 TRY\n  Assets:Cash  -842.35 TRY\n";
        let (t, at) = new_transaction(Beancount, D(2026, 2, 1), '-', "Market", Some(b));
        assert!(t.starts_with("2026-02-01 ! \"Market\" \"two\"\n"), "{t}");
        assert_eq!(&t[at..at + 6], "842.35");
    }

    #[test]
    fn a_deletion() {
        let j = "2026-01-01 A\n    a  1\n    b\n\n; note\n2026-01-02 B\n    a  2\n    b\n\n2026-01-03 C\n    a  3\n    b\n";
        let del = |at: usize| {
            let r = deletion(j, Dialect::Hledger, at).expect("a transaction");
            format!("{}{}", &j[..r.start], &j[r.end..])
        };
        // The middle one with its comment and the blank after it.
        assert_eq!(
            del(j.find("B").unwrap()),
            "2026-01-01 A\n    a  1\n    b\n\n2026-01-03 C\n    a  3\n    b\n"
        );
        // The last one with the blank before it.
        assert_eq!(
            del(j.find("C").unwrap()),
            "2026-01-01 A\n    a  1\n    b\n\n; note\n2026-01-02 B\n    a  2\n    b\n"
        );
        assert_eq!(
            deletion(j, Dialect::Hledger, j.find("; note").unwrap() + 2),
            None
        );
    }

    #[test]
    fn the_account_at_the_cursor() {
        let at = |line: &str| H.find(line).unwrap() + 2;
        assert_eq!(
            account_at(H, at("    expenses:food"), Hledger).as_deref(),
            Some("expenses:food")
        );
        assert_eq!(
            account_at(H, at("    income:salary"), Hledger).as_deref(),
            Some("income:salary")
        );
        assert_eq!(account_at(H, H.find("Ziraat").unwrap(), Hledger), None);
        let t =
            "2026-01-01 x\n    (budget:food)  *-1\n    * assets:broker:index fund  10 \"FUND A\"\n";
        assert_eq!(
            account_at(t, t.find("budget").unwrap(), Hledger).as_deref(),
            Some("budget:food")
        );
        assert_eq!(
            account_at(t, t.find("index").unwrap(), Hledger).as_deref(),
            Some("assets:broker:index fund")
        );
        let b = "2026-01-02 * \"Z\" \"S\"\n  receipt: \"x\"\n  Assets:Bank 1 TRY\n";
        assert_eq!(
            account_at(b, b.find("Bank").unwrap(), Beancount).as_deref(),
            Some("Assets:Bank")
        );
        assert_eq!(account_at(b, b.find("receipt").unwrap(), Beancount), None);
    }

    #[test]
    fn the_payees_of_a_text() {
        assert_eq!(payees_in(H, Hledger), ["Opening", "Ziraat", "Market"]);
        let b = "2026-01-02 * \"Ziraat\" \"Salary\"\n  A:B  1 TRY\n  C:D\n\n2026-01-03 * \"Narration only\"\n  A:B  1 TRY\n  C:D\n";
        assert_eq!(payees_in(b, Beancount), ["Ziraat"]);
        assert_eq!(separator("2026/01/02 x\n", Ledger), '/');
        assert_eq!(separator("", Ledger), '/');
    }
}
