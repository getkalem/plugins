//! The plugin's conformance: the manifest names what exists; its syntax,
//! Sublime Text's current Rust, loads as Kalem loads it, takes `.rs` and
//! `rust` from the older one built into Kalem, and parses and colors the
//! corpus; and the corpus is the Cargo workspace and the files the
//! plugin's other tests rely on.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use kalem_highlight::{Kind, Language, Span, SyntaxSource};
use serde_json::Value;
use syntect::parsing::{ParseState, SyntaxDefinition, SyntaxSet};

const MANIFEST: &str = include_str!("../plugin.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("plugin.json parses")
}

fn dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The corpus's Cargo workspace.
fn workspace() -> PathBuf {
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

/// The `.rs` files under `d`, `target/` left out.
fn rs_files(d: &Path) -> Vec<PathBuf> {
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
    walk(d, &mut out);
    out.sort();
    out
}

fn read(f: &str) -> String {
    std::fs::read_to_string(dir().join(f)).unwrap_or_else(|e| panic!("{f}: {e}"))
}

/// The program `name` of the toolchain running the tests.
fn tool(name: &str) -> Command {
    let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or("cargo".into()));
    let beside = cargo.with_file_name(name);
    Command::new(if beside.is_file() {
        beside
    } else {
        name.into()
    })
}

#[test]
fn manifest_is_complete() {
    let m = manifest();
    for key in ["id", "name", "version", "description", "api"] {
        assert!(m[key].is_string(), "`{key}` is a string");
    }
    assert_eq!(m["id"], "org.kalem.rust");
    assert!(m.get("main").is_none(), "declarative: no component");
    assert_eq!(list(&m["permissions"]), ["subprocess"]);
    let names: Vec<String> = list(&m["syntaxes"])
        .iter()
        .map(|f| {
            read(f)
                .lines()
                .find_map(|l| l.strip_prefix("name: "))
                .unwrap_or_else(|| panic!("{f} has a name"))
                .trim()
                .to_string()
        })
        .collect();
    assert_eq!(names, ["Rust"]);
    assert!(dir().join("syntaxes/LICENSE-sublimehq-rust.txt").is_file());
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
    for l in m["languages"].as_array().expect("languages") {
        let id = l["id"].as_str().expect("id");
        let syntax = l["syntax"].as_str().expect("syntax");
        assert!(
            names.iter().any(|n| n == syntax),
            "{id}: syntax {syntax} is shipped"
        );
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
    let out = tool("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace().join("app"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: Value = serde_json::from_slice(&out.stdout).expect("metadata");
    let root = PathBuf::from(meta["workspace_root"].as_str().expect("root"));
    assert_eq!(root, workspace(), "the corpus is a workspace of its own");
    let mut names: Vec<&str> = meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["app", "shapes"]);
    for f in rs_files(&workspace()) {
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
        tool("cargo")
            .args(args)
            .env("CARGO_TARGET_DIR", &target)
            .current_dir(workspace())
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

/// `corpus/edition.rs` is Rust as the current edition writes it: it
/// compiles, without a warning.
#[test]
fn the_edition_file_compiles() {
    let out_dir = std::env::temp_dir().join(format!("kalem-rust-edition-{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).expect("temporary folder");
    let out = tool("rustc")
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "lib",
            "--emit",
            "metadata",
        ])
        .arg("--out-dir")
        .arg(&out_dir)
        .arg(dir().join("corpus/edition.rs"))
        .output()
        .expect("rustc runs");
    let _ = std::fs::remove_dir_all(&out_dir);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && stderr.trim().is_empty(), "{stderr}");
}

/// The plugin's syntaxes registered as Kalem registers them, once for
/// the tests of this binary (registering replaces Kalem's set), and the
/// language of `.rs` files after it.
fn rust() -> Language {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        let sources: Vec<SyntaxSource> = list(&manifest()["syntaxes"])
            .iter()
            .map(|f| SyntaxSource {
                file: f.rsplit('/').next().unwrap_or(f).to_string(),
                text: read(f),
                base_only: false,
            })
            .collect();
        let r = kalem_highlight::register(&sources);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    });
    Language::find("rs").expect("Rust")
}

