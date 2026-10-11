//! The component: Kalem's `extension` world. Tool Version runs the
//! journal's tool through `process` (PT1); the status bar item names the
//! tool and the main journal of the journal the cursor is in (PT3c),
//! worked out by [`crate::journal`] from the settings and, for hledger,
//! from `hledger files`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use kalem_plugin::kalem::{self, Event, EventKind, Plugin, Reply, Scope};
use kalem_plugin::{editor, process, settings, ui};

use crate::Dialect;
use crate::journal::{self, Installed, Settings, ToolChoice};

/// The status bar item's id.
const STATUS: &str = "pta.journal";

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
}

/// The journal's dialect: the document's language, else its file name.
fn dialect_here() -> Option<(Dialect, Option<String>)> {
    let doc = editor::document()?;
    let d = doc
        .language
        .as_deref()
        .and_then(Dialect::of_language)
        .or_else(|| doc.path.as_deref().and_then(Dialect::of_path))?;
    let dir = doc
        .path
        .as_deref()
        .and_then(|p| p.rsplit_once(['/', '\\']).map(|(d, _)| d.to_string()));
    Some((d, dir))
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

/// Runs `program` with `args` in `dir`, or, where Kalem refuses that
/// folder (a plugin's programs run in the projects' folders only), in
/// the first project's; `done` gets how it ended, `None` when it could
/// not run at all.
fn run(
    program: &'static str,
    args: Vec<String>,
    dir: &str,
    done: impl FnOnce(Option<process::Exit>) + 'static,
) {
    let done = Rc::new(RefCell::new(Some(done)));
    let finish = move |r: Option<process::Exit>| {
        if let Some(f) = done.borrow_mut().take() {
            f(r);
        }
    };
    let here = process::Command::new(program).args(args.clone()).cwd(dir);
    let first = finish.clone();
    if process::run(&here, move |r| first(r.ok())).is_err() {
        let anywhere = process::Command::new(program).args(args);
        let second = finish.clone();
        if process::run(&anywhere, move |r| second(r.ok())).is_err() {
            finish(None);
        }
    }
}

/// Tool Version: what the journal's tool says it is.
fn version() -> Result<String, String> {
    let Some((d, dir)) = dialect_here() else {
        ui::notify("This document is no journal", ui::Level::Info);
        return Ok("null".into());
    };
    let mut c = process::Command::new(d.checker()).args(["--version"]);
    if let Some(dir) = &dir {
        c = c.cwd(dir);
    }
    let started = process::run(&c, move |r| match r {
        Ok(exit) => ui::notify(
            &crate::version_said(d, exit.status, &exit.stdout, &exit.stderr),
            ui::Level::Info,
        ),
        // Not found: Kalem says so through the run's end.
        Err(_) => ui::notify(&missing(d), ui::Level::Warning),
    });
    if let Err(e) = started {
        // Refused: a folder outside the projects, too many runs.
        ui::notify(&format!("{}: {e}", d.checker()), ui::Level::Warning);
    }
    Ok("null".into())
}

fn hide() {
    if let Some(d) = ITEM.with(|i| i.borrow_mut().take()) {
        d.dispose();
    }
}

fn show(tool: Dialect, main: &journal::Main) {
    let options = ui::StatusOptions {
        tooltip: Some(journal::status_tooltip(tool, main)),
        command: Some("pta.version".into()),
        alignment: ui::Alignment::Left,
        priority: 5,
    };
    if let Ok(d) = ui::status(STATUS, &journal::status_text(tool, main), &options) {
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
            set_installed(|i| i.ledger = Some(r.is_some_and(|e| e.status == Some(0))));
            run("hledger", vec!["--version".into()], &d, move |r| {
                set_installed(|i| i.hledger = Some(r.is_some_and(|e| e.status == Some(0))));
                if GENERATION.with(Cell::get) == generation {
                    refresh(Some(path));
                }
            });
        });
        return;
    }
    let tool = journal::tool(choice, dialect, &path, INSTALLED.with(Cell::get));
    if tool == Dialect::Hledger && s.journal.trim().is_empty() {
        // `hledger files` without `-f`: the journal LEDGER_FILE names and
        // its includes.
        run("hledger", vec!["files".into()], &dir, move |r| {
            if GENERATION.with(Cell::get) != generation {
                return;
            }
            let listed = r
                .filter(|e| e.status == Some(0))
                .map(|e| journal::files_listed(&e.stdout));
            show(tool, &journal::main_journal("", &path, listed.as_deref()));
        });
        return;
    }
    show(tool, &journal::main_journal(&s.journal, &path, None));
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

struct Pta;

impl Plugin for Pta {
    fn activate() -> Result<(), String> {
        let mut spec = kalem::spec(
            "pta.version",
            "Tool Version",
            Scope::only(crate::TEXT_TYPES),
        );
        spec.category = "Journal".into();
        kalem::command(spec, |_| version())?;
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
                refresh(Some(d.path.clone()));
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
