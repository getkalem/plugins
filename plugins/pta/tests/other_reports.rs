//! The other reports (PT7c) as the plugin runs them, on the corpus:
//! each report of each tool runs and prints something, the dated ones
//! within a period too, a pinned report of the setting's form runs, and
//! a Beancount journal's saved queries are listed by `bean-query` and
//! run. A tool not installed is skipped with a note.

// A tool not installed is said on standard error.
#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_plugin_pta::Dialect;
use kalem_plugin_pta::other::{self, Report};
use kalem_plugin_pta::write::Date;

fn corpus(d: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus").join(d)
}

const TODAY: Date = Date {
    y: 2026,
    m: 10,
    d: 11,
};

/// What the report printed, or `None` where its program is not
/// installed; a failure fails the test.
fn run(tool: Dialect, main: &Path, r: &Report, bounds: Option<(Date, Date)>) -> Option<String> {
    let (program, args) = other::command(tool, &main.to_string_lossy(), r, bounds);
    let o = match Command::new(program)
        .args(&args)
        .current_dir(main.parent().expect("folder"))
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{program} is not installed: its reports are not run");
            return None;
        }
        Err(e) => panic!("{program}: {e}"),
    };
    assert!(
        o.status.success(),
        "{} {args:?}: {}",
        r.title,
        String::from_utf8_lossy(&o.stderr)
    );
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

#[test]
fn each_tools_reports_run() {
    let jan = Some((
        Date {
            y: 2026,
            m: 1,
            d: 1,
        },
        Date {
            y: 2026,
            m: 2,
            d: 1,
        },
    ));
    for (tool, file) in [
        (Dialect::Hledger, "hledger/2026.journal"),
        (Dialect::Ledger, "ledger/business.ledger"),
        (Dialect::Beancount, "beancount/household.beancount"),
    ] {
        let main = corpus("").join(file);
        for r in other::reports(tool, TODAY) {
            let Some(out) = run(tool, &main, &r, None) else {
                break;
            };
            // Ledger's budget and forecast need periodic transactions,
            // which its corpus has none of.
            let empty_ok =
                tool == Dialect::Ledger && matches!(r.title.as_str(), "Budget" | "Forecast");
            assert!(empty_ok || !out.trim().is_empty(), "{tool:?} {}", r.title);
            if r.dated {
                run(tool, &main, &r, jan).expect("ran");
            }
        }
    }
    let main = corpus("hledger").join("2026.journal");
    let p = other::pinned(
        Dialect::Hledger,
        &["Food by month: balance 'expenses:food' -M".into()],
    );
    if let Some(out) = run(Dialect::Hledger, &main, &p[0], jan) {
        assert!(out.contains("expenses:food"), "{out}");
    }
}

#[test]
fn beancounts_saved_queries() {
    let dir = std::env::temp_dir().join(format!("pta-queries-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("folder");
    for e in std::fs::read_dir(corpus("beancount")).expect("corpus") {
        let p = e.expect("entry").path();
        std::fs::copy(&p, dir.join(p.file_name().expect("name"))).expect("copy");
    }
    let main = dir.join("household.beancount");
    let mut text = std::fs::read_to_string(&main).expect("journal");
    text.push_str("\n2026-01-01 query \"food\" \"SELECT account, sum(position) WHERE account ~ 'Food' GROUP BY account\"\n");
    std::fs::write(&main, text).expect("journal");
    let listed = Command::new("bean-query")
        .args(other::saved_queries_args(&main.to_string_lossy()))
        .output();
    let listed = match listed {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => {
            eprintln!("bean-query is not installed: the saved queries are not run");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
    };
    let saved = other::saved_queries(&listed);
    assert_eq!(saved.len(), 1, "{listed}");
    assert_eq!(saved[0].title, "Query: food");
    let out = run(Dialect::Beancount, &main, &saved[0], None).expect("ran");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.contains("Expenses:Food:Market"), "{out}");
}
