//! The status document (DESIGN.md, 2.1 to 2.3; phase 1): the status of a
//! repository as a document of Kalem's (`documents`, plugin API 0.2.5),
//! written again whenever the repository changes. Its keys need nothing
//! learned first, as lazygit's: Tab and the arrows fold and unfold its
//! sections, files and hunks, Enter opens what is under the cursor,
//! Alt+Enter (or `?`) lists what can be done with it, Escape (or `q`)
//! closes it; magit's letters act on it (`s`, `u`, `x`, `c c`, `P p`…).

use super::*;
use crate::content::Region;
use crate::git::diff::LineKind;
use crate::git::status::State;
use crate::refresh::missing_diffs;
use crate::target::{self, Target};
use crate::views::{self, ViewOptions};

/// The lines of context of the status's diffs.
const CONTEXT: u32 = 3;

/// Where the cursor goes when the status is written again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keep {
    /// At this byte of the new text.
    At(usize),
    /// On the thing it was on, as many lines into it, else (`following`)
    /// on the first thing after it still there, else on the innermost
    /// thing around it still there, else on its line.
    Item {
        /// The keys of the regions it was in, innermost first.
        keys: Vec<String>,
        /// Its line in the innermost one.
        delta: usize,
        /// Its line in the document.
        line: usize,
        /// What follows it before what holds it: after an action (a file
        /// staged leaves its section, the cursor goes to the next file),
        /// not after a fold.
        following: bool,
    },
}

/// What a fold key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FoldOp {
    Toggle,
    Unfold,
    Fold,
}

/// The status document's title.
fn title(root: &str) -> String {
    let name = root
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(root);
    format!("Git: {name}")
}

/// The first byte of line `line` (from 0), the last line's for one past it.
fn line_start(c: &Content, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut n = 0;
    for (i, b) in c.text.bytes().enumerate() {
        if b == b'\n' {
            n += 1;
            if n == line {
                return (i + 1).min(c.text.len());
            }
        }
    }
    // Past the end: the last line's start.
    c.text[..c.text.len().saturating_sub(1)]
        .rfind('\n')
        .map_or(0, |i| i + 1)
}

/// The number of lines of region `r`.
fn lines_of(c: &Content, r: &Region) -> usize {
    c.line_of(r.end.saturating_sub(1).max(r.start)) - c.line_of(r.start) + 1
}

/// Where the cursor goes in `new` for `keep`, `old` being the text it was
/// in.
pub(super) fn place(old: Option<&Content>, new: &Content, keep: &Keep) -> usize {
    let find = |key: &str| new.regions.iter().find(|r| r.key == key);
    match keep {
        Keep::At(o) => (*o).min(new.text.len()),
        Keep::Item {
            keys,
            delta,
            line,
            following,
        } => {
            if let Some(r) = keys.first().and_then(|k| find(k)) {
                let d = (*delta).min(lines_of(new, r) - 1);
                return line_start(new, new.line_of(r.start) + d);
            }
            let after = || {
                let old = old?;
                let first = keys.first()?;
                let at = old.regions.iter().position(|r| &r.key == first)?;
                let end = old.regions[at].end;
                old.regions[at + 1..]
                    .iter()
                    .filter(|r| r.start >= end && is_item(&r.key))
                    .find_map(|r| find(&r.key))
                    .map(|r| r.start)
            };
            let around = || keys.iter().skip(1).find_map(|k| find(k)).map(|r| r.start);
            let found = if *following {
                after().or_else(around)
            } else {
                around().or_else(after)
            };
            found.unwrap_or_else(|| line_start(new, *line))
        }
    }
}

/// A region the cursor stops on: what Alt+Up and Alt+Down step through,
/// and where it goes when what it was on is gone.
fn is_item(key: &str) -> bool {
    matches!(key, "head" | "upstream" | "tag" | "operation")
        || [
            "section:", "file:", "hunk:", "commit:", "branch:", "stash:", "big:",
        ]
        .iter()
        .any(|p| key.starts_with(p))
}

