//! Finding one's way in a graph (GR10): the graph's folder searched with
//! Kalem's search, its headings offered, the pages opened last, and the
//! whiteboards and canvases that their applications draw shown in the
//! file manager.

use super::{App, Ctx, Effect, Level, Pending};
use crate::config::Kind;
use crate::files;
use crate::index::{self, Index};

/// A list's entry: its label and second line.
type Item = (String, Option<String>);
/// A file, absolute, and a line from 1.
type Place = (String, u32);

/// The most headings Find Heading offers.
const HEADINGS: usize = 20_000;
/// The pages Recent Pages keeps.
const RECENT: usize = 50;

/// What a graph's search leaves out: the application's own folders, its
/// backups and trash, and the folders the graph hides.
pub(crate) fn search_ignore(index: &Index) -> Vec<String> {
    let mut out: Vec<String> = match index.graph.kind {
        Kind::Logseq => vec!["/logseq/".into()],
        Kind::Obsidian => vec!["/.obsidian/".into(), "/.trash/".into()],
    };
    out.extend(index.graph.hidden.iter().map(|h| format!("/{h}/")));
    out
}

impl App {
    /// A page opened: first among the recent ones.
    pub(super) fn track_recent(&mut self, path: &str) {
        let is_page = self
            .indexes
            .iter()
            .filter(|(r, _)| files::relative(r, path).is_some())
            .max_by_key(|(r, _)| r.len())
            .and_then(|(r, i)| i.page_of(files::relative(r, path)?))
            .is_some();
        if !is_page {
            return;
        }
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(RECENT);
    }

    /// The file `path` at `line` (from 1) opened, or, for a whiteboard or
    /// a canvas, shown in the file manager to be opened in its
    /// application.
    pub(super) fn open_or_reveal(&self, path: String, line: u32) -> Vec<Effect> {
        let drawn = self
            .indexes
            .iter()
            .filter(|(r, _)| files::relative(r, &path).is_some())
            .max_by_key(|(r, _)| r.len())
            .and_then(|(r, i)| {
                let rel = files::relative(r, &path)?;
                index::drawn_elsewhere(&i.graph, rel).then_some(i.graph.kind)
            });
        match drawn {
            Some(kind) => vec![Effect::Reveal {
                path,
                app: kind.title(),
            }],
            None => vec![Effect::Open { path, line }],
        }
    }

    /// The commands of GR10.
    pub(super) fn browse_command(&mut self, id: &str, root: &str, ctx: &Ctx) -> Vec<Effect> {
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        match id {
            "graph.search" => vec![Effect::Run {
                id: "search.folder".into(),
                args: serde_json::json!({
                    "path": index.graph.root,
                    "ignore": search_ignore(index),
                })
                .to_string(),
            }],
            "graph.findHeading" => {
                let (items, targets) = headings(index);
                let title = format!("A heading of {}", index.graph.name());
                if items.is_empty() {
                    return vec![Effect::Notify(
                        "The graph has no headings".into(),
                        Level::Info,
                    )];
                }
                let t = self.token(Pending::Places { targets });
                vec![Effect::Pick {
                    token: t,
                    title,
                    items,
                }]
            }
            "graph.recent" => {
                let current = ctx.path.as_deref().map(files::normalize);
                let mut items = Vec::new();
                let mut targets = Vec::new();
                for p in &self.recent {
                    if Some(p) == current.as_ref() {
                        continue;
                    }
                    let Some((r, i)) = self
                        .indexes
                        .iter()
                        .filter(|(r, _)| files::relative(r, p).is_some())
                        .max_by_key(|(r, _)| r.len())
                    else {
                        continue;
                    };
                    let Some(page) = files::relative(r, p).and_then(|rel| i.page_of(rel)) else {
                        continue;
                    };
                    let detail = if self.indexes.len() > 1 {
                        Some(i.graph.name().to_string())
                    } else {
                        page.path.clone()
                    };
                    items.push((page.title.clone(), detail));
                    targets.push((p.clone(), 1));
                }
                if items.is_empty() {
                    return vec![Effect::Notify(
                        "No other page of a graph opened yet".into(),
                        Level::Info,
                    )];
                }
                let t = self.token(Pending::Places { targets });
                vec![Effect::Pick {
                    token: t,
                    title: "A page opened last".into(),
                    items,
                }]
            }
            "graph.random" => {
                let mut pages: Vec<&String> =
                    index.pages().filter_map(|p| p.path.as_ref()).collect();
                if pages.is_empty() {
                    return vec![Effect::Notify("The graph has no page".into(), Level::Info)];
                }
                pages.sort();
                // The clock's random bits (API 0.2.10), else a counter
                // mixed with the index's version.
                self.made += 1;
                let bits = match self.random {
                    Some(r) => r(),
                    None => {
                        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                        for b in (self.made ^ index.version()).to_le_bytes() {
                            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
                        }
                        h
                    }
                };
                let rel = pages[(bits % pages.len() as u64) as usize].clone();
                let path = index.graph.path(&rel);
                self.open_or_reveal(path, 1)
            }
            _ => Vec::new(),
        }
    }

