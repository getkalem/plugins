//! Import from bank statements (PT9): a CSV statement read into the
//! journal by the tool. hledger's `import` through the statement's rules
//! (`statement.csv.rules`, hledger's convention): a dry run shown first,
//! then the real one, which hledger writes into the journal itself and
//! remembers (`.latest.statement.csv`), so a second import skips what was
//! taken; a statement without rules is offered rules from a template
//! made of its header row. Ledger's `convert`, whose output the plugin
//! appends. Beancount's importers are the user's beangulp script
//! (`import.py`), run with `python3 import.py extract` (PT9b), whose
//! entries the plugin appends too.

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

/// The user's beangulp script for `journal`: the setting `importer`
/// (beside the journal when relative), else `import.py` beside it.
pub fn beangulp_script(journal: &str, setting: &str) -> String {
    let set = setting.trim();
    if set.is_empty() {
        crate::journal::beside(journal, "import.py")
    } else {
        crate::journal::beside(journal, set)
    }
}

/// beangulp's extraction of `csv` by `script` (run by `python3`), the
/// journal's entries given so that those already in it are marked as
/// duplicates (`-e`).
pub fn beangulp_command(script: &str, csv: &str, journal: &str) -> Vec<String> {
    vec![
        script.to_string(),
        "extract".into(),
        "-e".into(),
        journal.to_string(),
        csv.to_string(),
    ]
}

/// What Python said last on standard error (a traceback's last line),
/// or the hint for a script not found.
pub fn python_said(stderr: &str, script: &str) -> String {
    let last = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("failed")
        .trim();
    if stderr.contains("can't open file")
        || stderr.contains("No such file") && stderr.contains(script)
    {
        format!(
            "No beangulp script at {script}: write one (beangulp's import.py) beside the journal, or name it in the setting importer"
        )
    } else {
        last.to_string()
    }
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
        // beangulp's: its mode line, its `**** FILE` headings and the
        // entries it found already in the journal (commented out as
        // duplicates) left out.
        Dialect::Beancount => {
            let mut out: Vec<&str> = Vec::new();
            let mut duplicate = false;
            for l in stdout.lines() {
                if l.starts_with("; duplicate of ") {
                    duplicate = true;
                    continue;
                }
                if duplicate {
                    if l.trim().is_empty() {
                        duplicate = false;
                    }
                    continue;
                }
                if l.starts_with(";; -*-") || l.starts_with("**** ") {
                    continue;
                }
                out.push(l);
            }
            out.join("\n").trim_matches('\n').to_string()
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

/// The `k`-th last transaction of hledger's `print -O json` by its
/// number (`tindex`): the first of the last `k` appended.
pub fn nth_last(print_json: &str, k: usize) -> Option<usize> {
    let all: serde_json::Value = serde_json::from_str(print_json).ok()?;
    let mut n: Vec<u64> = all
        .as_array()?
        .iter()
        .filter_map(|t| t["tindex"].as_u64())
        .collect();
    n.sort_unstable();
    n.len()
        .checked_sub(k.max(1))
        .and_then(|i| n.get(i))
        .map(|&t| t as usize)
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
pub(crate) fn field(header: &str) -> String {
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
pub(crate) fn date_format(date: &str) -> String {
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
        let bg = ";; -*- mode: beancount -*-\n\n**** /s/feb.csv\n\n; duplicate of /j/h.beancount:68\n; 2026-02-03 * \"MIGROS\"\n;   Assets:Bank:Ziraat  -412.50 TRY\n\n2026-02-05 * \"MAAS\"\n  Assets:Bank:Ziraat  45000.00 TRY\n\n";
        let (n, text) = drafted(Dialect::Beancount, bg);
        assert_eq!(n, 1);
        assert_eq!(
            text,
            "2026-02-05 * \"MAAS\"\n  Assets:Bank:Ziraat  45000.00 TRY\n"
        );
        assert_eq!(
            beangulp_command("/j/import.py", "/s/s.csv", "/j/main.beancount"),
            [
                "/j/import.py",
                "extract",
                "-e",
                "/j/main.beancount",
                "/s/s.csv"
            ]
        );
        assert_eq!(beangulp_script("/j/main.beancount", ""), "/j/import.py");
        assert_eq!(
            beangulp_script("/j/main.beancount", "tools/imp.py"),
            "/j/tools/imp.py"
        );
        assert!(
            python_said(
                "python3: can't open file '/j/import.py': [Errno 2] No such file or directory",
                "/j/import.py"
            )
            .starts_with("No beangulp script")
        );
        assert_eq!(
            python_said(
                "Traceback…\n  File x\nValueError: bad date\n",
                "/j/import.py"
            ),
            "ValueError: bad date"
        );
        let printed = r#"[{"tindex": 7}, {"tindex": 40}, {"tindex": 41}]"#;
        assert_eq!(nth_last(printed, 2), Some(40));
        assert_eq!(nth_last(printed, 1), Some(41));
        assert_eq!(nth_last(printed, 4), None);
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
