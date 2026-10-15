//! The accountant's journal (PT13), measured: 200,000 transactions over
//! ten yearly files, written for hledger and Ledger (one syntax both
//! read) and for Beancount, in `PTA_LARGE` (a folder; the temporary
//! one by default), then each tool asked as the plugin asks it, with the
//! time it took and the size of what it printed against `process`'s 16
//! MB, and hledger's register of the busiest account (some 100,000
//! postings) read by the plugin's parser. Run by hand: `cargo test -p
//! kalem-plugin-pta --test large -- --ignored --nocapture`.

// The measures are printed.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use kalem_plugin_pta::Dialect;
use kalem_plugin_pta::report::{self, Period, Section};
use kalem_plugin_pta::write::Date;

const YEARS: i32 = 10;
const PER_YEAR: usize = 20_000;
/// `process`'s limit on what a program prints.
const MAX_OUTPUT: usize = 16 * 1024 * 1024;

const EXPENSES: &[&str] = &[
    "food:market",
    "food:restaurant",
    "rent",
    "utilities:power",
    "utilities:water",
    "utilities:internet",
    "transport:fuel",
    "transport:transit",
    "health",
    "clothes",
    "books",
    "gifts",
    "travel",
    "phone",
    "insurance",
    "taxes",
    "education",
    "repairs",
    "subscriptions",
    "fees",
];
const PAYEES: &[&str] = &[
    "Migros",
    "Carrefour",
    "A101",
    "BIM",
    "Shell",
    "Opet",
    "IETT",
    "Turkcell",
    "Vodafone",
    "Enerjisa",
    "ISKI",
    "Superonline",
    "Kitapyurdu",
    "LC Waikiki",
    "Koton",
    "Pegasus",
    "THY",
    "Allianz",
    "Axa",
    "Starbucks",
    "Kahve Dunyasi",
    "Getir",
    "Yemeksepeti",
    "Trendyol",
    "Hepsiburada",
    "Amazon",
    "Apple",
    "Netflix",
    "Spotify",
    "Eczane",
];

