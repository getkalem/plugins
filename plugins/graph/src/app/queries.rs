//! Queries and tasks (GR9): a `{{query …}}` followed to its document, an
//! advanced query explained, and `t` in the Tasks and Query documents
//! cycling the keyword of the line's task in its file, as GR8c writes a
//! file the user is not editing.

use super::{App, Ctx, Effect, Level, SEP};
use crate::content::Target;
use crate::edit;
use crate::files::{self, Files};
use crate::query::{self, Shape};
use crate::scan;

impl App {
    /// The `{{query …}}` of the line starting at byte `start` of `text`
    /// (the note `rel` of graph `root`), opened as its document: the one
    /// at the cursor, or with `anywhere` the line's first. An advanced
    /// query at the cursor is explained.
    pub(super) fn follow_query(
        &mut self,
        root: &str,
        rel: &str,
        text: &str,
        start: usize,
        cursor: usize,
        anywhere: bool,
    ) -> Option<Vec<Effect>> {
        let end = text[start..].find('\n').map_or(text.len(), |p| start + p);
        let line = &text[start..end];
        let column = cursor.saturating_sub(start);
        let macros = query::macros(line);
        let found = macros
            .iter()
            .find(|(s, e, _)| *s <= column && column < *e)
            .or_else(|| macros.first().filter(|_| anywhere));
        let Some((_, _, inner)) = found else {
            return if anywhere {
                self.advanced_query(root, rel, text, start)
            } else {
                None
            };
        };
        let index = self.indexes.get(root)?;
        // The block holding it says whether it is a table.
        let n = text[..start].bytes().filter(|b| *b == b'\n').count() as u32;
        let flavor = self.flavor_in(root, rel);
        let scanned = scan::scan(
            text,
            flavor,
            &scan::Options {
                comma_properties: index.graph.comma_properties.clone(),
            },
        );
        let shape = scanned
            .blocks
            .iter()
            .rev()
            .find(|b| b.line <= n && n < b.end.max(b.line + 1))
            .map(|b| Shape::of(&b.props))
            .unwrap_or_default();
        let key = format!("{root}{SEP}{inner}{SEP}{}", shape.encode());
        Some(self.render("graph.query", &key, true).into_iter().collect())
    }

    /// When the line at `start` is inside `#+BEGIN_QUERY` … `#+END_QUERY`
    /// of the note `rel` of graph `root`, the query's results (Logseq's
    /// advanced queries, `crate::datalog`).
    fn advanced_query(
        &mut self,
        root: &str,
        rel: &str,
        text: &str,
        start: usize,
    ) -> Option<Vec<Effect>> {
        // The last query opened up to the end of the cursor's line (which
        // may be its first line), searched in the text itself: a copy in
        // capitals is not as long where a letter's capital is not (`ı`).
        let end = text[start..].find('\n').map_or(text.len(), |p| start + p);
        let open = scan::rfind_ci(&text[..end], "#+BEGIN_QUERY")?.start;
        // Closed before the cursor's line: not the one the cursor is in.
        if open < start && scan::find_ci(&text[open..start], "#+END_QUERY").is_some() {
            return None;
        }
        let close = scan::find_ci(&text[open..], "#+END_QUERY").map(|q| open + q.end)?;
        let current = self
            .indexes
            .get(root)
            .and_then(|i| i.page_of(rel))
            .map(|p| p.key.clone())
            .unwrap_or_default();
        let key = format!("{root}{SEP}{current}{SEP}{}", &text[open..close]);
        Some(
            self.render("graph.advancedQuery", &key, true)
                .into_iter()
                .collect(),
        )
    }

    /// `t` in the Tasks or a Query document: the keyword of the line's
    /// block cycled in its file, which is written unless Kalem holds it
    /// with unsaved changes; the documents follow.
    pub(super) fn cycle_in_document(&mut self, files: &dyn Files, ctx: &Ctx) -> Vec<Effect> {
        let Some((id, key)) = &ctx.doc else {
            return Vec::new();
        };
        let Some(content) = self.docs.get(&(id.clone(), key.clone())) else {
            return Vec::new();
        };
        let n = match &ctx.text {
            Some(t) => t[..ctx.cursor.min(t.len())]
                .bytes()
                .filter(|b| *b == b'\n')
                .count(),
            None => content.line_of(ctx.cursor),
        };
        let none = || vec![Effect::Notify("No block on this line".into(), Level::Info)];
        let Some(Target::Block { path, line }) = content.target(n).cloned() else {
            return none();
        };
        let root = key.split(SEP).next().unwrap_or_default().to_string();
        let Some(index) = self.indexes.get(&root) else {
            return none();
        };
        if index.graph.read_only {
            return vec![Effect::Notify(super::READ_ONLY.into(), Level::Info)];
        }
        let Some(rel) = files::relative(&index.graph.root, &path).map(str::to_string) else {
            return none();
        };
        let Some(was) = index
            .file(&rel)
            .and_then(|d| d.scanned.blocks.iter().find(|b| b.line == line))
            .map(|b| b.text.clone())
        else {
            return none();
        };
        if self.dirty.contains(&path) {
            return vec![Effect::Notify(
                format!(
                    "{} has unsaved changes: save it, or change the task there",
                    files::file_name(&path)
                ),
                Level::Warning,
            )];
        }
        let Ok(note) = files.read(&path) else {
            return vec![Effect::Notify(
                format!("{path} cannot be read"),
                Level::Error,
            )];
        };
        let flavor = self.flavor_in(&root, &rel);
        let Some(at) = super::outline::find_block_in(&note, line, &was, flavor) else {
            return vec![Effect::Notify(
                format!(
                    "The block is no longer in {}: index again",
                    files::file_name(&path)
                ),
                Level::Warning,
            )];
        };
        let offset = note
            .split_inclusive('\n')
            .take(at as usize)
            .map(str::len)
            .sum::<usize>();
        let cycle = index.graph.todo_cycle();
        let clock = self.clock_now(&root);
        let now = self.local_now.map(|n| edit::Now::at(n()));
        let Some(change) =
            edit::cycle_task(&note, flavor, cycle, offset, clock.as_ref(), now.as_ref())
        else {
            return none();
        };
        if let Err(e) = files.write(&path, &edit::apply(&note, &change.edits)) {
            return vec![Effect::Notify(e, Level::Error)];
        }
        if let Some(i) = self.indexes.get_mut(&root) {
            i.update(files, &path);
        }
        self.refresh(&root)
    }
}
