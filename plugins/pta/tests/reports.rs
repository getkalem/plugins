//! The balance (PT7a) as the plugin asks each tool for it, on the
//! corpus: each section's accounts with their subaccounts' amounts, and
//! its total, read from what the tool printed; at market value too
//! (PT10). A tool not installed is skipped with a note.

// A tool not installed is said on standard error.
#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_plugin_pta::Dialect;
use kalem_plugin_pta::report::{self, Part, Period, Row, Section};
use kalem_plugin_pta::write::Date;

fn corpus(d: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus").join(d)
}

fn run(program: &str, args: &[String], dir: &Path) -> Option<String> {
    let o = match Command::new(program).args(args).current_dir(dir).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{program} is not installed: its balance is not read");
            return None;
        }
        Err(e) => panic!("{program}: {e}"),
    };
    assert!(
        o.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The section as the component reads it: hledger's and Ledger's one
/// run, Beancount's levels and its total.
fn section(tool: Dialect, file: &str, s: Section, bounds: Option<(Date, Date)>) -> Option<Part> {
    section_as(tool, file, s, bounds, false)
}

/// The section, at market value where `valued`.
fn section_as(
    tool: Dialect,
    file: &str,
    s: Section,
    bounds: Option<(Date, Date)>,
    valued: bool,
) -> Option<Part> {
    let dir = corpus(tool.language());
    let main = dir.join(file).to_string_lossy().into_owned();
    let command = |depth, level| {
        let (p, mut a) = report::command(tool, &main, s, bounds, depth, level);
        if valued {
            a = report::value_args(tool, &mut a);
        }
        (p, a)
    };
    Some(match tool {
        Dialect::Hledger | Dialect::Ledger => {
            let (p, a) = command(0, 0);
            let out = run(p, &a, &dir)?;
            Part::of(if tool == Dialect::Hledger {
                report::hledger_rows(&out)
            } else {
                report::ledger_rows(&out)
            })
        }
        Dialect::Beancount => {
            let mut levels: Vec<Vec<Row>> = Vec::new();
            for level in 1..=4 {
                let (p, a) = command(0, level);
                levels.push(report::beancount_rows(&run(p, &a, &dir)?));
            }
            let (p, a) = command(0, 0);
            let total = report::beancount_rows(&run(p, &a, &dir)?);
            Part {
                rows: report::beancount_tree(&levels),
                total: total
                    .into_iter()
                    .next()
                    .map(|r| r.amounts)
                    .unwrap_or_default(),
            }
        }
    })
}

fn amounts<'a>(p: &'a Part, account: &str) -> &'a [String] {
    &p.rows
        .iter()
        .find(|r| r.account == account)
        .unwrap_or_else(|| panic!("{account} in {:?}", p.rows))
        .amounts
}

#[test]
fn hledgers_balance() {
    let Some(sheet) = section(Dialect::Hledger, "2026.journal", Section::Sheet, None) else {
        return;
    };
    assert_eq!(amounts(&sheet, "assets:bank:ziraat"), ["63.697,25 TRY"]);
    assert_eq!(amounts(&sheet, "assets:bank"), ["63.697,25 TRY"]);
    assert!(amounts(&sheet, "assets").contains(&"65.197,25 TRY".to_string()));
    assert!(!sheet.total.is_empty());
    assert!(
        sheet
            .rows
            .iter()
            .all(|r| !r.account.starts_with("expenses"))
    );
    let flow = section(Dialect::Hledger, "2026.journal", Section::Flow, None).expect("ran");
    assert_eq!(amounts(&flow, "expenses:food:market"), ["2.092,75 TRY"]);
    assert!(flow.rows.iter().all(|r| !r.account.starts_with("assets")));
    // The period's changes: January 2026 alone, not December 2025's.
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
    let flow = section(Dialect::Hledger, "2026.journal", Section::Flow, jan).expect("ran");
    assert_eq!(amounts(&flow, "expenses:food:market"), ["842,35 TRY"]);
    let sheet = section(Dialect::Hledger, "2026.journal", Section::Sheet, jan).expect("ran");
    assert_eq!(
        amounts(&sheet, "assets:bank:ziraat"),
        ["63.697,25 TRY"],
        "a balance at the end"
    );
}

