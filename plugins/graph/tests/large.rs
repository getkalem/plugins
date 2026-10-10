//! The large graph (GR11): a Logseq graph generated in memory at the
//! scale Logseq aims at, 10,000 pages and 1,000,000 blocks, 50 references
//! a page, one page referred to 1,000 times; what the plugin does with it
//! measured against its budgets. Ignored by default, as it takes some
//! seconds and wants an optimized build:
//!
//! ```text
//! cargo test --release -p kalem-plugin-graph --test large -- --ignored --nocapture
//! ```
//!
//! The budgets are for this native build; Kalem runs the plugin as
//! WebAssembly, some two to three times slower (the README has both).

// The measurements are printed: this test is run to read them.
#![allow(clippy::print_stderr)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kalem_plugin_graph::config::{Fallbacks, Graph, Kind};
use kalem_plugin_graph::files::{Files, Memory};
use kalem_plugin_graph::index::Index;
use kalem_plugin_graph::{layer, query, views};

/// The heap in use, and the most it held.
struct Counting;
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call goes to the system's allocator; the counters only
// add and take away the sizes it was given.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let n = NOW.fetch_add(l.size(), Ordering::Relaxed) + l.size();
        PEAK.fetch_max(n, Ordering::Relaxed);
        // SAFETY: as the caller's.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Ordering::Relaxed);
        // SAFETY: as the caller's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        if size > l.size() {
            let n = NOW.fetch_add(size - l.size(), Ordering::Relaxed) + size - l.size();
            PEAK.fetch_max(n, Ordering::Relaxed);
        } else {
            NOW.fetch_sub(l.size() - size, Ordering::Relaxed);
        }
        // SAFETY: as the caller's.
        unsafe { System.realloc(p, l, size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const PAGES: usize = 9_000;
const JOURNALS: usize = 1_000;
const BLOCKS: usize = 100;
const REFS: usize = 50;
const HUB_REFS: usize = 1_000;

/// A generator of numbers, the same every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const WORDS: &[&str] = &[
    "the", "graph", "editor", "plain", "text", "notes", "outline", "block", "link", "journal",
    "review", "draft", "plan", "idea", "meeting", "reading", "writing", "kalem", "plugin", "query",
];

fn uuid(n: usize) -> String {
    format!("6512c0de-{:04x}-4000-8000-{:012x}", n % 0xffff, n)
}

fn title(n: usize) -> String {
    format!("Page {n:05}")
}

/// A page's text: 100 blocks, 20 at the top level with four under each,
/// 50 references to other pages (one to Hub for the first 1,000 files),
/// a tag, a task or a block reference now and then, an id per page.
fn page_text(file: usize, r: &mut Lcg) -> String {
    let mut s = String::with_capacity(BLOCKS * 64);
    if file < PAGES {
        s.push_str("type:: note\ntags:: generated\n\n");
    }
    let mut refs = 0;
    for b in 0..BLOCKS {
        let depth = usize::from(b % 5 != 0);
        s.push_str(&"\t".repeat(depth));
        s.push_str("- ");
        match b % 10 {
            3 => s.push_str("TODO "),
            7 => s.push_str("LATER "),
            _ => {}
        }
        for _ in 0..6 {
            s.push_str(WORDS[r.below(WORDS.len())]);
            s.push(' ');
        }
        if b % 2 == 0 && refs < REFS {
            refs += 1;
            if refs == 1 && file < HUB_REFS {
                s.push_str("[[Hub]]");
            } else {
                s.push_str(&format!("[[{}]]", title(r.below(PAGES))));
            }
        }
        if b % 25 == 1 {
            s.push_str(&format!(" #tag{}", r.below(100)));
        }
        if b % 33 == 2 {
            s.push_str(&format!(" (({}))", uuid(r.below(PAGES))));
        }
        s.push('\n');
        if b == 0 && file < PAGES {
            s.push_str(&"\t".repeat(depth));
            s.push_str(&format!("  id:: {}\n", uuid(file)));
        }
    }
    s
}

fn journal_name(n: usize) -> String {
    let d = kalem_plugin_graph::date::Date::new(2020, 1, 1)
        .unwrap()
        .add_days(n as i64);
    format!("journals/{:04}_{:02}_{:02}.md", d.year, d.month, d.day)
}

/// The graph's files.
fn graph() -> Vec<(String, String)> {
    let mut r = Lcg(7);
    let mut files = vec![
        (
            "/big/logseq/config.edn".to_string(),
            "{:file/name-format :triple-lowbar}".to_string(),
        ),
        (
            "/big/pages/Hub.md".to_string(),
            "- The page everything links to\n".to_string(),
        ),
    ];
    for n in 0..PAGES + JOURNALS {
        let path = if n < PAGES {
            format!("/big/pages/{}.md", title(n))
        } else {
            format!("/big/{}", journal_name(n - PAGES))
        };
        files.push((path, page_text(n, &mut r)));
    }
    files
}

/// A journal of 5,000 blocks as Logseq writes a busy day: properties,
/// ids, references, tasks with their logbook.
fn long_journal() -> String {
    let mut r = Lcg(11);
    let mut s = String::new();
    for b in 0..5_000 {
        let depth = b % 3;
        let tab = "\t".repeat(depth);
        s.push_str(&format!("{tab}- "));
        if b % 7 == 0 {
            s.push_str("DOING ");
        }
        for _ in 0..5 {
            s.push_str(WORDS[r.below(WORDS.len())]);
            s.push(' ');
        }
        s.push_str(&format!("[[{}]] #tag{}", title(r.below(PAGES)), b % 50));
        if b % 9 == 0 {
            s.push_str(&format!(" (({}))", uuid(r.below(PAGES))));
        }
        s.push('\n');
        if b % 4 == 0 {
            s.push_str(&format!("{tab}  id:: {}\n", uuid(100_000 + b)));
        }
        if b % 7 == 0 {
            s.push_str(&format!(
                "{tab}  :LOGBOOK:\n{tab}  CLOCK: [2026-10-10 Sat 09:00:00]\n{tab}  :END:\n"
            ));
        }
    }
    s
}

/// The budgets passed over, said together at the end.
static OVER: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn over(what: String) {
    eprintln!("  OVER: {what}");
    OVER.lock().unwrap().push(what);
}

fn time<R>(what: &str, budget: Duration, f: impl FnOnce() -> R) -> R {
    let start = Instant::now();
    let r = f();
    let took = start.elapsed();
    eprintln!("{what}: {took:.2?} (budget {budget:.0?})");
    if took > budget {
        over(format!("{what}: {took:?} over {budget:?}"));
    }
    r
}

#[test]
#[ignore = "the large graph: run with --release --ignored"]
fn a_million_blocks() {
    let files = graph();
    let pairs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    let m = Memory::new(&pairs);
    drop(pairs);
    let text_bytes: usize = files.iter().map(|(_, t)| t.len()).sum();
    drop(files);
    eprintln!(
        "{} files, {} MB of text",
        PAGES + JOURNALS + 1,
        text_bytes >> 20
    );

    // Built: time and the memory the index holds.
    let before = NOW.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let mut index = time("index built", Duration::from_secs(2), || {
        Index::build(
            &m,
            Graph::load(&m, "/big", Kind::Logseq, &Fallbacks::default()),
        )
    });
    let held = NOW.load(Ordering::Relaxed) - before;
    let peak = PEAK.load(Ordering::Relaxed) - before;
    eprintln!(
        "index held: {} MB, at most {} MB while built",
        held >> 20,
        peak >> 20
    );
    if held >= 300 << 20 {
        over(format!("the index holds {} MB", held >> 20));
    }
    let (pages, journals, blocks, refs) = index.counts();
    eprintln!("{pages} pages, {journals} journals, {blocks} blocks, {refs} references");
    assert!(blocks >= 1_000_000, "{blocks}");
    assert_eq!(index.backlinks("hub").len(), HUB_REFS);

    // A save: one page written again, its text changed but not its name.
    let path = format!("/big/pages/{}.md", title(42));
    let mut text = m.read(&path).unwrap();
    text.push_str("- one more block with [[Hub]]\n");
    m.write(&path, &text).unwrap();
    time("a save indexed again", Duration::from_millis(10), || {
        assert!(index.update(&m, &path));
    });
    assert_eq!(index.backlinks("hub").len(), HUB_REFS + 1);

    // The panel and the document of the page with the most references.
    time(
        "the backlinks panel of a page with 1,000 references",
        Duration::from_millis(50),
        || views::backlinks_panel(&index, "hub", false),
    );
    time(
        "its unlinked references opened",
        Duration::from_millis(500),
        || views::backlinks_panel(&index, "hub", true),
    );
    let g = views::Glyphs::new("unicode");
    time(
        "the backlinks document of the same page",
        Duration::from_millis(500),
        || views::backlinks(&index, "hub", g),
    );

    // The layer over a busy day's journal, at every keystroke.
    let journal = long_journal();
    let o = time(
        "the layer over a journal of 5,000 blocks",
        Duration::from_millis(50),
        || {
            layer::overlays(
                Kind::Logseq,
                &journal,
                Some(&index),
                Some("journals/2026_10_10.md"),
            )
        },
    );
    assert!(!o.spans.is_empty() && !o.lines.is_empty());

    // The documents and a query over the whole graph.
    time("the Tasks document", Duration::from_millis(500), || {
        views::tasks(&index, kalem_plugin_graph::date::Date::new(2026, 10, 10))
    });
    time("the All pages document", Duration::from_millis(500), || {
        views::pages(&index, views::PageSort::Title)
    });
    time("the Graph document", Duration::from_secs(2), || {
        views::graph(&index)
    });
    let q = query::parse("(and [[Hub]] (task TODO LATER))").unwrap();
    let a = time("a query over the graph", Duration::from_millis(500), || {
        query::run(&q, &index, None)
    });
    assert!(!a.hits.is_empty());
    let over = OVER.lock().unwrap();
    assert!(over.is_empty(), "{over:#?}");
}

/// The same graph written to the folder `GRAPH_DIR` names, to measure
/// the plugin in Kalem itself (`kalem run graph graph.pages DIR`).
#[test]
#[ignore = "writes the large graph to GRAPH_DIR"]
fn write_the_large_graph() {
    let Some(dir) = std::env::var_os("GRAPH_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    for (path, text) in graph() {
        let p = dir.join(path.trim_start_matches("/big/"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}
