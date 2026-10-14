//! The Reconcile document (PT8): an account against a statement's
//! closing balance. The tool's totals (cleared, pending, both) and its
//! postings not cleared (`crate::reconcile`), the difference to the
//! target the user typed, written again on the journal's save; the
//! journal stays where postings are marked (`SPC m t`). Finish clears
//! the pending postings and writes the target as a balance assertion,
//! in the journal, once the difference is zero.

use std::cell::RefCell;
use std::collections::BTreeMap;

use kalem_plugin::{documents, editor, ui};

use crate::Dialect;
use crate::journal::{self, Main};
use crate::reconcile::{self, Figures, Which};
use crate::report::{self, Posting};
use crate::statement::{self, Line, Paired};
use crate::write::Date;

use super::{NotRun, resolve, run, today};

const DOC: &str = "pta.reconcile";
pub(super) const KIND: &str = "pta-reconcile";

/// An account's reconciliation.
#[derive(Debug)]
struct Reconciliation {
    tool: Dialect,
    doc: Option<u64>,
    /// The journal it started in, which Finish edits.
    journal: String,
    target: String,
    figures: Figures,
    postings: Vec<Posting>,
    /// The statement beside (PT8c): its file, its lines, and how they
    /// pair with the account's postings.
    statement: Option<(String, Vec<Line>, Paired)>,
    shown: report::Document,
    run: u64,
}

/// A Finish asked for, waiting for the command that writes it in the
/// journal.
#[derive(Debug)]
struct Finish {
    journal: String,
    account: String,
    target: reconcile::Amount,
    date: Date,
}

thread_local! {
    /// By the main journal and the account.
    static RECONCILES: RefCell<BTreeMap<(String, String), Reconciliation>> = const { RefCell::new(BTreeMap::new()) };
    static FINISH: RefCell<Option<Finish>> = const { RefCell::new(None) };
}

/// The reconciliation whose document is the current one.
pub(super) fn here() -> Option<(String, String)> {
    let n = documents::current()?;
    RECONCILES.with(|r| {
        r.borrow()
            .iter()
            .find(|(_, e)| e.doc == Some(n))
            .map(|(k, _)| k.clone())
    })
}

/// Reconcile (`SPC m R`): in a journal, the account of the posting at
/// the cursor (else one picked), its target asked (the tool's total of
/// the cleared and pending postings offered); in the document, run
/// again.
pub(super) fn command() -> Result<String, String> {
    if let Some(key) = here() {
        gather(&key, true);
        return Ok("null".into());
    }
    let Some((d, Some(path))) = super::dialect_here() else {
        ui::notify("This document is no journal with a file", ui::Level::Info);
        return Ok("null".into());
    };
    let text = editor::text(None);
    let at = editor::selected().head as usize;
    let account = crate::write::account_at(&text, at, d);
    let journal = path.clone();
    resolve(path, d, move |tool, main| match account {
        Some(a) => ask_target(tool, main, journal, a),
        None => pick(tool, main, journal),
    });
    Ok("null".into())
}

/// The asset and liability accounts, to pick one.
fn pick(tool: Dialect, main: Main, journal: String) {
    let (program, args): (&'static str, Vec<String>) = match tool {
        Dialect::Hledger => (
            "hledger",
            vec![
                "-f".into(),
                main.path.clone(),
                "accounts".into(),
                "type:AL".into(),
            ],
        ),
        Dialect::Ledger => (
            "ledger",
            vec![
                "-f".into(),
                main.path.clone(),
                "accounts".into(),
                "^Assets".into(),
                "^Liabilities".into(),
            ],
        ),
        Dialect::Beancount => (
            "bean-query",
            vec![
                "-f".into(),
                "csv".into(),
                main.path.clone(),
                "SELECT DISTINCT account WHERE account ~ '^(Assets|Liabilities):' ORDER BY account"
                    .into(),
            ],
        ),
    };
    let dir = journal::folder(&main.path).to_string();
    run(program, args, &dir, move |r| {
        let listed = match r {
            Ok(e) if e.status == Some(0) => String::from_utf8_lossy(&e.stdout).into_owned(),
            _ => return ui::notify(&super::missing(tool), ui::Level::Warning),
        };
        let accounts: Vec<String> = listed
            .lines()
            .skip(usize::from(tool == Dialect::Beancount))
            .map(|l| l.trim().trim_matches('"').to_string())
            .filter(|l| !l.is_empty())
            .collect();
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
                title: Some("Account to reconcile".into()),
                placeholder: None,
                many: false,
            },
            move |picked| {
                if let Some(a) = picked.first().and_then(|&i| accounts.get(i as usize)) {
                    ask_target(tool, main, journal, a.clone());
                }
            },
        );
    });
}

