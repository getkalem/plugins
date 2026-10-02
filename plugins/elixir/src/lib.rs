//! The Elixir language plugin of Kalem: Elixir, EEx and HEEx.
//!
//! A language plugin is declarative (Kalem's D57): `plugin.json` names the
//! file types, the Sublime syntaxes under `syntaxes/`, and the language
//! servers (Expert, the Elixir team's server, where it is installed;
//! ElixirLS otherwise) with their settings. Kalem's core reads the manifest,
//! adds the syntaxes to its highlighter and runs the server through its own
//! client; no code of this crate runs inside Kalem.

/// The manifest.
pub const MANIFEST: &str = include_str!("../plugin.json");
