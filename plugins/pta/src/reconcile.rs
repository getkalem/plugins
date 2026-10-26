//! Reconciliation (PT8): an account against a statement's closing
//! balance. The tool totals the account's cleared and pending postings
//! (hledger's `status:` queries, Ledger's `--cleared` and `--pending`,
//! Beancount's flags) and lists those not cleared; the plugin takes the
//! tool's total from the target the user typed, the difference shown,
//! and, once it is zero, clears the pending postings and writes the
//! target as a balance assertion, as the dialect writes one.
//!
//! Amounts are exact decimals: hledger's from its JSON (a mantissa, its
//! places and the commodity's style), Ledger's and Beancount's read from
//! their text with the decimal mark they write.

use std::collections::BTreeMap;
use std::ops::Range;

use serde_json::Value;

use crate::Dialect;
use crate::report::{Document, Posting, Style};
use crate::write::{self, Date};

/// An exact decimal: `mantissa / 10^scale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Decimal {
    pub mantissa: i128,
    pub scale: u32,
}

impl Decimal {
    fn rescale(self, scale: u32) -> Decimal {
        if scale <= self.scale {
            return self;
        }
        Decimal {
            mantissa: self.mantissa * 10i128.pow(scale - self.scale),
            scale,
        }
    }

    pub fn minus(self, o: Decimal) -> Decimal {
        let s = self.scale.max(o.scale);
        Decimal {
            mantissa: self.rescale(s).mantissa - o.rescale(s).mantissa,
            scale: s,
        }
    }

    pub fn is_zero(self) -> bool {
        self.mantissa == 0
    }

    /// `text`'s number: digits, a sign, group marks, and `mark` as the
    /// decimal mark.
    pub fn parse(text: &str, mark: char) -> Option<Decimal> {
        let t = text.trim();
        let (neg, t) = match t.strip_prefix('-') {
            Some(r) => (true, r.trim_start()),
            None => (false, t.strip_prefix('+').unwrap_or(t)),
        };
        let mut mantissa: i128 = 0;
        let mut scale = 0u32;
        let mut after = false;
        let mut digits = 0;
        for c in t.chars() {
            if c.is_ascii_digit() {
                mantissa = mantissa
                    .checked_mul(10)?
                    .checked_add(i128::from(c as u8 - b'0'))?;
                digits += 1;
                if after {
                    scale += 1;
                }
            } else if c == mark && !after {
                after = true;
            } else if matches!(
                c,
                '.' | ',' | '\'' | '_' | ' ' | '\u{a0}' | '\u{2009}' | '\u{202f}'
            ) && !after
            {
                // A digit group's mark.
            } else {
                return None;
            }
        }
        (digits > 0).then_some(Decimal {
            mantissa: if neg { -mantissa } else { mantissa },
            scale,
        })
    }
}

/// How a commodity's amounts are written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmountStyle {
    /// The commodity's symbol, as written (quoted when it must be).
    pub symbol: String,
    /// On the left (`$1.00`), else the right (`1,00 TRY`).
    pub left: bool,
    /// A blank between the symbol and the number.
    pub spaced: bool,
    pub decimal: char,
    /// The digit groups' mark and sizes (the last repeated).
    pub groups: Option<(char, Vec<usize>)>,
    /// Decimal places.
    pub precision: u32,
}

/// An amount: a number of a commodity, written in its style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Amount {
    pub quantity: Decimal,
    pub style: AmountStyle,
}

impl Amount {
    /// Written as the journal writes it.
    pub fn written(&self) -> String {
        let s = &self.style;
        let q = self.quantity.rescale(s.precision);
        let neg = q.mantissa < 0;
        let digits = q.mantissa.unsigned_abs().to_string();
        let scale = q.scale as usize;
        let digits = if digits.len() <= scale {
            format!("{}{digits}", "0".repeat(scale + 1 - digits.len()))
        } else {
            digits
        };
        let (int, frac) = digits.split_at(digits.len() - scale);
        let mut int_grouped = String::new();
        match &s.groups {
            Some((mark, sizes)) if !sizes.is_empty() => {
                let mut parts: Vec<&str> = Vec::new();
                let mut rest = int;
                let mut i = 0;
                loop {
                    let size = *sizes.get(i).unwrap_or(sizes.last().expect("a size"));
                    if rest.len() <= size || size == 0 {
                        parts.push(rest);
                        break;
                    }
                    parts.push(&rest[rest.len() - size..]);
                    rest = &rest[..rest.len() - size];
                    i += 1;
                }
                parts.reverse();
                int_grouped = parts.join(&mark.to_string());
            }
            _ => int_grouped.push_str(int),
        }
        let mut number = int_grouped;
        if !frac.is_empty() {
            number.push(s.decimal);
            number.push_str(frac);
        }
        let sign = if neg { "-" } else { "" };
        let gap = if s.spaced { " " } else { "" };
        if s.symbol.is_empty() {
            format!("{sign}{number}")
        } else if s.left {
            // `$-400.00`: hledger's and Ledger's sign after the symbol.
            format!("{}{gap}{sign}{number}", s.symbol)
        } else {
            format!("{sign}{number}{gap}{}", s.symbol)
        }
    }
}

/// A commodity's symbol as hledger writes it: quoted when it holds
/// anything but letters and currency signs.
fn symbol(c: &str) -> String {
    let plain = !c.is_empty()
        && c.chars()
            .all(|ch| ch.is_alphabetic() || "$€£¥₺₹₽₩¢".contains(ch));
    if plain || c.is_empty() {
        c.to_string()
    } else {
        format!("\"{c}\"")
    }
}

/// An amount of hledger's JSON (`acommodity`, `aquantity`, `astyle`).
pub fn hledger_amount(v: &Value) -> Option<Amount> {
    let q = &v["aquantity"];
    let mantissa = q["decimalMantissa"].as_i64()?;
    let places = q["decimalPlaces"].as_u64()? as u32;
    let st = &v["astyle"];
    let decimal = st["asdecimalmark"]
        .as_str()
        .and_then(|s| s.chars().next())
        .unwrap_or('.');
    let groups = st["asdigitgroups"].as_array().and_then(|g| {
        let mark = g.first()?.as_str()?.chars().next()?;
        let sizes: Vec<usize> = g
            .get(1)?
            .as_array()?
            .iter()
            .filter_map(|n| n.as_u64().map(|n| n as usize))
            .collect();
        Some((mark, sizes))
    });
    let precision = st["asprecision"].as_u64().map_or(places, |p| p as u32);
    Some(Amount {
        quantity: Decimal {
            mantissa: i128::from(mantissa),
            scale: places,
        },
        style: AmountStyle {
            symbol: symbol(v["acommodity"].as_str().unwrap_or("")),
            left: st["ascommodityside"].as_str() == Some("L"),
            spaced: st["ascommodityspaced"].as_bool().unwrap_or(true),
            decimal,
            groups,
            precision: precision.max(places),
        },
    })
}

