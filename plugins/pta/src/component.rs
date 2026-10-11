//! The component: Kalem's `extension` world. Tool Version runs the
//! journal's tool through `process` (PT1); the status bar item names the
//! tool and the main journal of the journal the cursor is in (PT3c),
//! worked out by [`crate::journal`] from the settings and, for hledger,
//! from `hledger files`, with the last check's mark; Align Amounts and
//! Enter align postings (PT4); Check Journal runs the tool's check on
//! the main journal and writes its problems as the document "Check: …"
//! (PT5), again on each save.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::rc::Rc;

use kalem_plugin::kalem::{self, Event, EventKind, Plugin, Reply, Scope};
use kalem_plugin::{documents, editor, process, settings, ui};

use crate::check::{self, Problem};
use crate::journal::{self, Installed, Main, Settings, ToolChoice};
use crate::{Dialect, align};

/// The status bar item's id.
const STATUS: &str = "pta.journal";
/// The check document's id and kind.
const CHECK_DOC: &str = "pta.check";
const CHECK_KIND: &str = "pta-check";
/// Keys in the check document: Vim's command mode, or the Word-like
/// profile.
const CHECK_WHEN: &str = "textType == pta-check && (vimCommand || !vimActive)";
const CHECK_ESCAPE_WHEN: &str =
    "textType == pta-check && !hasSelection && (vimCommand || !vimActive)";

/// A main journal's last check.
#[derive(Debug, Default)]
struct Checked {
    /// How many problems it found.
    problems: Option<usize>,
    /// Its document, while open.
    doc: Option<u64>,
    /// Each problem's bytes in the document.
    entries: Vec<(Range<usize>, Problem)>,
    /// Counts its runs: an older run's answer is dropped.
    run: u64,
}

thread_local! {
    /// The documents open, by number: their files.
    static DOCS: RefCell<BTreeMap<u64, String>> = const { RefCell::new(BTreeMap::new()) };
    /// The document the cursor was last in.
    static CURRENT: Cell<Option<u64>> = const { Cell::new(None) };
    /// Which tools answered `--version`.
    static INSTALLED: Cell<Installed> = const {
        Cell::new(Installed {
            hledger: None,
            ledger: None,
        })
    };
    /// Counts the item's refreshes: an answer for an older one is dropped.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
    static ITEM: RefCell<Option<kalem::Disposable>> = const { RefCell::new(None) };
    /// The checks, by main journal.
    static CHECKS: RefCell<BTreeMap<String, Checked>> = const { RefCell::new(BTreeMap::new()) };
    /// The tools whose absence a check on save has told.
    static TOLD: RefCell<BTreeSet<&'static str>> = const { RefCell::new(BTreeSet::new()) };
}

/// The journal's dialect: the document's language, else its file name.
fn dialect_here() -> Option<(Dialect, Option<String>)> {
    let doc = editor::document()?;
    let d = doc
        .language
        .as_deref()
        .and_then(Dialect::of_language)
        .or_else(|| doc.path.as_deref().and_then(Dialect::of_path))?;
    Some((d, doc.path))
}

/// The notice for a tool not found: how to install it, and the setting
/// that names where it is.
fn missing(d: Dialect) -> String {
    format!(
        "{}; or give its path as the setting programs.{}",
        d.install(),
        d.checker()
    )
}

/// Why a program did not run.
enum NotRun {
    /// Kalem refused the run (no project, too many runs): its words.
    Refused(String),
    /// The program could not start: not installed.
    Missing,
}

/// Runs `program` with `args` in `dir`, or, where Kalem refuses that
/// folder (a plugin's programs run in the projects' folders only), in
/// the first project's; `done` gets how it ended.
fn run(
    program: &'static str,
    args: Vec<String>,
    dir: &str,
    done: impl FnOnce(Result<process::Exit, NotRun>) + 'static,
) {
    let done = Rc::new(RefCell::new(Some(done)));
    let finish = move |r: Result<process::Exit, NotRun>| {
        if let Some(f) = done.borrow_mut().take() {
            f(r);
        }
    };
    let here = process::Command::new(program).args(args.clone()).cwd(dir);
    let first = finish.clone();
    if process::run(&here, move |r| first(r.map_err(|_| NotRun::Missing))).is_err() {
        let anywhere = process::Command::new(program).args(args);
        let second = finish.clone();
        if let Err(why) = process::run(&anywhere, move |r| second(r.map_err(|_| NotRun::Missing))) {
            finish(Err(NotRun::Refused(why)));
        }
    }
}

