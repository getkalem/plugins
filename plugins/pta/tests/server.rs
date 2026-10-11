//! The corpus through the language servers (PT3), spoken to by Kalem's
//! client as Kalem speaks to them: each server the manifest describes,
//! started in the corpus with the manifest's settings, asked what the
//! plugin's README says it gives. A server's formatting, where it offers
//! one, must leave a journal its tool reads as before: the formatted
//! file goes through `hledger print` or `bean-query`, beside the original.
//!
//! The programs: those `PTA_HLEDGER_LSP` and `PTA_BEANCOUNT_LSP` name
//! (paths, separated by `:`; two programs answer to `hledger-lsp`), else
//! the manifest's commands where they run. A server or a tool not
//! installed is skipped with a note, as on CI.

// A test skipped is said on standard error; the large journal's
// measures are printed.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_lsp::features;
use kalem_lsp::position::{Encoding, position};
use kalem_lsp::{Client, ServerConfig};
use serde_json::{Value, json};

fn manifest() -> Value {
    serde_json::from_str(kalem_plugin_pta::MANIFEST).expect("plugin.json parses")
}

fn corpus(d: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus").join(d)
}

/// What `program --version` prints, if it runs.
fn version(program: &str) -> Option<String> {
    Command::new(program)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The programs of server `key`: the variable's, else the manifest's
/// command where it runs.
fn programs(var: &str, key: &str) -> Vec<String> {
    let listed: Vec<String> = std::env::var(var)
        .unwrap_or_default()
        .split(':')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    let programs = if listed.is_empty() {
        vec![
            manifest()["servers"][key]["command"][0]
                .as_str()
                .expect("command")
                .to_string(),
        ]
    } else {
        listed
    };
    programs
        .into_iter()
        .filter(|p| {
            let runs = version(p).is_some();
            if !runs {
                eprintln!("{p} does not run here: its corpus test is skipped");
            }
            runs
        })
        .collect()
}

/// Formatters known to change what a journal means, by the version they
/// print: the damage is checked to be still there (the README warns of
/// it), so that a fixed release is noticed.
const CHANGES_MEANING: &[(&str, &str)] = &[(
    "hledger-lsp v0.7.0",
    "ptimoney's 0.7.0 drops an automated posting's multiplier (*-1) and writes a == assertion as =",
)];

struct Server {
    client: Client,
    enc: Encoding,
    root: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.client.shutdown();
    }
}

impl Server {
    fn start(key: &str, program: &str, root: PathBuf, language: &str, files: &[&str]) -> Server {
        let init = manifest()["servers"][key]["initializationOptions"].clone();
        Server::start_with(key, program, root, language, files, init)
    }

