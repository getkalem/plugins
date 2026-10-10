# Rust

Rust for [Kalem](https://github.com/getkalem/kalem): highlighting for the current editions, and rust-analyzer for completion, documentation, definitions, references, rename, diagnostics and formatting.

**Status: early** (RS1 and RS2 of [`rust_todo.md`](rust_todo.md)). A Cargo project is served, checked from the command line, and highlighted with Sublime Text's current Rust syntax; the settings, clippy, run and test, and rust-analyzer's own requests are the list's later tasks.

A language plugin is declarative (Kalem's D57): this folder holds a manifest, `plugin.json`, the Sublime syntax under `syntaxes/`, a corpus and its tests, and nothing that runs inside Kalem. Kalem's core reads the manifest, adds the syntax to its highlighter and starts rust-analyzer through its one language server client.

## Highlighting

Kalem has a Rust syntax built in, from before `async` and `.await` (Rust 1.39, 2019): it leaves `async` and `.await` plain, splits `1e-3` into a number and an operator, `r#type` into three pieces, and a `#!` line into operators, and misses `union`, a format string's named width (`{name:>width$}`) and four of the macro fragment specifiers. The plugin ships Sublime Text's current one, which reads all of these, the `#!` line and the TOML frontmatter of a single-file package (Cargo's `-Zscript`) too. While the plugin is installed it takes `.rs` files and `rust` source blocks in Org and Markdown from the built-in one.

Known differences, each from Kalem's highlighter or the syntax, not from Rust:

- The names a declaration gives (a struct's, an enum's, a trait's, a constant's or static's, a module's, a loop label) are not colored: the syntax names them (`entity.name.struct`, `entity.name.constant`, …), and Kalem's highlighter gives those scopes no color yet. The built-in syntax colored a constant's name.
- A type where it is used (`Option`, `Vec`, `String`, an enum's variant) has the keywords' color: the syntax scopes it `storage.type`, which Kalem colors as a keyword.
- Every variable and field is colored as a variable, not only where it is declared.
- `$name` is colored as a macro's variable wherever it is, in a comment too, as the syntax does for `macro_rules!`.
- `safe` in an `unsafe extern` block and `raw` in `&raw const` are plain.
- It is about 40% slower than the built-in one: Kalem's 558 Rust files of 380,000 lines take 40 s to highlight whole rather than 28 s, its largest, 17,897 lines, 1.9 s. Kalem colors the lines on the screen first, so this shows only when a long file is colored to its end.

## The server

rust-analyzer, found as `rust-analyzer` on the `PATH` or in `~/.cargo/bin`. Install it with one of:

- `rustup component add rust-analyzer`, the rustup component; rustup then runs the one of the toolchain the project's `rust-toolchain.toml` names;
- `brew install rust-analyzer`;
- a release from <https://github.com/rust-lang/rust-analyzer/releases>, renamed `rust-analyzer` and put on the `PATH`.

Kalem never installs a server by itself.

rustup puts a `rust-analyzer` in `~/.cargo/bin` whether or not the component is installed; without it, that program ends at once with rustup's reason, such as "Unknown binary 'rust-analyzer' in official toolchain '1.88-aarch64-apple-darwin'" for a project pinned to a toolchain installed without it. Kalem 0.6.1 and earlier start it again five times over half a minute and then say only that it stopped. Kalem's change for this (T3.8.1, not released yet) says at once that rust-analyzer did not start, in rustup's words and with the three ways above, in the status bar and from `kalem lsp check`; and `kalem lsp status` runs `rust-analyzer --version`, the manifest's `version`, printing the version or `does not run:` with the reason.

## The project

The root is the nearest folder up from the file holding a `Cargo.lock`. Cargo writes the lock where it puts the workspace's root, so a workspace is one root and one server, whatever member a file is in, and a workspace inside another folder with a `Cargo.toml` (this plugin's corpus inside this repository) is a root of its own. A project never built has no lock yet: its file's folder is the root, rust-analyzer finds the `Cargo.toml` above it by itself, and its first `cargo metadata` writes the lock. The server runs in the root, so rustup honors the project's `rust-toolchain.toml`.

A file of the standard library or of a dependency, reached by going to a definition, has a `Cargo.lock` of its own (a crate from crates.io ships one, and so does the standard library's folder). Up to Kalem 0.6.1 it becomes a root of its own, and a second rust-analyzer starts there; in the standard library's folder that one cannot load the workspace and answers nothing. Kalem's change for this (T3.8.1, not released yet) serves the file with the rust-analyzer that named it, which knows it: one server, and hover and definitions work inside the standard library.

## Settings

Kalem's settings panel (`SPC h v`, then **installed plugins**, then **Rust**) shows the rust-analyzer settings the plugin describes, with rust-analyzer's own defaults: the command run on save (`check`, or `clippy` for its lints too), the features turned on, a target folder of its own, build scripts and procedural macros, completion's automatic imports, diagnostics turned off, files left out, rustfmt's arguments or another formatter, and how the imports it adds are written. Or in Kalem's `settings.toml`, or a project's `.kalem/settings.toml`:

```toml
[plugins."org.kalem.rust".settings.rust-analyzer]
check.command = "clippy"   # clippy's lints on save too; "check" by default
cargo.features = "all"     # or a list of names
cargo.targetDir = true     # a target folder of its own: a build in the terminal does not wait for it

# Another rust-analyzer, and its environment.
[plugins."org.kalem.rust".servers.rust-analyzer]
command = ["/opt/rust-analyzer/rust-analyzer"]
env = { RA_LOG = "info" }
```

A change reaches rust-analyzer without starting it again; a new check command runs at the next save. Any other setting of rust-analyzer's can be put under the same table, nested as its name is dotted. `completion.callable.snippets` makes no difference yet: Kalem takes no snippets, so rust-analyzer completes a function by its name alone.

## Diagnostics

Two kinds, as in VS Code:

- **cargo's**: `cargo check` (or `cargo clippy`, by the setting `check.command`) runs when rust-analyzer has loaded the project and each time a file is saved, its progress in the status bar ("rust-analyzer: cargo check"), its errors and warnings for the whole workspace after it ends. The first run builds the dependencies.
- **rust-analyzer's own**, as you type: type mismatches, names not in snake case, unresolved imports, a file no module includes, and their quick fixes as code actions (`SPC c a`; "Rename to bad_name"). rust-analyzer gives these only to an editor that asks for them. Kalem 0.6.1 and earlier do not ask, and show cargo's only; Kalem's change for this (T3.8.2, not released yet) asks after a file opens and after its changes.

An error both find, a type mismatch, is listed twice, once from `rustc` and once from rust-analyzer, as VS Code lists it. While rust-analyzer loads the project (half a minute the first time, the standard library indexed) it may answer "content modified" to what it is asked; the same Kalem change asks again rather than showing that.

## A file outside a project

rust-analyzer serves only files of a Cargo project it has loaded. A `.rs` file with no `Cargo.toml` above it (a scratch file, a `rust-script`) gets a server that finds no project and answers nothing: no documentation, no completion, no diagnostics. Name the file in rust-analyzer's `linkedProjects` and it is served on its own, against the standard library: documentation, completion and definitions work. A `.kalem/settings.toml` in its folder does it, with paths relative to the folder:

```toml
[plugins."org.kalem.rust".settings.rust-analyzer]
linkedProjects = ["scratch.rs"]
```

Such a file gets no diagnostics from `cargo check`, which fails on a file that names no edition; it gets rust-analyzer's own, with a Kalem that asks for them (below). A file inside a project that no `mod` names is not served either; rust-analyzer's "unlinked file" diagnostic says so, and offers to add the `mod`.

The plugin does not refuse such files a server (`requireRoot`): that would refuse one to a project never built too, which has no `Cargo.lock` yet and is served from its file's folder.

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

`cargo test -p kalem-plugin-rust` runs the conformance tests: the manifest; the root against the one `cargo metadata` gives; the corpus built with its one deliberate error and `edition.rs` compiled without a warning; the syntax registered as Kalem registers it, found for `.rs` and `rust`, parsing every corpus file to its end, and coloring what the built-in one does not.

## The corpus

`corpus/edition.rs` holds the constructs Rust gained after Kalem's built-in syntax was written (`let`–`else`, let chains, `async` closures, `const` blocks, C strings, raw identifiers, `use<'a>`, `unsafe extern` with `safe` items, `&raw const`, exclusive range patterns, every macro fragment specifier); it compiles as a library. `corpus/script.rs` is a single-file package with a `#!` line and a frontmatter, which needs nightly Cargo.

`corpus/ws` is a Cargo workspace of its own (its own `[workspace]` table), not a member of this repository's: `shapes` has a trait, two structs and a `macro_rules!` macro, and `app` calls into it with one unit test. `app/tests/borrow.rs` does not compile, on purpose: it is the borrow error the diagnostics are checked on. `cargo build`, `cargo run` and `cargo test -p app --bin app` do not build it; `cargo check --all-targets`, rust-analyzer's check, does.

## Not done

See [`rust_todo.md`](rust_todo.md). In short: the two editors checked by hand; rustfmt when no server runs; `cargo run` and `cargo test` at the cursor; expand macro, open docs and rust-analyzer's other requests.

## Sources and licenses

- `syntaxes/Rust.sublime-syntax`: from [sublimehq/Packages](https://github.com/sublimehq/Packages) at `7f74b54` (2026-02-11), the package's MIT license beside it (`syntaxes/LICENSE-sublimehq-rust.txt`), unchanged.
- `corpus/`: for the tests, under this repository's license.
