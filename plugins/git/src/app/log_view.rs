//! The log as a document (DESIGN.md, 2.4): `SPC g L` shows the log of the
//! file it runs in (its renames followed), from a commit's view the log
//! from that commit, else the branch's: one commit a line, 200 at a time,
//! a last line loading 200 more. Enter shows a commit's view; Right
//! unfolds a commit's message and stat under its line, Left folds it, Tab
//! does either; Alt+Enter is the commit's menu (`commit_menu.rs`); Escape
//! and q close it. It is written as a commit's view is (under the key
//! `log:WHAT` in place of a hash), and takes its keys.

use super::commit_menu::{self, CommitAct};
use super::{App, Doc, Effect, Level, Pending};
use crate::content::Content;
use crate::git::cmd::{self, LogSpec};
use crate::git::log;
use crate::model::Folds;
use crate::refresh::{Output, TOO_LARGE};
use crate::target::{self, Target};
use crate::views::log::{self as view, Log};

/// The commits of a page of the log.
pub(super) const PAGE: u32 = 200;

/// The key a log's document is written under, beside the commits' views.
const KEY: &str = "log:";

/// A log shown as a document.
#[derive(Debug, Clone, Default)]
pub(super) struct ShownLog {
    spec: LogSpec,
    log: Log,
    folds: Folds,
    content: Content,
}

impl ShownLog {
    /// Its commit `hash`, when it is loaded.
    pub(super) fn commit(&self, hash: &str) -> Option<&log::Commit> {
        self.log.commits().find(|c| c.hash == hash)
    }
}

/// Whether the key of a commit's view is a log's.
pub(super) fn is_log(key: &str) -> bool {
    key.starts_with(KEY)
}

impl App {
    /// The log of `path` (in the repository), else from commit `from`,
    /// else of the branch: its first page read, its document opened.
    pub(super) fn log_open(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        path: Option<String>,
        from: Option<String>,
    ) {
        let what = path
            .clone()
            .or_else(|| from.clone())
            .unwrap_or_else(|| "HEAD".into());
        let key = format!("{KEY}{what}");
        let spec = LogSpec {
            range: from,
            count: PAGE,
            skip: 0,
            graph: false,
            path,
        };
        let Some(state) = self.repos.get_mut(root) else {
            return;
        };
        let mut shown = ShownLog {
            spec: spec.clone(),
            ..ShownLog::default()
        };
        shown.log.title = what;
        state.logs.insert(key.clone(), shown);
        self.run(
            out,
            Some(root.to_string()),
            cmd::log(&spec),
            Pending::Log {
                root: root.to_string(),
                key,
                show: true,
            },
        );
    }

    /// A page of the log read: added to its document.
    pub(super) fn log_done(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        key: &str,
        show: bool,
        result: Result<Output, String>,
    ) {
        let page = match result {
            Ok(o) if o.truncated => {
                return self.fail(out, format!("The log is too large to show: {TOO_LARGE}"));
            }
            Ok(o) if o.success() => match log::parse(&o.stdout) {
                Ok(p) => p,
                Err(e) => return self.fail(out, e),
            },
            // No commit yet.
            Ok(o) if o.message().contains("does not have any commits") => Vec::new(),
            other => return self.fail(out, super::message(&other)),
        };
        let Some(l) = self.repos.get_mut(root).and_then(|s| s.logs.get_mut(key)) else {
            return;
        };
        let first = l.log.lines.is_empty();
        l.log.extend(page, l.spec.count);
        let cursor = if show || first { Some(0) } else { None };
        self.render_log(out, root, key, show, cursor);
    }

    /// Writes the log's document: opened or shown when `show`, else
    /// written again where it is open; the cursor at `cursor`, else on its
    /// line.
    fn render_log(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        key: &str,
        show: bool,
        cursor: Option<usize>,
    ) {
        let opts = self.view_options();
        let Some(l) = self.repos.get_mut(root).and_then(|s| s.logs.get_mut(key)) else {
            return;
        };
        let content = view::render(&l.log, &l.folds, &opts);
        let (text, styles) = (content.text.clone(), content.styles.clone());
        let title = format!("Log: {}", l.log.title);
        l.content = content;
        out.push(Effect::Document {
            root: root.to_string(),
            commit: Some(key.to_string()),
            title,
            text,
            cursor,
            show,
            styles,
        });
    }