/// The statement's closing balance, asked: the tool's total offered.
fn ask_target(tool: Dialect, main: Main, journal: String, account: String) {
    let (program, args) = reconcile::total_command(tool, &main.path, &account, Which::Both);
    let dir = journal::folder(&main.path).to_string();
    run(program, args, &dir, move |r| {
        let offered = match r {
            Ok(e) if e.status == Some(0) => {
                reconcile::total_of(tool, &String::from_utf8_lossy(&e.stdout))
                    .first()
                    .map(reconcile::Amount::written)
                    .unwrap_or_default()
            }
            Ok(e) => {
                let said = String::from_utf8_lossy(&e.stderr);
                return ui::notify(
                    &format!(
                        "{}: {}",
                        tool.checker(),
                        said.lines().next().unwrap_or("failed")
                    ),
                    ui::Level::Warning,
                );
            }
            Err(NotRun::Missing) => return ui::notify(&super::missing(tool), ui::Level::Warning),
            Err(NotRun::TooLong) => return,
            Err(NotRun::Refused(why)) => {
                return ui::notify(&format!("{}: {why}", tool.checker()), ui::Level::Warning);
            }
        };
        ui::prompt(
            &format!("Statement balance of {account}"),
            &ui::PromptOptions {
                value: Some(offered),
                placeholder: None,
                password: false,
            },
            move |answer| {
                let Some(target) = answer.filter(|t| !t.trim().is_empty()) else {
                    return;
                };
                let key = (main.path.clone(), account);
                RECONCILES.with(|r| {
                    let mut r = r.borrow_mut();
                    let e = r.entry(key.clone()).or_insert(Reconciliation {
                        tool,
                        doc: None,
                        journal: journal.clone(),
                        target: String::new(),
                        figures: Figures::default(),
                        postings: Vec::new(),
                        statement: None,
                        shown: report::Document::default(),
                        run: 0,
                    });
                    e.target = target.trim().to_string();
                    e.journal = journal;
                });
                gather(&key, true);
            },
        );
    });
}

/// After a journal of `main` is saved: its open reconciliations run
/// again.
pub(super) fn saved(main: &str) {
    let keys: Vec<(String, String)> = RECONCILES.with(|r| {
        r.borrow()
            .iter()
            .filter(|(k, e)| k.0 == main && e.doc.is_some())
            .map(|(k, _)| k.clone())
            .collect()
    });
    for k in keys {
        gather(&k, false);
    }
}

/// The tool's total of `which` postings of `key`'s account.
fn total(
    tool: Dialect,
    key: &(String, String),
    which: Which,
    done: impl FnOnce(Vec<reconcile::Amount>) + 'static,
) {
    let (p, a) = reconcile::total_command(tool, &key.0, &key.1, which);
    run(p, a, journal::folder(&key.0), move |r| {
        done(match r {
            Ok(e) if e.status == Some(0) => {
                reconcile::total_of(tool, &String::from_utf8_lossy(&e.stdout))
            }
            _ => Vec::new(),
        });
    });
}

