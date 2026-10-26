//! A commit's menu (DESIGN.md, 2.2 and 2.4): Alt+Enter (or `SPC g /`) on
//! a commit of the log or of the status's Commits tab, or in a commit's
//! view, lists what can be done with it, each entry a command of its own
//! acting on the commit under the cursor: check it out (HEAD detached
//! there), cherry-pick it onto the current branch, revert it (asked
//! first), reset the branch to it (soft, mixed, or hard, asked first), fix
//! it up with what is staged. A cherry-pick or a revert stopped on a
//! conflict shows the status, which names the operation under way and
//! offers to continue, skip or abort it. Copy hash waits for a command of
//! Kalem's that puts a text on the clipboard (DESIGN.md, 9.5).

use super::document::key_label;
use super::{After, App, COMMIT_KIND, Doc, Effect, Level, Pending, Question, command_info};
use crate::actions;
use crate::git::cmd::{self, CommitKind, GitCommand};
use crate::git::refs::Operation;
use crate::model::Repo;
use crate::refresh::Output;

/// What an entry of a commit's menu does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommitAct {
    /// Checks it out, HEAD detached there.
    Checkout,
    /// Makes its changes a new commit on the current branch.
    CherryPick,
    /// Makes a new commit undoing its changes, after asking.
    Revert,
    /// Moves the branch to it: soft, mixed, or hard after asking.
    Reset,
    /// Makes a `fixup!` commit of what is staged, for it.
    Fixup,
}

impl CommitAct {
    /// The entry command `id` is, if it is one.
    pub(super) fn of(id: &str) -> Option<CommitAct> {
        Some(match id {
            "git.checkoutCommit" => CommitAct::Checkout,
            "git.cherryPick" => CommitAct::CherryPick,
            "git.revertCommit" => CommitAct::Revert,
            "git.resetHere" => CommitAct::Reset,
            "git.fixup" => CommitAct::Fixup,
            _ => return None,
        })
    }
}

/// What a commit's view and a log offer after the commit's own entries:
/// the repository's commands, which act there as anywhere.
const VIEW_COMMANDS: &[&str] = &[
    "git.status",
    "git.commit",
    "git.amend",
    "git.push",
    "git.pull",
    "git.fetch",
    "git.switchBranch",
    "git.newBranch",
    "git.close",
];

/// A commit's short hash.
fn short(hash: &str) -> &str {
    hash.get(..7).unwrap_or(hash)
}

/// The key a menu of a commit's view or of a log names for command `id`:
/// its key there, else its leader key (magit's letters are the status's
/// alone).
fn view_key(id: &str) -> Option<String> {
    let c = command_info(id)?;
    let here = c.kinds.contains(&COMMIT_KIND);
    let doc = c.doc_keys.iter().find(|k| here || c.view_keys.contains(k));
    doc.or(c.keys.first()).map(|k| key_label(k))
}

/// The branch as the menu and the messages name it: HEAD when it is
/// detached.
fn branch(repo: Option<&Repo>) -> String {
    repo.and_then(|r| r.status.branch.clone())
        .unwrap_or_else(|| "HEAD".into())
}

/// The merge, rebase, cherry-pick or revert under way: a bisection lets a
/// commit be picked or reverted.
fn under_way(repo: &Repo) -> Option<Operation> {
    repo.operation.filter(|o| *o != Operation::Bisect)
}

/// An entry of a commit's menu run where no commit is: said.
pub(super) fn no_commit(out: &mut Vec<Effect>) {
    out.push(Effect::Notify(
        "No commit here: the log (SPC g L) and the status's Commits tab (3) list them".into(),
        Level::Info,
    ));
}

impl App {
    /// The entries of commit `hash`'s menu, listed first in the status's
    /// menu, a log's and a commit's view's: Show (`show`, where the commit
    /// is a line), then what can be done with it.
    pub(super) fn commit_entries(
        &self,
        root: &str,
        hash: &str,
        show: bool,
    ) -> Vec<(&'static str, String)> {
        let short = short(hash);
        let on = branch(self.repos.get(root).and_then(|s| s.repo.as_ref()));
        let mut v = Vec::new();
        if show {
            v.push(("git.visit", format!("Show {short}")));
        }
        v.extend([
            (
                "git.checkoutCommit",
                format!("Check out {short}, HEAD detached there"),
            ),
            ("git.cherryPick", format!("Cherry-pick {short} onto {on}")),
            ("git.revertCommit", format!("Revert {short}…")),
            (
                "git.resetHere",
                format!("Reset {on} to {short} (soft, mixed, hard)…"),
            ),
            ("git.fixup", format!("Fix up {short} with what is staged")),
        ]);
        v
    }