/// Tool Version: what the journal's tool says it is.
fn version() -> Result<String, String> {
    let Some((d, path)) = dialect_here() else {
        ui::notify("This document is no journal", ui::Level::Info);
        return Ok("null".into());
    };
    let dir = path
        .as_deref()
        .map(journal::folder)
        .unwrap_or("")
        .to_string();
    run(
        d.checker(),
        vec!["--version".into()],
        &dir,
        move |r| match r {
            Ok(exit) => ui::notify(
                &crate::version_said(d, exit.status, &exit.stdout, &exit.stderr),
                ui::Level::Info,
            ),
            Err(NotRun::Missing) => ui::notify(&missing(d), ui::Level::Warning),
            Err(NotRun::Refused(why)) => {
                ui::notify(&format!("{}: {why}", d.checker()), ui::Level::Warning);
            }
        },
    );
    Ok("null".into())
}

/// Align Amounts (PT4): the amounts of the transaction at the cursor,
/// or of the transactions selected, at the column of `amount_column`;
/// with `line`, those of the cursor's line only (Enter's). One undo step.
fn align(line: bool) {
    let Some((d, _)) = dialect_here() else {
        return;
    };
    let s = align::Settings::read(settings::own);
    let text = editor::text(None);
    let sel = editor::selected();
    let (from, to) = (
        sel.anchor.min(sel.head) as usize,
        sel.anchor.max(sel.head) as usize,
    );
    let edits = if line {
        align::edits_on_line(&text, sel.head as usize, d, s.column)
    } else {
        align::edits(&text, from..to, d, s.column)
    };
    for (r, blanks) in &edits {
        let range = editor::Range {
            start: r.start as u64,
            end: r.end as u64,
        };
        if let Err(why) = editor::replace(range, blanks) {
            ui::notify(&why, ui::Level::Warning);
            return;
        }
    }
    if !edits.is_empty() {
        editor::transact("Align Amounts");
    }
}

/// Enter in a journal: the posting aligned first when `align_on_enter`
/// is on, then Kalem's own new line, which runs once this returns.
fn newline() -> Result<String, String> {
    if align::Settings::read(settings::own).on_enter {
        align(true);
    }
    kalem::run("edit.newline", "{}")?;
    Ok("null".into())
}

fn hide() {
    if let Some(d) = ITEM.with(|i| i.borrow_mut().take()) {
        d.dispose();
    }
}

/// The status bar item for `main`, read by `tool`, with its last check's
/// mark; a click checks it.
fn show(tool: Dialect, main: &Main) {
    let problems = CHECKS.with(|c| c.borrow().get(&main.path).and_then(|c| c.problems));
    let mut text = journal::status_text(tool, main);
    if let Some(n) = problems {
        text.push_str(" · ");
        text.push_str(&check::mark(n));
    }
    let options = ui::StatusOptions {
        tooltip: Some(journal::status_tooltip(tool, main)),
        command: Some("pta.check".into()),
        alignment: ui::Alignment::Left,
        priority: 5,
    };
    if let Ok(d) = ui::status(STATUS, &text, &options) {
        ITEM.with(|i| *i.borrow_mut() = Some(d));
    }
}

fn set_installed(f: impl FnOnce(&mut Installed)) {
    INSTALLED.with(|i| {
        let mut v = i.get();
        f(&mut v);
        i.set(v);
    });
}

