//! The Balance document (PT7a): the tool's balance report of the main
//! journal (`crate::report`), its sections run one after the other, laid
//! out as a tree the user folds by account; its period and depth picked,
//! written again on the journal's save, the cursor kept on its account.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use kalem_plugin::{documents, editor, ui};

use crate::Dialect;
use crate::journal::{self, Main};
use crate::report::{self, Part, Period, Row, Section};

use super::{NotRun, resolve, run, today};

/// The document's id and kind.
const DOC: &str = "pta.balance";
pub(super) const KIND: &str = "pta-balance";

/// A main journal's balance.
#[derive(Debug)]
struct Balance {
    tool: Dialect,
    doc: Option<u64>,
    period: Period,
    depth: usize,
    folded: BTreeSet<String>,
    /// Market values (`v`).
    valued: bool,
    parts: Vec<(Section, String, Part)>,
    shown: report::Document,
    /// Counts its runs: an older run's answer is dropped.
    run: u64,
}

thread_local! {
    static BALANCES: RefCell<BTreeMap<String, Balance>> = const { RefCell::new(BTreeMap::new()) };
}

/// The main journal whose Balance document is the current one.
pub(super) fn here() -> Option<String> {
    let n = documents::current()?;
    BALANCES.with(|b| {
        b.borrow()
            .iter()
            .find(|(_, e)| e.doc == Some(n))
            .map(|(m, _)| m.clone())
    })
}

/// Balance (`SPC m b`): the main journal's, opened; in the document,
/// written again. Its arguments (`kalem run`) may name the `period` and
/// ask for market values (`"value": true`).
pub(super) fn command(args: &str) -> Result<String, String> {
    if let Some(main) = here() {
        refresh(&main, true);
        return Ok("null".into());
    }
    let period = super::period_arg(args);
    let valued = super::arg(args, "value").map(|v| v == "true");
    match super::dialect_here() {
        Some((d, Some(path))) => {
            resolve(path, d, move |tool, main| {
                BALANCES.with(|b| {
                    let mut b = b.borrow_mut();
                    let e = b.entry(main.path.clone()).or_insert(Balance {
                        tool,
                        doc: None,
                        period: Period::All,
                        depth: 0,
                        folded: BTreeSet::new(),
                        valued: false,
                        parts: Vec::new(),
                        shown: report::Document::default(),
                        run: 0,
                    });
                    e.tool = tool;
                    if let Some(p) = period {
                        e.period = p;
                    }
                    if let Some(v) = valued {
                        e.valued = v;
                    }
                });
                gather(main, true);
            });
        }
        _ => ui::notify("This document is no journal with a file", ui::Level::Info),
    }
    Ok("null".into())
}

/// Runs the report again for `main`, written in its document.
fn refresh(main: &str, open: bool) {
    gather(
        Main {
            path: main.to_string(),
            why: journal::Why::Itself,
        },
        open,
    );
}

/// After a journal of `main` is saved: its open Balance written again.
pub(super) fn saved(main: &str) {
    let open = BALANCES.with(|b| b.borrow().get(main).is_some_and(|e| e.doc.is_some()));
    if open {
        refresh(main, false);
    }
}

/// Runs the tool for each section of `main`'s balance, then lays it out.
fn gather(main: Main, open: bool) {
    let Some((tool, period, depth, run_no)) = BALANCES.with(|b| {
        let mut b = b.borrow_mut();
        let e = b.get_mut(&main.path)?;
        e.run += 1;
        Some((e.tool, e.period, e.depth, e.run))
    }) else {
        return;
    };
    let bounds = period.bounds(today());
    let dir = journal::folder(&main.path).to_string();
    let path = main.path.clone();
    section(
        tool,
        path.clone(),
        Section::Sheet,
        bounds,
        depth,
        dir.clone(),
        move |sheet| {
            section(
                tool,
                path.clone(),
                Section::Flow,
                bounds,
                depth,
                dir,
                move |flow| {
                    let current =
                        BALANCES.with(|b| b.borrow().get(&path).map(|e| e.run)) == Some(run_no);
                    if !current {
                        return;
                    }
                    match (sheet, flow) {
                        (Ok(sheet), Ok(flow)) => {
                            let parts = vec![
                                (Section::Sheet, title(Section::Sheet, period, bounds), sheet),
                                (Section::Flow, title(Section::Flow, period, bounds), flow),
                            ];
                            BALANCES.with(|b| {
                                if let Some(e) = b.borrow_mut().get_mut(&path) {
                                    e.parts = parts;
                                }
                            });
                            write(&path, open, None);
                        }
                        // An empty reason was told already (`run`).
                        (Err(why), _) | (_, Err(why)) if why.is_empty() => {}
                        (Err(why), _) | (_, Err(why)) => ui::notify(&why, ui::Level::Warning),
                    }
                },
            );
        },
    );
}