    /// As [`Server::start`], with `init` its initialization options.
    fn start_with(
        key: &str,
        program: &str,
        root: PathBuf,
        language: &str,
        files: &[&str],
        init: Value,
    ) -> Server {
        let spec = &manifest()["servers"][key];
        let args: Vec<String> = spec["command"]
            .as_array()
            .expect("command")
            .iter()
            .skip(1)
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect();
        let client = Client::start(
            ServerConfig {
                name: key.into(),
                command: program.into(),
                args,
                root: root.clone(),
                settings: spec["settings"].clone(),
                initialization_options: init,
                ..ServerConfig::default()
            },
            Arc::new(|| {}),
        )
        .unwrap_or_else(|e| panic!("{program} starts: {e}"));
        let t = Instant::now();
        while !client.is_ready() {
            assert!(
                t.elapsed() < Duration::from_secs(60),
                "{program}: never ready"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        for f in files {
            client.did_open(&uri(&root, f), language, &read(&root, f));
        }
        let enc = client.encoding();
        Server { client, enc, root }
    }

    /// Asks until the answer is one `ok` takes; at most `secs`.
    fn ask(&self, method: &str, params: Value, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
        let t = Instant::now();
        loop {
            let r = self
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
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    /// The position `offset` bytes into `word` on the first line of `f`
    /// holding `line`.
    fn at(&self, f: &str, line: &str, word: &str, offset: usize) -> Value {
        let t = read(&self.root, f);
        let start = t.find(line).unwrap_or_else(|| panic!("{f}: {line}"));
        let byte = start + line.find(word).expect("word in line") + offset;
        position(&t, byte, self.enc).to_json()
    }

    /// The labels completed at the end of `prefix` on `line` of `f`.
    fn completed(&self, f: &str, line: &str, prefix: &str) -> Vec<String> {
        let place = self.at(f, line, prefix, prefix.len());
        let answer = self.ask(
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri(&self.root, f) }, "position": place }),
            30,
            |v| !features::completion_items(v, false).0.is_empty(),
        );
        features::completion_items(&answer, false)
            .0
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    /// The diagnostics of `f` once there are some, as their lines (from 1)
    /// and messages.
    fn diagnostics(&self, f: &str) -> Vec<(u64, String)> {
        let u = uri(&self.root, f);
        let t = Instant::now();
        loop {
            let d = self.client.diagnostics(&u);
            if !d.is_empty() {
                return d
                    .iter()
                    .map(|d| {
                        (
                            d["range"]["start"]["line"].as_u64().unwrap_or(0) + 1,
                            d["message"].as_str().unwrap_or("").to_string(),
                        )
                    })
                    .collect();
            }
            assert!(t.elapsed() < Duration::from_secs(60), "{f}: no diagnostics");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// `f` as the server formats it, or none when it offers no
    /// formatting.
    fn formatted(&self, f: &str) -> Option<String> {
        if !self.client.provides("documentFormattingProvider") {
            return None;
        }
        let answer = self.ask(
            "textDocument/formatting",
            json!({ "textDocument": { "uri": uri(&self.root, f) }, "options": { "tabSize": 4, "insertSpaces": true } }),
            30,
            |v| v.is_array(),
        );
        let before = read(&self.root, f);
        let edits = features::text_edits(&before, &answer, self.enc).expect("edits");
        Some(features::apply(&before, &edits))
    }
}

fn uri(root: &Path, f: &str) -> String {
    kalem_lsp::uri::from_path(&root.join(f))
}

fn read(root: &Path, f: &str) -> String {
    std::fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"))
}

/// A copy of corpus folder `d` to change, removed when dropped.
struct Copy(PathBuf);

impl Copy {
    fn of(d: &str, tag: &str) -> Copy {
        let to = std::env::temp_dir().join(format!("pta-{d}-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&to);
        std::fs::create_dir_all(&to).expect("copy");
        for e in std::fs::read_dir(corpus(d)).expect("corpus") {
            let p = e.expect("entry").path();
            std::fs::copy(&p, to.join(p.file_name().expect("name"))).expect("copy");
        }
        Copy(to)
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What `program` prints with `args` in `dir`, or `None` where it does
/// not run.
fn output(program: &str, args: &[&str], dir: &Path) -> Option<String> {
    let o = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    assert!(
        o.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Whether the tool reads `formatted`, written over `f` in a copy of the
/// corpus, as it reads the original; `None` where the tool is missing.
fn same_to_the_tool(d: &str, f: &str, formatted: &str, read_by: &[&str]) -> Option<bool> {
    let (program, args) = read_by.split_first().expect("a command");
    let original = output(program, args, &corpus(d))?;
    let copy = Copy::of(d, "formatted");
    std::fs::write(copy.0.join(f), formatted).expect("write");
    let after = output(program, args, &copy.0)?;
    Some(original == after)
}

/// Checks a server's formatting against the tool; `known` is what the
/// server's version is known to do wrong.
fn check_format(program: &str, formatted: Option<String>, same: impl FnOnce(&str) -> Option<bool>) {
    let Some(formatted) = formatted else {
        eprintln!("{program} offers no formatting");
        return;
    };
    let Some(same) = same(&formatted) else {
        eprintln!("the tool is not installed: {program}'s formatting is not checked");
        return;
    };
    let v = version(program).unwrap_or_default();
    match CHANGES_MEANING.iter().find(|(said, _)| v.contains(said)) {
        Some((_, damage)) => {
            assert!(
                !same,
                "{v} formats safely now: the README's warning ({damage}) can go"
            );
            eprintln!("{program}: as known, {damage}");
        }
        None => assert!(same, "{program} ({v}) changes what the journal means"),
    }
}

#[test]
fn the_corpus_through_hledger_lsp() {
    for program in programs("PTA_HLEDGER_LSP", "hledger-lsp") {
        let root = corpus("hledger");
        let s = Server::start(
            "hledger-lsp",
            &program,
            root,
            "hledger",
            &["2026.journal", "errors.journal"],
        );
        // Completion of the declared accounts, those of the included
        // `accounts.journal`.
        let labels = s.completed("2026.journal", "    expenses:food:market   ", "    exp");
        for want in ["expenses:food:market", "expenses:rent"] {
            assert!(
                labels.iter().any(|l| l == want),
                "{program}: {want} in {labels:?}"
            );
        }
        // The failed assertion of `errors.journal`, at its line.
        let d = s.diagnostics("errors.journal");
        assert!(
            d.iter()
                .any(|(line, m)| *line == 13 && m.to_lowercase().contains("assertion")),
            "{program}: {d:?}"
        );
        check_format(&program, s.formatted("2026.journal"), |text| {
            same_to_the_tool(
                "hledger",
                "2026.journal",
                text,
                &["hledger", "-f", "2026.journal", "print", "--auto"],
            )
        });
    }
}

#[test]
fn the_corpus_through_beancount_language_server() {
    for program in programs("PTA_BEANCOUNT_LSP", "beancount-language-server") {
        let root = corpus("beancount");
        let s = Server::start(
            "beancount-language-server",
            &program,
            root,
            "beancount",
            &["household.beancount", "errors.beancount"],
        );
        // Accounts opened in the included `accounts.beancount`, and the
        // journal's payees.
        let labels = s.completed(
            "household.beancount",
            "  Assets:Bank:Ziraat        45000",
            "  Ass",
        );
        assert!(
            labels.iter().any(|l| l == "Assets:Broker:FUND"),
            "{labels:?}"
        );
        let payees = s.completed(
            "household.beancount",
            "2026-01-02 * \"Ziraat\"",
            "2026-01-02 * \"Zi",
        );
        assert!(payees.iter().any(|l| l == "Ziraat"), "{payees:?}");
        // bean-check's error, where bean-check runs (the server runs it).
        if version("bean-check").is_some() {
            let d = s.diagnostics("errors.beancount");
            assert!(
                d.iter().any(|(_, m)| m.contains("unknown account")),
                "{d:?}"
            );
        } else {
            eprintln!("bean-check is not installed: the server's checks are skipped");
        }
        check_format(&program, s.formatted("household.beancount"), |text| {
            same_to_the_tool(
                "beancount",
                "household.beancount",
                text,
                &[
                    "bean-query",
                    "-f",
                    "csv",
                    "household.beancount",
                    "SELECT date, flag, payee, narration, account, position, price",
                ],
            )
        });
    }
}

/// The servers on the accountant's journal (PT13): the 200,000
/// transactions `tests/large.rs` writes in `PTA_LARGE`, each server
/// started there and told the main journal, the time until it is ready
/// and until its first completion of an account in the last year's file.
/// Run by hand after `tests/large.rs`: `PTA_LARGE=… cargo test -p
/// kalem-plugin-pta --test server -- --ignored --nocapture`.
#[test]
#[ignore = "the large journal and minutes of the servers' time: run by hand"]
fn the_servers_on_the_large_journal() {
    let Some(root) = std::env::var_os("PTA_LARGE").map(PathBuf::from) else {
        eprintln!("PTA_LARGE names no folder: run tests/large.rs first");
        return;
    };
    let timed = |key: &str,
                 program: &str,
                 language: &str,
                 main: &str,
                 file: &str,
                 line: &str,
                 prefix: &str| {
        let t = Instant::now();
        // Told the main journal, as the setting tells it.
        let init = json!({ "journal_file": root.join(main).to_string_lossy() });
        let s = Server::start_with(key, program, root.clone(), language, &[main, file], init);
        let ready = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let place = s.at(file, line, prefix, prefix.len());
        let answer = s.ask(
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri(&root, file) }, "position": place }),
            600,
            |v| !features::completion_items(v, false).0.is_empty(),
        );
        let n = features::completion_items(&answer, false).0.len();
        println!(
            "{program}: ready in {ready:.1} s, {n} completions in {:.1} s",
            t.elapsed().as_secs_f64()
        );
    };
    for program in programs("PTA_HLEDGER_LSP", "hledger-lsp") {
        timed(
            "hledger-lsp",
            &program,
            "hledger",
            "all.journal",
            "2026.journal",
            "    expenses:",
            "    exp",
        );
    }
    for program in programs("PTA_BEANCOUNT_LSP", "beancount-language-server") {
        timed(
            "beancount-language-server",
            &program,
            "beancount",
            "all.beancount",
            "2026.beancount",
            "  Expenses:",
            "  Exp",
        );
    }
}
