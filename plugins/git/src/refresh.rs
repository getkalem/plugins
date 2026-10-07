//! A refresh of the status (DESIGN.md, 3.5) as data in and commands out:
//! [`Refresh::start`] gives the first commands, each answer goes to
//! [`Refresh::feed`], which may ask for more, and [`Refresh::finish`]
//! makes the [`Repo`]. The component runs the commands through Kalem's
//! `process` interface as their answers arrive; [`load`] runs them one
//! after another through a [`Runner`], for the example and the tests.

use std::collections::BTreeSet;

use crate::git::cmd::{self, GitCommand, LogSpec, Untracked};
use crate::git::{diff, log, refs, status};
use crate::model::{Repo, Section};

/// How a git command ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    /// Its exit status; none when a signal stopped it.
    pub status: Option<i32>,
    /// What it printed.
    pub stdout: Vec<u8>,
    /// What it printed as errors.
    pub stderr: Vec<u8>,
}

impl Output {
    /// It ended with the status 0.
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }

    /// The message for the user: the first line of its errors that says
    /// something, or its status.
    pub fn message(&self) -> String {
        let err = String::from_utf8_lossy(&self.stderr);
        err.lines()
            .map(|l| l.trim())
            .find(|l| !l.is_empty() && !l.starts_with("hint:"))
            .map(|l| {
                l.strip_prefix("fatal: ")
                    .or_else(|| l.strip_prefix("error: "))
                    .unwrap_or(l)
                    .to_string()
            })
            .unwrap_or_else(|| match self.status {
                Some(s) => format!("git ended with the status {s}"),
                None => "git was stopped".to_string(),
            })
    }
}

/// Runs git commands in a repository and waits for them.
pub trait Runner {
    /// Runs `command`; an error when it could not start.
    fn run(&mut self, command: &GitCommand) -> Result<Output, String>;
}

/// A part of a refresh: one command each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// `git status`.
    Status,
    /// The operation under way.
    Operations,
    /// The nearest tag.
    Describe,
    /// The stashes.
    Stashes,
    /// The last commits, HEAD's first.
    Recent,
    /// Commits not pushed.
    Unpushed,
    /// Commits not pulled.
    Unpulled,
    /// Counts of the unstaged changes.
    CountsUnstaged,
    /// Counts of the staged changes.
    CountsStaged,
}

/// What a refresh asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Which untracked files are listed.
    pub untracked: Untracked,
    /// The last commits shown when nothing is unpushed or unpulled.
    pub recent: u32,
    /// Unpushed and unpulled commits shown at most.
    pub around: u32,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            untracked: Untracked::Normal,
            recent: 10,
            around: 20,
        }
    }
}

/// A refresh under way.
#[derive(Debug, Clone)]
pub struct Refresh {
    options: Options,
    repo: Repo,
    pending: BTreeSet<Part>,
    status_read: bool,
    error: Option<String>,
}

impl Refresh {
    /// Starts a refresh of the repository at `root`: the commands that
    /// need nothing else first.
    pub fn start(
        root: &str,
        generation: u64,
        options: Options,
    ) -> (Refresh, Vec<(Part, GitCommand)>) {
        let commands = vec![
            (Part::Status, cmd::status(options.untracked)),
            (Part::Operations, cmd::operations()),
            (Part::Describe, cmd::describe()),
            (Part::Stashes, cmd::stash_list()),
            (Part::CountsUnstaged, cmd::numstat(false)),
            (Part::CountsStaged, cmd::numstat(true)),
        ];
        let r = Refresh {
            options,
            repo: Repo {
                root: root.to_string(),
                generation,
                ..Repo::default()
            },
            pending: commands.iter().map(|(p, _)| *p).collect(),
            status_read: false,
            error: None,
        };
        (r, commands)
    }

