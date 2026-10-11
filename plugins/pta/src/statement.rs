//! The statement beside the reconciliation (PT8c): a bank statement's
//! lines (a CSV file, its columns found by their headers as the import's
//! rules template finds them) paired with the account's postings by
//! amount and date, the date within a few days. No tool does this: the
//! plugin compares what the statement and the tool say, and adds
//! nothing up.

use std::collections::BTreeSet;

use crate::import;
use crate::reconcile::{Decimal, text_amount};
use crate::report::Posting;
use crate::write::Date;

/// A line of the statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub date: Date,
    pub description: String,
    /// In the account's direction: money in above zero.
    pub amount: Decimal,
    /// As the statement writes it.
    pub written: String,
}

/// The headers of money in and money out where a statement has two
/// columns rather than one amount.
const IN: &[&str] = &[
    "credit", "deposit", "deposits", "in", "alacak", "haben", "paid_in",
];
const OUT: &[&str] = &[
    "debit",
    "withdrawal",
    "withdrawals",
    "out",
    "borç",
    "borc",
    "soll",
    "paid_out",
];

/// The statement's decimal mark: `,` where most amounts end in a comma
/// and one or two digits (`412,50`), else `.`.
fn mark_of(samples: &[&str]) -> char {
    let comma = samples
        .iter()
        .filter(|s| {
            let s = s.trim();
            s.rfind([',', '.'])
                .is_some_and(|i| s.as_bytes()[i] == b',' && (1..=2).contains(&(s.len() - i - 1)))
        })
        .count();
    if comma * 2 > samples.len() { ',' } else { '.' }
}

/// A date written as `format` (`%d.%m.%Y`, `%Y-%m-%d`, `%m/%d/%Y`).
fn date_in(s: &str, format: &str) -> Option<Date> {
    let parts: Vec<u32> = s
        .trim()
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let order: Vec<char> = format.split('%').filter_map(|p| p.chars().next()).collect();
    if parts.len() != 3 || order.len() != 3 {
        return None;
    }
    let (mut y, mut m, mut d) = (0, 0, 0);
    for (p, o) in parts.iter().zip(&order) {
        match o {
            'Y' => y = *p as i32,
            'y' => y = 2000 + *p as i32,
            'm' => m = *p,
            'd' => d = *p,
            _ => return None,
        }
    }
    let date = Date { y, m, d };
    crate::write::date_at(&date.written('-')).map(|(d, _, _)| d)
}

/// The statement's lines from its CSV text: the date, description and
/// amount columns (or money in and out) by their headers, the dates in
/// the format of the first, the amounts with the decimal mark most of
/// them have.
pub fn lines(csv: &str) -> Result<Vec<Line>, String> {
    let mut rows = crate::other::csv_rows_with(csv, separator(csv));
    rows.retain(|r| r.iter().any(|f| !f.trim().is_empty()));
    if rows.len() < 2 {
        return Err("The statement has no lines under its header".into());
    }
    let header: Vec<String> = rows.remove(0).iter().map(|h| import::field(h)).collect();
    let at = |names: &[&str]| header.iter().position(|h| names.contains(&h.as_str()));
    let date = at(&["date"]).ok_or("The statement has no date column")?;
    let description = at(&["description"]);
    let amount = at(&["amount"]);
    let (money_in, money_out) = (at(IN), at(OUT));
    if amount.is_none() && money_in.is_none() && money_out.is_none() {
        return Err("The statement has no amount column (or money in and out)".into());
    }
    let format = import::date_format(rows[0].get(date).map_or("", String::as_str));
    let samples: Vec<&str> = rows
        .iter()
        .flat_map(|r| {
            [amount, money_in, money_out]
                .into_iter()
                .flatten()
                .filter_map(|i| r.get(i))
        })
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .collect();
    let mark = mark_of(&samples);
    let number = |s: &str| {
        let t: String = s
            .chars()
            .filter(|c| !c.is_alphabetic() && !"$€£₺¥\"".contains(*c))
            .collect();
        Decimal::parse(t.trim(), mark)
    };
    let mut out = Vec::new();
    for r in rows {
        let get = |i: Option<usize>| i.and_then(|i| r.get(i)).map_or("", |s| s.trim());
        let Some(d) = date_in(get(Some(date)), &format) else {
            continue;
        };
        let (value, written) = match amount {
            Some(_) => (number(get(amount)), get(amount).to_string()),
            None => {
                let (i, o) = (get(money_in), get(money_out));
                let zero = Decimal {
                    mantissa: 0,
                    scale: 0,
                };
                let i = if i.is_empty() { Some(zero) } else { number(i) };
                let o = if o.is_empty() { Some(zero) } else { number(o) };
                let v = i.zip(o).map(|(i, o)| i.minus(o));
                let w = if get(money_in).is_empty() {
                    format!("-{}", get(money_out))
                } else {
                    get(money_in).to_string()
                };
                (v, w)
            }
        };
        let Some(value) = value else {
            continue;
        };
        out.push(Line {
            date: d,
            description: get(description).to_string(),
            amount: value,
            written,
        });
    }
    Ok(out)
}

