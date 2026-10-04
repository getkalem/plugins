//! What a spreadsheet's fill handle makes of the cells it starts from:
//! a series that goes on (numbers in a straight line, a date a day at a
//! time, numbered text, month and day names), or the cells over again.

use crate::sheet::Value;

/// A source cell, as the fill reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Its value.
    pub value: Value,
    /// It holds a formula.
    pub formula: bool,
    /// Its number is a date.
    pub date: bool,
}

/// What goes into a filled cell.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// A number.
    Number(f64),
    /// Text.
    Text(String),
    /// The source cell at this index again (its formula moved).
    Copy(usize),
}

/// Lists a fill goes round: English and Turkish months and days, in
/// full and short.
const LISTS: [&[&str]; 8] = [
    &[
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    &[
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ],
    &[
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ],
    &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
    &[
        "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim",
        "Kasım", "Aralık",
    ],
    &[
        "Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara",
    ],
    &[
        "Pazartesi",
        "Salı",
        "Çarşamba",
        "Perşembe",
        "Cuma",
        "Cumartesi",
        "Pazar",
    ],
    &["Pzt", "Sal", "Çar", "Per", "Cum", "Cmt", "Paz"],
];

/// How the source goes on.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// The source over again.
    Copy(usize),
    /// `a + b × position`.
    Line(f64, f64),
    /// Text and a number: prefix, first number, step, digits.
    Numbered(String, i64, i64, usize),
    /// A list, the first item's place in it, the step, and the case.
    List(&'static [&'static str], i64, i64, Case),
}

/// How a list's items are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// As the list has them.
    Title,
    /// All capitals.
    Upper,
    /// All small letters.
    Lower,
}

fn case_of(s: &str) -> Case {
    if s.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()) && s.chars().count() > 1 {
        Case::Upper
    } else if s.chars().next().is_some_and(char::is_lowercase) {
        Case::Lower
    } else {
        Case::Title
    }
}

/// Text ending in digits: what comes before them, and the number.
fn numbered(s: &str) -> Option<(String, i64, usize)> {
    let digits = s.chars().rev().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 15 {
        return None;
    }
    let cut = s.len() - digits;
    Some((s[..cut].to_owned(), s[cut..].parse().ok()?, digits))
}

fn list_of(s: &str) -> Option<(&'static [&'static str], i64)> {
    let low = s.to_lowercase();
    LISTS.iter().find_map(|l| {
        l.iter()
            .position(|x| x.to_lowercase() == low)
            .map(|i| (*l, i as i64))
    })
}

