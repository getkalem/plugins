//! Calendar dates, and the date patterns journals are named by: Logseq's
//! (`yyyy_MM_dd`, `MMM do, yyyy`, the tokens of cljs-time and date-fns)
//! and Obsidian's (`YYYY-MM-DD`, Moment's tokens). A pattern formats a
//! date and parses a name back, so that `journals/2026_10_03.md` and the
//! reference `[[Oct 3rd, 2026]]` are the same day.

use std::fmt;

/// A day of the proleptic Gregorian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// The year.
    pub year: i32,
    /// 1 to 12.
    pub month: u8,
    /// 1 to the month's length.
    pub day: u8,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

fn leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn month_len(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    /// The date, when it is one.
    pub fn new(year: i32, month: u8, day: u8) -> Option<Date> {
        ((1..=12).contains(&month)
            && day >= 1
            && day <= month_len(year, month)
            && (0..=9999).contains(&year))
        .then_some(Date { year, month, day })
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    pub fn to_days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01.
    pub fn from_days(days: i64) -> Date {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        Date {
            year: (y + i64::from(m <= 2)) as i32,
            month: m as u8,
            day: d as u8,
        }
    }

    /// The date `n` days later (earlier when negative).
    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.to_days() + n)
    }

    /// 0 for Monday to 6 for Sunday.
    pub fn weekday(self) -> u8 {
        // 1970-01-01 was a Thursday.
        (self.to_days() + 3).rem_euclid(7) as u8
    }

    /// `2026-10-03`.
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `2026-10-03` read back.
    pub fn parse_iso(s: &str) -> Option<Date> {
        let s = s.trim();
        let mut it = s.splitn(3, '-');
        let (y, m, d) = (it.next()?, it.next()?, it.next()?);
        if y.len() != 4 || m.is_empty() || m.len() > 2 || d.is_empty() || d.len() > 2 {
            return None;
        }
        Date::new(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.iso())
    }
}

/// `1st`, `2nd`, `3rd`, `4th`, `11th`, `21st`.
fn ordinal(n: u8) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Whose tokens a pattern is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Logseq's: `yyyy`, `MM`, `dd`, `do`, `MMM`, `EEE`, text in `'…'`.
    Logseq,
    /// Obsidian's, Moment's: `YYYY`, `MM`, `DD`, `Do`, `MMM`, `ddd`, text
    /// in `[…]`.
    Moment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Year4,
    Year2,
    Month2,
    Month1,
    MonthShort,
    MonthLong,
    Day2,
    Day1,
    DayOrdinal,
    WeekdayShort,
    WeekdayLong,
    Text(String),
}

/// A date pattern: `yyyy_MM_dd`, `MMM do, yyyy`, `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    tokens: Vec<Token>,
    source: String,
}

