//! The command lines the plugin runs (DESIGN.md, section 4 and appendix
//! B). A command is data: its arguments after `git`, its standard input,
//! and whether its output is parsed. The host runs it (Kalem's `process`
//! interface in the component, `std::process` in the example and the
//! tests); nothing here starts a program.
//!
//! Every path is given after `--`, so that git never reads a path as an
//! option, and no command opens an editor or asks on a terminal.

use std::fmt;

/// Options before every command: no pager, no colors, paths unquoted.
pub const GLOBAL: &[&str] = &[
    "--no-pager",
    "-c",
    "color.ui=never",
    "-c",
    "core.quotepath=off",
];

/// A git command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    /// Its arguments after `git` and [`GLOBAL`].
    pub args: Vec<String>,
    /// Written to its standard input, which is then closed.
    pub stdin: Option<Vec<u8>>,
    /// Its output is parsed: run under `LC_ALL=C`, so git's words are
    /// English whatever the user's language.
    pub parsed: bool,
    /// It only reads: run with `GIT_OPTIONAL_LOCKS=0`, so that a status
    /// never takes the index's lock from a command the user runs.
    pub reads: bool,
}

impl GitCommand {
    /// A command changing the repository, its messages in the user's
    /// language.
    pub fn new<I, S>(args: I) -> GitCommand
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        GitCommand {
            args: args.into_iter().map(Into::into).collect(),
            stdin: None,
            parsed: false,
            reads: false,
        }
    }

    /// A command that reads, its output parsed.
    pub fn read<I, S>(args: I) -> GitCommand
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        GitCommand {
            parsed: true,
            reads: true,
            ..GitCommand::new(args)
        }
    }

    /// The same, with `bytes` on its standard input.
    pub fn with_stdin(mut self, bytes: Vec<u8>) -> GitCommand {
        self.stdin = Some(bytes);
        self
    }

    /// The whole argument list, [`GLOBAL`] first.
    pub fn argv(&self) -> Vec<String> {
        GLOBAL
            .iter()
            .map(|s| s.to_string())
            .chain(self.args.iter().cloned())
            .collect()
    }

    /// The variables added to the user's environment.
    pub fn env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            // A push needing a password fails at once rather than
            // waiting for a terminal that is not there (3.7).
            ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
            // No command may open an editor.
            ("GIT_EDITOR".to_string(), "false".to_string()),
        ];
        if self.parsed {
            env.push(("LC_ALL".to_string(), "C".to_string()));
        }
        if self.reads {
            env.push(("GIT_OPTIONAL_LOCKS".to_string(), "0".to_string()));
        }
        env
    }
}

impl fmt::Display for GitCommand {
    /// The command as a shell would take it, for the process log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("git")?;
        for a in &self.args {
            f.write_str(" ")?;
            f.write_str(&shell_quote(a))?;
        }
        Ok(())
    }
}

/// `arg` quoted for a POSIX shell when it needs it.
pub fn shell_quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:@%+,^{}".contains(&b) || b >= 0x80);
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

fn paths<'a>(args: &mut Vec<String>, paths: impl IntoIterator<Item = &'a str>) {
    args.push("--".into());
    args.extend(paths.into_iter().map(str::to_string));
}

/// Which untracked files `git status` lists (the setting `untracked`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Untracked {
    /// Folders collapsed into one line.
    #[default]
    Normal,
    /// Every file.
    All,
    /// None.
    No,
}

impl Untracked {
    /// The setting's value.
    pub fn parse(s: &str) -> Option<Untracked> {
        match s {
            "normal" => Some(Untracked::Normal),
            "all" => Some(Untracked::All),
            "no" => Some(Untracked::No),
            _ => None,
        }
    }

    fn flag(self) -> &'static str {
        match self {
            Untracked::Normal => "--untracked-files=normal",
            Untracked::All => "--untracked-files=all",
            Untracked::No => "--untracked-files=no",
        }
    }
}

/// `git --version`.
pub fn version() -> GitCommand {
    GitCommand::read(["--version"])
}

/// The repository's root, from any folder inside it.
pub fn toplevel() -> GitCommand {
    GitCommand::read(["rev-parse", "--show-toplevel"])
}

