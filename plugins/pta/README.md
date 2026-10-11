# Plain text accounting (`pta`)

hledger, Ledger and Beancount journals in [Kalem](https://github.com/getkalem/kalem): written with completion and checks, checked by the tool you already have, and, as the work list goes on, imported from bank statements, reconciled against them and reported as documents. **The tool does the accounting**: hledger, Ledger and Beancount parse, balance and check; the plugin asks them and shows what they say. The work list is [`pta_todo.md`](pta_todo.md).

**Status: the spike (PT1), the highlighter (PT2), the servers (PT3), aligning amounts (PT4), checking by the tool (PT5), writing transactions (PT6), the balance and the register (PT7), reconciliation (PT8), import from bank statements (PT9) and prices (PT10)**, for Kalem 0.6.9 and later. One plugin with two halves in one manifest, both loaded:

- **The languages and their servers**, which Kalem's core runs: hledger (`.journal`, `.hledger`, `.j`, `.hledger.journal`) with [hledger-lsp](https://github.com/ptimoney/hledger-lsp), Ledger (`.ledger`, `.ldg`), Beancount (`.beancount`, `.bean`) with [beancount-language-server](https://github.com/polarmutex/beancount-language-server), and hledger's CSV rules (`.rules`). Completion of accounts, payees and tags, diagnostics and formatting come from the servers.
- **The component**, which runs the journal's tool: a status bar item naming the tool and the main journal of the journal you are in (`hledger · 2026.journal`; its tooltip says why that journal), and one command, *Tool Version* (in the palette under Journal, and the item's click), which asks hledger, Ledger or `bean-check` what it is, or says how to install it.
- **The highlighting**, the plugin's own syntaxes: one base for what the journals share (dates, statuses, descriptions, accounts, amounts and commodities, costs, lots, balance assertions, comments and their tags, periodic and automated transactions, directives), with hledger, Ledger and Beancount over it, and hledger's CSV rules beside them. Journals in Org's source blocks and Markdown's fenced blocks (`hledger`, `ledger`, `beancount`) are colored the same. Dates and amounts take the numbers' color, accounts the types', descriptions and payees the strings', statuses and directives the keywords', tag names the tags'; a tag is colored only where the tool reads one (not in a comment line of the file's own, nor in Beancount's comments).

`.dat` files are not claimed.

Nothing but the tool you already use is needed: the plugin computes no balance and has no engine of its own. hledger, Ledger and Beancount are the reference; the servers, where installed, add completion and their own checks.

## What it needs

- Kalem 0.6.9 or later. The component starts with the first journal opened.
- The tool you use: `brew install hledger ledger`; `pip install beancount`. A tool not on the PATH Kalem sees is named by the plugin's setting `programs.NAME` (`programs.bean-check` for a virtual environment's).
- The journal's folder among your projects (`projects.toml`): Kalem runs a plugin's programs in a project's folder only.
- The servers, if you want completion and their checks: one of the two hledger-lsp programs below; `brew install beancount-language-server` (or `cargo install beancount-language-server`). `kalem lsp status FILE` says which runs. Ledger has no server.

## The main journal and the tool

A journal that `include`s others is checked and reported as one: checking an included file alone gives false errors (an account opened in another file is "unknown"). The plugin's setting `journal` names the main journal (absolute, from `~/`, or beside the file). Unset, an hledger journal's main journal is the one `LEDGER_FILE` names when that journal includes the file (`hledger files` says so), a Beancount journal's the one its server is told (below), else the file itself. Kalem started from the Dock or a launcher may not see the `LEDGER_FILE` of your shell: set `journal` then.

The setting `tool` picks the program for hledger's and Ledger's journals: `auto` takes the file's own, and a `.ledger` file goes to Ledger where Ledger is installed and to hledger where only hledger is (hledger does not read Ledger's value expressions, such as `(-$132.00)`). A Beancount journal is always Beancount's.

## The Journal menu

