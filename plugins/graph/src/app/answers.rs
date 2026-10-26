//! The answers of the queries notes show (`{{query …}}` and advanced
//! queries), worked out away from the drawing. Kalem asks a layer for its
//! overlays as it draws, and since its 0.6.48 such a call has 100 ms at
//! most whatever the manifest's limits give the plugin's commands, the
//! plugin stopped past them; a query over a large graph takes longer (an
//! advanced one some seconds as WebAssembly). So the layer shows the
//! answer kept for the index's version, else the one kept for an earlier
//! version, or a mark until there is one, and asks for it; the command
//! `graph.answerQueries`, run by the plugin at the editor's next tick,
//! works out what was asked and has the layers drawn again.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use super::{App, Effect, Shown};
use crate::index::Index;

/// The command that works out the answers asked for.
pub(super) const ANSWER: &str = "graph.answerQueries";

/// The most answers kept; past them, all are forgotten.
const KEPT: usize = 256;

/// After an answer, how many times as long as it took the plugin waits
/// before working the query out again while the graph keeps changing: a
/// note typed into under a slow query is not held up at each pause.
const SPACING: i64 = 4;

/// The longest such wait, in milliseconds.
const MOST_WAIT: i64 = 5_000;

/// A query a note shows.
#[derive(Debug, Clone)]
pub(super) enum Asked {
    /// `{{query q}}`.
    Simple(String),
    /// An advanced query, in the page `current` (its key).
    Advanced {
        query: String,
        current: Option<String>,
    },
}

impl Asked {
    /// Its key among the answers of its graph.
    fn key(&self) -> String {
        match self {
            Asked::Simple(q) => q.clone(),
            Asked::Advanced { query, current } => {
                format!("{}\u{1f}{query}", current.as_deref().unwrap_or(""))
            }
        }
    }
}

/// A query's answer as its note shows it.
#[derive(Debug)]
pub(super) struct Answer {
    /// The index's version it was worked out from.
    version: u64,
    shown: Shown,
    /// When it was worked out and how long that took, in milliseconds
    /// (both 0 without Kalem's clock).
    at: i64,
    took: i64,
}

/// The answers kept and those asked for, by graph and query.
#[derive(Debug, Default)]
pub(super) struct Answers {
    kept: RefCell<BTreeMap<(String, String), Answer>>,
    asked: RefCell<BTreeMap<(String, String), Asked>>,
    /// `graph.answerQueries` is queued and has not run yet.
    queued: Cell<bool>,
}

impl App {
    /// What `ask` shows in a note of the graph of `index`. Asked while
    /// Kalem draws, so it is worked out here only when Kalem does not run
    /// the plugin's commands later (an index built at once, as in the
    /// tests); else the answer kept for an earlier version of the index,
    /// or a mark, while the answer is asked for.
    pub(super) fn query_answer(&self, index: &Index, ask: Asked) -> Shown {
        let key = (index.graph.root.clone(), ask.key());
        let kept = self
            .answers
            .kept
            .borrow()
            .get(&key)
            .map(|a| (a.version == index.version(), a.shown.clone()));
        match kept {
            Some((true, shown)) => shown,
            _ if !self.sliced() => self.work_out(index, key, &ask),
            kept => {
                self.answers.asked.borrow_mut().insert(key, ask);
                kept.map_or_else(
                    || {
                        let dots = if self.settings.glyphs == "ascii" {
                            "..."
                        } else {
                            "…"
                        };
                        Shown::new((format!("{} {dots}", self.query_mark()), Vec::new()))
                    },
                    |(_, shown)| shown,
                )
            }
        }
    }

    /// When answers were asked for and `graph.answerQueries` is not queued
    /// for them yet, the milliseconds it waits: none for a query without
    /// an answer, else the least any of them still has to wait.
    pub fn answers_due(&self) -> Option<u32> {
        let asked = self.answers.asked.borrow();
        if asked.is_empty() || self.answers.queued.get() {
            return None;
        }
        let kept = self.answers.kept.borrow();
        let wait = asked
            .keys()
            .map(|k| kept.get(k).map_or(0, |a| self.wait(a)))
            .min()
            .unwrap_or(0);
        self.answers.queued.set(true);
        Some(wait as u32)
    }

    /// Works out the answers asked for whose wait is over
    /// (`graph.answerQueries`), the others asked for again; the layers
    /// drawn again when one was worked out.
    pub(super) fn answer_queries(&self) -> Vec<Effect> {
        self.answers.queued.set(false);
        let asked = std::mem::take(&mut *self.answers.asked.borrow_mut());
        let mut out = Vec::new();
        for (key, ask) in asked {
            let Some(index) = self.indexes.get(&key.0) else {
                continue;
            };
            let state = self
                .answers
                .kept
                .borrow()
                .get(&key)
                .map(|a| (a.version == index.version(), self.wait(a)));
            match state {
                Some((true, _)) => {}
                Some((false, wait)) if wait > 0 => {
                    self.answers.asked.borrow_mut().insert(key, ask);
                }
                _ => {
                    self.work_out(index, key, &ask);
                    if out.is_empty() {
                        out.push(Effect::RefreshLayers);
                    }
                }
            }
        }
        if let Some(after_ms) = self.answers_due() {
            out.push(Effect::Later {
                id: ANSWER.into(),
                args: "{}".into(),
                after_ms,
            });
        }
        out
    }

    /// The milliseconds still to wait, after answer `a`, before its query
    /// is worked out again.
    fn wait(&self, a: &Answer) -> i64 {
        let now = self.now.map_or(0, |n| n());
        (a.at + SPACING * a.took - now).clamp(0, MOST_WAIT)
    }

    /// Works out `ask`'s answer from `index`, kept under `key`.
    fn work_out(&self, index: &Index, key: (String, String), ask: &Asked) -> Shown {
        let mark = self.query_mark();
        let start = self.now.map_or(0, |n| n());
        let shown = Shown::new(match ask {
            Asked::Simple(q) => crate::query::shown(index, q, self.day(), mark),
            Asked::Advanced { query, current } => {
                crate::datalog::shown(index, query, self.day(), current.as_deref(), mark)
            }
        });
        let at = self.now.map_or(0, |n| n());
        let mut kept = self.answers.kept.borrow_mut();
        if kept.len() > KEPT {
            kept.clear();
        }
        kept.insert(
            key,
            Answer {
                version: index.version(),
                shown: shown.clone(),
                at,
                took: at - start,
            },
        );
        shown
    }

    /// The mark a query's line starts with.
    fn query_mark(&self) -> &'static str {
        if self.settings.glyphs == "ascii" {
            "query:"
        } else {
            "⌕"
        }
    }
}
