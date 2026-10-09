//! The Rust language plugin of Kalem.
//!
//! A language plugin is declarative (Kalem's D57): `plugin.json` names the
//! file types and the language server, rust-analyzer, with its settings.
//! Rust's highlighting is Kalem's own (the Sublime syntax built into its
//! highlighter), so the plugin ships no syntax. Kalem's core reads the
//! manifest and runs the server through its own client; no code of this
//! crate runs inside Kalem.

/// The manifest.
pub const MANIFEST: &str = include_str!("../plugin.json");
