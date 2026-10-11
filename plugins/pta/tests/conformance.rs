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
    // Started with the first document of any kind: a bank statement (a
    // CSV file, which the plugin does not claim) is imported from (PT9),
    // and the journal's own files would start it too late.
    assert_eq!(strings(&m["activation"]), ["onEvent:document:open"]);
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
        // The highlighter's hard cases (PT2): an account with a space, a
        // quoted commodity with one.
        "assets:broker:index fund  ",
        "\"FUND A\"",
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
        // A negative amount in parentheses, a value expression.
        "(-$132.00)",
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
        // A sum as an amount.
        "800.00 + 42.35 TRY",
    ] {
        assert!(b.contains(construct), "beancount: {construct:?}");
    }
}

/// The bank statement and its CSV rules (`corpus/hledger/statement.csv`
/// and `.rules`, PT2's highlighting and PT9a's import): hledger reads
/// each line into the account the rules give it.
#[test]
fn the_csv_rules_put_each_line_in_its_account() {
    let Some(o) = run(
        "hledger",
        &["-f", "statement.csv", "print"],
        &corpus("hledger"),
    ) else {
        return;
    };
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let transactions: Vec<&str> = out.split("\n\n").filter(|t| !t.trim().is_empty()).collect();
    assert_eq!(transactions.len(), 4, "{out}");
    for (t, account) in transactions.iter().zip([
        "expenses:food:market",
        "income:salary",
        "expenses:food:restaurant",
        "expenses:unknown",
    ]) {
        assert!(
            t.contains("assets:bank:ziraat") && t.contains(account),
            "{account}: {t}"
        );
    }
    assert!(
        transactions[0].contains("shop:migros"),
        "{}",
        transactions[0]
    );
}

/// The servers (PT3) and the settings around them: each server says its
/// version and how to install it in a line short enough for the status
/// bar, where Kalem shows it when the server is missing; each server
/// setting is in a section a server reads (`hledgerLanguageServer`,
/// ptimoney's hledger-lsp, `hledger`, juev's); the plugin's own `journal`
/// and `tool` are the component's (PT3c).
#[test]
fn the_servers_and_their_settings() {
    let m = manifest();
    let servers = m["servers"].as_object().expect("servers");
    for (key, s) in servers {
        assert_eq!(strings(&s["version"]), ["--version"], "{key}");
        let install = s["install"].as_str().expect("install");
        assert!(
            install.len() <= 64,
            "{key}: {install} is too long for the status bar"
        );
    }
    let sections = &servers["hledger-lsp"]["settings"];
    let settings = m["settings"].as_object().expect("settings");
    for key in settings.keys().filter(|k| k.starts_with("settings.")) {
        let section = key.split('.').nth(1).expect("a section");
        assert!(sections[section].is_object(), "{key}: {section} is sent");
        let s = &settings[key];
        assert!(
            s["default"] != Value::Null && s["description"].is_string(),
            "{key}"
        );
    }
    // The servers' own settings Kalem merges over the manifest's (the
    // `ignore` and `initializationOptions` of a server), each of a server
    // the manifest describes, with the manifest's value as default.
    for key in settings.keys().filter(|k| k.starts_with("servers.")) {
        let mut parts = key.splitn(3, '.');
        let (_, server, field) = (parts.next(), parts.next().unwrap(), parts.next().unwrap());
        assert!(servers.contains_key(server), "{key}");
        assert!(
            field == "ignore" || field.starts_with("initializationOptions."),
            "{key}"
        );
    }
    assert_eq!(
        settings["servers.hledger-lsp.ignore"]["default"],
        servers["hledger-lsp"]["ignore"]
    );
    assert_eq!(
        strings(&servers["hledger-lsp"]["ignore"]),
        ["documentFormattingProvider"]
    );
    assert_eq!(settings["journal"]["default"], "");
    let tools = strings(&settings["tool"]["enum"]);
    assert_eq!(tools, ["auto", "hledger", "ledger", "beancount"]);
    for t in &tools[1..] {
        assert_eq!(
            kalem_plugin_pta::journal::ToolChoice::parse(t),
            kalem_plugin_pta::journal::ToolChoice::Only(
                Dialect::of_language(t).expect("a dialect")
            ),
        );
    }
}

