//! The Report documents (PT7c): the tool's other reports, picked from
//! one list (`SPC m r`, "Report…", after the Balance and the Register;
//! `o` in a Report), each shown as the
//! tool prints it ([`crate::other`]), a journal's syntax highlighted as
//! the journal; `p` picks the period of a report that takes one, `r`
//! runs it again, and it is written again when the journal is saved.

use std::cell::RefCell;
use std::collections::BTreeMap;

use kalem_plugin::{documents, kalem, settings, ui};

use crate::Dialect;
use crate::journal;
use crate::other::{self, Report};
use crate::report::Period;

use super::{NotRun, resolve, run, today};

const DOC: &str = "pta.report";
pub(super) const KIND: &str = "pta-report";

/// A report shown.
#[derive(Debug)]
struct Shown {
    tool: Dialect,
    report: Report,
    doc: Option<u64>,
    period: Period,
    run: u64,
}

thread_local! {
    /// By the main journal and the report's title.
    static REPORTS: RefCell<BTreeMap<(String, String), Shown>> = const { RefCell::new(BTreeMap::new()) };
}

/// The report whose document is the current one.
pub(super) fn here() -> Option<(String, String)> {
    let n = documents::current()?;
    REPORTS.with(|r| {
        r.borrow()
            .iter()
            .find(|(_, e)| e.doc == Some(n))
            .map(|(k, _)| k.clone())
    })
}

/// Report… (`SPC m r` in a journal, Doom's ledger-report; `o` in a
/// Report): the reports of the journal's tool, to pick one; from a
/// journal, the Balance and the Register first. Its arguments (`kalem
/// run`) may name the `report` by its title, and its `period`.
pub(super) fn command(args: &str) -> Result<String, String> {
    if let Some(title) = super::arg(args, "report") {
        let period = super::period_arg(args);
        let Some((d, Some(path))) = super::dialect_here() else {
            ui::notify("This document is no journal with a file", ui::Level::Info);
            return Ok("null".into());
        };
        resolve(path, d, move |tool, main| {
            named(tool, main.path, title, period);
        });
        return Ok("null".into());
    }
    let shown =
        here().and_then(|k| REPORTS.with(|r| r.borrow().get(&k).map(|e| (e.tool, k.0.clone()))));
    if let Some((tool, main)) = shown {
        pick(tool, main, false);
        return Ok("null".into());
    }
    if let Some(main) = super::balance::here() {
        let tool = super::balance::tool_of(&main).unwrap_or(Dialect::Hledger);
        pick(tool, main, false);
        return Ok("null".into());
    }
    let Some((d, Some(path))) = super::dialect_here() else {
        ui::notify("This document is no journal with a file", ui::Level::Info);
        return Ok("null".into());
    };
    resolve(path, d, |tool, main| pick(tool, main.path, true));
    Ok("null".into())
}

/// Opens the report titled `title` (any case), the setting's, a saved
/// query's (`Query: NAME`) or the tool's, of `period` when given.
fn named(tool: Dialect, main: String, title: String, period: Option<Period>) {
    let entries = settings::own("reports")
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    let mut list = other::pinned(tool, &entries);
    let open_it = move |list: Vec<Report>, main: String| match list
        .into_iter()
        .find(|r| r.title.eq_ignore_ascii_case(title.trim()))
    {
        Some(r) => open(tool, main, r, period),
        // An error: `kalem run` exits with 1.
        None => ui::notify(&format!("No report {title}"), ui::Level::Error),
    };
    if tool != Dialect::Beancount {
        list.extend(other::reports(tool, today()));
        return open_it(list, main);
    }
    let dir = journal::folder(&main).to_string();
    let queried = main.clone();
    run(
        "bean-query",
        other::saved_queries_args(&queried),
        &dir,
        move |r| {
            if let Ok(e) = &r
                && e.status == Some(0)
            {
                list.extend(other::saved_queries(&String::from_utf8_lossy(&e.stdout)));
            }
            list.extend(other::reports(tool, today()));
            open_it(list, main);
        },
    );
}

/// The plugin's own reports, run as their commands from a journal, with
/// their keys.
const OWN: &[(&str, &str, &str)] = &[
    ("Balance", "pta.balance", "SPC m b"),
    ("Register…", "pta.register", "SPC m g r"),
];

