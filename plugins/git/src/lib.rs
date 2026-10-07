//! Git in Kalem: the repository's status as a document in the editor, its
//! changed files with their diffs, staged by file, hunk or line, on Doom
//! Emacs's `SPC g` keys, with the arrows, Enter and Escape to move. The
//! design is `DESIGN.md` beside this crate.
//!
//! The plugin runs the `git` program and reads its porcelain output; it
//! holds no git of its own (G1). This crate is its library, the same in
//! the component and natively:
//!
//! - [`git`]: the command lines ([`git::cmd`]) and the readers of git's
//!   answers (status, diff, log, blame, references);
//! - [`refresh`]: a refresh of the status as data in and commands out, so
//!   that the component runs the commands as their answers arrive and the
//!   example runs them one after another;
//! - [`model`]: what the status knows, and what is folded;
//! - [`views`]: the documents (status, log, commit, blame, process log)
//!   written as [`content::Content`], text with styles and regions;
//! - [`target`]: what the cursor or a selection stands for;
//! - [`actions`]: what the leader's keys do to it, as plans of commands;
//! - [`patch`]: patches for hunks and lines;
//! - [`url`]: links to the remote's web site.
//!
//! - [`app`]: the plugin as Kalem runs it, a state machine of inputs
//!   (commands, git's answers, the user's) and effects, with [`panel`],
//!   the Git panel's widgets;
//! - `component` (built for WebAssembly only): Kalem's `extension` world
//!   over [`app`].
//!
//! `examples/git.rs` drives the library from the command line.

pub mod actions;
pub mod app;
#[cfg(target_arch = "wasm32")]
mod component;
pub mod content;
pub mod git;
pub mod model;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod panel;
pub mod patch;
pub mod refresh;
pub mod target;
pub mod url;
pub mod views;

/// The manifest, embedded so that the component carries its own
/// description.
pub const MANIFEST: &str = include_str!("../plugin.json");
