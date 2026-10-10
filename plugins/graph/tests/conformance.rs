//! The plugin's conformance: the manifest, and the three corpus graphs
//! (a Logseq graph in Markdown, one in Org, an Obsidian vault) read as
//! their applications read them: the graph found and its settings, the
//! pages, journals, blocks, tasks and references, every resolution rule,
//! the templates, and the documents written from them.

use std::path::{Path, PathBuf};

use kalem_plugin_graph::config::{self, Fallbacks, Format, Graph, Kind, NewFile, Workflow};
use kalem_plugin_graph::date::Date;
use kalem_plugin_graph::files::{self, Files, Native};
use kalem_plugin_graph::index::Index;
use kalem_plugin_graph::scan::RefKind;
use kalem_plugin_graph::{MANIFEST, template, views};
use serde_json::Value;

fn corpus(name: &str) -> String {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(name);
    files::normalize(&p.to_string_lossy())
}

fn load(name: &str) -> Index {
    let root = corpus(name);
    let kind = config::kind_of(&Native, &root).expect("a graph");
    Index::build(
        &Native,
        Graph::load(&Native, &root, kind, &Fallbacks::default()),
    )
}

fn titles(i: &Index, key: &str) -> Vec<String> {
    let mut t: Vec<String> = i
        .backlinks(key)
        .iter()
        .map(|b| {
            i.page_of(&b.path)
                .map_or(b.path.clone(), |p| p.title.clone())
        })
        .collect();
    t.sort();
    t.dedup();
    t
}

#[test]
fn the_manifest() {
    let m: Value = serde_json::from_str(MANIFEST).expect("plugin.json parses");
    assert_eq!(m["id"], "org.kalem.graph");
    assert_eq!(m["main"], "dist/graph.wasm");
    assert_eq!(m["api"], "^0.2.8");
    assert_eq!(m["activation"], serde_json::json!(["onStartup"]));
    assert_eq!(
        m["permissions"],
        serde_json::json!(["fs:read:workspace", "fs:write:workspace"])
    );
    let version = m["version"].as_str().unwrap();
    assert_eq!(
        version
            .split('.')
            .filter(|p| p.parse::<u64>().is_ok())
            .count(),
        3
    );
    let description = m["description"].as_str().unwrap();
    assert!(description.contains("Logseq") && description.contains("Obsidian"));
    // Every setting the plugin reads is declared, with a description.
    for key in kalem_plugin_graph::app::SETTINGS {
        let s = &m["settings"][key];
        assert!(
            s["description"].is_string(),
            "the setting {key} is declared"
        );
    }
}