/// The statement's field separator: the one its header has most of.
fn separator(csv: &str) -> char {
    let header = csv.lines().next().unwrap_or("");
    [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|s| header.matches(*s).count())
        .unwrap_or(',')
}

/// What one posting is in every list of the tool's: its place where the
/// tool gives it, else its transaction's number with its date and
/// amount.
pub fn key(p: &Posting) -> String {
    match (&p.place, p.transaction) {
        (Some((f, l)), _) => format!("{f}:{l}:{}", p.amount),
        (None, Some(t)) => format!("#{t}:{}:{}", p.date, p.amount),
        (None, None) => format!("{}:{}:{}", p.date, p.description, p.amount),
    }
}

/// The statement against the account's postings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paired {
    /// The postings found on the statement, by [`key`].
    pub found: BTreeSet<String>,
    /// The statement's lines without a posting.
    pub alone: Vec<Line>,
    /// The statement writes the account's amounts the other way round
    /// (a card's charges above zero).
    pub flipped: bool,
}

/// Pairs `lines` with `postings` (the account's, in the statement's
/// days): the same amount, the dates within `days`, the nearest first;
/// the statement's signs as written, or turned, whichever pairs more.
pub fn pair(lines: &[Line], postings: &[Posting], days: i64) -> Paired {
    let amounts: Vec<Option<(Date, Decimal)>> = postings
        .iter()
        .map(|p| {
            let d = crate::write::date_at(&p.date)?.0;
            let a = text_amount(&p.amount)?.quantity;
            Some((d, a))
        })
        .collect();
    let attempt = |flip: bool| {
        let mut candidates: Vec<(i64, usize, usize)> = Vec::new();
        for (li, l) in lines.iter().enumerate() {
            let want = if flip {
                Decimal {
                    mantissa: -l.amount.mantissa,
                    scale: l.amount.scale,
                }
            } else {
                l.amount
            };
            for (pi, a) in amounts.iter().enumerate() {
                let Some((d, q)) = a else { continue };
                let gap = (d.days() - l.date.days()).abs();
                if gap <= days && q.minus(want).is_zero() {
                    candidates.push((gap, li, pi));
                }
            }
        }
        candidates.sort_unstable();
        let (mut line_used, mut posting_used) =
            (vec![false; lines.len()], vec![false; postings.len()]);
        for (_, li, pi) in candidates {
            if !line_used[li] && !posting_used[pi] {
                line_used[li] = true;
                posting_used[pi] = true;
            }
        }
        (line_used, posting_used)
    };
    let (straight, turned) = (attempt(false), attempt(true));
    let count = |u: &[bool]| u.iter().filter(|x| **x).count();
    let flipped = count(&turned.0) > count(&straight.0);
    let (line_used, posting_used) = if flipped { turned } else { straight };
    Paired {
        found: postings
            .iter()
            .zip(&posting_used)
            .filter(|(_, u)| **u)
            .map(|(p, _)| key(p))
            .collect(),
        alone: lines
            .iter()
            .zip(&line_used)
            .filter(|(_, u)| !**u)
            .map(|(l, _)| l.clone())
            .collect(),
        flipped,
    }
}