/// The tool's totals and postings not cleared, one run after another,
/// then the document.
fn gather(key: &(String, String), open: bool) {
    let Some((tool, run_no)) = RECONCILES.with(|r| {
        let mut r = r.borrow_mut();
        let e = r.get_mut(key)?;
        e.run += 1;
        Some((e.tool, e.run))
    }) else {
        return;
    };
    let k = key.clone();
    total(tool, key, Which::Cleared, move |cleared| {
        let k2 = k.clone();
        total(tool, &k, Which::Pending, move |pending| {
            let k3 = k2.clone();
            total(tool, &k2, Which::Both, move |both| {
                let (p, a) = reconcile::open_command(tool, &k3.0, &k3.1);
                let dir = journal::folder(&k3.0).to_string();
                run(p, a, &dir, move |r| {
                    let postings = match r {
                        Ok(e) if e.status == Some(0) => {
                            reconcile::open_of(tool, &String::from_utf8_lossy(&e.stdout))
                        }
                        _ => Vec::new(),
                    };
                    let current =
                        RECONCILES.with(|m| m.borrow().get(&k3).map(|e| e.run)) == Some(run_no);
                    if !current {
                        return;
                    }
                    RECONCILES.with(|m| {
                        if let Some(e) = m.borrow_mut().get_mut(&k3) {
                            let total = reconcile::matching(&e.target, &both).cloned();
                            e.figures = Figures {
                                cleared: cleared.iter().map(reconcile::Amount::written).collect(),
                                pending: pending.iter().map(reconcile::Amount::written).collect(),
                                difference: total
                                    .as_ref()
                                    .and_then(|t| reconcile::difference(&e.target, t)),
                                total,
                                target: e.target.clone(),
                            };
                            e.postings = postings;
                        }
                    });
                    write(&k3, open);
                    pair(&k3);
                });
            });
        });
    });
}

/// The days a statement's line and its posting may be apart: the
/// setting `match_days`, 3.
fn match_days() -> i64 {
    kalem_plugin::settings::own("match_days")
        .and_then(|v| serde_json::from_str::<i64>(&v).ok())
        .unwrap_or(3)
        .clamp(0, 31)
}

/// Pair with the Statement (`SPC m r` in a CSV statement): its lines
/// against the postings of the reconciliation open (one picked where
/// several are), shown in its document and kept until another statement
/// is paired, paired again whenever the reconciliation is written again.
pub(super) fn with_statement() -> Result<String, String> {
    let Some(path) = editor::document().and_then(|d| d.path) else {
        return Ok("null".into());
    };
    let lines = match statement::lines(&editor::text(None)) {
        Ok(l) => l,
        Err(why) => {
            ui::notify(&why, ui::Level::Warning);
            return Ok("null".into());
        }
    };
    let open: Vec<(String, String)> = RECONCILES.with(|r| {
        r.borrow()
            .iter()
            .filter(|(_, e)| e.doc.is_some())
            .map(|(k, _)| k.clone())
            .collect()
    });
    let take = move |key: (String, String), path: String, lines: Vec<Line>| {
        RECONCILES.with(|r| {
            if let Some(e) = r.borrow_mut().get_mut(&key) {
                e.statement = Some((path, lines, Paired::default()));
            }
        });
        pair(&key);
    };
    match open.len() {
        0 => ui::notify(
            "No reconciliation open: Reconcile in the journal first (SPC m R)",
            ui::Level::Info,
        ),
        1 => take(open[0].clone(), path, lines),
        _ => {
            let items: Vec<ui::PickItem> = open
                .iter()
                .map(|(m, a)| ui::PickItem {
                    label: a.clone(),
                    detail: Some(journal::name(m).to_string()),
                })
                .collect();
            ui::quick_pick(
                &items,
                &ui::PickOptions {
                    title: Some("The reconciliation this statement is for".into()),
                    placeholder: None,
                    many: false,
                },
                move |picked| {
                    if let Some(k) = picked.first().and_then(|&i| open.get(i as usize)) {
                        take(k.clone(), path, lines);
                    }
                },
            );
        }
    }
    Ok("null".into())
}