/// The total of hledger's `balance -O json` (its second part).
pub fn hledger_total(json: &str) -> Vec<Amount> {
    let v: Value = serde_json::from_str(json).unwrap_or_default();
    v[1].as_array()
        .map(|a| a.iter().filter_map(hledger_amount).collect())
        .unwrap_or_default()
}

/// hledger's `register -O json`: rows of a date, a second date, the
/// description, the posting, the running total.
pub fn hledger_register(json: &str) -> Vec<Posting> {
    let v: Value = serde_json::from_str(json).unwrap_or_default();
    let written = |a: &Value| {
        a.as_array()
            .map(|l| {
                l.iter()
                    .filter_map(hledger_amount)
                    .map(|a| a.written())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    v.as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let p = &r[3];
                    Posting {
                        date: r[0].as_str().unwrap_or("").to_string(),
                        status: match p["pstatus"].as_str() {
                            Some("Cleared") => '*',
                            Some("Pending") => '!',
                            _ => ' ',
                        },
                        description: r[2].as_str().unwrap_or("").to_string(),
                        amount: written(&p["pamount"]),
                        total: written(&r[4]),
                        place: None,
                        transaction: p["ptransaction_"]
                            .as_str()
                            .and_then(|t| t.parse().ok())
                            .or_else(|| p["ptransaction_"].as_u64().map(|n| n as usize)),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The first number of an amount as `s` writes it: a run of digits and
/// their marks outside a quoted commodity (`"VWRL2"`) and a commodity's
/// name (`VWRL2`), ending with a digit; a blank in it only before a group
/// of three digits (`1 000,00`). Its start and end.
fn number_run(s: &str) -> Option<(usize, usize)> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut k = 0;
    while k < chars.len() {
        let c = chars[k].1;
        if c == '"' {
            // A quoted commodity, to its closing quote.
            k += 1;
            while k < chars.len() && chars[k].1 != '"' {
                k += 1;
            }
            k += 1;
        } else if c.is_alphabetic() {
            // A commodity's name, its digits too.
            while k < chars.len() && (chars[k].1.is_alphanumeric() || chars[k].1 == '_') {
                k += 1;
            }
        } else if c.is_ascii_digit() {
            let start = chars[k].0;
            let mut end = start;
            while let Some(&(i, c)) = chars.get(k) {
                let group = |rest: &[u8]| {
                    rest.len() >= 3
                        && rest[..3].iter().all(u8::is_ascii_digit)
                        && !rest.get(3).is_some_and(u8::is_ascii_digit)
                };
                if c.is_ascii_digit() {
                    end = i + 1;
                } else if !(matches!(
                    c,
                    '.' | ',' | '\'' | '_' | '\u{a0}' | '\u{2009}' | '\u{202f}'
                ) || (c == ' ' && group(&s.as_bytes()[i + 1..])))
                {
                    break;
                }
                k += 1;
            }
            return Some((start, end));
        } else {
            k += 1;
        }
    }
    None
}

/// An amount Ledger or Beancount wrote (`$11,619.00`, `EUR 120.00`,
/// `-45000.00 TRY`, `10 "VWRL2"`): its number the first one written, its
/// commodity what is before or after it (quoted, or a name with digits,
/// as written), and the style; the decimal mark the last of `.` and `,`
/// not followed by exactly three digits and another group.
pub fn text_amount(s: &str) -> Option<Amount> {
    let s = s.trim();
    let (mut start, end) = number_run(s)?;
    // The sign before the number (`-45000.00 TRY`, `$-19.00`), or before
    // the symbol (`-$19.00`).
    if s[..start].ends_with('-') {
        start -= 1;
    }
    let (before, number, after) = (&s[..start], &s[start..end], &s[end..]);
    let (sign_before, before) = match before.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, before),
    };
    let left = !before.trim().is_empty();
    let symbol = if left { before.trim() } else { after.trim() };
    let spaced = if left {
        before.ends_with(' ')
    } else {
        after.starts_with(' ')
    };
    let digits_only = number.trim_start_matches('-');
    let last = digits_only.rfind(['.', ',']);
    let decimal = match last {
        Some(i) => {
            let mark = digits_only.as_bytes()[i] as char;
            let tail = &digits_only[i + 1..];
            let other = if mark == '.' { ',' } else { '.' };
            if digits_only.contains(other) || tail.len() != 3 || digits_only[..i].contains(mark) {
                if digits_only[..i].contains(mark) {
                    other
                } else {
                    mark
                }
            } else {
                // `1,000` or `1.000`: a group, Ledger's and Beancount's
                // default writing `.` for decimals.
                '.'
            }
        }
        None => '.',
    };
    let mut q = Decimal::parse(number, decimal)?;
    if sign_before {
        q.mantissa = -q.mantissa;
    }
    let group = if decimal == '.' { ',' } else { '.' };
    let has_groups = digits_only.contains(group);
    Some(Amount {
        quantity: q,
        style: AmountStyle {
            symbol: symbol.to_string(),
            left,
            spaced,
            decimal,
            groups: has_groups.then(|| (group, vec![3])),
            precision: q.scale,
        },
    })
}

/// Which postings a total takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Cleared,
    Pending,
    /// Cleared and pending: what the statement's balance is matched
    /// with.
    Both,
}

/// The tool's total of `account` (with its subaccounts) in `main` for
/// `which` postings.
pub fn total_command(
    tool: Dialect,
    main: &str,
    account: &str,
    which: Which,
) -> (&'static str, Vec<String>) {
    let pattern = crate::report::account_pattern(account);
    match tool {
        Dialect::Hledger => {
            let mut a = vec![
                "-f".into(),
                main.into(),
                "balance".into(),
                format!("acct:{pattern}"),
            ];
            match which {
                Which::Cleared => a.push("status:*".into()),
                Which::Pending => a.push("status:!".into()),
                Which::Both => a.extend(["status:*".into(), "status:!".into()]),
            }
            a.extend(["-O".into(), "json".into()]);
            ("hledger", a)
        }
        Dialect::Ledger => {
            let mut a = vec![
                "-f".into(),
                main.into(),
                "balance".into(),
                "--flat".into(),
                "--balance-format".into(),
                "%(account)|%(scrub(display_total))\\n".into(),
            ];
            match which {
                Which::Cleared => a.push("--cleared".into()),
                Which::Pending => a.push("--pending".into()),
                Which::Both => a.extend(["--limit".into(), "cleared|pending".into()]),
            }
            a.extend(crate::report::ledger_pattern(account));
            ("ledger", a)
        }
        Dialect::Beancount => {
            let flag = match which {
                Which::Cleared => " AND flag != '!'",
                Which::Pending => " AND flag = '!'",
                Which::Both => "",
            };
            let q = format!(
                "SELECT units(sum(position)) AS b WHERE account ~ '{}'{flag}",
                pattern.replace('\'', "\\'")
            );
            (
                "bean-query",
                vec!["-f".into(), "csv".into(), main.into(), q],
            )
        }
    }
}

/// The amounts of a total as each tool printed it (hledger's JSON;
/// Ledger's rows, its total row where several accounts; Beancount's
/// CSV).
pub fn total_of(tool: Dialect, out: &str) -> Vec<Amount> {
    match tool {
        Dialect::Hledger => hledger_total(out),
        Dialect::Ledger => {
            let part = crate::report::Part::of(crate::report::ledger_rows(out));
            let amounts = if part.total.is_empty() {
                part.rows
                    .first()
                    .map(|r| r.amounts.clone())
                    .unwrap_or_default()
            } else {
                part.total
            };
            amounts.iter().filter_map(|a| text_amount(a)).collect()
        }
        Dialect::Beancount => crate::report::beancount_rows(out)
            .into_iter()
            .next()
            .map(|r| r.amounts.iter().filter_map(|a| text_amount(a)).collect())
            .unwrap_or_default(),
    }
}

/// The postings of `account` not cleared: hledger's pending and
/// unmarked (JSON), Ledger's uncleared (its register format),
/// Beancount's flagged `!`.
pub fn open_command(tool: Dialect, main: &str, account: &str) -> (&'static str, Vec<String>) {
    let pattern = crate::report::account_pattern(account);
    match tool {
        Dialect::Hledger => (
            "hledger",
            vec![
                "-f".into(),
                main.into(),
                "register".into(),
                format!("acct:{pattern}"),
                "status:!".into(),
                "status:".into(),
                "-O".into(),
                "json".into(),
            ],
        ),
        Dialect::Ledger => (
            "ledger",
            vec![
                "-f".into(),
                main.into(),
                "register".into(),
                "--uncleared".into(),
                "--register-format".into(),
                crate::report::LEDGER_REGISTER.into(),
            ]
            .into_iter()
            .chain(crate::report::ledger_pattern(account))
            .collect(),
        ),
        Dialect::Beancount => (
            "bean-query",
            vec![
                "-f".into(),
                "csv".into(),
                main.into(),
                format!(
                    "SELECT date, flag, payee, narration, position, balance, filename, lineno WHERE account ~ '{}' AND flag = '!'",
                    pattern.replace('\'', "\\'")
                ),
            ],
        ),
    }
}

/// The arguments of a run of the tool on `main` made to read the
/// journal from the standard input (`-f -`) instead: the text as typed,
/// unsaved, its includes still found from its folder, where the tool
/// runs.
pub fn from_input(args: Vec<String>, main: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        let after_f = out.last().is_some_and(|l| l == "-f");
        out.push(if after_f && a == main {
            "-".to_string()
        } else {
            a
        });
    }
    out
}

