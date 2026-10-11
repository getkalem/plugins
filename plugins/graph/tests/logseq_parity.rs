//! The plugin's answers against Logseq's own on the corpus `logseq-md`:
//! `tests/logseq/answers-0.10.9.json` holds what Logseq 0.10.9 answered,
//! read from its database through its API with the corpus open (its
//! simple and advanced queries, and the pages referring to each page), so
//! that the plugin finds what Logseq finds, not what this crate assumed
//! it does.

use std::collections::BTreeSet;
use std::path::Path;

use kalem_plugin_graph::config::{self, Fallbacks, Graph};
use kalem_plugin_graph::date::Date;
use kalem_plugin_graph::files::Native;
use kalem_plugin_graph::index::Index;
use kalem_plugin_graph::query::{self, Hit};
use kalem_plugin_graph::{datalog, files};

fn corpus() -> String {
    files::normalize(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("corpus/logseq-md")
            .to_string_lossy(),
    )
}

/// A hit as the answers write it: its page, and a block's first line.
fn written(i: &Index, root: &str, h: &Hit) -> Vec<String> {
    let first = |rel: &str, line: usize| {
        let text = std::fs::read_to_string(format!("{root}/{rel}")).unwrap();
        let l = text.lines().nth(line).unwrap_or("").trim();
        l.strip_prefix("- ")
            .or_else(|| l.strip_prefix("* "))
            .unwrap_or(l)
            .to_lowercase()
    };
    let title = |rel: &str| i.page_of(rel).unwrap().title.to_lowercase();
    match h {
        Hit::Page(k) => vec![i.page(k).unwrap().title.to_lowercase()],
        Hit::Props { rel } => vec![title(rel), first(rel, 0)],
        Hit::Block { rel, block } => {
            let line = i.file(rel).unwrap().scanned.blocks[*block].line as usize;
            vec![title(rel), first(rel, line)]
        }
    }
}

#[test]
fn queries_and_references_as_logseq_answers() {
    let root = corpus();
    let kind = config::kind_of(&Native, &root).unwrap();
    let i = Index::build(
        &Native,
        Graph::load(&Native, &root, kind, &Fallbacks::default()),
    );
    let answers: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/logseq/answers-0.10.9.json"),
        )
        .unwrap(),
    )
    .unwrap();
    // The day the answers were read on: relative days count from it.
    let today = Date::new(2026, 10, 11);
    let mut differ = Vec::new();
    for a in answers["queries"].as_array().unwrap() {
        let q = a["query"].as_str().unwrap();
        let want: BTreeSet<Vec<String>> = a["answer"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                r.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap().to_string())
                    .collect()
            })
            .collect();
        let hits = if a["kind"] == "simple" {
            query::run(&query::parse(q).unwrap(), &i, today).hits
        } else {
            let adv = datalog::parse(&format!("{{:query {q}}}")).unwrap();
            match datalog::run(&adv, &i, today, None).unwrap() {
                datalog::Found::Hits(a) => a.hits,
                datalog::Found::Table(..) => panic!("{q}: a table"),
            }
        };
        let got: BTreeSet<Vec<String>> = hits.iter().map(|h| written(&i, &root, h)).collect();
        if got != want {
            differ.push(format!(
                "{q}\n  Logseq only: {:?}\n  plugin only: {:?}",
                want.difference(&got).collect::<Vec<_>>(),
                got.difference(&want).collect::<Vec<_>>()
            ));
        }
    }
    for (path, refs) in answers["backlinks"].as_object().unwrap() {
        // A page Logseq wrote when it opened the graph is not the corpus's.
        let Some(page) = i.page_of(path) else {
            continue;
        };
        let want: BTreeSet<String> = refs
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap().to_string())
            .collect();
        let got: BTreeSet<String> = i
            .backlinks(&page.key)
            .iter()
            .filter_map(|b| i.page_of(&b.path))
            .map(|p| p.title.to_lowercase())
            .collect();
        if got != want {
            differ.push(format!(
                "references of {path}: Logseq {want:?}, plugin {got:?}"
            ));
        }
    }
    assert!(
        differ.is_empty(),
        "differ from Logseq's answers:\n{}",
        differ.join("\n")
    );
}