/// A section's title: what it shows, and when.
fn title(
    s: Section,
    period: Period,
    bounds: Option<(crate::write::Date, crate::write::Date)>,
) -> String {
    use crate::report::{EARLIEST, LATEST};
    match (s, bounds) {
        (_, None) => s.title().to_string(),
        (Section::Sheet, Some((_, end))) if end == LATEST => s.title().to_string(),
        (Section::Sheet, Some((_, end))) => {
            format!("{}, at the end of {}", s.title(), end.plus(-1).written('-'))
        }
        (Section::Flow, Some((begin, end))) => {
            let last = end.plus(-1).written('-');
            let dates = match (begin == EARLIEST, end == LATEST) {
                (true, true) => return s.title().to_string(),
                (true, false) => format!("up to {last}"),
                (false, true) => format!("from {}", begin.written('-')),
                (false, false) => format!("{} to {last}", begin.written('-')),
            };
            match period {
                // Typed dates: their name says them already.
                Period::Dates(..) => format!("{}, {dates}", s.title()),
                _ => format!("{}, {} ({dates})", s.title(), period.name()),
            }
        }
    }
}

/// One section from the tool: hledger's and Ledger's one run,
/// Beancount's a run a level of the tree and one for the total.
fn section(
    tool: Dialect,
    main: String,
    s: Section,
    bounds: Option<(crate::write::Date, crate::write::Date)>,
    depth: usize,
    dir: String,
    done: impl FnOnce(Result<Part, String>) + 'static,
) {
    if tool != Dialect::Beancount {
        let (program, mut args) = report::command(tool, &main, s, bounds, depth, 0);
        if valued(&main) {
            args = report::value_args(tool_or(&main), &mut args);
        }
        run(program, args, &dir, move |r| {
            done(read(tool, r).map(|out| {
                Part::of(if tool == Dialect::Hledger {
                    report::hledger_rows(&out)
                } else {
                    report::ledger_rows(&out)
                })
            }));
        });
        return;
    }
    levels(main, s, bounds, depth, dir, done);
}

/// What Beancount's balance is done with.
type Done = Box<dyn FnOnce(Result<Part, String>)>;

/// Beancount's levels as they come, and what is done with them.
struct Gathered {
    outs: Vec<Option<Result<String, String>>>,
    done: Option<Done>,
}

/// Beancount's tree: its accounts whole first (the deepest level, which
/// says how deep the tree goes, or the depth asked), then every level
/// above and the total at once, side by side: `bean-query` reads the
/// whole journal for each (some 7 s for 200,000 transactions).
fn levels(
    main: String,
    s: Section,
    bounds: Option<(crate::write::Date, crate::write::Date)>,
    depth: usize,
    dir: String,
    done: impl FnOnce(Result<Part, String>) + 'static,
) {
    let command = move |main: &str, level: usize| {
        let (program, mut args) =
            report::command(Dialect::Beancount, main, s, bounds, depth, level);
        if valued(main) {
            args = report::value_args(tool_or(main), &mut args);
        }
        (program, args)
    };
    let deepest = if depth > 0 { depth } else { 12 };
    let (program, args) = command(&main, deepest);
    let d = dir.clone();
    run(program, args, &dir, move |r| {
        let whole = match read(Dialect::Beancount, r) {
            Ok(out) => report::beancount_rows(&out),
            Err(why) => return done(Err(why)),
        };
        let max = whole
            .iter()
            .map(|r| r.account.split(':').count())
            .max()
            .unwrap_or(0);
        // The levels above the deepest, then the total (level 0).
        let wanted: Vec<usize> = (1..max.min(deepest)).chain([0]).collect();
        // Each level's output as it comes, and what to do with them all.
        let outs: Vec<Option<Result<String, String>>> = vec![None; wanted.len()];
        let done: Done = Box::new(done);
        let state = Rc::new(RefCell::new(Gathered {
            outs,
            done: Some(done),
        }));
        for (i, &level) in wanted.iter().enumerate() {
            let (program, args) = command(&main, level);
            let state = state.clone();
            let whole = whole.clone();
            run(program, args, &d, move |r| {
                let mut st = state.borrow_mut();
                st.outs[i] = Some(read(Dialect::Beancount, r));
                if st.outs.iter().any(Option::is_none) {
                    return;
                }
                let Some(done) = st.done.take() else { return };
                let outs: Result<Vec<String>, String> =
                    st.outs.iter_mut().filter_map(Option::take).collect();
                drop(st);
                done(outs.map(|mut outs| {
                    let total = outs.pop().unwrap_or_default();
                    let mut got: Vec<Vec<Row>> =
                        outs.iter().map(|o| report::beancount_rows(o)).collect();
                    got.push(whole);
                    Part {
                        rows: report::beancount_tree(&got),
                        total: report::beancount_rows(&total)
                            .into_iter()
                            .next()
                            .map(|r| r.amounts)
                            .unwrap_or_default(),
                    }
                }));
            });
        }
    });
}