/// The postings a tool read from the standard input, their places put
/// back in `main`: hledger names its input `-`, Ledger nothing.
pub fn placed_in(postings: &mut [Posting], main: &str) {
    for p in postings {
        if let Some((file, _)) = &mut p.place
            && (file.is_empty() || file == "-" || file == "/dev/stdin")
        {
            *file = main.to_string();
        }
    }
}

/// The postings not cleared, as the tool printed them.
pub fn open_of(tool: Dialect, out: &str) -> Vec<Posting> {
    match tool {
        Dialect::Hledger => hledger_register(out),
        Dialect::Ledger => crate::report::ledger_register(out),
        Dialect::Beancount => crate::report::beancount_register(out),
    }
}

/// The total's amount the target is in: the one whose symbol the target
/// writes, else the first.
pub fn matching<'a>(target: &str, totals: &'a [Amount]) -> Option<&'a Amount> {
    totals
        .iter()
        .find(|a| !a.style.symbol.is_empty() && target.contains(a.style.symbol.as_str()))
        .or_else(|| totals.first())
}

/// The difference to the target: `target` less the tool's total of the
/// cleared and pending postings, per commodity; `None` where the target
/// does not read in the total's style.
pub fn difference(target: &str, total: &Amount) -> Option<Amount> {
    let number: String = target
        .trim()
        .trim_start_matches(total.style.symbol.as_str())
        .trim_end_matches(total.style.symbol.as_str())
        .trim()
        .to_string();
    let t = Decimal::parse(&number, total.style.decimal)?;
    Some(Amount {
        quantity: t.minus(total.quantity),
        style: total.style.clone(),
    })
}

/// The target as the journal writes it, in the total's style.
pub fn target_amount(target: &str, total: &Amount) -> Option<Amount> {
    let d = difference(target, total)?;
    let q = Decimal {
        mantissa: d.quantity.mantissa + total.quantity.rescale(d.quantity.scale).mantissa,
        scale: d.quantity.scale,
    };
    Some(Amount {
        quantity: q,
        style: total.style.clone(),
    })
}

/// One commodity's reconciliation as the document shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Figures {
    /// The commodity's symbol; empty when the account has one commodity.
    pub commodity: String,
    pub cleared: Vec<String>,
    pub pending: Vec<String>,
    /// The tool's total of both.
    pub total: Option<Amount>,
    pub target: String,
    pub difference: Option<Amount>,
}

