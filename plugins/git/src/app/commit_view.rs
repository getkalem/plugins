//! A commit as a document (DESIGN.md, 2.4): Enter on a commit of the
//! status runs `git show` and writes its header, stat and diff as a
//! read-only document of the kind `git-commit-view`, highlighted as
//! `diff`. The arrows and Tab fold its files and hunks, Enter on a diff
//! line opens the file at that line, Escape closes it; nothing is staged
//! from it.

use super::{App, Doc, Effect, Level, Pending, ShownCommit};
use crate::content::{Content, Region};
use crate::git::cmd;
use crate::git::diff::{self, LineKind};
use crate::model::Folds;
use crate::refresh::Output;
use crate::target::{self, Target};
use crate::views;

/// The lines of context of a commit's diff.
const CONTEXT: u32 = 3;

/// What a fold key does in a commit view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Toggle,
    Unfold,
    Fold,
}

impl App {
    /// Enter on a commit: its view, read with `git show` the first time,
    /// shown again when it is open already.
    pub(super) fn show_commit(&mut self, out: &mut Vec<Effect>, root: &str, hash: &str) {
        let shown = self
            .repos
            .get(root)
            .is_some_and(|s| s.commits.contains_key(hash));
        if shown {
            self.render_commit(out, root, hash, true, Some(0));
            return;
        }
        self.run(
            out,
            Some(root.to_string()),
            cmd::show(hash, CONTEXT),
            Pending::Show {
                root: root.to_string(),
                hash: hash.to_string(),
            },
        );
    }

    /// `git show` ended: the commit's document.
    pub(super) fn show_done(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        hash: &str,
        result: Result<Output, String>,
    ) {
        let diff = match result {
            Ok(o) if o.success() => match diff::parse(&o.stdout) {
                Ok(d) => d,
                Err(e) => return self.fail(out, e),
            },
            other => return self.fail(out, super::message(&other)),
        };
        let Some(state) = self.repos.get_mut(root) else {
            return;
        };
        state.commits.insert(
            hash.to_string(),
            ShownCommit {
                diff,
                folds: Folds::default(),
                content: Content::new(),
            },
        );
        self.render_commit(out, root, hash, true, Some(0));
    }

    /// Writes the commit's document: opened or shown when `show`, else
    /// written again where it is open; the cursor at `cursor`, else on
    /// its line.
    fn render_commit(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        hash: &str,
        show: bool,
        cursor: Option<usize>,
    ) {
        let opts = self.view_options();
        let title = self.commit_title(root, hash);
        let Some(c) = self
            .repos
            .get_mut(root)
            .and_then(|s| s.commits.get_mut(hash))
        else {
            return;
        };
        let content = views::commit::render(&c.diff, &c.folds, &opts);
        let text = content.text.clone();
        let styles = content.styles.clone();
        c.content = content;
        out.push(Effect::Document {
            root: root.to_string(),
            commit: Some(hash.to_string()),
            title,
            text,
            cursor,
            show,
            styles,
        });
    }

    /// The document's title: the short hash and the subject, when the
    /// status knows the commit.
    fn commit_title(&self, root: &str, hash: &str) -> String {
        let short: String = hash.chars().take(7).collect();
        let subject = self
            .repos
            .get(root)
            .and_then(|s| s.repo.as_ref())
            .and_then(|r| {
                r.recent
                    .iter()
                    .chain(&r.unpushed)
                    .chain(&r.unpulled)
                    .find(|c| c.hash == hash)
                    .map(|c| c.subject.clone())
            });
        match subject {
            Some(s) => format!("{short} {s}"),
            None => format!("Commit {short}"),
        }
    }

