//! Journals with Windows's line endings (CRLF), which Kalem hands a
//! plugin as they are: what the plugin reads in a corpus journal is the
//! same with CRLF as with LF, and what it writes there ends its lines as
//! the journal does.

use std::path::Path;

use kalem_plugin_pta::write::{self, Date};
use kalem_plugin_pta::{Dialect, align, blocks, complete, reconcile};

const JOURNALS: &[(&str, &str, Dialect)] = &[
    ("hledger", "2026.journal", Dialect::Hledger),
    ("hledger", "2025.journal", Dialect::Hledger),
    ("hledger", "accounts.journal", Dialect::Hledger),
    ("ledger", "business.ledger", Dialect::Ledger),
    ("beancount", "household.beancount", Dialect::Beancount),
    ("beancount", "accounts.beancount", Dialect::Beancount),
];

fn read(folder: &str, file: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(folder)
        .join(file);
    std::fs::read_to_string(p).expect("corpus")
}

fn crlf(s: &str) -> String {
    s.replace('\n', "\r\n")
}

/// The CRLF text's offset of the LF text's `at`: a byte more a line.
fn moved(lf: &str, at: usize) -> usize {
    at + lf[..at].matches('\n').count()
}

#[test]
fn transactions_alike() {
    for (folder, file, d) in JOURNALS {
        let lf = read(folder, file);
        let cr = crlf(&lf);
        let (a, b) = (write::transactions(&lf, *d), write::transactions(&cr, *d));
        assert_eq!(a.len(), b.len(), "{file}");
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.date, y.date, "{file}");
            assert_eq!(moved(&lf, x.range.start), y.range.start, "{file}");
            assert_eq!(moved(&lf, x.range.end), y.range.end, "{file}");
            assert_eq!(
                x.status
                    .clone()
                    .map(|r| moved(&lf, r.start)..moved(&lf, r.end)),
                y.status,
                "{file}"
            );
            assert_eq!(moved(&lf, x.status_at), y.status_at, "{file}");
        }
    }
}

#[test]
fn accounts_alike() {
    for (folder, file, d) in JOURNALS {
        let lf = read(folder, file);
        let cr = crlf(&lf);
        let mut at = 0;
        for line in lf.split_inclusive('\n') {
            let end = at + line.len() - 1;
            assert_eq!(
                write::account_at(&lf, at, *d),
                write::account_at(&cr, moved(&lf, at), *d),
                "{file}: {line:?}"
            );
            assert_eq!(
                complete::slot(&lf, end, *d).map(|(s, b)| (s, moved(&lf, b))),
                complete::slot(&cr, moved(&lf, end), *d),
                "{file}: {line:?}"
            );
            at += line.len();
        }
        assert_eq!(
            complete::from_text(&lf, *d),
            complete::from_text(&cr, *d),
            "{file}"
        );
        assert_eq!(
            write::payees_in(&lf, *d),
            write::payees_in(&cr, *d),
            "{file}"
        );
        assert_eq!(
            write::separator(&lf, *d),
            write::separator(&cr, *d),
            "{file}"
        );
    }
}

#[test]
fn aligned_alike() {
    for (folder, file, d) in JOURNALS {
        let lf = read(folder, file);
        let cr = crlf(&lf);
        for column in [52, 0] {
            assert_eq!(
                align::format(&cr, *d, column),
                crlf(&align::format(&lf, *d, column)),
                "{file} at {column}"
            );
        }
    }
}

#[test]
fn reconciled_alike() {
    let lf = read("ledger", "business.ledger");
    let cr = crlf(&lf);
    let d = Date {
        y: 2026,
        m: 12,
        d: 31,
    };
    let none = reconcile::Accounts::new();
    assert_eq!(
        reconcile::unmarked_by(&lf, Dialect::Ledger, "Expenses", d, &none),
        reconcile::unmarked_by(&cr, Dialect::Ledger, "Expenses", d, &none)
    );
    let target = reconcile::text_amount("$100.00").unwrap();
    // As the component writes them: the lines ended as the journal's.
    let finished = |text: &str| {
        let edits = reconcile::finish(
            text,
            Dialect::Ledger,
            "Expenses:Software",
            std::slice::from_ref(&target),
            d,
            52,
            &none,
        );
        align::apply(text, &write::edits_ended_as(text, edits))
    };
    assert_eq!(finished(&cr), crlf(&finished(&lf)));
}

#[test]
fn written_alike() {
    for (folder, file, d) in JOURNALS {
        let lf = read(folder, file);
        let cr = crlf(&lf);
        assert_eq!(write::line_ending(&lf), "\n");
        assert_eq!(write::line_ending(&cr), "\r\n");
        let date = Date {
            y: 2026,
            m: 6,
            d: 1,
        };
        let at = write::place(&lf, *d, date, false);
        assert_eq!(moved(&lf, at), write::place(&cr, *d, date, false), "{file}");
        let (t, _) = write::new_transaction(*d, date, write::separator(&lf, *d), "Payee", None);
        let (a, _) = write::insertion(&lf, at, &t);
        let (b, _) = write::insertion(&cr, moved(&lf, at), &t);
        assert_eq!(write::ended_as(&cr, &b), crlf(&a), "{file}");
        // Sorted: the transactions' own text moved, endings and all.
        let sorted = |text: &str| {
            write::sort(text, *d, 0..0).map(|o| o.map(|(r, s)| align::apply(text, &[(r, s)])))
        };
        // A refusal (`apply account` between them) is one too.
        assert_eq!(
            sorted(&cr),
            sorted(&lf).map(|o| o.map(|s| crlf(&s))),
            "{file}"
        );
    }
}

#[test]
fn block_results_alike() {
    let lf = "* Money\n#+begin_src hledger\n2026-01-01 Shop\n    expenses:food  10 TRY\n    assets:cash\n#+end_src\nMore text.\n";
    let cr = crlf(lf);
    let output = "  10 TRY  expenses:food\n";
    let written = |text: &str| {
        let b = blocks::block_at(text, text.find("Shop").unwrap()).expect("a block");
        let (r, s) = blocks::write(text, &b, output);
        align::apply(text, &write::edits_ended_as(text, vec![(r, s)]))
    };
    assert_eq!(written(&cr), crlf(&written(lf)));
}
