//! Prices (PT10) as the plugin reads and writes them, on the corpus: the
//! last price of each pair as each tool lists it, and prices written the
//! plugin's way read back by the tool, with the journal's dates and
//! decimal mark. `pricehist` and `bean-price` fetch over the network,
//! which the tests leave out. A tool not installed is skipped with a
//! note.

// A tool not installed is said on standard error.
#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_plugin_pta::prices;
use kalem_plugin_pta::write::Date;

fn corpus(d: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus").join(d)
}

fn run(program: &str, args: &[String], dir: &Path) -> Option<String> {
    let o = match Command::new(program).args(args).current_dir(dir).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{program} is not installed: its prices are not read");
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

const CSV: &str = "date,base,quote,amount,source,type\n2026-03-02,EUR,USD,1.0921,ecb,reference\n2026-03-03,EUR,USD,1.0934,ecb,reference\n";

#[test]
fn ledgers_prices() {
    let dir = corpus("ledger");
    let main = dir.join("business.ledger").to_string_lossy().into_owned();
    let Some(listed) = run("ledger", &prices::list_args(&main), &dir) else {
        return;
    };
    let last = prices::last_dates(&listed);
    assert_eq!(
        last[&("EUR".to_string(), "$".to_string())],
        Date {
            y: 2026,
            m: 2,
            d: 1
        }
    );
    // Written after the journal, as the setting `prices` names a file.
    let to = std::env::temp_dir().join(format!("pta-prices-ledger-{}.ledger", std::process::id()));
    let s = &prices::sources(&["ecb EUR/USD $".into()])[0];
    let text = std::fs::read_to_string(&main).expect("journal")
        + "\n"
        + &prices::price_lines(CSV, s, '/', '.');
    std::fs::write(&to, text).expect("copy");
    let listed = run("ledger", &prices::list_args(&to.to_string_lossy()), &dir).expect("ran");
    let _ = std::fs::remove_file(&to);
    let last = prices::last_dates(&listed);
    assert_eq!(
        last[&("EUR".to_string(), "$".to_string())],
        Date {
            y: 2026,
            m: 3,
            d: 3
        },
        "{listed}"
    );
}

#[test]
fn hledgers_prices() {
    let dir = corpus("hledger");
    let to =
        std::env::temp_dir().join(format!("pta-prices-hledger-{}.journal", std::process::id()));
    let s = &prices::sources(&["ecb EUR/TRY".into()])[0];
    let csv = CSV.replace("USD,1.09", "TRY,36.9");
    // A journal of comma decimals, as the corpus writes TRY.
    let text = "decimal-mark ,\n\n".to_string()
        + &prices::price_lines(&csv, s, '-', prices::decimal_mark("decimal-mark ,\n"));
    assert!(text.contains("P 2026-03-03 EUR 36,934 TRY"), "{text}");
    std::fs::write(&to, &text).expect("journal");
    let listed = run("hledger", &prices::list_args(&to.to_string_lossy()), &dir);
    let _ = std::fs::remove_file(&to);
    let Some(listed) = listed else {
        return;
    };
    assert!(listed.contains("36,934 TRY"), "{listed}");
    assert_eq!(
        prices::last_dates(&listed)[&("EUR".to_string(), "TRY".to_string())],
        Date {
            y: 2026,
            m: 3,
            d: 3
        }
    );
}