/// Works out the tool and the main journal of the journal at `path`
/// (asking which tools are installed, and hledger which journal
/// `LEDGER_FILE` names, where that decides), then calls `done`.
fn resolve(path: String, dialect: Dialect, done: impl FnOnce(Dialect, Main) + 'static) {
    // Read again each time: Kalem may start the plugin before it has read
    // the user's settings.
    let s = Settings::read(settings::own);
    let choice = ToolChoice::parse(&s.tool);
    let dir = journal::folder(&path).to_string();
    if journal::needs_installed(choice, dialect, &path)
        && INSTALLED.with(Cell::get).ledger.is_none()
    {
        let d = dir.clone();
        run("ledger", vec!["--version".into()], &dir, move |r| {
            set_installed(|i| i.ledger = Some(r.is_ok_and(|e| e.status == Some(0))));
            run("hledger", vec!["--version".into()], &d, move |r| {
                set_installed(|i| i.hledger = Some(r.is_ok_and(|e| e.status == Some(0))));
                resolve(path, dialect, done);
            });
        });
        return;
    }
    let tool = journal::tool(choice, dialect, &path, INSTALLED.with(Cell::get));
    let set = s.journal_for(dialect).to_string();
    if tool == Dialect::Hledger && set.trim().is_empty() {
        // `hledger files` without `-f`: the journal LEDGER_FILE names and
        // its includes.
        run("hledger", vec!["files".into()], &dir, move |r| {
            let listed = r
                .ok()
                .filter(|e| e.status == Some(0))
                .map(|e| journal::files_listed(&e.stdout));
            done(tool, journal::main_journal("", &path, listed.as_deref()));
        });
        return;
    }
    done(tool, journal::main_journal(&set, &path, None));
}

/// The item for the journal at `path`, or none where it is no journal.
fn refresh(path: Option<String>) {
    let generation = GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    let Some((path, dialect)) = path.and_then(|p| Dialect::of_path(&p).map(|d| (p, d))) else {
        hide();
        return;
    };
    resolve(path, dialect, move |tool, main| {
        if GENERATION.with(Cell::get) == generation {
            show(tool, &main);
        }
    });
}

/// Checks the main journal of the journal at `path` (PT5) with its tool,
/// its problems written in the check document when `open`, or when that
/// document is open already, and counted in the status bar item.
fn check(path: String, open: bool) {
    let Some(dialect) = Dialect::of_path(&path) else {
        return;
    };
    let generation = GENERATION.with(Cell::get);
    resolve(path, dialect, move |tool, main| {
        let checks = journal_checks();
        let args = check::command(tool, &main.path, &checks);
        let said = check::said(tool, &main.path, &checks);
        let run_no = CHECKS.with(|c| {
            let mut c = c.borrow_mut();
            let entry = c.entry(main.path.clone()).or_default();
            entry.run += 1;
            entry.run
        });
        let dir = journal::folder(&main.path).to_string();
        run(tool.checker(), args, &dir, move |r| {
            let current =
                CHECKS.with(|c| c.borrow().get(&main.path).map(|c| c.run)) == Some(run_no);
            if !current {
                return;
            }
            match r {
                Ok(exit) => {
                    let out = if exit.stderr.iter().any(|b| !b.is_ascii_whitespace()) {
                        &exit.stderr
                    } else {
                        &exit.stdout
                    };
                    let problems =
                        check::problems(tool, exit.status, &String::from_utf8_lossy(out));
                    let report = check::report(&said, &main.path, &problems);
                    write_report(&main, &report, open);
                    if GENERATION.with(Cell::get) == generation {
                        show(tool, &main);
                    }
                }
                Err(NotRun::Missing) => {
                    // Said once a session on save; each time it is asked.
                    if open || TOLD.with(|t| t.borrow_mut().insert(tool.checker())) {
                        ui::notify(&missing(tool), ui::Level::Warning);
                    }
                }
                Err(NotRun::Refused(why)) => {
                    if open {
                        ui::notify(&format!("{}: {why}", tool.checker()), ui::Level::Warning);
                    }
                }
            }
        });
    });
}

/// The setting `checks`: hledger's checks added to `--strict`.
fn journal_checks() -> Vec<String> {
    settings::own("checks")
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
}

