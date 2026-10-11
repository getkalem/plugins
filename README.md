# Kalem plugins

The plugins of [Kalem](https://github.com/getkalem/kalem), the editor for Org, Markdown and other plain-text formats. Every mode, file type, language pack, completer and exporter beyond Kalem's small core (Org, Markdown, CSV, LaTeX) is developed here as a plugin, never in the core (Kalem's [design document](https://github.com/getkalem/kalem/blob/main/docs/design_document.md), sections 11.0 and 11.8).

**Status: early.** Kalem installs these plugins from the editor (Browse Plugins, or its offer when you open a file a plugin serves) and with `kalem plugin install NAME`. The plugin API for components (the WIT package `kalem:plugin`, 0.2.13 today, and the `kalem-plugin` bindings) still grows with Kalem's releases, and the `kalem-plugin` crate is not on crates.io yet: plugins take it from Kalem's git repository. Language plugins need no API: they are declarative.

| Plugin | What it gives | Kind |
|---|---|---|
| [`docx`](plugins/docx) | Word documents opened and edited as themselves: styles, lists, tables, pictures, notes, comments and tracked changes read; laid out by Kalem (the plugin API's flow interface); text edited and formatted in its runs, as tracked changes when the document tracks them; tracked changes accepted and rejected; comments added, answered, edited, resolved and deleted | component |
| [`elixir`](plugins/elixir) | Elixir, EEx and HEEx: highlighting, and Expert or ElixirLS for completion, documentation, definitions, references, diagnostics and formatting | language |
| [`git`](plugins/git) | Git: the status as a document, changed files with their diffs, staged by file, hunk or line with magit's keys, committed, pushed and pulled, also on Doom's `SPC g` keys; branches and stashes; changed lines marked beside the lines | component |
| [`graph`](plugins/graph) | Logseq graphs and Obsidian vaults opened as themselves: pages, journals, backlinks (a panel and a document), block references, tags and tasks found and followed on Doom's `SPC n r` keys; new journals from the graph's template; nothing converted, `logseq/` and `.obsidian/` never written | component |
| [`image-viewer`](plugins/image-viewer) | Pictures (PNG, JPEG, GIF, WebP, SVG and more) shown with zoom and their EXIF information; a JPEG turned or flipped by its orientation tag only | component |
| [`pta`](plugins/pta) | Plain text accounting: hledger, Ledger and Beancount journals, with hledger-lsp and beancount-language-server for completion and diagnostics, and the user's own tool run from Kalem: checks, balance, register and other reports, reconciliation, import from bank statements, prices | component and language |
| [`pdf-viewer`](plugins/pdf-viewer) | PDF files shown page by page with their outline, page labels, links and text | component |
| [`rust`](plugins/rust) | Rust: highlighting for the current editions, and rust-analyzer for completion, documentation, definitions, references, rename, diagnostics and formatting across a workspace's crates; Taplo and crates-lsp for `Cargo.toml` | language |
| [`xlsx`](plugins/xlsx) | Excel workbooks opened, edited and saved as themselves | component |

## How a plugin is built and shipped

- A component plugin is a Rust crate under `plugins/NAME`, compiled for `wasm32-unknown-unknown` and wrapped as a WebAssembly component (`kalem plugin build`; Kalem grants no WASI interface). Rust is the plugin language: a viewer implements the Rust trait through which Kalem uses every viewer (`kalem-viewer`'s `Viewer`), so its unit tests run natively.
- **Source in, WASM out.** Compiled components are never committed. On a tag `NAME-vX.Y.Z` the release workflow builds the component from the tagged source, hashes it, signs it with Sigstore (keyless, as the workflow) and publishes it as a release asset with `SHA256SUMS` and the signature, `NAME.wasm.sigstore.json`; `index.json` lists every published plugin and Kalem reads it as a static file.
- **What Kalem checks.** When Kalem installs a plugin by name, it checks the download against the SHA-256 that `index.json` gives. A plugin installed from a GitHub link or an archive URL has no SHA-256 to check. Kalem does not check the signature yet. Kalem has no plugins built in: every plugin is downloaded. To check a signature by hand:

  ```sh
  cosign verify-blob xlsx.wasm --bundle xlsx.wasm.sigstore.json \
    --certificate-identity https://github.com/getkalem/plugins/.github/workflows/release.yml@refs/tags/xlsx-v0.0.5 \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com
  ```
- **Language plugins are declarative** (Kalem's D57): a manifest whose `languages` and `servers` sections name file types, Sublime syntaxes and language servers, the syntax files, and settings. Kalem's core adds the syntaxes to its highlighter (resolving `extends`) and runs the servers through its one language server client; nothing of the plugin runs inside Kalem, so there is no component to build. Such a plugin is published as an archive of its folder, `NAME-vX.Y.Z.tar.gz`, and `index.json` marks it `"kind": "declarative"`. Kalem installs it like any other plugin. To try a language plugin from a checkout, set `KALEM_PLUGIN_PATH` to its parent folder (`KALEM_PLUGIN_PATH=$PWD/plugins kalem`). When the plugin is also installed, the installed copy serves the files, so remove it first (`kalem plugin remove ID`, the `id` of its `plugin.json`). Component plugins are found only among the installed ones.
- **When a plugin serves a file** is one declaration for every kind of plugin, the parts of its manifest that say it: the extensions a viewer `opens`; the extensions, whole names (`filenames`) and `#!` interpreters (`shebangs`) of its `languages`; the `markers` of its `layers`; and its own `applies`, a rule or a list of rules of `extensions`, `filenames`, `magic` (first bytes, `"25 50 44 46 2D"` for `%PDF-`, `??` for any byte), `shebangs` and `markers` (files or folders in a file's folder or above it, `.git`), a rule of both kinds asking both. `index.json` copies them as they are, and Kalem reads them from the index and from an installed manifest by the same code: the plugin it names for a file, to install or in the status bar, is the one it hands that file to once installed. Change them with the plugin's version, so that the index and the copy users install say the same.
- Kalem runs a component inside a sandbox: it sees only what the API grants, and it reaches a document only through the API. Each call has a memory and time budget: 1 GB and 10 s for a viewer, 64 MB and 100 ms for an extension plugin. A plugin's `limits` in `plugin.json` can ask for more: the workbook viewer asks for 4 GB, because a sheet of a million rows of ten cells needs 2 GB. Language servers, and the programs a plugin runs (with the permission `subprocess:NAME`), run outside the sandbox, with the user's rights. The permissions a plugin needs are declared in its `plugin.json` and shown to the user before installing.

## Adding a plugin

1. Once: `rustup target add wasm32-unknown-unknown` and `cargo install --locked wasm-tools`, which `kalem plugin build` uses.
2. `kalem plugin new NAME` copies `template/` to `plugins/NAME` and renames it; fill `plugin.json`, then run `python3 tools/build-index.py` (CI fails when `index.json` does not match the manifests).
3. `cargo test -p kalem-plugin-NAME` runs the plugin's tests (the template's `tests/conformance.rs` checks the manifest); `kalem plugin build plugins/NAME` builds the component, and `kalem plugin dev plugins/NAME` builds and installs it again at each change.
4. Add a line for yourself to `CODEOWNERS` and open a pull request. CI checks formatting and clippy, runs every plugin's tests, and builds every component against the Kalem revision that `Cargo.lock` pins.
5. When it is merged, a maintainer tags `NAME-vX.Y.Z` to publish it. The tag's version must be the `version` in `plugin.json`, or the release fails.

## What belongs here

A mode belongs in Kalem's core only when it is Org, Markdown, CSV or LaTeX. Everything else is a plugin, and a mode is worth building only when what the reader sees differs from the source text: markers hidden, objects drawn, a grid, a tree. A format whose view is its source is a **language pack** (highlighting plus an outline provider, a formatter, completion and diagnostics), also a plugin. See the section "Language packs" of the Book's chapter [Completers, modes and highlighters](https://github.com/getkalem/kalem/blob/main/book/part-3/completers-and-modes.org).

## Layout

| Path | Contents |
|---|---|
| `template/` | The crate a new plugin starts from: manifest, a small working viewer, a manifest test |
| `plugins/NAME/` | One crate per plugin |
| `crates/NAME/` | Libraries plugins share, never published on their own: `ooxml`, the Office formats' package, XML, relationships, encryption and theme (`xlsx`, `docx`) |
| `index.json` | The plugin index Kalem reads; generated by `tools/build-index.py` |
| `releases/` | One small text file per published release with its SHA-256, written by the release workflow |
| `tools/` | `build-index.py` |
| `shims/` | `web-time`, a small stand-in for the crates.io crate (set in `[patch.crates-io]` of `Cargo.toml`). It gives a component the time without a browser's JavaScript; `cfb`, which `xlsx` and `docx` use, needs it |

## License

MIT OR Apache-2.0, as Kalem. Every plugin in this repository is licensed the same way; a plugin kept in its own repository chooses its own license.
