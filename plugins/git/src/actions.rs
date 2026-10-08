//! What the leader's keys do to what they act on (DESIGN.md, 2.3): `SPC g
//! S` and `SPC g s` stage, `SPC g U` and `SPC g u` unstage, `SPC g R` and
//! `SPC g r` discard, `SPC g D` deletes. A [`Plan`] is the git commands,
//! run in order until one fails, and the question to ask first when what
//! it does cannot be undone.

use std::collections::BTreeMap;

use crate::git::cmd::{self, ApplyTo, CommitKind, GitCommand};
use crate::git::status::{Kind, State};
use crate::model::{Repo, Section};
use crate::patch::{self, Mode, Pick};
use crate::target::Target;

/// An action of the leader's keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// `SPC g S`, `SPC g s`.
    Stage,
    /// `SPC g U`, `SPC g u`.
    Unstage,
    /// `SPC g R`, `SPC g r`.
    Discard,
    /// `SPC g D`.
    Delete,
}

impl Act {
    fn past(self) -> &'static str {
        match self {
            Act::Stage => "Staged",
            Act::Unstage => "Unstaged",
            Act::Discard => "Reverted",
            Act::Delete => "Deleted",
        }
    }
}

/// What the plugin's settings say of actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Discarding asks first (the setting `confirm_discard`); deleting and
    /// staging every untracked file always do.
    pub confirm_discard: bool,
    /// The diffs were made without context lines (`context_lines = 0`).
    pub zero_context: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            confirm_discard: true,
            zero_context: false,
        }
    }
}

/// The commands an action runs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// The question to ask first, naming what is lost; none to go ahead.
    pub confirm: Option<String>,
    /// The commands, in order; the first that fails stops the others.
    pub commands: Vec<GitCommand>,
    /// What the status bar says when they all succeed.
    pub done: String,
}

struct Builder<'a> {
    act: Act,
    repo: &'a Repo,
    settings: Settings,
    plan: Plan,
    asks: Vec<String>,
    did: Vec<String>,
}

impl Builder<'_> {
    fn ask(&mut self, always: bool, question: String) {
        if always || self.settings.confirm_discard {
            self.asks.push(question);
        }
    }

    fn apply(&mut self, patch: Vec<u8>, to: ApplyTo, reverse: bool) {
        self.plan
            .commands
            .push(cmd::apply(patch, to, reverse, self.settings.zero_context));
    }
}

/// The plan of `act` on `targets`: what is under the cursor, or what a
/// selection covers ([`crate::target::in_selection`]).
pub fn plan(act: Act, targets: &[Target], repo: &Repo, settings: Settings) -> Result<Plan, String> {
    if targets.is_empty() {
        return Err("Nothing here to act on".into());
    }
    let mut b = Builder {
        act,
        repo,
        settings,
        plan: Plan::default(),
        asks: Vec::new(),
        did: Vec::new(),
    };
    let mut files: BTreeMap<Section, Vec<String>> = BTreeMap::new();
    let mut picks: BTreeMap<(Section, String), Vec<(usize, Pick)>> = BTreeMap::new();
    for t in targets {
        match t {
            Target::Section(s) => section(&mut b, *s)?,
            Target::File { section, path } => files.entry(*section).or_default().push(path.clone()),
            Target::Hunk {
                section,
                path,
                hunk,
            } => picks
                .entry((*section, path.clone()))
                .or_default()
                .push((*hunk, Pick::All)),
            Target::Lines {
                section,
                path,
                hunk,
                lines,
            } => picks
                .entry((*section, path.clone()))
                .or_default()
                .push((*hunk, Pick::Lines(lines.clone()))),
            _ if targets.len() == 1 => {
                return Err("This acts on files, hunks and lines: move to one".into());
            }
            _ => {}
        }
    }
    for (section, paths) in files {
        whole_files(&mut b, section, &paths)?;
    }
    for ((section, path), picks) in picks {
        parts(&mut b, section, &path, &picks)?;
    }
    if b.plan.commands.is_empty() {
        return Err("Nothing to do here".into());
    }
    if !b.asks.is_empty() {
        b.plan.confirm = Some(b.asks.join(" "));
    }
    b.plan.done = format!("{} {}", act.past(), b.did.join(", "));
    Ok(b.plan)
}

fn wrong(act: Act, section: Section) -> String {
    let what = match section {
        Section::Staged => "staged",
        Section::Unstaged => "not staged",
        Section::Untracked => "untracked",
        Section::Unmerged => "in conflict",
        _ => "not a change",
    };
    let hint = match (act, section) {
        (Act::Stage, Section::Staged) => ": it is staged already",
        (Act::Unstage, _) => ": only staged changes unstage",
        (_, Section::Unmerged) => ": resolve the conflict in the file, then stage it",
        _ => "",
    };
    format!("This is {what}{hint}")
}

