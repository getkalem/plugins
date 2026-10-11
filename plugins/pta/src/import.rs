//! Import from bank statements (PT9): a CSV statement read into the
//! journal by the tool. hledger's `import` through the statement's rules
//! (`statement.csv.rules`, hledger's convention): a dry run shown first,
//! then the real one, which hledger writes into the journal itself and
//! remembers (`.latest.statement.csv`), so a second import skips what was
//! taken; a statement without rules is offered rules from a template
//! made of its header row. Ledger's `convert`, whose output the plugin
//! appends. Beancount's importers are Python scripts (beangulp), which
//! the plugin may not run yet (PT9b).

use crate::Dialect;

/// hledger's import of `csv` into `journal` (`dry` for the dry run),
/// with `rules` when the setting names a rules file.
pub fn hledger_command(journal: &str, csv: &str, rules: Option<&str>, dry: bool) -> Vec<String> {
    let mut a = vec!["-f".to_string(), journal.to_string(), "import".to_string()];
    if dry {
        a.push("--dry-run".into());
    }
    if let Some(r) = rules.filter(|r| !r.trim().is_empty()) {
        a.extend(["--rules-file".into(), r.to_string()]);
    }
    a.push(csv.to_string());
    a
}

/// Ledger's conversion of `csv` (a header of Ledger's names: `date`,
/// `payee`, `amount`…) for `account`, its dates in `date_format`.
pub fn ledger_command(journal: &str, csv: &str, account: &str, date_format: &str) -> Vec<String> {
    let mut a = vec![
        "-f".to_string(),
        journal.to_string(),
        "convert".to_string(),
        csv.to_string(),
    ];
    if !account.trim().is_empty() {
        a.extend(["--account".into(), account.to_string()]);
    }
    if !date_format.trim().is_empty() {
        a.extend(["--input-date-format".into(), date_format.to_string()]);
    }
    a
}

/// What a dry run (or Ledger's conversion) gives: how many transactions,
/// and their text; hledger's first line ("would import 4 new
/// transactions from …:") left out.
pub fn drafted(tool: Dialect, stdout: &str) -> (usize, String) {
    let text = match tool {
        Dialect::Hledger => {
            let mut lines = stdout.lines();
            match lines.next() {
                Some(l)
                    if l.starts_with("would import") || l.starts_with("no new transactions") =>
                {
                    lines.collect::<Vec<_>>().join("\n")
                }
                Some(l) => std::iter::once(l)
                    .chain(lines)
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => String::new(),
            }
        }
        _ => stdout.to_string(),
    };
    let text = text.trim_matches('\n').to_string();
    let count = crate::write::transactions(&text, tool).len();
    let text = if text.is_empty() {
        text
    } else {
        format!("{text}\n")
    };
    (count, text)
}

/// The rules file hledger said is missing, from its error.
pub fn missing_rules(stderr: &str) -> Option<String> {
    let at = stderr.find(".rules: openFile: does not exist")?;
    let start = stderr[..at].rfind(['\n', ' ']).map_or(0, |i| i + 1);
    Some(stderr[start..at + ".rules".len()].trim().to_string())
}

/// A field's name in hledger's rules, from a header's: the usual ones by
/// meaning (`Date`, `Tarih` → `date`; `Description`, `Açıklama`,
/// `Payee` → `description`; `Amount`, `Tutar` → `amount`; `Balance`,
/// `Bakiye` → `balance`), another in lower case with `_` for blanks.
fn field(header: &str) -> String {
    let h = header.trim().trim_matches('"').to_lowercase();
    let is = |words: &[&str]| words.iter().any(|w| h == *w || h.starts_with(w));
    if is(&["date", "tarih", "datum", "fecha"]) {
        "date".into()
    } else if is(&[
        "description",
        "desc",
        "açıklama",
        "aciklama",
        "payee",
        "narration",
        "memo",
        "details",
    ]) {
        "description".into()
    } else if is(&["amount", "tutar", "betrag", "importe", "montant"]) {
        "amount".into()
    } else if is(&["balance", "bakiye", "saldo", "solde"]) {
        "balance".into()
    } else {
        h.chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_string()
    }
}

/// hledger's `date-format` for a statement's date (`2026-02-03`,
/// `03.02.2026`, `03/02/2026`, `2/3/2026`): day first where the first
/// part is above 12, or the separator is a dot; month first for a slash
/// whose second part is above 12.
fn date_format(date: &str) -> String {
    let d = date.trim().trim_matches('"');
    let sep = d.chars().find(|c| !c.is_ascii_digit()).unwrap_or('-');
    let parts: Vec<&str> = d.split(sep).collect();
    if parts.len() != 3 {
        return "%Y-%m-%d".into();
    }
    let num = |p: &str| p.parse::<u32>().unwrap_or(0);
    if parts[0].len() == 4 {
        return format!("%Y{sep}%m{sep}%d");
    }
    let day_first = sep == '.' || num(parts[0]) > 12 || (num(parts[1]) <= 12 && sep != '/');
    if day_first {
        format!("%d{sep}%m{sep}%Y")
    } else {
        format!("%m{sep}%d{sep}%Y")
    }
}