/// The reconciliation's figures, a block per target: each target is a
/// commodity's (its symbol, empty for an account of one commodity, whose
/// total is then the one the target names, else the first) with the
/// statement's balance typed; its block the tool's totals of that
/// commodity and the difference. A commodity with nothing cleared or
/// pending has a total of zero, in its amounts' style.
pub fn blocks(
    targets: &[(String, String)],
    cleared: &[Amount],
    pending: &[Amount],
    both: &[Amount],
) -> Vec<Figures> {
    let single = targets.len() == 1 && targets[0].0.is_empty();
    targets
        .iter()
        .map(|(commodity, target)| {
            let total = if single {
                matching(target, both).cloned()
            } else {
                both.iter()
                    .find(|a| a.style.symbol == *commodity)
                    .cloned()
                    .or_else(|| {
                        cleared
                            .iter()
                            .chain(pending)
                            .find(|a| a.style.symbol == *commodity)
                            .map(|a| a.style.clone())
                            .or_else(|| text_amount(target).map(|a| a.style))
                            .map(|style| Amount {
                                quantity: Decimal::default(),
                                style,
                            })
                    })
            };
            let of = |list: &[Amount]| -> Vec<String> {
                list.iter()
                    .filter(|a| single || a.style.symbol == *commodity)
                    .map(Amount::written)
                    .collect()
            };
            Figures {
                commodity: if single {
                    String::new()
                } else {
                    commodity.clone()
                },
                cleared: of(cleared),
                pending: of(pending),
                difference: total.as_ref().and_then(|t| difference(target, t)),
                total,
                target: target.clone(),
            }
        })
        .collect()
}

/// The Reconcile document (with `note` under its hint when given): the figures, then the postings not cleared,
/// each line's posting's index (as text) in `lines`; with a statement
/// beside (PT8c), each posting said to be on it (muted) or not (yellow),
/// then the statement's lines without a posting (red).
pub fn layout(
    header: &str,
    hint: &str,
    note: Option<&str>,
    figures: &[Figures],
    postings: &[Posting],
    statement: Option<&crate::statement::Paired>,
) -> Document {
    let mut doc = Document::default();
    let push = |doc: &mut Document, s: String, style: Option<Style>, at: Option<String>| {
        let start = doc.text.len();
        doc.text.push_str(&s);
        if let Some(st) = style {
            doc.styles.push((start..doc.text.len(), st));
        }
        doc.text.push('\n');
        doc.lines.push(at);
    };
    push(&mut doc, header.to_string(), Some(Style::Header), None);
    push(&mut doc, hint.to_string(), Some(Style::Hint), None);
    // A word on the figures themselves (the journal as typed not read).
    if let Some(n) = note {
        push(&mut doc, n.to_string(), Some(Style::Warning), None);
    }
    push(&mut doc, String::new(), None, None);
    for (k, f) in figures.iter().enumerate() {
        // An account of several commodities: a block for each.
        if !f.commodity.is_empty() {
            if k > 0 {
                push(&mut doc, String::new(), None, None);
            }
            push(&mut doc, f.commodity.clone(), Some(Style::Section), None);
        }
        let total = f.total.as_ref().map(Amount::written).unwrap_or_default();
        let diff = f
            .difference
            .as_ref()
            .map(Amount::written)
            .unwrap_or_else(|| "?".into());
        let figures = [
            ("Cleared", f.cleared.join(", ")),
            ("Pending", f.pending.join(", ")),
            ("Cleared and pending", total),
            ("Target", f.target.clone()),
            ("Difference", diff.clone()),
        ];
        let w = figures
            .iter()
            .map(|(_, v)| v.chars().count())
            .max()
            .unwrap_or(0);
        for (name, value) in figures {
            let line = format!("{name:<20}{}{value}", " ".repeat(w - value.chars().count()));
            let start = doc.text.len();
            let zero = f.difference.as_ref().is_some_and(|d| d.quantity.is_zero());
            let style = match name {
                "Difference" if zero => Some(Style::Total),
                "Difference" => Some(Style::Negative),
                _ => None,
            };
            if let Some(st) = style {
                doc.styles.push((start + 20..start + line.len(), st));
            }
            push(&mut doc, line, None, None);
        }
    }
    push(&mut doc, String::new(), None, None);
    let open: Vec<(usize, &Posting)> = postings
        .iter()
        .enumerate()
        .filter(|(_, p)| p.status != '*')
        .collect();
    push(
        &mut doc,
        format!("Not cleared: {}", open.len()),
        Some(Style::Section),
        None,
    );
    let width = |s: &str| s.chars().count();
    let desc = open
        .iter()
        .map(|(_, p)| width(&p.description))
        .max()
        .unwrap_or(0)
        .clamp(12, 40);
    let amount = open
        .iter()
        .map(|(_, p)| width(&p.amount))
        .max()
        .unwrap_or(0);
    for (i, p) in open {
        let d: String = if width(&p.description) > desc {
            let mut t: String = p.description.chars().take(desc - 1).collect();
            t.push('…');
            t
        } else {
            p.description.clone()
        };
        let line = format!(
            "{} {} {d}{}  {}{}",
            p.status,
            p.date,
            " ".repeat(desc - width(&d)),
            " ".repeat(amount - width(&p.amount)),
            p.amount
        );
        let start = doc.text.len();
        if crate::report::negative(&p.amount) {
            doc.styles.push((
                start + line.len() - p.amount.len()..start + line.len(),
                Style::Negative,
            ));
        }
        let line = match statement {
            Some(s) if s.found.contains(&crate::statement::key(p)) => {
                let mark = "  on the statement";
                doc.styles.push((
                    start + line.len()..start + line.len() + mark.len(),
                    Style::Hint,
                ));
                format!("{line}{mark}")
            }
            Some(_) => {
                let mark = "  not on the statement";
                doc.styles.push((
                    start + line.len()..start + line.len() + mark.len(),
                    Style::Warning,
                ));
                format!("{line}{mark}")
            }
            None => line,
        };
        push(&mut doc, line, None, Some(i.to_string()));
    }
    if let Some(s) = statement {
        push(&mut doc, String::new(), None, None);
        push(
            &mut doc,
            format!("On the statement alone: {}", s.alone.len()),
            Some(Style::Section),
            None,
        );
        if !s.alone.is_empty() {
            push(
                &mut doc,
                "Not in the journal: Import in the statement (SPC m i) takes them".into(),
                Some(Style::Hint),
                None,
            );
        }
        for l in &s.alone {
            push(
                &mut doc,
                format!(
                    "  {}  {}  {}",
                    l.date.written('-'),
                    l.description,
                    l.written
                ),
                Some(Style::Negative),
                None,
            );
        }
    }
    doc
}