/// The setting `reports`' pinned reports, then Beancount's saved
/// queries, then the tool's own; the plugin's own before them when
/// `own`.
fn pick(tool: Dialect, main: String, own: bool) {
    let entries = settings::own("reports")
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    let mut list = other::pinned(tool, &entries);
    if tool != Dialect::Beancount {
        list.extend(other::reports(tool, today()));
        offer(tool, main, list, own);
        return;
    }
    let dir = journal::folder(&main).to_string();
    run(
        "bean-query",
        other::saved_queries_args(&main),
        &dir,
        move |r| {
            if let Ok(e) = &r
                && e.status == Some(0)
            {
                list.extend(other::saved_queries(&String::from_utf8_lossy(&e.stdout)));
            }
            list.extend(other::reports(tool, today()));
            offer(tool, main, list, own);
        },
    );
}

/// The pick: each report's title, with hledger's and Ledger's arguments
/// (Beancount's queries are too long to read there); the plugin's own
/// first when `own`.
fn offer(tool: Dialect, main: String, list: Vec<Report>, own: bool) {
    let first: &'static [(&str, &str, &str)] = if own { OWN } else { &[] };
    let items: Vec<ui::PickItem> = first
        .iter()
        .map(|(title, _, keys)| ui::PickItem {
            label: (*title).to_string(),
            detail: Some((*keys).to_string()),
        })
        .chain(list.iter().map(|r| {
            let mut detail = r.args.join(" ");
            if detail.chars().count() > 40 {
                detail = detail.chars().take(39).collect::<String>() + "…";
            }
            ui::PickItem {
                label: r.title.clone(),
                detail: (tool != Dialect::Beancount).then_some(detail),
            }
        }))
        .collect();
    ui::quick_pick(
        &items,
        &ui::PickOptions {
            title: Some(format!("Report of {}", journal::name(&main))),
            placeholder: None,
            many: false,
        },
        move |picked| {
            let Some(&i) = picked.first() else { return };
            let i = i as usize;
            if let Some((_, id, _)) = first.get(i) {
                if let Err(why) = kalem::run(id, "{}") {
                    ui::notify(&why, ui::Level::Warning);
                }
            } else if let Some(r) = list.get(i - first.len()) {
                open(tool, main, r.clone(), None);
            }
        },
    );
}

/// Opens report `r` of `main`, of `period` when given.
fn open(tool: Dialect, main: String, r: Report, period: Option<Period>) {
    let key = (main, r.title.clone());
    REPORTS.with(|m| {
        let mut m = m.borrow_mut();
        let e = m.entry(key.clone()).or_insert(Shown {
            tool,
            report: r.clone(),
            doc: None,
            period: Period::All,
            run: 0,
        });
        e.tool = tool;
        e.report = r;
        if let Some(p) = period {
            e.period = p;
        }
    });
    gather(&key, true);
}

