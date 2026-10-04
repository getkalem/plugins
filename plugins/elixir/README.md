# Elixir

Elixir, EEx and HEEx for [Kalem](https://github.com/getkalem/kalem): highlighting, and a language server for completion, documentation, definitions, references, diagnostics and formatting.

A language plugin is declarative (Kalem's D57): this folder holds a manifest, `plugin.json`, the Sublime syntaxes under `syntaxes/`, and nothing that runs inside Kalem. Kalem's core reads the manifest, adds the syntaxes to its highlighter and starts the language server through its one client.

## What it gives

| File types | Highlighting | Server |
|---|---|---|
| `.ex`, `.exs`, `mix.lock`, `.formatter.exs`, `#!/usr/bin/env elixir` | Elixir, with `~H` sigils as HEEx, `~L` and `~E` as EEx, `~r` as a string, Markdown in `@doc` | yes |
| `.heex`, `.html.heex` | HTML (HEEx): components (`<.button>`, `<Module.comp>`), `{…}` and `<%= … %>` as Elixir | yes |
| `.eex`, `.html.eex`, `.leex` | HTML (EEx) | yes |

With a server running, in either of Kalem's editors:

| Feature | Vim profile | Word-like profile | Command |
|---|---|---|---|
| Completion, with each item's documentation beside the list | as you type, `.` and `:` | as you type | |
| A call's signature while typing its arguments (ElixirLS) | after `(` and `,` | after `(` and `,` | |
| Documentation at the cursor | `K`, `SPC c k` | | `code.documentation` |
| Go to definition | `gd`, `SPC c d` | F12 | `code.definition` |
| References | `gD`, `SPC c D` | Shift+F12 | `code.references` |
| Rename, with a preview of the changes | `SPC c r` | F2 | `code.rename` |
| Code actions (fixes, refactorings) | `SPC c a` | Ctrl+. | `code.actions` |
| Problems of the file, of open files | `SPC c x`, `SPC c X` | | `code.problems`, `code.allProblems` |
| Format the document (`mix format` when no server runs) | `SPC c f` | | `edit.formatDocument` |
| Problems underlined, the line number colored, the message under the mouse | in the text | in the text | |
| The problem on the cursor's line, the server's progress (Expert's build too) | the status bar | the status bar | |

Diagnostics are the compiler's, and Credo's and dialyzer's where the server runs them. Dialyzer is off by default: its first run builds a large table and takes minutes (turn it on with `dialyzerEnabled`, below). Files changed outside the editor (a checkout, `mix deps.get`) are told to the server, which compiles again.

## The server

The first of these that is installed is used; set `server` to choose one.

1. **Expert**, the Elixir team's language server: `expert --stdio` on the `PATH`. Releases: <https://github.com/expert-lsp/expert/releases>.
2. **ElixirLS**: `elixir-ls` or `language_server.sh` (`language_server.bat` on Windows) on the `PATH`, or in `~/.elixir-ls/release/`. `brew install elixir-ls`, or a release from <https://github.com/elixir-lsp/elixir-ls/releases> unzipped to `~/.elixir-ls/release`.

Expert builds its engine for the project's Elixir the first time (about forty seconds, cached after), then compiles and indexes the project; until then it answers about the standard library but not yet about the project's own code. Both servers were checked with `corpus/hello`: Expert 0.1.11 and ElixirLS 0.27.2, on Elixir 1.20 with OTP 28.

Kalem never installs a server by itself. When none is found, the status bar says so with these instructions.

The root is the outermost folder with a `mix.exs`, so an umbrella project is one workspace, and its applications under `apps/` are workspace folders of their own. A file outside a Mix project (a script beside no `mix.exs`) is highlighted but gets no server: starting one per folder is heavy, and ElixirLS would leave an `.elixir_ls` folder there. The status bar says so.

## Settings

In Kalem's `settings.toml`:

```toml
[plugins."org.kalem.elixir"]
server = "auto"            # "expert", "elixir-ls", or "off"

# Merged over the server's own settings (sent as `workspace/configuration`).
[plugins."org.kalem.elixir".settings.elixirLS]
mixEnv = "dev"             # MIX_ENV for the server's builds; the plugin's default is "test"
dialyzerEnabled = true     # off by default
fetchDeps = false

# Another program for a server, with its arguments, and its environment.
[plugins."org.kalem.elixir".servers.elixir-ls]
command = ["/opt/elixir-ls/language_server.sh"]
env = { ELS_INSTALL_PREFIX = "/opt/elixir-ls" }
```

`kalem lsp status FILE` shows what serves a file, and why nothing does. `kalem lsp check FILE…` prints a server's diagnostics without the editor. `kalem lsp ask hover FILE LINE:COL` asks a server one question.

## Installing

From Kalem: the Kalem menu's **Install Plugin…**, then `elixir` (or this folder's link, `https://github.com/getkalem/plugins/tree/main/plugins/elixir`). **Browse Plugins** lists it too. Kalem shows what the plugin is and which programs it may run before installing, and the open Elixir files are highlighted and served at once. On the command line: `kalem plugin install elixir`.

To work on the plugin itself, point Kalem at this repository's `plugins/` folder instead: `KALEM_PLUGIN_PATH=$PWD/plugins kalem`.

## Not done

- No embedded `iex`. Kalem ships no REPL (its D28); the terminal is one key away.
- Running tests at the cursor (`mix test FILE:LINE`) and the project's tests: the manifest lists the commands, and the editor runs them when Kalem's project run and test keys land (its tasks T2.7i.8 and T3.8.4).
- Regular expressions in `~r` are highlighted as strings: the PCRE syntax of the upstream package uses subroutine calls, which the pure-Rust regex engine of Kalem's highlighter does not have.

## Sources and licenses

- `syntaxes/*.sublime-syntax`: from [princemaple/elixir-sublime-syntax](https://github.com/princemaple/elixir-sublime-syntax) at `b63f8f0`, MIT (`syntaxes/LICENSE-elixir-sublime-syntax.txt`), unchanged.
- `syntaxes/bases/HTML*.sublime-syntax`: from [sublimehq/Packages](https://github.com/sublimehq/Packages) at `b2bdd29`, under its permissive license (`syntaxes/bases/LICENSE-sublimehq-packages.txt`), unchanged. They are bases for `extends` only, not languages of their own, so Kalem's HTML keeps `.html` files.
- `corpus/hello`: a Mix project for the tests, under this repository's license.
