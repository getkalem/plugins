//! Completion in a journal (the plugin's completer, plugin API 0.2.11): an
//! account on a posting line and in Beancount's directives, a payee on a
//! transaction's line, a commodity in an amount. The names come from the
//! lists the journal's tool gives of its main journal (`hledger accounts`,
//! `ledger payees`, `bean-query`'s), read ahead and again on each save,
//! else from the text itself. Ledger has no language server: this is its
//! completion; with a server, the plugin's items stand in for the
//! server's accounts and payees.

use crate::Dialect;

/// What the cursor is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Account,
    Payee,
    Commodity,
}

/// The names a journal knows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Known {
    pub accounts: Vec<String>,
    pub payees: Vec<String>,
    pub commodities: Vec<String>,
}

impl Known {
    /// The names of `slot`.
    pub fn of(&self, slot: Slot) -> &[String] {
        match slot {
            Slot::Account => &self.accounts,
            Slot::Payee => &self.payees,
            Slot::Commodity => &self.commodities,
        }
    }

    /// `slot`'s names, in place of what was known of them.
    pub fn set(&mut self, slot: Slot, names: Vec<String>) {
        match slot {
            Slot::Account => self.accounts = names,
            Slot::Payee => self.payees = names,
            Slot::Commodity => self.commodities = names,
        }
    }
}

/// The most items offered at once.
pub const MAX_ITEMS: usize = 60;

/// What is completed at byte `cursor` of `text`, and where the text it
/// replaces starts; none in a comment, a date, a note or where nothing is
/// named.
pub fn slot(text: &str, cursor: usize, d: Dialect) -> Option<(Slot, usize)> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    let start = text[..cursor].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[start..cursor];
    if before.starts_with([' ', '\t']) {
        posting(before, d).map(|(s, at)| (s, start + at))
    } else {
        header(before, d).map(|(s, at)| (s, start + at))
    }
}

/// A posting line up to the cursor: its account, or its amount's
/// commodity.
fn posting(before: &str, d: Dialect) -> Option<(Slot, usize)> {
    let mut at = before.len() - before.trim_start().len();
    let mut rest = &before[at..];
    // A comment under a transaction, or Beancount's metadata.
    if rest.starts_with([';', '#'])
        || (d == Dialect::Beancount && rest.starts_with(|c: char| c.is_ascii_lowercase()))
    {
        return None;
    }
    // A posting's own mark.
    if let Some(r) = rest.strip_prefix(['*', '!'])
        && r.starts_with([' ', '\t'])
    {
        let skip = rest.len() - r.trim_start().len();
        at += skip;
        rest = &rest[skip..];
    }
    // A virtual account.
    if let Some(r) = rest.strip_prefix(['(', '[']) {
        at += 1;
        rest = r;
    }
    let Some(end) = account_end(rest, d) else {
        return Some((Slot::Account, at));
    };
    // The amount: a commodity's letters after its number, or before it.
    let amount = &rest[end..];
    if amount.contains(';') {
        return None;
    }
    let word: usize = amount
        .chars()
        .rev()
        .take_while(|c| c.is_alphabetic() || *c == '_')
        .map(char::len_utf8)
        .sum();
    let lead = &amount[..amount.len() - word];
    let after_number = lead.contains(|c: char| c.is_ascii_digit())
        && lead.ends_with(|c: char| c == ' ' || c.is_ascii_digit());
    let first = lead.trim().is_empty() || lead.trim_end() == "-";
    (word > 0 && (after_number || first)).then(|| (Slot::Commodity, at + end + lead.len()))
}

/// Where an account written at the start of `rest` ends: two blanks, a
/// tab, a virtual account's bracket (Beancount: a blank); none while the
/// cursor is still in it.
fn account_end(rest: &str, d: Dialect) -> Option<usize> {
    rest.char_indices()
        .find(|&(i, c)| {
            c == '\t'
                || c == ')'
                || c == ']'
                || rest[i..].starts_with("  ")
                || (d == Dialect::Beancount && c == ' ')
        })
        .map(|(i, _)| i)
}

