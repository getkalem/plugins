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
            return self.advanced_query(text, start).filter(|_| anywhere);
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

    /// When the line at `start` is inside `#+BEGIN_QUERY` … `#+END_QUERY`,
    /// why nothing opens.
    fn advanced_query(&self, text: &str, start: usize) -> Option<Vec<Effect>> {
        let before = &text[..start];
        let end = text[start..].find('\n').map_or(text.len(), |p| start + p);
        let upto = &text[..end];
        let open = upto.rfind("#+BEGIN_QUERY")?;
        let closed = upto[open..].contains("#+END_QUERY") && before[open..].contains("#+END_QUERY");
        (!closed).then(|| {
            vec![Effect::Notify(
                "An advanced query (Datalog) is not run by Kalem: its simple queries, {{query …}}, are".into(),
                Level::Info,
            )]
        })
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
        let Some(at) = super::outline::find_block(&note, line, &was) else {
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
        let flavor = self.flavor_in(&root, &rel);
        let cycle = index.graph.todo_cycle();
        let Some(change) = edit::cycle_todo(&note, flavor, cycle, offset) else {
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
