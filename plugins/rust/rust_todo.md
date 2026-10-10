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
  Open: the branch reviewed and merged, and a Kalem released with it;
  the status bar seen in the two editors by hand.)
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
  section. (2) stays recorded. Open: the branch merged and released;
  the editors by hand; a crates.io dependency in the corpus, which
  needs its tests to fetch it.)
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
  (not in CI, which has no component), each key one of its settings
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
  save only. Open: the branch merged and released; the editors by
  hand (underlines and gutter marks are T3.8.2's open part); the
  borrow error asserted through the client (RS10b); Kalem's own
  workspace (RS10a).)

## RS5. Formatting: rustfmt through the server

- [ ] RS5 Format Document (`SPC c f`) formats through rust-analyzer,
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

## RS6. What the client already has, checked and written down

- [ ] RS6 Each feature the client implements, exercised on the corpus
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

## RS7. Cargo.toml, and Rust in Org and Markdown source blocks

- [ ] RS7a `Cargo.toml` stays the core's TOML (T2.7a.7); the plugin
  names nothing for it. Later, as the second server for the same
  files that T3.8.4 leaves open: `taplo` with SchemaStore's Cargo
  schema for the keys, and a crates server (`crates-lsp`) for the
  versions after `=`, the plugin naming them for the file name
  `Cargo.toml` only, so a plain `.toml` keeps the core's behavior;
  the design of a second server for the same files, and how their
  results merge, is Kalem's and serves Python (ruff beside
  basedpyright), PHP (PHPStan beside Intelephense) and the web plugin
  (ESLint) first.
- [ ] RS7b Rust inside an Org or Markdown source block gets the same
  completion when the block's document is inside a Cargo project: a
  temporary file in the project, by setting and off by default since
  it writes to disk (T3.8.6d's words). This is the core's (the
  block's text as a document of language `rust`, synchronized to the
  server as that file), designed for every language plugin at once
  (Python and Elixir blocks the same way); the plugin's part is only
  the language id `rust` the block names, already in RS1. Listed here
  so the README's "not done" can point at it.

## RS8. Run and test: `cargo run`, `cargo test`, and the lenses

- [ ] RS8 `commands` `test` `["cargo", "test"]` and `run` `["cargo",
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

## RS9. rust-analyzer's own requests, where Kalem can show them (later)

- [ ] RS9 The server's extensions, each a general shape Kalem gains
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

## RS10. Speed, the big corpus and the tests

- [ ] RS10a Kalem's own repository as the corpus by hand: opened in
  the GUI and the TUI, rust-analyzer's indexing (minutes, gigabytes)
  shown as progress, no keystroke delayed meanwhile, completion
  within the indexing time once it ends (the `lsp` completer's 1.5 s
  budget against rust-analyzer's first answer measured and recorded),
  `cargo check` on save against the terminal's `cargo build` (the
  lock on `target/`, the `cargo.targetDir` setting's case), a
  `Cargo.toml` edit reloading the workspace through the server's own
  watcher; memory and time recorded in the README.
- [ ] RS10b The crate's tests: `tests/conformance.rs` (the manifest
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

## RS11. Release and the Book

- [ ] RS11 `plugin.json` complete (`description` in the index's
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

## Open for the owner

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
- The corpus test against a real rust-analyzer in CI (RS10b, the
  component installed in `ci.yml`), or by hand as the elixir plugin
  was checked.
- The first version: the manifest says `0.1.0`, as elixir's line;
  `0.0.1`, as the components', if the owner prefers it.
