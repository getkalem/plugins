//! The component: Kalem's `extension` world. Tool Version runs the
//! journal's tool through `process` (PT1); the status bar item names the
//! tool and the main journal of the journal the cursor is in (PT3c),
//! worked out by [`crate::journal`] from the settings and, for hledger,
//! from `hledger files`, with the last check's mark; Align Amounts and
//! Enter align postings (PT4); Check Journal runs the tool's check on
//! the main journal and writes its problems as the document "Check: …"
//! (PT5), again on each save; New Transaction, Toggle Status, Copy
//! Transaction, the next and previous one, a date a day later or
//! earlier, and Sort Transactions write them (PT6); the Balance and the
//! Register (PT7), reconciliation (PT8), import (PT9) and prices (PT10)
//! in their own modules.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::rc::Rc;

use kalem_plugin::kalem::{self, Event, EventKind, Plugin, Reply, Scope};
use kalem_plugin::{documents, editor, process, settings, ui};

mod balance;
mod import;
mod other;
mod prices;
mod reconcile;
mod register;
mod table;

use crate::check::{self, Problem};
use crate::journal::{self, Installed, Main, Settings, ToolChoice};
use crate::write::{self, Date};
use crate::{Dialect, align};

/// The status bar item's id.
const STATUS: &str = "pta.journal";
/// The check document's id and kind.
const CHECK_DOC: &str = "pta.check";
const CHECK_KIND: &str = crate::CHECK_KIND;
/// The problems drawn in a file at most, as the editor takes them.
const MAX_DRAWN: usize = 10_000;
/// Keys in the check document: Vim's command mode, or the Word-like
/// profile.
const CHECK_WHEN: &str = "textType == pta-check && (vimCommand || !vimActive)";
const CHECK_ESCAPE_WHEN: &str =
    "textType == pta-check && !hasSelection && (vimCommand || !vimActive)";
/// Keys in the Balance document.
const BALANCE_WHEN: &str = "textType == pta-balance && (vimCommand || !vimActive)";
const BALANCE_ESCAPE_WHEN: &str =
    "textType == pta-balance && !hasSelection && (vimCommand || !vimActive)";
/// Keys in the Reconcile document.
const RECONCILE_WHEN: &str = "textType == pta-reconcile && (vimCommand || !vimActive)";
const RECONCILE_ESCAPE_WHEN: &str =
    "textType == pta-reconcile && !hasSelection && (vimCommand || !vimActive)";
/// Keys in the Import document, and in a CSV statement.
const IMPORT_WHEN: &str = "textType == pta-import && (vimCommand || !vimActive)";
const IMPORT_ESCAPE_WHEN: &str =
    "textType == pta-import && !hasSelection && (vimCommand || !vimActive)";
const CSV_LEADER_WHEN: &str = "textType == csv && vimCommand";
/// Keys in a Report document.
const REPORT_WHEN: &str = "textType == pta-report && (vimCommand || !vimActive)";
const REPORT_ESCAPE_WHEN: &str =
    "textType == pta-report && !hasSelection && (vimCommand || !vimActive)";
/// Keys in the Register document.
const REGISTER_WHEN: &str = "textType == pta-register && (vimCommand || !vimActive)";
const REGISTER_ESCAPE_WHEN: &str =
    "textType == pta-register && !hasSelection && (vimCommand || !vimActive)";

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
    /// The files its problems are drawn in.
    published: BTreeSet<String>,
}