/// The tool's check as the plugin runs it (PT5), on the corpus: each
/// error file gives one problem, at its line, in its file; each clean
/// journal none. A tool not installed is skipped.
#[test]
fn the_check_finds_each_error_files_problem() {
    use kalem_plugin_pta::check;
    for (d, file, line) in [
        (Dialect::Hledger, "errors.journal", Some(13)),
        (Dialect::Ledger, "errors.ledger", Some(9)),
        (Dialect::Beancount, "errors.beancount", Some(7)),
        (Dialect::Hledger, "2026.journal", None),
        (Dialect::Ledger, "business.ledger", None),
        (Dialect::Beancount, "household.beancount", None),
    ] {
        let dir = corpus(d.language());
        let main = dir.join(file).to_string_lossy().into_owned();
        let args = check::command(d, &main, &[]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let Some(o) = run(d.checker(), &args, &dir) else {
            continue;
        };
        let said = if o.stderr.iter().any(|b| !b.is_ascii_whitespace()) {
            &o.stderr
        } else {
            &o.stdout
        };
        let problems = check::problems(d, o.status.code(), &String::from_utf8_lossy(said));
        match line {
            None => assert!(problems.is_empty(), "{file}: {problems:?}"),
            Some(line) => {
                assert_eq!(problems.len(), 1, "{file}: {problems:?}");
                assert_eq!(problems[0].line, Some(line), "{file}");
                assert!(
                    problems[0]
                        .file
                        .as_deref()
                        .is_some_and(|f| f.ends_with(file)),
                    "{file}: {problems:?}"
                );
                assert!(!problems[0].message.is_empty());
            }
        }
    }
}

/// The Journal menu and button (PT11a), as the git plugin's: the menu
/// bar's and the F10 list's "Journal" lists the journal's commands
/// (`kalem_plugin_pta::MENU`) in its order, its separators kept, then
/// the pick itself; the button runs the pick (`pta.menu`); both where a
/// journal or the Check document is. Kalem drops an item that is not the
/// plugin's (`pta.`), so each is; and each title is the one the
/// component registers, which the menu shows.
#[test]
fn the_journal_menu_and_button() {
    let m = manifest();
    let menus = m["menus"].as_array().expect("menus");
    assert_eq!(menus.len(), 1);
    let menu = &menus[0];
    assert_eq!(menu["title"], "Journal");
    assert_eq!(menu["when"], kalem_plugin_pta::menu_when().as_str());
    let mut want: Vec<String> = kalem_plugin_pta::MENU
        .iter()
        .map(|(id, ..)| id.to_string())
        .collect();
    want.extend(["-".to_string(), "pta.menu".to_string()]);
    assert_eq!(strings(&menu["items"]), want);
    let buttons = m["buttons"].as_array().expect("buttons");
    assert_eq!(buttons.len(), 1);
    assert_eq!(buttons[0]["title"], "Journal");
    assert_eq!(buttons[0]["command"], "pta.menu");
    assert_eq!(buttons[0]["when"], menu["when"]);
    // The component registers each with the title the table gives.
    let source: String = include_str!("../src/component.rs")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for (id, title, _) in kalem_plugin_pta::MENU.iter().filter(|(id, ..)| *id != "-") {
        assert!(id.starts_with("pta."), "{id}");
        assert!(
            source.contains(&format!("command( \"{id}\", \"{title}\""))
                || source.contains(&format!("command(\"{id}\", \"{title}\"")),
            "{id} is registered as {title:?}"
        );
    }
    assert!(source.contains("command(\"pta.menu\", \"Commands…\""));
}