/// A run's standard output, or why there is none.
fn read(tool: Dialect, r: Result<kalem_plugin::process::Exit, NotRun>) -> Result<String, String> {
    match r {
        Ok(e) if e.status == Some(0) => Ok(String::from_utf8_lossy(&e.stdout).into_owned()),
        Ok(e) => {
            let said = String::from_utf8_lossy(&e.stderr);
            Err(format!(
                "{}: {}",
                tool.checker(),
                said.lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("failed")
            ))
        }
        Err(NotRun::Missing) => Err(super::missing(tool)),
        Err(NotRun::Refused(why)) => Err(format!("{}: {why}", tool.checker())),
        // Told by `run`.
        Err(NotRun::TooLong) => Err(String::new()),
    }
}

/// Lays out `main`'s balance and writes its document (opened when
/// `open`), the cursor on `account`, else on the line it was on.
fn write(main: &str, open: bool, account: Option<&str>) {
    let Some((doc, text, styles, cursor)) = BALANCES.with(|b| {
        let mut b = b.borrow_mut();
        let e = b.get_mut(main)?;
        let depth = if e.depth == 0 {
            "every level".to_string()
        } else {
            format!("{} levels", e.depth)
        };
        let header = format!(
            "Balance: {} · {} · {} · {depth}{}",
            journal::name(main),
            e.tool.language(),
            e.period.name(),
            if e.valued { " · market value" } else { "" }
        );
        let hint = "Tab fold · Enter register · p period · d depth · v value · r again · q close";
        e.shown = report::layout(&header, hint, &e.parts, &e.folded, super::glyphs());
        let cursor = account.and_then(|a| {
            let n = e.shown.lines.iter().position(|l| l.as_deref() == Some(a))?;
            Some(
                e.shown
                    .text
                    .split_inclusive('\n')
                    .take(n)
                    .map(str::len)
                    .sum::<usize>() as u64,
            )
        });
        Some((e.doc, e.shown.text.clone(), styles(&e.shown), cursor))
    }) else {
        return;
    };
    if let Some(n) = doc
        && documents::set_styled(n, &text, cursor, &styles).is_ok()
    {
        return;
    }
    BALANCES.with(|b| {
        if let Some(e) = b.borrow_mut().get_mut(main) {
            e.doc = None;
        }
    });
    if !open {
        return;
    }
    let spec = documents::Spec::new(
        DOC,
        main,
        &format!("Balance: {}", journal::name(main)),
        KIND,
    );
    match documents::open_styled(&spec, &text, cursor, &styles) {
        Ok(n) => {
            BALANCES.with(|b| {
                if let Some(e) = b.borrow_mut().get_mut(main) {
                    e.doc = Some(n);
                }
            });
            super::owned(n, main);
        }
        Err(why) => ui::notify(&why, ui::Level::Error),
    }
}

pub(super) fn styles(d: &report::Document) -> Vec<documents::StyledSpan> {
    use documents::{Color, TextStyle};
    d.styles
        .iter()
        .map(|(r, s)| documents::StyledSpan {
            start: r.start as u64,
            end: r.end as u64,
            style: match s {
                report::Style::Header => TextStyle::color(Color::Accent).bold(),
                report::Style::Section => TextStyle::color(Color::Default).bold(),
                report::Style::Hint | report::Style::Mark => TextStyle::color(Color::Muted),
                report::Style::Negative => TextStyle::color(Color::Red),
                report::Style::Total => TextStyle::color(Color::Default).bold(),
                report::Style::Warning => TextStyle::color(Color::Yellow),
            },
        })
        .collect()
}

/// Whether `main`'s balance shows market values.
fn valued(main: &str) -> bool {
    BALANCES.with(|b| b.borrow().get(main).is_some_and(|e| e.valued))
}

fn tool_or(main: &str) -> Dialect {
    tool_of(main).unwrap_or(Dialect::Hledger)
}