#[test]
fn a_logseq_graph_in_markdown() {
    let root = corpus("logseq-md");
    assert_eq!(config::kind_of(&Native, &root), Some(Kind::Logseq));
    // Found from any of its files.
    let mut not = Default::default();
    let found = config::find_root(
        &Native,
        &format!("{root}/journals/2026_10_03.md"),
        &[],
        &mut not,
    );
    assert_eq!(found, Some((root.clone(), Kind::Logseq)));
    let i = load("logseq-md");
    let g = &i.graph;
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    assert_eq!((g.format, g.workflow), (Format::Markdown, Workflow::Now));
    assert_eq!(
        (g.pages_dir.as_str(), g.journals_dir.as_str()),
        ("pages", "journals")
    );
    assert_eq!(g.hidden, ["private"]);
    assert_eq!(g.journal_template.as_deref(), Some("Daily"));
    assert_eq!(g.comma_properties, ["related", "alias", "tags"]);
    assert!(i.problems.is_empty(), "{:?}", i.problems);
    let (pages, journals, _, _) = i.counts();
    assert_eq!((pages, journals), (17, 10));
    // Never read: the backup, the hidden folder, the assets.
    assert!(i.page("ignored").is_none());
    // Titles from the file name, decoded, and from `title::`.
    assert!(
        i.page("questions?")
            .is_some_and(|p| p.path.as_deref() == Some("pages/Questions%3F.md"))
    );
    assert!(i.page("project/plugins/graph").is_some());
    assert_eq!(
        i.page("oct 3rd, 2026").unwrap().journal,
        Date::new(2026, 10, 3)
    );
    // Aliases, with and without brackets, resolve to the page.
    assert_eq!(i.resolve("Editor", "pages/Program.md"), "kalem");
    assert_eq!(i.resolve("the editor", "pages/Program.md"), "kalem");
    assert_eq!(i.page("kalem").unwrap().aliases, ["Editor", "The Editor"]);
    // What links to Kalem: pages, a journal, a property's value.
    assert_eq!(
        titles(&i, "kalem"),
        [
            "Oct 3rd, 2026",
            "Program",
            "Project/Plugins",
            "Project/Plugins/Graph",
            "Queries"
        ]
    );
    assert_eq!(titles(&i, "program"), ["Kalem"], "type:: [[Program]] links");
    assert_eq!(
        titles(&i, "org"),
        ["Kalem"],
        "related:: Org, Markdown are references"
    );
    assert!(i.page("org").unwrap().path.is_none());
    // A journal referenced by its title, and a date's reference to itself
    // leaves no backlink.
    assert_eq!(titles(&i, "oct 4th, 2026"), ["Oct 3rd, 2026"]);
    assert_eq!(titles(&i, "oct 3rd, 2026"), ["Oct 4th, 2026"]);
    assert!(titles(&i, "oct 10th, 2026").is_empty());
    // Code and code blocks hold no links; nested links do.
    for none in ["not a link", "not a link either"] {
        assert!(i.page(none).is_none(), "{none}");
    }
    assert!(i.page("nested").is_some());
    assert!(i.page("a [[nested]] link").is_some());
    // Tags are pages; a two-word tag too.
    assert!(i.tags().contains_key("software"));
    assert!(i.tags().contains_key("note graphs"));
    assert!(i.tags().contains_key("meeting"));
    // Block references and embeds.
    let (rel, b) = i.block("6512c0de-0001-4000-8000-000000000002").unwrap();
    assert_eq!(
        (rel, b.text.as_str()),
        ("pages/Kalem.md", "The core: Org, Markdown, CSV, LaTeX")
    );
    assert_eq!(
        i.block_backlinks("6512c0de-0001-4000-8000-000000000002")
            .len(),
        1
    );
    assert_eq!(
        i.block_backlinks("6512c0de-0001-4000-8000-000000000001")[0].kind,
        RefKind::EmbedBlock
    );
    assert!(i.block("6512c0de-0001-4000-8000-0000000000ff").is_none());
    // Blocks: collapsed, nested, headings.
    let k = &i.file("pages/Kalem.md").unwrap().scanned;
    assert!(k.blocks[1].collapsed);
    assert_eq!(k.blocks[3].parent, Some(1));
    assert_eq!(k.blocks[4].parent, Some(3));
    assert_eq!(k.blocks[5].heading, Some(1));
    // Tasks, with keywords, priorities and dates.
    let tasks = i.tasks();
    assert_eq!(tasks.len(), 10, "{tasks:?}");
    let p = &i.file("pages/Project___Plugins.md").unwrap().scanned;
    assert_eq!(p.blocks[1].priority, Some('A'));
    assert_eq!(p.blocks[1].scheduled, Date::new(2026, 10, 12));
    assert_eq!(p.blocks[2].deadline, Date::new(2026, 10, 31));
    // Unlinked mentions.
    let unlinked: Vec<String> = i.unlinked("kalem").iter().map(|b| b.path.clone()).collect();
    assert_eq!(
        unlinked,
        ["journals/2026_10_10.md", "pages/Questions%3F.md"]
    );
    // The journal template.
    let v = template::Values {
        title: "Oct 11th, 2026".into(),
        today: Date::new(2026, 10, 11),
        date: Date::new(2026, 10, 11),
    };
    assert_eq!(
        template::journal(&i, &Native, &v).unwrap(),
        "- ## Plan for [[Oct 11th, 2026]]\n\t- LATER Review yesterday: [[Oct 10th, 2026]]\n- ## Notes\n"
    );
    assert_eq!(
        i.journal_path(Date::new(2026, 10, 11).unwrap()),
        "journals/2026_10_11.md"
    );
}

#[test]
fn a_logseq_graph_in_org() {
    let i = load("logseq-org");
    let g = &i.graph;
    assert_eq!((g.format, g.workflow), (Format::Org, Workflow::Todo));
    assert_eq!(g.journal_template, None, "an empty template name is none");
    let (pages, journals, _, _) = i.counts();
    assert_eq!((pages, journals), (4, 2));
    assert_eq!(i.resolve("Editor", "journals/2026_10_03.org"), "kalem");
    assert_eq!(titles(&i, "kalem"), ["Oct 3rd, 2026", "Project/Plugins"]);
    assert!(i.page("not a link").is_none());
    assert!(i.page("https://example.com").is_none());
    let (rel, b) = i.block("6512c0de-0002-4000-8000-000000000001").unwrap();
    assert_eq!(
        (rel, b.text.as_str()),
        ("pages/Kalem.org", "An editor of [[Org]] files #software")
    );
    assert_eq!(
        i.block_backlinks("6512c0de-0002-4000-8000-000000000001")
            .len(),
        1
    );
    let k = &i.file("pages/Kalem.org").unwrap().scanned;
    assert_eq!(k.blocks[3].marker.as_deref(), Some("TODO"));
    assert_eq!(k.blocks[3].scheduled, Date::new(2026, 10, 12));
    assert_eq!(
        i.journal_path(Date::new(2026, 10, 11).unwrap()),
        "journals/2026_10_11.org"
    );
    assert_eq!(i.page_path("Project/New", None), "pages/Project___New.org");
}