/// A line starting with a date, up to the cursor: a transaction's payee,
/// or the account a Beancount directive names.
fn header(before: &str, d: Dialect) -> Option<(Slot, usize)> {
    let (_, len, _) = crate::write::date_at(before)?;
    let mut at = len;
    // hledger's second date.
    if let Some(r) = before[at..].strip_prefix('=')
        && let Some((_, l2, _)) = crate::write::date_at(r)
    {
        at += 1 + l2;
    }
    let rest = &before[at..];
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let blanks = |s: &str| s.len() - s.trim_start().len();
    at += blanks(rest);
    let mut rest = &before[at..];
    if d == Dialect::Beancount {
        let word: String = rest
            .chars()
            .take_while(|c| c.is_ascii_lowercase())
            .collect();
        if matches!(
            word.as_str(),
            "open" | "close" | "balance" | "pad" | "note" | "document"
        ) {
            let after = &rest[word.len()..];
            if !after.starts_with(' ') {
                return None;
            }
            // The account being typed: the last word, the first or (pad)
            // the second account.
            let last = after.rsplit(' ').next().unwrap_or("");
            let words = after.split_whitespace().count() + usize::from(after.ends_with(' '));
            let most = if word == "pad" { 2 } else { 1 };
            return (words <= most).then(|| (Slot::Account, before.len() - last.len()));
        }
        // A transaction: its flag, then its payee's string.
        let flag = rest
            .strip_prefix("txn")
            .or_else(|| rest.strip_prefix(['*', '!']))?;
        if !flag.starts_with(' ') {
            return None;
        }
        let quoted = flag.trim_start();
        let inner = quoted.strip_prefix('"')?;
        if inner.contains('"') {
            return None;
        }
        return Some((Slot::Payee, before.len() - inner.len()));
    }
    // hledger and Ledger: a status, a code, then the description.
    if let Some(r) = rest.strip_prefix(['*', '!'])
        && (r.is_empty() || r.starts_with([' ', '\t']))
    {
        at += 1 + blanks(r);
        rest = &before[at..];
    }
    if rest.starts_with('(') {
        let close = rest.find(')')?;
        at += close + 1;
        at += blanks(&before[at..]);
        rest = &before[at..];
    }
    // The payee: before hledger's ` | ` and a comment.
    if rest.contains(['|', ';']) {
        return None;
    }
    Some((Slot::Payee, at))
}

/// The names of `slot` `typed` leads to, best first: those it starts
/// (any case), then, for an account, those whose parts its parts start
/// (`ex:fo` for `expenses:food`, `food` for `expenses:food:market`), for
/// a payee, those holding it; [`MAX_ITEMS`] at most.
pub fn matches(known: &Known, slot: Slot, typed: &str) -> Vec<String> {
    let t = typed.to_lowercase();
    let mut first = Vec::new();
    let mut then = Vec::new();
    for name in known.of(slot) {
        let n = name.to_lowercase();
        if n.starts_with(&t) {
            first.push(name.clone());
        } else if !t.is_empty()
            && match slot {
                Slot::Account => parts_match(&n, &t),
                Slot::Payee => n.contains(&t),
                Slot::Commodity => false,
            }
        {
            then.push(name.clone());
        }
    }
    first.extend(then);
    first.truncate(MAX_ITEMS);
    first
}

/// `typed`'s parts (split at `:`) each the start of an account's part,
/// one after another, from any of its parts.
fn parts_match(account: &str, typed: &str) -> bool {
    let parts: Vec<&str> = account.split(':').collect();
    let want: Vec<&str> = typed.split(':').collect();
    (0..parts.len()).any(|s| {
        want.iter()
            .enumerate()
            .all(|(k, w)| parts.get(s + k).is_some_and(|p| p.starts_with(w)))
    })
}

/// The tool's runs listing the main journal's accounts, payees and
/// commodities.
pub fn list_commands(tool: Dialect, main: &str) -> Vec<(Slot, &'static str, Vec<String>)> {
    let run = |slot: Slot, program: &'static str, what: &str| {
        (
            slot,
            program,
            vec!["-f".to_string(), main.to_string(), what.to_string()],
        )
    };
    match tool {
        Dialect::Hledger => vec![
            run(Slot::Account, "hledger", "accounts"),
            run(Slot::Payee, "hledger", "payees"),
            run(Slot::Commodity, "hledger", "commodities"),
        ],
        Dialect::Ledger => vec![
            run(Slot::Account, "ledger", "accounts"),
            run(Slot::Payee, "ledger", "payees"),
            run(Slot::Commodity, "ledger", "commodities"),
        ],
        Dialect::Beancount => [
            (Slot::Account, "SELECT DISTINCT account ORDER BY account"),
            (Slot::Payee, "SELECT DISTINCT payee ORDER BY payee"),
            (
                Slot::Commodity,
                "SELECT DISTINCT currency ORDER BY currency",
            ),
        ]
        .into_iter()
        .map(|(slot, q)| {
            (
                slot,
                "bean-query",
                vec!["-f".into(), "csv".into(), main.into(), q.into()],
            )
        })
        .collect(),
    }
}