fn section(b: &mut Builder<'_>, s: Section) -> Result<(), String> {
    let paths: Vec<String> = b.repo.entries(s).iter().map(|e| e.path.clone()).collect();
    if paths.is_empty() {
        return Err("The section is empty".into());
    }
    let n = paths.len();
    let files = if n == 1 {
        "1 file".to_string()
    } else {
        format!("{n} files")
    };
    match (b.act, s) {
        (Act::Stage, Section::Untracked) => {
            b.ask(true, format!("Stage every untracked file ({files})?"));
            b.plan
                .commands
                .push(cmd::add(paths.iter().map(String::as_str)));
        }
        (Act::Stage, Section::Unstaged) => b.plan.commands.push(cmd::add_tracked()),
        (Act::Stage, Section::Unmerged) => {
            b.ask(true, format!("Mark {files} in conflict resolved?"));
            b.plan
                .commands
                .push(cmd::add(paths.iter().map(String::as_str)));
        }
        (Act::Unstage, Section::Staged) => b.plan.commands.push(cmd::unstage_all(b.repo.unborn())),
        (Act::Discard, Section::Unstaged) => {
            b.ask(
                false,
                format!(
                    "Revert the unstaged changes of {files}? They are in no commit, and are lost."
                ),
            );
            b.plan
                .commands
                .push(cmd::restore(paths.iter().map(String::as_str)));
        }
        (Act::Discard | Act::Delete, Section::Untracked) => {
            b.ask(
                true,
                format!("Delete {files} git does not track? Git cannot bring them back."),
            );
            b.plan
                .commands
                .push(cmd::clean(paths.iter().map(String::as_str)));
        }
        (act, s) => return Err(wrong(act, s)),
    }
    b.did.push(files);
    Ok(())
}

fn whole_files(b: &mut Builder<'_>, section: Section, paths: &[String]) -> Result<(), String> {
    let names = || match paths {
        [one] => one.clone(),
        _ => format!("{} files", paths.len()),
    };
    let all = || paths.iter().map(String::as_str);
    match (b.act, section) {
        (Act::Stage, Section::Untracked | Section::Unstaged | Section::Unmerged) => {
            b.plan.commands.push(cmd::add(all()));
        }
        (Act::Unstage, Section::Staged) => {
            let mut with_sources: Vec<&str> = Vec::new();
            for p in paths {
                with_sources.push(p);
                if let Some(from) = b.repo.entry(section, p).and_then(|e| e.from.as_deref()) {
                    with_sources.push(from);
                }
            }
            b.plan.commands.push(if b.repo.unborn() {
                cmd::rm_cached(with_sources)
            } else {
                cmd::restore_staged(with_sources)
            });
        }
        (Act::Discard, Section::Unstaged) => {
            b.ask(
                false,
                format!(
                    "Revert {}? Its unstaged changes are in no commit, and are lost.",
                    names()
                ),
            );
            b.plan.commands.push(cmd::restore(all()));
        }
        (Act::Discard | Act::Delete, Section::Untracked) => {
            b.ask(
                true,
                format!(
                    "Delete {}? Git does not track it, and cannot bring it back.",
                    names()
                ),
            );
            b.plan.commands.push(cmd::clean(all()));
        }
        (Act::Discard, Section::Staged) => {
            b.ask(false, format!("Revert the staged changes of {}?", names()));
            for p in paths {
                staged_file_discard(b, p)?;
            }
        }
        (Act::Delete, Section::Unstaged | Section::Staged) => {
            b.ask(
                true,
                format!(
                    "Delete {}? Its last committed version stays in the history.",
                    names()
                ),
            );
            b.plan.commands.push(cmd::rm(all()));
        }
        (act, s) => return Err(wrong(act, s)),
    }
    b.did.push(names());
    Ok(())
}

/// A staged file's changes taken out of the index and the working tree,
/// its unstaged changes left alone (2.3).
fn staged_file_discard(b: &mut Builder<'_>, path: &str) -> Result<(), String> {
    let e = b
        .repo
        .entry(Section::Staged, path)
        .ok_or_else(|| format!("{path} is not staged"))?;
    if e.worktree == State::Unmodified {
        if b.repo.unborn() {
            b.plan.commands.push(cmd::rm([path]));
        } else {
            let mut p = vec![path];
            p.extend(e.from.as_deref());
            b.plan.commands.push(cmd::restore_from_head(p));
        }
        return Ok(());
    }
    let Some(d) = b.repo.diff(Section::Staged, path) else {
        return Err(format!(
            "{path} has unstaged changes too: unfold its staged diff first, so that only that is reverted"
        ));
    };
    let p = patch::file_patch(d);
    b.apply(p.clone(), ApplyTo::WorkTree, true);
    b.apply(p, ApplyTo::Index, true);
    Ok(())
}