thread_local! {
    /// hledger run with `LC_ALL=C.UTF-8`: set once it could not read a
    /// journal's text in a locale that is not UTF-8 ([`run_input`]).
    static UTF8: Cell<bool> = const { Cell::new(false) };
    /// The documents open, by number: their files.
    static DOCS: RefCell<BTreeMap<u64, String>> = const { RefCell::new(BTreeMap::new()) };
    /// The names each main journal knows, as its tool listed them, for
    /// completion.
    static KNOWN: RefCell<BTreeMap<String, crate::complete::Known>> = const { RefCell::new(BTreeMap::new()) };
    /// Each journal's main journal, as completion last found it.
    static MAIN_OF: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
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
    /// The new transaction asked for, waiting for the plugin's command
    /// that writes it (the editor is reached only while a command runs).
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
    /// The journal last opened or gone to in this session: where a
    /// statement is imported when nothing names a journal.
    static LAST_JOURNAL: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Text to append at a file's end once it is the current document
    /// (`pta.append`): the file, the text, the undo step's name.
    static APPEND: RefCell<Option<(String, String, String)>> = const { RefCell::new(None) };
    /// `hledger files`' answers by folder, until a journal is saved.
    static FILES: RefCell<BTreeMap<String, Files>> = const { RefCell::new(BTreeMap::new()) };
    /// The plugin's commands waiting for a file to be the current
    /// document: Kalem opens a file `file.open` asks for after the
    /// commands queued with it have run.
    static WAITING: RefCell<Vec<(String, &'static str)>> = const { RefCell::new(Vec::new()) };
    /// A journal block's output, waiting for the command that writes it:
    /// the document's file, the block's start, the output.
    static BLOCK: RefCell<Option<(Option<String>, usize, String)>> = const { RefCell::new(None) };
    /// The programs running, in their order.
    static RUNNING: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    /// What the status bar item shows: the tool and the main journal.
    static SHOWN: RefCell<Option<(Dialect, Main)>> = const { RefCell::new(None) };
}

/// A new transaction to write.
#[derive(Debug)]
struct Pending {
    /// The journal it goes in.
    path: String,
    dialect: Dialect,
    date: Date,
    /// Its text, and where the cursor goes in it.
    text: String,
    cursor: usize,
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
    if d == Dialect::Beancount {
        return format!(
            "{}; or give their paths as the settings programs.bean-check and programs.bean-query",
            d.install()
        );
    }
    format!(
        "{}; or give its path as the setting programs.{}",
        d.install(),
        d.checker()
    )
}

/// Why a program did not run, or what it printed is not taken.
enum NotRun {
    /// Kalem refused the run (no project, too many runs): its words.
    Refused(String),
    /// The program could not start: not installed.
    Missing,
    /// It printed more than Kalem takes (`process`'s 16 MB): cut, so not
    /// shown, and the user told once ([`run`]).
    TooLong,
}

/// What `program` printed, whole: an output Kalem cut is refused, with a
/// notice naming a shorter period, rather than shown cut.
fn whole(program: &str, e: process::Exit) -> Result<process::Exit, NotRun> {
    if e.truncated {
        ui::notify(
            &format!(
                "{program} printed more than Kalem takes (16 MB): a shorter period shows less"
            ),
            ui::Level::Warning,
        );
        return Err(NotRun::TooLong);
    }
    Ok(e)
}

/// Runs `program` with `args` in `dir`, or, where Kalem refuses that
/// folder (a plugin's programs run in the projects' folders only), in
/// the first project's; `done` gets how it ended. While it runs, the
/// status bar item names it.
fn run(
    program: &'static str,
    args: Vec<String>,
    dir: &str,
    done: impl FnOnce(Result<process::Exit, NotRun>) + 'static,
) {
    run_input(program, args, dir, None, done);
}

/// [`run`], `input` given on the program's standard input. hledger in a
/// locale that is not UTF-8 (a Linux session without `LANG`; Debian's
/// hledger 1.32 reads by the locale) cannot read a journal's letters past
/// ASCII: when it says so it runs once more with `LC_ALL=C.UTF-8`, and so
/// from then on, a locale that works left as it is.
fn run_input(
    program: &'static str,
    args: Vec<String>,
    dir: &str,
    input: Option<Vec<u8>>,
    done: impl FnOnce(Result<process::Exit, NotRun>) + 'static,
) {
    let utf8 = program == "hledger" && UTF8.with(Cell::get);
    let done = Rc::new(RefCell::new(Some(done)));
    running(program, true);
    let again = (args.clone(), dir.to_string(), input.clone());
    let finish = move |r: Result<process::Exit, NotRun>| {
        running(program, false);
        let Some(f) = done.borrow_mut().take() else {
            return;
        };
        if program == "hledger"
            && !utf8
            && let Ok(e) = &r
            && e.status != Some(0)
            && journal::undecoded(&String::from_utf8_lossy(&e.stderr))
        {
            UTF8.with(|u| u.set(true));
            let (args, dir, input) = again.clone();
            return run_input(program, args, &dir, input, f);
        }
        f(r);
    };
    let with_input = |c: process::Command| {
        let c = if utf8 { c.env("LC_ALL", "C.UTF-8") } else { c };
        match &input {
            Some(i) => c.stdin(i.clone()),
            None => c,
        }
    };
    let here = with_input(process::Command::new(program).args(args.clone()).cwd(dir));
    let first = finish.clone();
    let taken = move |r: Result<process::Exit, String>| {
        r.map_err(|_| NotRun::Missing)
            .and_then(|e| whole(program, e))
    };
    if process::run(&here, move |r| first(taken(r))).is_err() {
        let anywhere = with_input(process::Command::new(program).args(args));
        let second = finish.clone();
        if let Err(why) = process::run(&anywhere, move |r| second(taken(r))) {
            finish(Err(NotRun::Refused(why)));
        }
    }
}

/// `program` started or ended: the status bar item names the programs
/// running (`hledger · all.journal · ✓ · hledger…`), the long runs of a
/// large journal seen.
fn running(program: &'static str, started: bool) {
    RUNNING.with(|r| {
        let mut r = r.borrow_mut();
        if started {
            r.push(program);
        } else if let Some(i) = r.iter().position(|p| *p == program) {
            r.remove(i);
        }
    });
    if let Some((tool, main)) = SHOWN.with(|s| s.borrow().clone()) {
        show(tool, &main);
    }
}

/// Main Journal and Tool…: the settings `journal` and `tool` from a
/// pick, in a journal: this file as the main journal, another typed, or
/// none (`LEDGER_FILE`'s, else each file itself); the tool the file's
/// own (`auto`), hledger or Ledger. The status bar item follows.
fn setup() -> Result<String, String> {
    let Some((_, Some(path))) = dialect_here() else {
        ui::notify("This document is no journal with a file", ui::Level::Info);
        return Ok("null".into());
    };
    let s = Settings::read(settings::own);
    let now = |on: bool| on.then(|| "now".to_string());
    let journal = s.journal.trim().to_string();
    let tool = s.tool.trim().to_string();
    let choices: Vec<(String, Option<String>, &'static str)> = vec![
        (
            format!("Main journal: this file, {}", journal::name(&path)),
            now(!journal.is_empty() && journal == path),
            "this",
        ),
        (
            "Main journal: another file…".into(),
            now(!journal.is_empty() && journal != path),
            "other",
        ),
        (
            "Main journal: LEDGER_FILE's, else each file itself".into(),
            now(journal.is_empty()),
            "none",
        ),
        (
            "Tool: the file's own (auto)".into(),
            now(tool.is_empty() || tool == "auto"),
            "auto",
        ),
        ("Tool: hledger".into(), now(tool == "hledger"), "hledger"),
        ("Tool: Ledger".into(), now(tool == "ledger"), "ledger"),
    ];
    let items: Vec<ui::PickItem> = choices
        .iter()
        .map(|(label, detail, _)| ui::PickItem {
            label: label.clone(),
            detail: detail.clone(),
        })
        .collect();
    let keys: Vec<&'static str> = choices.iter().map(|c| c.2).collect();
    ui::quick_pick(
        &items,
        &ui::PickOptions {
            title: Some("Main journal and tool".into()),
            placeholder: None,
            many: false,
        },
        move |picked| {
            let Some(&key) = picked.first().and_then(|&i| keys.get(i as usize)) else {
                return;
            };
            let set = |k: &str, v: serde_json::Value| {
                if let Err(why) = settings::set(k, &v.to_string()) {
                    ui::notify(&why, ui::Level::Warning);
                }
            };
            match key {
                "this" => set("journal", serde_json::json!(path)),
                "none" => set("journal", serde_json::Value::Null),
                "other" => ui::prompt(
                    "The main journal (absolute, from ~/, or beside the file)",
                    &ui::PromptOptions {
                        value: Some(journal),
                        placeholder: None,
                        password: false,
                    },
                    move |answer| {
                        if let Some(a) = answer.filter(|a| !a.trim().is_empty()) {
                            set("journal", serde_json::json!(a.trim()));
                        }
                    },
                ),
                tool => set("tool", serde_json::json!(tool)),
            }
        },
    );
    Ok("null".into())
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
            Err(NotRun::TooLong) => {}
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

/// Enter in a journal: at the end of a transaction's header, a new line
/// with the posting indent (`write::header_newline`), which Kalem's own
/// new line would not give the first posting; else the posting aligned
/// first when `align_on_enter` is on, then Kalem's own new line, which
/// runs once this returns.
fn newline() -> Result<String, String> {
    let sel = editor::selected();
    if sel.anchor == sel.head
        && let Some((d, _)) = dialect_here()
        && let Some(line) = write::header_newline(&editor::text(None), d, sel.head as usize)
    {
        // Inserted at the cursor, which goes after it.
        editor::insert(None, &line);
        editor::transact("New Line");
        return Ok("null".into());
    }
    if align::Settings::read(settings::own).on_enter {
        align(true);
    }
    kalem::run("edit.newline", "{}")?;
    Ok("null".into())
}

fn hide() {
    SHOWN.with(|s| s.borrow_mut().take());
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
        text.push_str(&check::mark(n, glyphs()));
    }
    if let Some(p) = RUNNING.with(|r| r.borrow().first().copied()) {
        text.push_str(" · ");
        text.push_str(p);
        text.push('…');
    }
    SHOWN.with(|s| *s.borrow_mut() = Some((tool, main.clone())));
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
        ledger_files(dir, move |listed| {
            done(tool, journal::main_journal("", &path, listed.as_deref()));
        });
        return;
    }
    done(tool, journal::main_journal(&set, &path, None));
}

/// `hledger files` without `-f`, run in `dir`: the journal LEDGER_FILE
/// names and its includes. hledger reads the whole journal for it (some
/// 9 s of a processor for 200,000 transactions), so the answer is kept
/// until a journal is saved, and asked once while it is on its way.
fn ledger_files(dir: String, done: impl FnOnce(Option<Vec<String>>) + 'static) {
    let mut done: Option<Waiting> = Some(Box::new(done));
    // The answer known, or none and whether this call asks for it.
    let (known, ask) = FILES.with(|f| {
        let mut f = f.borrow_mut();
        match f.get_mut(&dir) {
            Some(Files::Known(l)) => (Some(l.clone()), false),
            Some(Files::Asking(waiting)) => {
                waiting.extend(done.take());
                (None, false)
            }
            None => {
                let waiting = done.take().into_iter().collect();
                f.insert(dir.clone(), Files::Asking(waiting));
                (None, true)
            }
        }
    });
    if let (Some(listed), Some(d)) = (known, done.take()) {
        return d(listed);
    }
    if !ask {
        return;
    }
    let d = dir.clone();
    run("hledger", vec!["files".into()], &dir, move |r| {
        let listed = r
            .ok()
            .filter(|e| e.status == Some(0))
            .map(|e| journal::files_listed(&e.stdout));
        let waiting =
            FILES.with(
                |f| match f.borrow_mut().insert(d, Files::Known(listed.clone())) {
                    Some(Files::Asking(w)) => w,
                    _ => Vec::new(),
                },
            );
        for w in waiting {
            w(listed.clone());
        }
    });
}

/// One waiting for `hledger files`' answer.
type Waiting = Box<dyn FnOnce(Option<Vec<String>>)>;

/// `hledger files`' answer for a folder: known, or asked with those
/// waiting for it.
enum Files {
    Known(Option<Vec<String>>),
    Asking(Vec<Waiting>),
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
    // The current document's file and text, where a command runs: the
    // problems' columns in it are turned into the editor's.
    let known = editor::document()
        .and_then(|d| d.path)
        .map(|p| (p, editor::text(None)));
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
                    publish(&main.path, tool, &problems, known.as_ref());
                    let report = check::report(&said, &main.path, &problems, glyphs());
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
                Err(NotRun::TooLong) => {}
                Err(NotRun::Refused(why)) => {
                    if open {
                        ui::notify(&format!("{}: {why}", tool.checker()), ui::Level::Warning);
                    }
                }
            }
        });
    });
}