    /// The place picked from Find Heading or Recent Pages.
    pub(super) fn place_picked(
        &self,
        targets: &[(String, u32)],
        picked: Option<usize>,
    ) -> Vec<Effect> {
        match picked.and_then(|i| targets.get(i)) {
            Some((path, line)) => self.open_or_reveal(path.clone(), *line),
            None => Vec::new(),
        }
    }

    /// The references to the block at the cursor of the note `rel`, as a
    /// document; the reason there is none else.
    pub(super) fn block_references(
        &mut self,
        root: &str,
        rel: Option<&str>,
        ctx: &Ctx,
    ) -> Vec<Effect> {
        let say = |why: &str| vec![Effect::Notify(why.to_string(), Level::Info)];
        let (Some(rel), Some(text)) = (rel, ctx.text.as_deref()) else {
            return say("This document is no note of a graph");
        };
        let s = crate::scan::scan(
            text,
            self.flavor_in(root, rel),
            &crate::scan::Options::default(),
        );
        let line = text[..ctx.cursor.min(text.len())].matches('\n').count() as u32;
        let Some(b) = crate::edit::block_at(&s, line).map(|i| &s.blocks[i]) else {
            return say("No block here");
        };
        let index = &self.indexes[root];
        let id = match (&b.id, index.graph.kind) {
            (None, _) => None,
            (Some(id), Kind::Logseq) => Some(id.clone()),
            (Some(id), Kind::Obsidian) => index.page_of(rel).map(|p| format!("{}#^{id}", p.key)),
        };
        match id {
            Some(id) if !index.block_backlinks(&id).is_empty() => self
                .render(
                    "graph.blockReferences",
                    &format!("{root}{}{id}", super::SEP),
                    true,
                )
                .into_iter()
                .collect(),
            _ => say("Nothing refers to this block"),
        }
    }
}

/// The headings of the graph and the top-level blocks of its pages (not
/// its journals: their top-level blocks are the day's entries), with
/// their pages: the items and where each opens.
fn headings(index: &Index) -> (Vec<Item>, Vec<Place>) {
    let mut found: Vec<(String, String, String, u32)> = Vec::new();
    for (rel, d) in index.files() {
        let page = index.page_of(rel);
        let title = page.map_or_else(|| rel.clone(), |p| p.title.clone());
        let journal = page.is_some_and(|p| p.journal.is_some());
        let mut lines = std::collections::BTreeSet::new();
        for h in &d.scanned.headings {
            if !h.text.trim().is_empty() && lines.insert(h.line) {
                found.push((
                    h.text.trim().to_string(),
                    title.clone(),
                    rel.clone(),
                    h.line,
                ));
            }
        }
        if index.graph.kind == Kind::Logseq && !journal {
            for b in d.scanned.blocks.iter().filter(|b| b.depth == 0) {
                let text = b.text.lines().next().unwrap_or("").trim();
                // A code block is no title.
                if !text.is_empty() && !text.starts_with("```") && lines.insert(b.line) {
                    found.push((text.to_string(), title.clone(), rel.clone(), b.line));
                }
            }
        }
    }
    found.sort_by_key(|a| (a.1.to_lowercase(), a.3));
    found.truncate(HEADINGS);
    let targets = found
        .iter()
        .map(|(_, _, rel, line)| (index.graph.path(rel), line + 1))
        .collect();
    let items = found
        .into_iter()
        .map(|(t, page, _, _)| (t, Some(page)))
        .collect();
    (items, targets)
}