    /// A command run in the log's document `key`; whether the log took it,
    /// as [`App::commit_command`] does for a commit's view.
    pub(super) fn log_command(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        key: &str,
        id: &str,
        doc: &Doc,
    ) -> bool {
        let Some(content) = self
            .repos
            .get(root)
            .and_then(|s| s.logs.get(key))
            .map(|l| l.content.clone())
        else {
            // Not written by this plugin (it started again): closed.
            out.push(Effect::CloseDocument {
                root: root.to_string(),
                commit: Some(key.to_string()),
            });
            return true;
        };
        let head = doc.head.min(content.text.len());
        let commit = match target::at(&content, head) {
            Some(Target::Commit(hash)) => Some(hash),
            _ => None,
        };
        // An entry of the commit's menu, on the commit under the cursor.
        if let Some(act) = CommitAct::of(id) {
            match commit {
                Some(hash) => self.act_on_commit(out, root, act, &hash),
                None => commit_menu::no_commit(out),
            }
            return true;
        }
        match id {
            "git.close" => {
                if let Some(s) = self.repos.get_mut(root) {
                    s.logs.remove(key);
                }
                out.push(Effect::CloseDocument {
                    root: root.to_string(),
                    commit: Some(key.to_string()),
                });
            }
            "git.visit" => match commit {
                Some(hash) => self.show_commit(out, root, &hash),
                None if content.region_at(head).is_some_and(|r| r.key == "more") => {
                    self.log_more(out, root, key);
                }
                None => {}
            },
            "git.toggle" | "git.unfold" | "git.fold" => {
                if let Some(hash) = commit {
                    let fold_key = format!("commit:{hash}");
                    let Some(l) = self.repos.get_mut(root).and_then(|s| s.logs.get_mut(key)) else {
                        return true;
                    };
                    let folded = match id {
                        "git.toggle" => !l.folds.folded(&fold_key),
                        "git.unfold" => false,
                        _ => true,
                    };
                    l.folds.set(&fold_key, folded);
                    let needs = !folded && !l.log.details.contains_key(&hash);
                    let at = content
                        .regions
                        .iter()
                        .find(|r| r.key == fold_key)
                        .map(|r| r.start);
                    self.render_log(out, root, key, false, at);
                    if needs {
                        self.run(
                            out,
                            Some(root.to_string()),
                            cmd::message_and_stat(&hash),
                            Pending::LogDetails {
                                root: root.to_string(),
                                key: key.to_string(),
                                hash,
                            },
                        );
                    }
                }
            }
            "git.next" | "git.previous" => {
                let line = content.line_of(head);
                let starts = content
                    .regions
                    .iter()
                    .filter(|r| r.key.starts_with("commit:"))
                    .map(|r| r.start);
                let to = if id == "git.next" {
                    starts.filter(|s| content.line_of(*s) > line).min()
                } else {
                    starts.filter(|s| content.line_of(*s) < line).max()
                };
                if let Some(to) = to {
                    self.render_log(out, root, key, false, Some(to));
                }
            }
            "git.dispatch" => self.view_menu(out, root, commit.as_deref(), doc, true),
            "git.cycle" => out.push(Effect::Notify(
                "In a log: Enter shows the commit, Right and Left show and hide its message, Alt+Up and Alt+Down step, Alt+Enter lists what can be done with it, Escape or q closes; Enter on the last line loads more"
                    .into(),
                Level::Info,
            )),
            _ => out.push(Effect::Notify(
                "Nothing is staged from a log: the status (SPC g g) does that".into(),
                Level::Info,
            )),
        }
        true
    }

    /// The log's next page read.
    fn log_more(&mut self, out: &mut Vec<Effect>, root: &str, key: &str) {
        let Some(l) = self.repos.get_mut(root).and_then(|s| s.logs.get_mut(key)) else {
            return;
        };
        if l.log.complete {
            return;
        }
        l.spec.skip = l.log.commits().count() as u32;
        let command = cmd::log(&l.spec);
        self.run(
            out,
            Some(root.to_string()),
            command,
            Pending::Log {
                root: root.to_string(),
                key: key.to_string(),
                show: false,
            },
        );
    }

    /// A commit's message and stat read: under its line.
    pub(super) fn log_details_done(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        key: &str,
        hash: String,
        result: Result<Output, String>,
    ) {
        let text = match &result {
            Ok(o) if o.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            other => format!("(not read: {})", super::message(other)),
        };
        let Some(l) = self.repos.get_mut(root).and_then(|s| s.logs.get_mut(key)) else {
            return;
        };
        l.log.details.insert(hash, text);
        self.render_log(out, root, key, false, None);
    }
}