/// The rules for a statement whose text is `csv`: its header row's
/// fields, the date format and the decimal mark of its first data row,
/// `account1` the bank's account (unknown counterparts to hledger's
/// `expenses:unknown` and `income:unknown` until the rules say more).
pub fn rules_template(csv_name: &str, csv: &str, account1: &str) -> String {
    let mut lines = csv.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().unwrap_or("");
    let sep = [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|s| header.matches(*s).count())
        .unwrap_or(',');
    let fields: Vec<String> = header.split(sep).map(field).collect();
    let first: Vec<&str> = lines.next().unwrap_or("").split(sep).collect();
    let at = |name: &str| {
        fields
            .iter()
            .position(|f| f == name)
            .and_then(|i| first.get(i).copied())
    };
    let date = at("date")
        .map(date_format)
        .unwrap_or_else(|| "%Y-%m-%d".into());
    let amount = at("amount").unwrap_or("").trim().trim_matches('"');
    let comma = amount
        .rfind(',')
        .is_some_and(|i| amount.len() - i - 1 != 3 || !amount.contains('.'))
        && amount.rfind(',') > amount.rfind('.');
    let mut out = format!(
        "# hledger's rules for {csv_name}: its columns, its dates and amounts, and the\n\
         # accounts its lines go to (https://hledger.org/hledger.html#csv).\n"
    );
    out.push_str("skip 1\n");
    if sep != ',' {
        out.push_str(&format!(
            "separator {}\n",
            match sep {
                '\t' => "TAB".to_string(),
                s => s.to_string(),
            }
        ));
    }
    out.push_str(&format!("fields {}\n", fields.join(", ")));
    out.push_str(&format!("date-format {date}\n"));
    if comma {
        out.push_str("decimal-mark ,\n");
    }
    out.push_str(&format!("account1 {account1}\n"));
    out.push_str("\n# Where a line goes, by its description:\n# if %description GROCERY|MARKET\n#  account2 expenses:food\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commands() {
        assert_eq!(
            hledger_command("/j/2026.journal", "/s/statement.csv", None, true),
            [
                "-f",
                "/j/2026.journal",
                "import",
                "--dry-run",
                "/s/statement.csv"
            ]
        );
        assert!(
            hledger_command("/j", "/s.csv", Some("/r.rules"), false).ends_with(&[
                "--rules-file".into(),
                "/r.rules".into(),
                "/s.csv".into()
            ])
        );
        assert_eq!(
            ledger_command("/j/b.ledger", "/s.csv", "Assets:Checking", "%Y/%m/%d"),
            [
                "-f",
                "/j/b.ledger",
                "convert",
                "/s.csv",
                "--account",
                "Assets:Checking",
                "--input-date-format",
                "%Y/%m/%d"
            ]
        );
    }

    #[test]
    fn what_a_dry_run_says() {
        let out = "would import 2 new transactions from statement.csv:\n\n2026-02-03 MIGROS  ; shop:migros\n    assets:bank:ziraat       -412,50 TRY\n    expenses:food:market      412,50 TRY\n\n2026-02-05 MAAS\n    assets:bank:ziraat     45.000,00 TRY\n    income:salary\n\n";
        let (n, text) = drafted(Dialect::Hledger, out);
        assert_eq!(n, 2);
        assert!(
            text.starts_with("2026-02-03 MIGROS") && text.ends_with("income:salary\n"),
            "{text:?}"
        );
        assert_eq!(
            drafted(
                Dialect::Hledger,
                "no new transactions found in statement.csv\n"
            ),
            (0, String::new())
        );
        let l = "2026/02/05 * Client B\n    Expenses:Unknown  $500.00\n    Assets:Checking\n";
        assert_eq!(drafted(Dialect::Ledger, l).0, 1);
        assert_eq!(
            missing_rules("hledger: Error: /s/other.csv.rules: openFile: does not exist (No such file or directory)\n").as_deref(),
            Some("/s/other.csv.rules")
        );
        assert_eq!(missing_rules("hledger: Error: something else"), None);
    }

    #[test]
    fn a_rules_template() {
        let csv = "Date,Description,Amount,Balance\n2026-02-03,MIGROS KADIKOY,-412.50,63284.75\n";
        let r = rules_template("statement.csv", csv, "assets:bank:ziraat");
        assert!(r.contains("\nskip 1\nfields date, description, amount, balance\ndate-format %Y-%m-%d\naccount1 assets:bank:ziraat\n"), "{r}");
        assert!(!r.contains("decimal-mark"));
        let tr = "Tarih;Açıklama;Tutar;Bakiye\n03.02.2026;MIGROS;-412,50;63.284,75\n";
        let r = rules_template("ekstre.csv", tr, "assets:bank");
        assert!(r.contains("separator ;\nfields date, description, amount, balance\ndate-format %d.%m.%Y\ndecimal-mark ,\n"), "{r}");
        assert_eq!(date_format("2/13/2026"), "%m/%d/%Y");
        assert_eq!(date_format("13/02/2026"), "%d/%m/%Y");
        assert_eq!(field(" Txn Code "), "txn_code");
    }
}