/// Writes `report` in the check document of `main`: opened when `open`,
/// else written again only when it is open.
fn write_report(main: &Main, report: &check::Report, open: bool) {
    let styles: Vec<documents::StyledSpan> = report
        .styles
        .iter()
        .map(|(r, c)| {
            let style = match c {
                check::Color::Header => documents::TextStyle::color(documents::Color::Red).bold(),
                check::Color::Passed => documents::TextStyle::color(documents::Color::Green).bold(),
                check::Color::Place => documents::TextStyle::color(documents::Color::Accent),
                check::Color::Muted => documents::TextStyle::color(documents::Color::Muted),
            };
            documents::StyledSpan {
                start: r.start as u64,
                end: r.end as u64,
                style,
            }
        })
        .collect();
    let doc = CHECKS.with(|c| {
        let mut c = c.borrow_mut();
        let entry = c.entry(main.path.clone()).or_default();
        entry.problems = Some(report.entries.len());
        entry.entries = report.entries.clone();
        entry.doc
    });
    let first = report.entries.first().map(|(r, _)| r.start as u64);
    if let Some(n) = doc
        && documents::set_styled(n, &report.text, None, &styles).is_ok()
    {
        return;
    }
    // Closed by the user, or never opened.
    CHECKS.with(|c| {
        if let Some(e) = c.borrow_mut().get_mut(&main.path) {
            e.doc = None;
        }
    });
    if !open {
        return;
    }
    let title = format!("Check: {}", journal::name(&main.path));
    let spec = documents::Spec::new(CHECK_DOC, &main.path, &title, CHECK_KIND);
    match documents::open_styled(&spec, &report.text, first, &styles) {
        Ok(n) => CHECKS.with(|c| {
            if let Some(e) = c.borrow_mut().get_mut(&main.path) {
                e.doc = Some(n);
            }
        }),
        Err(why) => ui::notify(&why, ui::Level::Error),
    }
}

/// The main journal whose check document is the current one.
fn checked_here() -> Option<String> {
    let n = documents::current()?;
    CHECKS.with(|c| {
        c.borrow()
            .iter()
            .find(|(_, e)| e.doc == Some(n))
            .map(|(m, _)| m.clone())
    })
}

/// Check Journal: in a journal, its main journal; in a check document,
/// that one's again.
fn check_command() -> Result<String, String> {
    if let Some(main) = checked_here() {
        check(main, true);
        return Ok("null".into());
    }
    match dialect_here() {
        Some((_, Some(path))) => check(path, true),
        _ => ui::notify("This document is no journal", ui::Level::Info),
    }
    Ok("null".into())
}

/// Enter in the check document: the problem's place, in its file.
fn open_problem() -> Result<String, String> {
    let Some(main) = checked_here() else {
        return Ok("null".into());
    };
    let at = editor::selected().head as usize;
    let problem = CHECKS.with(|c| {
        c.borrow().get(&main).and_then(|e| {
            e.entries
                .iter()
                .find(|(r, _)| r.contains(&at))
                .map(|(_, p)| p.clone())
        })
    });
    let Some(Problem {
        file: Some(file),
        line,
        column,
        ..
    }) = problem
    else {
        return Ok("null".into());
    };
    // `file.open`'s column is a byte of the line from 0; the tools count
    // from 1 (characters: exact on an ASCII line, near on another).
    let column = column.map(|c| c.saturating_sub(1));
    let args = serde_json::json!({ "path": file, "line": line, "column": column });
    kalem::run("file.open", &args.to_string())?;
    Ok("null".into())
}

/// Closes the check document.
fn close_check() -> Result<String, String> {
    if let Some(n) = documents::current() {
        documents::close(n);
        CHECKS.with(|c| {
            for e in c.borrow_mut().values_mut() {
                if e.doc == Some(n) {
                    e.doc = None;
                }
            }
        });
    }
    Ok("null".into())
}

/// The cursor is in document `n` now.
fn current(n: u64) {
    if CURRENT.with(|c| c.replace(Some(n))) == Some(n) {
        return;
    }
    refresh(DOCS.with(|d| d.borrow().get(&n).cloned()));
}

