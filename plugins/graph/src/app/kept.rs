//! What the plugin keeps between sessions (`graph_todo.md` K23, Kalem's
//! plugin API 0.2.17, `state`): the pages opened last, and each graph's
//! toggles of the graph view (`J`, `O`) and its sort of All pages, as the
//! last session left them.

use super::App;
use crate::index::Index;
use crate::views::PageSort;

/// A stable short name for graph `root` (FNV-1a), a key of the state
/// kept whatever its path's length.
fn graph_key(root: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in root.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("graph:{h:016x}")
}

impl App {
    /// The pages opened last, as the last session left them.
    pub fn restore(&mut self) {
        let Some(kept) = self.kept else {
            return;
        };
        if let Some(list) =
            kept("recent").and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
        {
            self.recent = list;
            self.recent.truncate(super::browse::RECENT);
        }
    }

    /// Graph `index`'s toggles and sort as the last session left them.
    pub(super) fn restore_graph(&mut self, index: &Index) {
        let Some(kept) = self.kept else {
            return;
        };
        let root = index.graph.root.clone();
        let Some(v) = kept(&graph_key(&root))
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .filter(|v| v["root"].as_str() == Some(root.as_str()))
        else {
            return;
        };
        let base = index.graph.graph_view;
        let mut gv = base;
        if let Some(j) = v["journals"].as_bool() {
            gv.journals = j;
        }
        if let Some(o) = v["orphans"].as_bool() {
            gv.orphans = o;
        }
        if gv != base {
            self.graph_view.insert(root.clone(), gv);
        }
        if let Some(s) = v["sort"].as_str().and_then(PageSort::parse) {
            self.page_sort.insert(root, s);
        }
    }

    /// The pages opened last, kept.
    pub(super) fn keep_recent(&self) {
        if let Some(keep) = self.keep {
            let list = serde_json::to_string(&self.recent).unwrap_or_default();
            let _ = keep("recent", Some(&list));
        }
    }

    /// Graph `root`'s toggles and sort, kept.
    pub(super) fn keep_graph(&self, root: &str) {
        let Some(keep) = self.keep else {
            return;
        };
        let gv = self.graph_view.get(root);
        let v = serde_json::json!({
            "root": root,
            "journals": gv.map(|g| g.journals),
            "orphans": gv.map(|g| g.orphans),
            "sort": self.page_sort.get(root).copied().unwrap_or_default().key(),
        });
        let _ = keep(&graph_key(root), Some(&v.to_string()));
    }
}
