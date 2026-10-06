//! Excel workbooks opened, edited and saved as themselves.
//!
//! A Kalem plugin for SpreadsheetML (ECMA-376 part 1, `.xlsx`, `.xlsm`,
//! `.xltx`, `.xltm`). The file is never converted: the package is read as
//! parts ([`package`]), the grid is read from the worksheet parts with the
//! byte span of every cell ([`sheet`]), values are shown through their
//! number formats ([`numfmt`]) and styles ([`styles`]), and an edit rewrites
//! the one `<c>` element it touches ([`Workbook::set_cell`]). Every part the
//! plugin does not edit (charts, pivot tables, drawings, the VBA project)
//! is written back byte for byte, and a save without edits returns the
//! input unchanged. The VBA project's modules are read as source
//! ([`vba`]).
//!
//! Kalem loads plugins as WebAssembly components through the `document-viewer`
//! and `document-editor` contracts of its design document (11.13, D54). Until
//! the bindings (`kalem-plugin`, task T3.1.3) are published, this crate is
//! the format library those contracts will wrap: units are sheets, a unit's
//! grid is [`Workbook::grid`], an edit is [`Workbook::set_cell`], and save is
//! [`Workbook::save`].

pub mod calc;
pub mod cellref;
pub mod chart;
pub mod chart_more;
pub mod conditional;
pub mod fill;
pub mod formats;
pub mod formula;
pub mod functions;
pub mod legacy;
pub mod macros;
pub mod numfmt;
pub mod ods_style;
pub mod package;
pub mod pivot;
pub mod rels;
pub mod sheet;
pub mod structure;
pub mod styles;
pub mod validation;
pub mod vba;
pub mod viewer;
pub mod workbook;
pub mod xml;

/// The clock: the system's natively, Kalem's `clock` interface in a
/// component, where std's panics (wasm32-unknown-unknown has none).
pub(crate) mod time {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) use std::time::{Instant, SystemTime, UNIX_EPOCH};
    #[cfg(target_arch = "wasm32")]
    pub(crate) use web_time::{Instant, SystemTime, UNIX_EPOCH};
}

pub use cellref::{CellRef, Range};
pub use sheet::{Cell, Formula, FormulaKind, Sheet, Value};
pub use viewer::XlsxViewer;
pub use workbook::{
    Comment, DefinedName, Error, Input, SheetInfo, SheetKind, Visibility, Workbook,
};

/// The manifest, embedded so that the component carries its own description.
pub const MANIFEST: &str = include_str!("../plugin.json");

#[cfg(test)]
mod tests {
    use super::MANIFEST;

    #[test]
    fn manifest_is_json() {
        let value: serde_json::Value = serde_json::from_str(MANIFEST).expect("plugin.json parses");
        assert!(value.is_object());
    }
}

/// In a component, the clock and the randomness of formulas (`NOW()`,
/// `TODAY()`, `RAND()`) and of the macros' storage come from Kalem's
/// `clock` interface, as natively from the system.
#[cfg(target_arch = "wasm32")]
pub(crate) fn component_clock() {
    use kalem_plugin::viewer::kalem::plugin::clock;
    fn now() -> i64 {
        clock::now()
    }
    fn random() -> f64 {
        (clock::random() >> 11) as f64 / (1u64 << 53) as f64
    }
    ironcalc_base::platform::set_clock(now);
    ironcalc_base::platform::set_random(random);
    web_time::set_clock(now);
}

#[cfg(target_arch = "wasm32")]
kalem_plugin::export_viewer_of!(XlsxViewer);
