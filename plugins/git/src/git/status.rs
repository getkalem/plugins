//! `git status --porcelain=v2 --branch --show-stash -z` read (git's
//! documentation, "Porcelain Format Version 2"): the branch, its
//! upstream, ahead and behind, and one entry per changed path.

/// The status of a repository.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// HEAD's commit; none before the first commit.
    pub oid: Option<String>,
    /// The branch; none when HEAD is detached.
    pub branch: Option<String>,
    /// The upstream, as `origin/main`.
    pub upstream: Option<String>,
    /// Commits on the branch and not on the upstream.
    pub ahead: u32,
    /// Commits on the upstream and not on the branch.
    pub behind: u32,
    /// The stashes, when git said (2.35 and later).
    pub stashes: Option<u32>,
    /// The changed paths, in git's order.
    pub entries: Vec<Entry>,
}

impl Status {
    /// No commit yet.
    pub fn unborn(&self) -> bool {
        self.oid.is_none()
    }
}

/// How one side of a path changed: the index against HEAD, or the
/// working tree against the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Not changed.
    Unmodified,
    /// Its content changed.
    Modified,
    /// Its kind changed (a file became a link).
    TypeChanged,
    /// New.
    Added,
    /// Gone.
    Deleted,
    /// Moved, perhaps changed too.
    Renamed,
    /// Copied.
    Copied,
    /// In conflict.
    Unmerged,
}

impl State {
    fn from_char(c: u8) -> Result<State, String> {
        Ok(match c {
            b'.' => State::Unmodified,
            b'M' => State::Modified,
            b'T' => State::TypeChanged,
            b'A' => State::Added,
            b'D' => State::Deleted,
            b'R' => State::Renamed,
            b'C' => State::Copied,
            b'U' => State::Unmerged,
            c => return Err(format!("unknown change `{}`", c as char)),
        })
    }

    /// The word the status shows.
    pub fn word(self) -> &'static str {
        match self {
            State::Unmodified => "unmodified",
            State::Modified => "modified",
            State::TypeChanged => "type changed",
            State::Added => "new file",
            State::Deleted => "deleted",
            State::Renamed => "renamed",
            State::Copied => "copied",
            State::Unmerged => "conflict",
        }
    }
}

/// How a path conflicts (the `XY` of an unmerged entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// `DD`.
    BothDeleted,
    /// `AU`.
    AddedByUs,
    /// `UD`.
    DeletedByThem,
    /// `UA`.
    AddedByThem,
    /// `DU`.
    DeletedByUs,
    /// `AA`.
    BothAdded,
    /// `UU`.
    BothModified,
}

impl Conflict {
    fn parse(xy: &[u8]) -> Result<Conflict, String> {
        Ok(match xy {
            b"DD" => Conflict::BothDeleted,
            b"AU" => Conflict::AddedByUs,
            b"UD" => Conflict::DeletedByThem,
            b"UA" => Conflict::AddedByThem,
            b"DU" => Conflict::DeletedByUs,
            b"AA" => Conflict::BothAdded,
            b"UU" => Conflict::BothModified,
            _ => {
                return Err(format!(
                    "unknown conflict `{}`",
                    String::from_utf8_lossy(xy)
                ));
            }
        })
    }

    /// The words the status shows, as `git status` says them.
    pub fn words(self) -> &'static str {
        match self {
            Conflict::BothDeleted => "both deleted",
            Conflict::AddedByUs => "added by us",
            Conflict::DeletedByThem => "deleted by them",
            Conflict::AddedByThem => "added by them",
            Conflict::DeletedByUs => "deleted by us",
            Conflict::BothAdded => "both added",
            Conflict::BothModified => "both modified",
        }
    }
}

/// What an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A tracked path, changed in the index, the working tree, or both.
    Tracked,
    /// A path in conflict.
    Unmerged(Conflict),
    /// A path git does not track.
    Untracked,
    /// A path git ignores (listed only when asked).
    Ignored,
}

/// One changed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its path from the repository's root.
    pub path: String,
    /// The path it was renamed or copied from.
    pub from: Option<String>,
    /// What it is.
    pub kind: Kind,
    /// The index against HEAD: what is staged.
    pub index: State,
    /// The working tree against the index: what is not staged.
    pub worktree: State,
    /// It is a submodule.
    pub submodule: bool,
}

impl Entry {
    /// It has staged changes.
    pub fn staged(&self) -> bool {
        self.kind == Kind::Tracked && self.index != State::Unmodified
    }

    /// It has unstaged changes.
    pub fn unstaged(&self) -> bool {
        self.kind == Kind::Tracked && self.worktree != State::Unmodified
    }
}