#[test]
fn an_obsidian_vault() {
    let root = corpus("obsidian");
    assert_eq!(config::kind_of(&Native, &root), Some(Kind::Obsidian));
    let i = load("obsidian");
    let g = &i.graph;
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    assert_eq!(g.journals_dir, "Daily");
    assert_eq!(g.new_file, NewFile::Folder("Inbox".into()));
    assert_eq!(g.templates_dir.as_deref(), Some("Templates"));
    assert_eq!(g.journal_template.as_deref(), Some("Templates/Daily"));
    let (pages, journals, _, _) = i.counts();
    assert_eq!((pages, journals), (8, 3));
    assert!(i.page("old").is_none(), ".trash is not read");
    // The same name in two folders: the note's own folder first, else the
    // shortest path.
    assert_eq!(i.resolve("Kalem", "Home.md"), "kalem");
    assert_eq!(i.resolve("Kalem", "Archive/Kalem.md"), "archive/kalem");
    assert_eq!(i.resolve("Graph", "Kalem.md"), "projects/graph");
    // Aliases name the note and link nowhere.
    assert!(i.page("start").is_none());
    assert_eq!(i.page("home").unwrap().aliases, ["Start", "Index"]);
    // What links to Kalem: two wiki links and a heading link from Home, and
    // the project.
    assert_eq!(titles(&i, "kalem"), ["Graph", "Home"]);
    assert_eq!(
        titles(&i, "projects/graph"),
        ["Home", "Kalem"],
        "a Markdown link counts"
    );
    assert_eq!(titles(&i, "archive/kalem"), ["Home"]);
    // Comments, code, attachments and numbers make no link or tag.
    assert!(i.page("hidden").is_none());
    assert!(i.page("not a link").is_none());
    assert!(i.page("attachments/diagram.png").is_none());
    let tags: Vec<&String> = i.tags().keys().collect();
    assert_eq!(tags, ["home", "kalem/plugins", "tag/nested", "task"]);
    // Block ids and references to them.
    let (rel, b) = i.block("kalem#^core").unwrap();
    assert_eq!(
        (rel, b.text.as_str()),
        ("Kalem.md", "An editor of Org and Markdown.")
    );
    assert_eq!(i.block_backlinks("kalem#^core").len(), 1);
    assert_eq!(i.block_backlinks("home#^intro").len(), 1);
    // Tasks from check boxes.
    let markers: Vec<String> = i.tasks().into_iter().map(|t| t.marker).collect();
    assert_eq!(markers, ["TODO", "DONE", "DOING", "CANCELED"]);
    // The daily note and its template.
    assert_eq!(
        i.journal_path(Date::new(2026, 10, 11).unwrap()),
        "Daily/2026-10-11.md"
    );
    let v = template::Values {
        title: "2026-10-11".into(),
        today: Date::new(2026, 10, 11),
        date: Date::new(2026, 10, 11),
    };
    assert_eq!(
        template::journal(&i, &Native, &v).unwrap(),
        "# 2026-10-11\n\n## Tasks\n- [ ] \n\nWritten Sunday, October 11th 2026\n"
    );
    assert_eq!(i.page_path("Fresh", Some("Home.md")), "Inbox/Fresh.md");
}