/// The status: entries, the branch, its upstream, ahead and behind.
pub fn status(untracked: Untracked) -> GitCommand {
    GitCommand::read([
        "status",
        "--porcelain=v2",
        "--branch",
        "--show-stash",
        "-z",
        untracked.flag(),
    ])
}

/// The references whose presence says an operation is under way; their
/// names on the standard input, as [`crate::git::refs::parse_operations`]
/// reads the answer.
pub fn operations() -> GitCommand {
    let names = crate::git::refs::OPERATION_REFS
        .iter()
        .map(|(name, _)| format!("{name}\n"))
        .collect::<String>();
    GitCommand::read(["cat-file", "--batch-check"]).with_stdin(names.into_bytes())
}

/// The nearest tag with the commits since it: `TAG-N-gHASH`.
pub fn describe() -> GitCommand {
    GitCommand::read(["describe", "--tags", "--long"])
}

/// The stashes.
pub fn stash_list() -> GitCommand {
    GitCommand::read(["stash", "list", "--format=%gd%x00%s"])
}

/// What a log shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogSpec {
    /// A revision or range (`@{upstream}..HEAD`); HEAD when none.
    pub range: Option<String>,
    /// Commits at most.
    pub count: u32,
    /// Commits skipped first, for "Load more".
    pub skip: u32,
    /// The ASCII graph.
    pub graph: bool,
    /// One file's log, following its renames.
    pub path: Option<String>,
}

/// A log in [`crate::git::log::FORMAT`].
pub fn log(spec: &LogSpec) -> GitCommand {
    let mut args = vec![
        "log".to_string(),
        crate::git::log::FORMAT.to_string(),
        format!("-n{}", spec.count),
    ];
    if spec.skip > 0 {
        args.push(format!("--skip={}", spec.skip));
    }
    if spec.graph {
        args.push("--graph".into());
    }
    if let Some(r) = &spec.range {
        args.push(r.clone());
    }
    if let Some(p) = &spec.path {
        args.push("--follow".into());
        paths(&mut args, [p.as_str()]);
    }
    GitCommand::read(args)
}

/// The added and removed line counts of every changed file, staged or
/// not.
pub fn numstat(staged: bool) -> GitCommand {
    let mut args = vec!["diff", "--numstat", "-z", "-M"];
    if staged {
        args.push("--cached");
    }
    GitCommand::read(args)
}

/// The diff of one file: the staged changes, or the unstaged ones. A
/// renamed file names its old path too, so the diff shows the rename.
pub fn diff(path: &str, from: Option<&str>, staged: bool, context: u32) -> GitCommand {
    let mut args = vec!["diff".to_string(), format!("-U{context}"), "-M".to_string()];
    if staged {
        args.push("--cached".into());
    }
    let mut p = vec![path];
    p.extend(from);
    paths(&mut args, p);
    GitCommand::read(args)
}

/// An untracked file as a diff from nothing. Git answers it with the exit
/// status 1, as `diff --no-index` does whenever files differ.
pub fn diff_untracked(path: &str, context: u32) -> GitCommand {
    let mut args = vec![
        "diff".to_string(),
        "--no-index".to_string(),
        format!("-U{context}"),
    ];
    paths(&mut args, ["/dev/null", path]);
    GitCommand::read(args)
}

/// A commit: its header, its stat and its diff.
pub fn show(rev: &str, context: u32) -> GitCommand {
    GitCommand::read([
        "show".to_string(),
        "--format=fuller".to_string(),
        "--stat".to_string(),
        "--patch".to_string(),
        format!("-U{context}"),
        rev.to_string(),
        "--".to_string(),
    ])
}

/// The blame of a file, as it is or at `rev`.
pub fn blame(rev: Option<&str>, path: &str) -> GitCommand {
    let mut args = vec!["blame".to_string(), "--porcelain".to_string()];
    args.extend(rev.map(str::to_string));
    paths(&mut args, [path]);
    GitCommand::read(args)
}