fn parts(
    b: &mut Builder<'_>,
    section: Section,
    path: &str,
    picks: &[(usize, Pick)],
) -> Result<(), String> {
    if b.act == Act::Delete {
        return Err("Delete acts on whole files".into());
    }
    let d = b
        .repo
        .diff(section, path)
        .ok_or_else(|| format!("The diff of {path} is not loaded"))?;
    if !patch::partial_allowed(d) {
        // A new, deleted or binary file is one hunk: taken whole.
        if picks.iter().all(|(_, p)| *p == Pick::All) {
            return whole_files(b, section, &[path.to_string()]);
        }
        return Err(format!("{path}: {}", patch::Refusal::WholeFileOnly));
    }
    let lines: usize = picks
        .iter()
        .map(|(i, p)| match p {
            Pick::All => d.hunks.get(*i).map_or(0, |h| {
                let (a, r) = h.counts();
                (a + r) as usize
            }),
            Pick::Lines(v) => v.len(),
        })
        .sum();
    let what = match (picks, picks.first().map(|(_, p)| p)) {
        ([_], Some(Pick::All)) => format!("a hunk of {path}"),
        (_, _) if picks.iter().all(|(_, p)| *p == Pick::All) => {
            format!("{} hunks of {path}", picks.len())
        }
        _ => format!("lines of {path}"),
    };
    let refuse = |e: patch::Refusal| format!("{path}: {e}");
    match (b.act, section) {
        (Act::Stage, Section::Unstaged) => {
            let p = patch::build(d, picks, Mode::Forward).map_err(refuse)?;
            b.apply(p, ApplyTo::Index, false);
        }
        (Act::Unstage, Section::Staged) => {
            let p = patch::build(d, picks, Mode::Reverse).map_err(refuse)?;
            b.apply(p, ApplyTo::Index, true);
        }
        (Act::Discard, Section::Unstaged) => {
            let p = patch::build(d, picks, Mode::Reverse).map_err(refuse)?;
            b.ask(
                false,
                format!(
                    "Discard {what}? Its {lines} changed lines are in no commit, and are lost."
                ),
            );
            b.apply(p, ApplyTo::WorkTree, true);
        }
        (Act::Discard, Section::Staged) => {
            let p = patch::build(d, picks, Mode::Reverse).map_err(refuse)?;
            b.ask(false, format!("Discard the staged {what}?"));
            // The working tree first: when it differs there, nothing changes.
            b.apply(p.clone(), ApplyTo::WorkTree, true);
            b.apply(p, ApplyTo::Index, true);
        }
        (act, s) => return Err(wrong(act, s)),
    }
    b.did.push(what);
    Ok(())
}

/// A commit of what is staged.
pub fn commit(
    message: &str,
    kind: &CommitKind,
    signoff: bool,
    repo: &Repo,
) -> Result<Plan, String> {
    let staged = !repo.entries(Section::Staged).is_empty();
    if *kind == CommitKind::New && !staged {
        return Err("Nothing is staged: stage changes first (SPC g S)".into());
    }
    if matches!(
        kind,
        CommitKind::Amend | CommitKind::Extend | CommitKind::Reword
    ) && repo.unborn()
    {
        return Err("There is no commit to amend yet".into());
    }
    if matches!(
        kind,
        CommitKind::New | CommitKind::Amend | CommitKind::Reword
    ) && message_is_empty(message)
    {
        return Err("The message is empty: the commit is cancelled".into());
    }
    if repo
        .entries(Section::Unmerged)
        .iter()
        .any(|e| matches!(e.kind, Kind::Unmerged(_)))
    {
        return Err("Files are in conflict: resolve them and stage them first".into());
    }
    Ok(Plan {
        confirm: None,
        commands: vec![cmd::commit(
            cmd::cut_at_scissors(message, '#'),
            kind,
            signoff,
        )],
        done: match kind {
            CommitKind::New => "Committed".into(),
            CommitKind::Amend | CommitKind::Extend => "Amended the last commit".into(),
            CommitKind::Reword => "Reworded the last commit".into(),
            CommitKind::Fixup(h) => format!("Made a fixup commit for {}", h.get(..7).unwrap_or(h)),
        },
    })
}