/// The span at the start of `word` on the first line of `text` holding
/// `line`, and its text.
fn span_at<'t>(text: &'t str, line: &str, word: &str) -> (Kind, &'t str) {
    let n = text
        .lines()
        .position(|l| l.contains(line))
        .unwrap_or_else(|| panic!("no line with {line}"));
    let l = text.lines().nth(n).expect("line");
    let at = l.find(line).expect("line") + line.find(word).expect("word in line");
    let spans: Vec<Span> = kalem_highlight::highlight(rust(), text).swap_remove(n);
    spans
        .iter()
        .find(|s| s.range.contains(&at))
        .map(|s| (s.kind, &l[s.range.clone()]))
        .unwrap_or_else(|| panic!("{word} in {l:?}: no span in {spans:?}"))
}

fn kind_at(text: &str, line: &str, word: &str) -> Kind {
    span_at(text, line, word).0
}

/// The plugin's Rust is the one `.rs` files, `rust` source blocks and
/// the language's name find: it colors `async`, which the built-in one
/// does not.
#[test]
fn the_syntax_takes_rust_from_the_built_in_one() {
    let lang = rust();
    let m = manifest();
    let syntax = m["languages"][0]["syntax"].as_str().expect("syntax");
    for name in ["rs", "rust", syntax] {
        let found = Language::find(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(found.name(), "Rust");
        let line = &kalem_highlight::highlight(found, "async fn f() {}\n")[0];
        assert_eq!(line[0].kind, Kind::Keyword, "{name}: {line:?}");
        assert_eq!(line[0].range, 0..5, "{name}: `async`");
    }
    assert_eq!(lang.name(), syntax);
}

/// Every corpus file parses to its end without an error, through syntect
/// as Kalem builds its set (a TOML stand-in for the frontmatter's
/// embedding where syntect's own set has none).
#[test]
fn the_syntax_parses_every_corpus_file() {
    let mut b = SyntaxSet::load_defaults_newlines().into_builder();
    if !b
        .syntaxes()
        .iter()
        .any(|s| s.scope.to_string() == "source.toml")
    {
        b.add(
            SyntaxDefinition::load_from_str(
                "name: TOML\nscope: source.toml\ncontexts:\n  main:\n    - match: '.'\n",
                true,
                None,
            )
            .expect("TOML stand-in"),
        );
    }
    for f in list(&manifest()["syntaxes"]) {
        b.add(SyntaxDefinition::load_from_str(&read(&f), true, None).expect("syntax loads"));
    }
    let set = b.build();
    let syntax = set
        .syntaxes()
        .iter()
        .rev()
        .find(|s| s.name == "Rust")
        .expect("Rust");
    let files = rs_files(&dir().join("corpus"));
    assert_eq!(files.len(), 5, "{files:?}");
    for f in files {
        let text = std::fs::read_to_string(&f).expect("corpus");
        let mut state = ParseState::new(syntax);
        for (i, l) in text.split_inclusive('\n').enumerate() {
            state
                .parse_line(l, &set)
                .unwrap_or_else(|e| panic!("{}:{}: {e}", f.display(), i + 1));
        }
        // Back to Rust at its end: an item after it is colored as one (a
        // string, comment or context left open would swallow it).
        let after = format!("{text}fn after_the_file() {{}}\n");
        let lines = kalem_highlight::highlight(rust(), &after);
        let last = &lines[lines.len() - 2];
        assert_eq!(
            last.iter().map(|s| s.kind).collect::<Vec<_>>(),
            [Kind::Keyword, Kind::Function],
            "{}",
            f.display()
        );
    }
}

#[test]
fn the_workspace_is_highlighted() {
    let lib = read("corpus/ws/shapes/src/lib.rs");
    assert_eq!(kind_at(&lib, "//! Shapes", "//!"), Kind::Comment);
    assert_eq!(kind_at(&lib, "pub trait Area", "pub"), Kind::Keyword);
    assert_eq!(kind_at(&lib, "fn area(&self)", "fn"), Kind::Keyword);
    assert_eq!(kind_at(&lib, "fn area(&self)", "area"), Kind::Function);
    assert_eq!(kind_at(&lib, "$crate::Square", "$crate"), Kind::Variable);
    let main = read("corpus/ws/app/src/main.rs");
    assert_eq!(kind_at(&main, "fn total", "total"), Kind::Function);
    assert_eq!(kind_at(&main, "let circle", "circle"), Kind::Variable);
    assert_eq!(kind_at(&main, "Circle::new(1.0)", "1.0"), Kind::Number);
    assert_eq!(kind_at(&main, "square!(3.0)", "square!"), Kind::Macro);
    assert_eq!(kind_at(&main, "println!(\"", "\""), Kind::String);
}

/// What the built-in Rust mis-colors, as the current editions write it.
#[test]
fn the_current_editions_are_highlighted() {
    let t = read("corpus/edition.rs");
    // `async` and `.await` (the built-in one leaves both plain).
    assert_eq!(kind_at(&t, "async fn load", "async"), Kind::Keyword);
    assert_eq!(kind_at(&t, "add(1).await", "await"), Kind::Keyword);
    assert_eq!(kind_at(&t, "async |x: u32|", "async"), Kind::Keyword);
    // A float's exponent with its sign (the built-in one: `-` an operator).
    assert_eq!(span_at(&t, "1e-3", "1e-3"), (Kind::Number, "1e-3"));
    // Raw identifiers (the built-in one: `r`, `#`, `gen` apart).
    assert_eq!(span_at(&t, "fn r#gen", "r#gen"), (Kind::Function, "r#gen"));
    assert_eq!(
        span_at(&t, "r#type: u32", "r#type"),
        (Kind::Variable, "r#type")
    );
    // A named width in a format string.
    assert_eq!(
        span_at(&t, "{name:>width$}", "{name"),
        (Kind::Constant, "{name:>width$}")
    );
    // Every fragment specifier (the built-in one misses four).
    for spec in ["pat_param", "lifetime", "literal", "vis"] {
        let at = format!(":{spec}");
        assert_eq!(span_at(&t, &at, spec), (Kind::Keyword, spec), "{spec}");
    }
    // `union`, a contextual keyword.
    assert_eq!(kind_at(&t, "pub union Bits", "union"), Kind::Keyword);
    // C strings and raw C strings, prefix and text.
    assert_eq!(kind_at(&t, "c\"hello\"", "c"), Kind::Keyword);
    assert_eq!(kind_at(&t, "c\"hello\"", "\"hello\""), Kind::String);
    assert_eq!(
        span_at(&t, "cr#\"a", "#\"a"),
        (Kind::String, "#\"a \"quoted\" word\"#")
    );
    // Lifetimes in precise capturing, a let chain's bindings.
    assert_eq!(kind_at(&t, "use<'a>", "'a"), Kind::Keyword);
    assert_eq!(kind_at(&t, "&& *x % 2", "x"), Kind::Variable);
    // A single-file package: the `#!` line and the TOML frontmatter,
    // then Rust again.
    let s = read("corpus/script.rs");
    assert_eq!(kind_at(&s, "#!/usr/bin/env", "#!"), Kind::Comment);
    assert_eq!(
        kind_at(&s, "#!/usr/bin/env -S cargo", "cargo"),
        Kind::Constant
    );
    assert_eq!(kind_at(&s, "edition = ", "edition"), Kind::Variable);
    assert_eq!(kind_at(&s, "edition = \"2024\"", "\"2024\""), Kind::String);
    assert_eq!(kind_at(&s, "fn main", "fn"), Kind::Keyword);
}
