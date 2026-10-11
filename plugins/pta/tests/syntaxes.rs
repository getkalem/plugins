//! The highlighter (PT2): the plugin's syntaxes, hledger, Ledger and
//! Beancount over one journal base and hledger's CSV rules beside them,
//! load as Kalem loads them, take the languages' files and source blocks,
//! parse every corpus file to its end, and color a transaction of each
//! dialect as the tests below spell out, span by span; and the tags and
//! accounts the hledger syntax colors are those hledger itself reads.

// A comparison skipped (hledger not installed) is said on standard error.
#![allow(clippy::print_stderr)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use kalem_highlight::{Kind, Language, SyntaxSource};
use serde_json::Value;
use syntect::parsing::{ParseState, SyntaxDefinition, SyntaxSet};

use Kind::{Comment, Constant, Keyword, Number, Operator, String as Str, Tag, Type};

fn manifest() -> Value {
    serde_json::from_str(kalem_plugin_pta::MANIFEST).expect("plugin.json parses")
}

fn dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
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

/// A top-level key of a syntax file: `name`, `extends`, `hidden`.
fn key<'t>(text: &'t str, key: &str) -> Option<&'t str> {
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
        .map(str::trim)
}

/// The manifest's syntax files as Kalem reads them: the syntaxes, then
/// the bases only for `extends`.
fn sources() -> Vec<SyntaxSource> {
    let m = manifest();
    let mut out = Vec::new();
    for (key, base_only) in [("syntaxes", false), ("syntaxBases", true)] {
        for f in strings(&m[key]) {
            out.push(SyntaxSource {
                file: f.rsplit('/').next().unwrap_or(&f).to_string(),
                text: read(&f),
                base_only,
            });
        }
    }
    out
}

/// The syntaxes registered as Kalem registers them, once for this test
/// binary (registering replaces Kalem's set), with each language's id
/// and extensions naming its syntax, as Kalem's language loader does
/// (`languages::load_dirs`).
fn registered() {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        let r = kalem_highlight::register(&sources());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let mut aliases = Vec::new();
        for l in manifest()["languages"].as_array().expect("languages") {
            let syntax = l["syntax"].as_str().expect("syntax").to_string();
            let id = l["id"].as_str().expect("id").to_string();
            for name in std::iter::once(id).chain(strings(&l["extensions"])) {
                aliases.push((name.to_ascii_lowercase(), syntax.clone()));
            }
        }
        kalem_highlight::set_aliases(aliases);
    });
}

fn language(name: &str) -> Language {
    registered();
    Language::find(name).unwrap_or_else(|| panic!("no language for {name}"))
}

/// The spans of each line of `text` in the language `lang`, as their
/// text and kind.
fn colored<'t>(lang: &str, text: &'t str) -> Vec<Vec<(&'t str, Kind)>> {
    let lines = kalem_highlight::highlight(language(lang), text);
    text.split_inclusive('\n')
        .zip(lines)
        .map(|(l, spans)| spans.into_iter().map(|s| (&l[s.range], s.kind)).collect())
        .collect()
}

/// The spans of the line of `text` holding `line`, `text` colored from
/// its start.
fn line_of<'t>(lang: &str, text: &'t str, line: &str) -> Vec<(&'t str, Kind)> {
    let n = text
        .lines()
        .position(|l| l.contains(line))
        .unwrap_or_else(|| panic!("no line with {line:?}"));
    colored(lang, text).swap_remove(n)
}

#[test]
fn the_manifest_names_its_syntaxes() {
    let m = manifest();
    let names: Vec<(String, String)> = strings(&m["syntaxes"])
        .iter()
        .map(|f| {
            let text = read(f);
            let name = key(&text, "name").unwrap_or_else(|| panic!("{f} has a name"));
            (f.clone(), name.to_string())
        })
        .collect();
    let bases = strings(&m["syntaxBases"]);
    assert_eq!(bases, ["syntaxes/Journal (Base).sublime-syntax"]);
    let base = read(&bases[0]);
    assert_eq!(
        key(&base, "hidden"),
        Some("true"),
        "the base is no language"
    );
    for l in m["languages"].as_array().expect("languages") {
        let id = l["id"].as_str().expect("id");
        let syntax = l["syntax"].as_str().expect("syntax");
        let (file, _) = names
            .iter()
            .find(|(_, n)| n == syntax)
            .unwrap_or_else(|| panic!("{id}: syntax {syntax} is shipped"));
        let text = read(file);
        // The syntax takes the language's extensions on its own too, for
        // a Kalem that finds a syntax by extension.
        let exts = key(&text, "file_extensions").expect("file_extensions");
        assert_eq!(
            exts,
            format!("[{}]", strings(&l["extensions"]).join(", ")),
            "{id}"
        );
        // The three journals extend the base; the CSV rules stand alone.
        let extends = key(&text, "extends");
        if id == "hledger-rules" {
            assert_eq!(extends, None);
        } else {
            assert_eq!(extends, Some("Journal (Base).sublime-syntax"), "{id}");
        }
    }
    assert_eq!(names.len(), m["languages"].as_array().unwrap().len());
}