/// Reads git's answer.
pub fn parse(out: &[u8]) -> Result<Status, String> {
    let mut status = Status::default();
    let mut records = out.split(|&b| b == 0).filter(|r| !r.is_empty());
    while let Some(r) = records.next() {
        let text = String::from_utf8_lossy(r);
        match r[0] {
            b'#' => header(&mut status, &text)?,
            b'1' => {
                let f: Vec<&str> = text.splitn(9, ' ').collect();
                let [_, xy, sub, _, _, _, _, _, path] = f[..] else {
                    return Err(format!("a short entry: {text}"));
                };
                let (index, worktree) = states(xy)?;
                status.entries.push(Entry {
                    path: path.to_string(),
                    from: None,
                    kind: Kind::Tracked,
                    index,
                    worktree,
                    submodule: sub.starts_with('S'),
                });
            }
            b'2' => {
                let f: Vec<&str> = text.splitn(10, ' ').collect();
                let [_, xy, sub, _, _, _, _, _, _, path] = f[..] else {
                    return Err(format!("a short entry: {text}"));
                };
                let (index, worktree) = states(xy)?;
                let from = records
                    .next()
                    .ok_or_else(|| format!("a rename without its source: {text}"))?;
                status.entries.push(Entry {
                    path: path.to_string(),
                    from: Some(String::from_utf8_lossy(from).into_owned()),
                    kind: Kind::Tracked,
                    index,
                    worktree,
                    submodule: sub.starts_with('S'),
                });
            }
            b'u' => {
                let f: Vec<&str> = text.splitn(11, ' ').collect();
                let [_, xy, sub, _, _, _, _, _, _, _, path] = f[..] else {
                    return Err(format!("a short entry: {text}"));
                };
                status.entries.push(Entry {
                    path: path.to_string(),
                    from: None,
                    kind: Kind::Unmerged(Conflict::parse(xy.as_bytes())?),
                    index: State::Unmerged,
                    worktree: State::Unmerged,
                    submodule: sub.starts_with('S'),
                });
            }
            b'?' | b'!' => {
                let path = text.get(2..).unwrap_or_default().to_string();
                let untracked = r[0] == b'?';
                status.entries.push(Entry {
                    submodule: false,
                    from: None,
                    kind: if untracked {
                        Kind::Untracked
                    } else {
                        Kind::Ignored
                    },
                    index: State::Unmodified,
                    worktree: if untracked {
                        State::Added
                    } else {
                        State::Unmodified
                    },
                    path,
                });
            }
            _ => {
                return Err(format!(
                    "an entry git's documentation does not name: {text}"
                ));
            }
        }
    }
    Ok(status)
}

fn states(xy: &str) -> Result<(State, State), String> {
    let b = xy.as_bytes();
    if b.len() != 2 {
        return Err(format!("`{xy}` is not two letters"));
    }
    Ok((State::from_char(b[0])?, State::from_char(b[1])?))
}

fn header(status: &mut Status, line: &str) -> Result<(), String> {
    let mut f = line.splitn(3, ' ');
    let (_, key, value) = (f.next(), f.next().unwrap_or(""), f.next().unwrap_or(""));
    match key {
        "branch.oid" => status.oid = (value != "(initial)").then(|| value.to_string()),
        "branch.head" => status.branch = (value != "(detached)").then(|| value.to_string()),
        "branch.upstream" => status.upstream = Some(value.to_string()),
        "branch.ab" => {
            let mut parts = value.split(' ');
            let mut number = |sign: char| -> Result<u32, String> {
                parts
                    .next()
                    .and_then(|p| p.strip_prefix(sign))
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| format!("ahead and behind unreadable: {value}"))
            };
            status.ahead = number('+')?;
            status.behind = number('-')?;
        }
        "stash" => status.stashes = value.parse().ok(),
        // Headers a later git adds are skipped, as its documentation asks.
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(parts: &[&str]) -> Vec<u8> {
        let mut v = Vec::new();
        for p in parts {
            v.extend_from_slice(p.as_bytes());
            v.push(0);
        }
        v
    }

    #[test]
    fn every_kind_of_entry() {
        let out = z(&[
            "# branch.oid 5e1f1c0de5e1f1c0de5e1f1c0de5e1f1c0de5e1f",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +1 -2",
            "# stash 3",
            "1 .M N... 100644 100644 100644 aaa aaa src/lib.rs",
            "1 A. N... 000000 100644 100644 000 bbb docs/a file.md",
            "1 MM N... 100644 100644 100644 aaa bbb both.txt",
            "2 R. N... 100644 100644 100644 aaa aaa R100 new name.rs",
            "old name.rs",
            "u UU N... 100644 100644 100644 100644 a b c conflict.rs",
            "1 .M SC.. 160000 160000 160000 aaa aaa vendor/sub",
            "? notes/new.txt",
            "! target/",
        ]);
        let s = parse(&out).unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind, s.stashes), (1, 2, Some(3)));
        assert!(!s.unborn());
        let e = &s.entries;
        assert_eq!(e.len(), 8);
        assert!(e[0].unstaged() && !e[0].staged());
        assert_eq!(e[1].path, "docs/a file.md");
        assert_eq!(e[1].index, State::Added);
        assert!(e[2].staged() && e[2].unstaged());
        assert_eq!(e[3].path, "new name.rs");
        assert_eq!(e[3].from.as_deref(), Some("old name.rs"));
        assert_eq!(e[3].index, State::Renamed);
        assert_eq!(e[4].kind, Kind::Unmerged(Conflict::BothModified));
        assert!(e[5].submodule);
        assert_eq!(e[6].kind, Kind::Untracked);
        assert_eq!(e[7].kind, Kind::Ignored);
    }

    #[test]
    fn a_new_repository_and_a_detached_head() {
        let s = parse(&z(&["# branch.oid (initial)", "# branch.head main"])).unwrap();
        assert!(s.unborn());
        let s = parse(&z(&["# branch.oid abc", "# branch.head (detached)"])).unwrap();
        assert_eq!(s.branch, None);
        assert_eq!(s.upstream, None);
    }

    #[test]
    fn unknown_headers_are_skipped_and_bad_entries_refused() {
        assert!(parse(&z(&["# branch.future yes"])).is_ok());
        assert!(parse(&z(&["1 .M N..."])).is_err());
        assert!(parse(&z(&["2 R. N... 1 1 1 a a R100 lonely"])).is_err());
    }
}