fn current_path() -> Option<String> {
    let n = CURRENT.with(Cell::get)?;
    DOCS.with(|d| d.borrow().get(&n).cloned())
}

/// Registers command `id`, in the palette's Journal group, where `types`
/// are.
fn command(
    id: &str,
    title: &str,
    types: &[&str],
    f: impl FnMut(&str) -> Result<String, String> + 'static,
) -> Result<(), String> {
    let mut spec = kalem::spec(id, title, Scope::only(types));
    spec.category = "Journal".into();
    kalem::command(spec, f).map(|_| ())
}

struct Pta;

impl Plugin for Pta {
    fn activate() -> Result<(), String> {
        let journal = crate::journal_when();
        let leader = format!("{journal} && vimCommand");
        command("pta.version", "Tool Version", crate::TEXT_TYPES, |_| {
            version()
        })?;
        command("pta.align", "Align Amounts", crate::TEXT_TYPES, |_| {
            align(false);
            Ok("null".into())
        })?;
        kalem::keymap("space m a", "pta.align", Some(&leader))?;
        command(
            "pta.newline",
            "New Line, the Posting Aligned",
            crate::TEXT_TYPES,
            |_| newline(),
        )?;
        // Not in Vim's insert mode, whose layer takes Enter in a plain
        // text file before the keys (K10); `SPC m a` aligns there.
        kalem::keymap(
            "enter",
            "pta.newline",
            Some(&format!("{journal} && !vimCommand")),
        )?;
        let mut types = crate::TEXT_TYPES.to_vec();
        types.push(CHECK_KIND);
        command("pta.check", "Check Journal", &types, |_| check_command())?;
        kalem::keymap("space m k", "pta.check", Some(&leader))?;
        kalem::keymap("r", "pta.check", Some(CHECK_WHEN))?;
        command(
            "pta.checkOpen",
            "Open the Problem's Place",
            &[CHECK_KIND],
            |_| open_problem(),
        )?;
        kalem::keymap("enter", "pta.checkOpen", Some(CHECK_WHEN))?;
        command("pta.checkClose", "Close the Check", &[CHECK_KIND], |_| {
            close_check()
        })?;
        kalem::keymap("q", "pta.checkClose", Some(CHECK_WHEN))?;
        kalem::keymap("escape", "pta.checkClose", Some(CHECK_ESCAPE_WHEN))?;
        kalem::on(EventKind::DocumentOpen, |e| {
            if let Event::DocumentOpen(d) = e
                && let Some(p) = &d.path
            {
                DOCS.with(|docs| docs.borrow_mut().insert(d.document, p.clone()));
                if Dialect::of_path(p).is_some() {
                    CURRENT.with(|c| c.set(Some(d.document)));
                    refresh(Some(p.clone()));
                }
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::DocumentClose, |e| {
            if let Event::DocumentClose(n) = e {
                DOCS.with(|docs| docs.borrow_mut().remove(n));
                CHECKS.with(|c| {
                    for e in c.borrow_mut().values_mut() {
                        if e.doc == Some(*n) {
                            e.doc = None;
                        }
                    }
                });
                if CURRENT.with(Cell::get) == Some(*n) {
                    CURRENT.with(|c| c.set(None));
                    refresh(None);
                }
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::SelectionChanged, |e| {
            if let Event::SelectionChanged(s) = e {
                current(s.document);
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::DocumentAfterSave, |e| {
            if let Event::DocumentAfterSave(d) = e
                && Dialect::of_path(&d.path).is_some()
            {
                let on_save = settings::own("check_on_save")
                    .and_then(|v| serde_json::from_str::<bool>(&v).ok())
                    .unwrap_or(true);
                if on_save {
                    // Checking refreshes the item too, with its mark.
                    check(d.path.clone(), false);
                } else {
                    refresh(Some(d.path.clone()));
                }
            }
            Reply::Proceed
        })?;
        for key in ["journal", "tool"] {
            settings::watch(key, true, |_| refresh(current_path()))?;
        }
        Ok(())
    }
}

kalem_plugin::export_plugin!(Pta);