#[test]
fn ledgers_balance() {
    let Some(sheet) = section(Dialect::Ledger, "business.ledger", Section::Sheet, None) else {
        return;
    };
    assert_eq!(amounts(&sheet, "Assets:Checking"), ["$11,619.00"]);
    assert!(
        sheet
            .rows
            .iter()
            .all(|r| !r.account.starts_with("Expenses"))
    );
    let flow = section(Dialect::Ledger, "business.ledger", Section::Flow, None).expect("ran");
    assert_eq!(amounts(&flow, "Expenses"), ["$49.00", "EUR 120.00"]);
    assert!(flow.rows.iter().any(|r| r.account == "Budget:Travel"));
    assert!(flow.rows.iter().all(|r| !r.account.starts_with("Assets")));
    assert!(!flow.total.is_empty());
}

#[test]
fn beancounts_balance() {
    let Some(sheet) = section(
        Dialect::Beancount,
        "household.beancount",
        Section::Sheet,
        None,
    ) else {
        return;
    };
    assert!(sheet.rows.iter().any(|r| r.account == "Assets:Bank:Ziraat"));
    assert!(amounts(&sheet, "Assets").contains(&"200.00 EUR".to_string()));
    assert!(
        sheet
            .rows
            .iter()
            .all(|r| !r.account.starts_with("Expenses"))
    );
    let flow = section(
        Dialect::Beancount,
        "household.beancount",
        Section::Flow,
        None,
    )
    .expect("ran");
    assert!(
        amounts(&flow, "Expenses:Food")
            .iter()
            .any(|a| a.ends_with("TRY"))
    );
    assert!(!flow.total.is_empty());
    let _ = Period::All;
}

/// Market values (PT10): each tool values at its prices, hledger's
/// inferred from the costs, Ledger's from its `P` directives,
/// Beancount's from its prices and costs.
#[test]
fn each_tools_market_value() {
    let Some(sheet) = section_as(Dialect::Hledger, "2026.journal", Section::Sheet, None, true)
    else {
        return;
    };
    assert_eq!(amounts(&sheet, "assets:cash"), ["12.210,00 TRY"]);
    assert_eq!(amounts(&sheet, "assets"), ["79.407,25 TRY"]);
    if let Some(flow) = section_as(
        Dialect::Ledger,
        "business.ledger",
        Section::Flow,
        None,
        true,
    ) {
        assert_eq!(amounts(&flow, "Expenses"), ["$183.40"], "EUR at $1.12");
    }
    if let Some(sheet) = section_as(
        Dialect::Beancount,
        "household.beancount",
        Section::Sheet,
        None,
        true,
    ) {
        assert!(
            amounts(&sheet, "Assets").contains(&"87157.65 TRY".to_string()),
            "6 FUND at 380.00 TRY: {:?}",
            amounts(&sheet, "Assets")
        );
    }
}

/// The register (PT7b) of an account, each tool's: its postings with the
/// running total, and where each is (hledger's through `print -O json`).
#[test]
fn each_tools_register() {
    for (tool, file, account, first_total) in [
        (
            Dialect::Hledger,
            "2026.journal",
            "assets:bank:ziraat",
            "50.000,00 TRY",
        ),
        (
            Dialect::Ledger,
            "business.ledger",
            "Assets:Checking",
            "$10,000.00",
        ),
        (
            Dialect::Beancount,
            "household.beancount",
            "Assets:Bank:Ziraat",
            "50000.00 TRY",
        ),
    ] {
        let dir = corpus(tool.language());
        let main = dir.join(file).to_string_lossy().into_owned();
        let (p, a) = report::register_command(tool, &main, account, None);
        let Some(out) = run(p, &a, &dir) else {
            continue;
        };
        let rows = match tool {
            Dialect::Hledger => kalem_plugin_pta::reconcile::hledger_register(&out),
            Dialect::Ledger => report::ledger_register(&out),
            Dialect::Beancount => report::beancount_register(&out),
        };
        assert!(rows.len() >= 4, "{tool:?}: {rows:?}");
        assert_eq!(rows[0].total, first_total, "{tool:?}");
        // The account's postings only: its last total is its balance.
        let last = &rows[rows.len() - 1].total;
        let want = match tool {
            Dialect::Hledger => "63.697,25 TRY",
            Dialect::Ledger => "$11,619.00",
            Dialect::Beancount => "76857.65 TRY",
        };
        assert_eq!(last, want, "{tool:?}");
        // Within a period from mid-January, the same balance at the end:
        // the postings before it counted, not left out.
        let mid = Some((
            Date {
                y: 2026,
                m: 1,
                d: 15,
            },
            Date {
                y: 2026,
                m: 12,
                d: 1,
            },
        ));
        let (p, a) = report::register_command(tool, &main, account, mid);
        let out = run(p, &a, &dir).expect("ran");
        let within = match tool {
            Dialect::Hledger => kalem_plugin_pta::reconcile::hledger_register(&out),
            Dialect::Ledger => report::ledger_register(&out),
            Dialect::Beancount => report::beancount_register(&out),
        };
        assert!(within.len() < rows.len(), "{tool:?}: {within:?}");
        assert_eq!(&within[within.len() - 1].total, want, "{tool:?} within");
        let place = match tool {
            Dialect::Hledger => {
                let json = run(
                    "hledger",
                    &[
                        "-f".into(),
                        main.clone(),
                        "print".into(),
                        "-O".into(),
                        "json".into(),
                    ],
                    &dir,
                )
                .expect("ran");
                report::hledger_place(&json, rows[0].transaction.expect("its number"))
            }
            _ => rows[0].place.clone(),
        };
        let (f, line) = place.unwrap_or_else(|| panic!("{tool:?}: no place"));
        let text = std::fs::read_to_string(&f).expect("its file");
        let l = text.lines().nth(line as usize - 1).expect("its line");
        assert!(l.chars().next().is_some(), "{tool:?}: {f}:{line}");
    }
}