/// Draws the check's problems in their files as a language server's
/// diagnostics are (plugin API 0.2.14): each file's in place of the last
/// check's, and none left in a file that has none now. A column in the
/// file whose text is `known` counts characters as the editor does
/// (hledger's counts a tab to the next multiple of 8).
fn publish(main: &str, tool: Dialect, found: &[Problem], known: Option<&(String, String)>) {
    use kalem_plugin::problems as drawn;
    let marks = check::marks(main, found, |file| {
        known
            .filter(|(path, _)| journal::same_file(path, file))
            .map(|(_, text)| text.as_str())
    });
    let before = CHECKS.with(|c| {
        c.borrow_mut()
            .get_mut(main)
            .map(|e| std::mem::take(&mut e.published))
            .unwrap_or_default()
    });
    for file in before.iter().filter(|f| !marks.contains_key(*f)) {
        let _ = drawn::set(file, &[]);
    }
    let mut now = BTreeSet::new();
    for (file, list) in marks {
        let list: Vec<drawn::Problem> = list
            .into_iter()
            .take(MAX_DRAWN)
            .map(|d| drawn::Problem {
                line: d.line,
                column: d.column,
                length: d.length,
                severity: drawn::Severity::Error,
                message: d.message,
                source: Some(tool.checker().to_string()),
            })
            .collect();
        if drawn::set(&file, &list).is_ok() {
            now.insert(file);
        }
    }
    CHECKS.with(|c| {
        if let Some(e) = c.borrow_mut().get_mut(main) {
            e.published = now;
        }
    });
}