    /// Takes the answer to a part; the commands it calls for next.
    pub fn feed(&mut self, part: Part, result: Result<Output, String>) -> Vec<(Part, GitCommand)> {
        self.pending.remove(&part);
        let out = match result {
            Ok(o) => o,
            Err(e) => {
                if part == Part::Status {
                    self.error = Some(e);
                }
                return Vec::new();
            }
        };
        // Answers that fail say there is nothing: no tag, no stash, no
        // commit yet. Only the status's failure fails the refresh.
        let ok = out.success();
        match part {
            Part::Status => {
                if !ok {
                    self.error = Some(out.message());
                    return Vec::new();
                }
                match status::parse(&out.stdout) {
                    Ok(s) => self.repo.status = s,
                    Err(e) => {
                        self.error = Some(e);
                        return Vec::new();
                    }
                }
                self.status_read = true;
                return self.after_status();
            }
            Part::Operations if ok => self.repo.operation = refs::parse_operations(&out.stdout),
            Part::Describe if ok => self.repo.tag = refs::parse_describe(&out.stdout),
            Part::Stashes if ok => self.repo.stashes = refs::parse_stashes(&out.stdout),
            Part::Recent if ok => {
                self.repo.recent = log::commits(&out.stdout).unwrap_or_default();
                self.repo.head = self.repo.recent.first().cloned();
            }
            Part::Unpushed if ok => {
                self.repo.unpushed = log::commits(&out.stdout).unwrap_or_default()
            }
            Part::Unpulled if ok => {
                self.repo.unpulled = log::commits(&out.stdout).unwrap_or_default()
            }
            Part::CountsUnstaged | Part::CountsStaged if ok => {
                let section = if part == Part::CountsStaged {
                    Section::Staged
                } else {
                    Section::Unstaged
                };
                for n in diff::parse_numstat(&out.stdout).unwrap_or_default() {
                    if let Some(c) = n.counts {
                        self.repo.counts.insert((section, n.path), c);
                    }
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn after_status(&mut self) -> Vec<(Part, GitCommand)> {
        let s = &self.repo.status;
        let mut v = Vec::new();
        if !s.unborn() {
            v.push((
                Part::Recent,
                cmd::log(&LogSpec {
                    count: self.options.recent,
                    ..LogSpec::default()
                }),
            ));
            if s.upstream.is_some() {
                if s.ahead > 0 {
                    v.push((
                        Part::Unpushed,
                        cmd::log(&LogSpec {
                            range: Some("@{upstream}..HEAD".into()),
                            count: self.options.around,
                            ..LogSpec::default()
                        }),
                    ));
                }
                if s.behind > 0 {
                    v.push((
                        Part::Unpulled,
                        cmd::log(&LogSpec {
                            range: Some("HEAD..@{upstream}".into()),
                            count: self.options.around,
                            ..LogSpec::default()
                        }),
                    ));
                }
            }
        }
        self.pending.extend(v.iter().map(|(p, _)| *p));
        v
    }

    /// Every answer has come.
    pub fn done(&self) -> bool {
        self.pending.is_empty()
    }

    /// The repository; an error when its status could not be read.
    pub fn finish(self) -> Result<Repo, String> {
        if let Some(e) = self.error {
            return Err(e);
        }
        if !self.status_read {
            return Err("the status was not read".into());
        }
        Ok(self.repo)
    }
}

/// A refresh run to its end, one command after another.
pub fn load(
    runner: &mut impl Runner,
    root: &str,
    generation: u64,
    options: Options,
) -> Result<Repo, String> {
    let (mut refresh, mut queue) = Refresh::start(root, generation, options);
    while let Some((part, command)) = queue.pop() {
        let result = runner.run(&command);
        queue.extend(refresh.feed(part, result));
    }
    refresh.finish()
}

/// The diffs the folds call for and the repository has not loaded: the
/// unfolded files of the sections of files.
pub fn missing_diffs(
    repo: &Repo,
    folds: &crate::model::Folds,
    context: u32,
) -> Vec<((Section, String), GitCommand)> {
    let mut v = Vec::new();
    for section in [
        Section::Unmerged,
        Section::Untracked,
        Section::Unstaged,
        Section::Staged,
    ] {
        for e in repo.entries(section) {
            let key = crate::target::file_key(section, &e.path);
            if folds.folded(&key) || repo.diff(section, &e.path).is_some() {
                continue;
            }
            if let Some(c) = repo.diff_command(section, &e.path, context) {
                v.push(((section, e.path.clone()), c));
            }
        }
    }
    v
}

/// [`missing_diffs`] loaded through `runner`.
pub fn load_diffs(
    runner: &mut impl Runner,
    repo: &mut Repo,
    folds: &crate::model::Folds,
    context: u32,
) -> Result<(), String> {
    for ((section, path), command) in missing_diffs(repo, folds, context) {
        let out = runner.run(&command)?;
        // `diff --no-index` ends with 1 when the files differ.
        let ok = out.success() || (section == Section::Untracked && out.status == Some(1));
        if !ok {
            return Err(out.message());
        }
        repo.set_diff(section, &path, &out.stdout)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(stdout: &[u8]) -> Result<Output, String> {
        Ok(Output {
            status: Some(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        })
    }

    fn failed(stderr: &str) -> Result<Output, String> {
        Ok(Output {
            status: Some(128),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        })
    }

    #[test]
    fn the_status_calls_for_the_logs_it_needs() {
        let (mut r, first) = Refresh::start("/r", 1, Options::default());
        assert_eq!(first.len(), 6);
        let status = b"# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -0\0";
        let next = r.feed(Part::Status, ok(status));
        let parts: Vec<Part> = next.iter().map(|(p, _)| *p).collect();
        assert_eq!(parts, [Part::Recent, Part::Unpushed]);
        assert!(!r.done());
        for (p, _) in &first[1..] {
            r.feed(
                *p,
                failed("fatal: No names found, cannot describe anything."),
            );
        }
        r.feed(
            Part::Recent,
            ok(b"\x1eabc\0abc\0A\0a@x\x002026-01-01T00:00:00Z\0\0HEAD -> main\0Subject\n"),
        );
        r.feed(Part::Unpushed, ok(b""));
        assert!(r.done());
        let repo = r.finish().unwrap();
        assert_eq!(repo.head.unwrap().subject, "Subject");
        assert_eq!(repo.tag, None);
    }

    #[test]
    fn a_failed_status_fails_the_refresh() {
        let (mut r, _) = Refresh::start("/r", 1, Options::default());
        assert!(
            r.feed(
                Part::Status,
                failed("fatal: not a git repository (or any of the parent directories): .git\n")
            )
            .is_empty()
        );
        assert_eq!(
            r.finish().unwrap_err(),
            "not a git repository (or any of the parent directories): .git"
        );
    }

    #[test]
    fn messages() {
        let o = Output {
            status: Some(1),
            stdout: Vec::new(),
            stderr: b"hint: try this\nerror: Your local changes would be overwritten\n".to_vec(),
        };
        assert_eq!(o.message(), "Your local changes would be overwritten");
        assert_eq!(Output::default().message(), "git was stopped");
    }
}
