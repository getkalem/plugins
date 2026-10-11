# Plain text accounting (`pta`)

hledger, Ledger and Beancount journals in [Kalem](https://github.com/getkalem/kalem): written with completion and alignment, checked by the tool you already have on each save, reported as documents (balance, register, market value), reconciled against a statement, imported from bank statements, and kept in prices. **The tool does the accounting**: hledger, Ledger and Beancount parse, balance, value and check; the plugin asks them and shows what they say, and has no engine of its own. The work list is [`pta_todo.md`](pta_todo.md).

**Status: 0.1.0**, for Kalem 0.6.16 and later (plugin API 0.2.13). One plugin, one install, with two halves in one manifest, both loaded:

- **The languages and their servers**, which Kalem's core runs: hledger (`.journal`, `.hledger`, `.j`, `.hledger.journal`) with [hledger-lsp](https://github.com/ptimoney/hledger-lsp), Ledger (`.ledger`, `.ldg`), Beancount (`.beancount`, `.bean`) with [beancount-language-server](https://github.com/polarmutex/beancount-language-server), and hledger's CSV rules (`.rules`), each highlighted by the plugin's own syntaxes: one base for what the journals share (dates, statuses, descriptions, accounts, amounts and commodities, costs, lots, balance assertions, comments and their tags, periodic and automated transactions, directives), with hledger, Ledger and Beancount over it. Journals in Org's source blocks and Markdown's fenced blocks (`hledger`, `ledger`, `beancount`) are colored the same. Completion of accounts, payees and tags, hover, rename and the servers' diagnostics come from the servers.
- **The component**, which runs the journal's tool: the commands below, a status bar item naming the tool and the main journal of the journal you are in (`hledger · 2026.journal · ✓`), the Journal menu and button.

`.dat` files are not claimed.

## Install

```
kalem plugin install pta
```

Kalem also offers it when you open a journal. Then the tool you use, and, if you like, a server and a price program:

| | Tool | Server (completion, hover, rename) | Prices |
|---|---|---|---|
| hledger | `brew install hledger`, or a [release](https://hledger.org/install) | `npm install -g hledger-lsp`, or juev's: `go install github.com/juev/hledger-lsp/cmd/hledger-lsp@latest` (see below) | `pip install pricehist` |
| Ledger | `brew install ledger`, or your system's package | none exists | `pip install pricehist` |
| Beancount | `pip install beancount beanquery` (`bean-check`; `bean-query` for the reports) | `brew install beancount-language-server`, or `cargo install beancount-language-server` | `pip install beanprice` |

- **The journal's folder among your projects** (`projects.toml`, or Projects: Add Project): Kalem runs a plugin's programs in a project's folder only.
- A program not on the PATH Kalem sees (a virtual environment's `bean-check`, Kalem started from the Dock) is named by the setting `programs.NAME`.
- The component starts with the first document you open, so that a bank statement (a CSV file, which the plugin does not claim) can be imported before any journal is open.
- `kalem lsp status FILE` says which server runs for a journal.

## What it does, per dialect

| | hledger | Ledger | Beancount |
|---|---|---|---|
| Highlighting (the plugin) | journal and CSV rules | journal | journal |
| Completion, hover, definition, references, rename (the server) | hledger-lsp | none | beancount-language-server |
| Check Journal, on each save (the tool) | `hledger check --strict` and the setting `checks` | `ledger --pedantic --explicit balance` | `bean-check` |
| Align Amounts, Format Document (the plugin) | aligns; the server's formatting turned off | aligns | aligns; Format Document is the server's or `bean-format` |
| New Transaction, drafted from the payee's last (the tool) | `hledger print --match` | `ledger xact` | the payee's last entry |
| Toggle Status, Copy, Sort, Next, Previous, Date ±1 (the plugin) | yes | yes | yes |
| Balance and Register (the tool) | `hledger balance`, `register -O json` | `ledger balance`, `register` | `bean-query` |
| Market value (the tool) | `-V --infer-market-prices` | `-V` | `value()` |
| Other reports (the tool) | cash flow, income statement, budget, forecast, activity, statistics, lists, `print` | income statement, budget, forecast, cleared, statistics, lists, `print` | income statement, holdings, monthly expenses, lists, `PRINT`, the journal's saved queries |
| Reconcile, Finish (the tool's totals, the plugin's edits) | a `Balance assertion` transaction | a `Balance assertion` transaction | a `balance` directive |
| Import a bank statement (the tool) | `hledger import` through its rules, rules offered from the statement's header | `ledger convert` | not yet (your beangulp script) |
| Fetch Prices (your price program) | `pricehist` | `pricehist` | `bean-price --update` |

## Keys

In a journal, under `SPC m` in Vim's command mode, where Doom Emacs's `:lang ledger` has its keys (`a` add, `t` toggle, `d` delete, `r` report, `R` reconcile, `s` sort, `[[` and `]]`) and the plugin's others take letters Doom leaves free; in the Word-like profile, from the **Journal** menu (the menu bar, F10 in the terminal), the **Journal** button on the toolbar (the foot of the terminal's sidebar) and the palette's Journal group. The button, `SPC m m` and `?` in the plugin's documents offer the commands as a list, each with its keys.

| Keys | Command |
|---|---|
| `SPC m a` | New Transaction |
| `SPC m y` | Copy Transaction |
| `SPC m t` | Toggle Status |
| `SPC m d` | Delete Transaction |
| `SPC m =` | Align Amounts |
| `SPC m s` | Sort Transactions |
| `[[`, `]]` (or `Alt+Up`, `Alt+Down`) | Previous, Next Transaction |
| `Ctrl+Shift+H`, `Ctrl+Shift+L` | Date a Day Earlier, Later |
| `SPC m k` | Check Journal |
| `SPC m b` (or `SPC m g b`) | Balance |
| `SPC m g r` | Register |
| `SPC m r` | Report…: the Balance, the Register and the tool's other reports, to pick |
| `SPC m R` | Reconcile |
| `SPC m C` | Finish Reconciliation |
| `SPC m p` | Fetch Prices |
| `SPC m m` | The Journal's commands, to pick |
| `SPC m i`, `SPC m e` | In a CSV statement: Import Statement, Edit the Statement's Rules |
| Enter | On a posting, in the Word-like profile: aligns it, then a new line |

In the plugin's documents (both profiles; in Vim's command mode): **Check**: Enter the problem's place, `r` again, `q` or Escape close. **Balance**: Tab, Left and Right fold, Enter the account's register, `p` period, `d` depth, `v` market value, `r` again, `q` close. **Register**: Enter the posting, `p` period, `v` market value, `r` again, `q` close. **Reconcile**: Enter the posting, `t` the statement's balance, `C` finish, `r` again, `q` close. **Import**: `i` import, `e` the rules, `r` again, `q` close. **Report**: `p` period, `o` another report, `r` again, `q` close. `?` lists them.

## Settings

In `settings.toml`, under `[plugins."org.kalem.pta"]`:

| Setting | Default | What it does |
|---|---|---|
| `journal` | empty | The main journal, the one that includes the others (absolute, from `~/`, or beside the file); empty for `LEDGER_FILE`'s when it includes the file, else the file itself |
| `tool` | `auto` | The program for hledger's and Ledger's journals: `auto` (the file's own; a `.ledger` file is hledger's where only hledger is installed), `hledger` or `ledger` |
| `amount_column` | 52 | Where Align Amounts puts the amounts' last digits, as ledger-mode; 0 for as near the accounts as the longest posting allows |
| `align_on_enter` | on | Enter on a posting aligns it first |
| `check_on_save` | on | Check the main journal on each save |
| `checks` | `[]` | hledger's checks added to `--strict`: `ordereddates`, `payees`, `recentassertions`, `tags`, `uniqueleafnames` |
| `insert_at` | `date` | Where New and Copy Transaction write: at the date's place, or at the `end` |
| `toggle_postings` | off | Toggle Status on a posting line marks the posting, not its transaction |
| `rules` | empty | hledger's rules for a statement, when not its own beside it (`statement.csv.rules`) |
| `ledger_account`, `ledger_date_format` | empty | Ledger's import: the bank's account, and how the statement writes dates (`%d.%m.%Y`) |
| `reports` | `[]` | Reports of your own, first in Report…: `NAME: ARGUMENTS` (`Food by month: balance expenses:food -M`; for Beancount, a query) |
| `prices` | empty | Where Fetch Prices writes: a file your main journal includes (beside it when relative); empty for the main journal |
| `price_sources` | `[]` | hledger and Ledger: the pairs to fetch, `SOURCE BASE/QUOTE` (`ecb EUR/USD`), a third word for the journal's symbol (`ecb EUR/USD $`) |
| `programs.NAME` | empty | Where `hledger`, `ledger`, `bean-check`, `bean-query`, `pricehist` or `bean-price` is, when not on the PATH |
| `servers.beancount-language-server.initializationOptions.journal_file` | empty | Beancount's main journal, for its server (below) and the plugin |
| `servers.hledger-lsp.ignore` | `["documentFormattingProvider"]` | What of hledger-lsp Kalem leaves unused (below) |
| `settings.hledgerLanguageServer.…`, `settings.hledger.…` | the servers' | ptimoney's and juev's hledger-lsp settings, sent to the server |

## Not yet

Waiting on Kalem:

- **Marking from the reconciliation.** ledger-mode's reconcile buffer marks a posting where you press Space; a Kalem plugin can edit only the document a command runs in (K3), so here postings are marked in the journal (`SPC m t`) and the reconciliation is written again on save. Finish opens the journal and writes there.
- **The statement in a pane beside.** The Reconcile and Import documents open as documents of their own, not in a pane beside the journal.
- The tool's problems in the gutter beside the server's (K2); the servers' running balances and elided amounts as inlay hints (K4); `hledger` and `beancount` source blocks run by Org's Babel (K6); Enter aligning in Vim's insert mode.

Not done yet:

- **Reports as trees**: Report… shows the tool's text as it prints it; only the Balance folds. hledger's `roi`, which asks for its accounts, is not offered (pin it in `reports`).
- **The statement beside the reconciliation** (PT8c): a statement's rows paired with the account's postings by date and amount.
- **Beancount's import** (PT9b): your beangulp script, which needs a permission to run `python3`.
- A report inserted into an Org document as a table (PT11b), the commands from `kalem run` (PT11c), and journals of 200,000 transactions measured (PT13).

## Beside ledger-mode and Fava

- **ledger-mode** (Emacs, and Doom Emacs's `:lang ledger`, whose `SPC m` keys the plugin takes): the same keys' work (`C-c C-a` New Transaction through `ledger xact`, `C-c C-c` Toggle Status, alignment at column 52 and on Enter, `C-c C-r` Reconcile with its cleared, pending and target totals, `ledger-report` as Report…, its own reports the setting `reports`), for hledger and Beancount too. Not here: marking from the reconcile buffer (above), `ledger-schedule` (Doom's `SPC m S`), and Calc on an amount (`SPC m e`); Doom's `SPC m g s` (statistics) is a report of Report…, and its `SPC m g b`, the balance at point, opens the Balance. Here and not there: the tool's check on each save as a document, import from statements, prices fetched, the servers' completion and rename.
- **Fava** (Beancount's web interface): the same reports from the same tool, in the editor where the journal is written, offline, for hledger and Ledger too. Not here: charts (balances over time, treemaps), the query shell, documents attached to entries, and importers run from the browser.

## The main journal and the tool

A journal that `include`s others is checked and reported as one: checking an included file alone gives false errors (an account opened in another file is "unknown"). The plugin's setting `journal` names the main journal (absolute, from `~/`, or beside the file). Unset, an hledger journal's main journal is the one `LEDGER_FILE` names when that journal includes the file (`hledger files` says so), a Beancount journal's the one its server is told (below), else the file itself. Kalem started from the Dock or a launcher may not see the `LEDGER_FILE` of your shell: set `journal` then.

The setting `tool` picks the program for hledger's and Ledger's journals: `auto` takes the file's own, and a `.ledger` file goes to Ledger where Ledger is installed and to hledger where only hledger is (hledger does not read Ledger's value expressions, such as `(-$132.00)`). A Beancount journal is always Beancount's.

## Writing transactions

In a journal, under `SPC m` in Vim's command mode and in the palette's Journal group, each one undo step:

- *New Transaction* (`SPC m a`): asks the date (today offered; `yesterday`, `-3`, `5`, `10/5`, `friday`, `last friday` taken) and the payee (the journal's, the last written first, or another), then writes the transaction the tool drafts from that payee's last one (`hledger print --match`, `ledger xact`, Beancount's last entry), dated, without its old balance assertions, at its date's place (or at the end, `insert_at`), the cursor on its first amount.
- *Toggle Status* (`SPC m t`): unmarked, pending `!`, cleared `*`, unmarked (Beancount's `txn`, `!`, `*`); the transactions selected all to the first one's next; the posting at the cursor with `toggle_postings`.
- *Copy Transaction* (`SPC m y`): the transaction again, dated today, at its date's place.
- *Delete Transaction* (`SPC m d`): the transaction at the cursor, with its comment lines above it.
- *Next* and *Previous Transaction* (`]]`, `[[`; Alt+Down, Alt+Up).
- *Date a Day Later* and *Earlier* (`ctrl+shift+l`, `ctrl+shift+h` in Vim's command mode).
- *Sort Transactions* (`SPC m s`): those selected, or the file's, by date, each with the comment lines above it; refused where a directive between them (`apply account`, `alias`, `include`, `pushtag`…) would apply to others after sorting.

The outline is the server's document symbols.

## The balance and the register

*Balance* (`SPC m b`) opens the main journal's balance as a document: assets, liabilities and equity at the period's end, then income, expenses and the other accounts within the period, as a tree (each parent with its subaccounts' amounts), every amount and total as the tool computes and writes it. Tab, Right and Left fold and unfold an account (kept as you leave it), `p` picks the period (this month, last month, this quarter, this year, last year, all dates), `d` the depth, `v` shows market values (or back to cost), `r` runs it again, Enter opens the account's register, `q` or Escape closes it. It is written again when the journal is saved.

*Register* (`SPC m g r` on a posting's account, or Enter in the balance; else an account to pick) lists the account's postings, its subaccounts' too, with the tool's running total; Enter opens the journal at the posting, `p` picks the period, `v` the market values. The amounts are written as the journal writes them. The last 2,000 postings are shown.

## Import from bank statements

In a bank statement (a CSV file), *Import Statement* (`SPC m i`) shows what the tool would add to the journal, as the document *Import: statement.csv*; `i` there imports it, `e` opens the statement's rules, `r` runs it again. The journal is the setting `journal` (beside the statement when relative), else the one `LEDGER_FILE` names, else the journal you last opened.

- **hledger** imports through the statement's rules (`statement.csv.rules` beside it, or the setting `rules`), writes the journal itself and remembers what it took, so the next import of a longer statement adds only the new lines. A statement without rules is offered rules made from its header row (columns, date format, decimal mark, the bank's account), to check and save; `SPC m e` opens them.
- **Ledger** converts a statement whose header uses its names (`date`, `payee`, `amount`), for the account the setting `ledger_account` names and dates in `ledger_date_format`; the plugin appends the result to the journal, one undo step.
- **Beancount**'s statements are imported by your own beangulp script, which the plugin does not run yet.

## Other reports

*Report…* (`SPC m r`, Doom's ledger-report, or `o` in a Report) lists the Balance, the Register and the tool's other reports and opens the one picked as a document, as the tool prints it: hledger's cash flow, income statement, budget and forecast (monthly, from its periodic transactions), activity, statistics, its accounts, commodities, payees, tags and prices, and `print`; Ledger's income statement, budget, forecast (six months on), cleared report, statistics, lists and `print`; Beancount's through `bean-query`: income statement, holdings at cost and value, monthly expenses, balances, lists and `PRINT`, after the journal's own saved queries (its `query` directives). `print` comes in the journal's syntax and colors. `p` picks the period of a report that takes one (hledger's and Ledger's), `r` runs it again, and it is run again when the journal is saved.

The setting `reports` puts reports of your own first: `"Food by month: balance expenses:food -M"`, the tool's arguments after `-f JOURNAL` (quoted as a shell quotes), or, for Beancount, a query.

## Prices and market value

*Fetch Prices* (`SPC m p`) asks your price program for the prices since the journal's last and writes them at the end of the main journal, or of the file the setting `prices` names (beside the main journal when relative; your journal includes it), one undo step.

- **hledger and Ledger**: [pricehist](https://gitlab.com/chrisberkhout/pricehist) (`pip install pricehist`) fetches each pair of the setting `price_sources`, as `SOURCE BASE/QUOTE` (`ecb EUR/USD`; a third word where the journal writes the quote otherwise, `ecb EUR/USD $`), after the last price of the pair the tool lists, or the last 30 days; written as the journal writes prices (`P 2026-10-09 EUR 1.1206 USD`, with its date separator and decimal mark).
- **Beancount**: `bean-price --update` (`pip install beanprice`) fetches what its commodities' `price:` metadata names since their last price.

`v` in the Balance and the Register shows market values: hledger's `-V` (its transactions' costs taken as prices too), Ledger's `-V`, Beancount's `value()`; the header says "market value".

## Reconciliation

*Reconcile* (`SPC m R` on a posting of the account, or pick one of your asset and liability accounts) asks the statement's closing balance (the tool's total of the cleared and pending postings offered) and opens a document with the cleared total, the pending total, their sum, the target and the difference, then the postings not cleared yet. Mark postings in the journal as you tick them off the statement (`SPC m t`, pending), save, and the difference moves; `t` changes the target, Enter opens the journal at a posting. When the difference is zero, *Finish Reconciliation* (`C`, or `SPC m C`) asks the statement's date, clears the pending postings and writes a balance assertion for the target (hledger and Ledger: a "Balance assertion" transaction with a zero posting; Beancount: a `balance` directive the day after), one undo step. The totals are the tool's; the plugin only takes them from the target you typed.

## Checking by the tool

*Check Journal* (`SPC m k` in Vim's command mode, the palette, or a click on the status bar item) runs the journal's own tool on its main journal: `hledger check --strict` (with the checks the setting `checks` adds, such as `ordereddates`), `ledger --pedantic --explicit balance`, or `bean-check`. Its problems open as the document *Check: 2026.journal*: each one's place, the tool's message, and the lines it quoted; Enter opens the file at the problem, `r` checks again, `q` or Escape closes it. hledger stops at its first problem; Ledger and Beancount give them all.

Each save checks again (`check_on_save`, on): the status bar item shows `✓` or the problems' count (`hledger · 2026.journal · 1 problem`), and an open Check document is written anew. A tool not installed is said once, with how to install it.

The servers' diagnostics are their own and may disagree with the tool (hledger-lsp checks with rules of its own): the Check document is the tool's word. Kalem has no way yet for a plugin to put its problems in the gutter beside the server's.

## Aligning amounts

*Align Amounts* (`SPC m =` in Vim's command mode; the palette's Journal group) puts the amounts' last digits of the transaction at the cursor, or of every transaction selected, at one column: the setting `amount_column`, 52 as in ledger-mode, or 0 for as near the accounts as the longest posting allows. Only the blanks between account and amount change, two at the least: the amount as you wrote it (sign, decimal mark, digit groups, commodity and its side), a cost, a lot, a balance assertion and a comment stay as they are, and a posting without an amount is left alone. A value expression (`(-$132.00)`) is aligned by its closing parenthesis, a posting with a balance assertion alone by the assertion's amount. One undo step.

With `align_on_enter` (on), Enter on a posting aligns it first, as in ledger-mode, in the Word-like profile. Vim's insert mode takes Enter itself; there, `SPC m =` from command mode aligns.

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
| Settings sent | `settings.hledgerLanguageServer.…` | `settings.hledger.…` | its main journal, `servers.beancount-language-server.initializationOptions.journal_file`, read as it starts |

Kalem shows completion, hover, definition, references, rename, diagnostics, document symbols and formatting; it does not yet ask for the inlay hints all three give (an elided posting's amount; running balances in juev's), nor for code lenses.

The plugin turns hledger-lsp's formatting off (`servers.hledger-lsp.ignore`, `["documentFormattingProvider"]`), since ptimoney's 0.7.0 formats a journal into one that means something else; with juev's, whose formatting hledger reads as before, set it to `[]`.

Without its main journal, beancount-language-server checks an included file alone and reports its accounts as unknown (two false errors on `2026-01.beancount` in a test); told the main journal, the file is clean and completion knows every account of the journal. Set `servers.beancount-language-server.initializationOptions.journal_file` (absolute, from `~/`, or beside the file); the plugin takes it as Beancount's main journal too while `journal` is empty.

## Tests

`cargo test -p kalem-plugin-pta`: the manifest's two halves name what exists and agree; each tool's check, run as the plugin runs it, finds the one problem of each error file at its line and none in the clean journals, and each tool's error output (recorded) is read into its problems; every corpus journal aligned whole, at 52 and at 0, changes only the blanks before its amounts, is unchanged aligned again, and reads as before to its tool (`tests/align.rs`); its servers say their versions and how to install them in a line the status bar holds, and their settings are in sections the servers read; the main journal and the tool are worked out as said above (unit tests); the corpus through each server installed (`tests/server.rs`, with `PTA_HLEDGER_LSP` and `PTA_BEANCOUNT_LSP` naming programs, `:` between them): completion, the deliberate errors, and formatting the tool reads as before, ptimoney's 0.7.0 checked to still do the harm above; the syntaxes load as Kalem loads them, take the languages' files and source blocks, parse every corpus file to its end, color a transaction of each dialect and the rules file as the tests spell out, and color as tags and accounts what `hledger tags` and `hledger accounts` read; and the corpus, one journal per dialect (`corpus/hledger`, three files with `include`, and a bank statement with its CSV rules, which `hledger print` reads; `corpus/ledger`; `corpus/beancount`, two files), each passes its tool's strictest check (`hledger check --strict --auto`, `ledger --pedantic balance`, `bean-check`), and each `errors.*` file fails it at its line. A tool not installed is skipped with a note.
