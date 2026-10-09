//! The plugin's conformance: the manifest names what exists, the syntax it
//! names is Kalem's own and colors the corpus, and the corpus is the Cargo
//! workspace the plugin's other tests rely on.

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_highlight::{Kind, Language};
use serde_json::Value;

const MANIFEST: &str = include_str!("../plugin.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("plugin.json parses")
}

fn dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn corpus() -> PathBuf {
    dir().join("corpus/ws")
}

fn list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The `.rs` files of the corpus.
fn corpus_files() -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).expect("corpus folder") {
            let p = e.expect("entry").path();
            if p.is_dir() && p.file_name().is_some_and(|n| n != "target") {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&corpus(), &mut out);
    out.sort();
    out
}

#[test]
fn manifest_is_complete() {
    let m = manifest();
    for key in ["id", "name", "version", "description", "api"] {
        assert!(m[key].is_string(), "`{key}` is a string");
    }
    assert_eq!(m["id"], "org.kalem.rust");
    assert!(m.get("main").is_none(), "declarative: no component");
    assert!(m.get("syntaxes").is_none(), "Rust's syntax is Kalem's own");
    assert_eq!(list(&m["permissions"]), ["subprocess"]);
    let servers = m["servers"].as_object().expect("servers");
    for (key, s) in servers {
        assert!(!list(&s["command"]).is_empty(), "{key} has a command");
        assert!(s["install"].is_string(), "{key} says how to install it");
        // rust-analyzer asks for its settings as the section
        // `rust-analyzer` of `workspace/configuration`, which Kalem
        // answers from `settings` by that key.
        assert!(
            s["settings"]["rust-analyzer"].is_object(),
            "{key}: settings under `rust-analyzer`"
        );
    }
    let languages = m["languages"].as_array().expect("languages");
    for l in languages {
        let id = l["id"].as_str().expect("id");
        assert!(!list(&l["extensions"]).is_empty(), "{id} has extensions");
        assert!(
            list(&m["activation"]).contains(&format!("onLanguage:{id}")),
            "{id} activates the plugin"
        );
        for s in list(&l["servers"]) {
            assert!(servers.contains_key(&s), "{id}: server {s} is described");
        }
    }
}

