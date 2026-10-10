//! The documents of the three corpus graphs as the plugin writes them,
//! the same in both editors and under `kalem run`: each compared with its
//! file in `tests/snapshots` (`CORPUS.DOCUMENT.txt`). After a change meant
//! to show, `UPDATE_SNAPSHOTS=1 cargo test -p kalem-plugin-graph --test
//! snapshots` writes them anew, to be read in the diff.
//!
//! `kalem_run_prints_them` (ignored) runs a Kalem with batch mode and the
//! plugin installed, `KALEM=path/to/kalem`, and compares what it prints.

use std::path::{Path, PathBuf};

use kalem_plugin_graph::config::{self, Fallbacks, Graph};
use kalem_plugin_graph::date::Date;
use kalem_plugin_graph::files::{self, Native};
use kalem_plugin_graph::index::Index;
use kalem_plugin_graph::{query, views};

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(name)
}

fn load(name: &str) -> Index {
    let root = files::normalize(&corpus(name).to_string_lossy());
    let kind = config::kind_of(&Native, &root).expect("a graph");
    Index::build(
        &Native,
        Graph::load(&Native, &root, kind, &Fallbacks::default()),
    )
}

/// The documents of corpus `name`, by name.
fn documents(name: &str) -> Vec<(&'static str, String)> {
    let i = load(name);
    let g = views::Glyphs::new("unicode");
    let mut docs = vec![
        ("pages", views::pages(&i).text),
        ("journals", views::journals(&i, 30, g).text),
        ("tags", views::tags(&i).text),
        ("tasks", views::tasks(&i, Date::new(2026, 10, 10)).text),
        ("graph", views::graph(&i).text),
        ("backlinks", views::backlinks(&i, "kalem", g).text),
    ];
    if name == "logseq-md" {
        let shape = query::Shape::of(&[
            ("query-table".into(), "true".into()),
            ("query-properties".into(), "[:block :page]".into()),
        ]);
        let q = "(and [[Project/Plugins]] (task NOW LATER TODO DOING))";
        docs.push((
            "query",
            views::query(&i, q, &shape, Date::new(2026, 10, 10), g).text,
        ));
    }
    docs
}

fn snapshot(name: &str, doc: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.{doc}.txt"))
}

#[test]
fn the_documents_as_written() {
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    let mut differ = Vec::new();
    for name in ["logseq-md", "logseq-org", "obsidian"] {
        for (doc, text) in documents(name) {
            let path = snapshot(name, doc);
            if update {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &text).unwrap();
                continue;
            }
            let want = std::fs::read_to_string(&path).unwrap_or_else(|_| {
                panic!("{}: missing; UPDATE_SNAPSHOTS=1 writes it", path.display())
            });
            if want != text {
                differ.push(format!("{}:\n{text}", path.display()));
            }
        }
    }
    assert!(
        differ.is_empty(),
        "differ from their snapshots:\n{}",
        differ.join("\n")
    );
}

#[test]
#[ignore = "needs KALEM, a Kalem with kalem run and the plugin installed"]
fn kalem_run_prints_them() {
    let Some(kalem) = std::env::var_os("KALEM") else {
        return;
    };
    let root = corpus("logseq-md");
    // What does not depend on today's date.
    for (command, doc, file) in [
        ("graph.pages", "pages", root.clone()),
        ("graph.journals", "journals", root.clone()),
        ("graph.tags", "tags", root.clone()),
        ("graph.graph", "graph", root.clone()),
        (
            "graph.backlinksDocument",
            "backlinks",
            root.join("pages/Kalem.md"),
        ),
    ] {
        let out = std::process::Command::new(&kalem)
            .args(["run", "graph", command])
            .arg(&file)
            .output()
            .expect("kalem runs");
        assert!(
            out.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let want = std::fs::read_to_string(snapshot("logseq-md", doc)).unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap(), want, "{command}");
    }
}
