//! The journal's outline (K5, Kalem's plugin API 0.2.18), for the
//! outline sidebar: Org's headings where the journal writes them, as all
//! three tools take a line starting with `*` for a comment (`* Household`,
//! `** Food`: a level a star); else the years and months its transactions
//! begin, so that a long journal is gone through by date.

use std::collections::BTreeSet;

use crate::Dialect;
use crate::write;

/// A heading: its level from 1, its title, the byte its line starts at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u32,
    pub title: String,
    pub start: usize,
}

/// The headings of `text`.
pub fn headings(text: &str, dialect: Dialect) -> Vec<Heading> {
    let stars = org_headings(text);
    if stars.is_empty() {
        by_date(text, dialect)
    } else {
        stars
    }
}

/// The lines starting with stars and a blank: Org's headings.
fn org_headings(text: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let level = line.bytes().take_while(|&b| b == b'*').count();
        let rest = &line[level..];
        if level > 0 && rest.starts_with([' ', '\t']) && !rest.trim().is_empty() {
            out.push(Heading {
                level: level as u32,
                title: rest.trim().to_string(),
                start: at,
            });
        }
        at += line.len();
    }
    out
}

/// The months the transactions begin, in the text's order (a month where
/// the transaction before is of another), under their years when the
/// journal holds several; each at its first transaction's comments.
fn by_date(text: &str, dialect: Dialect) -> Vec<Heading> {
    let txs = write::transactions(text, dialect);
    let several = txs.iter().map(|t| t.date.y).collect::<BTreeSet<_>>().len() > 1;
    let mut out = Vec::new();
    let (mut year, mut month) = (None, None);
    for t in &txs {
        let (y, m) = (t.date.y, t.date.m);
        let start = t.with_comments.start;
        if several && year != Some(y) {
            out.push(Heading {
                level: 1,
                title: y.to_string(),
                start,
            });
            year = Some(y);
        }
        if month != Some((y, m)) {
            out.push(Heading {
                level: if several { 2 } else { 1 },
                title: format!("{y}-{m:02}"),
                start,
            });
            month = Some((y, m));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(h: &[Heading]) -> Vec<(u32, &str)> {
        h.iter().map(|h| (h.level, h.title.as_str())).collect()
    }

    #[test]
    fn org_headings_first() {
        let t = "* Household\n** Food\n2026-01-05 Grocer\n    expenses:food  1\n    assets:cash\n*not one\n* Rent\n";
        let h = headings(t, Dialect::Hledger);
        assert_eq!(titles(&h), [(1, "Household"), (2, "Food"), (1, "Rent")]);
        assert_eq!(h[1].start, t.find("** Food").unwrap());
        // A transaction's own `*` is after its date.
        let b = "2026-01-05 * \"Grocer\" \"\"\n  Expenses:Food  1 USD\n  Assets:Cash\n";
        assert_eq!(titles(&headings(b, Dialect::Beancount)), [(1, "2026-01")]);
    }

    #[test]
    fn months_and_years() {
        let one = "2026-01-05 A\n    a  1\n    b\n\n2026-01-20 B\n    a  1\n    b\n\n; rent\n2026-02-01 C\n    a  1\n    b\n";
        let h = headings(one, Dialect::Hledger);
        assert_eq!(titles(&h), [(1, "2026-01"), (1, "2026-02")]);
        // At the comment above the month's first transaction.
        assert_eq!(h[1].start, one.find("; rent").unwrap());
        let two = "2025/12/30 A\n    a  1\n    b\n\n2026/01/02 B\n    a  1\n    b\n";
        assert_eq!(
            titles(&headings(two, Dialect::Ledger)),
            [(1, "2025"), (2, "2025-12"), (1, "2026"), (2, "2026-01")]
        );
        assert!(headings("", Dialect::Hledger).is_empty());
    }
}
