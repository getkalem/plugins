//! The Rust language plugin of Kalem.
//!
//! A language plugin is declarative (Kalem's D57): `plugin.json` names the
//! file types, the Sublime syntax under `syntaxes/` (Sublime Text's current
//! Rust, which takes `.rs` from the older one built into Kalem's
//! highlighter) and the language server, rust-analyzer, with its settings.
//! Kalem's core reads the manifest, adds the syntax to its highlighter and
//! runs the server through its own client; no code of this crate runs
//! inside Kalem.

/// The manifest.
pub const MANIFEST: &str = include_str!("../plugin.json");
