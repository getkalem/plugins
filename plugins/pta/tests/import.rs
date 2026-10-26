//! Import from bank statements (PT9) as the plugin runs the tools, on a
//! copy of the corpus: hledger's dry run of `statement.csv` through its
//! rules, the import itself (the journal grows, a second import finds
//! nothing new), a statement without rules (the error read, then rules
//! from the template that hledger reads), and Ledger's conversion. A
//! tool not installed is skipped with a note.

// A tool not installed is said on standard error.
#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_plugin_pta::Dialect;
use kalem_plugin_pta::import;

fn corpus(d: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus").join(d)
}

/// A copy of corpus folder `d`, removed when dropped.
struct Copy(PathBuf);

impl Copy {
    fn of(d: &str, tag: &str) -> Copy {
        let to = std::env::temp_dir().join(format!("pta-import-{d}-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&to);
        std::fs::create_dir_all(&to).expect("copy");
        for e in std::fs::read_dir(corpus(d)).expect("corpus") {
            let p = e.expect("entry").path();
            std::fs::copy(&p, to.join(p.file_name().expect("name"))).expect("copy");
        }
        Copy(to)
    }

    fn path(&self, f: &str) -> String {
        self.0.join(f).to_string_lossy().into_owned()
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What `program` printed (standard output, standard error, success),
/// or `None` where it is not installed.
fn run(program: &str, args: &[String], dir: &Path) -> Option<(String, String, bool)> {
    match Command::new(program).args(args).current_dir(dir).output() {
        Ok(o) => Some((
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
            o.status.success(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{program} is not installed: its import is not tried");
            None
        }
        Err(e) => panic!("{program}: {e}"),
    }
}

#[test]
fn hledgers_import() {
    let c = Copy::of("hledger", "import");
    let (journal, csv) = (c.path("2026.journal"), c.path("statement.csv"));
    let Some((out, err, ok)) = run(
        "hledger",
        &import::hledger_command(&journal, &csv, None, true),
        &c.0,
    ) else {
        return;
    };
    assert!(ok, "{err}");
    let (n, text) = import::drafted(Dialect::Hledger, &out);
    assert_eq!(n, 4, "{out}");
    assert!(
        text.contains("expenses:food:market") && text.contains("63.284,75 TRY"),
        "{text}"
    );
    let before = std::fs::read_to_string(&journal).expect("journal");
    let (_, err, ok) = run(
        "hledger",
        &import::hledger_command(&journal, &csv, None, false),
        &c.0,
    )
    .expect("ran");
    assert!(ok, "{err}");
    let after = std::fs::read_to_string(&journal).expect("journal");
    assert!(
        after.starts_with(&before) && after.len() > before.len(),
        "the journal grew at its end"
    );
    assert!(c.0.join(".latest.statement.csv").is_file());
    let (out, _, ok) = run(
        "hledger",
        &import::hledger_command(&journal, &csv, None, true),
        &c.0,
    )
    .expect("ran");
    assert!(ok);
    assert_eq!(
        import::drafted(Dialect::Hledger, &out).0,
        0,
        "nothing new: {out}"
    );
}

#[test]
fn a_statement_without_rules() {
    let c = Copy::of("hledger", "rules");
    std::fs::copy(c.0.join("statement.csv"), c.0.join("other.csv")).expect("copy");
    let (journal, csv) = (c.path("2026.journal"), c.path("other.csv"));
    let Some((_, err, ok)) = run(
        "hledger",
        &import::hledger_command(&journal, &csv, None, true),
        &c.0,
    ) else {
        return;
    };
    assert!(!ok);
    let missing = import::missing_rules(&err).expect("the rules named");
    assert!(missing.ends_with("other.csv.rules"), "{missing}");
    let text = std::fs::read_to_string(&csv).expect("statement");
    std::fs::write(
        c.0.join("other.csv.rules"),
        import::rules_template("other.csv", &text, "assets:bank:ziraat"),
    )
    .expect("rules");
    let (out, err, ok) = run(
        "hledger",
        &import::hledger_command(&journal, &csv, None, true),
        &c.0,
    )
    .expect("ran");
    assert!(ok, "{err}");
    let (n, text) = import::drafted(Dialect::Hledger, &out);
    assert_eq!(n, 4, "{out}");
    assert!(text.contains("assets:bank:ziraat"), "{text}");
}

/// The template's rules read a statement whose months and days are
/// written with one digit and with two: hledger's `%m` takes only two.
#[test]
fn dates_without_a_leading_zero() {
    let c = Copy::of("hledger", "unpadded");
    let csv = c.path("us.csv");
    let text = "Date,Description,Amount\n2/13/2026,SHOP,-12.00\n02/14/2026,CAFE,-3.50\n3/2/2026,KIOSK,-1.00\n";
    std::fs::write(&csv, text).expect("statement");
    std::fs::write(
        c.0.join("us.csv.rules"),
        import::rules_template("us.csv", text, "assets:bank:ziraat"),
    )
    .expect("rules");
    let journal = c.path("2026.journal");
    let Some((out, err, ok)) = run(
        "hledger",
        &import::hledger_command(&journal, &csv, None, true),
        &c.0,
    ) else {
        return;
    };
    assert!(ok, "{err}");
    let (n, text) = import::drafted(Dialect::Hledger, &out);
    assert_eq!(n, 3, "{out}");
    for date in ["2026-02-13", "2026-02-14", "2026-03-02"] {
        assert!(text.contains(date), "{date}: {text}");
    }
}

#[test]
fn ledgers_conversion() {
    let c = Copy::of("ledger", "convert");
    std::fs::write(
        c.0.join("stmt.csv"),
        "Date,Payee,Amount\n2026/02/05,Client B,500.00\n2026/02/07,Cafe,-12.50\n",
    )
    .expect("statement");
    let args = import::ledger_command(
        &c.path("business.ledger"),
        &c.path("stmt.csv"),
        "Assets:Checking",
        "%Y/%m/%d",
    );
    let Some((out, err, ok)) = run("ledger", &args, &c.0) else {
        return;
    };
    assert!(ok, "{err}");
    let (n, text) = import::drafted(Dialect::Ledger, &out);
    assert_eq!(n, 2, "{out}");
    assert!(text.contains("Assets:Checking"));
}

/// Beancount's import (PT9b): the user's beangulp script run as the
/// plugin runs it, its new entries read, and nothing new once they are
/// in the journal (beangulp comments them out as duplicates). The
/// Python is `PTA_PYTHON`'s, else `python3`; one without beangulp skips
/// the test with a note.
#[test]
fn beancounts_import() {
    let python = std::env::var("PTA_PYTHON").unwrap_or_else(|_| "python3".into());
    let has = Command::new(&python)
        .args(["-c", "import beangulp"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !has {
        eprintln!("{python} has no beangulp: Beancount's import is not tried");
        return;
    }
    let c = Copy::of("beancount", "beangulp");
    std::fs::write(
        c.0.join("import.py"),
        "import beangulp\nfrom beangulp.importers import csvbase\n\n\nclass Bank(csvbase.Importer):\n    date = csvbase.Date(\"Date\", \"%Y-%m-%d\")\n    narration = csvbase.Column(\"Description\")\n    amount = csvbase.Amount(\"Amount\")\n\n    def identify(self, filepath):\n        return filepath.endswith(\".csv\")\n\n\nif __name__ == \"__main__\":\n    beangulp.Ingest([Bank(\"Assets:Bank:Ziraat\", \"TRY\")])()\n",
    )
    .expect("script");
    std::fs::write(
        c.0.join("feb.csv"),
        "Date,Description,Amount\n2026-02-03,MIGROS KADIKOY,-412.50\n2026-02-05,MAAS ODEMESI,45000.00\n",
    )
    .expect("statement");
    let journal = c.path("household.beancount");
    let script = import::beangulp_script(&journal, "");
    let args = import::beangulp_command(&script, &c.path("feb.csv"), &journal);
    let (out, err, ok) = run(&python, &args, &c.0).expect("ran");
    assert!(ok, "{err}");
    let (n, text) = import::drafted(Dialect::Beancount, &out);
    assert_eq!(n, 2, "{out}");
    assert!(
        !text.contains("****") && text.contains("Assets:Bank:Ziraat"),
        "{text}"
    );
    let mut j = std::fs::read_to_string(&journal).expect("journal");
    j.push('\n');
    j.push_str(&text);
    std::fs::write(&journal, j).expect("journal");
    let (out, err, ok) = run(&python, &args, &c.0).expect("ran");
    assert!(ok, "{err}");
    assert_eq!(import::drafted(Dialect::Beancount, &out).0, 0, "{out}");
}
