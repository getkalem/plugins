//! Insert a Report as a Table (PT11b): in an Org or Markdown document,
//! a report of the journal's tool picked (those the tool writes as CSV,
//! [`crate::other::as_table`]), its period picked where it takes one,
//! and its CSV written as the document's table under the cursor's line,
//! with a caption naming the report, the journal and the period; one
//! undo step. The journal is the one a statement would go into
//! ([`super::import::journal_of`]).

use std::cell::RefCell;

use kalem_plugin::{editor, kalem, settings, ui};

use crate::Dialect;
use crate::journal;
use crate::other::{self, Markup, Report};
use crate::report::Period;

use super::{NotRun, resolve, run, today};

thread_local! {
    /// The table to write once the plugin's command runs: the document's
    /// file and the text.
    static TABLE: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// The markup of the current document's tables.
fn markup() -> Option<Markup> {
    let doc = editor::document()?;
    let lang = doc.language.as_deref().unwrap_or("");
    let path = doc.path.as_deref().unwrap_or("").to_ascii_lowercase();
    if lang == "org" || path.ends_with(".org") {
        Some(Markup::Org)
    } else if lang == "markdown" || path.ends_with(".md") || path.ends_with(".markdown") {
        Some(Markup::Markdown)
    } else {
        None
    }
}

/// Insert a Report as a Table, in an Org or Markdown document.
pub(super) fn command() -> Result<String, String> {
    let Some(markup) = markup() else {
        ui::notify(
            "A report goes as a table into an Org or Markdown document",
            ui::Level::Info,
        );
        return Ok("null".into());
    };
    let Some(path) = editor::document().and_then(|d| d.path) else {
        ui::notify("Save the document first", ui::Level::Info);
        return Ok("null".into());
    };
    let at = path.clone();
    super::import::journal_of(&at, move |journal| {
        let Some(journal) = journal else {
            return ui::notify(
                "Which journal? Set the plugin's setting journal, or open the journal first",
                ui::Level::Warning,
            );
        };
        let Some(dialect) = Dialect::of_path(&journal) else {
            return ui::notify(&format!("{journal} is no journal"), ui::Level::Warning);
        };
        resolve(journal, dialect, move |tool, main| {
            pick(tool, main.path, path, markup);
        });
    });
    Ok("null".into())
}

/// The reports that are tables: the setting's, Beancount's saved
/// queries, the tool's.
fn pick(tool: Dialect, main: String, doc: String, markup: Markup) {
    let entries = settings::own("reports")
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    let mut list = other::pinned(tool, &entries);
    let (dir, queried) = (journal::folder(&main).to_string(), main.clone());
    let offer = move |mut list: Vec<Report>| {
        list.extend(other::reports(tool, today()));
        let tables: Vec<Report> = list
            .iter()
            .filter_map(|r| other::as_table(tool, r))
            .collect();
        let items: Vec<ui::PickItem> = tables
            .iter()
            .map(|r| ui::PickItem {
                label: r.title.clone(),
                detail: None,
            })
            .collect();
        ui::quick_pick(
            &items,
            &ui::PickOptions {
                title: Some(format!("Table from {}", journal::name(&main))),
                placeholder: None,
                many: false,
            },
            move |picked| {
                let Some(r) = picked.first().and_then(|&i| tables.get(i as usize)) else {
                    return;
                };
                if r.dated {
                    pick_period(tool, main, doc, markup, r.clone());
                } else {
                    gather(tool, main, doc, markup, r.clone(), Period::All);
                }
            },
        );
    };
    if tool != Dialect::Beancount {
        return offer(list);
    }
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
            offer(list);
        },
    );
}

fn pick_period(tool: Dialect, main: String, doc: String, markup: Markup, r: Report) {
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
            title: Some(format!("Period of {}", r.title)),
            placeholder: None,
            many: false,
        },
        move |picked| {
            if let Some(&p) = picked.first().and_then(|&i| Period::ALL.get(i as usize)) {
                gather(tool, main, doc, markup, r, p);
            }
        },
    );
}

/// Runs the report as CSV and has its table written.
fn gather(tool: Dialect, main: String, doc: String, markup: Markup, r: Report, period: Period) {
    let (program, args) = other::command(tool, &main, &r, period.bounds(today()));
    let dir = journal::folder(&main).to_string();
    run(program, args, &dir, move |result| {
        let csv = match result {
            Ok(e) if e.status == Some(0) => String::from_utf8_lossy(&e.stdout).into_owned(),
            Ok(e) => {
                let said = String::from_utf8_lossy(&e.stderr);
                let first = said
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("failed");
                return ui::notify(&format!("{program}: {first}"), ui::Level::Warning);
            }
            Err(NotRun::Missing) => return ui::notify(&super::missing(tool), ui::Level::Warning),
            Err(NotRun::TooLong) => return,
            Err(NotRun::Refused(why)) => {
                return ui::notify(&format!("{program}: {why}"), ui::Level::Warning);
            }
        };
        let csv = format!("{}{csv}", other::header(tool, &r).unwrap_or(""));
        let (title, table) = other::table(&csv, markup);
        if table.is_empty() {
            return ui::notify(&format!("{}: nothing to show", r.title), ui::Level::Info);
        }
        // hledger's own title names the dates.
        let dates = title.is_none();
        let mut caption = format!("{}, {}", title.unwrap_or(r.title), journal::name(&main));
        if r.dated && dates {
            caption.push_str(", ");
            caption.push_str(period.name());
        }
        let text = match markup {
            Markup::Org => format!("#+caption: {caption}\n{table}"),
            Markup::Markdown => format!("{table}\n*{caption}*\n"),
        };
        TABLE.with(|t| *t.borrow_mut() = Some((doc, text)));
        if let Err(why) = kalem::run("pta.insertTableWrite", "{}") {
            ui::notify(&why, ui::Level::Warning);
        }
    });
}

/// Writes the table kept by [`gather`] under the cursor's line of the
/// document it was asked in, when that is still the current one.
pub(super) fn write() -> Result<String, String> {
    let Some((doc, table)) = TABLE.with(|t| t.borrow_mut().take()) else {
        return Ok("null".into());
    };
    if !editor::document()
        .and_then(|d| d.path)
        .is_some_and(|p| journal::same_file(&p, &doc))
    {
        ui::notify(
            &format!("{} is no longer the current document", journal::name(&doc)),
            ui::Level::Warning,
        );
        return Ok("null".into());
    }
    let text = editor::text(None);
    let at = (editor::selected().head as usize).min(text.len());
    let line_end = text[at..].find('\n').map_or(text.len(), |n| at + n);
    let line_start = text[..at].rfind('\n').map_or(0, |n| n + 1);
    let (place, ins) = if text[line_start..line_end].trim().is_empty() {
        (line_start, table)
    } else if line_end == text.len() {
        (line_end, format!("\n{table}"))
    } else {
        (line_end + 1, table)
    };
    editor::insert(Some(place as u64), &ins);
    editor::transact("Insert Report as Table");
    Ok("null".into())
}