/// What the source makes when filled; `series` off copies it.
pub fn analyze(src: &[Item], series: bool) -> Pattern {
    let n = src.len();
    if !series || n == 0 || src.iter().any(|i| i.formula) {
        return Pattern::Copy(n);
    }
    // Numbers: a straight line through them (least squares), a single
    // date a day at a time.
    let numbers: Option<Vec<f64>> = src
        .iter()
        .map(|i| match i.value {
            Value::Number(v) => Some(v),
            _ => None,
        })
        .collect();
    if let Some(v) = numbers {
        if n == 1 {
            return if src[0].date {
                Pattern::Line(v[0], 1.0)
            } else {
                Pattern::Copy(n)
            };
        }
        let nf = n as f64;
        let mx = (nf - 1.0) / 2.0;
        let my = v.iter().sum::<f64>() / nf;
        let sxy: f64 = v
            .iter()
            .enumerate()
            .map(|(i, y)| (i as f64 - mx) * (y - my))
            .sum();
        let sxx: f64 = (0..n).map(|i| (i as f64 - mx).powi(2)).sum();
        let b = sxy / sxx;
        return Pattern::Line(my - b * mx, b);
    }
    let texts: Option<Vec<&str>> = src
        .iter()
        .map(|i| match &i.value {
            Value::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    let Some(texts) = texts else {
        return Pattern::Copy(n);
    };
    // Month or day names, the same list throughout and evenly apart.
    let places: Option<Vec<(&'static [&'static str], i64)>> =
        texts.iter().map(|t| list_of(t)).collect();
    if let Some(p) = places
        && p.iter().all(|x| std::ptr::eq(x.0, p[0].0))
    {
        let len = p[0].0.len() as i64;
        let step = if n == 1 {
            1
        } else {
            (p[1].1 - p[0].1).rem_euclid(len)
        };
        if p.windows(2)
            .all(|w| (w[1].1 - w[0].1).rem_euclid(len) == step)
        {
            return Pattern::List(p[0].0, p[0].1, step, case_of(texts[0]));
        }
    }
    // Text and a number, the same text throughout and evenly apart.
    let parts: Option<Vec<(String, i64, usize)>> = texts.iter().map(|t| numbered(t)).collect();
    if let Some(p) = parts
        && p.iter().all(|x| x.0 == p[0].0)
    {
        let step = if n == 1 { 1 } else { p[1].1 - p[0].1 };
        if p.windows(2).all(|w| w[1].1 - w[0].1 == step) {
            return Pattern::Numbered(p[0].0.clone(), p[0].1, step, p[0].2);
        }
    }
    Pattern::Copy(n)
}

impl Pattern {
    /// The cell at `p` places from the source's first (negative before it).
    pub fn at(&self, p: i64) -> Out {
        match self {
            Pattern::Copy(n) => Out::Copy(p.rem_euclid((*n).max(1) as i64) as usize),
            Pattern::Line(a, b) => Out::Number(a + b * p as f64),
            Pattern::Numbered(prefix, start, step, digits) => {
                let v = start + step * p;
                // Zero-padded as the source was (`Q01`, `Q02`).
                let num = if v < 0 {
                    format!("-{:0width$}", -v, width = *digits)
                } else {
                    format!("{v:0width$}", width = *digits)
                };
                Out::Text(format!("{prefix}{num}"))
            }
            Pattern::List(items, start, step, case) => {
                let len = items.len() as i64;
                let s = items[(start + step * p).rem_euclid(len) as usize];
                Out::Text(match case {
                    Case::Title => s.to_owned(),
                    Case::Upper => s.to_uppercase(),
                    Case::Lower => s.to_lowercase(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(v: f64) -> Item {
        Item {
            value: Value::Number(v),
            formula: false,
            date: false,
        }
    }

    fn text(t: &str) -> Item {
        Item {
            value: Value::Text(t.into()),
            formula: false,
            date: false,
        }
    }

    #[test]
    fn series_go_on() {
        // One number copies; two go on in a line, either way.
        assert_eq!(analyze(&[num(5.0)], true).at(3), Out::Copy(0));
        let p = analyze(&[num(2.0), num(4.0)], true);
        assert_eq!((p.at(2), p.at(-1)), (Out::Number(6.0), Out::Number(0.0)));
        // A date a day at a time.
        let d = Item {
            date: true,
            ..num(46_000.0)
        };
        assert_eq!(analyze(&[d], true).at(2), Out::Number(46_002.0));
        // A trend through uneven numbers, as Excel's least squares.
        let p = analyze(&[num(1.0), num(2.0), num(4.0)], true);
        let Out::Number(v) = p.at(3) else { panic!() };
        assert!((v - 16.0 / 3.0).abs() < 1e-9, "{v}");
        // Numbered text, zero-padded.
        assert_eq!(
            analyze(&[text("Item 1")], true).at(2),
            Out::Text("Item 3".into())
        );
        let p = analyze(&[text("Q01"), text("Q03")], true);
        assert_eq!(p.at(2), Out::Text("Q05".into()));
        // Months and days round their lists, in the source's case.
        assert_eq!(
            analyze(&[text("Kasım")], true).at(2),
            Out::Text("Ocak".into())
        );
        assert_eq!(analyze(&[text("MON")], true).at(1), Out::Text("TUE".into()));
        assert_eq!(
            analyze(&[text("jan"), text("mar")], true).at(2),
            Out::Text("may".into())
        );
        assert_eq!(
            analyze(&[text("Pazartesi")], true).at(-1),
            Out::Text("Pazar".into())
        );
        // Anything else, and every copy, the cells over again.
        assert_eq!(analyze(&[text("a"), text("b")], true).at(3), Out::Copy(1));
        assert_eq!(analyze(&[num(1.0), num(2.0)], false).at(2), Out::Copy(0));
        let f = Item {
            formula: true,
            ..num(3.0)
        };
        assert_eq!(analyze(&[f, num(1.0)], true).at(-1), Out::Copy(1));
    }
}