/// The blame of one line, from the document's own text when it has unsaved
/// changes (`--contents -`), so that the line is the one under the cursor.
pub fn blame_line(path: &str, line: u32, contents: Option<Vec<u8>>) -> GitCommand {
    let mut args = vec![
        "blame".to_string(),
        "--porcelain".to_string(),
        format!("-L{line},{line}"),
    ];
    if contents.is_some() {
        args.push("--contents".into());
        args.push("-".into());
    }
    paths(&mut args, [path]);
    let c = GitCommand::read(args);
    match contents {
        Some(bytes) => c.with_stdin(bytes),
        None => c,
    }
}

/// The remotes' names.
pub fn remotes() -> GitCommand {
    GitCommand::read(["remote"])
}

/// The repository's root and the folder's place in it, from a folder
/// inside it: two lines, the second empty at the root.
pub fn locate() -> GitCommand {
    GitCommand::read(["rev-parse", "--show-toplevel", "--show-prefix"])
}

/// The commits that changed a file, newest first, following renames: the
/// steps of the time machine.
pub fn file_revisions(path: &str) -> GitCommand {
    let mut args = vec![
        "log".to_string(),
        "--format=%H%x00%h%x00%an%x00%aI%x00%s".to_string(),
        "--follow".to_string(),
    ];
    paths(&mut args, [path]);
    GitCommand::read(args)
}

/// A file as it was at `rev` (`HEAD`; empty for the index's version).
pub fn show_file(rev: &str, path: &str) -> GitCommand {
    GitCommand::read(["show".to_string(), format!("{rev}:{path}")])
}

/// Stages files, new ones included.
pub fn add<'a>(files: impl IntoIterator<Item = &'a str>) -> GitCommand {
    let mut args = vec!["add".to_string()];
    paths(&mut args, files);
    GitCommand::new(args)
}

/// Stages every change to the tracked files.
pub fn add_tracked() -> GitCommand {
    GitCommand::new(["add", "-u"])
}

/// Stages everything, untracked files included.
pub fn add_all() -> GitCommand {
    GitCommand::new(["add", "-A"])
}

/// Unstages files.
pub fn restore_staged<'a>(files: impl IntoIterator<Item = &'a str>) -> GitCommand {
    let mut args = vec!["restore".to_string(), "--staged".to_string()];
    paths(&mut args, files);
    GitCommand::new(args)
}

/// Unstages files in a repository without a commit, where `restore` has
/// no HEAD to restore from.
pub fn rm_cached<'a>(files: impl IntoIterator<Item = &'a str>) -> GitCommand {
    let mut args = vec![
        "rm".to_string(),
        "--cached".to_string(),
        "-r".to_string(),
        "-q".to_string(),
    ];
    paths(&mut args, files);
    GitCommand::new(args)
}

/// Unstages everything.
pub fn unstage_all(unborn: bool) -> GitCommand {
    if unborn {
        rm_cached(["."])
    } else {
        GitCommand::new(["reset", "-q"])
    }
}

/// Puts files back as the index has them.
pub fn restore<'a>(files: impl IntoIterator<Item = &'a str>) -> GitCommand {
    let mut args = vec!["restore".to_string()];
    paths(&mut args, files);
    GitCommand::new(args)
}

/// Puts files back as HEAD has them, in the index and the working tree; a
/// path HEAD does not have is removed from both.
pub fn restore_from_head<'a>(files: impl IntoIterator<Item = &'a str>) -> GitCommand {
    let mut args = vec![
        "restore".to_string(),
        "--source=HEAD".to_string(),
        "--staged".to_string(),
        "--worktree".to_string(),
    ];
    paths(&mut args, files);
    GitCommand::new(args)
}

/// Deletes untracked files.
pub fn clean<'a>(files: impl IntoIterator<Item = &'a str>) -> GitCommand {
    let mut args = vec!["clean".to_string(), "-f".to_string(), "-q".to_string()];
    paths(&mut args, files);
    GitCommand::new(args)
}

/// Deletes tracked files, from the index and the working tree.
pub fn rm<'a>(files: impl IntoIterator<Item = &'a str>) -> GitCommand {
    let mut args = vec!["rm".to_string(), "-f".to_string(), "-q".to_string()];
    paths(&mut args, files);
    GitCommand::new(args)
}

/// Where a patch goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyTo {
    /// The index alone (staging, unstaging).
    Index,
    /// The working tree alone (discarding an unstaged change).
    WorkTree,
}

