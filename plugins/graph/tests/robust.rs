//! What a graph can hold that the plugin does not expect (GR11): files it
//! cannot read give one notice and never a panic, and notes broken in
//! every way the syntax allows go through the scanner, the layer, the
//! queries and the editing commands without one. A panic in Kalem stops
//! the plugin (the `diagnostics` interface's `panicked`); here it fails
//! the test.

use std::path::Path;

use kalem_plugin_graph::app::{App, Effect, Settings};
use kalem_plugin_graph::config::{self, Fallbacks, Graph, Kind};
use kalem_plugin_graph::date::Date;
use kalem_plugin_graph::edit::{self, Change};
use kalem_plugin_graph::files::{self, Files, Memory, Native};
use kalem_plugin_graph::index::Index;
use kalem_plugin_graph::layer::{self, Overlays};
use kalem_plugin_graph::scan::{self, Flavor};
use kalem_plugin_graph::{query, views};

/// A graph in a folder of its own under the system's temporary one.
fn folder(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("kalem-graph-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("logseq")).unwrap();
    std::fs::create_dir_all(dir.join("pages")).unwrap();
    files::normalize(&dir.to_string_lossy())
}

#[test]
fn what_cannot_be_read_is_one_notice() {
    let root = folder("unreadable");
    let at = |rel: &str| Path::new(&root).join(rel);
    // A configuration that does not parse, a page not in UTF-8, a page
    // over 16 MB.
    std::fs::write(at("logseq/config.edn"), "{:preferred-format :markdown").unwrap();
    std::fs::write(at("pages/Good.md"), "- links [[Bad]] and [[Huge]]\n").unwrap();
    std::fs::write(at("pages/Bad.md"), b"- caf\xe9 in Latin-1\n").unwrap();
    std::fs::write(
        at("pages/Huge.md"),
        "- x\n".repeat((files::MAX_BYTES / 4) + 1),
    )
    .unwrap();

    let mut app = App::new(Settings::default());
    app.settings(
        &Native,
        Settings {
            graphs: vec![(root.clone(), Kind::Logseq)],
            ..Settings::default()
        },
    );
    let out = app.opened(&Native, &format!("{root}/pages/Good.md"));
    let notices: Vec<&String> = out
        .iter()
        .filter_map(|e| match e {
            Effect::Notify(t, _) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("logseq/config.edn"), "{}", notices[0]);
    assert!(
        notices[0].ends_with("(2 more: All pages lists them)"),
        "{}",
        notices[0]
    );
    // The defaults used, the readable page indexed, the others named.
    let index = app.index(&root).expect("indexed");
    assert!(index.page("good").is_some_and(|p| p.path.is_some()));
    let pages = views::pages(index).text;
    assert!(pages.contains("Not read (3)"), "{pages}");
    assert!(pages.contains("Huge.md is larger than 16 MB"), "{pages}");
    assert!(pages.contains("Bad.md"), "{pages}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn embeds_of_themselves_are_one_line() {
    // A block embedding itself, two embedding each other: the layer shows
    // each embed as one line and never follows them further.
    let m = Memory::new(&[
        ("/g/logseq/config.edn", "{}"),
        (
            "/g/pages/Loop.md",
            "- me {{embed ((6512c0de-0000-4000-8000-000000000001))}}\n  id:: 6512c0de-0000-4000-8000-000000000001\n- a {{embed ((6512c0de-0000-4000-8000-000000000003))}}\n  id:: 6512c0de-0000-4000-8000-000000000002\n- b {{embed ((6512c0de-0000-4000-8000-000000000002))}} ((6512c0de-0000-4000-8000-000000000002))\n  id:: 6512c0de-0000-4000-8000-000000000003\n",
        ),
    ]);
    let i = Index::build(
        &m,
        Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
    );
    let text = m.read("/g/pages/Loop.md").unwrap();
    let o = layer::overlays(Kind::Logseq, &text, Some(&i), Some("pages/Loop.md"));
    check(&text, &o);
    let shown: Vec<&str> = o
        .spans
        .iter()
        .filter_map(|s| match &s.effect {
            layer::Effect::Replace(t, _) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        shown.contains(&"↳ me {{embed ((6512c0de-0000-4000-8000-000000000001))}}"),
        "{shown:?}"
    );
    let a = query::run(&query::parse("\"embed\"").unwrap(), &i, None);
    for h in &a.hits {
        assert!(query::hit_text(&i, h).len() < 200);
    }
}

/// The overlays keep to the text: spans and lines in order, apart, inside
/// it and on characters.
fn check(text: &str, o: &Overlays) {
    let mut end = 0;
    for s in &o.spans {
        assert!(
            s.start >= end && s.start < s.end && s.end <= text.len(),
            "{s:?} in {text:?}"
        );
        assert!(
            text.is_char_boundary(s.start) && text.is_char_boundary(s.end),
            "{s:?}"
        );
        end = s.end;
    }
    let mut end = 0;
    for l in &o.lines {
        assert!(
            l.start >= end && l.start < l.end && l.end <= text.len(),
            "{l:?} in {text:?}"
        );
        assert!(
            l.start == 0 || text.as_bytes()[l.start - 1] == b'\n',
            "{l:?}"
        );
        end = l.end;
    }
}

/// A generator of numbers, the same every run.
struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n.max(1) as u64) as usize
    }
    /// A byte of `text` that starts a character, or its end.
    fn boundary(&mut self, text: &str) -> usize {
        let mut at = self.below(text.len() + 1);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }
}

/// What the syntax of the three applications is made of, and what breaks
/// it.
const PIECES: &[&str] = &[
    "[[",
    "]]",
    "((",
    "))",
    "{{embed ",
    "{{query (and ",
    "{{query",
    "}}",
    "#",
    "#+",
    "#[[",
    "::",
    "\n",
    "\t- ",
    "- ",
    "  ",
    "```",
    "\n```\n",
    ":LOGBOOK:\n",
    ":END:",
    "%%",
    "==",
    "^^",
    "> [!note]-",
    "|",
    "^",
    "#^",
    "é",
    "日本",
    "\r\n",
    "TODO ",
    "DONE ",
    "[#A] ",
    "[#",
    "SCHEDULED: <2026-10-1",
    "DEADLINE: <2026-02-30 Mon>",
    "id:: ",
    "collapsed:: true\n",
    "---\n",
    "😀",
    "* ",
    "** ",
    ":PROPERTIES:\n",
    "#+BEGIN_QUERY\n",
    "#+END_SRC",
    "$$",
    "<% today %>",
    "[x] ",
    "[ ] ",
    "\t",
    "](",
    "![[",
    "\\",
    "\"",
    "(sort-by",
    "title:: ",
    "alias:: ",
    "tags:: [[a]], b",
    "[[a [[b]] c]]",
    "6512c0de-0001-4000-8000-000000000002",
];

/// `text` broken a few times: a piece put in, a part taken out, a line
/// doubled.
fn mutate(text: &str, r: &mut Lcg) -> String {
    let mut t = text.to_string();
    for _ in 0..1 + r.below(4) {
        match r.below(3) {
            0 => {
                let at = r.boundary(&t);
                t.insert_str(at, PIECES[r.below(PIECES.len())]);
            }
            1 if !t.is_empty() => {
                let a = r.boundary(&t);
                let mut b = (a + r.below(40)).min(t.len());
                while !t.is_char_boundary(b) {
                    b -= 1;
                }
                t.replace_range(a..b.max(a), "");
            }
            _ => {
                let lines: Vec<&str> = t.split_inclusive('\n').collect();
                if !lines.is_empty() {
                    let n = r.below(lines.len());
                    let mut out: String = lines[..=n].concat();
                    out.push_str(lines[n]);
                    out.push_str(&lines[n + 1..].concat());
                    t = out;
                }
            }
        }
    }
    t
}

/// Every editing command at `cursor`; each change applies to the text.
fn edit_everything(text: &str, flavor: Flavor, cursor: usize) {
    let date = Date::new(2026, 10, 12);
    let changes: Vec<Option<Change>> = vec![
        edit::cycle_todo(text, flavor, ["LATER", "NOW", "DONE"], cursor),
        edit::cycle_todo(text, flavor, ["TODO", "DOING", "DONE"], cursor),
        edit::set_priority(text, flavor, cursor, Some('A')),
        edit::set_priority(text, flavor, cursor, None),
        edit::set_date(text, flavor, cursor, "SCHEDULED", date),
        edit::set_date(text, flavor, cursor, "DEADLINE", None),
        edit::move_block(text, flavor, cursor, true),
        edit::move_block(text, flavor, cursor, false),
        edit::shift_block(text, flavor, cursor, true),
        edit::shift_block(text, flavor, cursor, false),
        edit::toggle_fold(text, cursor).ok(),
    ];
    for c in changes.into_iter().flatten() {
        for e in &c.edits {
            assert!(e.start <= e.end && e.end <= text.len(), "{e:?} in {text:?}");
            assert!(
                text.is_char_boundary(e.start) && text.is_char_boundary(e.end),
                "{e:?}"
            );
        }
        let _ = edit::apply(text, &c.edits);
    }
}

/// How many broken copies of each note: `ROBUST_ROUNDS` for a longer
/// search.
fn rounds() -> usize {
    std::env::var("ROBUST_ROUNDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(40)
}

/// Where the generator starts: `ROBUST_SEED` for another search.
fn seed() -> u64 {
    std::env::var("ROBUST_SEED")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2026)
}

#[test]
fn broken_notes_never_panic() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus");
    let mut r = Lcg(seed());
    for name in ["logseq-md", "logseq-org", "obsidian"] {
        let root = files::normalize(&corpus.join(name).to_string_lossy());
        let kind = config::kind_of(&Native, &root).unwrap();
        let index = Index::build(
            &Native,
            Graph::load(&Native, &root, kind, &Fallbacks::default()),
        );
        let rels: Vec<String> = index.files().map(|(rel, _)| rel.clone()).collect();
        for rel in rels {
            let original = Native.read(&index.graph.path(&rel)).unwrap();
            for _ in 0..rounds() {
                let text = mutate(&original, &mut r);
                for flavor in [Flavor::LogseqMarkdown, Flavor::LogseqOrg, Flavor::Obsidian] {
                    let s = scan::scan(&text, flavor, &scan::Options::default());
                    for b in &s.blocks {
                        assert!(b.line < b.end && b.end <= s.lines, "{b:?} in {text:?}");
                    }
                    for _ in 0..3 {
                        let cursor = r.boundary(&text);
                        edit_everything(&text, flavor, cursor);
                    }
                }
                for k in [Kind::Logseq, Kind::Obsidian] {
                    let o = layer::overlays(k, &text, Some(&index), Some(&rel));
                    check(&text, &o);
                    let o = layer::queries(o, &text, &|q| query::summary(&index, q, None, "⌕"));
                    check(&text, &o);
                }
                for line in text.lines() {
                    for (_, _, inner) in query::macros(line) {
                        if let Ok(q) = query::parse(inner) {
                            query::run(&q, &index, Date::new(2026, 10, 10));
                        }
                    }
                    let _ = scan::references(line, Flavor::LogseqMarkdown);
                    let _ = scan::references(line, Flavor::Obsidian);
                }
            }
        }
    }
}