/// Pairs `key`'s statement, when it has one, with the account's postings
/// in the statement's days (the tool's register), then writes the
/// document again.
fn pair(key: &(String, String)) {
    let Some((tool, lines)) = RECONCILES.with(|r| {
        let r = r.borrow();
        let e = r.get(key)?;
        Some((e.tool, e.statement.as_ref()?.1.clone()))
    }) else {
        return;
    };
    let days = match_days();
    let Some(span) = statement::span(&lines, days) else {
        return;
    };
    let (program, args) = report::register_command(tool, &key.0, &key.1, Some(span));
    let dir = journal::folder(&key.0).to_string();
    let key = key.clone();
    run(program, args, &dir, move |r| {
        let Ok(e) = r else { return };
        if e.status != Some(0) {
            return;
        }
        let out = String::from_utf8_lossy(&e.stdout);
        let rows = match tool {
            Dialect::Hledger => reconcile::hledger_register(&out),
            Dialect::Ledger => report::ledger_register(&out),
            Dialect::Beancount => report::beancount_register(&out),
        };
        let paired = statement::pair(&lines, &rows, days);
        let said = format!(
            "{} of the statement's {} lines found in the journal{}",
            lines.len() - paired.alone.len(),
            lines.len(),
            if paired.flipped {
                ", its amounts the other way round"
            } else {
                ""
            }
        );
        let changed = RECONCILES.with(|m| {
            let mut m = m.borrow_mut();
            let Some(e) = m.get_mut(&key) else {
                return false;
            };
            let Some(s) = e.statement.as_mut() else {
                return false;
            };
            let changed = s.2 != paired;
            s.2 = paired;
            changed
        });
        if changed {
            ui::notify(&said, ui::Level::Info);
            write(&key, false);
        }
    });
}

/// Writes the Reconcile document (opened when `open`).
fn write(key: &(String, String), open: bool) {
    let Some((doc, text, styles)) = RECONCILES.with(|r| {
        let mut r = r.borrow_mut();
        let e = r.get_mut(key)?;
        let header = format!("Reconcile: {} · {} · {}", key.1, journal::name(&key.0), e.tool.language());
        let hint = "Enter the posting · t target · C finish · r again · q close; mark postings in the journal (SPC m t), then save";
        e.shown = reconcile::layout(
            &header,
            hint,
            &e.figures,
            &e.postings,
            e.statement.as_ref().map(|(_, _, p)| p),
        );
        Some((e.doc, e.shown.text.clone(), super::balance::styles(&e.shown)))
    }) else {
        return;
    };
    if let Some(n) = doc
        && documents::set_styled(n, &text, None, &styles).is_ok()
    {
        return;
    }
    RECONCILES.with(|r| {
        if let Some(e) = r.borrow_mut().get_mut(key) {
            e.doc = None;
        }
    });
    if !open {
        return;
    }
    let spec = documents::Spec::new(
        DOC,
        &format!("{}#{}", key.0, key.1),
        &format!("Reconcile: {}", key.1),
        KIND,
    );
    match documents::open_styled(&spec, &text, None, &styles) {
        Ok(n) => {
            RECONCILES.with(|r| {
                if let Some(e) = r.borrow_mut().get_mut(key) {
                    e.doc = Some(n);
                }
            });
            super::owned(n, &key.0);
        }
        Err(why) => ui::notify(&why, ui::Level::Error),
    }
}

/// Enter in the document: the journal at the posting.
pub(super) fn visit() -> Result<String, String> {
    let Some(key) = here() else {
        return Ok("null".into());
    };
    let at = editor::selected().head as usize;
    let found = RECONCILES.with(|r| {
        let r = r.borrow();
        let e = r.get(&key)?;
        let n = e.shown.text[..at.min(e.shown.text.len())]
            .matches('\n')
            .count();
        let i: usize = e.shown.lines.get(n)?.as_deref()?.parse().ok()?;
        Some((e.tool, e.postings.get(i)?.clone()))
    });
    if let Some((tool, posting)) = found {
        super::register::go_to(tool, &key.0, posting);
    }
    Ok("null".into())
}

/// The target, asked again.
pub(super) fn retarget() -> Result<String, String> {
    let Some(key) = here() else {
        return Ok("null".into());
    };
    let now = RECONCILES
        .with(|r| r.borrow().get(&key).map(|e| e.target.clone()))
        .unwrap_or_default();
    ui::prompt(
        &format!("Statement balance of {}", key.1),
        &ui::PromptOptions {
            value: Some(now),
            placeholder: None,
            password: false,
        },
        move |answer| {
            if let Some(t) = answer.filter(|t| !t.trim().is_empty()) {
                RECONCILES.with(|r| {
                    if let Some(e) = r.borrow_mut().get_mut(&key) {
                        e.target = t.trim().to_string();
                    }
                });
                gather(&key, false);
            }
        },
    );
    Ok("null".into())
}