/// Every file of every corpus: its blocks inside the file, a parent before
/// its child, its references inside their lines.
#[test]
fn every_corpus_file_scans_whole() {
    for name in ["logseq-md", "logseq-org", "obsidian"] {
        let i = load(name);
        for (rel, d) in i.files() {
            let text = Native.read(&i.graph.path(rel)).unwrap();
            let lines: Vec<&str> = text.split('\n').collect();
            let s = &d.scanned;
            assert_eq!(s.lines as usize, lines.len(), "{rel}");
            for (n, b) in s.blocks.iter().enumerate() {
                assert!(b.line < b.end && b.end <= s.lines, "{rel}: block {n} {b:?}");
                if let Some(p) = b.parent {
                    assert!(p < n, "{rel}: block {n}'s parent");
                    assert_eq!(s.blocks[p].depth + 1, b.depth, "{rel}: block {n}'s depth");
                }
            }
            for r in &s.refs {
                let line = lines[r.line as usize];
                assert!(r.end as usize <= line.len(), "{rel}: {r:?}");
                if r.end > r.start {
                    assert!(line.is_char_boundary(r.start as usize), "{rel}: {r:?}");
                }
                if let Some(b) = r.block {
                    let b = &s.blocks[b];
                    assert!(
                        b.line <= r.line && r.line < b.end,
                        "{rel}: {r:?} outside its block"
                    );
                }
            }
        }
    }
}

/// The documents of every corpus are written without a line Enter does
/// not know.
#[test]
fn the_documents_of_every_corpus() {
    let g = views::Glyphs::new("unicode");
    for name in ["logseq-md", "logseq-org", "obsidian"] {
        let i = load(name);
        let docs = [
            views::pages(&i),
            views::journals(&i, 30, g),
            views::tags(&i),
            views::tasks(&i, Date::new(2026, 10, 10)),
            views::graph(&i),
            views::backlinks(&i, "kalem", g),
        ];
        for c in docs {
            assert_eq!(c.targets.len(), c.text.lines().count(), "{name}");
            for s in &c.styles {
                assert!(s.start < s.end && s.end <= c.text.len());
            }
        }
    }
    let i = load("logseq-md");
    let c = views::backlinks(&i, "kalem", g);
    assert!(
        c.text.starts_with(
            "Backlinks: Kalem\nlogseq-md · Logseq · pages/Kalem.md\n\nLinked references ("
        ),
        "{}",
        c.text
    );
    assert!(
        c.text
            .contains("▾ Oct 3rd, 2026 (1)\n    • TODO Read [[Kalem]] docs\n"),
        "{}",
        c.text
    );
    assert!(c.text.contains("Unlinked references (2)"), "{}", c.text);
}

#[test]
fn queries_on_the_corpus() {
    use kalem_plugin_graph::query::{self, Shape};
    let i = load("logseq-md");
    let today = Date::new(2026, 10, 10);
    let found = |q: &str| -> Vec<String> {
        let a = query::run(&query::parse(q).expect(q), &i, today);
        a.hits.iter().map(|h| query::hit_text(&i, h)).collect()
    };
    // The page's open tasks, the corpus's own query, a block reference
    // shown as its block's text.
    assert_eq!(
        found("(and [[Project/Plugins]] (task NOW LATER TODO DOING))"),
        [
            "TODO [#A] Write the graph plugin [[Project/Plugins/Graph]]",
            "DOING [#B] The plain-text accounting plugin",
            "LATER The Typst mode, after the mode binding",
            "NOW Review The core: Org, Markdown, CSV, LaTeX",
        ]
    );
    // A journal's day and a SCHEDULED date within the coming week.
    assert_eq!(
        found("(and (task NOW LATER TODO DOING) (between today +7d))"),
        [
            "NOW Start [[Project/Plugins/Graph]]",
            "TODO [#A] Write the graph plugin [[Project/Plugins/Graph]]",
        ]
    );
    assert_eq!(
        found("(namespace [[Project]])"),
        ["Project/Plugins", "Project/Plugins/Graph"]
    );
    // The table the corpus's block asks for, by its properties.
    let q = i.file("pages/Queries.md").unwrap();
    let holder = q
        .scanned
        .blocks
        .iter()
        .find(|b| b.text.contains("{{query (and [[Project/Plugins]]"))
        .unwrap();
    let shape = Shape::of(&holder.props);
    assert_eq!(
        shape.table.as_deref(),
        Some(&["block".to_string(), "page".to_string()][..])
    );
    let c = views::query(
        &i,
        "(and [[Project/Plugins]] (task NOW LATER TODO DOING))",
        &shape,
        today,
        views::Glyphs::new("unicode"),
    );
    let lines: Vec<&str> = c.text.lines().collect();
    assert_eq!(lines[1], "4 blocks");
    assert!(lines[3].starts_with("block ") && lines[3].ends_with("│ page"));
    assert!(
        lines[4].starts_with("TODO [#A] Write the graph plugin")
            && lines[4].ends_with("│ Project/Plugins")
    );
    // Each row opens its block.
    assert!(matches!(
        c.target(4),
        Some(kalem_plugin_graph::content::Target::Block { line: 1, .. })
    ));
}