    /// Alt+Enter (`SPC g /`) in a commit's view, or in a log (`log`) on
    /// commit `hash` or elsewhere: the commit's menu, then the repository's
    /// commands, each with its key there.
    pub(super) fn view_menu(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        hash: Option<&str>,
        doc: &Doc,
        log: bool,
    ) {
        let mut entries = hash
            .map(|h| self.commit_entries(root, h, log))
            .unwrap_or_default();
        match hash {
            Some(_) if log => {
                entries.push(("git.toggle", "Show or hide its message and files".into()));
            }
            Some(h) => entries.push(("git.log", format!("The log from {}", short(h)))),
            None => {}
        }
        for id in VIEW_COMMANDS {
            let c = command_info(id).expect("a command of the plugin");
            entries.push((id, c.title.trim_start_matches("Git: ").to_string()));
        }
        let items = entries
            .iter()
            .map(|(id, label)| (label.clone(), view_key(id)))
            .collect();
        let token = self.ask(Question::Commands {
            ids: entries.into_iter().map(|(id, _)| id).collect(),
            doc: Some(doc.clone()),
        });
        out.push(Effect::Pick {
            token,
            title: "Git".into(),
            items,
        });
    }

    /// Entry `act` of a commit's menu on commit `hash`, the repository read
    /// again first: what is staged, the branch and the operation under way
    /// as they are now.
    pub(super) fn act_on_commit(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        act: CommitAct,
        hash: &str,
    ) {
        self.refresh(out, root, After::OnCommit(act, hash.to_string()));
    }

    /// Entry `act` of a commit's menu on commit `hash`, the repository
    /// read.
    pub(super) fn on_commit(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        repo: &Repo,
        act: CommitAct,
        hash: &str,
    ) {
        let short = short(hash);
        let on = branch(Some(repo));
        let (subject, merge) = self.about(root, hash);
        let named = match &subject {
            Some(s) => format!("{short} ({s})"),
            None => short.to_string(),
        };
        // Git refuses them while another is under way; said here, the
        // refusal is not taken for a stop on a conflict.
        if matches!(act, CommitAct::CherryPick | CommitAct::Revert)
            && let Some(op) = under_way(repo)
        {
            out.push(Effect::Notify(
                format!(
                    "A {} is under way: continue or abort it first (Enter on its line in the status)",
                    op.name()
                ),
                Level::Warning,
            ));
            return;
        }
        match act {
            CommitAct::Checkout => self.run_plan(
                out,
                root,
                vec![cmd::checkout_detached(hash)],
                format!(
                    "Checked out {short}: HEAD is detached there (b b switches back to a branch)"
                ),
            ),
            CommitAct::CherryPick => self.replay(
                out,
                root,
                cmd::cherry_pick(hash, merge),
                format!("Cherry-picked {short} onto {on}"),
            ),
            CommitAct::Revert => {
                let what = if merge {
                    "the changes it merged in"
                } else {
                    "its changes"
                };
                let token = self.ask(Question::Replay {
                    root: root.to_string(),
                    command: cmd::revert(hash, merge),
                    done: format!("Reverted {short} on {on}"),
                });
                out.push(Effect::Confirm {
                    token,
                    message: format!("Revert {named}? A new commit on {on} undoes {what}."),
                });
            }
            CommitAct::Reset => {
                let token = self.ask(Question::Reset {
                    root: root.to_string(),
                    hash: hash.to_string(),
                    on: on.clone(),
                });
                out.push(Effect::Pick {
                    token,
                    title: format!("Reset {on} to {named}"),
                    items: vec![
                        (
                            "Soft".into(),
                            Some("The files and what is staged stay as they are".into()),
                        ),
                        (
                            "Mixed".into(),
                            Some("The files stay as they are; nothing is staged".into()),
                        ),
                        (
                            "Hard…".into(),
                            Some(
                                "The files as the commit has them: uncommitted changes are lost"
                                    .into(),
                            ),
                        ),
                    ],
                });
            }
            CommitAct::Fixup => {
                let kind = CommitKind::Fixup(hash.to_string());
                match actions::commit("", &kind, self.settings.signoff, repo) {
                    Ok(plan) => self.start_plan(out, root, plan),
                    Err(e) => out.push(Effect::Notify(e, Level::Warning)),
                }
            }
        }
    }