    /// A command run in the view of commit `hash`; whether the view took
    /// it (the status's own commands go on to the status).
    pub(super) fn commit_command(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        hash: &str,
        id: &str,
        doc: &Doc,
    ) -> bool {
        if matches!(id, "git.status" | "git.refresh") {
            return false;
        }
        self.current = Some(root.to_string());
        let Some(content) = self
            .repos
            .get(root)
            .and_then(|s| s.commits.get(hash))
            .map(|c| c.content.clone())
        else {
            // Not written by this plugin (it started again): read anew.
            self.show_commit(out, root, hash);
            return true;
        };
        let head = doc.head.min(content.text.len());
        match id {
            "git.close" => {
                if let Some(s) = self.repos.get_mut(root) {
                    s.commits.remove(hash);
                }
                out.push(Effect::CloseDocument {
                    root: root.to_string(),
                    commit: Some(hash.to_string()),
                });
            }
            "git.toggle" => self.fold_in_commit(out, root, hash, &content, head, Op::Toggle),
            "git.unfold" => self.fold_in_commit(out, root, hash, &content, head, Op::Unfold),
            "git.fold" => self.fold_in_commit(out, root, hash, &content, head, Op::Fold),
            "git.visit" => self.visit_in_commit(out, root, hash, &content, head),
            "git.next" | "git.previous" => {
                let line = content.line_of(head);
                let starts = content
                    .regions
                    .iter()
                    .filter(|r| r.key.starts_with("file:") || r.key.starts_with("hunk:"))
                    .map(|r| r.start);
                let to = if id == "git.next" {
                    starts.filter(|s| content.line_of(*s) > line).min()
                } else {
                    starts.filter(|s| content.line_of(*s) < line).max()
                };
                if let Some(to) = to {
                    self.render_commit(out, root, hash, false, Some(to));
                }
            }
            "git.dispatch" | "git.cycle" => out.push(Effect::Notify(
                "In a commit: Enter opens the file at the line, Tab, Right and Left fold, Escape closes"
                    .into(),
                Level::Info,
            )),
            _ => out.push(Effect::Notify(
                "Nothing is staged from a commit: the status (SPC g g) does that".into(),
                Level::Info,
            )),
        }
        true
    }

    /// Tab, Right and Left on the file or hunk at the cursor, as in the
    /// status.
    fn fold_in_commit(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        hash: &str,
        content: &Content,
        head: usize,
        op: Op,
    ) {
        let regions = content.regions_at(head);
        let foldable: Vec<&Region> = regions.iter().copied().filter(|r| r.foldable).collect();
        let Some(r) = foldable.last().copied() else {
            return;
        };
        let parent = foldable.len().checked_sub(2).map(|i| foldable[i].start);
        let on_first_line = content.line_of(head) == content.line_of(r.start);
        let Some(c) = self
            .repos
            .get_mut(root)
            .and_then(|s| s.commits.get_mut(hash))
        else {
            return;
        };
        let cursor = match op {
            Op::Toggle => {
                if c.folds.toggle(&r.key) {
                    r.start
                } else {
                    head
                }
            }
            Op::Unfold if r.folded => {
                c.folds.set(&r.key, false);
                head
            }
            // Unfolded already: into it, as a tree view's Right goes.
            Op::Unfold => {
                if !on_first_line {
                    return;
                }
                content.line_start(content.line_of(r.start) + 1).min(r.end)
            }
            Op::Fold if !r.folded => {
                c.folds.set(&r.key, true);
                r.start
            }
            // Folded already: to what holds it.
            Op::Fold => match parent {
                Some(p) => p,
                None => return,
            },
        };
        self.render_commit(out, root, hash, false, Some(cursor));
    }

    /// Enter: on a diff line, the file at that line as the commit left
    /// it; on a file's header, its fold.
    fn visit_in_commit(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        hash: &str,
        content: &Content,
        head: usize,
    ) {
        match target::at(content, head) {
            Some(Target::Hunk { path, hunk, .. }) => {
                let line = self
                    .repos
                    .get(root)
                    .and_then(|s| s.commits.get(hash))
                    .and_then(|c| c.diff.files.iter().find(|f| f.path() == path))
                    .and_then(|f| f.hunks.get(hunk))
                    .map(|h| {
                        let mut n = h.new_start.max(1);
                        if let Some(before) = target::line_in_hunk(content, head) {
                            for l in h.lines.iter().take(before) {
                                if matches!(l.kind, LineKind::Context | LineKind::Added) {
                                    n += 1;
                                }
                            }
                        }
                        n
                    });
                self.open_file(out, root, &path, line);
            }
            Some(Target::Lines { path, .. }) => self.open_file(out, root, &path, None),
            Some(Target::File { .. }) => {
                self.fold_in_commit(out, root, hash, content, head, Op::Toggle);
            }
            _ => {}
        }
    }
}