/// Each language's id (a source block's name in Org and Markdown), its
/// extensions (a file's) and its syntax's name find the plugin's syntax;
/// the base is found by none.
#[test]
fn the_syntaxes_take_the_languages_files_and_blocks() {
    for l in manifest()["languages"].as_array().expect("languages") {
        let syntax = l["syntax"].as_str().expect("syntax");
        let id = l["id"].as_str().expect("id").to_string();
        for name in [vec![id, syntax.to_string()], strings(&l["extensions"])].concat() {
            assert_eq!(language(&name).name(), syntax, "{name}");
        }
    }
    assert!(Language::find("Journal (Base)").is_none());
    // A journal in a source block: colored as in its file.
    let block = "2026-01-02 * Market\n    expenses:food  10 TRY\n";
    for lang in ["hledger", "ledger", "beancount"] {
        assert_eq!(colored(lang, block)[0][0], ("2026-01-02", Number), "{lang}");
    }
}

/// The corpus files of `ext` under `corpus/`.
fn corpus_files(ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in ["hledger", "ledger", "beancount"] {
        for e in std::fs::read_dir(dir().join("corpus").join(d)).expect("corpus") {
            let p = e.expect("entry").path();
            if p.extension().is_some_and(|x| x == ext) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Every corpus file parses to its end without an error (Kalem colors a
/// line it cannot parse not at all), through syntect with `extends`
/// resolved as Kalem resolves it; and the file leaves the syntax back at
/// its top: a line after it is colored as one at the top.
#[test]
fn every_corpus_file_parses_to_its_end() {
    let (flat, errors) = kalem_highlight::flatten(&sources());
    assert!(errors.is_empty(), "{errors:?}");
    let mut b = SyntaxSet::load_defaults_newlines().into_builder();
    for (file, text) in &flat {
        b.add(
            SyntaxDefinition::load_from_str(text, true, None)
                .unwrap_or_else(|e| panic!("{file}: {e}")),
        );
    }
    let set = b.build();
    let mut checked = 0;
    for (lang, ext, after, kind) in [
        (
            "hledger",
            "journal",
            "2026-12-31 * After the file\n",
            Number,
        ),
        ("ledger", "ledger", "2026/12/31 * After the file\n", Number),
        (
            "beancount",
            "beancount",
            "2026-12-31 open Assets:After\n",
            Number,
        ),
        ("hledger-rules", "rules", "skip 1\n", Keyword),
    ] {
        let syntax_name = language(lang).name();
        let syntax = set
            .syntaxes()
            .iter()
            .rev()
            .find(|s| s.name == syntax_name)
            .unwrap_or_else(|| panic!("{syntax_name}"));
        for f in corpus_files(ext) {
            let text = std::fs::read_to_string(&f).expect("corpus");
            let mut state = ParseState::new(syntax);
            for (i, l) in text.split_inclusive('\n').enumerate() {
                state
                    .parse_line(l, &set)
                    .unwrap_or_else(|e| panic!("{}:{}: {e}", f.display(), i + 1));
            }
            let whole = format!("{text}\n{after}");
            let lines = colored(lang, &whole);
            assert_eq!(
                lines[lines.len() - 1].first().map(|s| s.1),
                Some(kind),
                "{}: {:?}",
                f.display(),
                lines.last()
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 10, "the corpus's files");
}

#[test]
fn an_hledger_transaction() {
    let t = read("corpus/hledger/2026.journal");
    assert_eq!(
        line_of("hledger", &t, "2026-01-02 *"),
        [
            ("2026-01-02", Number),
            ("*", Keyword),
            ("(1001)", Constant),
            ("Ziraat", Str),
            ("salary", Str),
            ("; ", Comment),
            ("payroll:", Tag),
            ("january", Comment),
        ]
    );
    assert_eq!(
        line_of("hledger", &t, "45.000,00 TRY = 93.749,60 TRY"),
        [
            ("assets:bank:ziraat", Type),
            ("45.000,00", Number),
            ("TRY", Constant),
            ("=", Operator),
            ("93.749,60", Number),
            ("TRY", Constant),
        ]
    );
    assert_eq!(
        line_of("hledger", &t, "2026-01-03 !"),
        [
            ("2026-01-03", Number),
            ("!", Keyword),
            ("Market", Str),
            ("groceries", Str),
            ("; ", Comment),
            ("food:", Tag),
            (", ", Comment),
            ("shop:", Tag),
            ("migros", Comment),
        ]
    );
    assert_eq!(
        line_of("hledger", &t, "1.120,00 TRY  ; split"),
        [
            ("expenses:food:restaurant", Type),
            ("1.120,00", Number),
            ("TRY", Constant),
            ("; split with Ayşe", Comment),
        ]
    );
    assert_eq!(
        line_of("hledger", &t, "USD @@"),
        [
            ("assets:cash", Type),
            ("100,00", Number),
            ("USD", Constant),
            ("@@", Operator),
            ("3.410,00", Number),
            ("TRY", Constant),
        ]
    );
    assert_eq!(
        line_of("hledger", &t, "== 63.697,25"),
        [
            ("assets:bank:ziraat", Type),
            ("==", Operator),
            ("63.697,25", Number),
            ("TRY", Constant),
        ]
    );
    // The periodic and the automated transactions.
    assert_eq!(
        line_of("hledger", &t, "~ monthly"),
        [("~", Keyword), ("monthly from 2026-01", Constant)]
    );
    assert_eq!(
        line_of("hledger", &t, "= expenses:food"),
        [("=", Keyword), ("expenses:food", Str)]
    );
    assert_eq!(
        line_of("hledger", &t, "*-1"),
        [("budget:food", Type), ("*", Operator), ("-1", Number)]
    );
    // The directives.
    assert_eq!(
        line_of("hledger", &t, "decimal-mark"),
        [("decimal-mark", Keyword), (",", Constant)]
    );
    assert_eq!(
        line_of("hledger", &t, "include accounts"),
        [("include", Keyword), ("accounts.journal", Str)]
    );
    let a = read("corpus/hledger/accounts.journal");
    assert_eq!(
        line_of("hledger", &a, "account assets:cash"),
        [
            ("account", Keyword),
            ("assets:cash", Type),
            ("; ", Comment),
            ("type:", Tag),
            (" C", Comment),
        ]
    );
    assert_eq!(
        line_of("hledger", &a, "commodity 1.000,00 TRY"),
        [
            ("commodity", Keyword),
            ("1.000,00", Number),
            ("TRY", Constant)
        ]
    );
}

#[test]
fn a_ledger_transaction() {
    let t = read("corpus/ledger/business.ledger");
    assert_eq!(
        line_of("ledger", &t, "2026/01/05"),
        [
            ("2026/01/05", Number),
            ("*", Keyword),
            ("(INV-001)", Constant),
            // No payee and note in Ledger: the `|` is the payee's.
            ("Client A | consulting, January", Str),
        ]
    );
    assert_eq!(
        line_of("ledger", &t, "[Liabilities:Tax]"),
        [
            ("Liabilities:Tax", Type),
            ("$", Constant),
            ("-400.00", Number)
        ]
    );
    assert_eq!(
        line_of("ledger", &t, "$49.00"),
        [
            ("Expenses:Software", Type),
            ("$", Constant),
            ("49.00", Number),
            ("; ", Comment),
            (":tools:", Tag),
        ]
    );
    assert_eq!(
        line_of("ledger", &t, "EUR 120.00"),
        [
            ("Expenses:Travel", Type),
            ("EUR", Constant),
            ("120.00", Number)
        ]
    );
    assert_eq!(
        line_of("ledger", &t, "P 2026/01/01"),
        [
            ("P", Keyword),
            ("2026/01/01", Number),
            ("EUR", Constant),
            ("$", Constant),
            ("1.10", Number),
        ]
    );
    assert_eq!(
        line_of("ledger", &t, "check commodity"),
        [("check", Keyword), ("==", Operator), ("\"$\"", Str)]
    );
    assert_eq!(
        line_of("ledger", &t, "apply account Assets"),
        [("apply", Keyword), ("account", Keyword), ("Assets", Type)]
    );
    assert_eq!(
        line_of("ledger", &t, "alias chk"),
        [("alias", Keyword), ("chk", Type)]
    );
}

#[test]
fn a_beancount_transaction() {
    let t = read("corpus/beancount/household.beancount");
    assert_eq!(
        line_of("beancount", &t, "2026-01-02 *"),
        [
            ("2026-01-02", Number),
            ("*", Keyword),
            ("\"Ziraat\"", Str),
            ("\"Salary, January\"", Str),
            ("#payroll", Tag),
            ("^payslip-2026-01", Constant),
        ]
    );
    assert_eq!(
        line_of("beancount", &t, "receipt:"),
        [("receipt", Tag), ("\"P-0001\"", Str)]
    );
    assert_eq!(
        line_of("beancount", &t, "-45000.00 TRY"),
        [
            ("Income:Salary", Type),
            ("-45000.00", Number),
            ("TRY", Constant)
        ]
    );
    assert_eq!(
        line_of("beancount", &t, "{350.00 TRY} @"),
        [
            ("Assets:Broker:FUND", Type),
            ("-4", Number),
            ("FUND", Constant),
            ("350.00", Number),
            ("TRY", Constant),
            ("@", Operator),
            ("380.00", Number),
            ("TRY", Constant),
        ]
    );
    // A sum as an amount: the sign is the number's, the operator not.
    assert_eq!(
        line_of("beancount", &t, "800.00 + 42.35"),
        [
            ("Expenses:Food:Market", Type),
            ("800.00", Number),
            ("+", Operator),
            ("42.35", Number),
            ("TRY", Constant),
        ]
    );
    assert_eq!(
        line_of("beancount", &t, "txn"),
        [
            ("2026-01-15", Number),
            ("txn", Keyword),
            ("\"Market\"", Str),
            ("\"Groceries, two receipts\"", Str),
        ]
    );
    assert_eq!(
        line_of("beancount", &t, "balance Assets:Bank:Ziraat     50000.00"),
        [
            ("2026-01-01", Number),
            ("balance", Keyword),
            ("Assets:Bank:Ziraat", Type),
            ("50000.00", Number),
            ("TRY", Constant),
        ]
    );
    assert_eq!(
        line_of("beancount", &t, "pushtag"),
        [("pushtag", Keyword), ("#trip", Tag)]
    );
    assert_eq!(
        line_of("beancount", &t, "option \"title\""),
        [
            ("option", Keyword),
            ("\"title\"", Str),
            ("\"Household\"", Str)
        ]
    );
    let a = read("corpus/beancount/accounts.beancount");
    assert_eq!(
        line_of("beancount", &a, "Assets:Cash"),
        [
            ("2025-01-01", Number),
            ("open", Keyword),
            ("Assets:Cash", Type),
            ("TRY", Constant),
            ("EUR", Constant),
        ]
    );
}

#[test]
fn hledger_csv_rules() {
    let r = read("corpus/hledger/statement.csv.rules");
    assert_eq!(
        line_of("hledger-rules", &r, "fields"),
        [
            ("fields", Keyword),
            ("date", Tag),
            ("description", Tag),
            ("amount", Tag),
            ("balance", Tag),
        ]
    );
    assert_eq!(
        line_of("hledger-rules", &r, "date-format"),
        [
            ("date-format", Keyword),
            ("%Y", Constant),
            ("%m", Constant),
            ("%d", Constant),
        ]
    );
    assert_eq!(
        line_of("hledger-rules", &r, "account1"),
        [("account1", Tag), ("assets:bank:ziraat", Type)]
    );
    assert_eq!(
        line_of("hledger-rules", &r, "if %description"),
        [("if", Keyword), ("%description", Constant), ("MIGROS", Str)]
    );
    assert_eq!(
        line_of("hledger-rules", &r, "& %amount"),
        [("&", Operator), ("%amount", Constant), ("^-", Str)]
    );
    assert_eq!(
        line_of("hledger-rules", &r, " account2 expenses:food:market"),
        [("account2", Tag), ("expenses:food:market", Type)]
    );
    assert_eq!(line_of("hledger-rules", &r, "SALARY"), [("SALARY", Str)]);
    assert_eq!(
        line_of("hledger-rules", &r, "if,account2"),
        [("if", Keyword), ("account2", Tag), ("comment", Tag)]
    );
    assert_eq!(
        line_of("hledger-rules", &r, "KAFE|RESTORAN"),
        [("KAFE|RESTORAN", Str)]
    );
    assert_eq!(
        line_of("hledger-rules", &r, "# Ziraat"),
        [(r.lines().next().unwrap(), Comment)]
    );
}

/// The cases a journal syntax gets wrong easily (pta_todo.md PT2).
#[test]
fn the_cases_that_go_wrong_easily() {
    let t = read("corpus/hledger/2026.journal");
    // An account with a space in its name, and a quoted commodity with a
    // space in it.
    assert_eq!(
        line_of("hledger", &t, "\"FUND A\" @"),
        [
            ("assets:broker:index fund", Type),
            ("10,00", Number),
            ("\"FUND A\"", Constant),
            ("@", Operator),
            ("350,00", Number),
            ("TRY", Constant),
        ]
    );
    // A negative amount in parentheses, a Ledger value expression.
    let l = read("corpus/ledger/business.ledger");
    assert_eq!(
        line_of("ledger", &l, "(-$132.00)"),
        [
            ("chk", Type),
            ("-", Number),
            ("$", Constant),
            ("132.00", Number)
        ]
    );
    // A comment with a colon that is no tag: the file's own comments
    // carry none (hledger reads tags only in a transaction's, a posting's
    // and an account's), nor do Beancount's.
    assert_eq!(
        line_of("hledger", &t, "LEDGER_FILE names:"),
        [(t.lines().next().unwrap(), Comment)]
    );
    assert_eq!(
        colored("beancount", "2026-01-01 open Assets:X ; note: none\n")[0].last(),
        Some(&("; note: none", Comment))
    );
    // Ledger's payee ends at a `;` after a tab or two spaces only.
    assert_eq!(
        colored("ledger", "2026/01/01 Fish; chips  ; :food:\n")[0],
        [
            ("2026/01/01", Number),
            ("Fish; chips", Str),
            ("; ", Comment),
            (":food:", Tag),
        ]
    );
    // A posting's status, and Ledger's lot price and date.
    assert_eq!(
        colored(
            "ledger",
            "2026/01/01 x\n  * Assets:Fund  10 AAPL {$30} [2025/12/01] @ $35\n"
        )[1],
        [
            ("*", Keyword),
            ("Assets:Fund", Type),
            ("10", Number),
            ("AAPL", Constant),
            ("$", Constant),
            ("30", Number),
            ("2025/12/01", Number),
            ("@", Operator),
            ("$", Constant),
            ("35", Number),
        ]
    );
}

/// The tags and the accounts the hledger syntax colors in the corpus are
/// those hledger reads in it, where hledger is installed.
#[test]
fn hledger_reads_the_tags_and_accounts_colored() {
    let files = ["2026.journal", "accounts.journal", "2025.journal"];
    let mut tags = BTreeSet::new();
    let mut accounts = BTreeSet::new();
    for f in files {
        let text = read(&format!("corpus/hledger/{f}"));
        for line in colored("hledger", &text) {
            for (s, k) in line {
                match k {
                    Tag => {
                        tags.insert(s.trim_end_matches(':').to_string());
                    }
                    Type => {
                        accounts.insert(s.to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    let said = |args: &[&str]| -> Option<BTreeSet<String>> {
        let out = match Command::new("hledger")
            .args(["-f", "2026.journal", "--auto"])
            .args(args)
            .current_dir(dir().join("corpus/hledger"))
            .output()
        {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("hledger is not installed: its tags and accounts are not compared");
                return None;
            }
            Err(e) => panic!("hledger: {e}"),
        };
        assert!(out.status.success(), "{out:?}");
        Some(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::to_string)
                .collect(),
        )
    };
    let Some(read_tags) = said(&["tags"]) else {
        return;
    };
    // Less the hidden tags hledger adds itself (`_generated-posting`
    // on the automated postings).
    let read_tags: BTreeSet<String> = read_tags
        .into_iter()
        .filter(|t| !t.starts_with('_'))
        .collect();
    assert_eq!(tags, read_tags);
    assert_eq!(Some(accounts), said(&["accounts"]));
}
