//! Import (PT9): in a CSV statement, `SPC m i` shows what the tool would
//! import into the journal (the document "Import: …", in the journal's
//! syntax), and `i` there imports it: hledger writes the journal itself,
//! Ledger's conversion is appended by the plugin in the journal. A
//! statement without hledger's rules is offered rules from a template.

use std::cell::RefCell;
use std::collections::BTreeMap;

use kalem_plugin::{documents, editor, kalem, settings, ui};

use crate::Dialect;
use crate::import;
use crate::journal::{self, ToolChoice};

use super::{NotRun, run};

const DOC: &str = "pta.import";
pub(super) const KIND: &str = "pta-import";

/// A statement's import.
#[derive(Debug)]
struct Import {
    journal: String,
    tool: Dialect,
    doc: Option<u64>,
    count: usize,
    text: String,
}

thread_local! {
    /// By the statement's file.
    static IMPORTS: RefCell<BTreeMap<String, Import>> = const { RefCell::new(BTreeMap::new()) };
    /// Rules made from the template, waiting for the command that writes
    /// them in their new document.
    static RULES: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

fn own(key: &str) -> String {
    settings::own(key)
        .and_then(|v| serde_json::from_str::<String>(&v).ok())
        .unwrap_or_default()
}

/// The statement whose Import document is the current one.
pub(super) fn here() -> Option<String> {
    let n = documents::current()?;
    IMPORTS.with(|i| {
        i.borrow()
            .iter()
            .find(|(_, e)| e.doc == Some(n))
            .map(|(k, _)| k.clone())
    })
}

/// The statement's rules file: the setting `rules`, else hledger's
/// convention, beside it.
fn rules_of(csv: &str) -> String {
    let set = own("rules");
    if set.trim().is_empty() {
        format!("{csv}.rules")
    } else if set.starts_with('/') || set.starts_with('~') {
        set
    } else {
        format!("{}/{set}", journal::folder(csv))
    }
}

/// The journal of a file that is no journal (a statement goes into it, a
/// note's tables come from it): the setting `journal` (beside the file
/// when relative), else the one `LEDGER_FILE` names, else the journal
/// last opened in this session.
pub(super) fn journal_of(csv: &str, done: impl FnOnce(Option<String>) + 'static) {
    let set = own("journal");
    if !set.trim().is_empty() {
        let p = if set.starts_with('/') || set.starts_with('~') {
            set
        } else {
            format!("{}/{set}", journal::folder(csv))
        };
        return done(Some(p));
    }
    run(
        "hledger",
        vec!["files".into()],
        journal::folder(csv),
        move |r| {
            let listed = r
                .ok()
                .filter(|e| e.status == Some(0))
                .and_then(|e| journal::files_listed(&e.stdout).into_iter().next());
            done(listed.or_else(super::last_journal));
        },
    );
}

/// Import (`SPC m i` in a CSV statement): what the tool would import;
/// in the Import document, the dry run again.
pub(super) fn command() -> Result<String, String> {
    if let Some(csv) = here() {
        dry_run(csv, None, false);
        return Ok("null".into());
    }
    let Some(doc) = editor::document() else {
        return Ok("null".into());
    };
    let Some(csv) = doc.path.filter(|p| {
        let l = p.to_ascii_lowercase();
        l.ends_with(".csv") || l.ends_with(".tsv")
    }) else {
        ui::notify(
            "Import runs in a bank statement: a CSV file",
            ui::Level::Info,
        );
        return Ok("null".into());
    };
    let text = editor::text(None);
    journal_of(&csv.clone(), move |journal| {
        let Some(journal) = journal else {
            return ui::notify(
                "Which journal? Set the plugin's setting journal, or open the journal first",
                ui::Level::Warning,
            );
        };
        let Some(dialect) = crate::Dialect::of_path(&journal) else {
            return ui::notify(&format!("{journal} is no journal"), ui::Level::Warning);
        };
        let tool = journal::tool(
            ToolChoice::parse(&own("tool")),
            dialect,
            &journal,
            journal::Installed::default(),
        );
        IMPORTS.with(|i| {
            let mut i = i.borrow_mut();
            let e = i.entry(csv.clone()).or_insert(Import {
                journal: journal.clone(),
                tool,
                doc: None,
                count: 0,
                text: String::new(),
            });
            e.journal = journal;
            e.tool = tool;
        });
        dry_run(csv, Some(text), true);
    });
    Ok("null".into())
}

/// The dry run (Ledger's conversion), written in the Import document.
fn dry_run(csv: String, csv_text: Option<String>, open: bool) {
    let Some((journal, tool)) =
        IMPORTS.with(|i| i.borrow().get(&csv).map(|e| (e.journal.clone(), e.tool)))
    else {
        return;
    };
    let script = import::beangulp_script(&journal, &own("importer"));
    let (program, args) = match tool {
        Dialect::Beancount => ("python3", import::beangulp_command(&script, &csv, &journal)),
        Dialect::Ledger => (
            "ledger",
            import::ledger_command(
                &journal,
                &csv,
                &own("ledger_account"),
                &own("ledger_date_format"),
            ),
        ),
        _ => (
            "hledger",
            import::hledger_command(&journal, &csv, Some(&own("rules")), true),
        ),
    };
    let dir = journal::folder(&csv).to_string();
    run(program, args, &dir, move |r| match r {
        Ok(e) if e.status == Some(0) => {
            let (count, text) = import::drafted(tool, &String::from_utf8_lossy(&e.stdout));
            IMPORTS.with(|i| {
                if let Some(e) = i.borrow_mut().get_mut(&csv) {
                    e.count = count;
                    e.text = text;
                }
            });
            write(&csv, open);
        }
        Ok(e) => {
            let said = String::from_utf8_lossy(&e.stderr).into_owned();
            if tool == Dialect::Beancount {
                ui::notify(
                    &format!("python3: {}", import::python_said(&said, &script)),
                    ui::Level::Warning,
                );
            } else if let (Dialect::Hledger, Some(rules), Some(csv_text)) =
                (tool, import::missing_rules(&said), csv_text)
            {
                offer_rules(csv, rules, csv_text, journal);
            } else {
                let first = said
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("failed");
                ui::notify(&format!("{program}: {first}"), ui::Level::Warning);
            }
        }
        Err(NotRun::Missing) if tool == Dialect::Beancount => ui::notify(
            "python3 was not found: install Python with beangulp, or give its path (a virtual environment's) as the setting programs.python3",
            ui::Level::Warning,
        ),
        Err(NotRun::Missing) => ui::notify(&super::missing(tool), ui::Level::Warning),
        Err(NotRun::Refused(why)) => ui::notify(&format!("{program}: {why}"), ui::Level::Warning),
        Err(NotRun::TooLong) => {}
    });
}

/// A statement without rules: rules from a template, its bank account
/// picked among the journal's asset accounts, written as a new file.
fn offer_rules(csv: String, rules: String, csv_text: String, journal: String) {
    let name = journal::name(&rules).to_string();
    ui::confirm(
        &format!(
            "{} has no rules for hledger. Make {name} from its header row?",
            journal::name(&csv)
        ),
        move |yes| {
            if !yes {
                return;
            }
            let args = vec![
                "-f".into(),
                journal.clone(),
                "accounts".into(),
                "type:A".into(),
            ];
            run("hledger", args, journal::folder(&journal), move |r| {
                let accounts: Vec<String> = r
                    .ok()
                    .filter(|e| e.status == Some(0))
                    .map(|e| journal::files_listed(&e.stdout))
                    .unwrap_or_default();
                let items: Vec<ui::PickItem> = accounts
                    .iter()
                    .map(|a| ui::PickItem {
                        label: a.clone(),
                        detail: None,
                    })
                    .collect();
                ui::quick_pick(
                    &items,
                    &ui::PickOptions {
                        title: Some(format!("The bank's account of {}", journal::name(&csv))),
                        placeholder: None,
                        many: false,
                    },
                    move |picked| {
                        let account = picked
                            .first()
                            .and_then(|&i| accounts.get(i as usize))
                            .cloned()
                            .unwrap_or_else(|| "assets:bank".into());
                        let text = import::rules_template(journal::name(&csv), &csv_text, &account);
                        RULES.with(|r| *r.borrow_mut() = Some((rules.clone(), text)));
                        super::open_then(&rules, "pta.insertRules");
                    },
                );
            });
        },
    );
}

/// Writes the rules made from the template in their new document.
pub(super) fn insert_rules() -> Result<String, String> {
    let Some(path) = RULES.with(|r| r.borrow().as_ref().map(|(p, _)| p.clone())) else {
        return Ok("null".into());
    };
    let there = super::here_is(&path);
    if there == super::Here::Waiting {
        return Ok("null".into());
    }
    let Some((path, text)) = RULES.with(|r| r.borrow_mut().take()) else {
        return Ok("null".into());
    };
    if there == super::Here::Elsewhere || !editor::text(None).trim().is_empty() {
        ui::notify(
            &format!("{path} is not the new, empty rules file"),
            ui::Level::Warning,
        );
        return Ok("null".into());
    }
    editor::insert(Some(0), &text);
    editor::transact("Rules from the Template");
    ui::notify(
        "The rules: check the fields and the accounts, save, then Import again in the statement (SPC m i)",
        ui::Level::Info,
    );
    Ok("null".into())
}

/// Writes the Import document (opened when `open`).
fn write(csv: &str, open: bool) {
    let Some((doc, text, tool)) = IMPORTS.with(|i| {
        let i = i.borrow();
        let e = i.get(csv)?;
        let mut t = format!(
            "; Import: {} into {} by {}: {}\n",
            journal::name(csv),
            journal::name(&e.journal),
            e.tool.language(),
            match e.count {
                0 => "nothing new".to_string(),
                1 => "1 new transaction".to_string(),
                n => format!("{n} new transactions"),
            }
        );
        t.push_str(if e.tool == Dialect::Beancount {
            "; i import · e the importer · r again · q close\n\n"
        } else {
            "; i import · e the rules · r again · q close\n\n"
        });
        if e.count == 0 && e.tool == Dialect::Hledger {
            t.push_str("; hledger skips what it imported before (.latest.");
            t.push_str(journal::name(csv));
            t.push_str(")\n");
        }
        t.push_str(&e.text);
        Some((e.doc, t, e.tool))
    }) else {
        return;
    };
    if let Some(n) = doc
        && documents::set(n, &text, None).is_ok()
    {
        return;
    }
    IMPORTS.with(|i| {
        if let Some(e) = i.borrow_mut().get_mut(csv) {
            e.doc = None;
        }
    });
    if !open {
        return;
    }
    let spec = documents::Spec::new(DOC, csv, &format!("Import: {}", journal::name(csv)), KIND)
        .language(tool.language());
    match documents::open(&spec, &text, None) {
        Ok(n) => IMPORTS.with(|i| {
            if let Some(e) = i.borrow_mut().get_mut(csv) {
                e.doc = Some(n);
            }
        }),
        Err(why) => ui::notify(&why, ui::Level::Error),
    }
}

/// `i` in the Import document: the import, for real. hledger writes the
/// journal (and remembers the statement's last date), the journal then
/// opened at the first transaction imported; Ledger's conversion is
/// appended in the journal by the plugin.
pub(super) fn accept() -> Result<String, String> {
    let Some(csv) = here() else {
        return Ok("null".into());
    };
    let Some((journal, tool, count, text)) = IMPORTS.with(|i| {
        i.borrow()
            .get(&csv)
            .map(|e| (e.journal.clone(), e.tool, e.count, e.text.clone()))
    }) else {
        return Ok("null".into());
    };
    if count == 0 {
        ui::notify("Nothing new to import", ui::Level::Info);
        return Ok("null".into());
    }
    if tool != Dialect::Hledger {
        // Ledger's conversion and beangulp's entries: appended by the
        // plugin, one undo step.
        let title = format!("Import {}", journal::name(&csv));
        close_import(&csv);
        super::append(journal, text, title);
        return Ok("null".into());
    }
    let args = import::hledger_command(&journal, &csv, Some(&own("rules")), false);
    let dir = journal::folder(&csv).to_string();
    run("hledger", args, &dir, move |r| match r {
        Ok(e) if e.status == Some(0) => {
            let said = String::from_utf8_lossy(&e.stdout);
            ui::notify(said.lines().next().unwrap_or("imported"), ui::Level::Info);
            close_import(&csv);
            first_imported(journal, &text);
        }
        Ok(e) => {
            let said = String::from_utf8_lossy(&e.stderr);
            ui::notify(
                &format!(
                    "hledger: {}",
                    said.lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("failed")
                ),
                ui::Level::Warning,
            );
        }
        Err(_) => ui::notify(&super::missing(Dialect::Hledger), ui::Level::Warning),
    });
    Ok("null".into())
}

/// The journal at the first transaction just imported, `drafted` its
/// dry run: hledger's print of that transaction's day (not the whole
/// journal's, which may pass 16 MB), the transactions imported that day
/// the last of it.
fn first_imported(journal: String, drafted: &str) {
    let txs = crate::write::transactions(drafted, Dialect::Hledger);
    let Some(day) = txs.first().map(|t| t.date) else {
        return;
    };
    let that_day = txs.iter().filter(|t| t.date == day).count();
    let args = vec![
        "-f".into(),
        journal.clone(),
        "print".into(),
        "-O".into(),
        "json".into(),
        "-b".into(),
        day.written('-'),
        "-e".into(),
        day.plus(1).written('-'),
    ];
    run("hledger", args, journal::folder(&journal), move |r| {
        let Some(json) = r
            .ok()
            .filter(|e| e.status == Some(0))
            .map(|e| String::from_utf8_lossy(&e.stdout).into_owned())
        else {
            return;
        };
        let place =
            import::nth_last(&json, that_day).and_then(|n| crate::report::hledger_place(&json, n));
        if let Some((file, line)) = place {
            let args = serde_json::json!({ "path": file, "line": line });
            let _ = kalem::run("file.open", &args.to_string());
        }
    });
}

/// Opens the statement's rules (`SPC m e` in it, `e` in the Import
/// document); a missing file opens new, to be written on save.
pub(super) fn edit_rules() -> Result<String, String> {
    // In a Beancount import's document: the beangulp script.
    let importer = here().and_then(|csv| {
        IMPORTS.with(|i| {
            i.borrow()
                .get(&csv)
                .filter(|e| e.tool == Dialect::Beancount)
                .map(|e| import::beangulp_script(&e.journal, &own("importer")))
        })
    });
    let csv = here().or_else(|| editor::document().and_then(|d| d.path));
    let Some(csv) = csv else {
        return Ok("null".into());
    };
    let path = importer.unwrap_or_else(|| rules_of(&csv));
    let args = serde_json::json!({ "path": path });
    kalem::run("file.open", &args.to_string())?;
    Ok("null".into())
}

fn close_import(csv: &str) {
    if let Some(n) = IMPORTS.with(|i| i.borrow_mut().get_mut(csv).and_then(|e| e.doc.take())) {
        documents::close(n);
    }
}

/// Closes the Import document.
pub(super) fn close() -> Result<String, String> {
    if let Some(n) = documents::current() {
        documents::close(n);
        forget(n);
    }
    Ok("null".into())
}

/// Whether document `n` is one of these.
pub(super) fn has_doc(n: u64) -> bool {
    IMPORTS.with(|r| r.borrow().values().any(|e| e.doc == Some(n)))
}

/// Document `n` closed.
pub(super) fn forget(n: u64) {
    IMPORTS.with(|i| {
        for e in i.borrow_mut().values_mut() {
            if e.doc == Some(n) {
                e.doc = None;
            }
        }
    });
}