/// `v`: market values, or back.
pub(super) fn toggle_value() -> Result<String, String> {
    let Some(main) = here() else {
        return Ok("null".into());
    };
    BALANCES.with(|b| {
        if let Some(e) = b.borrow_mut().get_mut(&main) {
            e.valued = !e.valued;
        }
    });
    refresh(&main, false);
    Ok("null".into())
}

/// The tool of `main`'s balance.
pub(super) fn tool_of(main: &str) -> Option<Dialect> {
    BALANCES.with(|b| b.borrow().get(main).map(|e| e.tool))
}

/// The account on the cursor's line of the Balance document.
pub(super) fn account_here() -> Option<(String, String)> {
    let main = here()?;
    let at = editor::selected().head as usize;
    BALANCES.with(|b| {
        let b = b.borrow();
        let e = b.get(&main)?;
        let n = e.shown.text[..at.min(e.shown.text.len())]
            .matches('\n')
            .count();
        let a = e.shown.lines.get(n)?.clone()?;
        Some((main.clone(), a))
    })
}

/// Folds (`Some(true)`), unfolds (`Some(false)`) or toggles (`None`)
/// the account at the cursor; Left on an account without children folds
/// its parent.
pub(super) fn fold(how: Option<bool>) -> Result<String, String> {
    let Some((main, account)) = account_here() else {
        return Ok("null".into());
    };
    let target = BALANCES.with(|b| {
        let b = b.borrow();
        let e = b.get(&main)?;
        if e.shown.parents.contains(&account) {
            return Some(account.clone());
        }
        // A leaf: Left folds its parent, which keeps the cursor.
        if how == Some(true) {
            let mut a = account.as_str();
            while let Some((p, _)) = a.rsplit_once(':') {
                if e.shown.parents.contains(p) {
                    return Some(p.to_string());
                }
                a = p;
            }
        }
        None
    });
    let Some(target) = target else {
        return Ok("null".into());
    };
    BALANCES.with(|b| {
        if let Some(e) = b.borrow_mut().get_mut(&main) {
            let folded = e.folded.contains(&target);
            let fold = how.unwrap_or(!folded);
            if fold {
                e.folded.insert(target.clone());
            } else {
                e.folded.remove(&target);
            }
        }
    });
    write(&main, false, Some(&target));
    Ok("null".into())
}

/// The period, picked or typed; the balance run again.
pub(super) fn pick_period() -> Result<String, String> {
    let Some(main) = here() else {
        return Ok("null".into());
    };
    let now = BALANCES.with(|b| b.borrow().get(&main).map(|e| e.period));
    super::pick_period(
        "Period of the balance",
        now.unwrap_or(Period::All),
        move |p| {
            BALANCES.with(|b| {
                if let Some(e) = b.borrow_mut().get_mut(&main) {
                    e.period = p;
                }
            });
            refresh(&main, false);
        },
    );
    Ok("null".into())
}

/// The depth, picked; the report run again.
pub(super) fn pick_depth() -> Result<String, String> {
    let Some(main) = here() else {
        return Ok("null".into());
    };
    let depths = [1usize, 2, 3, 4, 0];
    let items: Vec<ui::PickItem> = depths
        .iter()
        .map(|d| ui::PickItem {
            label: if *d == 0 {
                "every level".into()
            } else {
                format!("{d} levels")
            },
            detail: None,
        })
        .collect();
    ui::quick_pick(
        &items,
        &ui::PickOptions {
            title: Some("Depth of the balance".into()),
            placeholder: None,
            many: false,
        },
        move |picked| {
            let Some(&i) = picked.first() else { return };
            let Some(&d) = depths.get(i as usize) else {
                return;
            };
            BALANCES.with(|b| {
                if let Some(e) = b.borrow_mut().get_mut(&main) {
                    e.depth = d;
                }
            });
            refresh(&main, false);
        },
    );
    Ok("null".into())
}

/// Closes the Balance document.
pub(super) fn close() -> Result<String, String> {
    if let Some(n) = documents::current() {
        documents::close(n);
        forget(n);
    }
    Ok("null".into())
}

/// Whether document `n` is one of these.
pub(super) fn has_doc(n: u64) -> bool {
    BALANCES.with(|r| r.borrow().values().any(|e| e.doc == Some(n)))
}

/// Document `n` closed.
pub(super) fn forget(n: u64) {
    BALANCES.with(|b| {
        for e in b.borrow_mut().values_mut() {
            if e.doc == Some(n) {
                e.doc = None;
            }
        }
    });
}