/// The names a list run printed, a name a line (`bean-query`'s CSV after
/// its header, a quoted one unquoted), blank ones left out.
pub fn list_of(tool: Dialect, out: &str) -> Vec<String> {
    let lines = out.lines().skip(usize::from(tool == Dialect::Beancount));
    lines
        .map(|l| {
            let l = l.trim();
            match l.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
                Some(inner) => inner.replace("\"\"", "\""),
                None => l.to_string(),
            }
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// The names `text` itself writes: its postings' accounts, its declared
/// ones (`account`, Beancount's `open`), its transactions' payees; for a
/// journal whose tool has not listed them yet.
pub fn from_text(text: &str, d: Dialect) -> Known {
    let mut k = Known::default();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let l = line.trim_end_matches(['\n', '\r']);
        if l.starts_with([' ', '\t']) {
            if let Some(a) = crate::write::account_at(text, start + l.len(), d) {
                k.accounts.push(a);
            }
        } else if let Some(name) = l.strip_prefix("account ") {
            k.accounts
                .push(name.split(';').next().unwrap_or("").trim().to_string());
        } else if let Some((_, len, _)) = crate::write::date_at(l) {
            let rest = l[len..].trim_start();
            if d == Dialect::Beancount {
                if let Some(a) = rest.strip_prefix("open ") {
                    if let Some(a) = a.split_whitespace().next() {
                        k.accounts.push(a.to_string());
                    }
                } else if let Some(q) = rest
                    .strip_prefix(['*', '!'])
                    .or_else(|| rest.strip_prefix("txn"))
                    .map(str::trim_start)
                    .and_then(|r| r.strip_prefix('"'))
                    && let Some(end) = q.find('"')
                    && q[end + 1..].trim_start().starts_with('"')
                {
                    k.payees.push(q[..end].to_string());
                }
            } else if let Some(cut) = Some(l.find(['|', ';']).unwrap_or(l.len()))
                && let Some((Slot::Payee, p)) = header(&l[..cut], d)
            {
                let payee = l[p..cut].trim();
                if !payee.is_empty() {
                    k.payees.push(payee.to_string());
                }
            }
        }
    }
    for list in [&mut k.accounts, &mut k.payees] {
        list.retain(|n| !n.is_empty());
        list.sort();
        list.dedup();
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    const HLEDGER: &str = "account assets:bank:checking\n\n2026-01-03 ! (12) Grocery store | groceries  ; food:\n    expenses:food:groceries        $84.35\n    ! assets:bank:checking\n    (budget:food)  $-84.35\n";

    fn at(text: &str, marker: &str, d: Dialect) -> Option<(Slot, String)> {
        // The cursor where `marker` ends in `text`.
        let cursor = text.find(marker).expect("marker") + marker.len();
        slot(text, cursor, d).map(|(s, start)| (s, text[start..cursor].to_string()))
    }

    #[test]
    fn what_is_completed_where() {
        let d = Dialect::Hledger;
        assert_eq!(
            at(HLEDGER, "    expenses:fo", d),
            Some((Slot::Account, "expenses:fo".into()))
        );
        assert_eq!(
            at(HLEDGER, "    ! assets:ba", d),
            Some((Slot::Account, "assets:ba".into()))
        );
        assert_eq!(
            at(HLEDGER, "    (budget:fo", d),
            Some((Slot::Account, "budget:fo".into()))
        );
        assert_eq!(
            at(HLEDGER, "(12) Groc", d),
            Some((Slot::Payee, "Groc".into()))
        );
        assert_eq!(at(HLEDGER, "Grocery store | groc", d), None);
        assert!(at(HLEDGER, "; foo", d).is_none());
        assert_eq!(at(HLEDGER, "2026-01", d), None);
        assert_eq!(at(HLEDGER, "account assets:bank", d), None);
        let amount = "2026-01-03 x\n    assets:cash  12.50 EU";
        assert_eq!(
            at(amount, "12.50 EU", d),
            Some((Slot::Commodity, "EU".into()))
        );
        let left = "2026-01-03 x\n    assets:cash  EU";
        assert_eq!(at(left, "  EU", d), Some((Slot::Commodity, "EU".into())));
        let ledger = "2026/01/09 * Software subscription\n    Expenses:So";
        assert_eq!(
            at(ledger, "    Expenses:So", Dialect::Ledger),
            Some((Slot::Account, "Expenses:So".into()))
        );
        assert_eq!(
            at(ledger, "* Soft", Dialect::Ledger),
            Some((Slot::Payee, "Soft".into()))
        );
        let b = Dialect::Beancount;
        let bean = "2026-01-01 open Assets:Ca\n2026-01-03 * \"Groc\" \"weekly\"\n  Expenses:Fo\n  memo: \"x\"\n2026-01-04 pad Assets:Cash Equity:Op\n";
        assert_eq!(
            at(bean, "open Assets:Ca", b),
            Some((Slot::Account, "Assets:Ca".into()))
        );
        assert_eq!(at(bean, "* \"Groc", b), Some((Slot::Payee, "Groc".into())));
        assert_eq!(at(bean, "\"Groc\" \"week", b), None);
        assert_eq!(
            at(bean, "  Expenses:Fo", b),
            Some((Slot::Account, "Expenses:Fo".into()))
        );
        assert_eq!(at(bean, "  memo", b), None);
        assert_eq!(
            at(bean, "Equity:Op", b),
            Some((Slot::Account, "Equity:Op".into()))
        );
    }

    #[test]
    fn names_matched() {
        let k = Known {
            accounts: vec![
                "assets:bank:checking".into(),
                "expenses:food".into(),
                "expenses:food:groceries".into(),
                "expenses:rent".into(),
            ],
            payees: vec!["Acme Corp".into(), "Grocery store".into()],
            commodities: vec!["EUR".into(), "USD".into()],
        };
        assert_eq!(
            matches(&k, Slot::Account, "exp"),
            ["expenses:food", "expenses:food:groceries", "expenses:rent"]
        );
        assert_eq!(
            matches(&k, Slot::Account, "ex:fo"),
            ["expenses:food", "expenses:food:groceries"]
        );
        assert_eq!(
            matches(&k, Slot::Account, "groc"),
            ["expenses:food:groceries"]
        );
        assert_eq!(
            matches(&k, Slot::Account, "Assets:B"),
            ["assets:bank:checking"]
        );
        assert_eq!(matches(&k, Slot::Payee, "store"), ["Grocery store"]);
        assert_eq!(matches(&k, Slot::Payee, "g"), ["Grocery store"]);
        assert_eq!(matches(&k, Slot::Commodity, "e"), ["EUR"]);
        assert_eq!(matches(&k, Slot::Payee, "").len(), 2);
    }

    #[test]
    fn lists() {
        let c = list_commands(Dialect::Ledger, "/j/b.ledger");
        assert_eq!(c[1].2, ["-f", "/j/b.ledger", "payees"]);
        assert_eq!(
            list_of(Dialect::Hledger, "assets\nassets:bank\n\n"),
            ["assets", "assets:bank"]
        );
        assert_eq!(
            list_of(Dialect::Beancount, "payee\nAcme\n\"Smith, \"\"J\"\"\"\n"),
            ["Acme", "Smith, \"J\""]
        );
        let k = from_text(HLEDGER, Dialect::Hledger);
        assert_eq!(
            k.accounts,
            [
                "assets:bank:checking",
                "budget:food",
                "expenses:food:groceries"
            ]
        );
        assert_eq!(k.payees, ["Grocery store"]);
        let b = from_text(
            "2026-01-01 open Assets:Cash USD\n2026-01-03 * \"Acme\" \"pay\"\n  Assets:Cash  1 USD\n",
            Dialect::Beancount,
        );
        assert_eq!(b.accounts, ["Assets:Cash"]);
        assert_eq!(b.payees, ["Acme"]);
    }
}
