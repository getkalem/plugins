//! Prices (PT10): fetched by the user's own price programs, written as
//! the journal writes prices, appended where the setting `prices` says
//! (the main journal by default). Beancount's `bean-price --update` reads
//! the journal's `price:` metadata and fetches what is missing since each
//! commodity's last price; for hledger and Ledger, `pricehist` fetches
//! the pairs the setting `price_sources` names (`ecb EUR/USD`, or `ecb
//! EUR/USD $` where the journal writes the quote as `$`), from the day
//! after the journal's last price of the pair, or the last 30 days.

use std::collections::BTreeMap;

use crate::write::{Date, date_at};

/// A price to fetch: `pricehist`'s source and pair (`ecb`, `EUR/USD`),
/// and the quote's symbol in the journal when it is another (`$`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub source: String,
    pub base: String,
    pub quote: String,
    pub quote_as: Option<String>,
}

impl Source {
    /// The quote as the journal writes it.
    pub fn quote_written(&self) -> &str {
        self.quote_as.as_deref().unwrap_or(&self.quote)
    }
}

/// The setting's `SOURCE BASE/QUOTE [SYMBOL]` entries; malformed ones
/// left out.
pub fn sources(entries: &[String]) -> Vec<Source> {
    entries
        .iter()
        .filter_map(|e| {
            let mut words = e.split_whitespace();
            let (source, pair) = (words.next()?, words.next()?);
            let (base, quote) = pair.split_once('/')?;
            (!base.is_empty() && !quote.is_empty()).then(|| Source {
                source: source.to_string(),
                base: base.to_string(),
                quote: quote.to_string(),
                quote_as: words.next().map(str::to_string),
            })
        })
        .collect()
}

/// How far back a pair without a price in the journal is fetched.
pub const FIRST_DAYS: i64 = 30;

/// `pricehist`'s run for `s`: after `last`, the journal's last price of
/// the pair, or from `FIRST_DAYS` before `today`.
pub fn pricehist_args(s: &Source, last: Option<Date>, today: Date) -> Vec<String> {
    let mut a = vec![
        "fetch".to_string(),
        s.source.clone(),
        format!("{}/{}", s.base, s.quote),
        "-o".to_string(),
        "csv".to_string(),
    ];
    match last {
        Some(d) => a.extend(["-sx".into(), d.written('-')]),
        None => a.extend(["-s".into(), today.plus(-FIRST_DAYS).written('-')]),
    }
    a
}

/// The tool's list of the journal's prices: `hledger prices`, `ledger
/// prices`.
pub fn list_args(main: &str) -> Vec<String> {
    vec!["-f".into(), main.into(), "prices".into()]
}

/// Beancount's `bean-price --update`: the prices since each commodity's
/// last, for the commodities whose `price:` metadata names a source.
pub fn bean_price_args(main: &str) -> Vec<String> {
    vec!["--update".into(), main.into()]
}

/// The commodity of an amount as written (`$1.10` → `$`, `36,50 TRY` →
/// `TRY`, `"FUND A"` kept whole).
fn commodity_of(amount: &str) -> String {
    if let Some(q) = amount.find('"') {
        return amount[q + 1..].split('"').next().unwrap_or("").to_string();
    }
    amount
        .chars()
        .filter(|c| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+' | ' ' | '\'' | '_')))
        .collect::<String>()
        .trim()
        .to_string()
}

/// The last date of each pair's price (base, quote) in the tool's list:
/// hledger's `P 2026-01-10 EUR 36,50 TRY`, Ledger's `2026/01/01 EUR
/// $1.10`.
pub fn last_dates(out: &str) -> BTreeMap<(String, String), Date> {
    let mut last: BTreeMap<(String, String), Date> = BTreeMap::new();
    for line in out.lines() {
        let l = line.trim();
        let l = l.strip_prefix("P ").unwrap_or(l);
        let Some((date, len, _)) = date_at(l) else {
            continue;
        };
        let rest = l[len..].trim_start();
        let (base, amount) = if let Some(q) = rest.strip_prefix('"') {
            match q.split_once('"') {
                Some((b, a)) => (b.to_string(), a),
                None => continue,
            }
        } else {
            match rest.split_once(char::is_whitespace) {
                Some((b, a)) => (b.to_string(), a),
                None => continue,
            }
        };
        let quote = commodity_of(amount.trim());
        if base.is_empty() || quote.is_empty() {
            continue;
        }
        let e = last.entry((base, quote)).or_insert(date);
        if date > *e {
            *e = date;
        }
    }
    last
}

/// An amount with its commodity as the journal writes it: a symbol of
/// one character (`$`, `€`) before it, a word after it.
fn with_commodity(amount: &str, commodity: &str) -> String {
    if commodity.chars().count() == 1 && !commodity.chars().all(char::is_alphanumeric) {
        format!("{commodity}{amount}")
    } else if commodity.chars().all(|c| c.is_alphabetic() || c == '_') {
        format!("{amount} {commodity}")
    } else {
        format!("{amount} \"{commodity}\"")
    }
}