/// Reconciliation (PT8) as each tool totals it, on the corpus: the
/// cleared and pending postings' total, the postings not cleared, and
/// the difference to a target typed in the journal's style.
#[test]
fn each_tools_reconciliation() {
    use kalem_plugin_pta::reconcile::{self, Which};
    for (tool, file, account, both, open, target, difference) in [
        (
            Dialect::Hledger,
            "2026.journal",
            "assets:bank:ziraat",
            "63.697,25 TRY",
            1,
            "63.700,00",
            "2,75 TRY",
        ),
        (
            Dialect::Ledger,
            "business.ledger",
            "Assets:Checking",
            "$11,619.00",
            1,
            "$11,600.00",
            "$-19.00",
        ),
        (
            Dialect::Beancount,
            "household.beancount",
            "Liabilities:Card",
            "-1120.00 TRY",
            1,
            "-1120.00",
            "0.00 TRY",
        ),
    ] {
        let dir = corpus(tool.language());
        let main = dir.join(file).to_string_lossy().into_owned();
        let (p, a) = reconcile::total_command(tool, &main, account, Which::Both);
        let Some(out) = run(p, &a, &dir) else {
            continue;
        };
        let totals = reconcile::total_of(tool, &out);
        let total = reconcile::matching(target, &totals).expect("a total");
        assert_eq!(total.written(), both, "{tool:?}");
        let d = reconcile::difference(target, total).expect("the target reads");
        assert_eq!(d.written(), difference, "{tool:?}");
        let (p, a) = reconcile::open_command(tool, &main, account);
        let postings = reconcile::open_of(tool, &run(p, &a, &dir).expect("ran"));
        assert_eq!(postings.len(), open, "{tool:?}: {postings:?}");
        assert!(postings.iter().all(|p| p.status != '*'), "{tool:?}");
    }
}

/// A statement beside the reconciliation (PT8c): its lines paired with
/// the account's postings the tool lists in the statement's days.
#[test]
fn a_statement_paired() {
    use kalem_plugin_pta::statement;
    let csv = "Date,Description,Amount\n2026-01-03,MIGROS,-842.35\n2026-01-31,KIRA,-15000.00\n2026-01-02,MAAS,45000.00\n2026-01-20,UNKNOWN FEE,-5.00\n";
    let lines = statement::lines(csv).expect("lines");
    let span = statement::span(&lines, 3);
    let dir = corpus("hledger");
    let main = dir.join("2026.journal").to_string_lossy().into_owned();
    let (p, a) = report::register_command(Dialect::Hledger, &main, "assets:bank:ziraat", span);
    let Some(out) = run(p, &a, &dir) else {
        return;
    };
    let rows = kalem_plugin_pta::reconcile::hledger_register(&out);
    let paired = statement::pair(&lines, &rows, 3);
    assert_eq!(paired.found.len(), 3, "{rows:?}");
    assert_eq!(paired.alone.len(), 1);
    assert_eq!(paired.alone[0].description, "UNKNOWN FEE");
    let market = rows
        .iter()
        .find(|r| r.amount == "-842,35 TRY")
        .expect("the market");
    assert!(paired.found.contains(&statement::key(market)));
}

