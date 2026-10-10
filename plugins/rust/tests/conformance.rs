//! The plugin's conformance: the manifest names what exists; its syntax,
//! Sublime Text's current Rust, loads as Kalem loads it, takes `.rs` and
//! `rust` from the older one built into Kalem, and parses and colors the
//! corpus; and the corpus is the Cargo workspace and the files the
//! plugin's other tests rely on.

// A comparison skipped (rust-analyzer not installed where the tests run)
// is said on standard error.
#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use kalem_highlight::{Kind, Language, Span, SyntaxSource};
use serde_json::{Value, json};
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
    }
    let ra = &servers["rust-analyzer"];
    // rust-analyzer asks for its settings as the section `rust-analyzer`
    // of `workspace/configuration`, which Kalem answers from `settings`
    // by that key.
    assert!(ra["settings"]["rust-analyzer"].is_object());
    // `kalem lsp status` runs it: rustup's proxy is found whether or not
    // the component is installed, and only running it tells.
    assert_eq!(list(&ra["version"]), ["--version"]);
    for l in m["languages"].as_array().expect("languages") {
        let id = l["id"].as_str().expect("id");
        let syntax = l["syntax"].as_str().expect("syntax");
        // Rust's syntax is the plugin's; a `Cargo.toml` is highlighted by
        // Kalem's own TOML.
        assert!(
            names.iter().any(|n| n == syntax) || (id, syntax) == ("toml", "TOML"),
            "{id}: syntax {syntax} is shipped"
        );
        let claimed = [list(&l["extensions"]), list(&l["filenames"])].concat();
        assert!(!claimed.is_empty(), "{id} claims files");
        assert!(
            list(&m["activation"]).contains(&format!("onLanguage:{id}")),
            "{id} activates the plugin"
        );
        for s in [list(&l["servers"]), list(&l["alongside"])].concat() {
            assert!(servers.contains_key(&s), "{id}: server {s} is described");
        }
        // Of TOML, `Cargo.toml` only, by its name: another `.toml` file
        // and `Cargo.lock` stay Kalem's TOML.
        assert!(
            list(&l["extensions"]).iter().all(|e| !e.ends_with("toml"))
                && list(&l["filenames"]).iter().all(|f| f == "Cargo.toml"),
            "{id} claims {claimed:?}"
        );
    }
}

/// `Cargo.toml` by two servers beside each other (RS7a): Taplo for its
/// keys, documented and completed from SchemaStore's Cargo schema given
/// by name (Taplo 0.10 cannot read SchemaStore's catalog), with
/// crates-lsp beside it for the crates' versions.
#[test]
fn cargo_toml_is_taplos_with_crates_lsp_beside_it() {
    let m = manifest();
    let cargo = m["languages"]
        .as_array()
        .expect("languages")
        .iter()
        .find(|l| list(&l["filenames"]).contains(&"Cargo.toml".to_string()))
        .expect("a language for Cargo.toml");
    assert_eq!(cargo["id"], "toml");
    assert_eq!(list(&cargo["filenames"]), ["Cargo.toml"]);
    assert!(list(&cargo["extensions"]).is_empty());
    assert_eq!(list(&cargo["servers"]), ["taplo"]);
    assert_eq!(list(&cargo["alongside"]), ["crates-lsp"]);
    let taplo = &m["servers"]["taplo"];
    assert_eq!(list(&taplo["command"]), ["taplo", "lsp", "stdio"]);
    let schema = &taplo["settings"]["evenBetterToml"]["schema"];
    assert_eq!(schema["enabled"], true);
    assert_eq!(
        schema["associations"]["Cargo\\.toml$"],
        "https://json.schemastore.org/cargo.json"
    );
    assert_eq!(list(&m["servers"]["crates-lsp"]["command"]), ["crates-lsp"]);
}

