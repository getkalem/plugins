//! Insert Template: one of the graph's templates, filled in, typed at the
//! cursor, as Obsidian's core Templates plugin inserts one (its Insert
//! template) and Logseq's `/Template` does, from the palette in a note of
//! either.

use super::{App, Effect, Level, Pending};
use crate::config::Kind;
use crate::files::Files;
use crate::index::Index;
use crate::template;

impl App {
    /// The graph `root`'s templates to pick from, for the note `from`
    /// whose cursor's line starts with `indent`.
    pub(super) fn template_pick(
        &mut self,
        root: &str,
        from: Option<String>,
        indent: String,
    ) -> Vec<Effect> {
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        let (labels, names): (Vec<String>, Vec<String>) = match index.graph.kind {
            Kind::Logseq => template::logseq_names(index)
                .into_iter()
                .map(|n| (n.clone(), n))
                .unzip(),
            Kind::Obsidian => obsidian_templates(index).into_iter().unzip(),
        };
        if names.is_empty() {
            let none = match index.graph.kind {
                Kind::Logseq => "No templates in this graph: a block with template:: NAME is one",
                Kind::Obsidian => {
                    "No templates in this vault: the notes of the templates' folder are, as .obsidian/templates.json names it"
                }
            };
            return vec![Effect::Notify(none.into(), Level::Info)];
        }
        let items = labels.into_iter().map(|l| (l, None)).collect();
        let token = self.token(Pending::InsertTemplate {
            root: root.to_string(),
            names,
            from,
            indent,
        });
        vec![Effect::Pick {
            token,
            title: "Insert the template".into(),
            items,
        }]
    }

    /// Template `name` of graph `root` filled in for the note `from`, its
    /// lines after the first under `indent`, typed at the cursor.
    pub(super) fn template_insert(
        &self,
        files: &dyn Files,
        root: &str,
        name: &str,
        from: Option<&str>,
        indent: &str,
    ) -> Vec<Effect> {
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        let today = self.known_today();
        let values = template::Values {
            title: from
                .and_then(|r| index.page_of(r))
                .map_or_else(String::new, |p| p.title.clone()),
            today,
            date: today,
            time: self
                .local_now
                .map(|now| (now().rem_euclid(86_400_000) / 1000) as u32),
        };
        let text = match index.graph.kind {
            Kind::Logseq => template::logseq_insert(index, files, name, &values, indent),
            Kind::Obsidian => template::obsidian(index, files, name, &values),
        };
        match text {
            Some(text) => vec![Effect::Run {
                id: "graph.insertText".into(),
                args: serde_json::json!({ "text": text }).to_string(),
            }],
            None => vec![Effect::Notify(
                format!("The template {name} is not there any more"),
                Level::Warning,
            )],
        }
    }
}

/// A vault's templates, the notes of its templates' folder: each its name
/// there and its path from the vault's folder, without `.md`.
fn obsidian_templates(index: &Index) -> Vec<(String, String)> {
    let Some(dir) = index
        .graph
        .templates_dir
        .as_deref()
        .filter(|d| !d.is_empty())
    else {
        return Vec::new();
    };
    let inside = format!("{dir}/");
    let mut found: Vec<(String, String)> = index
        .files()
        .filter_map(|(rel, _)| {
            let path = rel.strip_suffix(".md")?;
            let name = path.strip_prefix(&inside)?;
            Some((name.to_string(), path.to_string()))
        })
        .collect();
    found.sort_by_key(|(name, _)| name.to_lowercase());
    found
}
