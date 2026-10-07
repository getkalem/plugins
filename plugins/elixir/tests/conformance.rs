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

/// HEEx and EEx extend Sublime Text's HTML: loaded as Kalem loads them,
/// `extends` resolved by Kalem's highlighter, they highlight a template.
#[test]
fn templates_through_kalem() {
    let m = manifest();
    let mut sources = Vec::new();
    for (key, base_only) in [("syntaxes", false), ("syntaxBases", true)] {
        for f in list(&m[key]) {
            sources.push(kalem_highlight::SyntaxSource {
                file: f.rsplit('/').next().unwrap_or(&f).to_string(),
                text: std::fs::read_to_string(dir().join(&f)).expect("syntax"),
                base_only,
            });
        }
    }
    let r = kalem_highlight::register(&sources);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let heex = kalem_highlight::Language::find("heex").expect("HEEx");
    assert_eq!(heex.name(), "HTML (HEEx)");
    let spans = kalem_highlight::highlight(
        heex,
        "<div class={@cls}><.button>Hi <%= @name %></.button></div>\n",
    );
    let kinds: Vec<kalem_highlight::Kind> = spans[0].iter().map(|s| s.kind).collect();
    use kalem_highlight::Kind::{Function, Tag, Variable};
    assert!(kinds.contains(&Tag), "{kinds:?}");
    assert!(kinds.contains(&Function), "the component: {kinds:?}");
    assert!(kinds.contains(&Variable), "the assigns: {kinds:?}");
    // `~H` inside Elixir is HEEx too.
    let ex = kalem_highlight::Language::find("ex").expect("Elixir");
    let spans = kalem_highlight::highlight(ex, "~H\"\"\"\n<div>{@x}</div>\n\"\"\"\n");
    assert!(spans[1].iter().any(|s| s.kind == Tag), "{:?}", spans[1]);
    // `<%!-- --%>`, the comment the manifest gives HEEx and EEx, is a
    // comment whatever it holds, in both and in `~H`.
    let eex = kalem_highlight::Language::find("HTML (EEx)").expect("EEx");
    for (lang, text) in [
        (heex, "<p><%!-- <.x>{@a}</.x> --%></p>\n"),
        (eex, "<p><%!-- <%= f(@a) %> --%></p>\n"),
        (ex, "~H\"<p><%!-- {@a} --%></p>\"\n"),
    ] {
        let line = &kalem_highlight::highlight(lang, text)[0];
        let start = text.find("<%!--").expect("comment");
        let end = text.find("--%>").expect("comment") + 4;
        let comment = line
            .iter()
            .find(|s| s.range.start == start)
            .unwrap_or_else(|| panic!("{text}: {line:?}"));
        assert_eq!(comment.kind, kalem_highlight::Kind::Comment, "{text}");
        assert!(comment.range.end >= end, "{text}: {line:?}");
    }
    let comments: Vec<&Value> = m["languages"]
        .as_array()
        .expect("languages")
        .iter()
        .filter(|l| l["id"] != "elixir")
        .map(|l| &l["comment"]["block"])
        .collect();
    assert!(
        comments.iter().all(|c| list(c) == ["<%!--", "--%>"]),
        "{comments:?}"
    );
    // `.html` stays the built-in HTML: the bases are not languages.
    assert_ne!(
        kalem_highlight::Language::find("html").map(|l| l.name()),
        Some("HTML (Plain)")
    );
    // Every language's syntax, by the name the manifest gives (Kalem maps
    // the language's extensions to it).
    for l in m["languages"].as_array().expect("languages") {
        let syntax = l["syntax"].as_str().expect("syntax");
        let found = kalem_highlight::Language::find(syntax).map(|x| x.name());
        assert_eq!(found, Some(syntax), "{}", l["id"]);
    }
}