#[test]
fn broken_queries_never_panic() {
    let m = Memory::new(&[
        ("/g/logseq/config.edn", "{}"),
        ("/g/pages/A.md", "- TODO a [[b]]\n"),
    ]);
    let i = Index::build(
        &m,
        Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
    );
    let mut r = Lcg(9);
    let atoms = [
        "(",
        ")",
        "and",
        "or",
        "not",
        "task",
        "TODO",
        "[[b]]",
        "#t",
        "\"x",
        "\"",
        "between",
        "-7d",
        "+1w",
        "today",
        "[[Oct 3rd, 2026]]",
        "property",
        "page-property",
        "sort-by",
        "sample",
        "-3",
        "99999999999999999999",
        "namespace",
        "priority",
        "é",
        "]]",
        "[[",
        "#[[",
    ];
    for _ in 0..5_000 {
        let q: Vec<&str> = (0..1 + r.below(8))
            .map(|_| atoms[r.below(atoms.len())])
            .collect();
        let q = q.join(" ");
        if let Ok(parsed) = query::parse(&q) {
            query::run(&parsed, &i, Date::new(2026, 10, 10));
            query::run(&parsed, &i, None);
        }
        let line = format!("- x {{{{query {q}}}}} y");
        for (s, e, _) in query::macros(&line) {
            assert!(
                s < e && e <= line.len() && line.is_char_boundary(s) && line.is_char_boundary(e)
            );
        }
    }
}