/// The settings Kalem's settings panel shows: rust-analyzer's, keyed as
/// the section it asks for (`settings.rust-analyzer.check.command` is
/// `{"rust-analyzer": {"check": {"command": …}}}`, nested as
/// rust-analyzer reads it), typed as the panel knows (a union of types
/// is edited as JSON), each with a default and a description; what the
/// manifest sends has its value as default. Against rust-analyzer's own
/// schema too when it runs here: each key one of its settings, with its
/// default and its choices.
#[test]
fn settings_describe_what_rust_analyzer_reads() {
    let m = manifest();
    let sent = &m["servers"]["rust-analyzer"]["settings"];
    let settings = m["settings"].as_object().expect("settings");
    assert!(!settings.is_empty());
    let types = ["boolean", "integer", "string", "array", "object"];
    for (key, s) in settings {
        let name = key
            .strip_prefix("settings.rust-analyzer.")
            .unwrap_or_else(|| panic!("`{key}` is rust-analyzer's"));
        let union = list(&s["type"]);
        match &s["type"] {
            Value::String(t) => assert!(types.contains(&t.as_str()), "`{key}`: {t}"),
            Value::Array(_) => assert!(
                union.len() > 1
                    && union
                        .iter()
                        .all(|t| t == "null" || types.contains(&t.as_str())),
                "`{key}`: {union:?}"
            ),
            t => panic!("`{key}`: type {t}"),
        }
        let default = s
            .get("default")
            .unwrap_or_else(|| panic!("`{key}` has a default"));
        assert!(
            !default.is_null() || union.iter().any(|t| t == "null"),
            "`{key}`: a null default is one of its types"
        );
        assert!(
            s["description"]
                .as_str()
                .is_some_and(|d| d.starts_with("rust-analyzer: ")),
            "`{key}` says it is rust-analyzer's"
        );
        if let Some(choices) = s["enum"].as_array() {
            assert!(choices.contains(default), "`{key}`'s default is a choice");
        }
        let path: Vec<&str> = std::iter::once("rust-analyzer")
            .chain(name.split('.'))
            .collect();
        let at = path.iter().try_fold(sent, |v, k| v.get(*k));
        if let Some(v) = at {
            assert_eq!(v, default, "`{key}`'s default is what is sent");
        }
    }
    // What the manifest sends is described.
    fn leaves(v: &Value, prefix: &str, out: &mut Vec<String>) {
        match v {
            Value::Object(o) if !o.is_empty() => {
                for (k, x) in o {
                    leaves(x, &format!("{prefix}.{k}"), out);
                }
            }
            Value::Object(_) => {}
            _ => out.push(prefix.to_string()),
        }
    }
    let mut sent_keys = Vec::new();
    leaves(sent, "settings", &mut sent_keys);
    for k in sent_keys {
        assert!(settings.contains_key(&k), "`{k}` is described");
    }
    // rust-analyzer's own word, when it runs here (CI has no component).
    let schema = tool("rust-analyzer")
        .arg("--print-config-schema")
        .current_dir(dir())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok());
    let Some(schema) = schema else {
        eprintln!("rust-analyzer does not run here: its schema not compared");
        return;
    };
    let mut props = serde_json::Map::new();
    for group in schema.as_array().expect("groups") {
        if let Some(o) = group["properties"].as_object() {
            props.extend(o.clone());
        }
    }
    for (key, s) in settings {
        let name = key.replacen("settings.", "", 1);
        let p = props
            .get(&name)
            .unwrap_or_else(|| panic!("`{name}` is a setting of this rust-analyzer"));
        assert_eq!(s["default"], p["default"], "`{name}`'s default");
        if s.get("enum").is_some() {
            assert_eq!(s["enum"], p["enum"], "`{name}`'s choices");
        }
    }
}