/// Whether a message is empty once git cleans it: comment lines and what
/// follows the scissors line left out.
pub fn message_is_empty(message: &str) -> bool {
    cmd::cut_at_scissors(message, '#')
        .lines()
        .filter(|l| !l.starts_with('#'))
        .all(|l| l.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{diff, status};

    fn repo() -> Repo {
        let z = |parts: &[&str]| -> Vec<u8> {
            parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
        };
        let mut r = Repo {
            status: status::parse(&z(&[
                "# branch.oid abc",
                "# branch.head main",
                "1 .M N... 100644 100644 100644 a a f",
                "1 M. N... 100644 100644 100644 a b g",
                "1 MM N... 100644 100644 100644 a b h",
                "2 R. N... 100644 100644 100644 a a R100 new",
                "old",
                "? u",
            ]))
            .unwrap(),
            ..Repo::default()
        };
        let d = |p: &str| {
            diff::parse(
                format!(
                    "diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n"
                )
                .as_bytes(),
            )
            .unwrap()
            .files
            .remove(0)
        };
        r.diffs.insert((Section::Unstaged, "f".into()), d("f"));
        r.diffs.insert((Section::Staged, "h".into()), d("h"));
        r
    }

    fn file(section: Section, path: &str) -> Target {
        Target::File {
            section,
            path: path.into(),
        }
    }

    #[test]
    fn staging_and_unstaging_files() {
        let r = repo();
        let p = plan(
            Act::Stage,
            &[file(Section::Unstaged, "f")],
            &r,
            Settings::default(),
        )
        .unwrap();
        assert_eq!(p.commands[0].args, ["add", "--", "f"]);
        assert_eq!(p.confirm, None);
        assert_eq!(p.done, "Staged f");
        let p = plan(
            Act::Unstage,
            &[file(Section::Staged, "new")],
            &r,
            Settings::default(),
        )
        .unwrap();
        assert_eq!(
            p.commands[0].args,
            ["restore", "--staged", "--", "new", "old"]
        );
        assert!(
            plan(
                Act::Unstage,
                &[file(Section::Unstaged, "f")],
                &r,
                Settings::default()
            )
            .is_err()
        );
        assert!(
            plan(
                Act::Stage,
                &[Target::Commit("abc".into())],
                &r,
                Settings::default()
            )
            .is_err()
        );
    }

    #[test]
    fn a_hunk_and_lines() {
        let r = repo();
        let hunk = Target::Hunk {
            section: Section::Unstaged,
            path: "f".into(),
            hunk: 0,
        };
        let p = plan(Act::Stage, &[hunk], &r, Settings::default()).unwrap();
        assert_eq!(p.commands[0].args, ["apply", "--cached", "-"]);
        assert!(
            p.commands[0]
                .stdin
                .as_ref()
                .unwrap()
                .ends_with(b" a\n-b\n+B\n")
        );
        assert_eq!(p.done, "Staged a hunk of f");
        let lines = Target::Lines {
            section: Section::Unstaged,
            path: "f".into(),
            hunk: 0,
            lines: vec![2],
        };
        let p = plan(Act::Discard, &[lines], &r, Settings::default()).unwrap();
        assert_eq!(p.commands[0].args, ["apply", "--reverse", "-"]);
        assert!(p.confirm.unwrap().starts_with("Discard lines of f?"));
    }

    #[test]
    fn discarding_staged_changes_leaves_the_unstaged_ones() {
        let r = repo();
        // g has no unstaged change: restored from HEAD.
        let p = plan(
            Act::Discard,
            &[file(Section::Staged, "g")],
            &r,
            Settings::default(),
        )
        .unwrap();
        assert_eq!(
            p.commands[0].args,
            [
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                "g"
            ]
        );
        // h has both: its staged patch reversed, the working tree first.
        let p = plan(
            Act::Discard,
            &[file(Section::Staged, "h")],
            &r,
            Settings::default(),
        )
        .unwrap();
        assert_eq!(p.commands.len(), 2);
        assert_eq!(p.commands[0].args, ["apply", "--reverse", "-"]);
        assert_eq!(p.commands[1].args, ["apply", "--cached", "--reverse", "-"]);
    }

    #[test]
    fn what_asks_first() {
        let r = repo();
        let quiet = Settings {
            confirm_discard: false,
            ..Settings::default()
        };
        let p = plan(Act::Discard, &[file(Section::Unstaged, "f")], &r, quiet).unwrap();
        assert_eq!(p.confirm, None);
        let p = plan(Act::Delete, &[file(Section::Untracked, "u")], &r, quiet).unwrap();
        assert!(p.confirm.unwrap().contains("cannot bring it back"));
        let p = plan(
            Act::Stage,
            &[Target::Section(Section::Untracked)],
            &r,
            quiet,
        )
        .unwrap();
        assert_eq!(
            p.confirm.as_deref(),
            Some("Stage every untracked file (1 file)?")
        );
        assert_eq!(p.commands[0].args, ["add", "--", "u"]);
    }

    #[test]
    fn commits() {
        let r = repo();
        assert!(commit("# only a comment\n", &CommitKind::New, false, &r).is_err());
        let p = commit("Fix\n", &CommitKind::New, false, &r).unwrap();
        assert_eq!(p.done, "Committed");
        assert!(message_is_empty(
            "\n# x\n# ------------------------ >8 ------------------------\ndiff"
        ));
        assert!(!message_is_empty("Subject\n# x"));
    }
}