/// The names the main journal of the journal at `path` knows (its
/// accounts, payees and commodities), listed by its tool for completion;
/// again on each save.
fn learn(path: String) {
    let Some(dialect) = Dialect::of_path(&path) else {
        return;
    };
    let file = path.clone();
    resolve(path, dialect, move |tool, main| {
        MAIN_OF.with(|m| m.borrow_mut().insert(file, main.path.clone()));
        let dir = journal::folder(&main.path).to_string();
        for (slot, program, args) in crate::complete::list_commands(tool, &main.path) {
            let key = main.path.clone();
            run(program, args, &dir, move |r| {
                if let Ok(e) = r
                    && e.status == Some(0)
                {
                    let names = crate::complete::list_of(tool, &String::from_utf8_lossy(&e.stdout));
                    KNOWN.with(|k| k.borrow_mut().entry(key).or_default().set(slot, names));
                }
            });
        }
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
        Ok(n) => {
            CHECKS.with(|c| {
                if let Some(e) = c.borrow_mut().get_mut(&main.path) {
                    e.doc = Some(n);
                }
            });
            owned(n, &main.path);
        }
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
/// that one's again; in the Balance, a Register, a Reconcile or a
/// Report, the journal they are of.
fn check_command() -> Result<String, String> {
    if let Some(main) = checked_here().or_else(document_journal) {
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

/// The journal last opened or gone to.
fn last_journal() -> Option<String> {
    LAST_JOURNAL.with(|l| l.borrow().clone())
}

fn seen_journal(path: &str) {
    if Dialect::of_path(path).is_some() {
        LAST_JOURNAL.with(|l| *l.borrow_mut() = Some(path.to_string()));
    }
}

/// The cursor is in document `n` now.
fn current(n: u64) {
    if CURRENT.with(|c| c.replace(Some(n))) == Some(n) {
        return;
    }
    let path = DOCS.with(|d| d.borrow().get(&n).cloned());
    if let Some(p) = &path {
        seen_journal(p);
    }
    // One of the plugin's own documents: the item keeps naming the
    // journal it came from.
    if path.is_none() && own_document(n) {
        return;
    }
    refresh(path);
}

/// The plugin's document `n` just opened, of the journal `main`: the
/// item names that journal while it is current (the editor may make it
/// current before the plugin knows it is its own).
fn owned(n: u64, main: &str) {
    if CURRENT.with(Cell::get) == Some(n) {
        refresh(Some(main.to_string()));
    }
}

/// Whether document `n` is one the plugin writes.
fn own_document(n: u64) -> bool {
    CHECKS.with(|c| c.borrow().values().any(|e| e.doc == Some(n)))
        || balance::has_doc(n)
        || register::has_doc(n)
        || reconcile::has_doc(n)
        || other::has_doc(n)
        || import::has_doc(n)
}

/// The main journal of the plugin's document that is current: the
/// Balance's, a Register's, a Reconcile's, a Report's.
fn document_journal() -> Option<String> {
    balance::here()
        .or_else(|| register::here().map(|k| k.0))
        .or_else(|| reconcile::here().map(|k| k.0))
        .or_else(|| other::here().map(|k| k.0))
}

fn current_path() -> Option<String> {
    let n = CURRENT.with(Cell::get)?;
    DOCS.with(|d| d.borrow().get(&n).cloned())
}

/// Today, as the user's calendar has it.
fn today() -> Date {
    Date::of_millis(kalem_plugin::clock::local_now())
}

/// A command's argument `key` (JSON, `kalem run … --args '{"account":
/// "assets:bank"}'`), as text.
fn arg(args: &str, key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args).ok()?;
    match &v[key] {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The argument `period`: a name of the pick (`this month`, `last
/// year`, `all dates`) or dates (`2026`, `2026-Q1`,
/// `2026-01-15..2026-02-14`).
fn period_arg(args: &str) -> Option<crate::report::Period> {
    let name = arg(args, "period")?;
    let p = crate::report::Period::typed(&name);
    if p.is_none() {
        ui::notify(&format!("{name}: {PERIOD_HELP}"), ui::Level::Error);
    }
    p
}

/// What a period may be, said when one typed is not.
const PERIOD_HELP: &str = "a period is this month, last month, this quarter, this year, last year, all dates, or dates: 2026, 2026-03, 2026-Q1, 2026-03-05, 2026-01..2026-03 (January to March)";

/// Picks a period for `title`: one of the pick's names, or `Dates…`
/// typed (`now` offered when it is typed dates); `then` runs with the
/// one chosen.
fn pick_period(
    title: &str,
    now: crate::report::Period,
    then: impl FnOnce(crate::report::Period) + 'static,
) {
    use crate::report::Period;
    let mut items: Vec<ui::PickItem> = Period::ALL
        .iter()
        .map(|p| ui::PickItem {
            label: p.name(),
            detail: None,
        })
        .collect();
    items.push(ui::PickItem {
        label: "Dates…".into(),
        detail: Some("2026, 2026-03, 2026-Q1, 2026-01-15..2026-02-14".into()),
    });
    let title = title.to_string();
    ui::quick_pick(
        &items,
        &ui::PickOptions {
            title: Some(title.clone()),
            placeholder: None,
            many: false,
        },
        move |picked| {
            let Some(&i) = picked.first() else { return };
            if let Some(&p) = Period::ALL.get(i as usize) {
                then(p);
                return;
            }
            let value = matches!(now, Period::Dates(..)).then(|| now.name());
            ui::prompt(
                &format!("{title}: dates"),
                &ui::PromptOptions {
                    value,
                    placeholder: Some("2026-Q1, or 2026-01..2026-03 (January to March)".into()),
                    password: false,
                },
                move |answer| {
                    let Some(text) = answer else { return };
                    match Period::typed(&text) {
                        Some(p) => then(p),
                        None => ui::notify(
                            &format!("{}: {PERIOD_HELP}", text.trim()),
                            ui::Level::Warning,
                        ),
                    }
                },
            );
        },
    );
}

/// The setting `glyphs`: the documents' marks.
fn glyphs() -> crate::Glyphs {
    crate::Glyphs::of_setting(settings::own("glyphs").as_deref())
}

/// The setting `insert_at`: `end` puts a new transaction at the end of
/// the journal, else at its date's place.
fn at_end() -> bool {
    settings::own("insert_at").as_deref() == Some("\"end\"")
}

/// Applies `edits` (places of the text as the command found it) as one
/// undo step named `label`.
fn edit(edits: &[(Range<usize>, String)], label: &str) {
    // In a CRLF journal, what is written ends its lines so too.
    let ended;
    let edits = if edits.iter().any(|(_, s)| s.contains('\n')) {
        ended = write::edits_ended_as(&editor::text(None), edits.to_vec());
        &ended[..]
    } else {
        edits
    };
    for (r, s) in edits {
        let done = if r.is_empty() {
            editor::insert(Some(r.start as u64), s);
            Ok(())
        } else {
            editor::replace(
                editor::Range {
                    start: r.start as u64,
                    end: r.end as u64,
                },
                s,
            )
        };
        if let Err(why) = done {
            ui::notify(&why, ui::Level::Warning);
            return;
        }
    }
    if !edits.is_empty() {
        editor::transact(label);
    }
}

/// Puts the cursor at byte `at` of `text` (the document's text once the
/// command's edits are made), through `file.open`, which runs after
/// them.
fn put_cursor(path: &str, text: &str, at: usize) {
    let at = at.min(text.len());
    let line = text[..at].matches('\n').count() + 1;
    let column = at - text[..at].rfind('\n').map_or(0, |i| i + 1);
    let args = serde_json::json!({ "path": path, "line": line, "column": column });
    if let Err(why) = kalem::run("file.open", &args.to_string()) {
        ui::notify(&why, ui::Level::Warning);
    }
}

/// The journal the command runs in: its dialect, file and text, and the
/// selection; none (and said) elsewhere.
fn journal_here() -> Option<(Dialect, String, String, Range<usize>)> {
    let Some((d, Some(path))) = dialect_here() else {
        ui::notify("This document is no journal with a file", ui::Level::Info);
        return None;
    };
    let text = editor::text(None);
    let sel = editor::selected();
    let selection = sel.anchor.min(sel.head) as usize..sel.anchor.max(sel.head) as usize;
    Some((d, path, text, selection))
}

/// Toggle Status (`SPC m t`): the transaction at the cursor (or its
/// posting, with `toggle_postings`), or those selected.
fn toggle_status() -> Result<String, String> {
    let Some((d, _, text, sel)) = journal_here() else {
        return Ok("null".into());
    };
    let postings = settings::own("toggle_postings").as_deref() == Some("true");
    edit(&write::toggle(&text, d, sel, postings), "Toggle Status");
    Ok("null".into())
}

/// Delete Transaction (`SPC m d`, Doom's): the transaction at the cursor
/// with its comment lines above it and a blank line after it, one undo
/// step.
fn delete_transaction() -> Result<String, String> {
    let Some((d, _, text, _)) = journal_here() else {
        return Ok("null".into());
    };
    let at = editor::selected().head as usize;
    match write::deletion(&text, d, at) {
        Some(r) => edit(&[(r, String::new())], "Delete Transaction"),
        None => ui::notify("No transaction at the cursor", ui::Level::Info),
    }
    Ok("null".into())
}

/// The next transaction, or the previous one.
fn neighbour(forward: bool) -> Result<String, String> {
    let Some((d, path, text, sel)) = journal_here() else {
        return Ok("null".into());
    };
    let at = editor::selected().head as usize;
    let _ = sel;
    match write::neighbour(&text, d, at, forward) {
        Some(to) => put_cursor(&path, &text, to),
        None => ui::notify(
            if forward {
                "No transaction after this one"
            } else {
                "No transaction before this one"
            },
            ui::Level::Info,
        ),
    }
    Ok("null".into())
}

/// Copy Transaction (`SPC m y`): the transaction at the cursor again,
/// dated today, at its date's place; the cursor on the copy.
fn copy_transaction() -> Result<String, String> {
    let Some((d, path, text, _)) = journal_here() else {
        return Ok("null".into());
    };
    let at = editor::selected().head as usize;
    let Some((place, ins, start)) = write::copy(&text, d, at, today(), at_end()) else {
        ui::notify("No transaction here", ui::Level::Info);
        return Ok("null".into());
    };
    edit(&[(place..place, ins.clone())], "Copy Transaction");
    let after = format!("{}{}{}", &text[..place], ins, &text[place..]);
    put_cursor(&path, &after, place + start);
    Ok("null".into())
}

/// The transaction's date a day later, or earlier.
fn shift_date(days: i64) -> Result<String, String> {
    let Some((d, _, text, _)) = journal_here() else {
        return Ok("null".into());
    };
    let at = editor::selected().head as usize;
    match write::shift_date(&text, d, at, days) {
        Some(e) => edit(
            &[e],
            if days > 0 {
                "Date a Day Later"
            } else {
                "Date a Day Earlier"
            },
        ),
        None => ui::notify("No transaction here", ui::Level::Info),
    }
    Ok("null".into())
}

/// Sort Transactions (`SPC m s`): those selected, or the file's, by date.
fn sort_transactions() -> Result<String, String> {
    let Some((d, path, text, sel)) = journal_here() else {
        return Ok("null".into());
    };
    match write::sort(&text, d, sel) {
        Ok(Some((r, sorted))) => {
            // The cursor stays in its transaction, wherever it went.
            let at = editor::selected().head as usize;
            let txs = write::transactions(&text, d);
            let mine = write::at(&txs, at).map(|t| {
                (
                    text[t.with_comments.clone()].to_string(),
                    at - t.with_comments.start,
                )
            });
            edit(&[(r.clone(), sorted.clone())], "Sort Transactions");
            if let Some((own, offset)) = mine
                && let Some(i) = sorted.find(&own)
            {
                let after = format!("{}{}{}", &text[..r.start], sorted, &text[r.end..]);
                put_cursor(&path, &after, r.start + i + offset);
            }
        }
        Ok(None) => ui::notify("The transactions are in date order", ui::Level::Info),
        Err(why) => ui::notify(&why, ui::Level::Warning),
    }
    Ok("null".into())
}

/// New Transaction (`SPC m a`): its date asked (today's offered; smart
/// dates taken), then its payee from the journal's, the recent first,
/// or another typed; the tool's draft from the payee's last transaction
/// then written at the date's place by `pta.insertTransaction`.
fn new_transaction() -> Result<String, String> {
    let Some((d, path, text, _)) = journal_here() else {
        return Ok("null".into());
    };
    let today = today();
    let sep = write::separator(&text, d);
    let recent = write::payees_in(&text, d);
    ui::prompt(
        "Date of the new transaction",
        &ui::PromptOptions {
            value: Some(today.written(sep)),
            placeholder: Some("today, yesterday, 10/5, last friday, -3".into()),
            password: false,
        },
        move |answer| {
            let Some(typed) = answer else {
                return;
            };
            let Some(date) = write::smart_date(&typed, today) else {
                ui::notify(&format!("Not a date: {typed}"), ui::Level::Warning);
                return;
            };
            choose_payee(d, path, sep, date, recent);
        },
    );
    Ok("null".into())
}

/// The payees the tool knows, after the journal's recent ones; then the
/// draft for the one chosen.
fn choose_payee(d: Dialect, path: String, sep: char, date: Date, recent: Vec<String>) {
    resolve(path.clone(), d, move |tool, main| {
        let (program, args) = write::payees_command(tool, &main.path);
        let dir = journal::folder(&main.path).to_string();
        run(program, args, &dir, move |r| {
            let mut payees = recent;
            if let Ok(e) = r
                && e.status == Some(0)
            {
                let listed = String::from_utf8_lossy(&e.stdout).into_owned();
                let lines = listed.lines().skip(usize::from(tool == Dialect::Beancount));
                for p in lines {
                    let p = p.trim().trim_matches('"').to_string();
                    if !p.is_empty() && !payees.contains(&p) {
                        payees.push(p);
                    }
                }
            }
            let mut items: Vec<ui::PickItem> = payees
                .iter()
                .map(|p| ui::PickItem {
                    label: p.clone(),
                    detail: None,
                })
                .collect();
            items.push(ui::PickItem {
                label: "Another payee…".into(),
                detail: Some("a payee not in the journal yet".into()),
            });
            ui::quick_pick(
                &items,
                &ui::PickOptions {
                    title: Some(format!("Payee of the transaction of {}", date.written(sep))),
                    placeholder: None,
                    many: false,
                },
                move |picked| {
                    let Some(&i) = picked.first() else {
                        return;
                    };
                    let i = i as usize;
                    if let Some(payee) = payees.get(i).cloned() {
                        draft(d, tool, main, path, sep, date, payee);
                    } else {
                        ui::prompt(
                            "Payee",
                            &ui::PromptOptions {
                                value: None,
                                placeholder: None,
                                password: false,
                            },
                            move |answer| {
                                if let Some(payee) = answer.filter(|p| !p.trim().is_empty()) {
                                    pend(
                                        d,
                                        path,
                                        date,
                                        write::new_transaction(d, date, sep, payee.trim(), None),
                                    );
                                }
                            },
                        );
                    }
                },
            );
        });
    });
}

/// The tool's draft of a transaction of `payee` (its last one), then the
/// new transaction written.
fn draft(
    d: Dialect,
    tool: Dialect,
    main: Main,
    path: String,
    sep: char,
    date: Date,
    payee: String,
) {
    let (program, args) = write::draft_command(tool, &main.path, &payee, date);
    let dir = journal::folder(&main.path).to_string();
    run(program, args, &dir, move |r| {
        let drafted = r
            .ok()
            .filter(|e| e.status == Some(0))
            .map(|e| String::from_utf8_lossy(&e.stdout).into_owned());
        pend(
            d,
            path,
            date,
            write::new_transaction(d, date, sep, &payee, drafted.as_deref()),
        );
    });
}

/// Keeps the new transaction and runs the command that writes it.
fn pend(dialect: Dialect, path: String, date: Date, (text, cursor): (String, usize)) {
    PENDING.with(|p| {
        *p.borrow_mut() = Some(Pending {
            path,
            dialect,
            date,
            text,
            cursor,
        });
    });
    if let Err(why) = kalem::run("pta.insertTransaction", "{}") {
        ui::notify(&why, ui::Level::Warning);
    }
}

/// Writes the new transaction asked for, when its journal is the
/// current document: at its date's place, one undo step, the cursor on
/// its first amount.
fn insert_transaction() -> Result<String, String> {
    let Some(p) = PENDING.with(|p| p.borrow_mut().take()) else {
        return Ok("null".into());
    };
    let Some((_, Some(path))) = dialect_here() else {
        ui::notify(
            "The journal is no longer the current document",
            ui::Level::Warning,
        );
        return Ok("null".into());
    };
    if !journal::same_file(&path, &p.path) {
        ui::notify(
            "The journal is no longer the current document",
            ui::Level::Warning,
        );
        return Ok("null".into());
    }
    let text = editor::text(None);
    let place = write::place(&text, p.dialect, p.date, at_end());
    let (ins, start) = write::insertion(&text, place, &p.text);
    edit(&[(place..place, ins.clone())], "New Transaction");
    let after = format!("{}{}{}", &text[..place], ins, &text[place..]);
    put_cursor(&path, &after, place + start + p.cursor);
    Ok("null".into())
}

/// Opens `path` and runs the plugin's `command` once it is the current
/// document: at once where it is already, else when Kalem has opened it
/// (it opens a file after the commands queued with `file.open` have run).
fn open_then(path: &str, command: &'static str) {
    WAITING.with(|w| w.borrow_mut().push((path.to_string(), command)));
    let args = serde_json::json!({ "path": path });
    let _ = kalem::run("file.open", &args.to_string());
    if let Err(why) = kalem::run(command, "{}") {
        ui::notify(&why, ui::Level::Warning);
    }
}

/// Where a command meant for the file `path` is: there, or waiting for
/// it to open, or elsewhere.
#[derive(Debug, PartialEq, Eq)]
enum Here {
    There,
    Waiting,
    Elsewhere,
}

/// Whether the current document is `path` (the command's wait over), or
/// one is waiting for it ([`open_then`]).
fn here_is(path: &str) -> Here {
    let here = editor::document().and_then(|d| d.path);
    if here.is_some_and(|h| journal::same_file(&h, path)) {
        WAITING.with(|w| w.borrow_mut().retain(|(p, _)| !journal::same_file(p, path)));
        return Here::There;
    }
    if WAITING.with(|w| w.borrow().iter().any(|(p, _)| journal::same_file(p, path))) {
        Here::Waiting
    } else {
        Here::Elsewhere
    }
}

/// A document of `path` opened or current: the commands waiting for it
/// run.
fn arrived(path: &str) {
    let commands: Vec<&'static str> = WAITING.with(|w| {
        w.borrow()
            .iter()
            .filter(|(p, _)| journal::same_file(p, path))
            .map(|(_, c)| *c)
            .collect()
    });
    for c in commands {
        if let Err(why) = kalem::run(c, "{}") {
            ui::notify(&why, ui::Level::Warning);
        }
    }
}

/// Run the Journal Block (`SPC m x` in an Org document's `hledger`,
/// `ledger` or `beancount` block): its text given to its tool with its
/// `:cmdline`, the output written under it as `#+RESULTS:`.
fn run_block() -> Result<String, String> {
    let doc = editor::document();
    let path = doc.as_ref().and_then(|d| d.path.clone());
    let is_org = doc.as_ref().is_some_and(|d| d.mode == "org");
    let text = editor::text(None);
    let at = editor::selected().head as usize;
    let Some(b) = is_org.then(|| crate::blocks::block_at(&text, at)).flatten() else {
        ui::notify(
            "Run the Journal Block runs in an Org document's hledger, ledger or beancount source block",
            ui::Level::Info,
        );
        return Ok("null".into());
    };
    let (program, args) = crate::blocks::command(&b);
    // Its code as Kalem's Babel gives it: without Org's indentation and
    // escapes.
    let input = crate::blocks::code(&text, &b).into_bytes();
    let dir = path
        .as_deref()
        .map(journal::folder)
        .unwrap_or("")
        .to_string();
    let start = b.start;
    run_input(program, args, &dir, Some(input), move |r| {
        let said = match r {
            Ok(e) if e.status == Some(0) => String::from_utf8_lossy(&e.stdout).into_owned(),
            Ok(e) => String::from_utf8_lossy(&e.stderr).into_owned(),
            Err(NotRun::Missing) => {
                return ui::notify(&missing(b.dialect), ui::Level::Warning);
            }
            Err(NotRun::Refused(why)) => {
                return ui::notify(&format!("{program}: {why}"), ui::Level::Warning);
            }
            Err(NotRun::TooLong) => return,
        };
        BLOCK.with(|r| *r.borrow_mut() = Some((path, start, said)));
        if let Err(why) = kalem::run("pta.runBlockWrite", "{}") {
            ui::notify(&why, ui::Level::Warning);
        }
    });
    Ok("null".into())
}

/// Writes the block's results [`run_block`] keeps, in its document.
fn write_block() -> Result<String, String> {
    let Some((path, start, said)) = BLOCK.with(|r| r.borrow_mut().take()) else {
        return Ok("null".into());
    };
    let here = editor::document().and_then(|d| d.path);
    if path.is_some()
        && !here
            .zip(path.as_ref())
            .is_some_and(|(h, p)| journal::same_file(&h, p))
    {
        ui::notify(
            "The block's document is no longer the current one",
            ui::Level::Warning,
        );
        return Ok("null".into());
    }
    let text = editor::text(None);
    // The block where it was: its `#+begin_src` line's start, or after.
    let Some(b) = crate::blocks::block_at(&text, start.min(text.len())) else {
        return Ok("null".into());
    };
    let (range, ins) = crate::blocks::write(&text, &b, &said);
    edit(&[(range, ins)], "Run the Journal Block");
    Ok("null".into())
}

/// Appends `text` at the end of `path`, opened for it, as one undo step
/// named `title`: the plugin's command `pta.append` writes it, the editor
/// being reached only while a command runs.
fn append(path: String, text: String, title: String) {
    APPEND.with(|a| *a.borrow_mut() = Some((path.clone(), text, title)));
    open_then(&path, "pta.append");
}

/// Writes what [`append`] keeps at the end of the current document, when
/// it is the file it was meant for (waiting while that opens).
fn write_append() -> Result<String, String> {
    let Some(path) = APPEND.with(|a| a.borrow().as_ref().map(|(p, _, _)| p.clone())) else {
        return Ok("null".into());
    };
    match here_is(&path) {
        Here::There => {}
        Here::Waiting => return Ok("null".into()),
        Here::Elsewhere => {
            APPEND.with(|a| a.borrow_mut().take());
            ui::notify(
                &format!("{} is no longer the current document", journal::name(&path)),
                ui::Level::Warning,
            );
            return Ok("null".into());
        }
    }
    let Some((_, text, title)) = APPEND.with(|a| a.borrow_mut().take()) else {
        return Ok("null".into());
    };
    let current = editor::text(None);
    let at = current.len();
    let (ins, _) = write::insertion(&current, at, &text);
    editor::insert(Some(at as u64), &write::ended_as(&current, &ins));
    editor::transact(&title);
    Ok("null".into())
}

/// The Journal button's and `SPC m m`'s pick: the journal's commands
/// (`crate::MENU`) with their keys, or, in the Check document, its own;
/// the one chosen runs where the pick was asked, once it is answered.
fn menu() -> Result<String, String> {
    let entries: Vec<(&str, &str, &str)> = if import::here().is_some() {
        vec![
            ("pta.importAccept", "Import", "i"),
            ("pta.editRules", "Edit the Rules", "e"),
            ("pta.import", "Import Dry Run Again", "r"),
            ("pta.importClose", "Close the Import", "q"),
        ]
    } else if reconcile::here().is_some() {
        vec![
            ("pta.reconcileMark", "Mark Pending or Not", "Tab"),
            ("pta.reconcileVisit", "Go to the Posting", "Enter"),
            ("pta.reconcileTarget", "Statement Balance…", "t"),
            ("pta.finishReconcile", "Finish Reconciliation", "C"),
            ("pta.reconcile", "Reconcile Again", "r"),
            ("pta.reconcileClose", "Close the Reconciliation", "q"),
        ]
    } else if other::here().is_some() {
        vec![
            ("pta.reportPeriod", "Period…", "p"),
            ("pta.report", "Another Report…", "o"),
            ("pta.reportAgain", "Report Again", "r"),
            ("pta.reportClose", "Close the Report", "q"),
        ]
    } else if register::here().is_some() {
        vec![
            ("pta.registerVisit", "Go to the Posting", "Enter"),
            ("pta.registerPeriod", "Period…", "p"),
            ("pta.registerValue", "Market Value or Cost", "v"),
            ("pta.registerNarrow", "Narrow…", "/"),
            ("pta.register", "Register Again", "r"),
            ("pta.registerClose", "Close the Register", "q"),
        ]
    } else if balance::here().is_some() {
        vec![
            ("pta.register", "Register of the Account", "Enter"),
            ("pta.balanceToggle", "Fold or Unfold", "Tab"),
            ("pta.balancePeriod", "Period…", "p"),
            ("pta.balanceDepth", "Depth…", "d"),
            ("pta.balanceValue", "Market Value or Cost", "v"),
            ("pta.balance", "Balance Again", "r"),
            ("pta.balanceClose", "Close the Balance", "q"),
        ]
    } else if checked_here().is_some() {
        vec![
            ("pta.checkOpen", "Open the Problem's Place", "Enter"),
            ("pta.check", "Check Again", "r"),
            ("pta.checkClose", "Close the Check", "q"),
        ]
    } else {
        crate::MENU
            .iter()
            .copied()
            .filter(|(id, ..)| *id != "-")
            .collect()
    };
    let items: Vec<ui::PickItem> = entries
        .iter()
        .map(|(_, title, keys)| ui::PickItem {
            label: (*title).to_string(),
            detail: (!keys.is_empty()).then(|| (*keys).to_string()),
        })
        .collect();
    let ids: Vec<String> = entries.iter().map(|(id, ..)| (*id).to_string()).collect();
    ui::quick_pick(
        &items,
        &ui::PickOptions {
            title: Some("Journal".into()),
            placeholder: None,
            many: false,
        },
        move |picked| {
            if let Some(id) = picked.first().and_then(|&i| ids.get(i as usize))
                && let Err(why) = kalem::run(id, "{}")
            {
                ui::notify(&why, ui::Level::Warning);
            }
        },
    );
    Ok("null".into())
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
    /// The completer's items (plugin API 0.2.11): accounts, payees and
    /// commodities from the main journal's lists, else from the text.
    fn complete(
        _completer: &str,
        request: &kalem_plugin::completer::Request,
    ) -> Vec<kalem_plugin::completer::Item> {
        use crate::complete::{self as c, Slot};
        use kalem_plugin::completer::{Item, ItemKind};
        let Some(path) = request.path.as_deref() else {
            return Vec::new();
        };
        let Some(d) = Dialect::of_path(path) else {
            return Vec::new();
        };
        let Some(cursor) = request.cursor.checked_sub(request.base) else {
            return Vec::new();
        };
        let cursor = cursor as usize;
        let Some((slot, start)) = c::slot(&request.text, cursor, d) else {
            return Vec::new();
        };
        let typed = &request.text[start..cursor];
        // A Beancount payee's string closed, where nothing closes it yet.
        let after = &request.text[cursor..];
        let close = if d == Dialect::Beancount && slot == Slot::Payee && !after.starts_with('"') {
            "\""
        } else {
            ""
        };
        let main = MAIN_OF
            .with(|m| m.borrow().get(path).cloned())
            .unwrap_or_else(|| path.to_string());
        let known = match KNOWN.with(|k| k.borrow().get(&main).cloned()) {
            Some(k) if !k.of(slot).is_empty() => k,
            _ => c::from_text(&request.text, d),
        };
        c::matches(&known, slot, typed)
            .into_iter()
            .map(|name| Item {
                label: name.clone(),
                insert: format!("{name}{close}"),
                start: request.base + start as u64,
                cursor: None,
                kind: match slot {
                    Slot::Account => ItemKind::Symbol,
                    // Not a word's kind, which Enter leaves to the prose.
                    Slot::Payee => ItemKind::Symbol,
                    Slot::Commodity => ItemKind::Keyword,
                },
                detail: match slot {
                    Slot::Account => "account",
                    Slot::Payee => "payee",
                    Slot::Commodity => "commodity",
                }
                .to_string(),
                documentation: None,
            })
            .collect()
    }
    /// Format Document and format on save (plugin API 0.2.16, K11): the
    /// journal's amounts aligned at `amount_column`, every transaction's,
    /// as Align Amounts aligns a selection.
    fn format(
        _formatter: &str,
        request: &kalem_plugin::formatter::Request,
    ) -> Result<String, kalem_plugin::formatter::Refusal> {
        let d = Dialect::of_language(&request.text_type)
            .or_else(|| request.path.as_deref().and_then(Dialect::of_path))
            .ok_or_else(|| {
                kalem_plugin::formatter::refusal(
                    "Not a journal of hledger, Ledger or Beancount",
                    None,
                )
            })?;
        let column = align::Settings::read(settings::own).column;
        Ok(align::format(&request.text, d, column))
    }
    /// The outline sidebar's headings (plugin API 0.2.18, K5): Org's
    /// headings where the journal writes them, else its years and months.
    fn outline(
        _outline: &str,
        request: &kalem_plugin::outline::Request,
    ) -> Vec<kalem_plugin::outline::Heading> {
        let Some(d) = Dialect::of_language(&request.text_type)
            .or_else(|| request.path.as_deref().and_then(Dialect::of_path))
        else {
            return Vec::new();
        };
        crate::outline::headings(&request.text, d)
            .into_iter()
            .map(|h| kalem_plugin::outline::heading(h.level, &h.title, h.start))
            .collect()
    }
    /// Babel's journal blocks (plugin API 0.2.21, K6): an `hledger`,
    /// `ledger` or `beancount` source block of an Org document run by its
    /// tool, its text on standard input with its `:cmdline` (ob-ledger's;
    /// `:cmd` too), as `org-babel-execute:ledger` runs it.
    fn prepare_block(
        block: &kalem_plugin::executor::Block,
    ) -> Result<kalem_plugin::executor::Step, String> {
        use kalem_plugin::executor;
        let d = Dialect::of_language(&block.language.to_ascii_lowercase())
            .ok_or_else(|| format!("{} is not a journal's language", block.language))?;
        let cmdline = executor::header(block, ":cmdline")
            .or_else(|| executor::header(block, ":cmd"))
            .unwrap_or("");
        let (program, args) = crate::blocks::tool_command(d, cmdline);
        Ok(executor::run(program, args, Some(&block.body)))
    }
    /// The tool's report as the block's value: text, as ob-ledger gives
    /// it, which Babel writes in `: ` lines or an example block.
    fn finish_block(
        _block: &kalem_plugin::executor::Block,
        output: &str,
        _status: Option<i32>,
    ) -> Result<kalem_plugin::executor::Value, String> {
        Ok(kalem_plugin::executor::Value::Text(output.to_string()))
    }
    fn activate() -> Result<(), String> {
        let journal = crate::journal_when();
        let leader = format!("{journal} && vimCommand");
        command("pta.version", "Tool Version", crate::TEXT_TYPES, |_| {
            version()
        })?;
        command(
            "pta.setup",
            "Main Journal and Tool…",
            crate::TEXT_TYPES,
            |_| setup(),
        )?;
        command("pta.align", "Align Amounts", crate::TEXT_TYPES, |_| {
            align(false);
            Ok("null".into())
        })?;
        kalem::keymap("space m =", "pta.align", Some(&leader))?;
        command(
            "pta.newline",
            "New Line, the Posting Aligned",
            crate::TEXT_TYPES,
            |_| newline(),
        )?;
        // In Vim's insert mode too, from the Kalem whose keymap lets a
        // binding written for it (`vimMode == insert`) come before the
        // mode's own Enter (K10); in an older one that binding never runs
        // and `SPC m =` aligns.
        kalem::keymap(
            "enter",
            "pta.newline",
            Some(&format!("{journal} && !vimCommand")),
        )?;
        kalem::keymap(
            "enter",
            "pta.newline",
            Some(&format!("{journal} && vimMode == insert")),
        )?;
        let mut types = crate::TEXT_TYPES.to_vec();
        types.extend([
            CHECK_KIND,
            balance::KIND,
            register::KIND,
            reconcile::KIND,
            other::KIND,
        ]);
        command("pta.check", "Check Journal", &types, |_| check_command())?;
        command(
            "pta.checkSaved",
            "Check the Saved Journal",
            crate::TEXT_TYPES,
            |args| {
                if let Some(path) = arg(args, "path") {
                    check(path, false);
                }
                Ok("null".into())
            },
        )?;
        kalem::keymap("space m k", "pta.check", Some(&leader))?;
        // And from the Balance, a Register, a Reconcile or a Report: their
        // journal's check.
        let reports = [balance::KIND, register::KIND, reconcile::KIND, other::KIND]
            .map(|k| format!("textType == {k}"))
            .join(" || ");
        kalem::keymap(
            "space m k",
            "pta.check",
            Some(&format!("({reports}) && vimCommand")),
        )?;
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
        let t = crate::TEXT_TYPES;
        command("pta.newTransaction", "New Transaction", t, |_| {
            new_transaction()
        })?;
        kalem::keymap("space m a", "pta.newTransaction", Some(&leader))?;
        command(
            "pta.insertTransaction",
            "Write the New Transaction Asked For",
            t,
            |_| insert_transaction(),
        )?;
        command("pta.toggleStatus", "Toggle Status", t, |_| toggle_status())?;
        kalem::keymap("space m t", "pta.toggleStatus", Some(&leader))?;
        command("pta.deleteTransaction", "Delete Transaction", t, |_| {
            delete_transaction()
        })?;
        kalem::keymap("space m d", "pta.deleteTransaction", Some(&leader))?;
        command("pta.copyTransaction", "Copy Transaction", t, |_| {
            copy_transaction()
        })?;
        kalem::keymap("space m y", "pta.copyTransaction", Some(&leader))?;
        command("pta.sort", "Sort Transactions", t, |_| sort_transactions())?;
        kalem::keymap("space m s", "pta.sort", Some(&leader))?;
        command("pta.nextTransaction", "Next Transaction", t, |_| {
            neighbour(true)
        })?;
        command("pta.previousTransaction", "Previous Transaction", t, |_| {
            neighbour(false)
        })?;
        kalem::keymap("alt+down", "pta.nextTransaction", Some(&journal))?;
        kalem::keymap("alt+up", "pta.previousTransaction", Some(&journal))?;
        kalem::keymap("] ]", "pta.nextTransaction", Some(&leader))?;
        kalem::keymap("[ [", "pta.previousTransaction", Some(&leader))?;
        command("pta.dateLater", "Date a Day Later", t, |_| shift_date(1))?;
        command("pta.dateEarlier", "Date a Day Earlier", t, |_| {
            shift_date(-1)
        })?;
        kalem::keymap("ctrl+shift+l", "pta.dateLater", Some(&leader))?;
        kalem::keymap("ctrl+shift+h", "pta.dateEarlier", Some(&leader))?;
        let mut types = crate::TEXT_TYPES.to_vec();
        types.push(balance::KIND);
        command("pta.balance", "Balance", &types, balance::command)?;
        kalem::keymap("space m b", "pta.balance", Some(&leader))?;
        kalem::keymap("space m g b", "pta.balance", Some(&leader))?;
        kalem::keymap("r", "pta.balance", Some(BALANCE_WHEN))?;
        let b = &[balance::KIND];
        command("pta.balanceToggle", "Fold or Unfold", b, |_| {
            balance::fold(None)
        })?;
        kalem::keymap("tab", "pta.balanceToggle", Some(BALANCE_WHEN))?;
        command("pta.balanceFold", "Fold", b, |_| balance::fold(Some(true)))?;
        kalem::keymap("left", "pta.balanceFold", Some(BALANCE_WHEN))?;
        command("pta.balanceUnfold", "Unfold", b, |_| {
            balance::fold(Some(false))
        })?;
        kalem::keymap("right", "pta.balanceUnfold", Some(BALANCE_WHEN))?;
        command("pta.balancePeriod", "Period…", b, |_| {
            balance::pick_period()
        })?;
        kalem::keymap("p", "pta.balancePeriod", Some(BALANCE_WHEN))?;
        command("pta.balanceDepth", "Depth…", b, |_| balance::pick_depth())?;
        kalem::keymap("d", "pta.balanceDepth", Some(BALANCE_WHEN))?;
        command("pta.balanceValue", "Market Value or Cost", b, |_| {
            balance::toggle_value()
        })?;
        kalem::keymap("v", "pta.balanceValue", Some(BALANCE_WHEN))?;
        command("pta.balanceClose", "Close the Balance", b, |_| {
            balance::close()
        })?;
        kalem::keymap("q", "pta.balanceClose", Some(BALANCE_WHEN))?;
        kalem::keymap("escape", "pta.balanceClose", Some(BALANCE_ESCAPE_WHEN))?;
        let mut types = crate::TEXT_TYPES.to_vec();
        types.extend([balance::KIND, register::KIND]);
        command("pta.register", "Register", &types, register::command)?;
        kalem::keymap("space m g r", "pta.register", Some(&leader))?;
        kalem::keymap("enter", "pta.register", Some(BALANCE_WHEN))?;
        kalem::keymap("r", "pta.register", Some(REGISTER_WHEN))?;
        let r = &[register::KIND];
        command("pta.registerVisit", "Go to the Posting", r, |_| {
            register::visit()
        })?;
        kalem::keymap("enter", "pta.registerVisit", Some(REGISTER_WHEN))?;
        command("pta.registerPeriod", "Period…", r, |_| {
            register::pick_period()
        })?;
        kalem::keymap("p", "pta.registerPeriod", Some(REGISTER_WHEN))?;
        command("pta.registerValue", "Market Value or Cost", r, |_| {
            register::toggle_value()
        })?;
        kalem::keymap("v", "pta.registerValue", Some(REGISTER_WHEN))?;
        command("pta.registerNarrow", "Narrow…", r, |_| register::narrow())?;
        kalem::keymap("/", "pta.registerNarrow", Some(REGISTER_WHEN))?;
        command("pta.registerClose", "Close the Register", r, |_| {
            register::close()
        })?;
        kalem::keymap("q", "pta.registerClose", Some(REGISTER_WHEN))?;
        kalem::keymap("escape", "pta.registerClose", Some(REGISTER_ESCAPE_WHEN))?;
        let mut types = crate::TEXT_TYPES.to_vec();
        types.extend([balance::KIND, other::KIND]);
        command("pta.report", "Report…", &types, other::command)?;
        // Doom's `SPC m r`, ledger-report.
        kalem::keymap("space m r", "pta.report", Some(&leader))?;
        kalem::keymap("o", "pta.report", Some(REPORT_WHEN))?;
        let o = &[other::KIND];
        command("pta.reportAgain", "Report Again", o, |_| other::again())?;
        kalem::keymap("r", "pta.reportAgain", Some(REPORT_WHEN))?;
        command("pta.reportPeriod", "Period…", o, |_| other::pick_period())?;
        kalem::keymap("p", "pta.reportPeriod", Some(REPORT_WHEN))?;
        command("pta.reportClose", "Close the Report", o, |_| other::close())?;
        kalem::keymap("q", "pta.reportClose", Some(REPORT_WHEN))?;
        kalem::keymap("escape", "pta.reportClose", Some(REPORT_ESCAPE_WHEN))?;
        command(
            "pta.insertTable",
            "Insert a Report as a Table",
            &["org", "markdown"],
            |_| table::command(),
        )?;
        command(
            "pta.insertTableWrite",
            "Write the Report's Table",
            &["org", "markdown"],
            |_| table::write(),
        )?;
        let mut types = crate::TEXT_TYPES.to_vec();
        types.push(reconcile::KIND);
        command("pta.reconcile", "Reconcile", &types, |_| {
            reconcile::command()
        })?;
        kalem::keymap("space m shift+r", "pta.reconcile", Some(&leader))?;
        kalem::keymap("r", "pta.reconcile", Some(RECONCILE_WHEN))?;
        command(
            "pta.finishReconcile",
            "Finish Reconciliation",
            &types,
            |_| reconcile::finish(),
        )?;
        kalem::keymap("space m shift+c", "pta.finishReconcile", Some(&leader))?;
        kalem::keymap("shift+c", "pta.finishReconcile", Some(RECONCILE_WHEN))?;
        command(
            "pta.finishReconcileWrite",
            "Write the Finished Reconciliation",
            crate::TEXT_TYPES,
            |_| reconcile::finish_write(),
        )?;
        let k = &[reconcile::KIND];
        command("pta.reconcileVisit", "Go to the Posting", k, |_| {
            reconcile::visit()
        })?;
        kalem::keymap("enter", "pta.reconcileVisit", Some(RECONCILE_WHEN))?;
        command("pta.reconcileTarget", "Statement Balance…", k, |_| {
            reconcile::retarget()
        })?;
        kalem::keymap("t", "pta.reconcileTarget", Some(RECONCILE_WHEN))?;
        command("pta.reconcileMark", "Mark Pending or Not", k, |_| {
            reconcile::mark()
        })?;
        // Doom's ledger-reconcile toggle is Tab.
        kalem::keymap("tab", "pta.reconcileMark", Some(RECONCILE_WHEN))?;
        command(
            "pta.reconcileMarkWrite",
            "Write the Mark Asked For",
            crate::TEXT_TYPES,
            |_| reconcile::mark_write(),
        )?;
        command(
            "pta.reconcileLive",
            "Follow the Journal as Typed",
            crate::TEXT_TYPES,
            |_| reconcile::live(),
        )?;
        command("pta.reconcileClose", "Close the Reconciliation", k, |_| {
            reconcile::close()
        })?;
        kalem::keymap("q", "pta.reconcileClose", Some(RECONCILE_WHEN))?;
        kalem::keymap("escape", "pta.reconcileClose", Some(RECONCILE_ESCAPE_WHEN))?;
        command(
            "pta.import",
            "Import Statement",
            &["csv", import::KIND],
            |_| import::command(),
        )?;
        kalem::keymap("space m i", "pta.import", Some(CSV_LEADER_WHEN))?;
        kalem::keymap("r", "pta.import", Some(IMPORT_WHEN))?;
        command(
            "pta.editRules",
            "Edit the Statement's Rules",
            &["csv", import::KIND],
            |_| import::edit_rules(),
        )?;
        kalem::keymap("space m e", "pta.editRules", Some(CSV_LEADER_WHEN))?;
        command(
            "pta.reconcileWith",
            "Pair with the Reconciliation",
            &["csv"],
            |_| reconcile::with_statement(),
        )?;
        kalem::keymap("space m r", "pta.reconcileWith", Some(CSV_LEADER_WHEN))?;
        kalem::keymap("e", "pta.editRules", Some(IMPORT_WHEN))?;
        command("pta.importAccept", "Import", &[import::KIND], |_| {
            import::accept()
        })?;
        kalem::keymap("i", "pta.importAccept", Some(IMPORT_WHEN))?;
        command(
            "pta.importClose",
            "Close the Import",
            &[import::KIND],
            |_| import::close(),
        )?;
        kalem::keymap("q", "pta.importClose", Some(IMPORT_WHEN))?;
        kalem::keymap("escape", "pta.importClose", Some(IMPORT_ESCAPE_WHEN))?;
        command(
            "pta.insertRules",
            "Write the Rules from the Template",
            &["rules"],
            |_| import::insert_rules(),
        )?;
        command("pta.prices", "Fetch Prices", t, |_| prices::command())?;
        command("pta.runBlock", "Run the Journal Block", t, |_| run_block())?;
        kalem::keymap(
            "space m x",
            "pta.runBlock",
            Some(&format!("{journal} && editorMode == org && vimCommand")),
        )?;
        command(
            "pta.runBlockWrite",
            "Write the Journal Block's Results",
            t,
            |_| write_block(),
        )?;
        kalem::keymap("space m p", "pta.prices", Some(&leader))?;
        let mut types = crate::TEXT_TYPES.to_vec();
        types.push("text");
        command("pta.append", "Write the Fetched Text", &types, |_| {
            write_append()
        })?;
        let mut types = crate::TEXT_TYPES.to_vec();
        types.extend([
            CHECK_KIND,
            balance::KIND,
            register::KIND,
            reconcile::KIND,
            import::KIND,
            other::KIND,
        ]);
        command("pta.menu", "Commands…", &types, |_| menu())?;
        kalem::keymap("space m m", "pta.menu", Some(&leader))?;
        kalem::keymap("?", "pta.menu", Some(CHECK_WHEN))?;
        kalem::keymap("?", "pta.menu", Some(BALANCE_WHEN))?;
        kalem::keymap("?", "pta.menu", Some(REGISTER_WHEN))?;
        kalem::keymap("?", "pta.menu", Some(RECONCILE_WHEN))?;
        kalem::keymap("?", "pta.menu", Some(IMPORT_WHEN))?;
        kalem::keymap("?", "pta.menu", Some(REPORT_WHEN))?;
        kalem::on(EventKind::DocumentOpen, |e| {
            if let Event::DocumentOpen(d) = e
                && let Some(p) = &d.path
            {
                DOCS.with(|docs| docs.borrow_mut().insert(d.document, p.clone()));
                if Dialect::of_path(p).is_some() {
                    seen_journal(p);
                    CURRENT.with(|c| c.set(Some(d.document)));
                    refresh(Some(p.clone()));
                    learn(p.clone());
                }
                arrived(p);
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
                balance::forget(*n);
                register::forget(*n);
                reconcile::forget(*n);
                import::forget(*n);
                other::forget(*n);
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
                if let Some(p) = DOCS.with(|d| d.borrow().get(&s.document).cloned()) {
                    arrived(&p);
                }
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::DocumentAfterSave, |e| {
            if let Event::DocumentAfterSave(d) = e
                && Dialect::of_path(&d.path).is_some()
            {
                // An include may have come or gone.
                FILES.with(|f| f.borrow_mut().retain(|_, v| matches!(v, Files::Asking(_))));
                let on_save = settings::own("check_on_save")
                    .and_then(|v| serde_json::from_str::<bool>(&v).ok())
                    .unwrap_or(true);
                if on_save {
                    // Checking refreshes the item too, with its mark; as
                    // the plugin's command, where the saved journal's
                    // text is read for the problems' columns.
                    let args = serde_json::json!({ "path": d.path }).to_string();
                    if kalem::run("pta.checkSaved", &args).is_err() {
                        check(d.path.clone(), false);
                    }
                } else {
                    refresh(Some(d.path.clone()));
                }
                learn(d.path.clone());
                if let Some(dialect) = Dialect::of_path(&d.path) {
                    resolve(d.path.clone(), dialect, |_, main| {
                        balance::saved(&main.path);
                        register::saved(&main.path);
                        reconcile::saved(&main.path);
                        other::saved(&main.path);
                    });
                }
            }
            Reply::Proceed
        })?;
        // The journal as typed: an open Reconcile document follows it
        // (hledger, Ledger), its text read where a command runs.
        kalem::on(EventKind::DocumentChanged, |e| {
            if let Event::DocumentChanged(c) = e
                && let Some(p) = DOCS.with(|d| d.borrow().get(&c.document).cloned())
                && reconcile::follows_file(&p)
            {
                let _ = kalem::run("pta.reconcileLive", "{}");
            }
            Reply::Proceed
        })?;
        for key in ["journal", "tool", "glyphs"] {
            settings::watch(key, true, |_| refresh(current_path()))?;
        }
        Ok(())
    }
}

kalem_plugin::export_plugin!(Pta);
