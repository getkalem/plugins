//! The component: Kalem's `extension` world. The spike (PT1): Tool
//! Version runs the journal's tool through `process`, proving that the
//! component loads beside the manifest's languages.

use kalem_plugin::kalem::{self, Plugin, Scope};
use kalem_plugin::{editor, process, ui};

use crate::Dialect;

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
        Ok(())
    }
}

kalem_plugin::export_plugin!(Pta);
