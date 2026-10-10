//! Editing as an outliner (GR8): the commands that change the note at the
//! cursor (`crate::edit`), a block's `id::` written when it is first
//! referenced, and a page renamed with every reference to it (GR8c). A
//! file that is not the current document is written only when Kalem does
//! not hold it with unsaved changes.

use super::{App, Ctx, Effect, Level, Pending};
use crate::config::Kind;
use crate::date::Date;
use crate::edit::{self, Change, Edit};
use crate::files::{self, Files};
use crate::scan::{self, Flavor, RefKind};

/// A block offered to refer to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    /// Its file, relative to the root.
    rel: String,
    /// Its first line.
    line: u32,
    /// Its text, to find it again.
    text: String,
    /// Its id, when it has one: Logseq's UUID, Obsidian's `^id`.
    id: Option<String>,
}

/// The starts of `text`'s lines.
fn starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            text.bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        )
        .collect()
}

impl App {
    /// A document opened in Kalem, by its number.
    pub fn track_open(&mut self, number: u64, path: &str) {
        self.open.insert(number, files::normalize(path));
    }

    /// A document's text changed: unsaved until it is saved.
    pub fn track_changed(&mut self, number: u64) {
        if let Some(p) = self.open.get(&number) {
            self.dirty.insert(p.clone());
        }
    }

    /// The note a command runs in, with changes not saved: its text as the
    /// editor holds it goes into the index first, so that the command
    /// sees what the user typed.
    pub(super) fn take_unsaved(&mut self, ctx: &Ctx) {
        let (None, Some(path), Some(text)) = (&ctx.doc, &ctx.path, &ctx.text) else {
            return;
        };
        let path = files::normalize(path);
        if !self.dirty.contains(&path) {
            return;
        }
        let Some(root) = self
            .indexes
            .keys()
            .filter(|r| files::relative(r, &path).is_some())
            .max_by_key(|r| r.len())
            .cloned()
        else {
            return;
        };
        let rel = files::relative(&root, &path)
            .unwrap_or_default()
            .to_string();
        if let Some(index) = self.indexes.get_mut(&root) {
            index.update_unsaved(&rel, text);
        }
    }

    /// A document closed: one closed with changes not saved is read again
    /// from its file, the index having taken its unsaved text.
    pub fn closed_document(&mut self, files: &dyn Files, number: u64) -> Vec<Effect> {
        let Some(path) = self.open.get(&number).cloned() else {
            return Vec::new();
        };
        let was_unsaved = self.dirty.contains(&path);
        self.track_closed(number);
        if !was_unsaved || self.dirty.contains(&path) {
            return Vec::new();
        }
        let roots: Vec<String> = self
            .indexes
            .keys()
            .filter(|r| files::relative(r, &path).is_some())
            .cloned()
            .collect();
        let mut out = Vec::new();
        for root in roots {
            if self
                .indexes
                .get_mut(&root)
                .is_some_and(|i| i.update(files, &path))
            {
                out.extend(self.refresh(&root));
            }
        }
        out
    }

    /// A document closed.
    pub fn track_closed(&mut self, number: u64) {
        if let Some(p) = self.open.remove(&number)
            && !self.open.values().any(|o| *o == p)
        {
            self.dirty.remove(&p);
        }
    }

    /// A file saved: no longer unsaved.
    pub(super) fn track_saved(&mut self, path: &str) {
        self.dirty.remove(path);
    }

    /// How the file `rel` of graph `root` is read.
    pub(super) fn flavor_in(&self, root: &str, rel: &str) -> Flavor {
        let kind = self
            .indexes
            .get(root)
            .map_or(Kind::Logseq, |i| i.graph.kind);
        match (kind, files::stem_ext(rel).1.as_str()) {
            (Kind::Obsidian, _) => Flavor::Obsidian,
            (_, "org") => Flavor::LogseqOrg,
            _ => Flavor::LogseqMarkdown,
        }
    }

