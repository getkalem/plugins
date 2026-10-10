//! Logseq graphs and Obsidian vaults in Kalem, opened as themselves: the
//! folder found by its marker, its pages, journals, links, block
//! references, tags and tasks indexed, and what links to a page shown
//! beside it. The work list is `graph_todo.md`.
//!
//! This crate is the plugin's library, the same in the component and
//! natively:
//!
//! - [`config`]: a graph's folder and layout, from `logseq/config.edn` or
//!   Obsidian's settings files, and finding a file's graph;
//! - [`date`]: dates and the patterns journals are named by;
//! - [`edn`]: a reader of EDN, `config.edn`'s notation;
//! - [`files`]: the files, through Kalem's `fs` or natively;
//! - [`names`]: page names and Logseq's file names.
//!
//! - `component` (built for WebAssembly only): Kalem's `extension` world.

pub mod app;
#[cfg(target_arch = "wasm32")]
mod component;
pub mod config;
pub mod content;
pub mod date;
pub mod edit;
pub mod edn;
pub mod files;
pub mod index;
pub mod layer;
pub mod names;
pub mod scan;
pub mod template;
pub mod views;

/// The manifest, embedded so that the component carries its own
/// description.
pub const MANIFEST: &str = include_str!("../plugin.json");