/// The register narrowed by each tool's own query (PT7b's `/`).
#[test]
fn a_narrowed_register() {
    for (tool, file, account, query, payee) in [
        (
            Dialect::Hledger,
            "2026.journal",
            "assets:bank:ziraat",
            "desc:rent",
            "Rent",
        ),
        (
            Dialect::Ledger,
            "business.ledger",
            "Assets:Checking",
            "payee Client",
            "Client A pays",
        ),
        (
            Dialect::Beancount,
            "household.beancount",
            "Assets:Bank:Ziraat",
            "payee ~ 'Market'",
            "Market",
        ),
    ] {
        let dir = corpus(tool.language());
        let main = dir.join(file).to_string_lossy().into_owned();
        let (p, a) = report::narrowed_register(tool, &main, account, None, query);
        let Some(out) = run(p, &a, &dir) else {
            continue;
        };
        let rows = match tool {
            Dialect::Hledger => kalem_plugin_pta::reconcile::hledger_register(&out),
            Dialect::Ledger => report::ledger_register(&out),
            Dialect::Beancount => report::beancount_register(&out),
        };
        assert_eq!(rows.len(), 1, "{tool:?}: {rows:?}");
        assert!(rows[0].description.starts_with(payee), "{tool:?}: {rows:?}");
    }
}

/// A posting written with an alias or under `apply account` is the
/// account the tool reads, as Reconcile and Finish take it: Ledger's
/// corpus (`chk`, `apply account Assets`), and an hledger journal with
/// both.
#[test]
fn accounts_the_tools_read() {
    use kalem_plugin_pta::reconcile;
    let line = |text: &str, s: &str| text.lines().position(|l| l == s).expect(s) as u32 + 1;
    let dir = corpus("ledger");
    let main = dir.join("business.ledger").to_string_lossy().into_owned();
    let text = std::fs::read_to_string(&main).expect("corpus");
    let (p, a) = reconcile::accounts_command(Dialect::Ledger, &main).expect("a run");
    if let Some(out) = run(p, &a, &dir) {
        let accounts = reconcile::accounts_in(Dialect::Ledger, &out, &main, &text);
        let chk = text
            .lines()
            .position(|l| l.starts_with("    chk "))
            .unwrap() as u32
            + 1;
        assert_eq!(accounts[&chk], "Assets:Checking");
        assert_eq!(accounts[&line(&text, "    Checking")], "Assets:Checking");
    }
    let dir = std::env::temp_dir().join(format!("pta-accounts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("a.journal").to_string_lossy().into_owned();
    let text = "alias chk = assets:checking\n\n2026-01-05 Grocer\n    expenses:food  $42.35\n    ; a note\n    chk\n\napply account assets\n2026-01-06 Move\n    savings  $10\n    checking\nend apply account\n";
    std::fs::write(&main, text).unwrap();
    let (p, a) = reconcile::accounts_command(Dialect::Hledger, &main).expect("a run");
    if let Some(out) = run(p, &a, &dir) {
        let accounts = reconcile::accounts_in(Dialect::Hledger, &out, &main, text);
        assert_eq!(accounts[&line(text, "    chk")], "assets:checking");
        assert_eq!(accounts[&line(text, "    savings  $10")], "assets:savings");
        assert_eq!(accounts[&line(text, "    checking")], "assets:checking");
        assert_eq!(accounts.len(), 4, "{accounts:?}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Ledger's total of a commodity with digits in its name (`"VWRL2"`,
/// quoted as Ledger writes it) reads, and so does its difference.
#[test]
fn a_commodity_with_digits() {
    use kalem_plugin_pta::reconcile::{self, Which};
    let dir = std::env::temp_dir().join(format!("pta-digits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("a.ledger").to_string_lossy().into_owned();
    let text = "2026/01/01 * Buy\n    Assets:Broker  10 \"VWRL2\" @ $100.00\n    Assets:Cash\n\n2026/01/02 ! Buy\n    Assets:Broker  5 \"VWRL2\" @ $100.00\n    Assets:Cash\n";
    std::fs::write(&main, text).unwrap();
    let (p, a) = reconcile::total_command(Dialect::Ledger, &main, "Assets:Broker", Which::Both);
    if let Some(out) = run(p, &a, &dir) {
        let totals = reconcile::total_of(Dialect::Ledger, &out);
        assert_eq!(totals.len(), 1, "{out}");
        assert_eq!(totals[0].written(), "15 \"VWRL2\"");
        let d = reconcile::difference("15 \"VWRL2\"", &totals[0]).expect("the target reads");
        assert!(d.quantity.is_zero(), "{}", d.written());
    }
    let _ = std::fs::remove_dir_all(dir);
}