    /// The reset picked (0 soft, 1 mixed, 2 hard) of branch `on` to commit
    /// `hash`: a hard one asks first, its uncommitted changes being lost.
    pub(super) fn reset(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        hash: &str,
        on: &str,
        picked: u32,
    ) {
        let short = short(hash);
        let (how, done, confirm) = match picked {
            0 => (
                cmd::Reset::Soft,
                format!("Reset {on} to {short}, the files and what is staged kept"),
                None,
            ),
            1 => (
                cmd::Reset::Mixed,
                format!("Reset {on} to {short}, the files kept and nothing staged"),
                None,
            ),
            2 => (
                cmd::Reset::Hard,
                format!("Reset {on} hard to {short}"),
                Some(format!(
                    "Reset {on} hard to {short}? Uncommitted changes are lost: the files become as {short} has them."
                )),
            ),
            _ => return,
        };
        let plan = actions::Plan {
            confirm,
            commands: vec![cmd::reset(how, hash)],
            done,
            ..actions::Plan::default()
        };
        self.start_plan(out, root, plan);
    }

    /// Runs a cherry-pick or a revert, which may stop on a conflict.
    pub(super) fn replay(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        command: GitCommand,
        done: String,
    ) {
        let why = Pending::Replay {
            root: root.to_string(),
            done,
        };
        self.run(out, Some(root.to_string()), command, why);
    }

    /// A cherry-pick or a revert ended: done, or failed, perhaps stopped on
    /// a conflict ([`App::stopped`], once the repository is read again).
    pub(super) fn replay_done(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        done: String,
        result: Result<Output, String>,
    ) {
        match &result {
            Ok(o) if o.success() => {
                self.error = None;
                out.push(Effect::Notify(done, Level::Info));
                self.refresh(out, root, After::Nothing);
            }
            other => self.refresh(out, root, After::Stopped(super::message(other))),
        }
    }

    /// A cherry-pick or a revert failed with `why`: stopped on a conflict,
    /// its operation under way, the status is shown saying so (Enter on
    /// its line continues, skips or aborts it); else `why` is said.
    pub(super) fn stopped(&mut self, out: &mut Vec<Effect>, root: &str, repo: &Repo, why: String) {
        let Some(op) = under_way(repo) else {
            return self.fail(out, why);
        };
        out.push(Effect::Notify(
            format!(
                "The {} stopped: {why}. Resolve and stage the files in conflict (s), then Enter on its line in the status continues, skips or aborts it",
                op.name()
            ),
            Level::Warning,
        ));
        self.write_status(out, root, true);
    }

    /// Commit `hash`'s subject and whether it is a merge, as far as the
    /// plugin has read it: among the status's commits and a log's, or in
    /// its view's header.
    fn about(&self, root: &str, hash: &str) -> (Option<String>, bool) {
        let Some(s) = self.repos.get(root) else {
            return (None, false);
        };
        let listed = s
            .repo
            .iter()
            .flat_map(|r| r.recent.iter().chain(&r.unpushed).chain(&r.unpulled))
            .filter(|c| c.hash == hash);
        let logged = s.logs.values().filter_map(|l| l.commit(hash));
        if let Some(c) = listed.chain(logged).next() {
            return (Some(c.subject.clone()), c.parents.len() > 1);
        }
        let Some(c) = s.commits.get(hash) else {
            return (None, false);
        };
        // `git show --format=fuller`: a merge's `Merge:` line, then the
        // message indented by four spaces.
        let lines = || {
            c.diff
                .preamble
                .iter()
                .map(|l| String::from_utf8_lossy(l).into_owned())
        };
        let merge = lines().any(|l| l.starts_with("Merge: "));
        let subject = lines().find_map(|l| l.strip_prefix("    ").map(str::to_string));
        (subject, merge)
    }
}