/// The formatter for a file no server formats: rustfmt on standard input,
/// run in the root as Kalem runs it, so the project's `rustfmt.toml` is
/// read; the 2024 edition, since a file alone names none and rustfmt's
/// own default, 2015, rejects `async` (and 2021 rejects let chains).
/// On the corpus: the unformatted module formatted as rustfmt formats
/// the file itself, the workspace's `use_field_init_shorthand` applied,
/// a second run changing nothing; every other file left as it is.
#[test]
fn the_format_command_is_rustfmt_in_the_root() {
    let m = manifest();
    let cmd = list(&m["commands"]["format"]);
    assert_eq!(cmd, ["rustfmt", "--edition", "2024"]);
    let run = |text: &str| {
        use std::io::Write;
        let mut child = tool(&cmd[0])
            .args(&cmd[1..])
            .current_dir(workspace())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("rustfmt runs");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(text.as_bytes())
            .expect("written");
        let out = child.wait_with_output().expect("rustfmt ends");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("UTF-8")
    };
    let messy = read("corpus/ws/shapes/src/messy.rs");
    let formatted = run(&messy);
    assert_ne!(formatted, messy);
    assert!(formatted.contains("Point { x, y }"), "{formatted}");
    assert_eq!(run(&formatted), formatted, "a second run changes nothing");
    // As rustfmt formats the file itself (which finds the configuration
    // from the file's folder), its output after the path's line.
    let file = workspace().join("shapes/src/messy.rs");
    let out = tool("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .arg(&file)
        .output()
        .expect("rustfmt runs");
    let by_path = String::from_utf8(out.stdout).expect("UTF-8");
    let by_path = by_path
        .strip_prefix(&format!("{}:\n\n", file.display()))
        .unwrap_or_else(|| panic!("{by_path}"));
    assert_eq!(formatted, by_path);
    for f in rs_files(&dir().join("corpus")) {
        if f.ends_with("messy.rs") || f.ends_with("script.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).expect("corpus");
        assert_eq!(run(&text), text, "{} is formatted", f.display());
    }
}

/// rust-analyzer's own requests, each by the command of Kalem's that sends
/// it (`code.expandMacro` for `rust-analyzer/expandMacro`), asked about
/// and answered in the shapes Kalem knows.
#[test]
fn requests_are_rust_analyzers_for_kalems_commands() {
    let m = manifest();
    let requests = m["requests"].as_object().expect("requests");
    let mut commands: Vec<&str> = requests.keys().map(String::as_str).collect();
    commands.sort_unstable();
    assert_eq!(
        commands,
        [
            "code.expandMacro",
            "code.joinLines",
            "code.moveItemDown",
            "code.moveItemUp",
            "code.openDocs",
            "code.openManifest",
            "code.parentModule",
            "code.reloadProject",
            "code.structuralReplace",
            "edit.newline"
        ]
    );
    for (command, r) in requests {
        let method = r["method"].as_str().expect("method");
        assert!(
            method.starts_with("rust-analyzer/") || method.starts_with("experimental/"),
            "{command}: {method}"
        );
        let shape = r["shape"].as_str().unwrap_or("text");
        assert!(
            ["text", "url", "location", "edits", "workspaceEdit", "none"].contains(&shape),
            "{command}: {shape}"
        );
        let params = r["params"].as_str().unwrap_or("position");
        assert!(
            ["position", "document", "range", "ranges", "none"].contains(&params),
            "{command}: {params}"
        );
        if let Some(server) = r["server"].as_str() {
            assert!(m["servers"].get(server).is_some(), "{command}: {server}");
        }
    }
    // RS9's two: the expansion shown as Rust, the documentation's page.
    let expand = &requests["code.expandMacro"];
    assert_eq!(list(&expand["answer"]), ["/expansion"]);
    assert_eq!(expand["language"], "rust");
    assert_eq!(requests["code.openDocs"]["shape"], "url");
    // The last three: Enter's new line as edits at the cursor; the
    // structural search and replace's query made of Kalem's two inputs,
    // the selection given, the edits offered across the files.
    let enter = &requests["edit.newline"];
    assert_eq!(
        (&enter["method"], &enter["shape"]),
        (&json!("experimental/onEnter"), &json!("edits"))
    );
    let ssr = &requests["code.structuralReplace"];
    assert_eq!(ssr["shape"], "workspaceEdit");
    assert_eq!(
        ssr["extra"],
        json!({"query": "{search} ==>> {replace}", "parseOnly": false,
               "selections": "{selections}"})
    );
}

/// rust-analyzer's state in the status bar: the notification it sends
/// only to a client that asks (`experimental/serverStatus`), read as its
/// health, its message and whether it is quiescent.
#[test]
fn the_server_state_is_rust_analyzers() {
    let m = manifest();
    let ra = &m["servers"]["rust-analyzer"];
    assert_eq!(
        ra["capabilities"],
        json!({"experimental": {"serverStatusNotification": true}})
    );
    assert_eq!(
        ra["status"],
        json!({"method": "experimental/serverStatus", "text": "/message", "level": "/health",
               "warning": "warning", "error": "error", "idle": {"/quiescent": true}})
    );
}

/// The project's run and test commands, for Kalem's `SPC p R` and
/// `SPC p T`: Cargo's, in the root. No `testAtPoint`: a file and a line
/// do not name a test, and rust-analyzer does (its runnables). On the
/// corpus the run command runs `app`.
#[test]
fn the_run_and_test_commands_are_cargos() {
    let m = manifest();
    let commands = &m["commands"];
    assert_eq!(list(&commands["test"]), ["cargo", "test"]);
    assert_eq!(list(&commands["run"]), ["cargo", "run"]);
    assert!(commands.get("testAtPoint").is_none());
    let target = std::env::temp_dir().join(format!("kalem-rust-run-{}", std::process::id()));
    let run = list(&commands["run"]);
    let out = tool(&run[0])
        .args(&run[1..])
        .args(["--quiet", "--offline"])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(workspace())
        .output()
        .expect("cargo runs");
    let _ = std::fs::remove_dir_all(&target);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "21.57\n");
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
    // A project never built has no `Cargo.lock` yet: its file's folder
    // is the root, and rust-analyzer finds its `Cargo.toml` above it.
    // Refusing a server outside a marked folder would refuse it that.
    assert_ne!(s["requireRoot"], true, "a project never built is served");
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
    assert_eq!(files.len(), 6, "{files:?}");
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
