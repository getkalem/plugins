//! `git blame --porcelain` read: every line of the file with the commit
//! that last changed it, and each commit's details once.

use std::collections::BTreeMap;

/// The hash git gives the lines not committed yet.
pub const UNCOMMITTED: &str = "0000000000000000000000000000000000000000";

/// A blamed file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Blame {
    /// Its lines, in order.
    pub lines: Vec<BlameLine>,
    /// The commits the lines name, by hash.
    pub commits: BTreeMap<String, BlameCommit>,
}

/// A line of a blamed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// The commit that last changed it.
    pub hash: String,
    /// Its number in that commit's version of the file, from 1.
    pub orig_line: u32,
    /// Its number in the file, from 1.
    pub line: u32,
    /// Its text, without the newline.
    pub text: Vec<u8>,
}

/// What a blame says of a commit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlameCommit {
    /// Its author.
    pub author: String,
    /// When, in seconds since 1970.
    pub author_time: i64,
    /// The author's time zone, `+0300`.
    pub author_tz: String,
    /// Its subject.
    pub summary: String,
    /// The commit before it and the file's name there, for blaming
    /// further back.
    pub previous: Option<(String, String)>,
    /// The file's name in this commit.
    pub filename: String,
    /// It is the oldest commit the blame went to.
    pub boundary: bool,
}

impl BlameCommit {
    /// The day it was written, `2026-10-07`, in its author's time zone.
    pub fn day(&self) -> String {
        let offset = tz_seconds(&self.author_tz);
        day_of(self.author_time + offset)
    }
}

fn tz_seconds(tz: &str) -> i64 {
    let (sign, digits) = match tz.as_bytes().first() {
        Some(b'-') => (-1, &tz[1..]),
        Some(b'+') => (1, &tz[1..]),
        _ => return 0,
    };
    let (h, m) = digits.split_at(digits.len().min(2));
    sign * (h.parse::<i64>().unwrap_or(0) * 3600 + m.parse::<i64>().unwrap_or(0) * 60)
}

/// `YYYY-MM-DD` of a time in seconds since 1970 (Howard Hinnant's
/// civil-from-days).
pub fn day_of(seconds: i64) -> String {
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Reads a blame.
pub fn parse(out: &[u8]) -> Result<Blame, String> {
    let mut blame = Blame::default();
    let mut current: Option<(String, u32, u32)> = None;
    for line in out.split(|&b| b == b'\n') {
        if let Some(text) = line.strip_prefix(b"\t") {
            let (hash, orig_line, final_line) =
                current.take().ok_or("a blamed line without its commit")?;
            blame.lines.push(BlameLine {
                hash,
                orig_line,
                line: final_line,
                text: text.to_vec(),
            });
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(line);
        if current.is_none() {
            let mut f = text.split(' ');
            let (Some(hash), Some(orig), Some(fin)) = (f.next(), f.next(), f.next()) else {
                return Err(format!("a blame header unreadable: {text}"));
            };
            let (Ok(orig), Ok(fin)) = (orig.parse(), fin.parse()) else {
                return Err(format!("a blame header unreadable: {text}"));
            };
            blame.commits.entry(hash.to_string()).or_default();
            current = Some((hash.to_string(), orig, fin));
            continue;
        }
        let hash = &current.as_ref().map(|c| c.0.clone()).unwrap_or_default();
        let c = blame.commits.entry(hash.clone()).or_default();
        let (key, value) = text.split_once(' ').unwrap_or((&text, ""));
        match key {
            "author" => c.author = value.to_string(),
            "author-time" => c.author_time = value.parse().unwrap_or(0),
            "author-tz" => c.author_tz = value.to_string(),
            "summary" => c.summary = value.to_string(),
            "filename" => c.filename = value.to_string(),
            "boundary" => c.boundary = true,
            "previous" => {
                c.previous = value
                    .split_once(' ')
                    .map(|(h, f)| (h.to_string(), f.to_string()));
            }
            _ => {}
        }
    }
    Ok(blame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_commits() {
        let out = "\
a1b2c3d4e5 1 1 2
author Ayşe
author-mail <a@x>
author-time 1759827600
author-tz +0300
committer Ayşe
summary Align the columns
previous 9999999999 table.rs
filename src/table.rs
\tfn align() {
a1b2c3d4e5 2 2
filename src/table.rs
\t    x
0000000000000000000000000000000000000000 3 3 1
author Not Committed Yet
author-time 1759900000
author-tz +0000
summary Version of src/table.rs from src/table.rs
filename src/table.rs
\t}
";
        let b = parse(out.as_bytes()).unwrap();
        assert_eq!(b.lines.len(), 3);
        assert_eq!(b.lines[1].hash, "a1b2c3d4e5");
        assert_eq!(b.lines[1].text, b"    x");
        let c = &b.commits["a1b2c3d4e5"];
        assert_eq!(c.author, "Ayşe");
        assert_eq!(c.summary, "Align the columns");
        assert_eq!(c.previous, Some(("9999999999".into(), "table.rs".into())));
        assert_eq!(c.day(), "2025-10-07");
        assert!(b.commits.contains_key(UNCOMMITTED));
    }

    #[test]
    fn days() {
        assert_eq!(day_of(0), "1970-01-01");
        assert_eq!(day_of(951_782_400), "2000-02-29");
        assert_eq!(day_of(-1), "1969-12-31");
        assert_eq!(tz_seconds("-0130"), -5400);
    }
}