/// Where the cursor at `head` is, to keep it there.
fn keep_at(c: &Content, head: usize, following: bool) -> Keep {
    let regions = c.regions_at(head);
    let keys: Vec<String> = regions.iter().rev().map(|r| r.key.clone()).collect();
    let line = c.line_of(head);
    let delta = regions
        .last()
        .map_or(0, |r| line.saturating_sub(c.line_of(r.start)));
    Keep::Item {
        keys,
        delta,
        line,
        following,
    }
}

/// The key of a command as the menus show it: `Tab`, `c c`, `P p`.
pub(super) fn key_label(keys: &str) -> String {
    keys.split(' ')
        .map(|chord| match chord {
            "space" => "SPC".to_string(),
            "tab" => "Tab".into(),
            "shift+tab" => "Shift+Tab".into(),
            "enter" => "Enter".into(),
            "alt+enter" => "Alt+Enter".into(),
            "escape" => "Esc".into(),
            "right" => "→".into(),
            "left" => "←".into(),
            "alt+down" => "Alt+↓".into(),
            "alt+up" => "Alt+↑".into(),
            c => match c.strip_prefix("shift+") {
                Some(k) if k.chars().count() == 1 => k.to_uppercase(),
                _ => c.to_string(),
            },
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The key a menu names for command `id`: its key in the status document,
/// else its leader key.
fn menu_key(id: &str) -> Option<String> {
    let c = command_info(id)?;
    c.doc_keys.first().or(c.keys.first()).map(|k| key_label(k))
}

impl App {
    fn view_options(&self) -> ViewOptions {
        ViewOptions {
            glyphs: self.settings.glyphs,
            ..ViewOptions::default()
        }
    }

    /// Writes the status document of `root`: opened or shown when `show`,
    /// else written again where it is open, once the diffs its unfolded
    /// files need are read.
    pub(super) fn write_status(&mut self, out: &mut Vec<Effect>, root: &str, show: bool) {
        let reading = self.load_diffs(out, root);
        if reading && !show {
            return;
        }
        self.render_status(out, root, show);
    }

    /// Writes the status document of `root` as its repository and folds
    /// are now.
    fn render_status(&mut self, out: &mut Vec<Effect>, root: &str, show: bool) {
        let opts = self.view_options();
        let Some(state) = self.repos.get_mut(root) else {
            return;
        };
        let Some(repo) = state.repo.as_ref() else {
            return;
        };
        let content = views::status::render(repo, &state.folds, &opts);
        let cursor = state
            .keep
            .take()
            .map(|k| place(state.content.as_ref(), &content, &k));
        let text = content.text.clone();
        state.content = Some(content);
        state.shown = true;
        out.push(Effect::Document {
            root: root.to_string(),
            title: title(root),
            text,
            cursor,
            show,
        });
    }

    /// Starts reading the diffs the unfolded files need; whether some are
    /// being read.
    fn load_diffs(&mut self, out: &mut Vec<Effect>, root: &str) -> bool {
        let Some(state) = self.repos.get(root) else {
            return false;
        };
        let Some(repo) = state.repo.as_ref() else {
            return false;
        };
        let generation = repo.generation;
        let missing: Vec<_> = missing_diffs(repo, &state.folds, CONTEXT)
            .into_iter()
            .filter(|(k, _)| !state.loading.contains(k))
            .collect();
        for ((section, path), command) in missing {
            if let Some(s) = self.repos.get_mut(root) {
                s.loading.insert((section, path.clone()));
            }
            self.run(
                out,
                Some(root.to_string()),
                command,
                Pending::Diff {
                    root: root.to_string(),
                    generation,
                    section,
                    path,
                },
            );
        }
        self.repos.get(root).is_some_and(|s| !s.loading.is_empty())
    }

    /// A file's diff was read: kept, and the status written again once
    /// every diff it waited for is.
    pub(super) fn diff_done(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        generation: u64,
        section: Section,
        path: &str,
        result: Result<Output, String>,
    ) {
        let Some(state) = self.repos.get_mut(root) else {
            return;
        };
        let Some(repo) = state.repo.as_mut() else {
            return;
        };
        if repo.generation != generation {
            return;
        }
        state.loading.remove(&(section, path.to_string()));
        // `diff --no-index` ends with 1 when the files differ.
        let bytes = match &result {
            Ok(o) if o.success() || (section == Section::Untracked && o.status == Some(1)) => {
                o.stdout.clone()
            }
            _ => Vec::new(),
        };
        if repo.set_diff(section, path, &bytes).is_err() {
            let _ = repo.set_diff(section, path, b"");
        }
        if state.loading.is_empty() && state.shown {
            self.render_status(out, root, false);
        }
    }

    /// Runs command `id` in the status document of `root`; `false` when the
    /// document has nothing of its own to do with it (commit, push…).
    pub(super) fn status_command(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        id: &str,
        doc: &Doc,
    ) -> bool {
        self.current = Some(root.to_string());
        let Some(content) = self.repos.get(root).and_then(|s| s.content.clone()) else {
            // Not written by this plugin yet (it started again): written
            // anew.
            if command_info(id).is_some_and(|c| c.in_status_only) || id == "git.status" {
                self.refresh(out, root, After::Show);
                return true;
            }
            return false;
        };
        let head = doc.head.min(content.text.len());
        let anchor = doc.anchor.min(content.text.len());
        match id {
            "git.status" | "git.refresh" => {
                if let Some(s) = self.repos.get_mut(root) {
                    s.keep = Some(keep_at(&content, head, false));
                }
                self.refresh(out, root, After::Nothing);
            }
            "git.toggle" => self.fold(out, root, &content, head, FoldOp::Toggle),
            "git.unfold" => self.fold(out, root, &content, head, FoldOp::Unfold),
            "git.fold" => self.fold(out, root, &content, head, FoldOp::Fold),
            "git.cycle" => self.cycle(out, root, &content, head),
            "git.visit" => self.visit(out, root, &content, head, doc),
            "git.dispatch" => self.menu(out, root, &content, head, doc),
            "git.next" | "git.previous" => self.step(out, root, &content, head, id == "git.next"),
            "git.stage" | "git.unstage" | "git.discard" => {
                let act = match id {
                    "git.stage" => Act::Stage,
                    "git.unstage" => Act::Unstage,
                    _ => Act::Discard,
                };
                self.act_here(out, root, &content, anchor, head, act, false);
            }
            "git.stageFile" | "git.unstageFile" | "git.revertFile" | "git.deleteFile" => {
                let act = match id {
                    "git.stageFile" => Act::Stage,
                    "git.unstageFile" => Act::Unstage,
                    "git.revertFile" => Act::Discard,
                    _ => Act::Delete,
                };
                self.act_here(out, root, &content, anchor, head, act, true);
            }
            "git.stageAll" | "git.unstageAll" => {
                let Some(repo) = self.repos.get(root).and_then(|s| s.repo.clone()) else {
                    return true;
                };
                let (section, act) = if id == "git.stageAll" {
                    (Section::Unstaged, Act::Stage)
                } else {
                    (Section::Staged, Act::Unstage)
                };
                let settings = self.action_settings();
                match actions::plan(act, &[Target::Section(section)], &repo, settings) {
                    Ok(plan) => {
                        if let Some(s) = self.repos.get_mut(root) {
                            s.keep = Some(keep_at(&content, head, true));
                        }
                        self.start_plan(out, root, plan);
                    }
                    Err(_) => out.push(Effect::Notify(
                        if act == Act::Stage {
                            "No change of a tracked file to stage".into()
                        } else {
                            "Nothing is staged".into()
                        },
                        Level::Info,
                    )),
                }
            }
            "git.close" => {
                if let Some(s) = self.repos.get_mut(root) {
                    s.shown = false;
                    s.content = None;
                    s.keep = None;
                }
                out.push(Effect::CloseDocument {
                    root: root.to_string(),
                });
            }
            _ => return false,
        }
        true
    }

    fn action_settings(&self) -> actions::Settings {
        actions::Settings {
            confirm_discard: self.settings.confirm_discard,
            zero_context: false,
        }
    }

    /// Tab, Right and Left on the thing at `head`.
    fn fold(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        content: &Content,
        head: usize,
        op: FoldOp,
    ) {
        let regions = content.regions_at(head);
        let foldable: Vec<&Region> = regions.iter().copied().filter(|r| r.foldable).collect();
        let Some(r) = foldable.last().copied() else {
            return;
        };
        let parent = foldable.len().checked_sub(2).map(|i| foldable[i].start);
        let on_first_line = content.line_of(head) == content.line_of(r.start);
        let Some(state) = self.repos.get_mut(root) else {
            return;
        };
        let keep = match op {
            FoldOp::Toggle => {
                let folded = state.folds.toggle(&r.key);
                if folded {
                    Keep::At(r.start)
                } else {
                    Keep::At(head)
                }
            }
            FoldOp::Unfold if r.folded => {
                state.folds.set(&r.key, false);
                Keep::At(head)
            }
            // Unfolded already: into it, as a tree view's Right goes.
            FoldOp::Unfold => {
                if !on_first_line {
                    return;
                }
                Keep::At(line_start(content, content.line_of(r.start) + 1).min(r.end))
            }
            FoldOp::Fold if !r.folded => {
                state.folds.set(&r.key, true);
                Keep::At(r.start)
            }
            // Folded already: to what holds it.
            FoldOp::Fold => match parent {
                Some(p) => Keep::At(p),
                None => return,
            },
        };
        state.keep = Some(keep);
        self.load_diffs(out, root);
        self.render_status(out, root, false);
    }

    /// Shift+Tab: every file and hunk unfolded, then every section folded,
    /// then each as it starts.
    fn cycle(&mut self, out: &mut Vec<Effect>, root: &str, content: &Content, head: usize) {
        let Some(state) = self.repos.get_mut(root) else {
            return;
        };
        let Some(repo) = state.repo.as_ref() else {
            return;
        };
        state.cycle = (state.cycle + 1) % 3;
        match state.cycle {
            1 => {
                state.folds.reset();
                for section in [
                    Section::Unmerged,
                    Section::Untracked,
                    Section::Unstaged,
                    Section::Staged,
                ] {
                    for e in repo.entries(section) {
                        state
                            .folds
                            .set(&crate::target::file_key(section, &e.path), false);
                    }
                }
            }
            2 => {
                for r in content
                    .regions
                    .iter()
                    .filter(|r| r.key.starts_with("section:"))
                {
                    state.folds.set(&r.key, true);
                }
            }
            _ => state.folds.reset(),
        }
        state.keep = Some(keep_at(content, head, false));
        self.load_diffs(out, root);
        self.render_status(out, root, false);
    }

    /// Alt+Down and Alt+Up: the next or previous section, file, hunk or
    /// commit.
    fn step(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        content: &Content,
        head: usize,
        next: bool,
    ) {
        let line = content.line_of(head);
        let starts = content
            .regions
            .iter()
            .filter(|r| is_item(&r.key))
            .map(|r| r.start);
        let to = if next {
            starts.filter(|s| content.line_of(*s) > line).min()
        } else {
            starts.filter(|s| content.line_of(*s) < line).max()
        };
        if let Some(to) = to {
            out.push(Effect::Document {
                root: root.to_string(),
                title: title(root),
                text: content.text.clone(),
                cursor: Some(to),
                show: false,
            });
        }
    }

    /// `s`, `u`, `x` (and `SPC g S`, `SPC g U`, `SPC g R`, `SPC g D` with
    /// `whole`, its file) on the thing at the cursor, or on the lines
    /// selected.
    #[allow(clippy::too_many_arguments)]
    fn act_here(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        content: &Content,
        anchor: usize,
        head: usize,
        act: Act,
        whole: bool,
    ) {
        let Some(repo) = self.repos.get(root).and_then(|s| s.repo.clone()) else {
            return;
        };
        let mut targets = if anchor != head {
            target::in_selection(content, anchor, head)
        } else {
            target::at(content, head).into_iter().collect()
        };
        if whole {
            let mut files: Vec<Target> = Vec::new();
            for t in &targets {
                let f = match t.file() {
                    Some((section, path)) => Target::File {
                        section,
                        path: path.to_string(),
                    },
                    None => t.clone(),
                };
                if !files.contains(&f) {
                    files.push(f);
                }
            }
            targets = files;
        }
        let settings = self.action_settings();
        match actions::plan(act, &targets, &repo, settings) {
            Ok(plan) => {
                if let Some(s) = self.repos.get_mut(root) {
                    s.keep = Some(keep_at(content, head, true));
                }
                self.start_plan(out, root, plan);
            }
            Err(e) => out.push(Effect::Notify(e, Level::Warning)),
        }
    }

    /// Enter: the one obvious thing with what is under the cursor.
    fn visit(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        content: &Content,
        head: usize,
        doc: &Doc,
    ) {
        let Some(repo) = self.repos.get(root).and_then(|s| s.repo.clone()) else {
            return;
        };
        let pick = |app: &mut App, out: &mut Vec<Effect>, title: &str, ids: Vec<&'static str>| {
            let items = ids
                .iter()
                .map(|id| {
                    let c = command_info(id).expect("a command of the plugin");
                    (
                        c.title.trim_start_matches("Git: ").to_string(),
                        menu_key(id),
                    )
                })
                .collect();
            let token = app.ask(Question::Commands {
                ids,
                doc: Some(doc.clone()),
            });
            out.push(Effect::Pick {
                token,
                title: title.into(),
                items,
            });
        };
        match target::at(content, head) {
            None | Some(Target::Help) => self.menu(out, root, content, head, doc),
            Some(Target::Head) => pick(
                self,
                out,
                "The branch",
                vec!["git.switchBranch", "git.newBranch", "git.stash"],
            ),
            Some(Target::Upstream) => pick(
                self,
                out,
                "The upstream",
                vec!["git.push", "git.pull", "git.fetch"],
            ),
            Some(Target::Tag) => out.push(Effect::Notify(
                "The tags come in a later version of the plugin".into(),
                Level::Info,
            )),
            Some(Target::Operation) => out.push(Effect::Notify(
                "Continue, skip or abort it with git for now: the plugin's buttons for it come later"
                    .into(),
                Level::Info,
            )),
            Some(Target::Section(_)) => self.fold(out, root, content, head, FoldOp::Toggle),
            Some(Target::File { section, path }) => {
                let gone = repo.entry(section, &path).is_some_and(|e| {
                    e.worktree == State::Deleted
                        || (e.index == State::Deleted && e.worktree == State::Unmodified)
                });
                if gone {
                    out.push(Effect::Notify(
                        format!("{path} is deleted: Tab shows what was in it"),
                        Level::Info,
                    ));
                    return;
                }
                self.open_file(out, root, &path, None);
            }
            Some(Target::Hunk {
                section,
                path,
                hunk,
            }) => {
                let line = repo
                    .diff(section, &path)
                    .and_then(|d| d.hunks.get(hunk))
                    .map(|h| {
                        let before = target::line_in_hunk(content, head).unwrap_or(0);
                        let on_header = target::line_in_hunk(content, head).is_none();
                        let mut n = h.new_start.max(1);
                        if !on_header {
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
            Some(Target::Stash(index)) => {
                let token = self.ask(Question::Stash {
                    root: root.to_string(),
                    index,
                });
                out.push(Effect::Pick {
                    token,
                    title: format!("stash@{{{index}}}"),
                    items: vec![
                        ("Apply".into(), Some("Its changes, the stash kept".into())),
                        ("Pop".into(), Some("Its changes, the stash dropped".into())),
                        ("Drop…".into(), Some("The stash thrown away".into())),
                    ],
                });
            }
            Some(Target::Commit(hash)) => {
                let all = repo
                    .recent
                    .iter()
                    .chain(&repo.unpushed)
                    .chain(&repo.unpulled);
                let text = match all.into_iter().find(|c| c.hash == hash) {
                    Some(c) => format!(
                        "{} · {} · {} · {}",
                        c.short,
                        c.author,
                        c.date.get(..10).unwrap_or(&c.date),
                        c.subject
                    ),
                    None => hash,
                };
                out.push(Effect::Notify(text, Level::Info));
            }
            Some(Target::Branch(name)) => {
                let current = repo.status.branch.as_deref() == Some(name.as_str());
                if current {
                    out.push(Effect::Notify(format!("{name} is the branch you are on"), Level::Info));
                    return;
                }
                if let Some(s) = self.repos.get_mut(root) {
                    s.keep = Some(keep_at(content, head, false));
                }
                self.run_plan(
                    out,
                    root,
                    vec![cmd::switch(&name)],
                    format!("Switched to {name}"),
                );
            }
        }
    }

    fn open_file(&mut self, out: &mut Vec<Effect>, root: &str, path: &str, line: Option<u32>) {
        let full = format!("{}/{}", root.trim_end_matches('/'), path);
        let args = match line {
            Some(l) => format!("{{\"path\":{},\"line\":{l}}}", serde_string(&full)),
            None => json_object(&[("path", &full)]),
        };
        out.push(Effect::RunCommand {
            id: "file.open".into(),
            args,
        });
    }

    /// Alt+Enter and `?`: what can be done with the thing under the
    /// cursor, then every command, each with its key.
    fn menu(
        &mut self,
        out: &mut Vec<Effect>,
        root: &str,
        content: &Content,
        head: usize,
        doc: &Doc,
    ) {
        let repo = self.repos.get(root).and_then(|s| s.repo.clone());
        let here = target::at(content, head);
        let mut entries: Vec<(&'static str, String)> = Vec::new();
        let mut add = |id: &'static str, label: String| entries.push((id, label));
        match &here {
            Some(Target::File { section, path }) => {
                add("git.visit", format!("Open {path}"));
                if *section == Section::Staged {
                    add("git.unstage", format!("Unstage {path}"));
                } else {
                    add("git.stage", format!("Stage {path}"));
                }
                add("git.discard", format!("Discard the changes of {path}…"));
                if *section != Section::Untracked {
                    add("git.deleteFile", format!("Delete {path}…"));
                }
                add("git.toggle", "Show or hide its diff".into());
            }
            Some(Target::Hunk { section, .. }) => {
                add("git.visit", "Open the file at this line".into());
                if *section == Section::Staged {
                    add(
                        "git.unstage",
                        "Unstage the hunk (or the lines selected)".into(),
                    );
                } else {
                    add("git.stage", "Stage the hunk (or the lines selected)".into());
                }
                add(
                    "git.discard",
                    "Discard the hunk (or the lines selected)…".into(),
                );
                add("git.toggle", "Fold the hunk".into());
            }
            Some(Target::Section(s)) => {
                match s {
                    Section::Untracked => add("git.stage", "Stage every untracked file…".into()),
                    Section::Unstaged => add("git.stage", "Stage every change".into()),
                    Section::Staged => add("git.unstage", "Unstage everything".into()),
                    _ => {}
                }
                add("git.toggle", "Fold or unfold the section".into());
            }
            Some(Target::Branch(b)) => add("git.visit", format!("Switch to {b}")),
            Some(Target::Stash(n)) => add("git.visit", format!("Apply, pop or drop stash@{{{n}}}")),
            _ => {}
        }
        let general: &[&'static str] = &[
            "git.commit",
            "git.amend",
            "git.extend",
            "git.reword",
            "git.push",
            "git.pull",
            "git.fetch",
            "git.switchBranch",
            "git.newBranch",
            "git.stash",
            "git.stageAll",
            "git.unstageAll",
            "git.cycle",
            "git.refresh",
            "git.close",
        ];
        let staged = repo
            .as_ref()
            .is_some_and(|r| !r.entries(Section::Staged).is_empty());
        for id in general {
            // A commit offered first when something is staged.
            if *id == "git.commit" && staged {
                entries.insert(0, (id, "Commit what is staged".into()));
                continue;
            }
            let c = command_info(id).expect("a command of the plugin");
            entries.push((id, c.title.trim_start_matches("Git: ").to_string()));
        }
        let items = entries
            .iter()
            .map(|(id, label)| (label.clone(), menu_key(id)))
            .collect();
        let ids = entries.into_iter().map(|(id, _)| id).collect();
        let token = self.ask(Question::Commands {
            ids,
            doc: Some(doc.clone()),
        });
        out.push(Effect::Pick {
            token,
            title: "Git".into(),
            items,
        });
    }
}

/// `s` as a JSON string.
fn serde_string(s: &str) -> String {
    let fields = json_object(&[("v", s)]);
    // `{"v":"…"}` → `"…"`.
    fields
        .strip_prefix("{\"v\":")
        .and_then(|f| f.strip_suffix('}'))
        .unwrap_or("\"\"")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Style;

    fn content(lines: &[(&str, &[&str])]) -> Content {
        let mut c = Content::new();
        for (key, text) in lines {
            c.open(*key, true, false);
            for t in *text {
                c.line(t, Style::Normal);
            }
            c.close();
        }
        c
    }

    #[test]
    fn the_cursor_follows_what_it_was_on() {
        let old = content(&[
            ("file:unstaged:a", &["a", "@@", "+x"]),
            ("file:unstaged:b", &["b"]),
            ("section:staged", &["Staged"]),
        ]);
        // On file a's third line: a is still there, so the same line of it.
        let keep = keep_at(&old, old.text.find("+x").unwrap(), true);
        let new = content(&[
            ("section:x", &["new"]),
            ("file:unstaged:a", &["a", "@@", "+x"]),
        ]);
        assert_eq!(place(Some(&old), &new, &keep), new.text.find("+x").unwrap());
        // a staged and gone: the next file.
        let keep = keep_at(&old, 0, true);
        let new = content(&[("file:unstaged:b", &["b"]), ("section:staged", &["Staged"])]);
        assert_eq!(place(Some(&old), &new, &keep), 0);
        let new = content(&[("section:staged", &["Staged", "a"])]);
        assert_eq!(place(Some(&old), &new, &keep), 0);
        // Nothing of it: its line, clamped.
        let new = content(&[("x", &["one"])]);
        assert_eq!(place(Some(&old), &new, &keep_at(&old, 6, true)), 0);
    }

    #[test]
    fn keys_are_shown_as_people_write_them() {
        assert_eq!(key_label("shift+p p"), "P p");
        assert_eq!(key_label("alt+enter"), "Alt+Enter");
        assert_eq!(key_label("space g shift+s"), "SPC g S");
        assert_eq!(menu_key("git.stage").as_deref(), Some("s"));
        assert_eq!(menu_key("git.status").as_deref(), Some("SPC g g"));
        assert_eq!(title("/home/a/org/"), "Git: org");
        assert_eq!(serde_string("a\"b"), "\"a\\\"b\"");
    }
}