/// `pricehist`'s CSV (`date,base,quote,amount,source,type`) for `s` as
/// price directives the journal writes: `P DATE BASE AMOUNT`, its dates
/// with `sep`, its decimal mark `decimal`.
pub fn price_lines(csv: &str, s: &Source, sep: char, decimal: char) -> String {
    let mut out = String::new();
    for line in csv.lines().skip(1) {
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        let (Some(date), Some(amount)) = (f.first(), f.get(3)) else {
            continue;
        };
        let Some((d, _, _)) = date_at(date) else {
            continue;
        };
        let amount = if decimal == ',' {
            amount.replace('.', ",")
        } else {
            (*amount).to_string()
        };
        out.push_str(&format!(
            "P {} {} {}\n",
            d.written(sep),
            s.base,
            with_commodity(&amount, s.quote_written())
        ));
    }
    out
}

/// How many prices `text` writes: lines with `P ` or Beancount's `price`.
pub fn count(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            l.starts_with("P ")
                || date_at(l).is_some_and(|(_, n, _)| l[n..].trim_start().starts_with("price "))
        })
        .count()
}

/// The journal's decimal mark, as its text declares it (hledger's
/// `decimal-mark ,`); `.` otherwise.
pub fn decimal_mark(text: &str) -> char {
    if text
        .lines()
        .any(|l| l.trim_start().starts_with("decimal-mark ,"))
    {
        ','
    } else {
        '.'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sources() {
        let s = sources(&[
            "ecb EUR/USD".into(),
            "yahoo  AAPL/USD".into(),
            "ecb EUR/USD $".into(),
            "nothing".into(),
            "x /Y".into(),
        ]);
        assert_eq!(s.len(), 3);
        assert_eq!(
            (
                s[1].source.as_str(),
                s[1].base.as_str(),
                s[1].quote.as_str()
            ),
            ("yahoo", "AAPL", "USD")
        );
        assert_eq!(s[2].quote_written(), "$");
        let today = Date {
            y: 2026,
            m: 10,
            d: 11,
        };
        assert_eq!(
            pricehist_args(
                &s[0],
                Some(Date {
                    y: 2026,
                    m: 1,
                    d: 11
                }),
                today
            ),
            ["fetch", "ecb", "EUR/USD", "-o", "csv", "-sx", "2026-01-11"]
        );
        assert_eq!(
            pricehist_args(&s[0], None, today)[5..],
            ["-s", "2026-09-11"]
        );
    }

    #[test]
    fn the_last_dates() {
        let h = "P 2026-01-10 EUR 36,50 TRY\nP 2026-01-12 USD 34,10 TRY\nP 2026-01-15 \"FUND A\" 350,00 TRY\nP 2026-02-01 EUR 37,00 TRY\nP 2026-03-01 EUR 1.10 USD\n";
        let l = last_dates(h);
        let key = |b: &str, q: &str| (b.to_string(), q.to_string());
        assert_eq!(
            l[&key("EUR", "TRY")],
            Date {
                y: 2026,
                m: 2,
                d: 1
            }
        );
        assert_eq!(
            l[&key("EUR", "USD")],
            Date {
                y: 2026,
                m: 3,
                d: 1
            }
        );
        assert_eq!(
            l[&key("FUND A", "TRY")],
            Date {
                y: 2026,
                m: 1,
                d: 15
            }
        );
        let l = last_dates("2026/01/01 EUR             $1.10\n2026/02/01 EUR             $1.12\n");
        assert_eq!(
            l[&key("EUR", "$")],
            Date {
                y: 2026,
                m: 2,
                d: 1
            }
        );
    }

    #[test]
    fn the_price_lines() {
        let csv = "date,base,quote,amount,source,type\n2026-10-08,EUR,USD,1.0921,ecb,reference\n2026-10-09,EUR,USD,1.0934,ecb,reference\n";
        let s = &sources(&["ecb EUR/USD".into(), "ecb EUR/USD $".into()]);
        assert_eq!(
            price_lines(csv, &s[0], '-', '.'),
            "P 2026-10-08 EUR 1.0921 USD\nP 2026-10-09 EUR 1.0934 USD\n"
        );
        assert_eq!(
            price_lines(csv, &s[1], '/', ','),
            "P 2026/10/08 EUR $1,0921\nP 2026/10/09 EUR $1,0934\n"
        );
        assert_eq!(count(&price_lines(csv, &s[0], '-', '.')), 2);
        assert_eq!(count("2026-10-09 price EUR 1.1206 USD\n; x\n"), 1);
        assert_eq!(decimal_mark("; x\ndecimal-mark ,\n"), ',');
        assert_eq!(decimal_mark("2026-01-01 x\n"), '.');
    }
}
