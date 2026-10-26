//! The commit message as a document (DESIGN.md, 2.4 and G7): `c c`, Amend
//! and Reword open the repository's `COMMIT_EDITMSG` in the editor, filled
//! as git fills it for its editor (the amended commit's message first);
//! saving it commits with what it says, git leaving out its `#` lines,
//! and closing it without a commit cancels, as `git commit` with an
//! editor and Magit's message buffer do. Kalem's plugin documents are
//! read only, so the message is written in the file git keeps it in, and
//! git reads it from there (`-F`). The setting `commit_message = "prompt"`
//! asks for a one-line message instead.

use super::{App, Effect, Level, Pending};
use crate::git::cmd::{self, CommitKind};
use crate::git::status::State;
use crate::model::{Repo, Section};
use crate::refresh::Output;

/// The command that writes the message's first text into its document,
/// run in it once Kalem has opened it.
pub const WRITE: &str = "git.writeMessage";

/// The message being written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Message {
    root: String,
    kind: CommitKind,
    /// Its file, absolute.
    path: String,
    /// Shown in the editor.
    open: bool,
    /// Its commit runs.
    committing: bool,
    /// Its first text, written into its document by [`WRITE`] once
    /// Kalem has opened it.
    fill: Option<String>,
}

impl App {
    /// The message of a commit of kind `kind` in the repository at `root`
    /// opened as a document, starting with `old` (the amended commit's).
    pub(super) fn message_open(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        kind: CommitKind,
        old: String,
    ) {
        self.run(
            out,
            Some(root.to_string()),
            cmd::message_file(),
            Pending::MessageFile {
                root: root.to_string(),
                kind,
                old,
            },
        );
    }

    /// Git said where the message's file is: it is opened, and filled
    /// unless it holds a message whose commit failed after its document
    /// was closed.
    pub(super) fn message_file_done(
        &mut self,
        out: &mut Vec<Effect>,
        root: String,
        kind: CommitKind,
        old: String,
        result: Result<Output, String>,
    ) {
        let file = match &result {
            Ok(o) if o.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            other => return self.fail(out, super::message(other)),
        };
        let path = if file.starts_with('/') || file.get(1..3) == Some(":\\") {
            file
        } else {
            format!("{}/{file}", root.trim_end_matches('/'))
        };
        let Some(repo) = self.repos.get(&root).and_then(|s| s.repo.clone()) else {
            return;
        };
        let kept = self.kept_message.take().is_some_and(|k| k == path);
        out.push(Effect::RunCommand {
            id: "file.open".into(),
            args: super::json_object(&[("path", &path)]),
        });
        let fill = (!kept).then(|| template(&repo, &kind, &old));
        if fill.is_some() {
            out.push(Effect::RunCommand {
                id: WRITE.into(),
                args: "null".into(),
            });
        }
        out.push(Effect::Notify(
            "Write the message: saving commits, closing without saving cancels".into(),
            Level::Info,
        ));
        self.message = Some(Message {
            root,
            kind,
            path,
            open: true,
            committing: false,
            fill,
        });
    }

    /// The message's file and its first text, once ([`WRITE`] writes it
    /// into the document in front when that is the file's).
    pub fn message_fill(&mut self) -> Option<(String, String)> {
        let m = self.message.as_mut()?;
        Some((m.path.clone(), m.fill.take()?))
    }

    /// Whether `path` is the message being written: a file of the
    /// repository's own folder, which the status and the marks leave
    /// alone.
    pub(super) fn is_message(&self, path: &str) -> bool {
        self.message
            .as_ref()
            .is_some_and(|m| super::same_path(&m.path, path))
    }