/// Finish (`SPC m C`, `C` in the document): with the difference zero,
/// the statement's date asked (today offered), then, in the journal,
/// its pending postings of the account cleared and the target written
/// as a balance assertion; else the difference said.
pub(super) fn finish() -> Result<String, String> {
    let key = here().or_else(|| {
        let (_, path) = super::dialect_here()?;
        let path = path?;
        RECONCILES.with(|r| {
            r.borrow()
                .iter()
                .find(|(_, e)| journal::same_file(&e.journal, &path))
                .map(|(k, _)| k.clone())
        })
    });
    let Some(key) = key else {
        ui::notify(
            "No reconciliation of this journal: Reconcile first (SPC m R)",
            ui::Level::Info,
        );
        return Ok("null".into());
    };
    let Some((journal, target, difference)) = RECONCILES.with(|r| {
        let r = r.borrow();
        let e = r.get(&key)?;
        let total = e.figures.total.as_ref()?;
        Some((
            e.journal.clone(),
            reconcile::target_amount(&e.target, total),
            e.figures.difference.clone(),
        ))
    }) else {
        ui::notify("The reconciliation has no total yet", ui::Level::Info);
        return Ok("null".into());
    };
    let (Some(target), Some(difference)) = (target, difference) else {
        ui::notify("The target does not read as an amount", ui::Level::Warning);
        return Ok("null".into());
    };
    if !difference.quantity.is_zero() {
        ui::notify(
            &format!("Not finished: the difference is {}", difference.written()),
            ui::Level::Warning,
        );
        return Ok("null".into());
    }
    let today = today();
    ui::prompt(
        "Date of the statement",
        &ui::PromptOptions {
            value: Some(today.written('-')),
            placeholder: Some("the balance assertion's date".into()),
            password: false,
        },
        move |answer| {
            let Some(typed) = answer else { return };
            let Some(date) = crate::write::smart_date(&typed, today) else {
                return ui::notify(&format!("Not a date: {typed}"), ui::Level::Warning);
            };
            FINISH.with(|f| {
                *f.borrow_mut() = Some(Finish {
                    journal: journal.clone(),
                    account: key.1.clone(),
                    target,
                    date,
                })
            });
            // The journal first, then the edits in it.
            super::open_then(&journal, "pta.finishReconcileWrite");
        },
    );
    Ok("null".into())
}

/// Writes the Finish asked for, in its journal: one undo step.
pub(super) fn finish_write() -> Result<String, String> {
    let Some(journal) = FINISH.with(|f| f.borrow().as_ref().map(|f| f.journal.clone())) else {
        return Ok("null".into());
    };
    match super::here_is(&journal) {
        super::Here::There => {}
        super::Here::Waiting => return Ok("null".into()),
        super::Here::Elsewhere => {
            FINISH.with(|f| f.borrow_mut().take());
            ui::notify(
                "The journal is no longer the current document",
                ui::Level::Warning,
            );
            return Ok("null".into());
        }
    }
    let Some(f) = FINISH.with(|f| f.borrow_mut().take()) else {
        return Ok("null".into());
    };
    let Some((d, Some(_))) = super::dialect_here() else {
        return Ok("null".into());
    };
    let text = editor::text(None);
    let column = crate::align::Settings::read(kalem_plugin::settings::own).column;
    let edits = reconcile::finish(&text, d, &f.account, &f.target, f.date, column);
    let cleared = edits.len().saturating_sub(1);
    super::edit(&edits, "Finish Reconciliation");
    ui::notify(
        &format!(
            "Reconciled {} at {}: {cleared} marks cleared, the balance assertion written; save to check",
            f.account,
            f.target.written()
        ),
        ui::Level::Info,
    );
    Ok("null".into())
}

/// Closes the document.
pub(super) fn close() -> Result<String, String> {
    if let Some(n) = documents::current() {
        documents::close(n);
        forget(n);
    }
    Ok("null".into())
}

/// Whether document `n` is one of these.
pub(super) fn has_doc(n: u64) -> bool {
    RECONCILES.with(|r| r.borrow().values().any(|e| e.doc == Some(n)))
}

/// Document `n` closed.
pub(super) fn forget(n: u64) {
    RECONCILES.with(|r| {
        for e in r.borrow_mut().values_mut() {
            if e.doc == Some(n) {
                e.doc = None;
            }
        }
    });
}
