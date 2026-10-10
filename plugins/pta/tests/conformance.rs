//! The plugin's conformance: the manifest's two halves (the languages and
//! servers Kalem's core reads, the component and the programs it runs)
//! name what exists and agree; and the corpus, one journal per dialect,
//! is what its own tool says it is: each clean journal passes the tool's
//! strictest check, each error file fails it at its line. A tool not
//! installed where the tests run is skipped with a note.

// A check skipped (the tool not installed) is said on standard error.
#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_plugin_pta::{Dialect, MANIFEST};
use serde_json::Value;

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("plugin.json parses")
}

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(rel)
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_manifest_has_both_halves() {
    let m = manifest();
    assert_eq!(m["id"], "org.kalem.pta");
    assert_eq!(m["main"], "dist/pta.wasm");
    assert_eq!(m["api"], "^0.2.9");
    // No activation: Kalem starts it with the first journal it serves (a
    // Kalem before 0.6.9, at its start).
    assert!(m.get("activation").is_none());
    let languages = m["languages"].as_array().expect("languages");
    let ids: Vec<&str> = languages.iter().filter_map(|l| l["id"].as_str()).collect();
    assert_eq!(ids, ["hledger", "ledger", "beancount", "hledger-rules"]);
    // Each dialect's extensions are the ones the component reads.
    for l in languages {
        let id = l["id"].as_str().unwrap();
        for e in strings(&l["extensions"]) {
            let found = Dialect::of_path(&format!("x.{e}"));
            match Dialect::of_language(id) {
                Some(d) => assert_eq!(found, Some(d), "{id}: .{e}"),
                None => assert_eq!(found, None, "{id}: .{e}"),
            }
        }
        assert!(l["comment"]["line"].is_string(), "{id}: a line comment");
        // Every server a language names is declared.
        for s in strings(&l["servers"]) {
            let spec = &m["servers"][&s];
            assert!(spec.is_object(), "{id}: server {s}");
            assert!(
                spec["command"].is_array() && spec["install"].is_string(),
                "{s}"
            );
        }
    }
    assert!(Dialect::of_path("x.dat").is_none(), ".dat is not claimed");
    // The programs the component runs are permitted, each with its
    // setting for where it is.
    let permissions = strings(&m["permissions"]);
    for d in Dialect::ALL {
        let program = d.checker();
        assert!(
            permissions.contains(&format!("subprocess:{program}")),
            "{program}"
        );
        assert!(
            m["settings"][format!("programs.{program}")].is_object(),
            "{program}"
        );
    }
    let description = m["description"].as_str().unwrap();
    for name in ["hledger", "Ledger", "Beancount"] {
        assert!(description.contains(name), "{name}");
    }
}

/// `program` with `args` in `dir`, or `None` when it is not installed.
fn run(program: &str, args: &[&str], dir: &Path) -> Option<std::process::Output> {
    match Command::new(program).args(args).current_dir(dir).output() {
        Ok(o) => Some(o),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{program} is not installed: its corpus check is skipped");
            None
        }
        Err(e) => panic!("{program}: {e}"),
    }
}

/// The tool's strictest check of `file`.
fn check(d: Dialect, file: &str) -> Option<std::process::Output> {
    let dir = corpus(d.language());
    match d {
        Dialect::Hledger => run(
            "hledger",
            &["-f", file, "check", "--strict", "--auto"],
            &dir,
        ),
        Dialect::Ledger => run("ledger", &["-f", file, "--pedantic", "balance"], &dir),
        Dialect::Beancount => run("bean-check", &[file], &dir),
    }
}

#[test]
fn each_clean_journal_passes_its_tool() {
    for (d, file) in [
        (Dialect::Hledger, "2026.journal"),
        (Dialect::Ledger, "business.ledger"),
        (Dialect::Beancount, "household.beancount"),
    ] {
        let Some(o) = check(d, file) else { continue };
        assert!(
            o.status.success(),
            "{file}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

#[test]
fn each_error_file_fails_at_its_line() {
    for (d, file, said) in [
        // A balance assertion that does not hold.
        (Dialect::Hledger, "errors.journal", "errors.journal:13:49:"),
        // A transaction that does not balance, lines 7 to 9.
        (Dialect::Ledger, "errors.ledger", "lines 7-9"),
        // A posting to an account never opened.
        (
            Dialect::Beancount,
            "errors.beancount",
            "errors.beancount:7:",
        ),
    ] {
        let Some(o) = check(d, file) else { continue };
        assert!(!o.status.success(), "{file} passed");
        let out = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(out.contains(said), "{file}: {out}");
    }
}

#[test]
fn the_corpus_has_what_the_list_names() {
    let read = |rel: &str| std::fs::read_to_string(corpus(rel)).unwrap();
    let h = format!(
        "{}{}{}",
        read("hledger/2026.journal"),
        read("hledger/accounts.journal"),
        read("hledger/2025.journal")
    );
    for construct in [
        "include ",
        "account ",
        "commodity ",
        "decimal-mark ,",
        "~ monthly",
        "\n= ",
        " * ",
        " ! ",
        " @ ",
        " @@ ",
        " = ",
        " == ",
        "; food:",
    ] {
        assert!(h.contains(construct), "hledger: {construct:?}");
    }
    let l = read("ledger/business.ledger");
    for construct in [
        "apply account",
        "alias ",
        "\nP ",
        "\nD ",
        "[Liabilities",
        "(Budget",
        "check ",
        "assert ",
    ] {
        assert!(l.contains(construct), "ledger: {construct:?}");
    }
    let b = format!(
        "{}{}",
        read("beancount/household.beancount"),
        read("beancount/accounts.beancount")
    );
    for construct in [
        "option ",
        "plugin ",
        " open ",
        " close ",
        " commodity ",
        " pad ",
        " balance ",
        " price ",
        " note ",
        " document ",
        " event ",
        " custom ",
        "include ",
        "{350.00 TRY}",
        " @ ",
        "#payroll",
        "^trade-001",
        "receipt: ",
        "pushtag",
        " txn ",
    ] {
        assert!(b.contains(construct), "beancount: {construct:?}");
    }
}
