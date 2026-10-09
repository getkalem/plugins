//! Word documents opened, edited and saved as themselves.
//!
//! A Kalem plugin for WordprocessingML (ECMA-376 part 1, `.docx`,
//! `.docm`, `.dotx`, `.dotm`): Kalem's task T3.7.5, decisions D54 and D55
//! of its design document. The file is never converted: the package is
//! read as parts through the Office layer it shares with the workbook
//! plugin ([`kalem_ooxml`]); the stories (the body, headers and footers,
//! notes, comments) are read with the byte span of every element
//! ([`story`]); styles ([`styles`]), lists ([`numbering`]) and the theme
//! are resolved into what each paragraph and run looks like ([`flow`]);
//! and an edit rewrites the paragraph it touches and nothing else
//! ([`edit`], [`Document::replace`]). Every part the plugin does not edit
//! is written back byte for byte, and a save without edits returns the
//! input unchanged.
//!
//! Kalem lays documents out itself and a plugin never draws (D54); the
//! interface through which the plugin will hand Kalem its paragraphs is
//! the docx list's WP5. Until it exists, the plugin is a viewer of the
//! `document-viewer` contract that gives Kalem the document's text,
//! outline and information ([`viewer`]), and this crate is the format
//! library that interface will carry.

pub mod chars;
pub mod document;
pub mod edit;
pub mod flow;
pub mod numbering;
pub mod props;
pub mod story;
pub mod styles;
pub mod viewer;

pub use document::{DocView, Document, Error, Kind, Properties, Section};
pub use flow::{ParaAt, StoryId};
pub use viewer::DocxViewer;

/// The clock: the system's natively, Kalem's `clock` interface in a
/// component, where std's panics (wasm32-unknown-unknown has none).
pub(crate) mod time {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) use std::time::{SystemTime, UNIX_EPOCH};
    #[cfg(target_arch = "wasm32")]
    pub(crate) use web_time::{SystemTime, UNIX_EPOCH};
}

/// The manifest, embedded so that the component carries its own
/// description.
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

/// In a component, the compound file reader's clock (an encrypted
/// document) comes from Kalem's `clock` interface.
#[cfg(target_arch = "wasm32")]
pub(crate) fn component_clock() {
    fn now() -> i64 {
        kalem_plugin::viewer::kalem::plugin::clock::now()
    }
    web_time::set_clock(now);
}

#[cfg(target_arch = "wasm32")]
kalem_plugin::export_viewer_of!(DocxViewer);
