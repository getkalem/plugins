//! The corpus through rust-analyzer, spoken to by Kalem's language server
//! client as Kalem speaks to it: the server the manifest describes,
//! started in the corpus's workspace with the manifest's settings, then
//! what the plugin's README says it gives, asked and checked (RS4, RS5,
//! RS6 of `rust_todo.md`). It runs where rust-analyzer runs and says it
//! skipped otherwise (CI installs no component).
//!
//! Not here, with its reason: a completion's automatic `use` (Kalem does
//! not ask for it yet, RS6).

// A test skipped is said on standard error.
#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_lsp::features;
use kalem_lsp::position::{Encoding, position};
use kalem_lsp::{Client, ServerConfig};
use serde_json::{Value, json};

const MANIFEST: &str = include_str!("../plugin.json");

fn dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn workspace() -> PathBuf {
    dir().join("corpus/ws")
}

/// rust-analyzer as the manifest names it, if it runs here (rustup's
/// proxy is found without its component, and only running it tells).
fn rust_analyzer(m: &Value) -> Option<String> {
    let program = m["servers"]["rust-analyzer"]["command"][0]
        .as_str()?
        .to_string();
    std::process::Command::new(&program)
        .arg("--version")
        .current_dir(workspace())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| program)
}

struct Server {
    client: Client,
    enc: Encoding,
    target: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.client.shutdown();
        let _ = std::fs::remove_dir_all(&self.target);
    }
}

fn uri(f: &str) -> String {
    kalem_lsp::uri::from_path(&workspace().join(f))
}

fn text(f: &str) -> String {
    std::fs::read_to_string(workspace().join(f)).unwrap_or_else(|e| panic!("{f}: {e}"))
}

/// What `lib.rs` is opened with: the file and a function named against
/// Rust's custom, which rust-analyzer itself flags (`non_snake_case`), and
/// cargo never sees (it is not on the disk).
const BAD_NAME: &str = "\n/// Named against the custom.\npub fn BadName() {}\n";

/// The position of `word` in `line` (the first line holding it) of `f`,
/// `offset` bytes into the word.
fn at(s: &Server, f: &str, line: &str, word: &str, offset: usize) -> Value {
    let t = text(f);
    let start = t.find(line).unwrap_or_else(|| panic!("{f}: {line}"));
    let byte = start + line.find(word).expect("word in line") + offset;
    position(&t, byte, s.enc).to_json()
}

