# rust: Rust served by rust-analyzer, a plugin that is a manifest

The list for the `rust` plugin of getkalem/plugins (`plugins/rust`,
crate `kalem-plugin-rust`, id `org.kalem.rust`): Kalem's task T3.8.6d,
decisions D57 (one language server client in the core, the plugins
declare the servers), D58 (rust-analyzer as the one Rust server,
proposed, confirmed by the corpus) and D16 (Sublime syntaxes for the
highlighter). Written 2026-10-09 before the folder existed, at the
repository's root, and moved here with the crate the same day; each task
says what was done and what is open. The elixir plugin is the model: a
declarative language plugin (D57), which is a manifest, syntax files,
settings and a corpus project, with a conformance test in its crate and
nothing that runs inside Kalem. What that plugin does for Elixir (Expert
or ElixirLS started by Kalem's one client, the Mix project as the root)
this one does for Rust with rust-analyzer and the Cargo workspace. The
letters are RS; R is the roadmap's.

## A manifest, and what Kalem already has

Three things a Rust plugin might ship are Kalem's already. Rust is
among the built-in syntaxes of the highlighter (syntect 5.3's default
set, `("rust", "Rust")` in `kalem-highlight`), so `.rs` files and
`#+begin_src rust` blocks are colored today; `//` and `/* */` are known
to the core's comment commands; `Cargo.toml`, `Cargo.lock`,
`rust-toolchain.toml`, `rustfmt.toml` and `.cargo/config.toml` are
TOML, the core's (T2.7a.7). So the plugin's work is rust-analyzer:
finding it, starting it in the right root with the right
configuration, and making what it answers reachable. It ships a syntax
only if the built-in one is behind the current edition: RS2 found that
it is, and ships Sublime Text's current one.

What rust-analyzer needs from the client, the Elixir servers did not:
its configuration arrives as the answer to `workspace/configuration`
for the section `rust-analyzer` (the client answers a section by
walking `settings`, so the manifest's `settings` holds one object under
that key) or as `initializationOptions`, which would be the section's
contents unwrapped, not merged with the user's settings by Kalem, and
so not sent (whether any setting needs it at `initialize` is RS3d's);
it reports loading, build scripts, indexing and
`cargo check` as `$/progress`, so no `busyLog`; it offers UTF-8
positions, which the client takes; it registers file watchers
dynamically, which the client declines (`dynamicRegistration: false`),
so rust-analyzer watches with its own `notify` and Kalem's
`didChangeWatchedFiles` arrive beside it; it sends
`experimental/serverStatus` and has a dozen requests of its own (RS9).
Everything the client lacks is listed by task as waiting on Kalem
(T3.8.2: the light bulb, lenses, inlay hints, semantic tokens,
snippets; T3.8.4: the install recipe run with consent, run and test
commands, a second server), and what the plugin needs beyond the
manifest is proposed as an addition for every language plugin, in the
words of the other servers it serves, never in rust-analyzer's.

Each task is done when it works in both editors, is said in the
manifest where a manifest can say it (a declarative plugin has no
code), is checked against rust-analyzer on the corpus workspace and on
Kalem's own repository, and has its tests: the conformance test of the
crate for the manifest and the syntax, the corpus test through the
client for the server. "Checked against rust-analyzer" names the
version checked and the toolchain it ran with, as the elixir README
names Expert 0.1.11. The tasks are in the order they are done, the
spike first.

## RS1. The spike: a Cargo project served

- [~] RS1 `plugins/rust` in `plugins/elixir`'s shape: `Cargo.toml`
  (`kalem-plugin-rust`, no dependencies, `serde_json` and
  `kalem-highlight` for the test), `src/lib.rs` holding the manifest,
  `tests/conformance.rs`, and the smallest `plugin.json` that serves a
  file: `id` `org.kalem.rust`, `name` "Rust", `api` `^0.1` as
  elixir's, `activation` `onLanguage:rust`, `permissions`
  `subprocess`; one language, `rust` (`extensions` `rs`; `shebangs`
  `rust-script` and `cargo`, for a single-file package's
  `#!/usr/bin/env cargo`; `comment` `//` and `/* */`; `brackets` the
  three pairs; `syntax` "Rust", the built-in one); one server,
  `rust-analyzer` (`command` `["rust-analyzer"]`, `candidates` it and
  `~/.cargo/bin/rust-analyzer`, `rootMarkers` `["Cargo.lock"]`, the
  nearest; no `requireRoot`, see RS3c; `install` the three ways of
  RS3a; `settings` `{"rust-analyzer": {}}` for now). The corpus:
  `corpus/ws`, a Cargo workspace of two crates (`shapes` with a trait,
  two structs and a `macro_rules!` macro; `app` calling into it, with
  one unit test, and a deliberate borrow error for RS4 in an
  integration test, `app/tests/borrow.rs`, so that `cargo build`,
  `cargo run` and the unit test at the cursor still build), with its
  own `[workspace]` table so Cargo roots it there, `Cargo.lock`
  committed, `target/` ignored. Done when `kalem lsp status
  corpus/ws/app/src/main.rs` names the plugin and the server, `kalem
  lsp ask hover` answers with rustdoc, and completion after `.` on the
  struct lists its methods in both editors. Noted: Kalem starts a
  server in its root (`current_dir(&config.root)`), so rustup's proxy
  picks the toolchain `rust-toolchain.toml` names.
  (Done 2026-10-09, but for the editors: the crate, the manifest, the
  corpus, a line in `CODEOWNERS` and the plugin's entry in
  `index.json`, with no download until a tag (CI's `build-index.py
  --check` wants every manifest there). Two changes from the plan.
  The root marker is `Cargo.lock`, the nearest, not `Cargo.toml`, the
  outermost: with the outermost `Cargo.toml`, a corpus file rooted at
  this repository, rust-analyzer loaded the plugins workspace and
  answered nothing about the corpus, a file of no crate it had
  loaded. Cargo writes `Cargo.lock` where it puts the workspace's
  root, so the nearest lock is the root Cargo chose: this repository,
  Kalem's checkout and the corpus are one root each, every member
  inside it. A project never built has none: its file's folder is the
  root, rust-analyzer finds the `Cargo.toml` above it and answers, and
  its first `cargo metadata` writes the lock, so the next file roots
  at the workspace (checked on a copy of the corpus without its lock).
  No `initializationOptions`, for the reason above. The conformance
  test has five tests: the manifest complete; the root equal to the
  `workspace_root` `cargo metadata` gives, for every corpus file; the
  corpus checked, its unit test run, and its one error E0502 in
  `borrow.rs` with `--all-targets` (in a temporary target folder);
  Kalem's `Rust` found by the manifest's name and by `.rs`; the corpus
  colored. Checked with Kalem 0.6.0 and rust-analyzer 1.99.0
  (b940084d 2026-09-28, the rustup component of Rust 1.99.0), from the
  command line with `KALEM_PLUGIN_PATH=plugins/rust`: `kalem lsp
  status` names the plugin, the server and the root `corpus/ws` for
  both crates; `ask hover` on `Circle` gives its declaration and its
  rustdoc; `ask completion` after `Circle::new(1.0).` lists `scaled`
  and `area` with their documentation, in 0.2 s; `ask definition` on
  `scaled` and `ask references` on `Area` cross the two crates;
  `check` finds the E0502 in `borrow.rs` and nothing in `main.rs`;
  `ask format` says `main.rs` is formatted. Open: completion and
  hover in the graphical and the terminal editor, by hand.)

## RS2. The highlighter: the current edition

- [x] RS2 The built-in Rust (the sublimehq/Packages snapshot inside
  syntect 5.3) against the current `Rust/Rust.sublime-syntax` of
  sublimehq/Packages on `corpus/edition.rs`, a file of the constructs
  the 2021 and 2024 editions added or Rust gained since the snapshot:
  `let`–`else`, let chains, `async` closures and blocks, `const {}`
  blocks, c-string literals (`c"…"`), raw identifiers, `use<'a>`
  precise capturing, `unsafe extern`, `#[diagnostic::…]`, labeled
  blocks, `impl Trait` in argument and return position,
  `macro_rules!` with every fragment specifier, attributes with nested
  meta, doc comments with fenced code. Where the built-in one
  mis-scopes a line, the newer file is shipped as
  `syntaxes/Rust.sublime-syntax` with its license beside it, the
  pinned commit named in the README, unchanged as the elixir plugin's
  HTML bases are, named in `syntaxes`, and takes `.rs` from the
  built-in one (a plugin's syntax comes first for its extensions); its
  embeds resolved against what Kalem's set has, through
  `kalem_highlight::register` in the conformance test as elixir's
  `templates_through_kalem` does. Where it does not, no file is
  shipped and the README says why.
  (Done 2026-10-10. The built-in one is behind: it leaves `async` and
  `.await` plain (it predates Rust 1.39), splits `1e-3` into a number
  and an operator, `r#gen` and `r#type` into three pieces, a `#!` line
  into operators, misses `union`, a format string's named width
  (`{name:>width$}`), the fragment specifiers `pat_param`,
  `lifetime`, `literal` and `vis`, and in `kalem-core`'s `paste.rs`
  took a `'` inside a string for a character and colored thirteen
  lines of code after it as a string. Shipped: sublimehq/Packages'
  file at 7f74b54 (2026-02-11), byte for byte, with the Rust
  package's own MIT license as
  `syntaxes/LICENSE-sublimehq-rust.txt` (the Packages license names
  no exception for it; the package carries its own). It uses `pop:
  1` and embeds `scope:source.toml` for a single-file package's
  frontmatter, both of which syntect 5.3 and Kalem's TOML resolve;
  `register` reports no error, and a plugin's syntax named "Rust"
  wins over the built-in one for `rs`, `rust` and the name, since
  syntect looks up newest first. Checked on 558 files and 380,000
  lines (Kalem's crates, this repository's): no parse error through
  syntect; 40 s to highlight all of them whole in a release build
  against the built-in one's 28 s, Kalem's `viewer.rs` (17,897 lines)
  1.9 s; of the lines the built-in one colored and this one leaves
  plain, all but `paste.rs`'s are ALL_CAPS names inside macro calls,
  which the built-in one guessed were constants. Kalem 0.6.0 lists
  `syntaxes: Rust` for the plugin. The corpus gained `edition.rs`
  (compiled by the test with `rustc --edition 2024`, no warning; no
  `dyn*`, which is unstable) and `script.rs` (a `#!` line and a
  frontmatter, nightly's, so not compiled). The conformance test has
  eight tests: the syntax registered as Kalem does, once per test
  binary; found for `rs`, `rust` and "Rust", coloring `async`; every
  corpus file parsed to its end through syntect (with a TOML stand-in,
  syntect's set having none) and Rust again after it; the workspace's
  colors; and each mis-coloring above, fixed. Without the syntax in
  the manifest four of them fail. Known differences, in the README:
  the names a declaration gives get no color, because Kalem maps none
  of `entity.name.struct`, `.enum`, `.trait`, `.impl`, `.constant`,
  `.label`, `.macro` or `.module` to a kind (the built-in one colored
  a constant's name); a type where it is used is `storage.type`, which
  Kalem colors as a keyword; every variable is colored, not only
  declared ones; `$name` in a comment is colored as a metavariable,
  by the syntax's prototype, as the built-in one does; `safe` and
  `raw` are plain. Kalem's part, for every syntax that follows
  Sublime's current scope names (C, C++, Go, Java, TypeScript, Python
  and the rest of sublimehq/Packages): `entity.name.struct`, `.enum`,
  `.union`, `.trait`, `.interface`, `.impl` and `.namespace` as types,
  `entity.name.constant` as a constant, `entity.name.macro` as a
  macro, `entity.name.label` as a constant; `storage.type` as a type
  only once the built-in set stops using it for `struct` and `fn`.)

## RS3. The server: found, in the right root, told its settings

- [~] RS3a Finding rust-analyzer, in the order T3.8.6d gives: the
  rustup component (the `rust-analyzer` proxy in `~/.cargo/bin`,
  which resolves to the component of the toolchain the root selects),
  then the `PATH` (Homebrew's, a release binary renamed), through
  `candidates` `["rust-analyzer", "~/.cargo/bin/rust-analyzer"]`;
  `install` says "`rustup component add rust-analyzer`; or `brew
  install rust-analyzer`; or a release from
  https://github.com/rust-lang/rust-analyzer/releases on the PATH".
  The trap: rustup's proxy exists whether or not the component is
  installed, and without it exits at once with "'rust-analyzer' is
  not installed for the toolchain …", which the client sees as a
  crash and restarts with backoff. Kalem's part, for every language
  whose toolchain ships proxies (rustup's `rust-analyzer` and
  `rustfmt`, pyenv's shims, corepack's): a server that exits within
  its first second shows its standard error as the reason in the
  status bar with the `install` text, and is not restarted; the
  recipe run with consent is T3.8.4's open item and waits there. Done
  when a machine without the component says so in `kalem lsp status`
  and in the status bar.
  (Done 2026-10-10 in this plugin and on Kalem's branch
  `rust-server-start` (the worktree `org-rust` beside Kalem's
  checkout), not merged: Kalem's checkout had another session's work
  in progress on `main`. Reproduced first on Kalem 0.6.0, with rustup
  1.29.1, without touching rustup's settings: a project whose
  `rust-toolchain.toml` pins 1.88 (installed without the component;
  the proxy says "error: Unknown binary 'rust-analyzer' in official
  toolchain '1.88-aarch64-apple-darwin'." and nothing on how to
  install it), and one whose toolchain is a folder by `path` with
  `rustc` and `cargo` only. `kalem lsp status` said the server was
  found; `ask` and `check` started it six times in 36 s and gave up
  with "stopped (Some(1)) … see `kalem lsp log`", a command that does
  not exist, and rustup's reason was nowhere, not even in `check
  --log`. Kalem's part as made, for every language rather than "the
  first second": a server whose process ends before it answers
  `initialize` (or refuses it, or is ended for no answer in 120 s) is
  not restarted, since it would end the same way; the reason is the
  server's own (the error it refused `initialize` with, else the last
  three lines of its standard error, which the client now reads to
  its end before the exit is an event) with the plugin's `install`
  text, said as a notice, by the `SPC c` keys and in the status bar;
  `code.restartServer` tries again; exit codes in words. And a
  server's `version` in the manifest (T3.8.4): `kalem lsp status`
  runs it in the root with the server's environment and prints the
  version, or "does not run:" with the reason and the install text,
  exiting 1. Tested in Kalem: the fake server's new `absent` behavior
  (its reason on standard error, exit 1 at once) in the client's
  tests and in the editor's service test (one notice, nothing started
  past the backoff, the status bar's text, a restart once mended);
  `server_version` on `sh`. The Book's "Language plugins", T3.8.1,
  T3.8.4 and T3.8.6d, and the changelog with it. Here: `"version":
  ["--version"]` in the manifest, checked by the conformance test, and
  the trap in the README. Checked with the branch's terminal build:
  `status` prints "version: rust-analyzer 1.99.0 (b940084d
  2026-09-28)" for the corpus and "does not run: error: Unknown
  binary …" with the install text for the 1.88 project; `ask hover`
  there says "rust-analyzer did not start: error: Unknown binary …
  (rustup component add rust-analyzer; …)" in 3.4 s rather than 36.
  Merged as getkalem/kalem#27 (2026-10-10), released in Kalem 0.6.7
  (2026-10-10). Open: the status bar seen in the two editors by hand.)
- [~] RS3b The root. Decided in RS1: the nearest `Cargo.lock`, the
  root Cargo chose; a workspace's members are one root and one
  server; `rust-toolchain.toml` is honored by the proxy because the
  server starts in the root. Left: (1) a file of a dependency or of
  the standard library, reached by going to a definition, has a
  `Cargo.lock` of its own (a crate from crates.io ships one, and so
  does the `library` folder of `rust-src`), so `kalem lsp status`
  gives it a root of its own and Kalem would start a second
  rust-analyzer there, loading that crate or the whole standard
  library. Kalem's part, for every language: a file opened from a
  server's answer (a definition, a reference, a symbol) and outside
  every root that server serves stays with that server, read-only to
  it, rather than starting another; the same holds for Go's module
  cache, Python's `site-packages`, Node's packages outside the root
  and Elixir's `deps` of another project. Checked in the editors on
  `Vec::push` and on a dependency of a corpus crate once the corpus
  has one. (2) A crate excluded from a workspace (`exclude` in its
  `[workspace]`) and never built on its own has no lock and roots at
  its file's folder until rust-analyzer's first `cargo metadata`
  writes one; recorded, not changed.
  (Done 2026-10-10 on Kalem's branch `rust-server-start`, after
  RS3a's commit, not merged. Worse than "loading the whole standard
  library": with Kalem 0.6.0's rule, `iter`'s definition from the
  corpus (`core/src/slice/mod.rs`) roots at the `library` folder, and
  a rust-analyzer started there cannot load it ("failed to parse
  manifest" for `library/Cargo.toml`, which needs a nightly Cargo;
  `profiler_builtins`'s build script failing), so hover there answered
  nothing after 11 s. Kalem's part as made: the service keeps the
  files each answer names outside the answering server's root, by
  real path, with that server's key; a file among them opens in that
  server when it runs and serves the file's language, unless a server
  already runs in the file's own root (a monorepo's other workspace
  keeps its own). Not read-only: rust-analyzer serves such a file as
  any, and edits to the standard library are the user's business.
  Tested in Kalem with the fake server naming a file of a `library`
  folder beside its project, a root of its own: served by the
  project's server, one server; with the rule off the test fails. By
  hand through Kalem's service with rust-analyzer 1.99.0 on the
  corpus (a throwaway example on the branch): the definition of
  `iter` opened `core/src/slice/mod.rs` in the corpus's server ("ready,
  2 documents"), and hover there gave `core::slice::Iter`'s
  declaration, 8 s from start. The Book's "Language plugins", T3.8.1,
  T3.8.6d and the changelog with it; here, the README's project
  section. (2) stays recorded. Merged as getkalem/kalem#27, released
  in Kalem 0.6.7. Open: the editors by hand; a crates.io dependency
  in the corpus, which needs its tests to fetch it.)
- [~] RS3c A `.rs` outside any Cargo project (a `rust-script`, a
  scratch file, the file an Org block will export in RS7b): no
  `requireRoot`, the file's folder as the root, and what rust-analyzer
  does with it checked: served against the sysroot as a detached file
  (what it does for a file the client opens with no project around
  it), or served only when named in `detachedFiles` at start, which
  Kalem cannot do today; the outcome in the README under known
  differences, the manifest changed to `requireRoot` true if the
  server only errors.
  (Done 2026-10-10 but for Kalem's part, proposed below and not
  made. Checked with rust-analyzer 1.99.0 on a scratch file in a
  folder with nothing above it, through Kalem 0.6.0 and the branch
  build: rust-analyzer logs "failed to find any projects" and answers
  nothing, no documentation, completion or diagnostic; it does not
  serve an opened file on its own. Named in `linkedProjects` (the
  successor of `detachedFiles`; a path relative to the root works) it
  serves the file on its own against the standard library:
  documentation, completion (125 items after a `String`'s `.`),
  definitions into `alloc`; its `cargo check` fails ("`package.edition`
  is unspecified", rust-analyzer's health then "warning"), and its own
  diagnostics come only by pull (RS4). A `.kalem/settings.toml` in the
  file's folder with `linkedProjects = ["scratch.rs"]` under
  `plugins."org.kalem.rust".settings.rust-analyzer` does it today, in
  Kalem 0.6.0 too; the README says so. A file inside a project that no
  `mod` names is not served either, and rust-analyzer's "unlinked
  file" diagnostic saying so comes only by pull. No `requireRoot`: it
  would refuse a server to a project never built (no `Cargo.lock` yet,
  served from its file's folder since RS1); the conformance test
  checks that it stays out. Kalem's part, for every language whose
  server needs its loose files named: root markers in groups tried in
  order, the nearest of the first group found deciding
  (`[["Cargo.lock"], ["Cargo.toml"]]`: the workspace by its lock, a
  crate never built by its manifest; Python's lock files before
  `pyproject.toml`, a JavaScript workspace's before `package.json`),
  so that "outside any project" is a fact Kalem knows (no marker of
  any group); and `${looseFiles}` in a server's `settings`, the open
  files it serves that are outside any project, kept current with
  `didChangeConfiguration` (`"linkedProjects": ["${looseFiles}"]`
  here; gopls, pyright, clangd and TypeScript's servers serve loose
  files on their own and would not use it). Open: that part, if the
  owner wants it.)
- [~] RS3d The settings, described for Kalem's settings panel as
  elixir's ElixirLS settings are (`settings.rust-analyzer.NAME` keys
  with `type`, `default`, `enum` or `examples`, and a description
  starting "rust-analyzer: "), the described default equal to what
  the manifest sends, checked by the conformance test as elixir's
  `settings_describe_what_elixir_ls_is_sent` does; the keys of RS4
  and RS5 plus `cargo.features` (`"all"` or a list),
  `cargo.allTargets`, `cargo.targetDir` (a second target folder so
  the server's `cargo check` and the terminal's `cargo build` do not
  wait on each other's lock; off as rust-analyzer's default, see the
  open questions), `cargo.buildScripts.enable`, `procMacro.enable`,
  `procMacro.ignored`, `completion.autoimport.enable`,
  `completion.callable.snippets` (`add_parentheses` rather than its
  default `fill_arguments`: the client declares no `snippetSupport`,
  so until the snippet engine (T3.8.3) an argument placeholder would
  be inserted as text), `diagnostics.disabled`,
  `diagnostics.experimental.enable`, `files.excludeDirs`,
  `rustfmt.extraArgs`, `rustfmt.overrideCommand`,
  `imports.granularity.group`, `imports.prefix`; the user's
  `plugins."org.kalem.rust".settings.rust-analyzer` merged over them
  and sent again with `didChangeConfiguration`, which rust-analyzer
  answers by asking for the section again. Done when a setting
  changed in the panel reaches the server without a restart (`check`
  to `clippy`, and the next save shows clippy's warnings).
  (Done 2026-10-10, but for the panel clicked by hand. Twenty-two
  settings described, their defaults and choices those of
  rust-analyzer 1.99's own schema (`rust-analyzer
  --print-config-schema`), which corrected this list in two places:
  `files.excludeDirs` is `files.exclude` now, and `imports.prefix`
  defaults to `crate`. Also described: `cargo.noDefaultFeatures`.
  Settings that take one of several types (`cargo.features`, `"all"`
  or a list; `cargo.targetDir`, null, a boolean or a path;
  `check.allTargets`, `check.features`, `rustfmt.overrideCommand`,
  with null for "as the other setting says") have a VS Code style
  list of types, which Kalem's panel edits as JSON;
  `procMacro.ignored` is an object, also JSON. The manifest sends
  nothing beyond rust-analyzer's defaults, `completion.callable.
  snippets` included: probed with Kalem's capabilities (no
  `snippetSupport`), rust-analyzer completes `scaled` as `scaled`,
  with no parentheses or places, whatever the setting says, so
  `add_parentheses` would change nothing until T3.8.3 and
  `fill_arguments` is right after it. The conformance test, as
  elixir's: each key rust-analyzer's, typed as the panel knows (a
  union only of those types and null), a default (null only where
  null is a type), a choice's default among its choices, the
  description "rust-analyzer: …", what is sent described and equal
  to its default; and, when rust-analyzer runs where the test does
  (CI too, since RS10b's component), each key one of its settings
  with its default and choices, which a wrong default for
  `imports.prefix` fails. Read by Kalem's own `plugin_settings::read`:
  22 fields, booleans, enums, texts with examples, lists, and the
  unions as JSON. Through Kalem's service as the panel drives it (the
  user's settings set, `settings_changed`), on a copy of the corpus
  with a `v.len() == 0`: no restart (the same server, "ready, 1
  documents"), and 0.4 s after the save clippy's `len_zero` warning.
  The README's settings section, with the TOML. Open: the panel by
  hand, which lists installed plugins only, not those of
  `KALEM_PLUGIN_PATH`.)

## RS4. Diagnostics: `cargo check` on save, clippy by setting

- [~] RS4 `checkOnSave` true and `check.command` `check` in the
  manifest, `clippy` by setting (what this repository's CI runs;
  slower); `check.allTargets`, `check.extraArgs`, `check.features`
  and `check.workspace` described; the run's progress ("cargo check")
  in the status bar from `$/progress`; the diagnostics in the status
  bar and the lists (`SPC c x`, `SPC c X`), underlined and marked in
  the gutter when T3.8.2 lands that; rust-analyzer's own diagnostics
  (unresolved imports, type mismatches, missing fields; the
  experimental ones off) beside cargo's, with their quick fixes as
  code actions (`SPC c a`, Ctrl+. in the Word-like profile, once
  T3.8.7 binds them; the request is implemented); a `cargo check`
  that takes minutes on a large workspace (Kalem's own) blocks
  nothing, the criterion of T3.8.6d. The corpus asserts the borrow
  error of `app`'s test file through `kalem lsp check` and, in the
  crate's test, through the client (RS10b). Known differences to
  record: cargo's diagnostics arrive on save, not while typing, and
  the first run after a fresh checkout builds the dependencies.
  Seen in RS3c, and the first thing RS4 needs from Kalem:
  rust-analyzer 1.99 gives its own diagnostics only to an editor that
  asks (`textDocument/diagnostic`, the protocol's pull model), even to
  one that never said it can, and sends `workspace/diagnostic/refresh`
  when they change. What it pushes (`publishDiagnostics`) is cargo's,
  and a first batch of its own that it takes back when it reloads the
  workspace after the build scripts. Kalem's client has only the push
  model, so no diagnostic of rust-analyzer's own has ever reached
  Kalem: on the corpus with an unresolved import and a function named
  `BadName`, `kalem lsp check` lists cargo's E0432 and nothing else,
  while a pull answers rust-analyzer's `non_snake_case` too; type
  mismatches, missing fields and "unlinked file" go the same way.
  Kalem's part, T3.8.2's "push and pull" for every server that has
  it (rust-analyzer, typescript-language-server's successors, Roslyn,
  clangd's newer releases): `textDocument.diagnostic` and
  `workspace.diagnostics.refreshSupport` declared; a document's
  diagnostics pulled after it opens and after its changes settle,
  and again on `workspace/diagnostic/refresh`; the pulled and the
  pushed kept apart per document and shown together.
  (Done 2026-10-10 on Kalem's branch `rust-server-start`, not merged,
  and here. `checkOnSave` and `check.command` are rust-analyzer's
  defaults, described for the panel in RS3d and not sent; the default
  on save stays the owner's question. Kalem's part as made: pull
  support declared (`textDocument.diagnostic`,
  `workspace.diagnostics.refreshSupport`); a document's diagnostics
  asked for once the server is ready, after it opens and after each
  change, one request at a time per document and one more at most
  when it changes meanwhile; asked again on
  `workspace/diagnostic/refresh`; the report's `resultId` sent back,
  an "unchanged" answer keeping the report; a cancelled answer asked
  again on the next message after 300 ms; the given and the pushed
  kept apart and read together (the status bar's counts, the lists,
  `kalem lsp check`, stale when either is older than the text). And,
  found on the way: a request rust-analyzer cancels while it loads
  the project ("content modified"; code actions asked at its start
  came back so, and were shown as an error) is asked again after
  0.5, 1 and 2 s while the document is unchanged, and said as "still
  busy (loading the project)" after that. Tested in Kalem with two
  new behaviors of the fake server, `pull` (pushes `TODO`, gives
  `bad` when asked, a note after a save with a refresh, "unchanged"
  when asked again with the same report) and `busy` (two hovers
  answered "content modified"), in the client's tests and in the
  service test; without the retries the service test fails with the
  busy message. With rust-analyzer 1.99.0, the branch's terminal
  build on a copy of the corpus with a `fn BadName()` and a type
  mismatch: `kalem lsp check` lists rust-analyzer's `non_snake_case`
  warning and its type mismatch beside cargo's (Kalem 0.6.0 lists
  cargo's only), and the linked loose file of RS3c its type
  mismatch. Through Kalem's service: during the save's check the
  status bar says "rust-analyzer: cargo check", then "2 errors, 1
  warnings"; code actions at `BadName` start with rust-analyzer's
  "Rename to bad_name", and asked from the server's first second on,
  none came back as an error (one took 1.5 s, asked again). The first
  load of the corpus, the standard library indexed, took 27 s. Known
  differences, in the README: an error both find is listed twice
  (`rustc` and rust-analyzer), as VS Code lists it; cargo's after the
  save only. Merged as getkalem/kalem#27, released in Kalem 0.6.7;
  the borrow error asserted through the client (RS10b); Kalem's own
  workspace (RS10a). Then, in Kalem 0.6.12 (merged 2026-10-10,
  a180227a): the underlines and the gutter's colored
  line numbers were Kalem's since 2026-10-04 (c2483f81), T3.8.2's note
  being out of date; what was missing is made: errors, warnings and
  the rest underlined in three colors (red, orange or yellow in the
  terminal, blue; `view::Flag` in place of a bool, LaTeX's checks red
  and blue as before) and a problem over several lines underlined on
  each of them (the fake server's `SPAN`, in the service test). Open:
  the editors by hand; a mark when line numbers are off, the light
  bulb (T3.8.2).)

## RS5. Formatting: rustfmt through the server

- [~] RS5 Format Document (`SPC c f`) formats through rust-analyzer,
  which runs the toolchain's rustfmt with the crate's edition and the
  root's `rustfmt.toml`; `rustfmt.extraArgs` and
  `rustfmt.overrideCommand` described (`leptosfmt`, a nightly rustfmt
  for unstable options); rustfmt missing from the toolchain (`rustup
  component add rustfmt`) is the server's `showMessage`, shown in the
  status bar; `commands.format` for a file no server serves:
  `["rustfmt", "--edition", "2024"]` on standard input (rustfmt
  defaults to the 2015 edition, which rejects `async` and `dyn`; a
  file outside a Cargo project has no edition of its own, and 2024
  parses the older ones, `gen` as a name excepted). The corpus asserts
  rustfmt equality: `kalem lsp ask format` on an unformatted file
  equals `rustfmt --edition 2021` of it. Formatting on save and of a
  selection are T3.8.2's.
  (Done 2026-10-10, but for the editors by hand. The corpus gained
  `shapes/src/messy.rs`, unformatted on purpose (`pub mod messy;` in
  `shapes`, no warning), and `corpus/ws/rustfmt.toml` with
  `use_field_init_shorthand = true`, whose effect shows (`Point { x,
  y }`) and which changes no other corpus file. Through rust-analyzer
  1.99.0, Kalem 0.6.0 and the branch alike: `kalem lsp ask format` on
  `messy.rs` equals rustfmt's output for the 2024 edition, the
  configuration read (rust-analyzer runs rustfmt on standard input in
  the file's folder). `commands.format` is `["rustfmt", "--edition",
  "2024"]`: 2021 would reject let chains; run in the root as Kalem
  runs it, so the project's `rustfmt.toml` is read. Through Kalem's
  service with the server off, Format Document's command formats
  `messy.rs` as rustfmt does. The conformance test runs the command
  as Kalem does: the module formatted, the configuration applied, a
  second run changing nothing, equal to rustfmt's output for the file
  by its path, and every other corpus file (but `script.rs`, whose
  frontmatter is nightly's) left as it is. Not as this item expected:
  rustfmt missing from the toolchain is no `showMessage`. rustup's
  proxy exits 1 ("'rustfmt' is not installed for the … toolchain"),
  rust-analyzer takes that for a syntax error, logs "rustfmt failed"
  and answers `null`, which is also its answer for a formatted file,
  and Kalem said "Already formatted". Kalem's part, on its branch: a
  `null` answer said as "rust-analyzer changed nothing", an empty list
  still as formatted; the fake server's `null` for a text it cannot
  read, in the service test. The README's formatting section says
  where the reason is (`kalem lsp check --log`). Merged as
  getkalem/kalem#27, released in Kalem 0.6.7. Open: the editors by
  hand.)

## RS6. What the client already has, checked and written down

- [~] RS6 Each feature the client implements, exercised on the corpus
  in both editors and recorded in the README's table as elixir's
  (feature, Vim keys, Word-like keys, command): completion as you
  type and on the server's trigger characters (`.`, `:`, `'`, `(`),
  with the item's rustdoc beside the list, fetched with
  `completionItem/resolve`; a trait method completed with its `use`
  added (the item's `additionalTextEdits`, which the client applies
  with the edit or does not: checked, and if not, T3.8.2's "automatic
  imports" named as what it waits on); the call's signature after `(`
  and `,`; documentation at the cursor (`K`, `SPC c k`) with the
  rustdoc's Markdown rendered and its fenced code highlighted as Rust;
  definition, declaration, type definition, implementations (`SPC c
  i` lists a trait's impls) and references across the two crates;
  rename across the two crates as one undo step, a field's rename
  reaching its struct literals; document symbols in the outline
  (`SPC s i`); `code.restartServer` after a `Cargo.toml` edit the
  server's own watcher missed. Each row asserted in the crate's test
  of RS10b. Written down as known differences: the position encoding
  negotiated is UTF-8, which rust-analyzer offers, so the mapping is
  the simpler one; hover on a `std` item carries the whole rustdoc
  page, long in the terminal editor's card; a workspace symbol search
  (`SPC s I`, `SPC c J`) waits for T3.8.2.
  (Done 2026-10-10 through Kalem's service, on a copy of the corpus
  with rust-analyzer 1.99.0 and the branch, but for the editors by
  hand and RS10b's assertions. Loaded in 11.5 s. Completion after
  `circle.`: `scaled` and `radius` first, each with its rustdoc given
  with the item (no `resolveSupport` declared, so rust-analyzer sends
  it at once; `completionItem/resolve` is not needed); trigger
  characters `:`, `.`, `'`, `(`. Signature after `Circle::new(`: `fn
  new(radius: f64) -> Circle` and its rustdoc. Hover on `Circle`: the
  crate, the declaration as Rust, the rustdoc. Definition and
  declaration of `scaled` into `shapes`; type definition of `circle`
  at `pub struct Circle`; implementations of `Area`, its two `impl`s;
  references of `Area` across the crates (RS1, `kalem lsp ask`); the
  file's symbols nested. Hover on `len` after an emoji on the line:
  right (UTF-8 positions). Rename of `scaled` to `grown`: "2 changes
  in 2 files", applied to both open documents, one undo step in each;
  of the field `radius` to `r`: 6 changes, `Self { radius }` became
  `Self { r }` with the parameter renamed too (rust-analyzer keeps the
  shorthand). A crate added on the disk, with the workspace's and
  `app`'s `Cargo.toml` changed: its function completed 4 s later,
  without a restart (rust-analyzer's watcher and Kalem's
  `didChangeWatchedFiles`). Not as expected: no automatic import.
  Probed directly: rust-analyzer offers an item not in scope (`area
  (use shapes::Area)`) only to an editor that declares
  `completionItem.resolveSupport` for `additionalTextEdits`, and gives
  the `use` when the item is resolved (`Area, ` into the use list);
  Kalem declares none, so the item is not offered at all. Kalem's
  part, T3.8.2's automatic imports, for every server that adds
  imports so (rust-analyzer, typescript-language-server, basedpyright,
  gopls): that property declared, and an item taken that came with
  `data` and no edits resolved before it is applied (a short wait) and
  its `additionalTextEdits` put in the same change. The README's table
  of features and its known differences. Open: that part; the editors
  by hand; RS10b.)

## RS7. Cargo.toml, and Rust in Org and Markdown source blocks

- [~] RS7a `Cargo.toml` stays the core's TOML (T2.7a.7); the plugin
  names nothing for it. Later, as the second server for the same
  files that T3.8.4 leaves open: `taplo` with SchemaStore's Cargo
  schema for the keys, and a crates server (`crates-lsp`) for the
  versions after `=`, the plugin naming them for the file name
  `Cargo.toml` only, so a plain `.toml` keeps the core's behavior;
  the design of a second server for the same files, and how their
  results merge, is Kalem's and serves Python (ruff beside
  basedpyright), PHP (PHPStan beside Intelephense) and the web plugin
  (ESLint) first.
  (Done 2026-10-10 for its first half. The plugin claims no TOML
  file, which the conformance test now checks (no extension ending in
  `toml`, no file name starting `Cargo.`). With Kalem 0.6.0, a
  `Cargo.toml` of the corpus: `kalem lsp status` says no language
  plugin serves it; it is highlighted by Kalem's TOML; `kalem check`
  says nothing of it and `kalem fmt --check` that its language has no
  formatter, T2.7a.7's TOML pack (outline, formatting, syntax errors)
  being still open in Kalem. The README says what it gets. Then the
  second server, Kalem's first, in Kalem 0.6.12 (merged 2026-10-10,
  a180227a): a language's servers beside its own
  (`alongside`), each in its own root and kept in step, their
  diagnostics with the first's under their sources, completion and
  code actions joined, any other question to the first that has it,
  `servers.KEY.enabled = false` to turn one off, Language Server
  Status and `kalem lsp status` naming them; tested with the fake
  server's `beside` linter. Here: the language `toml` ("Cargo
  manifest") claims `Cargo.toml` by its name, highlighted by Kalem's
  TOML, its server `taplo` (`taplo lsp stdio`) and `crates-lsp` beside
  it; another `.toml` file and `Cargo.lock` stay Kalem's (the
  conformance test). Taplo 0.10.0 cannot read SchemaStore's catalog
  ("failed to fetch catalog"), so its settings associate `Cargo.toml`
  with `https://json.schemastore.org/cargo.json` by name. Checked with
  the branch's terminal build, Taplo 0.10.0 and crates-lsp 0.4.3 on a
  copy of the corpus: documentation of `edition` and 112 key
  completions at `descr` (`description` first) from Taplo, Format
  Document by Taplo; `serde = "0.9.0"` said as "serde: 1.0.229"
  (information), a crate crates.io does not have as "Unknown crate" (a
  warning), version completion `1.0.229` from crates-lsp;
  `rustfmt.toml` served by no plugin. Not in the corpus test: both
  read the network. Open: the editors by hand.)
- [ ] RS7b Rust inside an Org or Markdown source block gets the same
  completion when the block's document is inside a Cargo project: a
  temporary file in the project, by setting and off by default since
  it writes to disk (T3.8.6d's words). This is the core's (the
  block's text as a document of language `rust`, synchronized to the
  server as that file), designed for every language plugin at once
  (Python and Elixir blocks the same way); the plugin's part is only
  the language id `rust` the block names, already in RS1. Listed here
  so the README's "not done" can point at it.
  (Seen 2026-10-10: a `rust` block in Org or Markdown is highlighted
  with the plugin's syntax, which Kalem finds by the name `rust` once
  the plugin is loaded (RS2's test); no server serves it. Kalem's
  todo has no task for language servers in source blocks besides
  T3.8.6d's sentence, so the core's part has no number yet. The
  README says so.
  Put off by the owner on 2026-10-10, while Kalem's other parts
  of RS4, RS7a and RS8 were made.)

## RS8. Run and test: `cargo run`, `cargo test`, and the lenses

- [~] RS8 `commands` `test` `["cargo", "test"]` and `run` `["cargo",
  "run"]` in the manifest now (read, not used until the project's run
  and test keys, T2.7i.8, `SPC p R` and `SPC p T`); `testAtPoint`
  left out with its reason: `{file}` and `{line}` cannot name the
  test, and rust-analyzer can (`experimental/runnables` at the cursor
  gives `cargo test -p CRATE -- path::to::test --exact`, and its
  `Run` and `Run Test` code lenses carry the same as the argument of
  its client command `rust-analyzer.runSingle`). Kalem's part,
  general: a lens or a runnables request whose command the plugin
  maps to a run or test command, for every server that offers
  runnables (gopls's `test` lens, ElixirLS's test lenses,
  rust-analyzer's `runSingle`), declared in the manifest as a client
  command id and the field of its argument that holds the program
  and its arguments, rust-analyzer's `cargoArgs` and `executableArgs`
  named only inside this plugin; waits on T3.8.2's lenses. The
  `Debug` lens is omitted with its reason: Kalem has no debugger
  (design document, §1.4).
  (Done 2026-10-10 for the plugin's part. `commands.test` and
  `commands.run` in the manifest, no `testAtPoint`; the conformance
  test checks them and runs the run command on the corpus (`21.57`).
  Kalem reads them and uses only `format` so far: T2.7i.8 lists `p R`
  and `p T` as not done. On the corpus `cargo test` fails, on purpose
  (`app/tests/borrow.rs`, RS4's error). Probed with rust-analyzer
  1.99.0: `experimental/runnables` at the unit test gives five, the
  test's `cargo test --package app --bin app --
  tests::total_adds_the_areas --exact --nocapture --include-ignored`
  in the workspace's root (it passes, `--bin app` not building the
  file with the error), the module's, `cargo check -p app
  --all-targets`, `cargo run -p app` and `cargo test -p app
  --all-targets`. `textDocument/codeLens` gives nothing to a client
  that does not list `rust-analyzer.runSingle` in
  `experimental.commands`; with it, "Run" over `main`, "Run Tests"
  over the module and "Run Test" over the test, each that command with
  the runnable as its argument (`cargoArgs`, `executableArgs`,
  `workspaceRoot`). Kalem's part as this item says, with that: the
  manifest names the client command a server's lens or runnable
  carries and how its argument makes a program, its arguments and a
  folder, and Kalem declares the command to the server, shows the
  lens, and runs it as `SPC p T` runs the project's tests; gopls and
  ElixirLS name theirs. Then T2.7i.8's keys, in Kalem 0.6.12 (merged
  2026-10-10, a180227a): Run Project, Test
  Project and Test at Cursor (`project.run`, `project.test`,
  `project.testAtCursor`; `SPC p R`, `SPC p T`) run the file's
  plugin's `run`, `test` and `testAtPoint` in the project's root as
  its server finds it, `{file}` and `{line}` filled in, the output in
  a read-only document as it comes (standard error too) and how it
  ended there and in the status bar; the same command again, or its
  document closed, stops it; without the plugin's, a command is asked
  for and run by the shell. With the branch's terminal build on the
  corpus, `SPC p T` (Vim keys) opens "cargo test (ws)" with
  `borrow.rs`'s E0502 and ends "cargo test: exit status 101". Open:
  the test at the cursor by rust-analyzer's runnables and lenses (the
  part above); the editors by hand.)

## RS9. rust-analyzer's own requests, where Kalem can show them (later)

- [~] RS9 The server's extensions, each a general shape Kalem gains
  once and the plugin declares by name: *text at the cursor, shown in
  a read-only document* (`rust-analyzer/expandMacro`,
  `rust-analyzer/viewSyntaxTree` of the selection,
  `rust-analyzer/analyzerStatus`; the same shape serves clangd's
  `textDocument/ast` and ElixirLS's expand macro through
  `workspace/executeCommand`, so the command `code.expandMacro` is one
  key in both plugins); *an edit at the cursor*
  (`experimental/joinLines`, `experimental/moveItem` up and down,
  `experimental/onEnter`, which continues a `///` comment on Enter);
  *a location* (`experimental/parentModule`,
  `experimental/openCargoToml`; clangd's
  `textDocument/switchSourceHeader` is this shape); *a URL opened*
  (`experimental/externalDocs`, "open docs"); *a workspace edit from
  two fields* (`experimental/ssr`, structural search and replace, as
  a query-replace with a preview); *a notification shown as the
  server's state* (`experimental/serverStatus` with its `health` and
  `quiescent`, the status bar's "indexing" until quiescent; clangd's
  `textDocument/clangd.fileStatus` and Metals's `metals/status` are
  the same shape); and `rust-analyzer/reloadWorkspace` and
  `rust-analyzer/rebuildProcMacros` as commands. The manifest names
  the method, the shape and the command id for each; the client
  learns the shapes, not the methods. Proposed in the plugin's README
  under "not done" with this list until Kalem has the shapes; done
  when expand macro and open docs work on the corpus's macro call.
  (Done 2026-10-10 on Kalem's branch `rust-server-start`, not merged,
  and here, for the shapes of text, a page, a place, edits and none;
  the server's state in the status bar, structural search and
  replace and Enter's `onEnter` left. Kalem's part as made: the
  manifest's `requests`, keyed by Kalem's command, each a `method`,
  `params` (`position`, `document`, `range`, `ranges`, `none`, with
  `extra` fields), a `shape` (`text` shown as documentation is, with
  a `title` and a `language`; `url` opened in the browser, `http` and
  `https` only, by the editors, not by the core; `location`;
  `edits`, snippet markers taken out; `none`), `answer` pointers,
  and a `server` when only one of the plugin's has it; Kalem's
  commands `code.expandMacro`, `code.openDocs`, `code.parentModule`,
  `code.openManifest`, `code.reloadProject`, `code.joinLines`,
  `code.moveItemUp` and `code.moveItemDown`, each shown where the
  document's server has the request (the when-clause
  `server:code.expandMacro`), "does not provide" elsewhere; `kalem
  lsp ask code.… FILE LINE:COL` asks one (an address printed, not
  opened). Tested with the fake server's own requests, one per
  shape, in the service test, and the reading in a unit test. With
  rust-analyzer 1.99.0 through the branch's terminal build, on the
  corpus: Expand Macro on `square!(3.0)` gives `square!` and
  `shapes::Square { side: 3.0 }` as Rust; Open Documentation on
  `println!` gives doc.rust-lang.org's page, on `square!` a docs.rs
  address that exists only for a published crate; Go to Parent Module
  from `messy.rs` lands on `pub mod messy;`, Go to Project File on
  `app/Cargo.toml`; Move Item Down moves `fn new` with its comment
  below `distance`; Join Lines and Reload Project do theirs. No
  syntax tree: `rust-analyzer/viewSyntaxTree` answers a JSON tree for
  VS Code's own view, on one line, so neither the manifest nor Kalem
  has the command. The README's table of the requests. Merged as
  getkalem/kalem#27, released in Kalem 0.6.7. Then the three shapes
  left, on Kalem's branch `rust-server-requests` (2026-10-10, not
  merged), each general: (1) the server's state, a server's
  `capabilities` added to Kalem's where Kalem says nothing
  (rust-analyzer sends `experimental/serverStatus` only to a client
  with `serverStatusNotification`) and its `status`, the
  notification's method and JSON pointers for its text, its level (the
  server's words for a warning and an error) and the values that say
  it is idle (clangd's file status and Metals's status fit the same
  fields); the client keeps the last state, the status bar says the
  text while the server works only in part (after the problem on the
  cursor's line and the progress, before the counts), Language Server
  Status too, a server not idle counts as busy so `kalem lsp check`
  waits, and `check` prints the state on standard error. (2) Enter:
  the request keyed `edit.newline` (the command Enter runs in code) is
  asked as Enter makes a new line there and in Vim's Insert mode,
  before Kalem's new line reaches the server; nothing waits, Kalem's
  new line is made at once, and the server's edits, read against the
  text Enter was pressed in, take its place as one edit of the text
  now when Kalem's new line is the one change since (the document's
  text revision), joined to its undo step (`History::join_next`) with
  the cursor at the snippet's `$0`; dropped quietly otherwise, and
  `null` leaves Kalem's. The `edits` shape keeps a snippet's cursor
  for every request (Move Item's `$0`). (3) Structural Search and
  Replace (`code.structuralReplace`): `{search}` and `{replace}` in a
  request's `extra` are asked one after the other (labels of Kalem's
  own, Turkish too) and `{selections}` is the selection in a list,
  empty when nothing is selected; the shape `workspaceEdit` offers the
  edits as a rename's are. `kalem lsp ask` takes `--input NAME=TEXT`
  and asks `edit.newline` too; a request that finds nothing says
  "nothing for" its command (it said "no that request"). Tested with
  the fake server's `fake/status`, `fake/onEnter` and `fake/ssr` in
  the service test (one undo step, the cursor, an answer dropped when
  typed on, nothing outside a comment, the selection, a refused query,
  the state in the status bar and the report, busy), the reading in
  unit tests. With rust-analyzer 1.99.0 through the branch's terminal
  build: Enter at the end of `/// A shape with an area.` in
  `shapes/src/lib.rs` gives `/// ` on the next line in the Word and
  the Vim profiles (a pty); SSR `square!($a) ==>> square!($a + 1.0)`
  offers 2 changes in `main.rs`, `Circle::new($a)` 1, `square!(` is
  refused with rust-analyzer's parse error; a project whose path
  dependency is missing says "cargo check failed to start: …". The
  corpus test asserts rust-analyzer's `onEnter` and `ssr` answers.
  Merged as getkalem/kalem#28 (2026-10-10), in Kalem 0.6.10. Then the
  pin of `kalem-highlight` and `kalem-lsp` moved to e24a9959 (#28's
  merge) and the corpus test asserts rust-analyzer's state, started
  with the manifest's `capabilities` and `status`: healthy and
  quiescent once the corpus loaded; without the capability it tells
  none (checked, the test failing after two minutes). Open: the
  graphical editor by hand.)

## RS10. Speed, the big corpus and the tests

- [~] RS10a Kalem's own repository as the corpus by hand: opened in
  the GUI and the TUI, rust-analyzer's indexing (minutes, gigabytes)
  shown as progress, no keystroke delayed meanwhile, completion
  within the indexing time once it ends (the `lsp` completer's 1.5 s
  budget against rust-analyzer's first answer measured and recorded),
  `cargo check` on save against the terminal's `cargo build` (the
  lock on `target/`, the `cargo.targetDir` setting's case), a
  `Cargo.toml` edit reloading the workspace through the server's own
  watcher; memory and time recorded in the README.
  (Done 2026-10-10 through Kalem's service, not in the editors by
  hand: a throwaway example on Kalem's branch opened its own
  `crates/kalem-core/src/lsp.rs` with this plugin, three runs. The
  release run, edits in the first minute only: ready at 0.1 s; the
  indexing to 67 s; the first hover at 78 s (116 ms); completion after
  `spec.` 24 items in 90 ms, then 7 ms, inside the `lsp` completer's
  1.5 s; 263 edits while it indexed, `lsp::sync` median 0.09 ms, p99
  0.46 ms, slowest 1.26 ms, `lsp::tick` slowest 0.56 ms, so no
  keystroke waits; memory 3.2 GB loaded, 4.4 GB at its peak (2.3 and
  3.1 GB in the other runs); a save's check 2 s, its progress in the
  status bar; `cargo build -p org-syntax` meanwhile 1.1 s, "Blocking
  waiting for file lock on package cache" for a moment; a
  `Cargo.toml` touched, reloaded at 0.2 s and done at 1.2 s. The
  debug run, with an edit every 200 ms throughout: the indexing
  restarted by each one, ending at 285 s, the first check 97 s, the
  slowest sync 212 ms (a debug build). A run that took a pause in the
  progress for the end asked completion too early: nothing within the
  1.5 s budget and hover unanswered for 30 s, which is what a user
  gets until the indexing ends. The README's table. Open: the editors
  by hand; the build-folder lock during a long check, not measured,
  for the owner's `cargo.targetDir` question.)
- [~] RS10b The crate's tests: `tests/conformance.rs` (the manifest
  complete and consistent, every described setting of RS3d, the
  syntax of RS2 through Kalem's highlighter) runs everywhere (RS1
  made it, with the root and the corpus checked through Cargo); a
  corpus test through `kalem-lsp` as a dev-dependency pinned by
  revision, as elixir pins `kalem-highlight` (the client started with
  the manifest's server spec on `corpus/ws`, waiting for quiescence,
  then the assertions of RS4, RS5 and RS6: completion after `.`, the
  trait method with its `use`, hover text, definition and rename
  across the crates, the borrow diagnostic, rustfmt equality) that
  runs when `rust-analyzer` is found and says it skipped otherwise;
  `rustup component add rust-analyzer` in `ci.yml`'s test job, the
  toolchain being there already (or by hand, the open question
  below). The version checked named in the README: rust-analyzer's
  date version and the Rust it ran with.
  (Done 2026-10-10 but for CI, the owner's question. `tests/server.rs`
  with `kalem-lsp` at the revision `kalem-highlight` is pinned to: the
  manifest's server and settings, a temporary `cargo.targetDir` so
  that nothing is written into the corpus, the three files opened,
  then documentation on `Circle`, completion after `.` (`scaled` and
  the trait's `area`: before a call's parentheses rust-analyzer offers
  methods only, no field), definition of `scaled` into `shapes`,
  references of `Area` into `app`, its two implementations, rename of
  `scaled` with one edit in each file, formatting of `messy.rs` equal
  to rustfmt's with the `rustfmt.toml` read, and cargo's E0502 in the
  unopened `borrow.rs`; each asked again while rust-analyzer loads.
  13 s with rust-analyzer 1.99.0 (b940084d 2026-09-28) on Rust
  1.99.0. Without rust-analyzer on the `PATH` it says it skipped
  (checked). Not asserted, with their reasons in the file: the
  automatic `use` (RS6) and rust-analyzer's own diagnostics, which
  the client of that revision does not ask for (RS4); both once the
  pin moves past Kalem's branch. The README names the versions.
  Then, the branch merged: the pin of `kalem-highlight` and `kalem-lsp`
  moved to b0b87650 (Kalem's `main` with getkalem/kalem#27, 0.6.6 and
  after; only these two packages changed in the lock, the old
  `kalem-highlight` kept for elixir), `completion_items` taking its
  new `keep_raw`; and rust-analyzer's own diagnostics asserted: `lib.rs`
  opened with a `pub fn BadName() {}` added in memory, never on the
  disk, and its `non_snake_case` from rust-analyzer, which comes only
  by pull, found on that line (with the line moved, the test fails,
  checked). Then CI: the test job's toolchain with the
  `rust-analyzer` and `rustfmt` components (the minimal profile has
  neither, and rust-analyzer's formatting runs rustfmt), and a step
  running `rust-analyzer --version` before the tests, since the test
  skips and passes when it finds none that runs. Open: the automatic
  `use`, once Kalem asks for it.)

## RS11. Release and the Book

- [x] RS11 `plugin.json` complete (`description` in the index's
  words), the README in elixir's shape (what it gives as a table, the
  server and how to install it, the project and its root, the
  settings with the TOML example, installing, not done, sources and
  licenses), a line in the repository's README table (`CODEOWNERS`
  and `index.json` have it since RS1; the index entry gets its
  download from `releases/` at the tag), the tag
  `rust-v0.1.0` once RS1 to RS6 hold on the corpus (the release
  workflow packs the folder from the tagged source, the corpus inside
  it as elixir's is); the Book's chapter `book/part-3/rust.org` from
  elixir's (what is followed: the grammar, and the server as the
  oracle; the project; installing a server; known differences with
  the versions checked; not implemented), T3.8.6d and T3.8.8 with
  this plugin's progress and D58's Rust row confirmed by the corpus,
  in the same pull request as the code (D53).
  (Done 2026-10-10 but for the release, which publishes and is the
  owner's to start. `plugin.json`'s description is the index's, the
  version 0.1.0. The README in elixir's shape with more: its status
  (what works with Kalem 0.6.0, what waits for Kalem's next release),
  what it gives as a table, the server, the project, the settings
  with the TOML, diagnostics, formatting, run and test, `Cargo.toml`
  and source blocks, a file outside a project, a large workspace,
  installing, not done, sources and licenses. A line in the
  repository's README table. In Kalem, on its branch with the code
  the chapter describes (D53): `book/part-3/rust.org` from elixir's
  (what is followed, the project, installing a server, the known
  differences with the versions checked, not implemented), linked
  from the Book's index and from "Language plugins"; `kalem book check
  book` says 66 pages, no problem. T3.8.6d and T3.8.8 with this
  plugin's progress, R5.12 with the chapter, and D58's row with Rust
  confirmed. The release itself: `main` pushed with this plugin's
  commits, then the tag `rust-v0.1.0` pushed; the release workflow
  packs the folder from the tagged source (the corpus inside it, as
  elixir's), signs it, makes the GitHub release, and commits "Publish
  rust-v0.1.0" with `releases/rust-v0.1.0.sha256` and the index's
  download. It runs with Kalem 0.6.0: the `version` and `requests`
  of the manifest are read by Kalem's next release and passed over
  by 0.6.0. Released 2026-10-10 as `rust-v0.1.0`, the owner's
  version kept: `main` pulled (rebased over docx 0.0.5), pushed, the
  tag pushed; the workflow's release carries `rust-v0.1.0.tar.gz`
  (53,570 bytes, 30 files, the corpus inside, no `target`), its
  Sigstore signature and `SHA256SUMS`; "Publish rust-v0.1.0" on
  `main` with the hash in `releases/` and the index's download; CI
  green. Then `rust-v0.1.1` with Kalem 0.6.7 (getkalem/kalem#27
  released): the pin moved, rust-analyzer's own diagnostics in the
  corpus test, the README's "not released yet" made 0.6.7. Then
  `rust-v0.1.2` with Kalem 0.6.12: `Cargo.toml` by Taplo with
  crates-lsp beside it, and the README naming 0.6.12 for it and for
  `SPC p R` and `SPC p T`; the pin of `kalem-highlight` and
  `kalem-lsp` moved to Kalem 0.6.12's version commit (dec56dcb).)

## Open for the owner

- Kalem's part of RS3c (root markers in groups, `${looseFiles}`) and
  of RS6 (automatic imports): wanted or not.

- The default on save: `cargo check` (faster) or `cargo clippy` (what
  the repository's CI runs, so the editor shows what CI will).
- `cargo.targetDir`: off as rust-analyzer ships it, or on by the
  manifest, so the server's check and a terminal `cargo build` never
  wait on each other, at the cost of a second `target/` of Kalem's
  size.
- Whether Kalem's built-in set moves to sublimehq/Packages' current
  syntaxes for every language (RS2 found its Rust older than Rust 1.39,
  and its others are of the same snapshot), after which this plugin's
  copy could go; and the scope names of RS2's note mapped to kinds in
  Kalem's highlighter.
- The first version: the manifest says `0.1.0`, as elixir's line;
  `0.0.1`, as the components', if the owner prefers it.