/// The root is the nearest folder holding a `Cargo.lock`: Cargo writes
/// it where it puts the workspace's root, so a workspace is one root and
/// one server, and a workspace inside another (this corpus inside the
/// plugins repository) is a root of its own. Checked against what
/// `cargo metadata` says the corpus's root is.
#[test]
fn the_root_is_cargos_workspace_root() {
    let m = manifest();
    let s = &m["servers"]["rust-analyzer"];
    assert_eq!(list(&s["rootMarkers"]), ["Cargo.lock"]);
    assert_ne!(s["rootOutermost"], true, "the nearest, not the outermost");
    let out = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(corpus().join("app"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: Value = serde_json::from_slice(&out.stdout).expect("metadata");
    let root = PathBuf::from(meta["workspace_root"].as_str().expect("root"));
    assert_eq!(root, corpus(), "the corpus is a workspace of its own");
    let mut names: Vec<&str> = meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["app", "shapes"]);
    for f in corpus_files() {
        let found = f
            .ancestors()
            .skip(1)
            .find(|d| list(&s["rootMarkers"]).iter().any(|k| d.join(k).exists()));
        assert_eq!(found, Some(root.as_path()), "{}", f.display());
    }
}

/// The corpus builds, its test passes, and the one error is the borrow
/// error of `app/tests/borrow.rs`, which only a check of every target
/// (rust-analyzer's default) sees.
#[test]
fn the_corpus_compiles_but_for_one_borrow_error() {
    let target = std::env::temp_dir().join(format!("kalem-rust-corpus-{}", std::process::id()));
    let cargo = |args: &[&str]| {
        Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
            .args(args)
            .env("CARGO_TARGET_DIR", &target)
            .current_dir(corpus())
            .output()
            .expect("cargo runs")
    };
    let check = cargo(&["check", "--workspace", "--offline"]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let test = cargo(&["test", "--offline", "-p", "app", "--bin", "app"]);
    assert!(
        test.status.success(),
        "{}",
        String::from_utf8_lossy(&test.stderr)
    );
    let all = cargo(&[
        "check",
        "--workspace",
        "--all-targets",
        "--offline",
        "--message-format",
        "json",
    ]);
    let _ = std::fs::remove_dir_all(&target);
    assert!(!all.status.success(), "the borrow error is there");
    let errors: Vec<(String, String)> = String::from_utf8_lossy(&all.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-message" && v["message"]["level"] == "error")
        .filter_map(|v| {
            let code = v["message"]["code"]["code"].as_str()?.to_string();
            let file = v["message"]["spans"][0]["file_name"].as_str()?.to_string();
            Some((code, file))
        })
        .collect();
    assert_eq!(
        errors,
        [("E0502".to_string(), "app/tests/borrow.rs".to_string())]
    );
}

/// The language's syntax is the one Kalem has built in, by the name the
/// manifest gives and by the extension.
#[test]
fn the_syntax_is_kalems() {
    let m = manifest();
    for l in m["languages"].as_array().expect("languages") {
        let syntax = l["syntax"].as_str().expect("syntax");
        let found = Language::find(syntax).map(Language::name);
        assert_eq!(found, Some(syntax), "{}", l["id"]);
        for ext in list(&l["extensions"]) {
            assert_eq!(Language::find(&ext).map(Language::name), Some(syntax));
        }
    }
}

/// The kind of the span at `word` on line `line` (0-based) of
/// `text`.
fn kind_at(text: &str, line: usize, word: &str) -> Kind {
    let rust = Language::find("rs").expect("Rust");
    let lines = kalem_highlight::highlight(rust, text);
    let l = text.lines().nth(line).expect("line");
    let at = l
        .find(word)
        .unwrap_or_else(|| panic!("{word} not on line {line}"));
    lines[line]
        .iter()
        .find(|s| s.range.contains(&at))
        .map(|s| s.kind)
        .unwrap_or_else(|| panic!("{word}: no span in {:?}", lines[line]))
}

#[test]
fn the_corpus_is_highlighted() {
    let files = corpus_files();
    assert_eq!(files.len(), 3, "{files:?}");
    let lib = std::fs::read_to_string(corpus().join("shapes/src/lib.rs")).expect("lib.rs");
    assert_eq!(kind_at(&lib, 0, "//!"), Kind::Comment);
    assert_eq!(kind_at(&lib, 4, "pub"), Kind::Keyword);
    assert_eq!(kind_at(&lib, 6, "fn"), Kind::Keyword);
    assert_eq!(kind_at(&lib, 6, "area"), Kind::Function);
    let main = std::fs::read_to_string(corpus().join("app/src/main.rs")).expect("main.rs");
    assert_eq!(kind_at(&main, 5, "total"), Kind::Function);
    assert_eq!(kind_at(&main, 11, "square!"), Kind::Macro);
    assert_eq!(kind_at(&main, 10, "1.0"), Kind::Number);
    assert_eq!(kind_at(&main, 12, "\"{:.2}\""), Kind::String);
    // Every file is colored to its end: a string or comment left open
    // would leave the last line one.
    let rust = Language::find("rs").expect("Rust");
    for f in files {
        let text = std::fs::read_to_string(&f).expect("corpus");
        let lines = kalem_highlight::highlight(rust, &text);
        let last = lines.iter().rev().find(|l| !l.is_empty()).expect("spans");
        assert!(
            last.iter()
                .all(|s| !matches!(s.kind, Kind::String | Kind::Comment)),
            "{}: {last:?}",
            f.display()
        );
    }
}