/// Asks until the answer is one `ok` takes, rust-analyzer answering
/// nothing (or cancelling) while it loads the project; at most `secs`.
fn ask(s: &Server, method: &str, params: Value, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let r = s
            .client
            .request(method, params.clone())
            .wait(Duration::from_secs(30));
        if let Ok(v) = &r
            && ok(v)
        {
            return v.clone();
        }
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "{method}: {r:?} after {}s",
            t.elapsed().as_secs()
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn start(program: String, m: &Value) -> Server {
    let target = std::env::temp_dir().join(format!("kalem-rust-server-{}", std::process::id()));
    // The manifest's settings, and a target folder of the test's own so
    // that nothing is written into the corpus.
    let mut settings = m["servers"]["rust-analyzer"]["settings"].clone();
    settings["rust-analyzer"]["cargo"]["targetDir"] = json!(target);
    let client = Client::start(
        ServerConfig {
            name: "rust-analyzer".into(),
            command: program.into(),
            root: workspace(),
            settings,
            ..ServerConfig::default()
        },
        Arc::new(|| {}),
    )
    .expect("rust-analyzer starts");
    let t = Instant::now();
    while !client.is_ready() {
        assert!(t.elapsed() < Duration::from_secs(60), "never ready");
        std::thread::sleep(Duration::from_millis(50));
    }
    for f in [
        "app/src/main.rs",
        "shapes/src/lib.rs",
        "shapes/src/messy.rs",
    ] {
        let mut t = text(f);
        if f == "shapes/src/lib.rs" {
            t.push_str(BAD_NAME);
        }
        client.did_open(&uri(f), "rust", &t);
    }
    let enc = client.encoding();
    Server {
        client,
        enc,
        target,
    }
}

#[test]
fn the_corpus_through_rust_analyzer() {
    let m: Value = serde_json::from_str(MANIFEST).expect("plugin.json parses");
    let Some(program) = rust_analyzer(&m) else {
        eprintln!("rust-analyzer does not run here: the corpus test skipped");
        return;
    };
    let s = start(program, &m);
    let main = "app/src/main.rs";
    let lib = "shapes/src/lib.rs";
    let doc = |f: &str| json!({ "uri": uri(f) });

    // Documentation at the cursor: the declaration and the rustdoc.
    let hover = ask(
        &s,
        "textDocument/hover",
        json!({ "textDocument": doc(main), "position": at(&s, main, "Circle::new(1.0)", "Circle", 1) }),
        180,
        |v| features::hover_text(v).is_some_and(|t| t.contains("A circle.")),
    );
    let hover = features::hover_text(&hover).expect("hover");
    assert!(hover.contains("pub struct Circle"), "{hover}");

    // Completion after `.` (before a call's parentheses, so methods only):
    // the inherent method, and the trait's, imported.
    let completion = ask(
        &s,
        "textDocument/completion",
        json!({ "textDocument": doc(main), "position": at(&s, main, "Circle::new(1.0).scaled", ".scaled", 1) }),
        60,
        |v| !features::completion_items(v, false).0.is_empty(),
    );
    let (items, _) = features::completion_items(&completion, false);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    for want in ["scaled", "area"] {
        assert!(
            labels.iter().any(|l| l.split('(').next() == Some(want)),
            "{want} in {labels:?}"
        );
    }
    let scaled = items.iter().find(|i| i.label == "scaled").expect("scaled");
    assert_eq!(
        scaled.documentation.as_deref(),
        Some("The circle `factor` times as wide.")
    );

    // Definition across the crates.
    let definition = ask(
        &s,
        "textDocument/definition",
        json!({ "textDocument": doc(main), "position": at(&s, main, ".scaled(2.0)", "scaled", 1) }),
        60,
        |v| !features::locations(v).is_empty(),
    );
    let place = &features::locations(&definition)[0];
    assert!(place.path.ends_with(lib), "{}", place.path.display());
    let line = text(lib)
        .lines()
        .position(|l| l.contains("pub fn scaled"))
        .expect("scaled");
    assert_eq!(place.start.line as usize, line);

    // References and implementations of the trait, across the crates.
    let area = at(&s, lib, "pub trait Area", "Area", 1);
    let references = ask(
        &s,
        "textDocument/references",
        json!({ "textDocument": doc(lib), "position": area, "context": { "includeDeclaration": true } }),
        60,
        |v| features::locations(v).len() > 3,
    );
    let files: Vec<PathBuf> = features::locations(&references)
        .into_iter()
        .map(|l| l.path)
        .collect();
    assert!(files.iter().any(|p| p.ends_with(main)), "{files:?}");
    let implementations = ask(
        &s,
        "textDocument/implementation",
        json!({ "textDocument": doc(lib), "position": at(&s, lib, "pub trait Area", "Area", 1) }),
        60,
        |v| !features::locations(v).is_empty(),
    );
    assert_eq!(features::locations(&implementations).len(), 2);

    // Rename across the crates: one edit in each file.
    let rename = ask(
        &s,
        "textDocument/rename",
        json!({ "textDocument": doc(main), "position": at(&s, main, ".scaled(2.0)", "scaled", 1), "newName": "grown" }),
        60,
        |v| !v.is_null(),
    );
    let mut changed: Vec<String> = match (&rename["changes"], &rename["documentChanges"]) {
        (Value::Object(c), _) => c.keys().cloned().collect(),
        (_, Value::Array(d)) => d
            .iter()
            .filter_map(|e| e["textDocument"]["uri"].as_str().map(str::to_string))
            .collect(),
        _ => panic!("{rename}"),
    };
    changed.sort();
    let mut want = vec![uri(main), uri(lib)];
    want.sort();
    assert_eq!(changed, want);

    // Formatting: the unformatted module as rustfmt formats it, the
    // workspace's `rustfmt.toml` read.
    let messy = "shapes/src/messy.rs";
    let format = ask(
        &s,
        "textDocument/formatting",
        json!({ "textDocument": doc(messy), "options": { "tabSize": 4, "insertSpaces": true } }),
        60,
        |v| v.as_array().is_some_and(|a| !a.is_empty()),
    );
    let before = text(messy);
    let edits = features::text_edits(&before, &format, s.enc).expect("edits");
    let formatted = features::apply(&before, &edits);
    assert!(formatted.contains("Point { x, y }"), "{formatted}");
    let rustfmt = std::process::Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .arg(workspace().join(messy))
        .output()
        .expect("rustfmt runs");
    let by_path = String::from_utf8(rustfmt.stdout).expect("UTF-8");
    let header = format!("{}:\n\n", workspace().join(messy).display());
    assert_eq!(formatted, by_path.strip_prefix(&header).unwrap_or(&by_path));

    // cargo's diagnostics: the borrow error of the test file, which no
    // editor has open, from the check rust-analyzer runs at its start.
    let borrow = uri("app/tests/borrow.rs");
    let t = Instant::now();
    loop {
        let codes: Vec<String> = s
            .client
            .diagnostics(&borrow)
            .iter()
            .filter_map(|d| d["code"].as_str().map(str::to_string))
            .collect();
        if codes.iter().any(|c| c == "E0502") {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(180),
            "no borrow error: {codes:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    // rust-analyzer's own diagnostics, which it gives only when asked
    // (the pull model): the function named against the custom, on the
    // line it has in the text the editor sent.
    let lib_uri = uri(lib);
    let bad_line = text(lib).lines().count() + 2;
    let t = Instant::now();
    loop {
        let own: Vec<(String, u64)> = s
            .client
            .diagnostics(&lib_uri)
            .iter()
            .filter(|d| d["source"] == "rust-analyzer")
            .filter_map(|d| {
                Some((
                    d["code"].as_str()?.to_string(),
                    d["range"]["start"]["line"].as_u64()?,
                ))
            })
            .collect();
        if own.contains(&("non_snake_case".to_string(), bad_line as u64)) {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(120),
            "no non_snake_case at line {bad_line}: {own:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}