In a journal and in the Check document, the menu bar (F10 in the terminal) has a **Journal** menu with the commands below, and the toolbar (the foot of the terminal's sidebar) a **Journal** button; the button, `SPC m m` and `?` in the Check document offer the journal's commands as a list to pick from, each with its keys, and run the one picked where you were.

## Writing transactions

In a journal, under `SPC m` in Vim's command mode and in the palette's Journal group, each one undo step:

- *New Transaction* (`SPC m t`): asks the date (today offered; `yesterday`, `-3`, `5`, `10/5`, `friday`, `last friday` taken) and the payee (the journal's, the last written first, or another), then writes the transaction the tool drafts from that payee's last one (`hledger print --match`, `ledger xact`, Beancount's last entry), dated, without its old balance assertions, at its date's place (or at the end, `insert_at`), the cursor on its first amount.
- *Toggle Status* (`SPC m c`): unmarked, pending `!`, cleared `*`, unmarked (Beancount's `txn`, `!`, `*`); the transactions selected all to the first one's next; the posting at the cursor with `toggle_postings`.
- *Copy Transaction* (`SPC m y`): the transaction again, dated today, at its date's place.
- *Next* and *Previous Transaction* (Alt+Down, Alt+Up).
- *Date a Day Later* and *Earlier* (`ctrl+shift+l`, `ctrl+shift+h` in Vim's command mode).
- *Sort Transactions* (`SPC m s`): those selected, or the file's, by date, each with the comment lines above it; refused where a directive between them (`apply account`, `alias`, `include`, `pushtag`…) would apply to others after sorting.

The outline is the server's document symbols.

## The balance and the register

*Balance* (`SPC m b`) opens the main journal's balance as a document: assets, liabilities and equity at the period's end, then income, expenses and the other accounts within the period, as a tree (each parent with its subaccounts' amounts), every amount and total as the tool computes and writes it. Tab, Right and Left fold and unfold an account (kept as you leave it), `p` picks the period (this month, last month, this quarter, this year, last year, all dates), `d` the depth, `v` shows market values (or back to cost), `r` runs it again, Enter opens the account's register, `q` or Escape closes it. It is written again when the journal is saved.

*Register* (`SPC m R` on a posting's account, or Enter in the balance; else an account to pick) lists the account's postings, its subaccounts' too, with the tool's running total; Enter opens the journal at the posting, `p` picks the period, `v` the market values. The amounts are written as the journal writes them. The last 2,000 postings are shown.

## Import from bank statements

In a bank statement (a CSV file), *Import Statement* (`SPC m i`) shows what the tool would add to the journal, as the document *Import: statement.csv*; `i` there imports it, `e` opens the statement's rules, `r` runs it again. The journal is the setting `journal` (beside the statement when relative), else the one `LEDGER_FILE` names, else the journal you last opened.

- **hledger** imports through the statement's rules (`statement.csv.rules` beside it, or the setting `rules`), writes the journal itself and remembers what it took, so the next import of a longer statement adds only the new lines. A statement without rules is offered rules made from its header row (columns, date format, decimal mark, the bank's account), to check and save; `SPC m e` opens them.
- **Ledger** converts a statement whose header uses its names (`date`, `payee`, `amount`), for the account the setting `ledger_account` names and dates in `ledger_date_format`; the plugin appends the result to the journal, one undo step.
- **Beancount**'s statements are imported by your own beangulp script, which the plugin does not run yet.

## Prices and market value

*Fetch Prices* (`SPC m p`) asks your price program for the prices since the journal's last and writes them at the end of the main journal, or of the file the setting `prices` names (beside the main journal when relative; your journal includes it), one undo step.

- **hledger and Ledger**: [pricehist](https://gitlab.com/chrisberkhout/pricehist) (`pip install pricehist`) fetches each pair of the setting `price_sources`, as `SOURCE BASE/QUOTE` (`ecb EUR/USD`; a third word where the journal writes the quote otherwise, `ecb EUR/USD $`), after the last price of the pair the tool lists, or the last 30 days; written as the journal writes prices (`P 2026-10-09 EUR 1.1206 USD`, with its date separator and decimal mark).
- **Beancount**: `bean-price --update` (`pip install beanprice`) fetches what its commodities' `price:` metadata names since their last price.

`v` in the Balance and the Register shows market values: hledger's `-V` (its transactions' costs taken as prices too), Ledger's `-V`, Beancount's `value()`; the header says "market value".

## Reconciliation

*Reconcile* (`SPC m r` on a posting of the account, or pick one of your asset and liability accounts) asks the statement's closing balance (the tool's total of the cleared and pending postings offered) and opens a document with the cleared total, the pending total, their sum, the target and the difference, then the postings not cleared yet. Mark postings in the journal as you tick them off the statement (`SPC m c`, pending), save, and the difference moves; `t` changes the target, Enter opens the journal at a posting. When the difference is zero, *Finish Reconciliation* (`C`, or `SPC m C`) asks the statement's date, clears the pending postings and writes a balance assertion for the target (hledger and Ledger: a "Balance assertion" transaction with a zero posting; Beancount: a `balance` directive the day after), one undo step. The totals are the tool's; the plugin only takes them from the target you typed.

## Checking by the tool

*Check Journal* (`SPC m k` in Vim's command mode, the palette, or a click on the status bar item) runs the journal's own tool on its main journal: `hledger check --strict` (with the checks the setting `checks` adds, such as `ordereddates`), `ledger --pedantic --explicit balance`, or `bean-check`. Its problems open as the document *Check: 2026.journal*: each one's place, the tool's message, and the lines it quoted; Enter opens the file at the problem, `r` checks again, `q` or Escape closes it. hledger stops at its first problem; Ledger and Beancount give them all.

Each save checks again (`check_on_save`, on): the status bar item shows `✓` or the problems' count (`hledger · 2026.journal · 1 problem`), and an open Check document is written anew. A tool not installed is said once, with how to install it.

The servers' diagnostics are their own and may disagree with the tool (hledger-lsp checks with rules of its own): the Check document is the tool's word. Kalem has no way yet for a plugin to put its problems in the gutter beside the server's.

## Aligning amounts

*Align Amounts* (`SPC m a` in Vim's command mode; the palette's Journal group) puts the amounts' last digits of the transaction at the cursor, or of every transaction selected, at one column: the setting `amount_column`, 52 as in ledger-mode, or 0 for as near the accounts as the longest posting allows. Only the blanks between account and amount change, two at the least: the amount as you wrote it (sign, decimal mark, digit groups, commodity and its side), a cost, a lot, a balance assertion and a comment stay as they are, and a posting without an amount is left alone. A value expression (`(-$132.00)`) is aligned by its closing parenthesis, a posting with a balance assertion alone by the assertion's amount. One undo step.

With `align_on_enter` (on), Enter on a posting aligns it first, as in ledger-mode: from the Kalem release after 0.6.15, in the Word-like profile. Vim's insert mode takes Enter itself; there, `SPC m a` from command mode aligns.

Format Document (`SPC c f`) in an hledger or Ledger journal: none (hledger-lsp's is left unused, and `hledger print` rewrites a journal); Align Amounts, with the whole file selected, is the formatter. In a Beancount journal: its server's, else set `[formatters] beancount = "bean-format -"`.

## The servers, measured

On the corpus, through Kalem's client (`kalem lsp ask`, `kalem lsp check`) and asked directly where Kalem cannot ask yet, with hledger 1.52.4 and Beancount 3.2.3. The server's checks are its own: the tool's check (PT5) is the one to trust.

| | hledger-lsp, ptimoney's 0.7.0 | hledger-lsp, juev's 0.2.64 | beancount-language-server 1.9.2 |
|---|---|---|---|
| Install | `npm install -g hledger-lsp` | `go install github.com/juev/hledger-lsp/cmd/hledger-lsp@latest`, or a [release](https://github.com/juev/hledger-lsp/releases) | `brew install beancount-language-server`, or `cargo install beancount-language-server` |
| Completion | accounts (declared ones by default); none at a payee | accounts (with a balance by default); payees, with their last description | accounts, payees, tags, currencies |
| Hover on an account | declared, how many postings; its place wrong, as for definition | its balance: wrong where hledger infers an amount (78.697,25 TRY for 63.697,25) | nothing |
| Definition of an account | the main journal's first line, not its `account` directive | its `account` directive | its `open` directive |
| References | every file | every file | every file |
| Rename an account | the open file only (7 of 10 places): the declaration and the year before keep the old name | every file but a periodic transaction's posting | every file |
| Diagnostics | the failed assertion at its line; warns of an automated posting's virtual account, `(budget:food)`, as undeclared | the failed assertion at its line; an error on an automated posting's multiplier, `*-1`, which hledger reads | `bean-check`'s errors; a `!` transaction flagged |
| Formatting | **changes what the journal means**: drops an automated posting's `*-1` and writes `==` as `=` (undo puts it back) | amounts aligned; hledger reads it as before | amounts aligned; `bean-query` reads it as before |
| Settings sent | `settings.hledgerLanguageServer.…` | `settings.hledger.…` | its main journal, `servers.beancount-language-server.initializationOptions.journal_file`, read as it starts (from the Kalem after 0.6.15) |

Kalem shows completion, hover, definition, references, rename, diagnostics, document symbols and formatting; it does not yet ask for the inlay hints all three give (an elided posting's amount; running balances in juev's), nor for code lenses.

The plugin turns hledger-lsp's formatting off (`servers.hledger-lsp.ignore`, `["documentFormattingProvider"]`), since ptimoney's 0.7.0 formats a journal into one that means something else; with juev's, whose formatting hledger reads as before, set it to `[]`. Kalem leaves a server's capability unused from the release after 0.6.15; Kalem 0.6.15 and earlier format with the server whatever the plugin says, so there, do not use Format Document with ptimoney's 0.7.0 on a journal with automated postings or `==` assertions (undo puts it back).

Without its main journal, beancount-language-server checks an included file alone and reports its accounts as unknown (two false errors on `2026-01.beancount` in a test); told the main journal, the file is clean and completion knows every account of the journal. Set `servers.beancount-language-server.initializationOptions.journal_file` (absolute, from `~/`, or beside the file); the plugin takes it as Beancount's main journal too while `journal` is empty. Kalem hands it to the server from the release after 0.6.15.

## Tests

`cargo test -p kalem-plugin-pta`: the manifest's two halves name what exists and agree; each tool's check, run as the plugin runs it, finds the one problem of each error file at its line and none in the clean journals, and each tool's error output (recorded) is read into its problems; every corpus journal aligned whole, at 52 and at 0, changes only the blanks before its amounts, is unchanged aligned again, and reads as before to its tool (`tests/align.rs`); its servers say their versions and how to install them in a line the status bar holds, and their settings are in sections the servers read; the main journal and the tool are worked out as said above (unit tests); the corpus through each server installed (`tests/server.rs`, with `PTA_HLEDGER_LSP` and `PTA_BEANCOUNT_LSP` naming programs, `:` between them): completion, the deliberate errors, and formatting the tool reads as before, ptimoney's 0.7.0 checked to still do the harm above; the syntaxes load as Kalem loads them, take the languages' files and source blocks, parse every corpus file to its end, color a transaction of each dialect and the rules file as the tests spell out, and color as tags and accounts what `hledger tags` and `hledger accounts` read; and the corpus, one journal per dialect (`corpus/hledger`, three files with `include`, and a bank statement with its CSV rules, which `hledger print` reads; `corpus/ledger`; `corpus/beancount`, two files), each passes its tool's strictest check (`hledger check --strict --auto`, `ledger --pedantic balance`, `bean-check`), and each `errors.*` file fails it at its line. A tool not installed is skipped with a note.
