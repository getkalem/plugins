//! A graph indexed a slice at a time (`graph_todo.md` K13, Kalem's plugin
//! API 0.2.17, `later`): a large graph's first index, or another while the
//! one it has serves, leaves the editor answering between slices. Each
//! slice runs in `graph.indexing`, which asks for the next; the status bar
//! shows how far it is, and a command that needs the graph meanwhile
//! builds the rest at once.

use std::collections::BTreeSet;

use super::{App, Effect, Level};
use crate::config::{self, Graph, Kind};
use crate::files::{self, Files};
use crate::index::Building;

/// The command each slice runs in.
pub(super) const INDEXING: &str = "graph.indexing";

/// A graph being indexed.
#[derive(Debug)]
pub(super) struct Build {
    pub(super) building: Building,
    pub(super) kind: Kind,
    /// Its files changed on disk meanwhile, read again once it is built.
    pub(super) changed: BTreeSet<String>,
    /// Its counts said once it is built (Index Again).
    announce: bool,
    /// Its notes opened meanwhile, in order: Recent Pages once it is
    /// built.
    pub(super) opened: Vec<String>,
    /// How far it is, as the status bar last showed it.
    shown: u32,
}

impl App {
    /// Whether graphs are indexed a slice at a time: Kalem runs the
    /// plugin's commands later, and gives it a clock.
    pub(super) fn sliced(&self) -> bool {
        self.slice_ms.is_some() && self.now.is_some()
    }

    /// The root of the graph being indexed that `path` (absolute,
    /// normalized) is in.
    pub(super) fn building_root(&self, path: &str) -> Option<String> {
        self.building
            .keys()
            .filter(|r| files::relative(r, path).is_some())
            .max_by_key(|r| r.len())
            .cloned()
    }

    /// The graph `path` (absolute, normalized) is in, its first index
    /// started a slice at a time when graphs are, so none until it is
    /// built; else as [`App::graph_of`].
    pub(super) fn graph_soon(
        &mut self,
        files: &dyn Files,
        path: &str,
        out: &mut Vec<Effect>,
    ) -> Option<String> {
        if !self.sliced() || self.indexed_root(path).is_some() {
            return self.graph_of(files, path, out);
        }
        if self.building_root(path).is_none()
            && let Some((root, kind)) = self.find_graph(files, path)
        {
            self.start_index(files, &root, kind, false, out);
        }
        None
    }

    /// Graph `root` indexed again: a slice at a time when graphs are, the
    /// index it has serving meanwhile; else at once. Its counts said once
    /// it is built when `announce`.
    pub(super) fn index_again(
        &mut self,
        files: &dyn Files,
        root: &str,
        kind: Kind,
        announce: bool,
        out: &mut Vec<Effect>,
    ) {
        if self.sliced() {
            self.start_index(files, root, kind, announce, out);
            out.push(self.status());
            return;
        }
        let root = self.load(files, root, kind, out);
        if announce {
            out.push(self.counts_notice(&root));
        }
        out.extend(self.refresh(&root));
    }

    /// Starts indexing graph `root` a slice at a time, again if it is
    /// being indexed (its settings changed): the first slice asked for.
    fn start_index(
        &mut self,
        files: &dyn Files,
        root: &str,
        kind: Kind,
        announce: bool,
        out: &mut Vec<Effect>,
    ) {
        let building = Building::new(Graph::load(files, root, kind, &self.settings.fallbacks));
        let root = building.graph().root.clone();
        let announce = announce || self.building.get(&root).is_some_and(|b| b.announce);
        let opened = self
            .building
            .get(&root)
            .map(|b| b.opened.clone())
            .unwrap_or_default();
        let build = Build {
            building,
            kind,
            changed: BTreeSet::new(),
            announce,
            opened,
            shown: 0,
        };
        // A slice at a time: one started again goes on in the slice
        // already asked for.
        if self.building.insert(root.clone(), build).is_none() {
            out.push(next_slice(&root));
        }
    }

    /// A slice of a graph's index (`graph.indexing`, its root in `args`):
    /// the next asked for, or the index put in place once it is built.
    pub(super) fn index_slice(&mut self, files: &dyn Files, args: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        let root = serde_json::from_str::<serde_json::Value>(args)
            .ok()
            .and_then(|v| v["root"].as_str().map(str::to_string));
        let (Some(root), Some(now), Some(ms)) = (root, self.now, self.slice_ms) else {
            return out;
        };
        let Some(b) = self.building.get_mut(&root) else {
            return out;
        };
        let until = now() + i64::from(ms);
        if !b.building.step(files, &mut || now() < until) {
            out.push(next_slice(&root));
            // How far it is, on the status bar when that moved on.
            let p = b.building.percent();
            if p != b.shown {
                b.shown = p;
                out.push(self.status());
            }
            return out;
        }
        if let Some(b) = self.building.remove(&root) {
            self.finish_index(files, b, &mut out);
        }
        out
    }

    /// Graph `root`'s index, the rest built at once when it is being built
    /// a slice at a time: a command needs it now.
    pub(super) fn finish_now(&mut self, files: &dyn Files, root: &str, out: &mut Vec<Effect>) {
        // Built again when its settings changed meanwhile.
        while let Some(mut b) = self.building.remove(root) {
            while !b.building.step(files, &mut || true) {}
            self.finish_index(files, b, out);
        }
    }

    /// Index `b`, built, put in place: the files changed meanwhile read
    /// again, its graph's documents, panel and status written anew. Built
    /// again when the graph's settings changed meanwhile.
    fn finish_index(&mut self, files: &dyn Files, b: Build, out: &mut Vec<Effect>) {
        let root = b.building.graph().root.clone();
        if b.changed
            .iter()
            .filter_map(|p| files::relative(&root, p))
            .any(config::is_settings)
        {
            self.start_index(files, &root, b.kind, b.announce, out);
            return;
        }
        let mut index = b.building.into_index();
        if !b.changed.is_empty() {
            index.update_all(files, b.changed.iter().map(String::as_str));
        }
        let root = self.put_index(index, b.kind, out);
        if b.announce {
            out.push(self.counts_notice(&root));
        }
        for p in &b.opened {
            self.track_recent(p);
        }
        if self
            .current
            .as_deref()
            .is_some_and(|p| files::relative(&root, p).is_some())
        {
            self.last = Some(root.clone());
        }
        out.extend(self.refresh(&root));
    }

    /// Graph `root`'s counts, said when Index Again is done.
    fn counts_notice(&self, root: &str) -> Effect {
        let i = &self.indexes[root];
        let (pages, journals, blocks, refs) = i.counts();
        Effect::Notify(
            format!(
                "{}: {pages} pages, {journals} of them journals, {blocks} blocks, {refs} references",
                i.graph.name()
            ),
            Level::Info,
        )
    }
}

/// The next slice of graph `root`'s index asked for, at the editor's next
/// tick.
fn next_slice(root: &str) -> Effect {
    Effect::Later {
        id: INDEXING.into(),
        args: serde_json::json!({ "root": root }).to_string(),
        after_ms: 0,
    }
}
