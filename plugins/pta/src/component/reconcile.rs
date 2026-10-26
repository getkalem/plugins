//! The Reconcile document (PT8): an account against a statement's
//! closing balance. The tool's totals (cleared, pending, both) and its
//! postings not cleared (`crate::reconcile`), the difference to the
//! target the user typed, written again on the journal's save; the
//! journal stays where postings are marked (`SPC m t`). Finish clears
//! the pending postings and writes the target as a balance assertion
//! where it holds at the statement's date, in the journal, once the
//! difference is zero.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use kalem_plugin::{documents, editor, kalem, ui};

use crate::Dialect;
use crate::journal::{self, Main};
use crate::reconcile::{self, Figures, Which};
use crate::report::{self, Posting};
use crate::statement::{self, Line, Paired};
use crate::write::Date;

use super::{NotRun, resolve, run, run_input, today};

const DOC: &str = "pta.reconcile";
pub(super) const KIND: &str = "pta-reconcile";

/// A commodity's symbol (empty for an account of one commodity) and the
/// statement's balance typed for it.
type Target = (String, String);

/// An account's reconciliation.
#[derive(Debug)]
struct Reconciliation {
    tool: Dialect,
    doc: Option<u64>,
    /// The journal it started in, which Finish edits.
    journal: String,
    /// The statement's balances: a commodity's symbol (empty for an
    /// account of one commodity) and the balance typed.
    targets: Vec<(String, String)>,
    /// A block of figures a commodity.
    figures: Vec<Figures>,
    postings: Vec<Posting>,
    /// The statement beside (PT8c): its file, its lines, and how they
    /// pair with the account's postings.
    statement: Option<(String, Vec<Line>, Paired)>,
    /// The line the cursor goes to when the document is shown again.
    cursor_line: Option<usize>,
    shown: report::Document,
    run: u64,
    /// The journal's text as typed, unsaved, the figures were read from
    /// (`reconcile_live`); none when they are of the file as saved.
    live: Option<Rc<Vec<u8>>>,
    /// The text as typed did not read: the figures are of the last that
    /// did.
    unreadable: bool,
}

/// A Finish asked for, waiting for the command that writes it in the
/// journal.
#[derive(Debug)]
struct Finish {
    journal: String,
    /// The main journal the tool reads, and the tool.
    main: String,
    tool: Dialect,
    /// The journal's text the tool read, the accounts it read there
    /// (`reconcile::accounts_in`) and what a balance assertion at the
    /// date would assert (`reconcile::asserted`; none where the tool did
    /// not say), once it has answered.
    read: Option<(String, reconcile::Accounts, Option<Asserted>)>,
    account: String,
    targets: Vec<reconcile::Amount>,
    date: Date,
}

/// The amounts a balance assertion asserts, or the target the balance
/// at the statement's date is not and that balance.
type Asserted = Result<Vec<reconcile::Amount>, Box<(reconcile::Amount, reconcile::Amount)>>;

/// A posting to mark pending from the Reconcile document, waiting for
/// its journal to be the current document.
#[derive(Debug)]
struct Mark {
    file: String,
    line: u32,
    amount: String,
    key: (String, String),
    /// The Reconcile document's line it was on, for the cursor after.
    at_line: usize,
}

thread_local! {
    static MARK: RefCell<Option<Mark>> = const { RefCell::new(None) };
    /// The reconciliation to show again once its journal is saved, with
    /// the line for the cursor.
    static RETURN: RefCell<Option<((String, String), usize)>> = const { RefCell::new(None) };
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
    let line = text[..at.min(text.len())].matches('\n').count() as u32 + 1;
    let journal = path.clone();
    resolve(path, d, move |tool, main| match account {
        Some(a) => account_read(tool, main, journal, text, line, a),
        None => pick(tool, main, journal),
    });
    Ok("null".into())
}

