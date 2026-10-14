//! The Register document (PT7b): the tool's register of an account
//! (`crate::report`), a line per posting with the running total; Enter
//! opens the journal at the posting, by the place the tool gives
//! (hledger's found in `hledger print -O json` when asked).

use std::cell::RefCell;
use std::collections::BTreeMap;

use kalem_plugin::{documents, editor, kalem, ui};

use crate::Dialect;
use crate::journal::{self, Main};
use crate::report::{self, Period, Posting};

use super::{NotRun, resolve, run, today};

const DOC: &str = "pta.register";
pub(super) const KIND: &str = "pta-register";

/// An account's register.
#[derive(Debug)]
struct Register {
    tool: Dialect,
    main: String,
    account: String,
    doc: Option<u64>,
    period: Period,
    /// Market values (`v`).
    valued: bool,
    /// The tool's query narrowing it (`/`), empty for none.
    query: String,
    rows: Vec<Posting>,
    shown: report::Document,
    run: u64,
}

thread_local! {
    /// By the main journal and the account.
    static REGISTERS: RefCell<BTreeMap<(String, String), Register>> = const { RefCell::new(BTreeMap::new()) };
}

/// The register whose document is the current one.
pub(super) fn here() -> Option<(String, String)> {
    let n = documents::current()?;
    REGISTERS.with(|r| {
        r.borrow()
            .iter()
            .find(|(_, e)| e.doc == Some(n))
            .map(|(k, _)| k.clone())
    })
}

/// Register (`SPC m g r`; Enter on an account of the Balance): in the
/// Balance, of the account at the cursor; in a journal, of the account
/// the arguments name (`kalem run`: `account`, `period`), else of the
/// posting at the cursor, else of one picked; in a Register, written
/// again.
pub(super) fn command(args: &str) -> Result<String, String> {
    if let Some(key) = here() {
        gather(&key, true);
        return Ok("null".into());
    }
    if let Some((main, account)) = super::balance::account_here() {
        let tool = super::balance::tool_of(&main).unwrap_or(Dialect::Hledger);
        open(tool, main, account, None);
        return Ok("null".into());
    }
    // A line of the Balance without an account: its headings, totals and
    // blank lines.
    if super::balance::here().is_some() {
        ui::notify("No account on this line", ui::Level::Info);
        return Ok("null".into());
    }
    let Some((d, Some(path))) = super::dialect_here() else {
        ui::notify("This document is no journal with a file", ui::Level::Info);
        return Ok("null".into());
    };
    let period = super::period_arg(args);
    let account = super::arg(args, "account").or_else(|| {
        let text = editor::text(None);
        let at = editor::selected().head as usize;
        crate::write::account_at(&text, at, d)
    });
    resolve(path, d, move |tool, main| match account {
        Some(a) => open(tool, main.path, a, period),
        None => pick_account(tool, main),
    });
    Ok("null".into())
}

/// The accounts the tool knows, to pick one.
fn pick_account(tool: Dialect, main: Main) {
    let (program, args): (&'static str, Vec<String>) = match tool {
        Dialect::Beancount => (
            "bean-query",
            vec![
                "-f".into(),
                "csv".into(),
                main.path.clone(),
                "SELECT DISTINCT account ORDER BY account".into(),
            ],
        ),
        _ => (
            tool.checker(),
            vec!["-f".into(), main.path.clone(), "accounts".into()],
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
                title: Some("Register of the account".into()),
                placeholder: None,
                many: false,
            },
            move |picked| {
                if let Some(a) = picked.first().and_then(|&i| accounts.get(i as usize)) {
                    open(tool, main.path, a.clone(), None);
                }
            },
        );
    });
}

/// Opens the register of `account` in `main`, of `period` when given.
fn open(tool: Dialect, main: String, account: String, period: Option<Period>) {
    let key = (main.clone(), account.clone());
    REGISTERS.with(|r| {
        let mut r = r.borrow_mut();
        let e = r.entry(key.clone()).or_insert(Register {
            tool,
            main,
            account,
            doc: None,
            period: Period::All,
            valued: false,
            query: String::new(),
            rows: Vec::new(),
            shown: report::Document::default(),
            run: 0,
        });
        if let Some(p) = period {
            e.period = p;
        }
    });
    gather(&key, true);
}