impl Pattern {
    /// `source` read in `dialect`'s tokens; a letter that is no token is
    /// text.
    pub fn new(source: &str, dialect: Dialect) -> Pattern {
        let chars: Vec<char> = source.chars().collect();
        let mut tokens = Vec::new();
        let mut text = String::new();
        let mut i = 0;
        let run = |i: usize, c: char| chars[i..].iter().take_while(|x| **x == c).count();
        while i < chars.len() {
            let c = chars[i];
            // Quoted text.
            let (open, close) = match dialect {
                Dialect::Logseq => ('\'', '\''),
                Dialect::Moment => ('[', ']'),
            };
            if c == open {
                let end = chars[i + 1..]
                    .iter()
                    .position(|x| *x == close)
                    .map_or(chars.len(), |p| i + 1 + p);
                text.extend(&chars[i + 1..end]);
                i = end + 1;
                continue;
            }
            let n = run(i, c);
            let token = match (dialect, c, n) {
                (Dialect::Logseq, 'y', 4..) | (Dialect::Moment, 'Y', 4..) => Some(Token::Year4),
                (Dialect::Logseq, 'y', 2) | (Dialect::Moment, 'Y', 2) => Some(Token::Year2),
                (_, 'M', 4..) => Some(Token::MonthLong),
                (_, 'M', 3) => Some(Token::MonthShort),
                (_, 'M', 2) => Some(Token::Month2),
                (_, 'M', 1) => Some(Token::Month1),
                (Dialect::Logseq, 'd', 2) | (Dialect::Moment, 'D', 2) => Some(Token::Day2),
                (Dialect::Logseq, 'd', 1) | (Dialect::Moment, 'D', 1)
                    if chars.get(i + 1) == Some(&'o') =>
                {
                    Some(Token::DayOrdinal)
                }
                (Dialect::Logseq, 'd', 1) | (Dialect::Moment, 'D', 1) => Some(Token::Day1),
                (Dialect::Logseq, 'E', 4..) | (Dialect::Moment, 'd', 4..) => {
                    Some(Token::WeekdayLong)
                }
                (Dialect::Logseq, 'E', 1..=3) | (Dialect::Moment, 'd', 3) => {
                    Some(Token::WeekdayShort)
                }
                _ => None,
            };
            match token {
                Some(t) => {
                    if !text.is_empty() {
                        tokens.push(Token::Text(std::mem::take(&mut text)));
                    }
                    let width = if t == Token::DayOrdinal { 2 } else { n };
                    tokens.push(t);
                    i += width;
                }
                None => {
                    text.push(c);
                    i += 1;
                }
            }
        }
        if !text.is_empty() {
            tokens.push(Token::Text(text));
        }
        Pattern {
            tokens,
            source: source.to_string(),
        }
    }

    /// The pattern as it was written.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// `date` written in the pattern.
    pub fn format(&self, date: Date) -> String {
        let mut out = String::new();
        for t in &self.tokens {
            match t {
                Token::Year4 => out.push_str(&format!("{:04}", date.year)),
                Token::Year2 => out.push_str(&format!("{:02}", date.year.rem_euclid(100))),
                Token::Month2 => out.push_str(&format!("{:02}", date.month)),
                Token::Month1 => out.push_str(&date.month.to_string()),
                Token::MonthShort => out.push_str(&MONTHS[usize::from(date.month) - 1][..3]),
                Token::MonthLong => out.push_str(MONTHS[usize::from(date.month) - 1]),
                Token::Day2 => out.push_str(&format!("{:02}", date.day)),
                Token::Day1 => out.push_str(&date.day.to_string()),
                Token::DayOrdinal => out.push_str(&ordinal(date.day)),
                Token::WeekdayShort => {
                    out.push_str(&WEEKDAYS[usize::from(date.weekday())][..3]);
                }
                Token::WeekdayLong => out.push_str(WEEKDAYS[usize::from(date.weekday())]),
                Token::Text(s) => out.push_str(s),
            }
        }
        out
    }

