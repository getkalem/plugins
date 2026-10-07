//! `git log` in the plugin's own format: a record separator, then fields
//! separated by NUL bytes, one commit a line, so that `--graph`'s columns
//! stand before the separator and its lines between commits stand alone.

/// The format [`crate::git::cmd::log`] asks for.
pub const FORMAT: &str = "--format=%x1e%H%x00%h%x00%an%x00%ae%x00%aI%x00%P%x00%D%x00%s";

/// A commit of a log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Commit {
    /// The graph's columns before it, with `--graph`.
    pub graph: String,
    /// Its full hash.
    pub hash: String,
    /// Its short hash.
    pub short: String,
    /// Its author's name.
    pub author: String,
    /// Its author's address.
    pub email: String,
    /// When it was written, ISO 8601 (`2026-10-07T18:56:47+03:00`).
    pub date: String,
    /// Its parents' hashes.
    pub parents: Vec<String>,
    /// The references at it (`HEAD -> main`, `origin/main`, `tag: v1`).
    pub refs: Vec<String>,
    /// Its subject line.
    pub subject: String,
}

impl Commit {
    /// The day it was written, `2026-10-07`.
    pub fn day(&self) -> &str {
        self.date.get(..10).unwrap_or(&self.date)
    }
}

/// A line of a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogLine {
    /// A commit.
    Commit(Commit),
    /// A line of the graph alone, between commits.
    Graph(String),
}

/// Reads a log.
pub fn parse(out: &[u8]) -> Result<Vec<LogLine>, String> {
    let mut v = Vec::new();
    for line in out.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Some(sep) = line.iter().position(|&b| b == 0x1e) else {
            v.push(LogLine::Graph(String::from_utf8_lossy(line).into_owned()));
            continue;
        };
        let graph = String::from_utf8_lossy(&line[..sep]).into_owned();
        let fields: Vec<String> = line[sep + 1..]
            .split(|&b| b == 0)
            .map(|f| String::from_utf8_lossy(f).into_owned())
            .collect();
        let [hash, short, author, email, date, parents, refs, subject] = &fields[..] else {
            return Err(format!(
                "a log line with {} fields: {}",
                fields.len(),
                String::from_utf8_lossy(line)
            ));
        };
        v.push(LogLine::Commit(Commit {
            graph,
            hash: hash.clone(),
            short: short.clone(),
            author: author.clone(),
            email: email.clone(),
            date: date.clone(),
            parents: parents.split_whitespace().map(str::to_string).collect(),
            refs: refs
                .split(", ")
                .filter(|r| !r.is_empty())
                .map(str::to_string)
                .collect(),
            subject: subject.clone(),
        }));
    }
    Ok(v)
}

/// The commits of a log, its graph lines left out.
pub fn commits(out: &[u8]) -> Result<Vec<Commit>, String> {
    Ok(parse(out)?
        .into_iter()
        .filter_map(|l| match l {
            LogLine::Commit(c) => Some(c),
            LogLine::Graph(_) => None,
        })
        .collect())
}

/// A revision of a file, from [`crate::git::cmd::file_revisions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// Its full hash.
    pub hash: String,
    /// Its short hash.
    pub short: String,
    /// Its author's name.
    pub author: String,
    /// When, ISO 8601.
    pub date: String,
    /// Its subject.
    pub subject: String,
}

/// Reads the revisions of a file, newest first.
pub fn parse_revisions(out: &[u8]) -> Vec<Revision> {
    out.split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let f: Vec<String> = l
                .split(|&b| b == 0)
                .map(|f| String::from_utf8_lossy(f).into_owned())
                .collect();
            let [hash, short, author, date, subject] = &f[..] else {
                return None;
            };
            Some(Revision {
                hash: hash.clone(),
                short: short.clone(),
                author: author.clone(),
                date: date.clone(),
                subject: subject.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fields: &[&str]) -> String {
        format!("\x1e{}", fields.join("\0"))
    }

    #[test]
    fn commits_with_and_without_the_graph() {
        let a = record(&[
            "a1b2c3d4",
            "a1b2c3d",
            "Ayşe",
            "a@x",
            "2026-10-07T10:00:00+03:00",
            "p1 p2",
            "HEAD -> main, origin/main, tag: v1",
            "Merge: it",
        ]);
        let b = record(&[
            "e4f5",
            "e4f",
            "Bo",
            "b@x",
            "2026-10-06T10:00:00Z",
            "",
            "",
            "First",
        ]);
        let out = format!("*   {a}\n|\\  \n| * {b}\n");
        let v = parse(out.as_bytes()).unwrap();
        assert_eq!(v.len(), 3);
        let LogLine::Commit(c) = &v[0] else { panic!() };
        assert_eq!(c.graph, "*   ");
        assert_eq!(c.parents, ["p1", "p2"]);
        assert_eq!(c.refs, ["HEAD -> main", "origin/main", "tag: v1"]);
        assert_eq!(c.day(), "2026-10-07");
        assert_eq!(v[1], LogLine::Graph("|\\  ".into()));
        let cs = commits(out.as_bytes()).unwrap();
        assert_eq!(cs.len(), 2);
        assert!(cs[1].parents.is_empty() && cs[1].refs.is_empty());
    }

    #[test]
    fn a_short_record_is_refused() {
        assert!(parse(b"\x1ea\0b").is_err());
    }

    #[test]
    fn revisions() {
        let v = parse_revisions(
            b"h1\0h\0A\x002026-01-01T00:00:00Z\0One\nh2\0i\0B\x002025-01-01T00:00:00Z\0Two\n",
        );
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].subject, "Two");
    }
}