/// The account of the posting at `line` of `journal` (its text `text`)
/// as the tool reads it, an alias or `apply account` resolved
/// (`reconcile::accounts_in`), else as written; then its target asked.
fn account_read(
    tool: Dialect,
    main: Main,
    journal: String,
    text: String,
    line: u32,
    written: String,
) {
    let Some((program, mut args)) = reconcile::accounts_command(tool, &main.path) else {
        return ask_target(tool, main, journal, written);
    };
    // The main journal's text as typed, on the standard input.
    let live = journal::same_file(&journal, &main.path);
    if live {
        args = reconcile::from_input(args, &main.path);
    }
    let file = if live {
        "-".to_string()
    } else {
        journal.clone()
    };
    let input = live.then(|| text.clone().into_bytes());
    let dir = journal::folder(&main.path).to_string();
    run_input(program, args, &dir, input, move |r| {
        let read = match r {
            Ok(e) if e.status == Some(0) => {
                let out = String::from_utf8_lossy(&e.stdout);
                reconcile::accounts_in(tool, &out, &file, &text).remove(&line)
            }
            _ => None,
        };
        ask_target(tool, main, journal, read.unwrap_or(written));
    });
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

/// The statement's closing balance asked, a commodity at a time for an
/// account of several: the tool's totals offered.
fn ask_target(tool: Dialect, main: Main, journal: String, account: String) {
    let (program, args) = reconcile::total_command(tool, &main.path, &account, Which::Both);
    let dir = journal::folder(&main.path).to_string();
    run(program, args, &dir, move |r| {
        let totals = match r {
            Ok(e) if e.status == Some(0) => {
                reconcile::total_of(tool, &String::from_utf8_lossy(&e.stdout))
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
        let offered: Vec<(String, String)> = if totals.len() <= 1 {
            vec![(
                String::new(),
                totals
                    .first()
                    .map(reconcile::Amount::written)
                    .unwrap_or_default(),
            )]
        } else {
            totals
                .iter()
                .map(|a| (a.style.symbol.clone(), a.written()))
                .collect()
        };
        let who = account.clone();
        ask_each(
            who,
            offered.into(),
            Vec::new(),
            Box::new(move |targets| {
                let key = (main.path.clone(), account);
                RECONCILES.with(|r| {
                    let mut r = r.borrow_mut();
                    let e = r.entry(key.clone()).or_insert(Reconciliation {
                        tool,
                        doc: None,
                        journal: journal.clone(),
                        targets: Vec::new(),
                        figures: Vec::new(),
                        postings: Vec::new(),
                        statement: None,
                        cursor_line: None,
                        shown: report::Document::default(),
                        run: 0,
                        live: None,
                        unreadable: false,
                    });
                    e.targets = targets;
                    e.journal = journal;
                });
                // Beside the journal, as ledger-mode's reconcile window:
                // a pane of its own, unless it is open already.
                let shown =
                    RECONCILES.with(|r| r.borrow().get(&key).is_some_and(|e| e.doc.is_some()));
                if !shown && beside() {
                    let _ = kalem::run("pane.splitRight", "{}");
                }
                gather(&key, true);
            }),
        );
    });
}

/// The statement's balances asked one after another, each offered as
/// `rest` has it: a commodity's symbol (empty for an account of one) and
/// the balance; `done` with them all, and nothing when one is not given.
fn ask_each(
    account: String,
    mut rest: std::collections::VecDeque<Target>,
    mut got: Vec<Target>,
    done: Box<dyn FnOnce(Vec<Target>)>,
) {
    let Some((commodity, offered)) = rest.pop_front() else {
        return done(got);
    };
    let title = if commodity.is_empty() {
        format!("Statement balance of {account}")
    } else {
        format!(
            "Statement balance of {account} in {commodity} ({} of {})",
            got.len() + 1,
            got.len() + 1 + rest.len()
        )
    };
    ui::prompt(
        &title,
        &ui::PromptOptions {
            value: Some(offered),
            placeholder: None,
            password: false,
        },
        move |answer| {
            let Some(t) = answer.filter(|t| !t.trim().is_empty()) else {
                return;
            };
            got.push((commodity, t.trim().to_string()));
            ask_each(account, rest, got, done);
        },
    );
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
    let back = RETURN.with(|r| {
        let mut r = r.borrow_mut();
        if r.as_ref().is_some_and(|(k, _)| k.0 == main) {
            r.take()
        } else {
            None
        }
    });
    for k in keys {
        if back.as_ref().is_some_and(|(b, _)| *b == k) {
            continue;
        }
        gather(&k, false);
    }
    // A posting just marked from it: the Reconcile document shown again,
    // its cursor on the line it was on.
    if let Some((k, line)) = back {
        RECONCILES.with(|r| {
            if let Some(e) = r.borrow_mut().get_mut(&k) {
                e.cursor_line = Some(line);
            }
        });
        gather(&k, true);
    }
}

/// The tool's total of `which` postings of `key`'s account, read from
/// `input` when given (the journal as typed); none when the tool failed.
fn total(
    tool: Dialect,
    key: &(String, String),
    which: Which,
    input: Option<Rc<Vec<u8>>>,
    done: impl FnOnce(Option<Vec<reconcile::Amount>>) + 'static,
) {
    let (p, mut a) = reconcile::total_command(tool, &key.0, &key.1, which);
    if input.is_some() {
        a = reconcile::from_input(a, &key.0);
    }
    run_input(
        p,
        a,
        journal::folder(&key.0),
        input.map(|t| t.to_vec()),
        move |r| {
            done(match r {
                Ok(e) if e.status == Some(0) => Some(reconcile::total_of(
                    tool,
                    &String::from_utf8_lossy(&e.stdout),
                )),
                _ => None,
            });
        },
    );
}

/// The tool's totals and postings not cleared, one run after another,
/// then the document.
fn gather(key: &(String, String), open: bool) {
    gather_from(key, open, None);
}

/// [`gather`], the journal read from `input` when given (its text as
/// typed): where the tool cannot read it, the figures shown stay, said
/// to be of the last text that read.
fn gather_from(key: &(String, String), open: bool, input: Option<Rc<Vec<u8>>>) {
    let Some((tool, run_no)) = RECONCILES.with(|r| {
        let mut r = r.borrow_mut();
        let e = r.get_mut(key)?;
        e.run += 1;
        Some((e.tool, e.run))
    }) else {
        return;
    };
    let k = key.clone();
    let i1 = input.clone();
    total(tool, key, Which::Cleared, input.clone(), move |cleared| {
        let k2 = k.clone();
        let i2 = i1.clone();
        total(tool, &k, Which::Pending, i1, move |pending| {
            let k3 = k2.clone();
            let i3 = i2.clone();
            total(tool, &k2, Which::Both, i2, move |both| {
                let k4 = k3.clone();
                open_postings(tool, &k4, i3.clone(), move |postings| {
                    let current =
                        RECONCILES.with(|m| m.borrow().get(&k3).map(|e| e.run)) == Some(run_no);
                    if !current {
                        return;
                    }
                    let read = cleared.is_some()
                        && pending.is_some()
                        && both.is_some()
                        && postings.is_some();
                    RECONCILES.with(|m| {
                        if let Some(e) = m.borrow_mut().get_mut(&k3) {
                            if i3.is_some() && !read {
                                // Half typed: the last figures stay.
                                e.unreadable = true;
                                return;
                            }
                            e.figures = reconcile::blocks(
                                &e.targets,
                                &cleared.unwrap_or_default(),
                                &pending.unwrap_or_default(),
                                &both.unwrap_or_default(),
                            );
                            e.postings = postings.unwrap_or_default();
                            e.live = i3.clone();
                            e.unreadable = false;
                        }
                    });
                    write(&k3, open);
                    pair(&k3);
                });
            });
        });
    });
}

/// The postings of `key`'s account not cleared, read from `input` when
/// given; none when the tool failed. hledger's take their transactions'
/// statuses (`reconcile::with_statuses`): its register gives a posting's
/// own mark only, so a posting under a pending transaction read as
/// unmarked.
fn open_postings(
    tool: Dialect,
    key: &(String, String),
    input: Option<Rc<Vec<u8>>>,
    done: impl FnOnce(Option<Vec<Posting>>) + 'static,
) {
    let (p, mut a) = reconcile::open_command(tool, &key.0, &key.1);
    if input.is_some() {
        a = reconcile::from_input(a, &key.0);
    }
    let statuses = reconcile::statuses_command(&a);
    let dir = journal::folder(&key.0).to_string();
    let there = dir.clone();
    let main = key.0.clone();
    let bytes = input.as_ref().map(|t| t.to_vec());
    run_input(p, a, &dir, bytes.clone(), move |r| {
        let mut list = match r {
            Ok(e) if e.status == Some(0) => {
                reconcile::open_of(tool, &String::from_utf8_lossy(&e.stdout))
            }
            _ => return done(None),
        };
        if bytes.is_some() {
            reconcile::placed_in(&mut list, &main);
        }
        if tool != Dialect::Hledger || list.iter().all(|p| p.status != ' ') {
            return done(Some(list));
        }
        run_input("hledger", statuses, &there, bytes, move |r| {
            if let Ok(e) = r
                && e.status == Some(0)
            {
                let statuses = reconcile::hledger_statuses(&String::from_utf8_lossy(&e.stdout));
                reconcile::with_statuses(&mut list, &statuses);
            }
            done(Some(list));
        });
    });
}

/// Whether the Reconcile documents follow their journal as typed: the
/// setting `reconcile_live`, on.
fn follows() -> bool {
    kalem_plugin::settings::own("reconcile_live").as_deref() != Some("false")
}

/// Whether the file at `path` is the main journal of an open
/// reconciliation that follows it as typed: hledger's and Ledger's, which
/// read the standard input.
pub(super) fn follows_file(path: &str) -> bool {
    follows()
        && RECONCILES.with(|r| {
            r.borrow().iter().any(|(k, e)| {
                e.doc.is_some() && e.tool != Dialect::Beancount && journal::same_file(&k.0, path)
            })
        })
}

/// The journal changed (`pta.reconcileLive`, queued on its change): its
/// open reconciliations run again on its text as typed, when it is the
/// current document and their main journal.
pub(super) fn live() -> Result<String, String> {
    let Some((_, Some(path))) = super::dialect_here() else {
        return Ok("null".into());
    };
    if !follows_file(&path) {
        return Ok("null".into());
    }
    let keys: Vec<(String, String)> = RECONCILES.with(|r| {
        r.borrow()
            .iter()
            .filter(|(k, e)| {
                e.doc.is_some() && e.tool != Dialect::Beancount && journal::same_file(&k.0, &path)
            })
            .map(|(k, _)| k.clone())
            .collect()
    });
    let text = Rc::new(editor::text(None).into_bytes());
    for k in keys {
        let same = RECONCILES.with(|r| {
            r.borrow()
                .get(&k)
                .and_then(|e| e.live.clone())
                .is_some_and(|l| *l == *text)
        });
        if !same {
            gather_from(&k, false, Some(text.clone()));
            continue;
        }
        // Back to the text the figures are of (an undo): they hold again.
        let was = RECONCILES.with(|r| {
            r.borrow_mut()
                .get_mut(&k)
                .map(|e| std::mem::replace(&mut e.unreadable, false))
        });
        if was == Some(true) {
            write(&k, false);
        }
    }
    Ok("null".into())
}

/// Whether the Reconcile document opens in a pane beside the journal:
/// the setting `reconcile_beside`, on.
fn beside() -> bool {
    kalem_plugin::settings::own("reconcile_beside").as_deref() != Some("false")
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
    let Some((doc, text, styles, cursor)) = RECONCILES.with(|r| {
        let mut r = r.borrow_mut();
        let e = r.get_mut(key)?;
        let header = format!(
            "Reconcile: {} · {} · {}",
            key.1,
            journal::name(&key.0),
            e.tool.language()
        );
        let hint = "Tab mark pending · Enter the posting · t target · C finish · r again · q close";
        let note = e.unreadable.then_some(
            "As typed, the journal does not read: the figures are of the last text that did",
        );
        e.shown = reconcile::layout(
            &header,
            hint,
            note,
            &e.figures,
            &e.postings,
            e.statement.as_ref().map(|(_, _, p)| p),
        );
        let cursor = e.cursor_line.take().map(|n| {
            e.shown
                .text
                .split_inclusive('\n')
                .take(n)
                .map(str::len)
                .sum::<usize>() as u64
        });
        Some((
            e.doc,
            e.shown.text.clone(),
            super::balance::styles(&e.shown),
            cursor,
        ))
    }) else {
        return;
    };
    // Asked to show it (opened, or a posting marked from it): opened
    // again, which shows it; else written where it is.
    if !open
        && let Some(n) = doc
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
    match documents::open_styled(&spec, &text, cursor, &styles) {
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

/// Tab in the Reconcile document (ledger-mode's reconcile toggle): the
/// posting on the cursor's line marked pending in its journal, or
/// unmarked; the journal saved, the totals run again, and the document
/// shown again. A plugin edits only the document its command runs in,
/// so the journal is opened for it.
pub(super) fn mark() -> Result<String, String> {
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
        Some((e.tool, e.postings.get(i)?.clone(), n, e.live.clone()))
    });
    let Some((tool, posting, at_line, input)) = found else {
        ui::notify("No posting on this line", ui::Level::Info);
        return Ok("null".into());
    };
    let amount = posting.amount.clone();
    let k = key.clone();
    super::register::place_of(tool, &key.0, posting, input, move |place| {
        let Some((file, line)) = place else {
            return ui::notify(
                "The tool did not say where the posting is",
                ui::Level::Warning,
            );
        };
        MARK.with(|m| {
            *m.borrow_mut() = Some(Mark {
                file: file.clone(),
                line,
                amount,
                key: k,
                at_line,
            })
        });
        super::open_then(&file, "pta.reconcileMarkWrite");
    });
    Ok("null".into())
}

/// Marks the posting [`mark`] keeps, in its journal once that is the
/// current document: the account's posting of the transaction at its
/// line (the one with its amount where there are several), one undo
/// step; then the journal saved.
pub(super) fn mark_write() -> Result<String, String> {
    let Some(file) = MARK.with(|m| m.borrow().as_ref().map(|m| m.file.clone())) else {
        return Ok("null".into());
    };
    match super::here_is(&file) {
        super::Here::There => {}
        super::Here::Waiting => return Ok("null".into()),
        super::Here::Elsewhere => {
            MARK.with(|m| m.borrow_mut().take());
            return Ok("null".into());
        }
    }
    let Some(m) = MARK.with(|m| m.borrow_mut().take()) else {
        return Ok("null".into());
    };
    let Some((d, _)) = super::dialect_here() else {
        return Ok("null".into());
    };
    let text = editor::text(None);
    let Some(at) = posting_line(&text, d, m.line, &m.key.1, &m.amount) else {
        ui::notify(
            &format!("No posting of {} found there", m.key.1),
            ui::Level::Warning,
        );
        return Ok("null".into());
    };
    match crate::write::toggle_pending(&text, d, at) {
        Some(edits) if !edits.is_empty() => {
            super::edit(&edits, "Mark Pending");
            RETURN.with(|r| *r.borrow_mut() = Some((m.key, m.at_line)));
            if let Err(why) = kalem::run("app.save", "{}") {
                ui::notify(&why, ui::Level::Warning);
            }
        }
        _ => ui::notify("The posting is cleared already", ui::Level::Info),
    }
    Ok("null".into())
}

/// The start of the posting of `account` (or under it) in the
/// transaction holding line `line` (from 1) of `text`: the one on that
/// line, else the one written with `amount`, else the first.
fn posting_line(text: &str, d: Dialect, line: u32, account: &str, amount: &str) -> Option<usize> {
    let start: usize = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1) as usize)
        .map(str::len)
        .sum();
    let txs = crate::write::transactions(text, d);
    let t =
        crate::write::at(&txs, start).or_else(|| txs.iter().find(|t| t.range.start >= start))?;
    let squeeze = |s: &str| s.split_whitespace().collect::<String>();
    let mut candidates = Vec::new();
    let mut offset = t.range.start;
    for l in text[t.range.clone()].split_inclusive('\n') {
        if offset > t.range.start
            && crate::write::account_at(text, offset, d)
                .is_some_and(|a| a == account || a.starts_with(&format!("{account}:")))
        {
            candidates.push((offset, l));
        }
        offset += l.len();
    }
    candidates
        .iter()
        .find(|(o, _)| *o == start)
        .or_else(|| {
            candidates
                .iter()
                .find(|(_, l)| !amount.is_empty() && squeeze(l).contains(&squeeze(amount)))
        })
        .or(candidates.first())
        .map(|(o, _)| *o)
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
        Some((e.tool, e.postings.get(i)?.clone(), e.live.clone()))
    });
    if let Some((tool, posting, input)) = found {
        super::register::go_to_in(tool, &key.0, posting, input);
    }
    Ok("null".into())
}

