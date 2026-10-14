//! Logseq's flashcards (GR17): Review Flashcards opens the cards of the
//! graph due now, one at a time, as Logseq's review does; `a` shows the
//! answer, `f`, `t` and `r` score the card (forgotten, remembered with
//! effort, remembered) and write its `card-*` properties into its file,
//! as GR8c writes a file the user is not editing; `n` passes over it.

use std::collections::BTreeSet;

use super::{App, Effect, Level};
use crate::config::Kind;
use crate::edit;
use crate::files::{self, Files};
use crate::srs;

/// A review under way in a graph.
#[derive(Debug, Default, Clone)]
pub(super) struct Review {
    /// The cards passed over (`n`), by file and line.
    pub skipped: BTreeSet<(String, u32)>,
    /// Whether the answer of the card shown is.
    pub shown: bool,
    /// How many cards were scored.
    pub reviewed: usize,
}

impl App {
    /// The cards of graph `root` and those of them due at `now`, but the
    /// ones passed over, in the order they are asked.
    pub(super) fn due_cards(&self, root: &str, now: i64) -> (Vec<srs::Card>, Vec<usize>) {
        let Some(index) = self.indexes.get(root) else {
            return (Vec::new(), Vec::new());
        };
        let all = srs::cards(index);
        let skipped = self.review.get(root).map(|r| &r.skipped);
        let due = all
            .iter()
            .enumerate()
            .filter(|(_, c)| srs::due(c, now))
            .filter(|(_, c)| !skipped.is_some_and(|s| s.contains(&(c.rel.clone(), c.line))))
            .map(|(i, _)| i)
            .collect();
        (all, due)
    }

    /// Milliseconds since 1970, when Kalem gives the plugin its clock.
    fn millis(&self) -> Option<i64> {
        self.now.map(|f| f())
    }

    /// Flashcard command `id` in graph `root`.
    pub(super) fn card_command(&mut self, files: &dyn Files, id: &str, root: &str) -> Vec<Effect> {
        if self
            .indexes
            .get(root)
            .is_some_and(|i| i.graph.kind == Kind::Obsidian)
        {
            return vec![Effect::Notify(
                "Flashcards are Logseq's: a vault has none".into(),
                Level::Info,
            )];
        }
        match id {
            "graph.flashcards" => {
                self.review.insert(root.to_string(), Review::default());
            }
            "graph.cardShow" => self.review.entry(root.to_string()).or_default().shown = true,
            "graph.cardSkip" => {
                let (all, due) = self.due_cards(root, self.millis().unwrap_or(0));
                let review = self.review.entry(root.to_string()).or_default();
                if let Some(c) = due.first().map(|i| &all[*i]) {
                    review.skipped.insert((c.rel.clone(), c.line));
                }
                review.shown = false;
            }
            _ => {
                let score = match id {
                    "graph.cardForgotten" => 1,
                    "graph.cardRecalled" => 3,
                    _ => 5,
                };
                return self.score_card(files, root, score);
            }
        }
        self.render("graph.cards", root, true).into_iter().collect()
    }

    /// The card asked in graph `root` scored `score`: its next review
    /// worked out as Logseq works it out and written into its block.
    fn score_card(&mut self, files: &dyn Files, root: &str, score: u8) -> Vec<Effect> {
        let notify = |text: String, level| vec![Effect::Notify(text, level)];
        let Some(now) = self.millis() else {
            return notify(
                "This Kalem gives plugins no clock: a review's next day cannot be worked out"
                    .into(),
                Level::Warning,
            );
        };
        let (all, due) = self.due_cards(root, now);
        let Some(card) = due.first().map(|i| &all[*i]) else {
            return notify("No card is due".into(), Level::Info);
        };
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        let path = index.graph.path(&card.rel);
        if self.dirty.contains(&path) {
            return notify(
                format!(
                    "{} has unsaved changes: save it, then score the card",
                    files::file_name(&path)
                ),
                Level::Warning,
            );
        }
        let Ok(note) = files.read(&path) else {
            return notify(format!("{path} cannot be read"), Level::Error);
        };
        let flavor = self.flavor_in(root, &card.rel);
        let Some(at) = super::outline::find_block_in(&note, card.line, &card.text, flavor) else {
            return notify(
                format!(
                    "The card is no longer in {}: index again",
                    files::file_name(&path)
                ),
                Level::Warning,
            );
        };
        // Logseq's matrix and settings, read each time: Logseq may have
        // changed them.
        let matrix = files
            .read(&index.graph.path("logseq/srs-of-matrix.edn"))
            .map(|t| srs::Matrix::read(&t))
            .unwrap_or_default();
        let settings = files
            .read(&index.graph.path("logseq/config.edn"))
            .map(|t| srs::settings(&t))
            .unwrap_or_default();
        let props = srs::review(card, score, now, &matrix, &settings);
        let Some(edits) = edit::set_properties(&note, flavor, at, &props) else {
            return notify(
                "The card's properties cannot be written".into(),
                Level::Warning,
            );
        };
        if let Err(e) = files.write(&path, &edit::apply(&note, &edits)) {
            return notify(e, Level::Error);
        }
        if let Some(i) = self.indexes.get_mut(root) {
            i.update(files, &path);
        }
        let review = self.review.entry(root.to_string()).or_default();
        review.reviewed += 1;
        review.shown = false;
        self.refresh(root)
    }
}