/// The dates of `account`'s postings in `text` marked neither on
/// themselves nor on their transaction and dated on or before `date`: a
/// balance assertion of the statement's balance at `date` would count
/// them, and fail. Beancount's transactions are all marked (`*`, `!` or
/// `txn`): none there. A posting's account is the tool's (`accounts`,
/// [`account_of`]).
pub fn unmarked_by(
    text: &str,
    dialect: Dialect,
    account: &str,
    date: Date,
    accounts: &Accounts,
) -> Vec<Date> {
    let mut out = Vec::new();
    if dialect == Dialect::Beancount {
        return out;
    }
    for t in write::transactions(text, dialect) {
        let marked = t
            .status
            .as_ref()
            .is_some_and(|r| matches!(text[r.clone()].trim(), "!" | "*"));
        if t.date > date || marked {
            continue;
        }
        let mut offset = t.range.start;
        for line in text[t.range.clone()].split_inclusive('\n') {
            if offset > t.range.start
                && let Some(a) = account_of(text, offset, dialect, accounts)
                && under(&a, account)
            {
                let own = line.trim_start();
                if !(own.starts_with('!') || own.starts_with('*')) {
                    out.push(t.date);
                }
            }
            offset += line.len();
        }
    }
    out
}

/// Whether `posting_account` is `account` or under it.
fn under(posting_account: &str, account: &str) -> bool {
    posting_account == account
        || posting_account
            .strip_prefix(account)
            .is_some_and(|r| r.starts_with(':'))
}

/// The account the tool reads at each posting line of a file, by line
/// from 1: an alias (`alias chk=Assets:Checking`) and `apply account`
/// resolved, which the text alone does not show.
pub type Accounts = BTreeMap<u32, String>;

/// Ledger's register line of a posting's place and account.
const LEDGER_ACCOUNTS: &str = "%(filename)\u{1f}%(beg_line)\u{1f}%(account)\\n";

/// The run of the tool that names every posting's place and account:
/// Ledger's register in a form of its own, hledger's `print` as JSON
/// (each transaction's lines, its postings in order). Beancount has no
/// aliases: none.
pub fn accounts_command(tool: Dialect, main: &str) -> Option<(&'static str, Vec<String>)> {
    match tool {
        Dialect::Hledger => Some((
            "hledger",
            vec![
                "-f".into(),
                main.into(),
                "print".into(),
                "-O".into(),
                "json".into(),
            ],
        )),
        Dialect::Ledger => Some((
            "ledger",
            vec![
                "-f".into(),
                main.into(),
                "register".into(),
                "--empty".into(),
                "--register-format".into(),
                LEDGER_ACCOUNTS.into(),
            ],
        )),
        Dialect::Beancount => None,
    }
}

/// The accounts of `file`'s posting lines in the output of a run of
/// [`accounts_command`], `text` being the file's text the tool read;
/// `file` `-` for a text the tool read on its standard input (hledger
/// names it `-`, Ledger nothing).
pub fn accounts_in(tool: Dialect, out: &str, file: &str, text: &str) -> Accounts {
    let here = |name: &str| {
        crate::journal::same_file(name, file)
            || (file == "-" && matches!(name, "" | "-" | "/dev/stdin"))
    };
    let mut accounts = Accounts::new();
    match tool {
        Dialect::Ledger => {
            for l in out.lines() {
                let mut f = l.split('\u{1f}');
                if let (Some(name), Some(line), Some(account)) = (f.next(), f.next(), f.next())
                    && here(name)
                    && let Ok(n) = line.trim().parse::<u32>()
                {
                    accounts.insert(n, account.trim().to_string());
                }
            }
        }
        Dialect::Hledger => {
            // A transaction's posting lines, in order, are its postings.
            let starts: Vec<usize> = std::iter::once(0)
                .chain(text.match_indices('\n').map(|(i, _)| i + 1))
                .collect();
            let all: Value = serde_json::from_str(out).unwrap_or_default();
            for t in all.as_array().into_iter().flatten() {
                let pos = &t["tsourcepos"];
                let (Some(name), Some(first), Some(end)) = (
                    pos[0]["sourceName"].as_str(),
                    pos[0]["sourceLine"].as_u64(),
                    pos[1]["sourceLine"].as_u64(),
                ) else {
                    continue;
                };
                if !here(name) {
                    continue;
                }
                let postings = t["tpostings"].as_array().map_or(&[][..], |p| &p[..]);
                let mut i = 0;
                for n in first + 1..end {
                    let Some(&offset) = starts.get(n as usize - 1) else {
                        break;
                    };
                    if write::account_at(text, offset, Dialect::Hledger).is_some() {
                        if let Some(a) = postings.get(i).and_then(|p| p["paccount"].as_str()) {
                            accounts.insert(n as u32, a.to_string());
                        }
                        i += 1;
                    }
                }
            }
        }
        Dialect::Beancount => {}
    }
    accounts
}

/// The account of the posting at `offset` in `text`: the tool's where
/// `accounts` names one for its line, else as written.
pub fn account_of(
    text: &str,
    offset: usize,
    dialect: Dialect,
    accounts: &Accounts,
) -> Option<String> {
    let written = write::account_at(text, offset, dialect)?;
    let line = text[..offset.min(text.len())].matches('\n').count() as u32 + 1;
    Some(accounts.get(&line).cloned().unwrap_or(written))
}

