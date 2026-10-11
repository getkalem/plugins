//! Fetch Prices (`SPC m p`, PT10): the user's own price program fetches,
//! the plugin writes what it gave at the end of the prices file (the
//! setting `prices`, else the main journal), one undo step. Beancount's
//! `bean-price --update` on the main journal; for hledger and Ledger,
//! `pricehist` for each pair of the setting `price_sources`, after the
//! journal's last price of it as the tool lists them.

use std::collections::BTreeMap;

use kalem_plugin::{editor, settings, ui};

use super::{NotRun, append, dialect_here, resolve, run, today};
use crate::Dialect;
use crate::journal;
use crate::prices::{self, Source};
use crate::write::{self, Date};

fn own(key: &str) -> String {
    settings::own(key)
        .and_then(|v| serde_json::from_str::<String>(&v).ok())
        .unwrap_or_default()
}

/// Where fetched prices go: the setting `prices` (beside the main
/// journal when relative), else the main journal.
fn target(main: &str) -> String {
    let set = own("prices");
    if set.trim().is_empty() {
        main.to_string()
    } else {
        journal::beside(main, set.trim())
    }
}

/// The last non-empty line a program said on standard error.
fn last_said(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("failed")
        .trim()
        .to_string()
}

/// The notice for a price program not run.
fn not_run(program: &str, install: &str, why: NotRun) {
    let said = match why {
        NotRun::Missing => format!(
            "{program} is not installed: {install}; or give its path as the setting programs.{program}"
        ),
        NotRun::Refused(why) => format!("{program}: {why}"),
    };
    ui::notify(&said, ui::Level::Warning);
}

/// Fetch Prices in the current journal.
pub(super) fn command() -> Result<String, String> {
    let Some((dialect, Some(path))) = dialect_here() else {
        ui::notify("This document is no journal", ui::Level::Info);
        return Ok("null".into());
    };
    let text = editor::text(None);
    let sep = write::separator(&text, dialect);
    let decimal = prices::decimal_mark(&text);
    resolve(path, dialect, move |tool, main| {
        ui::notify("Fetching prices…", ui::Level::Info);
        if tool == Dialect::Beancount {
            bean_price(main.path);
        } else {
            pricehist(tool, main.path, sep, decimal);
        }
    });
    Ok("null".into())
}

/// Beancount's `bean-price --update`: its directives as it writes them.
fn bean_price(main: String) {
    let dir = journal::folder(&main).to_string();
    run(
        "bean-price",
        prices::bean_price_args(&main),
        &dir,
        move |r| match r {
            Ok(e) if e.status == Some(0) => {
                let got = String::from_utf8_lossy(&e.stdout).into_owned();
                fetched(&main, got, Vec::new());
            }
            Ok(e) => ui::notify(
                &format!("bean-price: {}", last_said(&e.stderr)),
                ui::Level::Warning,
            ),
            Err(why) => not_run("bean-price", "pip install beanprice", why),
        },
    );
}

/// A run of `pricehist` over the setting's pairs.
struct Fetch {
    main: String,
    sources: Vec<Source>,
    last: BTreeMap<(String, String), Date>,
    today: Date,
    sep: char,
    decimal: char,
    /// The price directives so far.
    got: String,
    /// The pairs that failed, with what `pricehist` said.
    failed: Vec<String>,
}

/// `pricehist` for each pair of `price_sources`, after the last price
/// the tool lists for it.
fn pricehist(tool: Dialect, main: String, sep: char, decimal: char) {
    let entries = settings::own("price_sources")
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    let sources = prices::sources(&entries);
    if sources.is_empty() {
        ui::notify(
            "Name the prices to fetch in the setting price_sources, such as \"ecb EUR/USD\"",
            ui::Level::Warning,
        );
        return;
    }
    let dir = journal::folder(&main).to_string();
    run(tool.checker(), prices::list_args(&main), &dir, move |r| {
        let listed = r
            .ok()
            .filter(|e| e.status == Some(0))
            .map(|e| String::from_utf8_lossy(&e.stdout).into_owned())
            .unwrap_or_default();
        next(Fetch {
            main,
            sources,
            last: prices::last_dates(&listed),
            today: today(),
            sep,
            decimal,
            got: String::new(),
            failed: Vec::new(),
        });
    });
}

/// Fetches the first pair left, then the rest; the prices written once
/// none is.
fn next(mut f: Fetch) {
    if f.sources.is_empty() {
        let Fetch {
            main, got, failed, ..
        } = f;
        fetched(&main, got, failed);
        return;
    }
    let s = f.sources.remove(0);
    let last = f
        .last
        .get(&(s.base.clone(), s.quote_written().to_string()))
        .copied();
    if last.is_some_and(|d| d >= f.today) {
        next(f);
        return;
    }
    let args = prices::pricehist_args(&s, last, f.today);
    let dir = journal::folder(&f.main).to_string();
    run("pricehist", args, &dir, move |r| {
        match r {
            Ok(e) if e.status == Some(0) => {
                let csv = String::from_utf8_lossy(&e.stdout);
                f.got
                    .push_str(&prices::price_lines(&csv, &s, f.sep, f.decimal));
            }
            Ok(e) => f.failed.push(format!(
                "{} {}/{}: {}",
                s.source,
                s.base,
                s.quote,
                last_said(&e.stderr)
            )),
            Err(why) => {
                not_run("pricehist", "pip install pricehist", why);
                return;
            }
        }
        next(f);
    });
}

/// Says what was fetched and appends it where prices go.
fn fetched(main: &str, got: String, failed: Vec<String>) {
    if !failed.is_empty() {
        ui::notify(
            &format!("pricehist: {}", failed.join("; ")),
            ui::Level::Warning,
        );
    }
    let n = prices::count(&got);
    if n == 0 {
        if failed.is_empty() {
            ui::notify("No new prices", ui::Level::Info);
        }
        return;
    }
    let to = target(main);
    ui::notify(
        &format!(
            "{n} price{} fetched into {}",
            if n == 1 { "" } else { "s" },
            journal::name(&to)
        ),
        ui::Level::Info,
    );
    append(to, got, "Fetch Prices".into());
}