/// Applies `patch`, made by [`crate::patch`], forward or in reverse.
/// `zero_context` for a diff made without context lines.
pub fn apply(patch: Vec<u8>, to: ApplyTo, reverse: bool, zero_context: bool) -> GitCommand {
    let mut args = vec!["apply".to_string()];
    if to == ApplyTo::Index {
        args.push("--cached".into());
    }
    if reverse {
        args.push("--reverse".into());
    }
    if zero_context {
        args.push("--unidiff-zero".into());
    }
    args.push("-".into());
    GitCommand::new(args).with_stdin(patch)
}

/// What a commit is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitKind {
    /// A new commit.
    New,
    /// The last commit replaced, with the given message.
    Amend,
    /// The last commit replaced, its message kept.
    Extend,
    /// A `fixup!` commit for the commit named.
    Fixup(String),
}

/// The scissors line of a commit message, after the comment character:
/// what follows it is not part of the message.
pub const SCISSORS: &str = " ------------------------ >8 ------------------------";

/// A commit with `message` on the standard input, cut at the scissors line
/// already ([`cut_at_scissors`]); git strips its comment lines and
/// trailing blank lines, as it does a message the user edited. (With
/// `-F`, git takes a message as not edited, so `--cleanup=scissors` would
/// keep the comment lines.)
pub fn commit(message: &str, kind: &CommitKind, signoff: bool) -> GitCommand {
    let mut args = vec!["commit".to_string()];
    match kind {
        CommitKind::New => {}
        CommitKind::Amend => args.push("--amend".into()),
        CommitKind::Extend => {
            args.push("--amend".into());
            args.push("--no-edit".into());
        }
        CommitKind::Fixup(hash) => args.push(format!("--fixup={hash}")),
    }
    if signoff {
        args.push("--signoff".into());
    }
    match kind {
        CommitKind::Extend | CommitKind::Fixup(_) => GitCommand::new(args),
        CommitKind::New | CommitKind::Amend => {
            args.push("--cleanup=strip".into());
            args.push("-F".into());
            args.push("-".into());
            GitCommand::new(args).with_stdin(message.as_bytes().to_vec())
        }
    }
}

/// The last commit's message, for amending and rewording.
pub fn last_message() -> GitCommand {
    GitCommand::read(["log", "-1", "--format=%B"])
}

/// The commit message template the user configured, if any.
pub fn commit_template() -> GitCommand {
    GitCommand::read(["config", "--path", "--get", "commit.template"])
}

/// The character starting a comment line of a commit message.
pub fn comment_char() -> GitCommand {
    GitCommand::read(["config", "--get", "core.commentChar"])
}

/// Fetches the upstream's remote.
pub fn fetch(prune: bool) -> GitCommand {
    let mut args = vec!["fetch"];
    if prune {
        args.push("--prune");
    }
    GitCommand::new(args)
}

/// How Pull pulls (the setting `pull`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PullMode {
    /// As git's `pull.rebase` says.
    #[default]
    Config,
    /// By rebasing.
    Rebase,
    /// By merging.
    Merge,
    /// Only by fast-forwarding.
    FfOnly,
}

impl PullMode {
    /// The setting's value.
    pub fn parse(s: &str) -> Option<PullMode> {
        match s {
            "config" => Some(PullMode::Config),
            "rebase" => Some(PullMode::Rebase),
            "merge" => Some(PullMode::Merge),
            "ff-only" => Some(PullMode::FfOnly),
            _ => None,
        }
    }
}

/// Pulls.
pub fn pull(mode: PullMode) -> GitCommand {
    let mut args = vec!["pull"];
    match mode {
        PullMode::Config => {}
        PullMode::Rebase => args.push("--rebase"),
        PullMode::Merge => args.push("--no-rebase"),
        PullMode::FfOnly => args.push("--ff-only"),
    }
    args.push("--no-edit");
    GitCommand::new(args)
}

/// How a push goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Push {
    /// To the upstream.
    Plain,
    /// To `remote`'s branch of the same name, made the upstream.
    SetUpstream {
        /// The remote, such as `origin`.
        remote: String,
        /// The branch.
        branch: String,
    },
    /// Over the remote's branch, only if it is where it was last fetched.
    ForceWithLease,
}

