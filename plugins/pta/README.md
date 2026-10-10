# Plain text accounting (`pta`)

hledger, Ledger and Beancount journals in [Kalem](https://github.com/getkalem/kalem): written with completion and checks, checked by the tool you already have, and, as the work list goes on, imported from bank statements, reconciled against them and reported as documents. **The tool does the accounting**: hledger, Ledger and Beancount parse, balance and check; the plugin asks them and shows what they say. The work list is [`pta_todo.md`](pta_todo.md).

**Status: the spike (PT1)**, for Kalem 0.6.9. One plugin with two halves in one manifest, both loaded:

- **The languages and their servers**, which Kalem's core runs: hledger (`.journal`, `.hledger`, `.j`, `.hledger.journal`) with [hledger-lsp](https://github.com/ptimoney/hledger-lsp), Ledger (`.ledger`, `.ldg`), Beancount (`.beancount`, `.bean`) with [beancount-language-server](https://github.com/polarmutex/beancount-language-server), and hledger's CSV rules (`.rules`). Completion of accounts, payees and tags, diagnostics and formatting come from the servers. No highlighting yet (PT2).
- **The component**, which runs the journal's tool: for now one command, *Tool Version* (in the palette under Journal), which asks hledger, Ledger or `bean-check` what it is, or says how to install it.

`.dat` files are not claimed.

## What it needs

- Kalem 0.6.9 or later. The component starts with the first journal opened.
- The tools it runs, where you want them: `brew install hledger ledger`; `pip install beancount`. A tool not on the PATH Kalem sees is named by the plugin's setting `programs.NAME` (`programs.bean-check` for a virtual environment's).
- The servers, for completion and diagnostics: `npm install -g hledger-lsp`; `cargo install beancount-language-server`. `kalem lsp status FILE` says which runs.

## Measured

With hledger 1.52.4, Ledger 3.4.1, Beancount 3.2.3 and hledger-lsp 0.7.0 (ptimoney's, from npm), on the corpus:

- Completion after an account's first letter lists the journal's declared accounts, those of its `include`d files too.
- The deliberate error of `corpus/hledger/errors.journal`, a balance assertion that does not hold, is a diagnostic at its line.
- hledger-lsp warns that an automated posting's virtual account, `(budget:food)` in `2026.journal`, is not declared, taking the parentheses for its name; hledger itself accepts the file. The server's checks are its own, not hledger's: the tool's check (PT5) is the one to trust.

## Tests

`cargo test -p kalem-plugin-pta`: the manifest's two halves name what exists and agree; and the corpus, one journal per dialect (`corpus/hledger`, three files with `include`; `corpus/ledger`; `corpus/beancount`, two files), each passes its tool's strictest check (`hledger check --strict --auto`, `ledger --pedantic balance`, `bean-check`), and each `errors.*` file fails it at its line. A tool not installed is skipped with a note.