    /// The date `s` names in the pattern, ignoring case; `None` when it
    /// is not the pattern's or no day.
    pub fn parse(&self, s: &str) -> Option<Date> {
        let lower = s.to_lowercase();
        let b = lower.as_bytes();
        let mut at = 0;
        let (mut year, mut month, mut day) = (None, None, None);
        let digits = |at: usize, min: usize, max: usize| -> Option<(u32, usize)> {
            let n = b[at..]
                .iter()
                .take(max)
                .take_while(|c| c.is_ascii_digit())
                .count();
            (n >= min).then(|| (lower[at..at + n].parse().unwrap_or(0), at + n))
        };
        let name = |at: usize, names: &[&str], short: bool| -> Option<(usize, usize)> {
            // The longest name that matches, so that "May" is not "Ma".
            names
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    let n = n.to_lowercase();
                    let n = if short { n[..3].to_string() } else { n };
                    lower[at..].starts_with(&n).then_some((i, at + n.len()))
                })
                .max_by_key(|(_, end)| *end)
        };
        for t in &self.tokens {
            match t {
                Token::Year4 => {
                    let (v, next) = digits(at, 4, 4)?;
                    year = Some(v as i32);
                    at = next;
                }
                Token::Year2 => {
                    let (v, next) = digits(at, 2, 2)?;
                    year = Some(2000 + v as i32);
                    at = next;
                }
                Token::Month2 => {
                    let (v, next) = digits(at, 2, 2)?;
                    month = Some(v as u8);
                    at = next;
                }
                Token::Month1 => {
                    let (v, next) = digits(at, 1, 2)?;
                    month = Some(v as u8);
                    at = next;
                }
                Token::MonthShort | Token::MonthLong => {
                    let (i, next) = name(at, &MONTHS, *t == Token::MonthShort)?;
                    month = Some(i as u8 + 1);
                    at = next;
                }
                Token::Day2 => {
                    let (v, next) = digits(at, 2, 2)?;
                    day = Some(v as u8);
                    at = next;
                }
                Token::Day1 => {
                    let (v, next) = digits(at, 1, 2)?;
                    day = Some(v as u8);
                    at = next;
                }
                Token::DayOrdinal => {
                    let (v, next) = digits(at, 1, 2)?;
                    let suffix = lower.get(next..next + 2)?;
                    if !matches!(suffix, "st" | "nd" | "rd" | "th") {
                        return None;
                    }
                    day = Some(v as u8);
                    at = next + 2;
                }
                Token::WeekdayShort | Token::WeekdayLong => {
                    let (_, next) = name(at, &WEEKDAYS, *t == Token::WeekdayShort)?;
                    at = next;
                }
                Token::Text(text) => {
                    let text = text.to_lowercase();
                    if !lower[at..].starts_with(&text) {
                        return None;
                    }
                    at += text.len();
                }
            }
        }
        if at != lower.len() {
            return None;
        }
        Date::new(year?, month?, day?)
    }
}