/// After a journal of `main` is saved: its open registers written again.
pub(super) fn saved(main: &str) {
    let keys: Vec<(String, String)> = REGISTERS.with(|r| {
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

/// Runs the tool's register and writes the document.
fn gather(key: &(String, String), open: bool) {
    let Some((tool, period, valued, query, run_no)) = REGISTERS.with(|r| {
        let mut r = r.borrow_mut();
        let e = r.get_mut(key)?;
        e.run += 1;
        Some((e.tool, e.period, e.valued, e.query.clone(), e.run))
    }) else {
        return;
    };
    let (program, mut args) =
        report::narrowed_register(tool, &key.0, &key.1, period.bounds(today()), &query);
    if valued {
        args = report::value_args(tool, &mut args);
    }
    let dir = journal::folder(&key.0).to_string();
    let key = key.clone();
    run(program, args, &dir, move |r| {
        let current = REGISTERS.with(|m| m.borrow().get(&key).map(|e| e.run)) == Some(run_no);
        if !current {
            return;
        }
        let out = match r {
            Ok(e) if e.status == Some(0) => String::from_utf8_lossy(&e.stdout).into_owned(),
            Ok(e) => {
                let said = String::from_utf8_lossy(&e.stderr);
                let first = said
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("failed");
                return ui::notify(&format!("{}: {first}", tool.checker()), ui::Level::Warning);
            }
            Err(NotRun::Missing) => return ui::notify(&super::missing(tool), ui::Level::Warning),
            Err(NotRun::TooLong) => return,
            Err(NotRun::Refused(why)) => {
                return ui::notify(&format!("{}: {why}", tool.checker()), ui::Level::Warning);
            }
        };
        let rows = match tool {
            Dialect::Hledger => crate::reconcile::hledger_register(&out),
            Dialect::Ledger => report::ledger_register(&out),
            Dialect::Beancount => report::beancount_register(&out),
        };
        REGISTERS.with(|m| {
            if let Some(e) = m.borrow_mut().get_mut(&key) {
                e.rows = rows;
            }
        });
        write(&key, open);
    });
}

/// Writes the register's document (opened when `open`), the cursor on
/// its last posting when opened.
fn write(key: &(String, String), open: bool) {
    let Some((doc, text, styles, last)) = REGISTERS.with(|r| {
        let mut r = r.borrow_mut();
        let e = r.get_mut(key)?;
        let mut header = format!(
            "Register: {} · {} · {} · {}{}",
            e.account,
            journal::name(&e.main),
            e.tool.language(),
            e.period.name(),
            if e.valued { " · market value" } else { "" }
        );
        if !e.query.is_empty() {
            header.push_str(" · ");
            header.push_str(&e.query);
        }
        let hint = "Enter the posting · p period · v value · / narrow · r again · q close";
        e.shown = report::register_layout(&header, hint, &e.rows);
        let last = e.shown.lines.iter().rposition(Option::is_some).map(|n| {
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
            last,
        ))
    }) else {
        return;
    };
    if let Some(n) = doc
        && documents::set_styled(n, &text, None, &styles).is_ok()
    {
        return;
    }
    REGISTERS.with(|r| {
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
        &format!("Register: {}", key.1),
        KIND,
    );
    match documents::open_styled(&spec, &text, last, &styles) {
        Ok(n) => {
            REGISTERS.with(|r| {
                if let Some(e) = r.borrow_mut().get_mut(key) {
                    e.doc = Some(n);
                }
            });
            super::owned(n, &key.0);
        }
        Err(why) => ui::notify(&why, ui::Level::Error),
    }
}

/// Enter in the Register: the journal at the posting.
pub(super) fn visit() -> Result<String, String> {
    let Some(key) = here() else {
        return Ok("null".into());
    };
    let at = editor::selected().head as usize;
    let Some((tool, posting)) = REGISTERS.with(|r| {
        let r = r.borrow();
        let e = r.get(&key)?;
        let n = e.shown.text[..at.min(e.shown.text.len())]
            .matches('\n')
            .count();
        let i: usize = e.shown.lines.get(n)?.as_deref()?.parse().ok()?;
        Some((e.tool, e.rows.get(i)?.clone()))
    }) else {
        return Ok("null".into());
    };
    go_to(tool, &key.0, posting);
    Ok("null".into())
}

/// The journal at `posting`: its place, or hledger's transaction's.
pub(super) fn go_to(tool: Dialect, main: &str, posting: Posting) {
    place_of(tool, main, posting, |place| match place {
        Some((file, line)) => go(&file, line),
        None => ui::notify(
            "hledger did not say where the transaction is",
            ui::Level::Warning,
        ),
    });
}

/// Where `posting` is (its file and line): the tool's word, or hledger's
/// transaction's, found in `hledger print -O json` of `main` for the
/// posting's day (the whole journal's may pass 16 MB).
pub(super) fn place_of(
    tool: Dialect,
    main: &str,
    posting: Posting,
    done: impl FnOnce(Option<(String, u32)>) + 'static,
) {
    if let Some(place) = posting.place {
        return done(Some(place));
    }
    let Some(n) = posting.transaction else {
        return done(None);
    };
    let mut args = vec![
        "-f".into(),
        main.to_string(),
        "print".into(),
        "-O".into(),
        "json".into(),
    ];
    if let Some((day, _, _)) = crate::write::date_at(&posting.date) {
        args.extend([
            "-b".into(),
            day.written('-'),
            "-e".into(),
            day.plus(1).written('-'),
        ]);
    }
    let dir = journal::folder(main).to_string();
    run(tool.checker(), args, &dir, move |r| {
        let place = r
            .ok()
            .filter(|e| e.status == Some(0))
            .and_then(|e| report::hledger_place(&String::from_utf8_lossy(&e.stdout), n));
        done(place);
    });
}

fn go(file: &str, line: u32) {
    let args = serde_json::json!({ "path": file, "line": line });
    if let Err(why) = kalem::run("file.open", &args.to_string()) {
        ui::notify(&why, ui::Level::Warning);
    }
}

/// The period, picked; the register run again.
pub(super) fn pick_period() -> Result<String, String> {
    let Some(key) = here() else {
        return Ok("null".into());
    };
    let items: Vec<ui::PickItem> = Period::ALL
        .iter()
        .map(|p| ui::PickItem {
            label: p.name().to_string(),
            detail: None,
        })
        .collect();
    ui::quick_pick(
        &items,
        &ui::PickOptions {
            title: Some("Period of the register".into()),
            placeholder: None,
            many: false,
        },
        move |picked| {
            let Some(&i) = picked.first() else { return };
            let Some(&p) = Period::ALL.get(i as usize) else {
                return;
            };
            REGISTERS.with(|r| {
                if let Some(e) = r.borrow_mut().get_mut(&key) {
                    e.period = p;
                }
            });
            gather(&key, false);
        },
    );
    Ok("null".into())
}

/// `/`: the register narrowed by the tool's own query (hledger's
/// `desc:market amt:>100`, Ledger's `payee market`, a Beancount
/// condition such as `narration ~ 'Market'`); empty for all of it.
pub(super) fn narrow() -> Result<String, String> {
    let Some(key) = here() else {
        return Ok("null".into());
    };
    let Some((tool, now)) =
        REGISTERS.with(|r| r.borrow().get(&key).map(|e| (e.tool, e.query.clone())))
    else {
        return Ok("null".into());
    };
    let example = match tool {
        Dialect::Hledger => "desc:market amt:>100",
        Dialect::Ledger => "payee market",
        Dialect::Beancount => "narration ~ 'Market'",
    };
    ui::prompt(
        &format!(
            "Narrow the register ({}'s query, empty for all)",
            tool.language()
        ),
        &ui::PromptOptions {
            value: Some(now),
            placeholder: Some(example.into()),
            password: false,
        },
        move |answer| {
            let Some(q) = answer else { return };
            REGISTERS.with(|r| {
                if let Some(e) = r.borrow_mut().get_mut(&key) {
                    e.query = q.trim().to_string();
                }
            });
            gather(&key, false);
        },
    );
    Ok("null".into())
}

/// `v`: market values, or back.
pub(super) fn toggle_value() -> Result<String, String> {
    let Some(key) = here() else {
        return Ok("null".into());
    };
    REGISTERS.with(|r| {
        if let Some(e) = r.borrow_mut().get_mut(&key) {
            e.valued = !e.valued;
        }
    });
    gather(&key, false);
    Ok("null".into())
}

/// Closes the Register document.
pub(super) fn close() -> Result<String, String> {
    if let Some(n) = documents::current() {
        documents::close(n);
        forget(n);
    }
    Ok("null".into())
}

/// Whether document `n` is one of these.
pub(super) fn has_doc(n: u64) -> bool {
    REGISTERS.with(|r| r.borrow().values().any(|e| e.doc == Some(n)))
}

/// Document `n` closed.
pub(super) fn forget(n: u64) {
    REGISTERS.with(|r| {
        for e in r.borrow_mut().values_mut() {
            if e.doc == Some(n) {
                e.doc = None;
            }
        }
    });
}
