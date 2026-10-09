# Rust

Rust for [Kalem](https://github.com/getkalem/kalem): rust-analyzer for completion, documentation, definitions, references, rename, diagnostics and formatting.

**Status: the spike** (RS1 of [`rust_todo.md`](rust_todo.md)). A Cargo project is served, checked from the command line; the settings, clippy, run and test, and rust-analyzer's own requests are the list's later tasks.

A language plugin is declarative (Kalem's D57): this folder holds a manifest, `plugin.json`, a corpus project and its tests, and nothing that runs inside Kalem. It ships no syntax: Rust's highlighting is Kalem's own, the Sublime syntax built into its highlighter. Kalem's core reads the manifest and starts rust-analyzer through its one language server client.

## The server

rust-analyzer, found as `rust-analyzer` on the `PATH` or in `~/.cargo/bin`. Install it with one of:

- `rustup component add rust-analyzer`, the rustup component; rustup then runs the one of the toolchain the project's `rust-toolchain.toml` names;
- `brew install rust-analyzer`;
- a release from <https://github.com/rust-lang/rust-analyzer/releases>, renamed `rust-analyzer` and put on the `PATH`.

Kalem never installs a server by itself.

## The project

The root is the nearest folder up from the file holding a `Cargo.lock`. Cargo writes the lock where it puts the workspace's root, so a workspace is one root and one server, whatever member a file is in, and a workspace inside another folder with a `Cargo.toml` (this plugin's corpus inside this repository) is a root of its own. A project never built has no lock yet: its file's folder is the root, rust-analyzer finds the `Cargo.toml` above it by itself, and its first `cargo metadata` writes the lock. The server runs in the root, so rustup honors the project's `rust-toolchain.toml`.

## Checked

With Kalem 0.6.0 and rust-analyzer 1.99.0 (the rustup component of Rust 1.99.0), on `corpus/ws`, from this repository's top:

```sh
export KALEM_PLUGIN_PATH=$PWD/plugins/rust
kalem lsp status plugins/rust/corpus/ws/app/src/main.rs           # the plugin, the server, the root corpus/ws
kalem lsp ask hover plugins/rust/corpus/ws/app/src/main.rs 11:18   # Circle's declaration and rustdoc
kalem lsp ask completion plugins/rust/corpus/ws/app/src/main.rs 11:35   # after `Circle::new(1.0).`: scaled, area, …
kalem lsp ask definition plugins/rust/corpus/ws/app/src/main.rs 11:36   # into the shapes crate
kalem lsp check plugins/rust/corpus/ws/app/tests/borrow.rs         # the borrow error, E0502
kalem lsp ask format plugins/rust/corpus/ws/app/src/main.rs        # already formatted
```

`cargo test -p kalem-plugin-rust` runs the conformance tests: the manifest, the root against the one `cargo metadata` gives, the corpus built with its one deliberate error, and Kalem's Rust syntax on the corpus.

## The corpus

`corpus/ws` is a Cargo workspace of its own (its own `[workspace]` table), not a member of this repository's: `shapes` has a trait, two structs and a `macro_rules!` macro, and `app` calls into it with one unit test. `app/tests/borrow.rs` does not compile, on purpose: it is the borrow error the diagnostics are checked on. `cargo build`, `cargo run` and `cargo test -p app --bin app` do not build it; `cargo check --all-targets`, rust-analyzer's check, does.

## Not done

See [`rust_todo.md`](rust_todo.md). In short: the two editors checked by hand; rust-analyzer's settings described for Kalem's settings panel; clippy on save; rustfmt when no server runs; `cargo run` and `cargo test` at the cursor; expand macro, open docs and rust-analyzer's other requests. A file of a dependency or of the standard library, reached by going to a definition, gets a root of its own (its crate's or the library's `Cargo.lock`), so Kalem would start a second server there.
