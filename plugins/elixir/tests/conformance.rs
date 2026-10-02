//! The plugin's conformance: the manifest names what exists, and the
//! syntaxes load and highlight Elixir as Kalem's highlighter does.
//! `HTML (EEx)` and `HTML (HEEx)` extend Sublime Text's HTML, which syntect
//! does not resolve; Kalem resolves `extends` itself, and its tests cover
//! them with this plugin's files.

use std::path::Path;

use serde_json::Value;
use syntect::parsing::{ParseState, ScopeStack, SyntaxDefinition, SyntaxSet};

const MANIFEST: &str = include_str!("../plugin.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("plugin.json parses")
}

fn dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
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

#[test]
fn manifest_is_complete() {
    let m = manifest();
    for key in ["id", "name", "version", "description", "api"] {
        assert!(m[key].is_string(), "`{key}` is a string");
    }
    assert_eq!(m["id"], "org.kalem.elixir");
    let servers = m["servers"].as_object().expect("servers");
    for (key, s) in servers {
        assert!(!list(&s["command"]).is_empty(), "{key} has a command");
        assert!(s["install"].is_string(), "{key} says how to install it");
        assert_eq!(
            list(&s["rootMarkers"]),
            ["mix.exs"],
            "{key}: the Mix project is the root"
        );
    }
    let names: Vec<String> = list(&m["syntaxes"])
        .iter()
        .map(|f| {
            let text =
                std::fs::read_to_string(dir().join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
            text.lines()
                .find_map(|l| l.strip_prefix("name: "))
                .unwrap_or_else(|| panic!("{f} has a name"))
                .trim()
                .to_string()
        })
        .collect();
    for f in list(&m["syntaxBases"]) {
        assert!(dir().join(&f).is_file(), "{f}");
    }
    for l in m["languages"].as_array().expect("languages") {
        let id = l["id"].as_str().expect("id");
        let syntax = l["syntax"].as_str().expect("syntax");
        assert!(
            names.iter().any(|n| n == syntax),
            "{id}: syntax {syntax} is shipped"
        );
        assert!(!list(&l["extensions"]).is_empty(), "{id} has extensions");
        for s in list(&l["servers"]) {
            assert!(servers.contains_key(&s), "{id}: server {s} is described");
        }
    }
}

/// The syntaxes that extend none, added to syntect's.
fn set() -> SyntaxSet {
    let mut b = SyntaxSet::load_defaults_newlines().into_builder();
    for f in list(&manifest()["syntaxes"]) {
        let text = std::fs::read_to_string(dir().join(&f)).expect("syntax");
        if text.lines().any(|l| l.starts_with("extends:")) {
            continue;
        }
        b.add(
            SyntaxDefinition::load_from_str(&text, true, None)
                .unwrap_or_else(|e| panic!("{f}: {e}")),
        );
    }
    b.build()
}

/// The scopes of `word` on `line` of `text`.
fn scopes_of(set: &SyntaxSet, text: &str, line: usize, word: &str) -> String {
    let syntax = set.find_syntax_by_extension("ex").expect("Elixir");
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    for (i, l) in text.split_inclusive('\n').enumerate() {
        let ops = state.parse_line(l, set).expect("parses");
        let at = l.find(word).filter(|_| i == line);
        let mut pos_stack = None;
        for (pos, op) in ops {
            if let Some(a) = at
                && pos > a
                && pos_stack.is_none()
            {
                pos_stack = Some(format!("{stack:?}"));
            }
            stack.apply(&op).expect("scope op");
        }
        if let Some(s) = pos_stack.or_else(|| at.map(|_| format!("{stack:?}"))) {
            return s;
        }
    }
    panic!("{word} not on line {line}")
}

#[test]
fn elixir_is_highlighted() {
    let set = set();
    let text = std::fs::read_to_string(dir().join("corpus/hello/lib/hello.ex")).expect("corpus");
    assert!(scopes_of(&set, &text, 0, "defmodule").contains("keyword.declaration.module.elixir"));
    assert!(scopes_of(&set, &text, 4, "greet").contains("entity.name.function.elixir"));
    assert!(scopes_of(&set, &text, 5, "Hello,").contains("string.quoted.double.elixir"));
    // Every corpus file parses to its end with the scopes balanced.
    for f in [
        "lib/hello.ex",
        "lib/shout.ex",
        "test/hello_test.exs",
        "mix.exs",
    ] {
        let text = std::fs::read_to_string(dir().join("corpus/hello").join(f)).expect("corpus");
        let syntax = set.find_syntax_by_extension("ex").expect("Elixir");
        let mut state = ParseState::new(syntax);
        for l in text.split_inclusive('\n') {
            state
                .parse_line(l, &set)
                .unwrap_or_else(|e| panic!("{f}: {e}"));
        }
    }
}
