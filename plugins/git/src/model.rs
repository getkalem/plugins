//! What the status knows of a repository (DESIGN.md, 3.5), and what of it
//! is folded.

use std::collections::BTreeMap;

use crate::git::cmd::{self, GitCommand};
use crate::git::diff::{self, FileDiff};
use crate::git::log::Commit;
use crate::git::refs::{Branch, Operation, Stash};
use crate::git::status::{Entry, Kind, Status};

/// A section of the status, in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    /// Paths in conflict.
    Unmerged,
    /// Paths git does not track.
    Untracked,
    /// Changes not staged.
    Unstaged,
    /// Changes staged.
    Staged,
    /// The local branches.
    Branches,
    /// The stashes.
    Stashes,
    /// Commits not pushed to the upstream.
    Unpushed,
    /// Commits on the upstream not pulled.
    Unpulled,
    /// The last commits, when nothing is unpushed or unpulled.
    Recent,
    /// The files of a commit shown as a document (not in the status).
    Shown,
}

impl Section {
    /// Every section, in order.
    pub const ALL: [Section; 10] = [
        Section::Unmerged,
        Section::Untracked,
        Section::Unstaged,
        Section::Staged,
        Section::Branches,
        Section::Stashes,
        Section::Unpushed,
        Section::Unpulled,
        Section::Recent,
        Section::Shown,
    ];

    /// Its name in region keys.
    pub fn key(self) -> &'static str {
        match self {
            Section::Unmerged => "unmerged",
            Section::Untracked => "untracked",
            Section::Unstaged => "unstaged",
            Section::Staged => "staged",
            Section::Branches => "branches",
            Section::Stashes => "stashes",
            Section::Unpushed => "unpushed",
            Section::Unpulled => "unpulled",
            Section::Recent => "recent",
            Section::Shown => "shown",
        }
    }

    /// The section named `key`.
    pub fn from_key(key: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Its sections of files hold paths; the others commits or stashes.
    pub fn holds_files(self) -> bool {
        matches!(
            self,
            Section::Unmerged | Section::Untracked | Section::Unstaged | Section::Staged
        )
    }
}

/// A repository as the status shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Repo {
    /// Its root folder.
    pub root: String,
    /// What `git status` said.
    pub status: Status,
    /// HEAD's commit, when there is one.
    pub head: Option<Commit>,
    /// The nearest tag and the commits since it.
    pub tag: Option<(String, u32)>,
    /// An operation under way.
    pub operation: Option<Operation>,
    /// The stashes.
    pub stashes: Vec<Stash>,
    /// The branches, local and remote, the latest commit first.
    pub branches: Vec<Branch>,
    /// Commits not pushed to the upstream.
    pub unpushed: Vec<Commit>,
    /// Commits on the upstream not pulled.
    pub unpulled: Vec<Commit>,
    /// The last commits.
    pub recent: Vec<Commit>,
    /// Added and removed lines by section and path; a binary file has
    /// none.
    pub counts: BTreeMap<(Section, String), (u32, u32)>,
    /// The diffs loaded, by section and path: a file's diff is loaded when
    /// it is first unfolded, and dropped at the next refresh.
    pub diffs: BTreeMap<(Section, String), FileDiff>,
    /// The refresh that made it.
    pub generation: u64,
}

impl Repo {
    /// The entries of a section of files, in git's order.
    pub fn entries(&self, section: Section) -> Vec<&Entry> {
        self.status
            .entries
            .iter()
            .filter(|e| match section {
                Section::Unmerged => matches!(e.kind, Kind::Unmerged(_)),
                Section::Untracked => e.kind == Kind::Untracked,
                Section::Unstaged => e.unstaged(),
                Section::Staged => e.staged(),
                _ => false,
            })
            .collect()
    }

    /// The entry of `path` in `section`.
    pub fn entry(&self, section: Section, path: &str) -> Option<&Entry> {
        self.entries(section).into_iter().find(|e| e.path == path)
    }

    /// A file's diff, when loaded.
    pub fn diff(&self, section: Section, path: &str) -> Option<&FileDiff> {
        self.diffs.get(&(section, path.to_string()))
    }

    /// The command that loads a file's diff.
    pub fn diff_command(&self, section: Section, path: &str, context: u32) -> Option<GitCommand> {
        let e = self.entry(section, path)?;
        Some(match section {
            Section::Untracked => cmd::diff_untracked(path, context),
            Section::Staged => cmd::diff(path, e.from.as_deref(), true, context),
            Section::Unstaged | Section::Unmerged => cmd::diff(path, None, false, context),
            _ => return None,
        })
    }

    /// Keeps a file's diff, read from git's answer to [`Repo::diff_command`].
    pub fn set_diff(&mut self, section: Section, path: &str, out: &[u8]) -> Result<(), String> {
        let d = diff::parse(out)?;
        let file = d.files.into_iter().next().unwrap_or_else(|| FileDiff {
            new_path: Some(path.to_string()),
            ..FileDiff::default()
        });
        self.diffs.insert((section, path.to_string()), file);
        Ok(())
    }

    /// The local branches, the latest commit first.
    pub fn local_branches(&self) -> Vec<&Branch> {
        self.branches.iter().filter(|b| !b.remote).collect()
    }

    /// No commit yet.
    pub fn unborn(&self) -> bool {
        self.status.unborn()
    }

    /// Whether the status shows the recent commits: when nothing is
    /// unpushed or unpulled.
    pub fn shows_recent(&self) -> bool {
        self.unpushed.is_empty() && self.unpulled.is_empty() && !self.recent.is_empty()
    }
}

/// What the user folded and unfolded, by region key; a key not named is
/// as [`default_folded`] says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Folds {
    set: BTreeMap<String, bool>,
}

/// Whether a region starts folded: files of the status, commits of a log
/// and runs of the process log do; sections, hunks and the files of a
/// commit shown do not.
pub fn default_folded(key: &str) -> bool {
    (key.starts_with("file:") && !key.starts_with("file:shown:"))
        || key.starts_with("commit:")
        || key.starts_with("run:")
}

impl Folds {
    /// Whether the region `key` is folded.
    pub fn folded(&self, key: &str) -> bool {
        self.set
            .get(key)
            .copied()
            .unwrap_or_else(|| default_folded(key))
    }

    /// Folds or unfolds it.
    pub fn set(&mut self, key: &str, folded: bool) {
        if folded == default_folded(key) {
            self.set.remove(key);
        } else {
            self.set.insert(key.to_string(), folded);
        }
    }

    /// Folds it when unfolded and the other way; its new state.
    pub fn toggle(&mut self, key: &str) -> bool {
        let f = !self.folded(key);
        self.set(key, f);
        f
    }

    /// Forgets what the user did: every region as it starts.
    pub fn reset(&mut self) {
        self.set.clear();
    }

    /// The keys unfolded against their default.
    pub fn unfolded_files(&self) -> impl Iterator<Item = &str> {
        self.set
            .iter()
            .filter(|(k, f)| k.starts_with("file:") && !**f)
            .map(|(k, _)| k.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_remember_only_what_differs() {
        let mut f = Folds::default();
        assert!(f.folded("file:unstaged:a"));
        assert!(!f.folded("section:staged"));
        assert!(!f.toggle("file:unstaged:a"));
        assert_eq!(f.unfolded_files().collect::<Vec<_>>(), ["file:unstaged:a"]);
        assert!(f.toggle("file:unstaged:a"));
        assert_eq!(f, Folds::default());
    }

    #[test]
    fn sections_round_trip_their_keys() {
        for s in Section::ALL {
            assert_eq!(Section::from_key(s.key()), Some(s));
        }
    }
}
