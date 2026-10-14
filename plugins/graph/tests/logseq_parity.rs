//! The plugin's answers against Logseq's own on the corpora `logseq-md`
//! and `logseq-org`: `tests/logseq/answers-0.10.9.json` and
//! `answers-org-0.10.9.json` hold what Logseq 0.10.9 answered, read from
//! its database through its API with the corpus open (its simple and
//! advanced queries, the pages referring to each page, its graph view),
//! so that the plugin finds what Logseq finds, not what this crate
//! assumed it does.

use std::collections::BTreeSet;
use std::path::Path;

use kalem_plugin_graph::config::{self, Fallbacks, Graph};
use kalem_plugin_graph::date::Date;
use kalem_plugin_graph::files::Native;
use kalem_plugin_graph::index::Index;
use kalem_plugin_graph::query::{self, Hit};
use kalem_plugin_graph::{datalog, files};

/// The corpora and the files of Logseq's answers on them.
const CORPORA: [(&str, &str); 2] = [
    ("logseq-md", "answers-0.10.9.json"),
    ("logseq-org", "answers-org-0.10.9.json"),
];

fn corpus(name: &str) -> String {
    files::normalize(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("corpus")
            .join(name)
            .to_string_lossy(),
    )
}

/// Corpus `name` indexed, and Logseq's answers in `file`.
fn load(name: &str, file: &str) -> (String, Index, serde_json::Value) {
    let root = corpus(name);
    let kind = config::kind_of(&Native, &root).unwrap();
    let i = Index::build(
        &Native,
        Graph::load(&Native, &root, kind, &Fallbacks::default()),
    );
    let answers = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/logseq")
                .join(file),
        )
        .unwrap(),
    )
    .unwrap();
    (root, i, answers)
}

/// A hit as the answers write it: its page, and a block's first line.
fn written(i: &Index, root: &str, h: &Hit) -> Vec<String> {
    let first = |rel: &str, line: usize| {
        let text = std::fs::read_to_string(format!("{root}/{rel}")).unwrap();
        let l = text.lines().nth(line).unwrap_or("").trim();
        // A Markdown block's bullet, an Org headline's stars.
        let stars = l.trim_start_matches('*');
        l.strip_prefix("- ")
            .or_else(|| {
                (stars.len() < l.len())
                    .then(|| stars.strip_prefix(' '))
                    .flatten()
            })
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
    let mut differ = Vec::new();
    for (name, file) in CORPORA {
        differ.extend(
            queries_and_references(name, file)
                .into_iter()
                .map(|d| format!("{name}: {d}")),
        );
    }
    assert!(
        differ.is_empty(),
        "differ from Logseq's answers:\n{}",
        differ.join("\n")
    );
}

/// Where the plugin's queries and references differ from Logseq's on
/// corpus `name`.
fn queries_and_references(name: &str, file: &str) -> Vec<String> {
    let (root, i, answers) = load(name, file);
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
    differ
}

#[test]
fn the_graph_view_as_logseq_draws_it() {
    for (name, file) in CORPORA {
        graph_view(name, file);
    }
}

/// The plugin's graph view of corpus `name` against Logseq's.
fn graph_view(name: &str, file: &str) {
    let (_, i, answers) = load(name, file);
    // Logseq's own, not drawn by the plugin: a page named by the block
    // `{{embed ((uuid))}}` embeds, which Logseq 0.10.9 makes by mistake.
    let logseq_only = ["6512c0de-0001-4000-8000-000000000001"];
    let want_nodes: BTreeSet<String> = answers["graph_view"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .filter(|n| !logseq_only.contains(&n.as_str()))
        .collect();
    let want_links: BTreeSet<(String, String)> = answers["graph_view"]["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            let l = l.as_array().unwrap();
            (
                l[0].as_str().unwrap().to_string(),
                l[1].as_str().unwrap().to_string(),
            )
        })
        .filter(|(a, b)| !logseq_only.contains(&a.as_str()) && !logseq_only.contains(&b.as_str()))
        .collect();
    let d = kalem_plugin_graph::views::drawing(&i, None)
        .drawing
        .unwrap();
    let got_nodes: BTreeSet<String> = d.nodes.iter().map(|n| n.id.clone()).collect();
    let got_links: BTreeSet<(String, String)> = d
        .edges
        .iter()
        .map(|(a, b)| {
            if a < b {
                (a.clone(), b.clone())
            } else {
                (b.clone(), a.clone())
            }
        })
        .collect();
    assert_eq!(
        (
            want_nodes.difference(&got_nodes).collect::<Vec<_>>(),
            got_nodes.difference(&want_nodes).collect::<Vec<_>>()
        ),
        (Vec::<&String>::new(), Vec::<&String>::new()),
        "{name}: nodes: Logseq's only, the plugin's only"
    );
    assert_eq!(
        (
            want_links.difference(&got_links).collect::<Vec<_>>(),
            got_links.difference(&want_links).collect::<Vec<_>>()
        ),
        (Vec::new(), Vec::new()),
        "{name}: links: Logseq's only, the plugin's only"
    );
}
