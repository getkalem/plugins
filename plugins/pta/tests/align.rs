//! Align Amounts (PT4) on every corpus journal, whole: only the blanks
//! between accounts and amounts change, a second alignment changes
//! nothing, and the journal's own tool reads the aligned file as it read
//! the original (`hledger print`, `ledger register`, `bean-query`), where
//! the tool is installed.

// A comparison skipped (the tool not installed) is said on standard error.
#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_plugin_pta::Dialect;
use kalem_plugin_pta::align::{apply, edits};

fn corpus(d: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus").join(d)
}

/// The corpus's journals: folder, file, dialect, and the tool's reading
/// of it (its output is compared, not its success: an error file's
/// reading is its error).
const JOURNALS: &[(&str, &str, Dialect, &[&str])] = &[
    (
        "hledger",
        "2026.journal",
        Dialect::Hledger,
        &["hledger", "-f", "2026.journal", "print", "--auto"],
    ),
    (
        "hledger",
        "2025.journal",
        Dialect::Hledger,
        &["hledger", "-f", "2025.journal", "print"],
    ),
    (
        "hledger",
        "accounts.journal",
        Dialect::Hledger,
        &["hledger", "-f", "accounts.journal", "accounts"],
    ),
    (
        "hledger",
        "errors.journal",
        Dialect::Hledger,
        &["hledger", "-f", "errors.journal", "print", "-I"],
    ),
    (
        "ledger",
        "business.ledger",
        Dialect::Ledger,
        &["ledger", "-f", "business.ledger", "register"],
    ),
    (
        "ledger",
        "errors.ledger",
        Dialect::Ledger,
        &["ledger", "-f", "errors.ledger", "register"],
    ),
    (
        "beancount",
        "household.beancount",
        Dialect::Beancount,
        &[
            "bean-query",
            "-f",
            "csv",
            "household.beancount",
            "SELECT date, flag, payee, narration, account, position, price",
        ],
    ),
    (
        "beancount",
        "accounts.beancount",
        Dialect::Beancount,
        &[
            "bean-query",
            "-f",
            "csv",
            "accounts.beancount",
            "SELECT account, open_date",
        ],
    ),
];

/// The text without its blanks: what alignment must not change.
fn unblanked(t: &str) -> String {
    t.chars().filter(|c| *c != ' ' && *c != '\t').collect()
}

/// What `args` prints in `dir`, both outputs; `None` where the program
/// is not installed.
fn reading(args: &[&str], dir: &Path) -> Option<String> {
    let o = match Command::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => panic!("{}: {e}", args[0]),
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    Some(text)
}

#[test]
fn every_journal_aligned_reads_as_before() {
    for column in [52, 0] {
        for (folder, file, dialect, read) in JOURNALS {
            let dir = corpus(folder);
            let text = std::fs::read_to_string(dir.join(file)).expect("corpus");
            let e = edits(&text, 0..text.len(), *dialect, column);
            let aligned = apply(&text, &e);
            assert_eq!(unblanked(&aligned), unblanked(&text), "{file} at {column}");
            assert_eq!(
                aligned.lines().count(),
                text.lines().count(),
                "{file}: lines"
            );
            assert!(
                edits(&aligned, 0..aligned.len(), *dialect, column).is_empty(),
                "{file} at {column}: aligned twice"
            );
            // Only the gaps of postings with an amount: every edit's text
            // is blanks, and so is what it replaced.
            for (r, s) in &e {
                assert!(text[r.clone()].chars().all(|c| c == ' ' || c == '\t'));
                assert!(s.chars().all(|c| c == ' ') && s.len() >= 2, "{file}");
            }
            let Some(before) = reading(read, &dir) else {
                eprintln!("{} is not installed: {file} aligned is not read", read[0]);
                continue;
            };
            let copy = std::env::temp_dir().join(format!(
                "pta-align-{folder}-{column}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&copy);
            std::fs::create_dir_all(&copy).expect("copy");
            for f in std::fs::read_dir(&dir).expect("corpus") {
                let p = f.expect("entry").path();
                std::fs::copy(&p, copy.join(p.file_name().expect("name"))).expect("copy");
            }
            std::fs::write(copy.join(file), &aligned).expect("write");
            let after = reading(read, &copy).expect("the tool ran before");
            // An error's message names the file's folder (macOS's
            // /private one too) and quotes its lines, blanks and all:
            // compared without the folders, blanks run together.
            let folders: Vec<String> = [&copy, &dir]
                .iter()
                .flat_map(|d| {
                    let real = std::fs::canonicalize(d).unwrap_or_else(|_| (*d).clone());
                    [
                        real.to_string_lossy().into_owned(),
                        d.to_string_lossy().into_owned(),
                    ]
                })
                .collect();
            let _ = std::fs::remove_dir_all(&copy);
            let strip = |s: &str| {
                let mut s = s.to_string();
                for f in &folders {
                    s = s.replace(f.as_str(), "");
                }
                s.split_whitespace().collect::<Vec<_>>().join(" ")
            };
            assert_eq!(strip(&after), strip(&before), "{file} at {column}");
        }
    }
}

#[test]
fn the_corpus_has_postings_to_align() {
    // Not vacuous: the hledger journal's postings move at column 52.
    let text = std::fs::read_to_string(corpus("hledger").join("2026.journal")).expect("corpus");
    assert!(edits(&text, 0..text.len(), Dialect::Hledger, 52).len() > 10);
}