    /// The message's document saved: committed with what it says.
    pub(super) fn message_saved(&mut self, out: &mut Vec<Effect>) {
        let Some(m) = self.message.as_mut() else {
            return;
        };
        if m.committing {
            return;
        }
        m.committing = true;
        let (root, command) = (
            m.root.clone(),
            cmd::commit_file(&m.path, &m.kind, self.settings.signoff),
        );
        let done = match m.kind {
            CommitKind::New => "Committed",
            CommitKind::Reword => "Reworded the last commit",
            _ => "Amended the last commit",
        };
        self.run(
            out,
            Some(root.clone()),
            command,
            Pending::MessageCommit {
                root,
                done: done.into(),
            },
        );
    }

    /// The message's document closed: a message not committed is given up.
    pub(super) fn message_closed(&mut self, out: &mut Vec<Effect>) {
        match self.message.as_mut() {
            Some(m) if m.committing => m.open = false,
            Some(_) => {
                self.message = None;
                out.push(Effect::Notify(
                    "The commit is cancelled: its message was closed without a commit".into(),
                    Level::Info,
                ));
            }
            None => {}
        }
    }

    /// The commit of the message ended: its document closed when it is
    /// still open; a refusal (an empty message, a hook) said, the document
    /// left open to fix, or kept in its file for the next commit when it
    /// was closed.
    pub(super) fn message_commit_done(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        done: String,
        result: Result<Output, String>,
    ) {
        let Some(m) = self.message.take() else {
            return;
        };
        match &result {
            Ok(o) if o.success() => {
                if m.open {
                    // Kalem closes the document in front: the message's,
                    // brought there first.
                    out.push(Effect::RunCommand {
                        id: "file.open".into(),
                        args: super::json_object(&[("path", &m.path)]),
                    });
                    out.push(Effect::RunCommand {
                        id: "file.close".into(),
                        args: "null".into(),
                    });
                }
                self.error = None;
                out.push(Effect::Notify(done, Level::Info));
            }
            other => {
                let why = super::message(other);
                if m.open {
                    out.push(Effect::Notify(
                        format!("Not committed: {why}"),
                        Level::Warning,
                    ));
                    self.message = Some(Message {
                        committing: false,
                        ..m
                    });
                } else {
                    out.push(Effect::Notify(
                        format!("Not committed: {why}. The message is kept for the next commit"),
                        Level::Warning,
                    ));
                    self.kept_message = Some(m.path);
                }
            }
        }
        self.refresh(out, root, super::After::Nothing);
    }
}

/// The message's first text, as `git commit` writes it for its editor:
/// the amended commit's message (none for a new one), then, in comments,
/// how to end it, the branch and what is staged.
pub fn template(repo: &Repo, kind: &CommitKind, old: &str) -> String {
    let mut s = String::new();
    let old = old.trim_end();
    if !old.is_empty() {
        s.push_str(old);
        s.push('\n');
    }
    s.push('\n');
    s.push_str("# Please enter the commit message for your changes. Lines starting\n");
    s.push_str("# with '#' will be ignored, and an empty message aborts the commit.\n");
    s.push_str("# Save to commit; close without saving to cancel.\n#\n");
    match &repo.status.branch {
        Some(b) => s.push_str(&format!("# On branch {b}\n")),
        None => s.push_str("# HEAD detached\n"),
    }
    if *kind == CommitKind::Reword {
        s.push_str("#\n# Only the message changes: what is staged stays staged.\n");
        return s;
    }
    let staged = repo.entries(Section::Staged);
    if !staged.is_empty() {
        s.push_str("#\n# Changes to be committed:\n");
        for e in staged {
            let what = match e.index {
                State::Added => "new file",
                State::Deleted => "deleted",
                State::Renamed => "renamed",
                State::Copied => "copied",
                State::TypeChanged => "typechange",
                _ => "modified",
            };
            match &e.from {
                Some(from) => s.push_str(&format!("#\t{what}:   {from} -> {}\n", e.path)),
                None => s.push_str(&format!("#\t{what}:   {}\n", e.path)),
            }
        }
    }
    s.push_str("#\n");
    s
}