    /// A new block ID: a UUID of version 4, from the clock's random bits
    /// when Kalem gives them, else from `seed` and a counter.
    fn new_id(&mut self, seed: &str) -> String {
        self.made += 1;
        let bits = |salt: u64| -> u64 {
            if let Some(r) = self.random {
                return r();
            }
            // FNV-1a of the seed, the counter and the salt.
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in seed
                .bytes()
                .chain(self.made.to_le_bytes())
                .chain(salt.to_le_bytes())
            {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            h
        };
        let (a, b) = (bits(1), bits(2));
        let hex = format!("{a:016x}{b:016x}");
        format!(
            "{}-{}-4{}-{:x}{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[13..16],
            8 + (b >> 62) as u8 % 4,
            &hex[17..20],
            &hex[20..32]
        )
    }

    /// Insert Block Reference: every block of the graph with text, those
    /// with an id first.
    pub(super) fn block_ref_pick(&mut self, root: &str, from: Option<String>) -> Vec<Effect> {
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        let mut choices: Vec<(bool, String, Choice)> = Vec::new();
        for (rel, d) in index.files() {
            let title = index.page_of(rel).map_or(rel.clone(), |p| p.title.clone());
            for b in &d.scanned.blocks {
                if b.text.trim().is_empty() || b.props.iter().any(|(k, _)| k == "template") {
                    continue;
                }
                choices.push((
                    b.id.is_none(),
                    title.clone(),
                    Choice {
                        rel: rel.clone(),
                        line: b.line,
                        text: b.text.clone(),
                        id: b.id.clone(),
                    },
                ));
            }
        }
        if choices.is_empty() {
            return vec![Effect::Notify(
                "The graph has no block yet".into(),
                Level::Info,
            )];
        }
        choices.sort_by(|a, b| {
            (a.0, a.1.to_lowercase(), a.2.line).cmp(&(b.0, b.1.to_lowercase(), b.2.line))
        });
        let items = choices
            .iter()
            .map(|(_, title, c)| (c.text.clone(), Some(title.clone())))
            .collect();
        let choices = choices.into_iter().map(|(.., c)| c).collect();
        let t = self.token(Pending::InsertBlockRef {
            root: root.to_string(),
            choices,
            from,
        });
        vec![Effect::Pick {
            token: t,
            title: "Insert a reference to the block".into(),
            items,
        }]
    }

    /// The block chosen: its reference inserted, its id written first when
    /// it has none (in its file, or in the current document by the editor).
    pub(super) fn block_ref_answer(
        &mut self,
        files: &dyn Files,
        root: &str,
        choice: Choice,
        from: Option<String>,
    ) -> Vec<Effect> {
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        let kind = index.graph.kind;
        let page = index
            .page_of(&choice.rel)
            .map(|p| (p.key.clone(), p.path.clone()));
        let insert = |text: String| Effect::Run {
            id: "graph.insertText".into(),
            args: serde_json::json!({ "text": text }).to_string(),
        };
        if let Some(id) = &choice.id {
            return vec![insert(match kind {
                Kind::Logseq => format!("(({id}))"),
                Kind::Obsidian => {
                    let name = page
                        .and_then(|(_, p)| p)
                        .map_or(String::new(), |p| files::stem_ext(&p).0.to_string());
                    format!("[[{name}#^{id}]]")
                }
            })];
        }
        if kind == Kind::Obsidian {
            return vec![Effect::Notify(
                "Only blocks with a ^id can be referred to in a vault yet".into(),
                Level::Info,
            )];
        }
        let id = self.new_id(&format!("{}:{}:{}", choice.rel, choice.line, choice.text));
        let path = self.indexes[root].graph.path(&choice.rel);
        // In the document the command came from: the editor writes both.
        if from.as_deref() == Some(choice.rel.as_str()) {
            return vec![Effect::Run {
                id: "graph.refBlockNow".into(),
                args: serde_json::json!({ "line": choice.line, "text": choice.text, "id": id })
                    .to_string(),
            }];
        }
        if self.dirty.contains(&path) {
            return vec![Effect::Notify(
                format!(
                    "{} has unsaved changes: save it, then insert the reference",
                    files::file_name(&path)
                ),
                Level::Warning,
            )];
        }
        let Ok(text) = files.read(&path) else {
            return vec![Effect::Notify(
                format!("{path} cannot be read"),
                Level::Error,
            )];
        };
        let Some(e) = find_block(&text, choice.line, &choice.text)
            .and_then(|line| edit::add_property(&text, line, "id", &id))
        else {
            return vec![Effect::Notify(
                format!(
                    "The block is no longer in {}: index again",
                    files::file_name(&path)
                ),
                Level::Warning,
            )];
        };
        if let Err(e) = files.write(&path, &edit::apply(&text, &[e])) {
            return vec![Effect::Notify(e, Level::Error)];
        }
        let mut out = Vec::new();
        if let Some(i) = self.indexes.get_mut(root) {
            i.update(files, &path);
        }
        out.extend(self.refresh(root));
        out.push(insert(format!("(({id}))")));
        out
    }

    /// The commands of the note at the cursor.
    pub(super) fn outline_command(
        &mut self,
        files: &dyn Files,
        id: &str,
        root: &str,
        rel: Option<&str>,
        ctx: &Ctx,
    ) -> Vec<Effect> {
        let args: serde_json::Value =
            serde_json::from_str(&ctx.args).unwrap_or(serde_json::Value::Null);
        // The answers' commands that name their graph.
        if id == "graph.renameNow" {
            let root = args["root"].as_str().unwrap_or(root).to_string();
            let key = args["key"].as_str().unwrap_or_default().to_string();
            let title = args["title"].as_str().unwrap_or_default().to_string();
            return self.rename(files, &root, &key, &title, ctx);
        }
        let (Some(rel), Some(text)) = (rel, ctx.text.as_deref()) else {
            return vec![Effect::Notify(
                "This document is no note of a graph".into(),
                Level::Info,
            )];
        };
        let flavor = self.flavor_in(root, rel);
        let cycle = self
            .indexes
            .get(root)
            .map_or(["LATER", "NOW", "DONE"], |i| i.graph.todo_cycle());
        let cursor = ctx.cursor.min(text.len());
        let done = |change: Option<Change>, label: &str, why: &str| match change {
            Some(c) => vec![Effect::Edits {
                edits: c.edits,
                label: label.to_string(),
                line: c.line,
            }],
            None => vec![Effect::Notify(why.to_string(), Level::Info)],
        };
        match id {
            "graph.cycleTodo" => done(
                edit::cycle_todo(text, flavor, cycle, cursor),
                "Cycle Task",
                "No block at the cursor",
            ),
            "graph.setPriority" => {
                if flavor == Flavor::Obsidian {
                    return vec![Effect::Notify(
                        "A vault's notes have no priorities".into(),
                        Level::Info,
                    )];
                }
                let t = self.token(Pending::Priority);
                vec![Effect::Pick {
                    token: t,
                    title: "Priority".into(),
                    items: vec![
                        ("A".into(), Some("[#A]".into())),
                        ("B".into(), Some("[#B]".into())),
                        ("C".into(), Some("[#C]".into())),
                        ("None".into(), None),
                    ],
                }]
            }
            "graph.setPriorityNow" => {
                let p = args["priority"].as_str().and_then(|p| p.chars().next());
                done(
                    edit::set_priority(text, flavor, cursor, p),
                    "Set Priority",
                    "No block at the cursor",
                )
            }
            "graph.schedule" | "graph.deadline" => {
                if flavor == Flavor::Obsidian {
                    return vec![Effect::Notify(
                        "A vault's tasks have no SCHEDULED or DEADLINE".into(),
                        Level::Info,
                    )];
                }
                let key = if id == "graph.schedule" {
                    "SCHEDULED"
                } else {
                    "DEADLINE"
                };
                let t = self.token(Pending::Date { key });
                vec![Effect::Prompt {
                    token: t,
                    title: format!(
                        "{} (empty or - removes it)",
                        if key == "SCHEDULED" {
                            "Scheduled for"
                        } else {
                            "Due on"
                        }
                    ),
                    value: None,
                    placeholder: Some("2026-10-12, oct 12, +3, friday".into()),
                }]
            }
            "graph.setDateNow" => {
                let key = args["key"].as_str().unwrap_or("SCHEDULED");
                let date = args["date"].as_str().and_then(Date::parse_iso);
                let label = if key == "SCHEDULED" {
                    "Schedule"
                } else {
                    "Deadline"
                };
                done(
                    edit::set_date(text, flavor, cursor, key, date),
                    label,
                    "No block at the cursor",
                )
            }
            "graph.moveBlockUp" | "graph.moveBlockDown" => done(
                edit::move_block(text, flavor, cursor, id == "graph.moveBlockUp"),
                "Move Block",
                "No block of the same level there",
            ),
            "graph.indent" => done(
                edit::shift_block(text, flavor, cursor, true),
                "Indent Block",
                "The block has no block above it to go under",
            ),
            "graph.outdent" => done(
                edit::shift_block(text, flavor, cursor, false),
                "Outdent Block",
                "The block is at the top level",
            ),
            "graph.newBlock" => match edit::new_block(text, flavor, cursor)
                .filter(|_| !self.indexes.get(root).is_some_and(|i| i.graph.read_only))
            {
                Some(c) => vec![Effect::Edits {
                    edits: c.edits,
                    label: "New Block".into(),
                    line: None,
                }],
                // Not on a block's first line, or in a mirror Logseq writes:
                // Enter as Markdown has it, a list's next item or a new line.
                None => {
                    let start = text[..cursor].rfind('\n').map_or(0, |p| p + 1);
                    let line = text[start..].trim_start_matches([' ', '\t']);
                    let item = line.starts_with("- ") || line.starts_with("* ") || line == "-";
                    vec![Effect::Run {
                        id: if item {
                            "markdown.newline"
                        } else {
                            "edit.newline"
                        }
                        .into(),
                        args: "null".into(),
                    }]
                }
            },
            "graph.toggleFold" => {
                if flavor != Flavor::LogseqMarkdown {
                    return vec![Effect::Run {
                        id: "view.fold".into(),
                        args: "null".into(),
                    }];
                }
                match edit::toggle_fold(text, cursor) {
                    Ok(c) => done(Some(c), "Fold Block", ""),
                    Err(why) => vec![Effect::Notify(why.to_string(), Level::Info)],
                }
            }
            "graph.refBlockNow" => {
                let line = args["line"].as_u64().unwrap_or(0) as u32;
                let block = args["text"].as_str().unwrap_or_default();
                let new = args["id"].as_str().unwrap_or_default();
                match find_block(text, line, block)
                    .and_then(|l| edit::add_property(text, l, "id", new))
                {
                    Some(e) => {
                        let at = Edit {
                            start: cursor,
                            end: cursor,
                            text: format!("(({new}))"),
                        };
                        // The id line, then the reference, in the order of
                        // the text.
                        let mut edits = vec![e, at];
                        edits.sort_by_key(|e| e.start);
                        vec![Effect::Edits {
                            edits,
                            label: "Insert Block Reference".into(),
                            line: None,
                        }]
                    }
                    None => vec![Effect::Notify(
                        "The block is no longer there".into(),
                        Level::Warning,
                    )],
                }
            }
            "graph.renamePage" => {
                let Some(index) = self.indexes.get(root) else {
                    return Vec::new();
                };
                if index.graph.kind == Kind::Obsidian {
                    return vec![Effect::Notify(
                        "A note of a vault is renamed with its file, which Kalem gives plugins no way to do yet".into(),
                        Level::Info,
                    )];
                }
                let Some(page) = index.page_of(rel).cloned() else {
                    return Vec::new();
                };
                if page.journal.is_some() {
                    return vec![Effect::Notify(
                        "A journal is named by its date".into(),
                        Level::Info,
                    )];
                }
                let t = self.token(Pending::Rename {
                    root: root.to_string(),
                    key: page.key.clone(),
                });
                vec![Effect::Prompt {
                    token: t,
                    title: format!("Rename {} to", page.title),
                    value: Some(page.title),
                    placeholder: None,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// Page `key` of graph `root` renamed `title`: its `title::` set, and
    /// every reference to it by its title (not by an alias) written with
    /// the new one, in every file of the graph. A file Kalem holds with
    /// unsaved changes stops it all, before anything is written.
    fn rename(
        &mut self,
        files: &dyn Files,
        root: &str,
        key: &str,
        title: &str,
        ctx: &Ctx,
    ) -> Vec<Effect> {
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        let Some(page) = index.page(key).cloned() else {
            return vec![Effect::Notify("No such page".into(), Level::Warning)];
        };
        let new_key = crate::names::key(title);
        if new_key != page.key && index.page(&new_key).is_some_and(|p| p.path.is_some()) {
            return vec![Effect::Notify(
                format!("A page called {title} exists: Kalem does not merge pages"),
                Level::Warning,
            )];
        }
        let current = ctx
            .path
            .as_deref()
            .and_then(|p| files::relative(root, p))
            .map(str::to_string);
        // The files to change: the page's own and those linking to it.
        let mut rels: Vec<String> = index
            .backlinks(key)
            .iter()
            .map(|b| b.path.to_string())
            .collect();
        rels.extend(page.path.clone());
        rels.sort();
        rels.dedup();
        let blocked: Vec<String> = rels
            .iter()
            .filter(|r| Some(r.as_str()) != current.as_deref())
            .filter(|r| self.dirty.contains(&index.graph.path(r)))
            .cloned()
            .collect();
        if !blocked.is_empty() {
            return vec![Effect::Notify(
                format!("Save first: {} (unsaved changes)", blocked.join(", ")),
                Level::Warning,
            )];
        }
        let comma = index.graph.comma_properties.clone();
        let old = page.title.clone();
        let mut out = Vec::new();
        let mut written = Vec::new();
        for rel in &rels {
            let flavor = self.flavor_in(root, rel);
            let own = page.path.as_deref() == Some(rel.as_str());
            if Some(rel.as_str()) == current.as_deref() {
                let Some(text) = ctx.text.as_deref() else {
                    continue;
                };
                let edits = rename_edits(text, flavor, &comma, &old, title, own);
                if !edits.is_empty() {
                    out.push(Effect::Edits {
                        edits,
                        label: "Rename Page".into(),
                        line: None,
                    });
                }
                continue;
            }
            let path = self.indexes[root].graph.path(rel);
            let Ok(text) = files.read(&path) else {
                continue;
            };
            let edits = rename_edits(&text, flavor, &comma, &old, title, own);
            if edits.is_empty() {
                continue;
            }
            match files.write(&path, &edit::apply(&text, &edits)) {
                Ok(()) => written.push(path),
                Err(e) => out.push(Effect::Notify(e, Level::Error)),
            }
        }
        for p in &written {
            if let Some(i) = self.indexes.get_mut(root) {
                i.update(files, p);
            }
        }
        out.push(Effect::Notify(
            format!("{old} is now {title}: {} files written", written.len()),
            Level::Info,
        ));
        out.extend(self.refresh(root));
        out
    }
}

/// The first line of the block that starts at `line` with `text`, or,
/// the file having changed, the line of the block with that text nearest
/// to it.
pub(super) fn find_block(note: &str, line: u32, text: &str) -> Option<u32> {
    let s = scan::scan(note, Flavor::LogseqMarkdown, &scan::Options::default());
    if s.blocks.iter().any(|b| b.line == line && b.text == text) {
        return Some(line);
    }
    s.blocks
        .iter()
        .filter(|b| b.text == text)
        .min_by_key(|b| b.line.abs_diff(line))
        .map(|b| b.line)
}

/// The edits renaming the page `old` to `new` in `text`: references to it
/// by that name, and in its own file (`own`) its `title::`.
fn rename_edits(
    text: &str,
    flavor: Flavor,
    comma: &[String],
    old: &str,
    new: &str,
    own: bool,
) -> Vec<Edit> {
    let st = starts(text);
    let s = scan::scan(
        text,
        flavor,
        &scan::Options {
            comma_properties: comma.to_vec(),
        },
    );
    let same = |t: &str| t.trim().to_lowercase() == old.trim().to_lowercase();
    let spaced = new.contains(char::is_whitespace);
    let mut edits = Vec::new();
    let mut property_lines = std::collections::BTreeSet::new();
    for r in &s.refs {
        if !matches!(
            r.kind,
            RefKind::Page | RefKind::Tag | RefKind::EmbedPage | RefKind::Property
        ) || !same(&r.target)
        {
            continue;
        }
        if r.start == r.end {
            // A property's value: the line rewritten below.
            property_lines.insert(r.line);
            continue;
        }
        let ls = st[r.line as usize];
        let (a, b) = (ls + r.start as usize, ls + r.end as usize);
        let span = &text[a..b];
        let Some(p) = span.to_lowercase().find(&old.trim().to_lowercase()) else {
            continue;
        };
        let (s0, s1) = (a + p, a + p + old.trim().len());
        if r.kind == RefKind::Tag && span.starts_with('#') && !span.starts_with("#[[") && spaced {
            edits.push(Edit {
                start: a,
                end: b,
                text: format!("#[[{new}]]"),
            });
        } else {
            edits.push(Edit {
                start: s0,
                end: s1,
                text: new.to_string(),
            });
        }
    }
    for line in property_lines {
        let ls = st[line as usize];
        let le = st.get(line as usize + 1).copied().unwrap_or(text.len());
        let l = &text[ls..le];
        let Some(p) = l.find("::") else { continue };
        let value_start = ls + p + 2;
        let value = &text[value_start..le];
        let body = value.trim_end_matches(['\n', '\r']);
        let items: Vec<String> = body
            .split(',')
            .map(|item| {
                let t = item.trim();
                let bare = t
                    .trim_start_matches('#')
                    .trim_start_matches("[[")
                    .trim_end_matches("]]");
                if same(bare) {
                    let lead = &item[..item.len() - item.trim_start().len()];
                    let shown = if t.starts_with("[[") || spaced {
                        format!("[[{new}]]")
                    } else if t.starts_with('#') {
                        format!("#{new}")
                    } else {
                        new.to_string()
                    };
                    format!("{lead}{shown}")
                } else {
                    item.to_string()
                }
            })
            .collect();
        let joined = items.join(",");
        if joined != body {
            edits.push(Edit {
                start: value_start,
                end: value_start + body.len(),
                text: joined,
            });
        }
    }
    if own {
        // `title::` (`#+title:` in Org) set: replaced, or written first.
        let key = if flavor == Flavor::LogseqOrg {
            "#+title:"
        } else {
            "title::"
        };
        let found = text.split_inclusive('\n').scan(0, |at, l| {
            let start = *at;
            *at += l.len();
            Some((start, l))
        });
        let mut done = false;
        for (start, l) in found {
            let t = l.trim_start();
            if t.to_lowercase().starts_with(key) {
                let value_start = start + (l.len() - t.len()) + key.len();
                let end = start + l.trim_end_matches(['\n', '\r']).len();
                edits.push(Edit {
                    start: value_start,
                    end,
                    text: format!(" {new}"),
                });
                done = true;
                break;
            }
            if !t.is_empty() && !t.contains("::") && !t.starts_with("#+") {
                break;
            }
        }
        if !done {
            edits.push(Edit {
                start: 0,
                end: 0,
                text: format!("{key} {new}\n"),
            });
        }
    }
    edits.sort_by_key(|e| e.start);
    edits.dedup_by_key(|e| e.start);
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_renamed() {
        let t = "alias:: Short\ntags:: [[Old Name]], other\n\n- see [[Old Name]] and #[[old name]]\n- {{embed [[Old Name]]}} and [[Short]]\n";
        let e = rename_edits(
            t,
            Flavor::LogseqMarkdown,
            &["alias".into(), "tags".into()],
            "Old Name",
            "New Name",
            false,
        );
        assert_eq!(
            edit::apply(t, &e),
            "alias:: Short\ntags:: [[New Name]], other\n\n- see [[New Name]] and #[[New Name]]\n- {{embed [[New Name]]}} and [[Short]]\n"
        );
        // A tag of one word given a name with a space.
        let t = "- #old and #older\n";
        let e = rename_edits(t, Flavor::LogseqMarkdown, &[], "old", "two words", false);
        assert_eq!(edit::apply(t, &e), "- #[[two words]] and #older\n");
        // The page's own title.
        let t = "alias:: X\n\n- body\n";
        let e = rename_edits(t, Flavor::LogseqMarkdown, &[], "Old", "New", true);
        assert_eq!(edit::apply(t, &e), "title:: New\nalias:: X\n\n- body\n");
        let t = "title:: Old\n- body\n";
        let e = rename_edits(t, Flavor::LogseqMarkdown, &[], "Old", "New", true);
        assert_eq!(edit::apply(t, &e), "title:: New\n- body\n");
        let o = "#+title: Old\n* body [[Old]]\n";
        let e = rename_edits(o, Flavor::LogseqOrg, &[], "Old", "New", true);
        assert_eq!(edit::apply(o, &e), "#+title: New\n* body [[New]]\n");
    }

    #[test]
    fn blocks_found_again() {
        let t = "- a\n- b\n- c\n";
        assert_eq!(find_block(t, 1, "b"), Some(1));
        assert_eq!(
            find_block("- x\n- a\n- b\n", 1, "b"),
            Some(2),
            "moved down a line"
        );
        assert_eq!(find_block(t, 1, "z"), None);
    }
}
