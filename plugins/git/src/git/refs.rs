//! References: branches, stashes, the nearest tag, and the operation under
//! way (a merge, a rebase stopped on a conflict, a cherry-pick, a revert,
//! a bisection).

/// The format of [`crate::git::cmd::branches`].
pub const BRANCH_FORMAT: &str =
    "--format=%(refname)%00%(refname:short)%00%(upstream:short)%00%(upstream:track)%00%(HEAD)";

/// A branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Its short name: `main`, `origin/main`.
    pub name: String,
    /// A remote's branch.
    pub remote: bool,
    /// HEAD is on it.
    pub current: bool,
    /// Its upstream, as `origin/main`.
    pub upstream: Option<String>,
    /// Commits ahead of the upstream.
    pub ahead: u32,
    /// Commits behind it.
    pub behind: u32,
    /// The upstream is gone from the remote.
    pub gone: bool,
}

/// Reads [`crate::git::cmd::branches`]; a remote's `HEAD` is left out.
pub fn parse_branches(out: &[u8]) -> Vec<Branch> {
    let text = String::from_utf8_lossy(out);
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\0').collect();
            let [full, name, upstream, track, head] = f[..] else {
                return None;
            };
            if full.starts_with("refs/remotes/") && full.ends_with("/HEAD") {
                return None;
            }
            let number = |word: &str| -> u32 {
                track
                    .split(['[', ']', ','])
                    .map(str::trim)
                    .find_map(|p| p.strip_prefix(word))
                    .and_then(|n| n.trim().parse().ok())
                    .unwrap_or(0)
            };
            Some(Branch {
                name: name.to_string(),
                remote: full.starts_with("refs/remotes/"),
                current: head == "*",
                upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
                ahead: number("ahead"),
                behind: number("behind"),
                gone: track.contains("gone"),
            })
        })
        .collect()
}

/// A stash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// Its number: `stash@{N}`.
    pub index: u32,
    /// Its message, as `On main: half done`.
    pub message: String,
}

impl Stash {
    /// Its name for git.
    pub fn name(&self) -> String {
        format!("stash@{{{}}}", self.index)
    }
}

/// Reads [`crate::git::cmd::stash_list`].
pub fn parse_stashes(out: &[u8]) -> Vec<Stash> {
    String::from_utf8_lossy(out)
        .lines()
        .filter_map(|l| {
            let (name, message) = l.split_once('\0')?;
            let index = name
                .strip_prefix("stash@{")?
                .strip_suffix('}')?
                .parse()
                .ok()?;
            Some(Stash {
                index,
                message: message.to_string(),
            })
        })
        .collect()
}

/// The nearest tag and the commits since it, from `TAG-N-gHASH`.
pub fn parse_describe(out: &[u8]) -> Option<(String, u32)> {
    let text = String::from_utf8_lossy(out);
    let line = text.trim();
    let mut parts = line.rsplitn(3, '-');
    let _hash = parts.next()?.strip_prefix('g')?;
    let distance = parts.next()?.parse().ok()?;
    let tag = parts.next()?;
    (!tag.is_empty()).then(|| (tag.to_string(), distance))
}

/// An operation under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// A merge with conflicts, or waiting for its commit.
    Merge,
    /// A rebase stopped on a conflict.
    Rebase,
    /// A cherry-pick stopped.
    CherryPick,
    /// A revert stopped.
    Revert,
    /// A bisection.
    Bisect,
}

impl Operation {
    /// The word the status's line starts with.
    pub fn word(self) -> &'static str {
        match self {
            Operation::Merge => "Merging",
            Operation::Rebase => "Rebasing",
            Operation::CherryPick => "Cherry-picking",
            Operation::Revert => "Reverting",
            Operation::Bisect => "Bisecting",
        }
    }
}

/// The references asked of `git cat-file --batch-check`, in order, with
/// the operation each says is under way.
pub const OPERATION_REFS: &[(&str, Operation)] = &[
    ("MERGE_HEAD", Operation::Merge),
    ("REBASE_HEAD", Operation::Rebase),
    ("CHERRY_PICK_HEAD", Operation::CherryPick),
    ("REVERT_HEAD", Operation::Revert),
    ("refs/bisect/bad", Operation::Bisect),
];

/// Reads the answer of [`crate::git::cmd::operations`]: a line per name,
/// `NAME missing` for the absent ones. The first present one wins.
pub fn parse_operations(out: &[u8]) -> Option<Operation> {
    let text = String::from_utf8_lossy(out);
    text.lines()
        .zip(OPERATION_REFS)
        .find(|(line, _)| !line.ends_with(" missing") && !line.trim().is_empty())
        .map(|(_, (_, op))| *op)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches() {
        let out = b"refs/heads/main\0main\0origin/main\0[ahead 1, behind 2]\0*\n\
refs/heads/old\0old\0origin/old\0[gone]\0 \n\
refs/remotes/origin/HEAD\0origin\0\0\0 \n\
refs/remotes/origin/main\0origin/main\0\0\0 \n";
        let b = parse_branches(out);
        assert_eq!(b.len(), 3);
        assert!(b[0].current && (b[0].ahead, b[0].behind) == (1, 2));
        assert!(b[1].gone);
        assert!(b[2].remote && b[2].upstream.is_none());
    }

    #[test]
    fn stashes_and_tags() {
        let s = parse_stashes(b"stash@{0}\0On main: half\nstash@{12}\0WIP on x: y\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].index, 12);
        assert_eq!(s[1].name(), "stash@{12}");
        assert_eq!(
            parse_describe(b"v0.1.0-3-gabc1234\n"),
            Some(("v0.1.0".into(), 3))
        );
        assert_eq!(
            parse_describe(b"release-2-0-0-gabc\n"),
            Some(("release-2-0".into(), 0))
        );
        assert_eq!(parse_describe(b"fatal: No names found\n"), None);
    }

    #[test]
    fn operations() {
        let none = b"MERGE_HEAD missing\nREBASE_HEAD missing\nCHERRY_PICK_HEAD missing\nREVERT_HEAD missing\nrefs/bisect/bad missing\n";
        assert_eq!(parse_operations(none), None);
        let merge = b"abc commit 200\nREBASE_HEAD missing\nCHERRY_PICK_HEAD missing\nREVERT_HEAD missing\nrefs/bisect/bad missing\n";
        assert_eq!(parse_operations(merge), Some(Operation::Merge));
        let pick = b"MERGE_HEAD missing\nREBASE_HEAD missing\nabc commit 1\nREVERT_HEAD missing\nrefs/bisect/bad missing\n";
        assert_eq!(parse_operations(pick), Some(Operation::CherryPick));
    }
}