/// After a journal of `main` is saved: its open reports run again.
pub(super) fn saved(main: &str) {
    let keys: Vec<(String, String)> = REPORTS.with(|r| {
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

/// Runs the report and writes its document.
fn gather(key: &(String, String), open: bool) {
    let Some((tool, report, period, run_no)) = REPORTS.with(|r| {
        let mut r = r.borrow_mut();
        let e = r.get_mut(key)?;
        e.run += 1;
        Some((e.tool, e.report.clone(), e.period, e.run))
    }) else {
        return;
    };
    let (program, args) = other::command(tool, &key.0, &report, period.bounds(today()));
    let dir = journal::folder(&key.0).to_string();
    let key = key.clone();
    run(program, args, &dir, move |r| {
        let current = REPORTS.with(|m| m.borrow().get(&key).map(|e| e.run)) == Some(run_no);
        if !current {
            return;
        }
        let said = match r {
            Ok(e) if e.status == Some(0) => String::from_utf8_lossy(&e.stdout).into_owned(),
            Ok(e) => format!(
                "{program} failed:\n\n{}",
                String::from_utf8_lossy(&e.stderr).trim_end()
            ),
            Err(NotRun::Missing) => {
                let said = if tool == Dialect::Beancount {
                    "bean-query is not installed: pip install beanquery; or give its path as the setting programs.bean-query".to_string()
                } else {
                    super::missing(tool)
                };
                return ui::notify(&said, ui::Level::Warning);
            }
            Err(NotRun::TooLong) => return,
            Err(NotRun::Refused(why)) => {
                return ui::notify(&format!("{program}: {why}"), ui::Level::Warning);
            }
        };
        write(&key, &said, open);
    });
}

/// Writes the report's document (opened when `open`): a header, the
/// keys, and what the tool printed.
fn write(key: &(String, String), said: &str, open: bool) {
    let Some((doc, tool, journal_syntax, text)) = REPORTS.with(|r| {
        let r = r.borrow();
        let e = r.get(key)?;
        let mut header = format!(
            "Report: {} · {} · {}",
            e.report.title,
            journal::name(&key.0),
            e.tool.language()
        );
        if e.report.dated {
            header.push_str(" · ");
            header.push_str(&e.period.name());
        }
        let hint = if e.report.dated {
            "p period · o another report · r again · q close"
        } else {
            "o another report · r again · q close"
        };
        let comment = if e.report.journal { "; " } else { "" };
        let text = format!("{comment}{header}\n{comment}{hint}\n\n{said}");
        Some((e.doc, e.tool, e.report.journal, text))
    }) else {
        return;
    };
    // The header and the keys as the other documents have them.
    let styles = {
        use documents::{Color, StyledSpan, TextStyle};
        let header_end = text.find('\n').unwrap_or(text.len());
        let hint_end = text[header_end + 1..]
            .find('\n')
            .map_or(text.len(), |i| header_end + 1 + i);
        vec![
            StyledSpan {
                start: 0,
                end: header_end as u64,
                style: TextStyle::color(Color::Accent).bold(),
            },
            StyledSpan {
                start: header_end as u64 + 1,
                end: hint_end as u64,
                style: TextStyle::color(Color::Muted),
            },
        ]
    };
    if let Some(n) = doc
        && documents::set_styled(n, &text, None, &styles).is_ok()
    {
        return;
    }
    REPORTS.with(|r| {
        if let Some(e) = r.borrow_mut().get_mut(key) {
            e.doc = None;
        }
    });
    if !open {
        return;
    }
    let mut spec = documents::Spec::new(
        DOC,
        &format!("{}#{}", key.0, key.1),
        &format!("Report: {}", key.1),
        KIND,
    );
    if journal_syntax {
        spec = spec.language(tool.language());
    }
    match documents::open_styled(&spec, &text, None, &styles) {
        Ok(n) => {
            REPORTS.with(|r| {
                if let Some(e) = r.borrow_mut().get_mut(key) {
                    e.doc = Some(n);
                }
            });
            super::owned(n, &key.0);
        }
        Err(why) => ui::notify(&why, ui::Level::Error),
    }
}

/// `r` in a Report: run again.
pub(super) fn again() -> Result<String, String> {
    if let Some(key) = here() {
        gather(&key, true);
    }
    Ok("null".into())
}

/// `p` in a Report: the period, picked or typed; the report run again.
pub(super) fn pick_period() -> Result<String, String> {
    let Some(key) = here() else {
        return Ok("null".into());
    };
    let dated = REPORTS.with(|r| r.borrow().get(&key).is_some_and(|e| e.report.dated));
    if !dated {
        ui::notify("This report takes no period", ui::Level::Info);
        return Ok("null".into());
    }
    let now = REPORTS.with(|r| r.borrow().get(&key).map(|e| e.period));
    super::pick_period(
        "Period of the report",
        now.unwrap_or(Period::All),
        move |p| {
            REPORTS.with(|r| {
                if let Some(e) = r.borrow_mut().get_mut(&key) {
                    e.period = p;
                }
            });
            gather(&key, false);
        },
    );
    Ok("null".into())
}

/// Closes the Report document.
pub(super) fn close() -> Result<String, String> {
    if let Some(n) = documents::current() {
        documents::close(n);
        forget(n);
    }
    Ok("null".into())
}

/// Whether document `n` is one of these.
pub(super) fn has_doc(n: u64) -> bool {
    REPORTS.with(|r| r.borrow().values().any(|e| e.doc == Some(n)))
}

/// Document `n` closed.
pub(super) fn forget(n: u64) {
    REPORTS.with(|r| {
        for e in r.borrow_mut().values_mut() {
            if e.doc == Some(n) {
                e.doc = None;
            }
        }
    });
}