/// A date the user typed: `2026-10-03`, `today`, `yesterday`,
/// `tomorrow`, `+2`, `-3`, a weekday (the last one on or before `today`),
/// `oct 3` or `3 oct` (this year).
pub fn parse_input(s: &str, today: Date) -> Option<Date> {
    let s = s.trim().to_lowercase();
    if let Some(d) = Date::parse_iso(&s) {
        return Some(d);
    }
    match s.as_str() {
        "" | "today" => return Some(today),
        "yesterday" => return Some(today.add_days(-1)),
        "tomorrow" => return Some(today.add_days(1)),
        _ => {}
    }
    if let Some(n) = s.strip_prefix('+').and_then(|n| n.parse::<i64>().ok()) {
        return Some(today.add_days(n));
    }
    if let Some(n) = s.strip_prefix('-').and_then(|n| n.parse::<i64>().ok()) {
        return Some(today.add_days(-n));
    }
    if let Some(w) = WEEKDAYS
        .iter()
        .position(|w| w.to_lowercase() == s || w[..3].to_lowercase() == s)
    {
        let back = (i64::from(today.weekday()) - w as i64).rem_euclid(7);
        return Some(today.add_days(-back));
    }
    let words: Vec<&str> = s.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    if words.len() == 2 {
        let month = |w: &str| {
            MONTHS
                .iter()
                .position(|m| w.len() >= 3 && m.to_lowercase().starts_with(w))
                .map(|i| i as u8 + 1)
        };
        let day = |w: &str| {
            w.trim_end_matches(|c: char| c.is_ascii_alphabetic())
                .parse::<u8>()
                .ok()
        };
        let (m, d) = match (month(words[0]), month(words[1])) {
            (Some(m), _) => (m, day(words[1])?),
            (_, Some(m)) => (m, day(words[0])?),
            _ => return None,
        };
        return Date::new(today.year, m, d);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u8, day: u8) -> Date {
        Date::new(y, m, day).unwrap()
    }

    #[test]
    fn days_and_weekdays() {
        assert_eq!(d(1970, 1, 1).to_days(), 0);
        assert_eq!(Date::from_days(0), d(1970, 1, 1));
        assert_eq!(d(2026, 10, 10).weekday(), 5, "a Saturday");
        assert_eq!(d(2024, 2, 28).add_days(1), d(2024, 2, 29));
        assert_eq!(d(2026, 12, 31).add_days(1), d(2027, 1, 1));
        for n in [-800_000, -1, 0, 1, 20_000, 2_900_000] {
            assert_eq!(Date::from_days(n).to_days(), n);
        }
        assert!(Date::new(2026, 2, 29).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
    }

    #[test]
    fn logseq_patterns() {
        let file = Pattern::new("yyyy_MM_dd", Dialect::Logseq);
        assert_eq!(file.format(d(2026, 10, 3)), "2026_10_03");
        assert_eq!(file.parse("2026_10_03"), Some(d(2026, 10, 3)));
        assert_eq!(file.parse("2026_10_3"), None);
        let title = Pattern::new("MMM do, yyyy", Dialect::Logseq);
        assert_eq!(title.format(d(2026, 10, 3)), "Oct 3rd, 2026");
        assert_eq!(title.format(d(2026, 10, 11)), "Oct 11th, 2026");
        assert_eq!(title.format(d(2026, 10, 22)), "Oct 22nd, 2026");
        assert_eq!(title.parse("oct 3rd, 2026"), Some(d(2026, 10, 3)));
        assert_eq!(title.parse("Oct 3rd 2026"), None);
        let long = Pattern::new("EEEE, dd.MM.yyyy", Dialect::Logseq);
        assert_eq!(long.format(d(2026, 10, 10)), "Saturday, 10.10.2026");
        assert_eq!(long.parse("Saturday, 10.10.2026"), Some(d(2026, 10, 10)));
        let quoted = Pattern::new("'Day' yyyy-MM-dd", Dialect::Logseq);
        assert_eq!(quoted.format(d(2026, 1, 2)), "Day 2026-01-02");
        let may = Pattern::new("MMMM d, yyyy", Dialect::Logseq);
        assert_eq!(may.parse("May 4, 2026"), Some(d(2026, 5, 4)));
        assert_eq!(may.parse("March 4, 2026"), Some(d(2026, 3, 4)));
    }

    #[test]
    fn moment_patterns() {
        let p = Pattern::new("YYYY-MM-DD", Dialect::Moment);
        assert_eq!(p.format(d(2026, 10, 3)), "2026-10-03");
        assert_eq!(p.parse("2026-10-03"), Some(d(2026, 10, 3)));
        let p = Pattern::new("dddd, MMMM Do YYYY", Dialect::Moment);
        assert_eq!(p.format(d(2026, 10, 3)), "Saturday, October 3rd 2026");
        assert_eq!(p.parse("saturday, october 3rd 2026"), Some(d(2026, 10, 3)));
        let p = Pattern::new("[Week] YYYY-MM-DD", Dialect::Moment);
        assert_eq!(p.format(d(2026, 10, 3)), "Week 2026-10-03");
    }

    #[test]
    fn typed_dates() {
        let today = d(2026, 10, 10);
        assert_eq!(parse_input("2026-10-03", today), Some(d(2026, 10, 3)));
        assert_eq!(parse_input("", today), Some(today));
        assert_eq!(parse_input("yesterday", today), Some(d(2026, 10, 9)));
        assert_eq!(parse_input("-3", today), Some(d(2026, 10, 7)));
        assert_eq!(parse_input("+1", today), Some(d(2026, 10, 11)));
        assert_eq!(parse_input("monday", today), Some(d(2026, 10, 5)));
        assert_eq!(parse_input("sat", today), Some(today));
        assert_eq!(parse_input("oct 3", today), Some(d(2026, 10, 3)));
        assert_eq!(parse_input("3 october", today), Some(d(2026, 10, 3)));
        assert_eq!(parse_input("3rd oct", today), Some(d(2026, 10, 3)));
        assert_eq!(parse_input("someday", today), None);
    }
}
