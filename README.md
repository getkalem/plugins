# Kalem plugins

The plugins of [Kalem](https://github.com/getkalem/kalem), the editor for Org, Markdown and other plain-text formats. Every mode, file type, language pack, completer and exporter beyond Kalem's small core (Org, `.klm`, Markdown, CSV, LaTeX) is developed here as a plugin, never in the core (design document, sections 11.0 and 11.8).

**Status: early.** The plugin API for components (a WIT definition and the `kalem-plugin` bindings, Kalem's task T3.1.3) is not published yet. Language plugins need no API: they are declarative, and Kalem loads them today.

| Plugin | What it gives | Kind |
|---|---|---|
| [`docx`](plugins/docx) | Word documents opened and edited as themselves: styles, lists, tables, notes, comments and tracked changes read; laid out by Kalem (plugin API 0.2.7's flow interface); text edited in its runs, as tracked changes when the document tracks them; tracked changes accepted and rejected | component |
| [`elixir`](plugins/elixir) | Elixir, EEx and HEEx: highlighting, and Expert or ElixirLS for completion, documentation, definitions, references, diagnostics and formatting | language |
| [`git`](plugins/git) | Git: the status as a document, changed files with their diffs, staged by file, hunk or line, committed and pushed on Doom's `SPC g` keys (phase 0: the library) | component |
| [`pdf-viewer`](plugins/pdf-viewer) | PDF files shown page by page with their outline, page labels, links and text | component |
| [`rust`](plugins/rust) | Rust: highlighting for the current editions, and rust-analyzer for completion, documentation, definitions, references, rename, diagnostics and formatting across a workspace's crates | language |
| [`xlsx`](plugins/xlsx) | Excel workbooks opened, edited and saved as themselves | component |

## How a plugin is built and shipped

- A plugin is a Rust crate under `plugins/NAME`, compiled for `wasm32-unknown-unknown` and wrapped as a WebAssembly component (`kalem plugin build`; Kalem grants no WASI interface). Rust is the plugin language: the contract a viewer implements is the trait Kalem's own viewers implement, so its unit tests run natively.
- **Source in, WASM out.** Compiled components are never committed. On a tag `NAME-vX.Y.Z` the release workflow builds the component from the tagged source, hashes it, signs it with Sigstore (keyless, as the workflow) and publishes it as a release asset with `SHA256SUMS` and the signature, `NAME.wasm.sigstore.json`; `index.json` lists every published plugin and Kalem reads it as a static file.
- **What Kalem checks.** Kalem checks a download against the SHA-256 `index.json` gives, and the plugins it has built in against the SHA-256 its own source pins; it does not check the signature yet. To check one by hand:

  ```sh
  cosign verify-blob xlsx.wasm --bundle xlsx.wasm.sigstore.json \
    --certificate-identity https://github.com/getkalem/plugins/.github/workflows/release.yml@refs/tags/xlsx-v0.0.5 \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com
  ```
- **Language plugins are declarative** (Kalem's D57): a manifest whose `languages` and `servers` sections name file types, Sublime syntaxes and language servers, the syntax files, and settings. Kalem's core adds the syntaxes to its highlighter (resolving `extends`) and runs the servers through its one language server client; nothing of the plugin runs inside Kalem, so there is no component to build. Such a plugin is published as an archive of its folder, and `index.json` marks it `"kind": "declarative"`. Until Kalem's installer exists, a plugin folder is used from `~/.config/kalem/plugins/` or from the folders of `KALEM_PLUGIN_PATH`.
- **When a plugin serves a file** is one declaration for every kind of plugin, the parts of its manifest that say it: the extensions a viewer `opens`; the extensions, whole names (`filenames`) and `#!` interpreters (`shebangs`) of its `languages`; the `markers` of its `layers`; and its own `applies`, a rule or a list of rules of `extensions`, `filenames`, `magic` (first bytes, `"25 50 44 46 2D"` for `%PDF-`, `??` for any byte), `shebangs` and `markers` (files or folders in a file's folder or above it, `.git`), a rule of both kinds asking both. `index.json` copies them as they are, and Kalem reads them from the index and from an installed manifest by the same code: the plugin it names for a file, to install or in the status bar, is the one it hands that file to once installed. Change them with the plugin's version, so that the index and the copy users install say the same.
- Kalem runs a plugin inside a sandbox: it sees only what the API grants, within a time and memory budget (a viewer's is 1 GB and 10 s a call unless its manifest's `limits` asks for others, as the workbook viewer's 4 GB: a sheet of a million rows needs 2), and never touches the document text, only ranges and edits. The permissions a plugin needs are declared in its `plugin.json` and shown to the user before installing.

## Adding a plugin

1. `kalem plugin new NAME` copies `template/` to `plugins/NAME` and renames it; fill `plugin.json`.
2. `cargo test -p kalem-plugin-NAME` runs the conformance tests; `kalem plugin build plugins/NAME` builds the component, and `kalem plugin dev plugins/NAME` builds and installs it again at each change.
3. Add a line for yourself to `CODEOWNERS` and open a pull request. CI builds every plugin against the current API and runs the conformance suite.
4. When it is merged, tag `NAME-vX.Y.Z` to publish.

## What belongs here

A mode belongs in Kalem's core only when it is Org, `.klm`, Markdown, CSV or LaTeX. Everything else is a plugin, and a mode is worth building only when what the reader sees differs from the source text: markers hidden, objects drawn, a grid, a tree. A format whose view is its source is a **language pack** (highlighting plus an outline provider, a formatter, completion and diagnostics), also a plugin. See group 2.7g of Kalem's `todo.md`.

## Layout

| Path | Contents |
|---|---|
| `template/` | The crate a new plugin starts from: manifest, stub, conformance test |
| `plugins/NAME/` | One crate per plugin |
| `crates/NAME/` | Libraries plugins share, never published on their own: `ooxml`, the Office formats' package, XML, relationships, encryption and theme (`xlsx`, `docx`) |
| `index.json` | The plugin index Kalem reads; generated by `tools/build-index.py` |
| `releases/` | One small text file per published component with its SHA-256, written by the release workflow |
| `tools/` | `build-index.py` |

## License

MIT OR Apache-2.0, as Kalem. Every plugin in this repository is licensed the same way; a plugin kept in its own repository chooses its own license.