/// The days the statement covers, widened by `days` either side: the
/// postings to pair with are the account's in them.
pub fn span(lines: &[Line], days: i64) -> Option<(Date, Date)> {
    let first = lines.iter().map(|l| l.date).min()?;
    let last = lines.iter().map(|l| l.date).max()?;
    Some((first.plus(-days), last.plus(days + 1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posting(date: &str, amount: &str, n: usize) -> Posting {
        Posting {
            date: date.into(),
            status: ' ',
            description: format!("p{n}"),
            amount: amount.into(),
            total: String::new(),
            place: None,
            transaction: Some(n),
        }
    }

    #[test]
    fn a_statement_read_and_paired() {
        let csv = "Tarih;Açıklama;Tutar;Bakiye\n03.02.2026;MIGROS;-412,50;63.284,75\n05.02.2026;MAAS;45.000,00;108.284,75\n06.02.2026;CAFE;-12,00;108.272,75\n";
        let l = lines(csv).expect("lines");
        assert_eq!(l.len(), 3);
        assert_eq!(
            l[1].date,
            Date {
                y: 2026,
                m: 2,
                d: 5
            }
        );
        assert_eq!(
            l[1].amount,
            Decimal {
                mantissa: 4_500_000,
                scale: 2
            }
        );
        let postings = [
            posting("2026-02-04", "-412,50 TRY", 1),
            posting("2026-02-05", "45.000,00 TRY", 2),
            posting("2026-02-20", "-99,00 TRY", 3),
        ];
        let p = pair(&l, &postings, 3);
        assert!(!p.flipped);
        assert_eq!(p.found.len(), 2);
        assert!(p.found.contains(&key(&postings[0])) && !p.found.contains(&key(&postings[2])));
        assert_eq!(p.alone.len(), 1);
        assert_eq!(p.alone[0].description, "CAFE");
        // Too far apart: not paired.
        assert_eq!(
            pair(&l, &[posting("2026-02-10", "-412,50 TRY", 1)], 3)
                .found
                .len(),
            0
        );
        assert_eq!(
            span(&l, 3),
            Some((
                Date {
                    y: 2026,
                    m: 1,
                    d: 31
                },
                Date {
                    y: 2026,
                    m: 2,
                    d: 10
                }
            ))
        );
    }

    #[test]
    fn money_in_and_out_and_a_card() {
        let csv =
            "Date,Description,Debit,Credit\n2026-02-03,Shop,25.00,\n2026-02-04,Refund,,5.00\n";
        let l = lines(csv).expect("lines");
        assert_eq!(
            l[0].amount,
            Decimal {
                mantissa: -2500,
                scale: 2
            }
        );
        assert_eq!(
            l[1].amount,
            Decimal {
                mantissa: 500,
                scale: 2
            }
        );
        // A card statement: charges above zero, the liability's postings
        // below.
        let card = "Date,Description,Amount\n2026-02-03,Shop,25.00\n2026-02-04,Cafe,4.50\n";
        let l = lines(card).expect("lines");
        let p = pair(
            &l,
            &[
                posting("2026-02-03", "$-25.00", 1),
                posting("2026-02-05", "$-4.50", 2),
            ],
            3,
        );
        assert!(p.flipped && p.found.len() == 2 && p.alone.is_empty());
        assert!(lines("Nothing\n1\n").is_err());
    }
}
