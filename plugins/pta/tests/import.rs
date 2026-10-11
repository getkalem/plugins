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