/// A number from a seed, the same every run.
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn folder() -> PathBuf {
    let dir = std::env::var_os("PTA_LARGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("pta-large"));
    std::fs::create_dir_all(&dir).expect("folder");
    dir
}

/// Writes the journals once: `all.journal` including `accounts.journal`
/// and `YYYY.journal`, and `all.beancount` the same in Beancount's.
fn write_journals(dir: &Path) {
    if dir.join("all.journal").is_file() && dir.join("all.beancount").is_file() {
        return;
    }
    let mut seed = 0x2545_F491_4F6C_DD1D_u64;
    let mut accounts = String::from("; The accounts of the large journal.\ncommodity TRY\n");
    let mut bean_accounts = String::from("option \"operating_currency\" \"TRY\"\n\n");
    let all: Vec<String> = EXPENSES
        .iter()
        .map(|e| format!("expenses:{e}"))
        .chain(
            [
                "income:salary",
                "assets:bank:checking",
                "assets:cash",
                "liabilities:card",
                "equity:opening",
            ]
            .map(String::from),
        )
        .collect();
    let bean = |a: &str| {
        a.split(':')
            .map(|p| {
                let mut c = p.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(":")
    };
    for a in &all {
        writeln!(accounts, "account {a}").unwrap();
        writeln!(bean_accounts, "2016-01-01 open {} TRY", bean(a)).unwrap();
    }
    std::fs::write(dir.join("accounts.journal"), accounts).unwrap();
    let mut main =
        String::from("; 200,000 transactions over ten years.\ninclude accounts.journal\n");
    let mut bean_main = String::from("include \"accounts.beancount\"\n");
    bean_main.push_str(&bean_accounts);
    std::fs::write(dir.join("accounts.beancount"), "").unwrap();
    for y in 0..YEARS {
        let year = 2017 + y;
        let (mut text, mut btext) = (String::new(), String::new());
        if y == 0 {
            text.push_str("2017-01-01 * Opening balances\n    assets:bank:checking   250000.00 TRY\n    equity:opening\n\n");
            btext.push_str("2017-01-01 * \"Opening balances\"\n  Assets:Bank:Checking  250000.00 TRY\n  Equity:Opening\n\n");
        }
        for i in 0..PER_YEAR {
            let day = (i * 365 / PER_YEAR) as i64;
            let d = Date {
                y: year,
                m: 1,
                d: 1,
            }
            .plus(day);
            let date = d.written('-');
            let r = next(&mut seed);
            let payee = PAYEES[(r % PAYEES.len() as u64) as usize];
            let status = if r.is_multiple_of(7) { "" } else { "* " };
            let bstatus = if r.is_multiple_of(7) { "!" } else { "*" };
            if i % 400 == 0 {
                // The salary, about monthly.
                writeln!(
                    text,
                    "{date} {status}Employer | salary\n    assets:bank:checking    45000.00 TRY\n    income:salary\n"
                )
                .unwrap();
                writeln!(
                    btext,
                    "{date} {bstatus} \"Employer\" \"salary\"\n  Assets:Bank:Checking  45000.00 TRY\n  Income:Salary\n"
                )
                .unwrap();
                continue;
            }
            let e = EXPENSES[((r >> 8) % EXPENSES.len() as u64) as usize];
            let amount = format!("{}.{:02}", 10 + (r >> 16) % 2000, (r >> 32) % 100);
            let from = match (r >> 40) % 10 {
                0..=6 => "assets:bank:checking",
                7 => "assets:cash",
                _ => "liabilities:card",
            };
            writeln!(
                text,
                "{date} {status}{payee} | purchase {i}\n    expenses:{e}    {amount} TRY\n    {from}\n"
            )
            .unwrap();
            writeln!(
                btext,
                "{date} {bstatus} \"{payee}\" \"purchase {i}\"\n  {}  {amount} TRY\n  {}\n",
                bean(&format!("expenses:{e}")),
                bean(from)
            )
            .unwrap();
        }
        std::fs::write(dir.join(format!("{year}.journal")), text).unwrap();
        std::fs::write(dir.join(format!("{year}.beancount")), btext).unwrap();
        writeln!(main, "include {year}.journal").unwrap();
        writeln!(bean_main, "include \"{year}.beancount\"").unwrap();
    }
    std::fs::write(dir.join("all.journal"), main).unwrap();
    std::fs::write(dir.join("all.beancount"), bean_main).unwrap();
}

/// Runs `program` as the plugin would, printing the time and the size;
/// `None` where it is not installed.
fn measure(what: &str, program: &str, args: &[String], dir: &Path) -> Option<(Vec<u8>, f64)> {
    let start = Instant::now();
    let o = match Command::new(program).args(args).current_dir(dir).output() {
        Ok(o) => o,
        Err(_) => {
            eprintln!("{program} is not installed: {what} not measured");
            return None;
        }
    };
    let secs = start.elapsed().as_secs_f64();
    let size = o.stdout.len();
    println!(
        "{what:<42} {secs:>7.2} s  {:>8.1} MB{}{}",
        size as f64 / 1_048_576.0,
        if size > MAX_OUTPUT {
            "  OVER 16 MB"
        } else {
            ""
        },
        if o.status.success() {
            String::new()
        } else {
            format!(
                "  FAILED: {}",
                String::from_utf8_lossy(&o.stderr)
                    .lines()
                    .next()
                    .unwrap_or("")
            )
        }
    );
    Some((o.stdout, secs))
}

#[test]
#[ignore = "minutes of the tools' time: run by hand"]
fn the_accountants_journal() {
    let dir = folder();
    let start = Instant::now();
    write_journals(&dir);
    println!(
        "journals in {} ({:.1} s)",
        dir.display(),
        start.elapsed().as_secs_f64()
    );
    let today = Date {
        y: 2026,
        m: 10,
        d: 11,
    };
    let main = dir.join("all.journal").to_string_lossy().into_owned();
    let bean = dir.join("all.beancount").to_string_lossy().into_owned();
    let check = |tool: Dialect, m: &str| kalem_plugin_pta::check::command(tool, m, &[]);
    measure(
        "hledger check --strict",
        "hledger",
        &check(Dialect::Hledger, &main),
        &dir,
    );
    measure(
        "ledger balance (the check)",
        "ledger",
        &check(Dialect::Ledger, &main),
        &dir,
    );
    measure(
        "bean-check",
        "bean-check",
        &check(Dialect::Beancount, &bean),
        &dir,
    );
    for (tool, m) in [
        (Dialect::Hledger, &main),
        (Dialect::Ledger, &main),
        (Dialect::Beancount, &bean),
    ] {
        for (name, period) in [("all dates", Period::All), ("this year", Period::ThisYear)] {
            let (p, a) = report::command(
                tool,
                m,
                Section::Sheet,
                period.bounds(today),
                0,
                if tool == Dialect::Beancount { 2 } else { 0 },
            );
            measure(
                &format!("{} balance sheet, {name}", tool.language()),
                p,
                &a,
                &dir,
            );
        }
        for (name, period) in [
            ("all dates", Period::All),
            ("this month", Period::ThisMonth),
            ("this year", Period::ThisYear),
        ] {
            let (p, a) = report::register_command(
                tool,
                m,
                if tool == Dialect::Beancount {
                    "Assets:Bank:Checking"
                } else {
                    "assets:bank:checking"
                },
                period.bounds(today),
            );
            let Some((out, _)) = measure(
                &format!("{} register checking, {name}", tool.language()),
                p,
                &a,
                &dir,
            ) else {
                continue;
            };
            if tool == Dialect::Hledger {
                let text = String::from_utf8_lossy(&out);
                let t = Instant::now();
                let rows = kalem_plugin_pta::reconcile::hledger_register(&text);
                println!(
                    "  {} postings read from hledger's JSON in {:.2} s",
                    rows.len(),
                    t.elapsed().as_secs_f64()
                );
            }
        }
    }
}

/// The outline (K5) of the accountant's ten years in one text, as the
/// sidebar asks it once a version: months under years, read from the
/// text alone (`cargo test -p kalem-plugin-pta --test large --
/// --ignored --nocapture outline`).
#[test]
#[ignore = "writes and reads 200,000 transactions"]
fn the_outline_of_a_large_journal() {
    let dir = folder();
    if !dir.join("2026.journal").exists() {
        write_journals(&dir);
    }
    for (tool, ext) in [
        (Dialect::Hledger, "journal"),
        (Dialect::Beancount, "beancount"),
    ] {
        let text: String = (2017..=2026)
            .map(|y| std::fs::read_to_string(dir.join(format!("{y}.{ext}"))).unwrap_or_default())
            .collect();
        let t = Instant::now();
        let heads = kalem_plugin_pta::outline::headings(&text, tool);
        println!(
            "  {} outline of {:.1} MB: {} headings in {:.3} s",
            tool.language(),
            text.len() as f64 / 1e6,
            heads.len(),
            t.elapsed().as_secs_f64()
        );
        assert!(heads.len() > 100, "{tool:?}");
    }
}