/// Pushes.
pub fn push(how: &Push) -> GitCommand {
    match how {
        Push::Plain => GitCommand::new(["push"]),
        Push::SetUpstream { remote, branch } => GitCommand::new([
            "push".to_string(),
            "--set-upstream".to_string(),
            remote.clone(),
            branch.clone(),
        ]),
        Push::ForceWithLease => GitCommand::new(["push", "--force-with-lease"]),
    }
}

/// The local and remote branches, in [`crate::git::refs::BRANCH_FORMAT`].
pub fn branches() -> GitCommand {
    GitCommand::read([
        "for-each-ref",
        crate::git::refs::BRANCH_FORMAT,
        "refs/heads",
        "refs/remotes",
    ])
}

/// Switches to a local branch.
pub fn switch(branch: &str) -> GitCommand {
    GitCommand::new(["switch", branch])
}

/// Makes a branch and switches to it: from HEAD, or tracking a remote
/// branch.
pub fn switch_create(name: &str, track: Option<&str>) -> GitCommand {
    let mut args = vec!["switch".to_string(), "-c".to_string(), name.to_string()];
    if let Some(t) = track {
        args.push("--track".into());
        args.push(t.to_string());
    }
    GitCommand::new(args)
}

/// The URL of a remote.
pub fn remote_url(remote: &str) -> GitCommand {
    GitCommand::read(["remote", "get-url", remote])
}

/// The current branch's upstream, as `origin/main`.
pub fn upstream() -> GitCommand {
    GitCommand::read(["rev-parse", "--abbrev-ref", "@{upstream}"])
}

/// Makes a repository in the folder it runs in.
pub fn init() -> GitCommand {
    GitCommand::new(["init"])
}

/// `message` up to its scissors line (`# ------------------------ >8`),
/// where `git commit -v` puts the diff for reading.
pub fn cut_at_scissors(message: &str, comment: char) -> &str {
    let line = format!("{comment}{SCISSORS}");
    let mut at = 0;
    for l in message.split_inclusive('\n') {
        if l.trim_end_matches(['\n', '\r']) == line {
            return &message[..at];
        }
        at += l.len();
    }
    message
}

/// Whether `name` may be a branch's name.
pub fn check_branch_name(name: &str) -> GitCommand {
    GitCommand::read(["check-ref-format", "--branch", name])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_come_after_the_separator() {
        let c = add(["-rf", "a b"]);
        assert_eq!(c.args, ["add", "--", "-rf", "a b"]);
        assert_eq!(c.to_string(), "git add -- -rf 'a b'");
    }

    #[test]
    fn reading_commands_are_parsed_without_locks() {
        let env = status(Untracked::Normal).env();
        assert!(env.contains(&("LC_ALL".into(), "C".into())));
        assert!(env.contains(&("GIT_OPTIONAL_LOCKS".into(), "0".into())));
        let env = add(["x"]).env();
        assert!(!env.iter().any(|(k, _)| k == "LC_ALL"));
        assert!(env.contains(&("GIT_TERMINAL_PROMPT".into(), "0".into())));
    }

    #[test]
    fn a_commit_message_goes_on_standard_input() {
        let c = commit("Fix it\n", &CommitKind::New, false);
        assert_eq!(c.stdin.as_deref(), Some(&b"Fix it\n"[..]));
        assert!(c.args.contains(&"--cleanup=strip".to_string()));
        let c = commit("", &CommitKind::Extend, true);
        assert_eq!(c.args, ["commit", "--amend", "--no-edit", "--signoff"]);
        assert_eq!(c.stdin, None);
    }

    #[test]
    fn the_scissors_cut_the_message() {
        let m = "Subject\n\n# comment\n# ------------------------ >8 ------------------------\ndiff --git a/x b/x\n";
        assert_eq!(cut_at_scissors(m, '#'), "Subject\n\n# comment\n");
        assert_eq!(cut_at_scissors("No scissors\n", '#'), "No scissors\n");
        assert_eq!(
            cut_at_scissors(
                "A\n; ------------------------ >8 ------------------------\nB",
                ';'
            ),
            "A\n"
        );
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("@{upstream}..HEAD"), "@{upstream}..HEAD");
    }
}