/// The targets, asked again: each commodity's balance in turn.
pub(super) fn retarget() -> Result<String, String> {
    let Some(key) = here() else {
        return Ok("null".into());
    };
    let now = RECONCILES
        .with(|r| r.borrow().get(&key).map(|e| e.targets.clone()))
        .unwrap_or_default();
    let account = key.1.clone();
    ask_each(
        account,
        now.into(),
        Vec::new(),
        Box::new(move |targets| {
            RECONCILES.with(|r| {
                if let Some(e) = r.borrow_mut().get_mut(&key) {
                    e.targets = targets;
                }
            });
            gather(&key, false);
        }),
    );
    Ok("null".into())
}

/// Finish (`SPC m C`, `C` in the document): with the difference zero,
/// the statement's date asked (today offered), then, in the journal,
/// its pending postings of the account cleared and the target written
/// as a balance assertion where it holds at that date; else the
/// difference said.
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
    let Some((journal, blocks, tool)) = RECONCILES.with(|r| {
        let r = r.borrow();
        let e = r.get(&key)?;
        Some((e.journal.clone(), e.figures.clone(), e.tool))
    }) else {
        ui::notify("The reconciliation has no total yet", ui::Level::Info);
        return Ok("null".into());
    };
    // Every commodity's difference zero: each target asserted.
    let mut targets = Vec::new();
    for f in &blocks {
        let Some(total) = f.total.as_ref() else {
            ui::notify("The reconciliation has no total yet", ui::Level::Info);
            return Ok("null".into());
        };
        let (Some(target), Some(difference)) = (
            reconcile::target_amount(&f.target, total),
            f.difference.as_ref(),
        ) else {
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
        targets.push(target);
    }
    if targets.is_empty() {
        ui::notify("The reconciliation has no total yet", ui::Level::Info);
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
                    main: key.0.clone(),
                    tool,
                    read: None,
                    account: key.1.clone(),
                    targets,
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
    // Once the tool has read the accounts and the balance: written where
    // it read them.
    if let Some((read, accounts, asserted)) = &f.read {
        if *read != text {
            ui::notify(
                "The journal changed while the tool read it: finish again",
                ui::Level::Warning,
            );
        } else {
            finish_with(d, &f, &text, accounts, asserted.clone());
        }
        return Ok("null".into());
    }
    // The accounts as the tool reads them: a posting written with an
    // alias or under `apply account` is the account's too.
    let Some((program, mut args)) = reconcile::accounts_command(f.tool, &f.main) else {
        // Beancount's: no aliases, and its `balance` counts what the
        // target counts.
        let targets = f.targets.clone();
        finish_with(d, &f, &text, &reconcile::Accounts::new(), Some(Ok(targets)));
        return Ok("null".into());
    };
    let live = journal::same_file(&f.journal, &f.main);
    if live {
        args = reconcile::from_input(args, &f.main);
    }
    let file = if live {
        "-".to_string()
    } else {
        f.journal.clone()
    };
    let input = live.then(|| text.clone().into_bytes());
    let dir = journal::folder(&f.main).to_string();
    let there = dir.clone();
    let asked = reconcile::asserted_command(f.tool, &f.main, &f.account, f.date);
    run_input(program, args, &dir, input.clone(), move |r| {
        let accounts = match r {
            Ok(e) if e.status == Some(0) => {
                let out = String::from_utf8_lossy(&e.stdout);
                reconcile::accounts_in(f.tool, &out, &file, &text)
            }
            _ => reconcile::Accounts::new(),
        };
        // Then the balance the assertion would check at the statement's
        // date: an outstanding check counts in it, not in the target.
        let Some((program, mut args)) = asked else {
            let targets = f.targets.clone();
            return again(f, text, accounts, Some(Ok(targets)));
        };
        if live {
            args = reconcile::from_input(args, &f.main);
        }
        run_input(program, args, &there, input, move |r| {
            let asserted = match r {
                Ok(e) if e.status == Some(0) => Some(reconcile::asserted(
                    f.tool,
                    &String::from_utf8_lossy(&e.stdout),
                    &f.account,
                    &f.targets,
                )),
                _ => None,
            };
            again(f, text, accounts, asserted);
        });
    });
    Ok("null".into())
}

/// Keeps what the tool read for `f` and runs Finish's command again,
/// which writes it where the journal is the document (a process's answer
/// has none).
fn again(mut f: Finish, text: String, accounts: reconcile::Accounts, asserted: Option<Asserted>) {
    f.read = Some((text, accounts, asserted));
    FINISH.with(|x| *x.borrow_mut() = Some(f));
    if let Err(why) = kalem::run("pta.finishReconcileWrite", "{}") {
        ui::notify(&why, ui::Level::Warning);
    }
}

/// Finish's edits in its journal, `text`, a posting's account the tool's
/// (`accounts`): the account's pending marks cleared, and the balance
/// assertion written where it holds at the statement's date (`asserted`),
/// one undo step. Where it does not (a posting not cleared, such as an
/// outstanding check, dated on or before it, counts in the balance
/// there), the marks are cleared all the same, as ledger-mode's finish
/// clears them, and the message says why no assertion was written.
fn finish_with(
    d: Dialect,
    f: &Finish,
    text: &str,
    accounts: &reconcile::Accounts,
    asserted: Option<Asserted>,
) {
    let column = crate::align::Settings::read(kalem_plugin::settings::own).column;
    let amounts = match &asserted {
        Some(Ok(a)) => a.clone(),
        _ => Vec::new(),
    };
    let edits = reconcile::finish(text, d, &f.account, &amounts, f.date, column, accounts);
    let cleared = edits.len() - usize::from(!amounts.is_empty());
    super::edit(&edits, "Finish Reconciliation");
    let written = |l: &[reconcile::Amount]| {
        l.iter()
            .map(reconcile::Amount::written)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let date = f.date.written('-');
    let (said, level) = match asserted {
        Some(Ok(amounts)) => {
            // Ledger's assertion of the account alone, where its
            // subaccounts have postings.
            let alone = if written(&amounts) == written(&f.targets) {
                String::new()
            } else {
                format!(
                    " ({} alone, as Ledger asserts it: {})",
                    f.account,
                    written(&amounts)
                )
            };
            (
                format!(
                    "Reconciled {} at {}: {cleared} marks cleared, the balance asserted{alone}; save to check",
                    f.account,
                    written(&f.targets)
                ),
                ui::Level::Info,
            )
        }
        Some(Err(off)) => {
            let (target, balance) = *off;
            let early = reconcile::unmarked_by(text, d, &f.account, f.date, accounts);
            let why = match early.as_slice() {
                [] => String::new(),
                [one] => format!(
                    ": a posting not marked, dated {}, counts in it",
                    one.written('-')
                ),
                [first, ..] => format!(
                    ": {} postings not marked dated on or before it count in it, the first on {}",
                    early.len(),
                    first.written('-')
                ),
            };
            (
                format!(
                    "Reconciled {}: {cleared} marks cleared; no balance assertion written, the balance on {date} being {}, not {}{why}",
                    f.account,
                    balance.written(),
                    target.written()
                ),
                ui::Level::Info,
            )
        }
        None => (
            format!(
                "Reconciled {}: {cleared} marks cleared; no balance assertion written, {} not giving the balance on {date}",
                f.account,
                f.tool.checker()
            ),
            ui::Level::Warning,
        ),
    };
    ui::notify(&said, level);
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