/// Finish: the edits that clear the pending postings of `account` in
/// `text` (a pending transaction's mark, or a pending posting's), and
/// the balance assertion written at `date`'s place: hledger's and
/// Ledger's a transaction "Balance assertion" with a zero posting
/// asserting `target`, Beancount's a `balance` directive the day after
/// (Beancount checks at the day's start). hledger's and Ledger's
/// assertion is aligned at `column` (`amount_column`; 0 leaves it near
/// the account), as Align Amounts aligns. A posting's account is the
/// tool's (`accounts`, [`account_of`]).
pub fn finish(
    text: &str,
    dialect: Dialect,
    account: &str,
    targets: &[Amount],
    date: Date,
    column: usize,
    accounts: &Accounts,
) -> Vec<(Range<usize>, String)> {
    let mut edits = Vec::new();
    for t in write::transactions(text, dialect) {
        let body = &text[t.range.clone()];
        let mut touches = false;
        let mut offset = t.range.start;
        for line in body.split_inclusive('\n') {
            let line_end = offset + line.len();
            if offset > t.range.start
                && let Some(a) = account_of(text, offset, dialect, accounts)
                && under(&a, account)
            {
                touches = true;
                // A posting's own pending mark.
                let trimmed = line.trim_start();
                if trimmed.starts_with("! ") || trimmed.starts_with("!\t") {
                    let at = offset + (line.len() - trimmed.len());
                    edits.push((at..at + 1, "*".to_string()));
                }
            }
            offset = line_end;
        }
        if touches
            && let Some(r) = t.status.clone()
            && text[r.clone()].trim() == "!"
        {
            edits.push((r.start..r.start + 1, "*".to_string()));
        }
    }
    let sep = write::separator(text, dialect);
    let (when, assertion) = match dialect {
        Dialect::Beancount => {
            // A `balance` directive a commodity.
            let next = date.plus(1);
            let lines: String = targets
                .iter()
                .map(|t| format!("{} balance {account}  {}\n", next.written(sep), t.written()))
                .collect();
            (next, lines)
        }
        _ => {
            // One transaction, a zero posting asserting each commodity.
            let mut t = format!("{} * Balance assertion\n", date.written(sep));
            for target in targets {
                let zero = Amount {
                    quantity: Decimal::default(),
                    style: target.style.clone(),
                };
                t.push_str(&format!(
                    "    {account}  {} = {}\n",
                    zero.written(),
                    target.written()
                ));
            }
            (date, t)
        }
    };
    let assertion = if dialect == Dialect::Beancount {
        assertion
    } else {
        let aligned = crate::align::edits(&assertion, 0..0, dialect, column);
        crate::align::apply(&assertion, &aligned)
    };
    let place = write::place(text, dialect, when, false);
    let (ins, _) = write::insertion(text, place, &assertion);
    edits.push((place..place, ins));
    edits.sort_by_key(|(r, _)| r.start);
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOTAL: &str = r#"[[], [{"acommodity": "TRY", "aquantity": {"decimalMantissa": 6369725, "decimalPlaces": 2}, "astyle": {"ascommodityside": "R", "ascommodityspaced": true, "asdecimalmark": ",", "asdigitgroups": [".", [3]], "asprecision": 2}}]]"#;

    #[test]
    fn hledgers_amounts() {
        let t = hledger_total(TOTAL);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].written(), "63.697,25 TRY");
        let neg = Amount {
            quantity: Decimal {
                mantissa: -84235,
                scale: 2,
            },
            ..t[0].clone()
        };
        assert_eq!(neg.written(), "-842,35 TRY");
        let fund = hledger_amount(&serde_json::json!({"acommodity": "FUND A", "aquantity": {"decimalMantissa": 1000, "decimalPlaces": 2}, "astyle": {"ascommodityside": "R", "ascommodityspaced": true, "asdecimalmark": ",", "asdigitgroups": null, "asprecision": 2}})).unwrap();
        assert_eq!(fund.written(), "10,00 \"FUND A\"");
        let dollars = hledger_amount(&serde_json::json!({"acommodity": "$", "aquantity": {"decimalMantissa": -1234567, "decimalPlaces": 2}, "astyle": {"ascommodityside": "L", "ascommodityspaced": false, "asdecimalmark": ".", "asdigitgroups": [",", [3]], "asprecision": 2}})).unwrap();
        assert_eq!(dollars.written(), "$-12,345.67");
    }

    #[test]
    fn the_difference() {
        let t = &hledger_total(TOTAL)[0];
        assert_eq!(difference("63.697,25", t).unwrap().written(), "0,00 TRY");
        assert_eq!(
            difference("63.700,00 TRY", t).unwrap().written(),
            "2,75 TRY"
        );
        assert_eq!(difference("63000", t).unwrap().written(), "-697,25 TRY");
        assert_eq!(
            target_amount("63000", t).unwrap().written(),
            "63.000,00 TRY"
        );
        assert!(difference("lots", t).is_none());
        let l = text_amount("$11,619.00").unwrap();
        assert_eq!(
            (l.quantity, l.style.left, l.style.decimal),
            (
                Decimal {
                    mantissa: 1161900,
                    scale: 2
                },
                true,
                '.'
            )
        );
        assert_eq!(difference("$11,600.00", &l).unwrap().written(), "$-19.00");
        let b = text_amount("-45000.00 TRY").unwrap();
        assert_eq!(b.written(), "-45000.00 TRY");
        assert_eq!(text_amount("EUR 120.00").unwrap().style.symbol, "EUR");
        assert_eq!(
            text_amount("1.234,5 TRY").unwrap().quantity,
            Decimal {
                mantissa: 12345,
                scale: 1
            }
        );
        // A commodity with digits, quoted (Ledger) or not (Beancount).
        let fund = text_amount("10 \"VWRL2\"").unwrap();
        assert_eq!(
            (fund.quantity, fund.style.symbol.as_str(), fund.style.left),
            (
                Decimal {
                    mantissa: 10,
                    scale: 0
                },
                "\"VWRL2\"",
                false
            )
        );
        assert_eq!(fund.written(), "10 \"VWRL2\"");
        assert_eq!(text_amount("-2.5 VWRL2").unwrap().written(), "-2.5 VWRL2");
        assert_eq!(
            text_amount("\"FUND 1\" 3.25").unwrap().written(),
            "\"FUND 1\" 3.25"
        );
        let minus = text_amount("-$19.00").unwrap();
        assert_eq!(
            (minus.written(), minus.style.symbol.as_str()),
            ("$-19.00".into(), "$")
        );
        assert_eq!(
            text_amount("1 000,50 EUR").unwrap().quantity,
            Decimal {
                mantissa: 100050,
                scale: 2
            }
        );
        assert!(text_amount("\"V2\"").is_none());
    }

    #[test]
    fn hledgers_register() {
        let json = r#"[["2026-01-03", null, "Market | groceries", {"pstatus": "Pending", "ptransaction_": "4", "pamount": [{"acommodity": "TRY", "aquantity": {"decimalMantissa": -84235, "decimalPlaces": 2}, "astyle": {"ascommodityside": "R", "ascommodityspaced": true, "asdecimalmark": ",", "asdigitgroups": [".", [3]], "asprecision": 2}}]}, [{"acommodity": "TRY", "aquantity": {"decimalMantissa": 9290725, "decimalPlaces": 2}, "astyle": {"ascommodityside": "R", "ascommodityspaced": true, "asdecimalmark": ",", "asdigitgroups": [".", [3]], "asprecision": 2}}]]]"#;
        let r = hledger_register(json);
        assert_eq!(r.len(), 1);
        assert_eq!(
            (r[0].status, r[0].amount.as_str(), r[0].total.as_str()),
            ('!', "-842,35 TRY", "92.907,25 TRY")
        );
        assert_eq!(r[0].transaction, Some(4));
    }

    #[test]
    fn finishing() {
        let text = "2026-01-02 * Salary\n    assets:bank  45.000,00 TRY\n    income:salary\n\n2026-01-03 ! Market\n    expenses:food  842,35 TRY\n    assets:bank\n\n2026-01-04 Card\n    expenses:food  10,00 TRY\n    ! assets:bank\n\n2026-01-05 ! Other\n    expenses:food  1,00 TRY\n    assets:cash\n";
        let target = target_amount("44.147,65", &hledger_total(TOTAL)[0]).unwrap();
        let edits = finish(
            text,
            Dialect::Hledger,
            "assets:bank",
            std::slice::from_ref(&target),
            Date {
                y: 2026,
                m: 1,
                d: 31,
            },
            0,
            &Accounts::new(),
        );
        let out = crate::align::apply(text, &edits);
        assert!(out.contains("2026-01-03 * Market"), "{out}");
        assert!(out.contains("    * assets:bank\n"), "{out}");
        assert!(
            out.contains("2026-01-05 ! Other"),
            "another account's pending stays"
        );
        assert!(
            out.ends_with(
                "\n2026-01-31 * Balance assertion\n    assets:bank  0,00 TRY = 44.147,65 TRY\n"
            ),
            "{out}"
        );
        // At the amount column: the zero's last digit at column 52.
        let at52 = crate::align::apply(
            text,
            &finish(
                text,
                Dialect::Hledger,
                "assets:bank",
                std::slice::from_ref(&target),
                Date {
                    y: 2026,
                    m: 1,
                    d: 31,
                },
                52,
                &Accounts::new(),
            ),
        );
        let last = at52.lines().last().unwrap();
        assert!(last.starts_with("    assets:bank   "), "{last:?}");
        assert_eq!(last.find(" TRY ="), Some(52), "{last:?}");
        let b = "2026-01-15 ! \"M\" \"G\"\n  Expenses:Food  842.35 TRY\n  Assets:Bank\n";
        let t = text_amount("76857.65 TRY").unwrap();
        let out = crate::align::apply(
            b,
            &finish(
                b,
                Dialect::Beancount,
                "Assets:Bank",
                std::slice::from_ref(&t),
                Date {
                    y: 2026,
                    m: 1,
                    d: 31,
                },
                52,
                &Accounts::new(),
            ),
        );
        assert!(out.starts_with("2026-01-15 * \"M\""), "{out}");
        assert!(
            out.ends_with("\n2026-02-01 balance Assets:Bank  76857.65 TRY\n"),
            "{out}"
        );
    }

    #[test]
    fn several_commodities() {
        let eur = text_amount("200.00 EUR").unwrap();
        let usd = text_amount("$100.00").unwrap();
        let cleared = [eur.clone(), usd.clone()];
        let pending = [text_amount("-20.00 EUR").unwrap()];
        let both = [text_amount("180.00 EUR").unwrap(), usd.clone()];
        let b = blocks(
            &[
                ("EUR".into(), "180.00 EUR".into()),
                ("$".into(), "$99.00".into()),
                ("GBP".into(), "5.00 GBP".into()),
            ],
            &cleared,
            &pending,
            &both,
        );
        assert_eq!(b.len(), 3);
        assert_eq!(
            (
                b[0].commodity.as_str(),
                b[0].cleared.clone(),
                b[0].pending.clone()
            ),
            (
                "EUR",
                vec!["200.00 EUR".to_string()],
                vec!["-20.00 EUR".to_string()]
            )
        );
        assert_eq!(b[0].difference.as_ref().unwrap().written(), "0.00 EUR");
        assert_eq!(b[1].difference.as_ref().unwrap().written(), "$-1.00");
        // Nothing of it cleared or pending: a total of zero.
        assert_eq!(b[2].total.as_ref().unwrap().written(), "0.00 GBP");
        assert_eq!(b[2].difference.as_ref().unwrap().written(), "5.00 GBP");
        // One commodity: the block of before, without a heading.
        let one = blocks(
            &[(String::new(), "$100.00".into())],
            std::slice::from_ref(&usd),
            &[],
            std::slice::from_ref(&usd),
        );
        assert_eq!(one[0].commodity, "");
        let doc = layout("h", "k", None, &b, &[], None);
        assert!(doc.text.contains("\nEUR\nCleared"), "{}", doc.text);
        assert!(doc.text.contains("\n\n$\nCleared"), "{}", doc.text);
        assert!(
            !layout("h", "k", None, &one, &[], None)
                .text
                .contains("\n$\n")
        );
        // A note under the hint (the journal as typed did not read).
        assert!(
            layout("h", "k", Some("As typed"), &one, &[], None)
                .text
                .starts_with("h\nk\nAs typed\n\n")
        );
        // Finish asserts each commodity.
        let text = "2026-01-01 * Opening\n    assets:cash  200.00 EUR\n    assets:cash  $100.00\n    equity:opening\n";
        let targets = [
            text_amount("180.00 EUR").unwrap(),
            text_amount("$100.00").unwrap(),
        ];
        let date = Date {
            y: 2026,
            m: 1,
            d: 31,
        };
        let out = crate::align::apply(
            text,
            &finish(
                text,
                Dialect::Hledger,
                "assets:cash",
                &targets,
                date,
                0,
                &Accounts::new(),
            ),
        );
        // Aligned with each other (column 0: as near the accounts as they go).
        let tail = out
            .split("2026-01-31 * Balance assertion\n")
            .nth(1)
            .unwrap_or_default();
        let lines: Vec<&str> = tail.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(
            lines[0].starts_with("    assets:cash ")
                && lines[0].ends_with(" 0.00 EUR = 180.00 EUR"),
            "{out}"
        );
        assert!(
            lines[1].starts_with("    assets:cash ") && lines[1].ends_with(" $0.00 = $100.00"),
            "{out}"
        );
        let bean = "2026-01-01 * \"Opening\"\n  Assets:Cash  200.00 EUR\n  Equity:Opening\n";
        let out = crate::align::apply(
            bean,
            &finish(
                bean,
                Dialect::Beancount,
                "Assets:Cash",
                &targets,
                date,
                0,
                &Accounts::new(),
            ),
        );
        assert!(
            out.ends_with("\n2026-02-01 balance Assets:Cash  180.00 EUR\n2026-02-01 balance Assets:Cash  $100.00\n"),
            "{out}"
        );
    }

    #[test]
    fn unmarked_postings_before_the_statement() {
        let text = "2026-01-05 * Cafe\n    expenses:food  20.00 EUR\n    assets:wallet\n\n2026-01-06 ! Taxi\n    expenses:transport  $15.00\n    assets:wallet\n\n2026-01-07 Museum\n    expenses:fun  12.00 EUR\n    assets:wallet\n\n2026-01-08 Kiosk\n    expenses:fun  1.00 EUR\n    ! assets:wallet\n\n2026-01-09 Bus\n    expenses:transport  2.00 EUR\n    assets:card\n";
        let d = |day| Date {
            y: 2026,
            m: 1,
            d: day,
        };
        // A pending transaction's postings are marked; so is a posting's
        // own mark; another account's posting does not count.
        assert!(
            unmarked_by(
                text,
                Dialect::Hledger,
                "assets:wallet",
                d(6),
                &Accounts::new()
            )
            .is_empty()
        );
        assert_eq!(
            unmarked_by(
                text,
                Dialect::Hledger,
                "assets:wallet",
                d(7),
                &Accounts::new()
            ),
            [d(7)]
        );
        assert_eq!(
            unmarked_by(
                text,
                Dialect::Hledger,
                "assets:wallet",
                d(31),
                &Accounts::new()
            ),
            [d(7)]
        );
        assert!(
            unmarked_by(
                text,
                Dialect::Beancount,
                "assets:wallet",
                d(31),
                &Accounts::new()
            )
            .is_empty()
        );
    }

    #[test]
    fn read_from_the_input() {
        let (_, args) = total_command(Dialect::Hledger, "/j/m.journal", "assets:bank", Which::Both);
        let live = from_input(args.clone(), "/j/m.journal");
        assert_eq!(live.len(), args.len());
        let at = args.iter().position(|a| a == "/j/m.journal").unwrap();
        assert_eq!(live[at], "-");
        assert_eq!(live[at - 1], "-f");
        // An argument equal to the path elsewhere stays.
        let other = from_input(vec!["print".into(), "/j/m.journal".into()], "/j/m.journal");
        assert_eq!(other, ["print", "/j/m.journal"]);
        let p = |file: &str| Posting {
            date: "2026-01-03".into(),
            status: ' ',
            description: "x".into(),
            amount: "1 EUR".into(),
            total: String::new(),
            place: Some((file.into(), 7)),
            transaction: None,
        };
        let mut list = [p("-"), p(""), p("/j/accounts.journal")];
        placed_in(&mut list, "/j/m.journal");
        let files: Vec<&str> = list
            .iter()
            .map(|p| p.place.as_ref().unwrap().0.as_str())
            .collect();
        assert_eq!(
            files,
            ["/j/m.journal", "/j/m.journal", "/j/accounts.journal"]
        );
    }

    #[test]
    fn accounts_the_tool_reads() {
        // Ledger's register lines: the file's postings only, `-` for a
        // text read on the standard input (Ledger names it nothing).
        let out = "/j/a.ledger\u{1f}5\u{1f}Assets:Checking\n/j/b.ledger\u{1f}2\u{1f}Expenses:Food\n\u{1f}7\u{1f}Assets:Cash\n";
        assert_eq!(
            accounts_in(Dialect::Ledger, out, "/j/a.ledger", ""),
            Accounts::from([(5, "Assets:Checking".to_string())])
        );
        assert_eq!(
            accounts_in(Dialect::Ledger, out, "-", ""),
            Accounts::from([(7, "Assets:Cash".to_string())])
        );
        // hledger's print: a transaction's posting lines in order, a
        // comment line skipped.
        let text = "alias chk = assets:checking\n\n2026-01-05 ! Grocer\n    expenses:food  $42.35\n    ; note\n    chk\n\n2026-01-06 Kiosk\n    expenses:food  $1.00\n    chk\n";
        let pos = |a, b| {
            format!(
                r#"[{{"sourceName": "-", "sourceLine": {a}, "sourceColumn": 1}}, {{"sourceName": "-", "sourceLine": {b}, "sourceColumn": 1}}]"#
            )
        };
        let json = format!(
            r#"[{{"tsourcepos": {}, "tpostings": [{{"paccount": "expenses:food"}}, {{"paccount": "assets:checking"}}]}}, {{"tsourcepos": {}, "tpostings": [{{"paccount": "expenses:food"}}, {{"paccount": "assets:checking"}}]}}]"#,
            pos(3, 7),
            pos(8, 11)
        );
        let accounts = accounts_in(Dialect::Hledger, &json, "-", text);
        assert_eq!(
            accounts,
            Accounts::from([
                (4, "expenses:food".to_string()),
                (6, "assets:checking".to_string()),
                (9, "expenses:food".to_string()),
                (10, "assets:checking".to_string()),
            ])
        );
        // Finish and its check take the tool's account: `chk` is
        // assets:checking.
        let d = Date {
            y: 2026,
            m: 1,
            d: 31,
        };
        let none = Accounts::new();
        assert!(unmarked_by(text, Dialect::Hledger, "assets:checking", d, &none).is_empty());
        let kiosk = Date { d: 6, ..d };
        assert_eq!(
            unmarked_by(text, Dialect::Hledger, "assets:checking", d, &accounts),
            [kiosk]
        );
        let target = text_amount("$43.35").unwrap();
        let cleared = |accounts: &Accounts| {
            crate::align::apply(
                text,
                &finish(
                    text,
                    Dialect::Hledger,
                    "assets:checking",
                    std::slice::from_ref(&target),
                    d,
                    0,
                    accounts,
                ),
            )
        };
        assert!(cleared(&accounts).contains("2026-01-05 * Grocer"));
        assert!(cleared(&none).contains("2026-01-05 ! Grocer"));
    }
}
