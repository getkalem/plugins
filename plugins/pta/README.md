# Plain text accounting (`pta`)

hledger, Ledger and Beancount journals in [Kalem](https://github.com/getkalem/kalem): written with completion and alignment, checked by the tool you already have on each save, reported as documents (balance, register, market value), reconciled against a statement, imported from bank statements, and kept in prices. **The tool does the accounting**: hledger, Ledger and Beancount parse, balance, value and check; the plugin asks them and shows what they say, and has no engine of its own. The work list is [`pta_todo.md`](pta_todo.md).

**Status: 0.9.1**, for Kalem 0.6.30 and later (plugin API 0.2.18). 0.9.1 formats a large journal in linear time, so Format Document and format on save no longer have Kalem 0.6.48 and later stop the plugin. 0.9.0 runs hledger, Ledger and Beancount source blocks through Kalem's Babel, `C-c C-c` and export included, under its rules (with Kalem 0.6.39 and later; earlier, Run the Journal Block runs them). 0.8.0 brings the journal's outline in the outline sidebar (its Org headings, else its months), and runs hledger in a UTF-8 locale when the session's is not one (a Linux server or container without LANG). 0.7.2, for Kalem 0.6.27 and later (plugin API 0.2.16), has everything but what 0.8.0 brings; it writes a journal with Windows's line endings (CR LF) in them throughout, and reads Windows paths with `\` and any case. 0.7.1 has Reconcile and Finish take the account the tool reads at a posting, one written with an alias (Ledger's `chk`) or under `apply account` too. 0.7.0 brought Format Document and format on save aligning a whole journal at `amount_column`. 0.6.0, for Kalem 0.6.22 and later (plugin API 0.2.14), has everything but what 0.7.0 brings; it brought completion of accounts, payees and commodities from the tool's lists of the main journal. 0.5.0 has everything but what 0.6.0 brings; it brought typed periods (a year, a quarter, a month, a day or a range of them, in Dates… and in `kalem run`'s `period`), Reconcile for an account of several commodities (a block and an assertion each), the Reconcile document following an hledger or Ledger journal as it is typed (`reconcile_live`), and hledger's Return on Investment in Report…. 0.4.0 has everything but what 0.5.0 brings; it brought hledger's, Ledger's and bean-check's problems drawn in the journal as a language server's diagnostics are, the servers' inlay hints (an elided posting's amount; running balances from ptimoney's hledger-lsp once its setting is on), and Enter on a line of the Balance without an account saying so. 0.3.0, for Kalem 0.6.21 and later (plugin API 0.2.13), has everything but what 0.4.0 brings; it brought Beancount's import through your beangulp script, Tab in the Reconcile document marking a posting in the journal, the Reconcile document in a pane beside the journal, Run the Journal Block in Org, and Enter aligning a posting in Vim's insert mode too; on Kalem 0.6.18 to 0.6.20 all of it works but that Enter (insert mode keeps its own there; `SPC m =` aligns). 0.2.0, for Kalem 0.6.18 and later, has everything but what 0.3.0 and 0.4.0 bring; 0.1.0, for Kalem 0.6.16 and 0.6.17, has everything but Report… (`SPC m r`; in 0.1.0 it opens the Balance) and the setting `reports`, Insert a Report as a Table, the `kalem run` commands and the setting `glyphs`, the statement paired with the reconciliation (`match_days`), the Register narrowed (`/`) and its running total kept within a period, Main Journal and Tool…, and large journals' outputs refused rather than cut.

One plugin, one install, with two halves in one manifest, both loaded:

- **The languages and their servers**, which Kalem's core runs: hledger (`.journal`, `.hledger`, `.j`, `.hledger.journal`) with [hledger-lsp](https://github.com/ptimoney/hledger-lsp), Ledger (`.ledger`, `.ldg`), Beancount (`.beancount`, `.bean`) with [beancount-language-server](https://github.com/polarmutex/beancount-language-server), and hledger's CSV rules (`.rules`), each highlighted by the plugin's own syntaxes: one base for what the journals share (dates, statuses, descriptions, accounts, amounts and commodities, costs, lots, balance assertions, comments and their tags, periodic and automated transactions, directives), with hledger, Ledger and Beancount over it. Journals in Org's source blocks and Markdown's fenced blocks (`hledger`, `ledger`, `beancount`) are colored the same. Completion, hover, rename and diagnostics come from the servers. Each server completes different things (accounts, payees, tags); see [The servers, measured](#the-servers-measured).
- **The component**, which runs the journal's tool: the commands below, a status bar item naming the tool and the main journal of the journal you are in (`hledger · 2026.journal · ✓`), the Journal menu and button.

`.dat` files are not claimed.

## Install

From Kalem: the Kalem menu, *Browse Plugins*, then *Plain text accounting*; Kalem also offers it when you open a journal. On the command line:

```
kalem plugin install pta
```

pta 0.8.0 needs Kalem 0.6.30 or later (plugin API 0.2.18); 0.7.0 to 0.7.2 need Kalem 0.6.27 or later (plugin API 0.2.16); 0.4.0 to 0.6.0 need Kalem 0.6.22 or later (plugin API 0.2.14). Kalem 0.6.21 and earlier install it but do not start it, and say "the plugin asks for what it was not granted: kalem:plugin/problems@0.2.14": update Kalem.

Then the tool you use, and, if you like, a server and a price program:

| | Tool | Server (completion, hover, rename) | Prices |
|---|---|---|---|
| hledger | `brew install hledger`, or a [release](https://hledger.org/install) | `npm install -g hledger-lsp`, or juev's: `go install github.com/juev/hledger-lsp/cmd/hledger-lsp@latest` (see below) | `pip install pricehist` |
| Ledger | `brew install ledger`, or your system's package | none exists | `pip install pricehist` |
| Beancount | `pip install beancount beanquery` (`bean-check` for Check Journal; `bean-query` for everything else: the Balance, the Register, Reconcile, New Transaction and the reports) | `brew install beancount-language-server`, or `cargo install beancount-language-server` | `pip install beanprice`; `pip install beangulp` for import |

- **The journal's folder among your projects** (`projects.toml`, or Project: Add Project): Kalem runs a plugin's programs in a project's folder only.
- A program not on the PATH Kalem sees (a virtual environment's `bean-check`, Kalem started from the Dock) is named by the setting `programs.NAME`.
- The component starts with the first document you open, so that a bank statement (a CSV file, which the plugin does not claim) can be imported before any journal is open.
- `kalem lsp status FILE` says which server runs for a journal.

## What it does, per dialect

| | hledger | Ledger | Beancount |
|---|---|---|---|
| Highlighting (the plugin) | journal and CSV rules | journal | journal |
| Completion of accounts, payees and commodities (the plugin, from the tool's lists) | `hledger accounts`, `payees`, `commodities` | `ledger accounts`, `payees`, `commodities` | `bean-query` |
| Outline: Org headings, else years and months (the plugin) | yes | yes | yes |
| Hover, definition, references, rename, and the server's own completion (tags, dates) (the server) | hledger-lsp | none | beancount-language-server |
| Check Journal, on each save (the tool) | `hledger check --strict` and the setting `checks` | `ledger --pedantic --explicit balance` | `bean-check` |
| Align Amounts, Format Document (the plugin) | aligns; the server's formatting turned off | aligns | aligns; Format Document is the server's when it runs |
| New Transaction, drafted from the payee's last (the tool) | `hledger print --match` | `ledger xact` | the payee's last entry |
| Toggle Status, Copy, Sort, Next, Previous, Date ±1 (the plugin) | yes | yes | yes |
| Balance and Register (the tool) | `hledger balance`, `register -O json` | `ledger balance`, `register` | `bean-query` |
| Market value (the tool) | `-V --infer-market-prices` | `-V` | `value()` |
| Other reports (the tool) | cash flow, income statement, budget, forecast, activity, statistics, lists, `print` | income statement, budget, forecast, cleared, statistics, lists, `print` | income statement, holdings, monthly expenses, lists, `PRINT`, the journal's saved queries |
| Reconcile, Finish (the tool's totals, the plugin's edits) | a `Balance assertion` transaction | a `Balance assertion` transaction | a `balance` directive |
| Import a bank statement (the tool) | `hledger import` through its rules, rules offered from the statement's header | `ledger convert` | your beangulp script (`import.py extract`) |
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
| `SPC m x` | In an Org document's journal block: Run the Journal Block |
| `SPC m s` | Sort Transactions |
| `[[`, `]]` (Vim's command mode), `Alt+Up`, `Alt+Down` (both profiles) | Previous, Next Transaction |
| `Ctrl+Shift+H`, `Ctrl+Shift+L` (Vim's command mode) | Date a Day Earlier, Later |
| `SPC m k` | Check Journal |
| `SPC m b` (or `SPC m g b`) | Balance |
| `SPC m g r` | Register |
| `SPC m r` | Report…: the Balance, the Register and the tool's other reports, to pick |
| `SPC m R` | Reconcile |
| `SPC m C` | Finish Reconciliation |
| `SPC m p` | Fetch Prices |
| `SPC m m` | The Journal's commands, to pick |
| | Tool Version: what the journal's tool says it is (`--version`), from the menu or the palette |
| `SPC m i`, `SPC m e`, `SPC m r` | In a CSV statement: Import Statement, Edit the Statement's Rules, Pair with the Reconciliation (Vim's command mode; in the Word-like profile, from the palette) |
| Enter | On a posting, in the Word-like profile: aligns it, then a new line |

In the plugin's documents (both profiles; in Vim's command mode): **Check**: Enter the problem's place, `r` again, `q` or Escape close. **Balance**: Tab, Left and Right fold, Enter the account's register, `p` period, `d` depth, `v` market value, `r` again, `q` close. **Register**: Enter the posting, `p` period, `v` market value, `r` again, `q` close. **Reconcile**: Tab marks the posting pending or not, Enter the posting, `t` the statement's balance, `C` finish, `r` again, `q` close. **Import**: `i` import, `e` the rules, `r` again, `q` close. **Report**: `p` period, `o` another report, `r` again, `q` close. Escape closes each of them too. `?` lists them.

## Settings

In `settings.toml`, under `[plugins."org.kalem.pta"]`:

| Setting | Default | What it does |
|---|---|---|
| `journal` | empty | The main journal, the one that includes the others (absolute, from `~/`, or beside the file); empty for `LEDGER_FILE`'s when it includes the file (hledger's journals only; Ledger's use the file itself), else the file itself |
| `tool` | `auto` | The program for hledger's and Ledger's journals: `auto` (the file's own; a `.ledger` file is hledger's where only hledger is installed), `hledger` or `ledger` |
| `amount_column` | 52 | Where Align Amounts puts the amounts' last digits, as ledger-mode; 0 for as near the accounts as the longest posting allows |
| `align_on_enter` | on | Enter on a posting aligns it first |
| `check_on_save` | on | Check the main journal on each save |
| `checks` | `[]` | hledger's checks added to `--strict`: `ordereddates`, `payees`, `recentassertions`, `tags`, `uniqueleafnames` |
| `insert_at` | `date` | Where New and Copy Transaction write: at the date's place, or at the `end` |
| `toggle_postings` | off | Toggle Status on a posting line marks the posting, not its transaction |
| `rules` | empty | hledger's rules for a statement, when not its own beside it (`statement.csv.rules`) |
| `ledger_account`, `ledger_date_format` | empty | Ledger's import: the bank's account, and how the statement writes dates (`%d.%m.%Y`) |
| `glyphs` | `unicode` | The Balance's fold marks and a passed check's mark: `▾ ▸ ✓`, or `v > ok` (`ascii`) |
| `reconcile_beside` | on | Reconcile opens its document in a pane beside the journal |
| `reconcile_live` | on | An open Reconcile document follows the journal as you type, before you save (hledger and Ledger) |
| `match_days` | 3 | How many days apart a statement's line and its posting may be, paired |
| `reports` | `[]` | Reports of your own, first in Report…: `NAME: ARGUMENTS` (`Food by month: balance expenses:food -M`; for Beancount, a query) |
| `importer`, `programs.python3` | empty | Beancount's import: your beangulp script (beside the main journal when relative; `import.py` there when empty), and the Python that has beangulp |
| `prices` | empty | Where Fetch Prices writes: a file your main journal includes (beside it when relative); empty for the main journal |
| `price_sources` | `[]` | hledger and Ledger: the pairs to fetch, `SOURCE BASE/QUOTE` (`ecb EUR/USD`), a third word for the journal's symbol (`ecb EUR/USD $`) |
| `programs.NAME` | empty | Where `hledger`, `ledger`, `bean-check`, `bean-query`, `pricehist` or `bean-price` is, when not on the PATH |
| `servers.beancount-language-server.initializationOptions.journal_file` | empty | Beancount's main journal, for its server (below) and the plugin |
| `servers.hledger-lsp.ignore` | `["documentFormattingProvider"]` | What of hledger-lsp Kalem leaves unused (below) |
| `settings.hledgerLanguageServer.…`, `settings.hledger.…` | the servers' | ptimoney's and juev's hledger-lsp settings, sent to the server |

## Not yet


- **Reports as trees**: Report… shows the tool's text as it prints it; only the Balance folds.

## Beside ledger-mode and Fava

- **ledger-mode** (Emacs, and Doom Emacs's `:lang ledger`, whose `SPC m` keys the plugin takes): the same keys' work (`C-c C-a` New Transaction through `ledger xact`, `C-c C-c` Toggle Status, alignment at column 52 and on Enter, `C-c C-r` Reconcile with its cleared, pending and target totals, `ledger-report` as Report…, its own reports the setting `reports`), for hledger and Beancount too. Tab in the Reconcile document marks a posting as ledger-mode's reconcile buffer does (Doom's key), saving the journal. Not here: `ledger-schedule` (Doom's `SPC m S`), and Calc on an amount (`SPC m e`); Doom's `SPC m g s` (statistics) is a report of Report…, and its `SPC m g b`, the balance at point, opens the Balance. Here and not there: the tool's check on each save as a document, import from statements, prices fetched, the servers' completion and rename.
- **Fava** (Beancount's web interface): the same reports from the same tool, in the editor where the journal is written, offline, for hledger and Ledger too. Not here: charts (balances over time, treemaps), the query shell, documents attached to entries, and importers run from the browser.

## The main journal and the tool

A journal that `include`s others is checked and reported as one: checking an included file alone gives false errors (an account opened in another file is "unknown"). The plugin's setting `journal` names the main journal (absolute, from `~/`, or beside the file). Unset, an hledger journal's main journal is the one `LEDGER_FILE` names when that journal includes the file (`hledger files` says so), a Beancount journal's the one its server is told (below), else the file itself. Kalem started from the Dock or a launcher may not see the `LEDGER_FILE` of your shell: set `journal` then.

*Main Journal and Tool…* (the Journal menu, the palette) sets both from a pick: this file as the main journal, another typed, or none. The setting `tool` picks the program for hledger's and Ledger's journals: `auto` takes the file's own, and a `.ledger` file goes to Ledger where Ledger is installed and to hledger where only hledger is (hledger does not read Ledger's value expressions, such as `(-$132.00)`). A Beancount journal is always Beancount's.

## Writing transactions
As you type, the plugin completes the account on a posting (and in Beancount's `open`, `close`, `balance`, `pad`, `note` and `document`), the payee on a transaction's line, and the commodity in an amount, from what the journal's tool lists of the main journal: its accounts, payees and commodities, read when a journal opens and again on each save (before that, those the text writes), a balance assertion that fails as you type not stopping the tool (hledger's `-I`, Ledger's `--permissive`, as for New Transaction's draft and Reconcile's accounts; the check keeps them). A typed part matches the start of a name, or of its parts in order (`ex:fo` for `expenses:food`, `groc` for `expenses:food:groceries`); a payee also by any of its words. Enter or Tab takes the item. Ledger has no language server, so this is its completion; with hledger-lsp or beancount-language-server, the plugin's accounts and payees stand in for the server's, and the server still completes the rest (tags, dates).

In a journal, under `SPC m` in Vim's command mode and in the palette's Journal group, each one undo step:
In a journal, under `SPC m` in Vim's command mode and in the palette's Journal group, each one undo step:

- *New Transaction* (`SPC m a`): asks the date (today offered; `yesterday`, `-3`, `5`, `10/5`, `friday`, `last friday` taken) and the payee (the journal's, the last written first, or another), then writes the transaction the tool drafts from that payee's last one (`hledger print --match`, `ledger xact`, Beancount's last entry), dated, not cleared (no status, no posting's mark; Beancount's flag kept) and without its old balance assertions (a comment's `=` kept), at its date's place (or at the end, `insert_at`), the cursor on its first amount.
- *Toggle Status* (`SPC m t`): unmarked, pending `!`, cleared `*`, unmarked (Beancount's `txn`, `!`, `*`); the transactions selected all to the first one's next; the posting at the cursor with `toggle_postings`.
- *Copy Transaction* (`SPC m y`): the transaction again, dated today, at its date's place, not cleared and without its balance assertions, as New Transaction writes a draft.
- *Delete Transaction* (`SPC m d`): the transaction at the cursor, with its comment lines above it.
- *Next* and *Previous Transaction* (`]]`, `[[`; Alt+Down, Alt+Up).
- *Date a Day Later* and *Earlier* (`ctrl+shift+l`, `ctrl+shift+h` in Vim's command mode).
- *Sort Transactions* (`SPC m s`): those selected, or the file's, by date, each with the comment lines above it; refused where a directive between them (`apply account`, `alias`, `include`, `pushtag`…) would apply to others after sorting.

The outline sidebar (`gO` with Vim's keys, Ctrl+Shift+O) lists the journal's Org headings (`* Household`, `** Food`, lines all three tools read as comments), else the years and months its transactions begin, for Kalem 0.6.30 and later.

## The balance and the register

*Balance* (`SPC m b`) opens the main journal's balance as a document: assets, liabilities and equity at the period's end, then income, expenses and the other accounts within the period, as a tree (each parent with its subaccounts' amounts), every amount and total as the tool computes and writes it. Tab, Right and Left fold and unfold an account (kept as you leave it), `p` picks the period (this month, last month, this quarter, this year, last year, all dates, or Dates… typed: `2026`, `2026-03`, `2026-Q1`, `2026-03-05`, or a range such as `2026-01..2026-03`, January to March), `d` the depth, `v` shows market values (or back to cost), `r` runs it again, Enter opens the account's register, `q` or Escape closes it. It is written again when the journal is saved.

*Register* (`SPC m g r` on a posting's account, or Enter in the balance; else an account to pick) lists the account's postings, its subaccounts' too, with the tool's running total; Enter opens the journal at the posting, `p` picks the period (the running total stays the account's balance, the postings before the period counted; Beancount's first line is its opening balance), `v` the market values, `/` narrows it by the tool's own query (hledger's `desc:market amt:>100`, Ledger's `payee market`, a Beancount condition such as `narration ~ 'Market'`; the running total then the postings shown). The amounts are written as the journal writes them. The last 2,000 postings are shown.

## Import from bank statements

In a bank statement (a CSV or TSV file), *Import Statement* (`SPC m i`) shows what the tool would add to the journal, as the document *Import: statement.csv*; `i` there imports it, `e` opens the statement's rules, `r` runs it again. The journal is the setting `journal` (beside the statement when relative), else the one `LEDGER_FILE` names (asked through `hledger files`, so only where hledger is installed), else the journal you last opened.

- **hledger** imports through the statement's rules (`statement.csv.rules` beside it, or the setting `rules`), writes the journal itself and remembers what it took, so the next import of a longer statement adds only the new lines. A statement without rules is offered rules made from its header row (columns, date format, decimal mark, the bank's account), to check and save; `SPC m e` opens them.
- **Ledger** converts a statement whose header uses its names (`date`, `payee`, `amount`), for the account the setting `ledger_account` names and dates in `ledger_date_format`; the plugin appends the result to the journal, one undo step.
- **Beancount** imports through your own [beangulp](https://github.com/beancount/beangulp) script: `import.py` beside the main journal (or the setting `importer`), run as `python3 import.py extract -e JOURNAL STATEMENT`, so that what the journal has already is left out. `e` in the Import document opens the script; `i` appends its entries to the journal, one undo step. A Python with beangulp in a virtual environment is named by the setting `programs.python3`. beangulp's own importers write the bank's posting only: your script's hooks, or you, add the other side, and Check Journal says which transactions do not balance yet.

## Other reports

*Report…* (`SPC m r`, Doom's ledger-report, or `o` in a Report) lists the Balance and the Register (only when you start it from a journal), then the tool's other reports, and opens the one picked as a document, as the tool prints it: hledger's cash flow, income statement, budget and forecast (monthly, from its periodic transactions), activity, return on investment (`roi`, which asks its investment and profit-and-loss queries first, the last answers offered, and values at the end in market prices), statistics, its accounts, commodities, payees, tags and prices, and `print`; Ledger's income statement, budget, forecast (six months on), cleared report, statistics, lists and `print`; Beancount's through `bean-query`: income statement, holdings at cost and value, monthly expenses, balances, lists and `PRINT`, after the journal's own saved queries (its `query` directives). `print` comes in the journal's syntax and colors. `p` picks the period of a report that takes one (hledger's and Ledger's), `r` runs it again, and it is run again when the journal is saved.

The setting `reports` puts reports of your own before the tool's (after the Balance and the Register): `"Food by month: balance expenses:food -M"`, the tool's arguments after `-f JOURNAL` (quoted as a shell quotes), or, for Beancount, a query.

## Reports in notes

In an Org or Markdown document, *Insert a Report as a Table* (the palette's Journal group) offers the reports the tool writes as CSV (hledger's balance reports, register and `print`; Ledger's balance, register and `print`; Beancount's queries, its saved ones too), asks the period where the report takes one, and writes the table under the cursor's line with a caption naming the report, the journal and the period, one undo step: a monthly note keeps its figures. The journal is the setting `journal` (beside the note when relative), else `LEDGER_FILE`'s, else the journal you last opened. *Run the Journal Block* (`SPC m x` in Vim's command mode, or the palette), with the cursor in an Org document's `hledger`, `ledger` or `beancount` source block, gives the block's text to its tool on standard input with the block's `:cmdline` (ob-ledger's header argument; `:cmd` too; for Beancount, a query, `BALANCES` without one) and writes the output under the block as `#+RESULTS:` in fixed-width lines, replacing the results of the last run. Kalem's own Babel runs the same blocks too, with Kalem's plugin API 0.2.21 and later: name `hledger`, `ledger` or `beancount` in Kalem's setting `babel.load_languages`, and Enter in Vim's Normal mode (`C-c C-c` with the Emacs keys) asks before it runs the block as Emacs's ob-ledger does, `:cmdline` `bal` by default, and writes the tool's output under `#+RESULTS:` as Org writes it (an example block from ten lines on).

## From the command line

`kalem run` runs the plugin's commands without a window and prints the documents they write (Kalem 0.6.18 and later, which wait for the tool's answer). Arguments go as JSON:

```sh
kalem run pta pta.check 2026.journal
kalem run pta pta.balance 2026.journal --args '{"period": "this year", "value": true}'
kalem run pta pta.register 2026.journal --args '{"account": "assets:bank"}'
kalem run pta pta.report 2026.journal --args '{"report": "Cash Flow", "period": "last month"}'
kalem run pta pta.report 2026.journal --args '{"report": "Return on Investment", "investment": "acct:assets:broker", "pnl": "acct:income:investment"}'
```

A period is `this month`, `last month`, `this quarter`, `this year`, `last year`, `all dates`, or dates as Dates… takes them (`2026-Q1`, `2026-01-15..2026-02-14`; a range takes the whole of its second part, and either side may be left out); a report is named by its title in Report… (any case), a saved Beancount query as `Query: NAME`; the Balance and the Register have their own commands, `pta.balance` and `pta.register`. An unknown report is an error, and `kalem run` exits with 1. An unknown period is an error too: the document is still printed, for all dates, and `kalem run` exits with 1. The setting `glyphs` (`ascii`) writes the Balance's fold marks and a passed check's mark as `v > ok` for a terminal font without `▾ ▸ ✓`.

## Prices and market value

*Fetch Prices* (`SPC m p`) asks your price program for the prices since the journal's last and writes them at the end of the main journal, or of the file the setting `prices` names (beside the main journal when relative; your journal includes it), one undo step.

- **hledger and Ledger**: [pricehist](https://gitlab.com/chrisberkhout/pricehist) (`pip install pricehist`) fetches each pair of the setting `price_sources`, as `SOURCE BASE/QUOTE` (`ecb EUR/USD`; a third word where the journal writes the quote otherwise, `ecb EUR/USD $`), after the last price of the pair the tool lists, or the last 30 days; written as the journal writes prices (`P 2026-10-09 EUR 1.1206 USD`). The date separator is the one of the file's first transaction. The decimal mark is `,` when the file has a `decimal-mark ,` line, else `.`.
- **Beancount**: `bean-price --update` (`pip install beanprice`) fetches what its commodities' `price:` metadata names since their last price.

`v` in the Balance and the Register shows market values: hledger's `-V` (its transactions' costs taken as prices too), Ledger's `-V`, Beancount's `value()`; the header says "market value".

## Reconciliation

*Reconcile* (`SPC m R` on a posting of the account, or pick one of your asset and liability accounts) asks the statement's closing balance (the tool's total of the cleared and pending postings offered) and opens a document with the cleared total, the pending total, their sum, the target and the difference, then the postings not cleared yet. An account in several currencies (a travel wallet, a multi-currency bank account) gets its balance asked for each currency in turn and a block of these figures for each. The Reconcile document opens in a pane beside the journal (`reconcile_beside`). As you tick postings off the statement, Tab on one in the Reconcile document marks it pending in the journal (or takes the mark back), saves the journal and shows the reconciliation again with the difference moved; `SPC m t` in the journal marks one too, and the Reconcile document follows the journal as you type, before you save (`reconcile_live`, on; hledger and Ledger read the unsaved text on their standard input, Beancount's follows on save; while the text as typed does not read, the last figures stay, said so); `t` changes the target (each currency's in turn), Enter opens the journal at a posting. When the difference is zero (every currency's), *Finish Reconciliation* (`C`, or `SPC m C`) asks the statement's date, clears the pending postings and writes a balance assertion for each target (hledger and Ledger: a "Balance assertion" transaction with a zero posting for each currency, hledger's `=*` counting the subaccounts as the totals do, Ledger's `=` the account alone, so that where its subaccounts have postings Ledger's asserts the account's own balance; Beancount: a `balance` directive for each, the day after), one undo step. A posting of the account marked neither on itself nor on its transaction and dated on or before the statement's date, such as an outstanding check, does not stop it: the pending marks are cleared all the same, as ledger-mode's Finish clears them, and the balance assertion, which would count that posting and fail, is left out, the message saying why. The assertion is written only where the tool's balance of the account at the statement's date, every posting counted, is the target. Finish edits only the file where you started Reconcile: pending postings of the account in other included files stay pending. The totals are the tool's; the plugin only subtracts them from the target you typed.

With the bank's statement open (a CSV file), *Pair with the Reconciliation* (`SPC m r` there) pairs its lines with the account's postings by amount and date (within `match_days`, 3): in the Reconcile document each posting not cleared says whether it is on the statement, and the statement's lines with no posting are listed in red, for Import to take. A statement that writes the account's amounts the other way round (a card's charges above zero) is paired turned. It is paired again each time the reconciliation is written, until another is paired.

## Checking by the tool

*Check Journal* (`SPC m k` in Vim's command mode, the palette, or a click on the status bar item) runs the journal's own tool on its main journal: `hledger check --strict` (with the checks the setting `checks` adds, such as `ordereddates`), `ledger --pedantic --explicit balance`, or `bean-check`. Its problems open as the document *Check: 2026.journal*: each one's place, the tool's message, and the lines it quoted; Enter opens the file at the problem, `r` checks again, `q` or Escape closes it. hledger stops at its first problem; Ledger and Beancount give them all.

Each save checks again (`check_on_save`, on): the status bar item shows `✓` or the problems' count (`hledger · 2026.journal · 1 problem`), and an open Check document is written anew. A tool not installed is said once, with how to install it.

The tool's problems are also drawn in the journal as a language server's diagnostics are: a wavy line under each problem's place (the stretch hledger marks, else the whole line), the line number in the problem's color, the problem on the cursor's line in the status bar, its message on hover, and List Problems (`SPC c x`). Each check sets them anew, so a problem fixed and saved goes away. They stand beside the servers' diagnostics, which are their own and may disagree with the tool (hledger-lsp checks with rules of its own): the tool's are the truth.

## Aligning amounts

*Align Amounts* (`SPC m =` in Vim's command mode; the palette's Journal group) puts the amounts' last digits of the transaction at the cursor, or of every transaction selected, at one column: the setting `amount_column`, 52 as in ledger-mode, or 0 for as near the accounts as the longest posting allows. Only the blanks between account and amount change, two at the least: the amount as you wrote it (sign, decimal mark, digit groups, commodity and its side), a cost, a lot, a balance assertion and a comment stay as they are, and a posting without an amount is left alone. A value expression (`(-$132.00)`) is aligned by its closing parenthesis, a posting with a balance assertion alone by the assertion's amount. One undo step.

With `align_on_enter` (on), Enter on a posting aligns it first, as in ledger-mode: in the Word-like profile, and in Vim's insert mode too.

Format Document (`SPC c f`), and saving with `editor.format_on_save` on, align every transaction's amounts at `amount_column`, as Align Amounts aligns the transactions selected; nothing else of the journal changes (hledger-lsp's formatting is left unused, and `hledger print` rewrites a journal). In a Beancount journal the server's formatting comes first when the server runs. A formatter set in Kalem's settings comes before both: `[formatters] beancount = "bean-format -"`, or `off`.

## Large journals

Every tool call runs beside the editor, which never waits for it; the status bar item names the program running (`hledger · all.journal · ✓ · hledger…`). The tool reads the whole journal for each call, so on a large journal each takes the tool's time. Measured on the accountant's journal of `tests/large.rs`, 200,000 transactions over ten yearly files, on an Apple M-series Mac shared with other work (`cargo test -p kalem-plugin-pta --test large -- --ignored --nocapture`):

| | hledger 1.52 | Ledger 3.4 | Beancount 3.2 |
|---|---|---|---|
| Check | 9.9 s | 1.0 s | 2.0 s (its cache warm) |
| Balance sheet, a run | 7.8 s | 1.3 s | 7.2 s a level, the levels side by side |
| Register of the busiest account, this month | 7.8 s, 1.6 MB | 0.9 s, 0.1 MB | 7.1 s, 0.1 MB |
| … all dates (140,000 postings) | 184 MB: refused | 3.3 s, 13.5 MB | 11.3 s, 14.1 MB |
| A server's first completion | ptimoney's 12.8 s, juev's 4.5 s | none | 10.0 s |

Kalem takes at most 16 MB of what a program prints. An output past it is not shown cut: a notice says so and names a shorter period. hledger's register comes as JSON (its amounts' styles and statuses), some 1.3 KB a posting, so past about 12,000 postings of one account in the period it is refused, where Ledger's and Beancount's, some 100 bytes a posting, pass 150,000; the Register shows the last 2,000 either way. The plugin reads 14,000 postings of hledger's JSON in 0.1 s. hledger's `files` (which journal `LEDGER_FILE` names) is asked once until a journal is saved, and the place of a posting from hledger's `print` of its day only.

## Platforms

Built and tried on macOS, and on Linux (Debian 13, aarch64) with Debian's hledger 1.32 and Ledger 3.3 and Beancount 3.2 from pip: the tests pass, and in the terminal editor the checks, the Balance, Format Document, completion and a reconciliation work. On Linux, install Kalem's full build (`kalem-editor-…`, which wants `libxkbcommon` even for `kalem tui`): the terminal-only release (`kalem-terminal-…`) has no plugin host, so there only the plugin's languages load (highlighting, the servers), not its commands. Where the locale is not UTF-8 (a server or a container without `LANG`), Debian's hledger cannot read a journal's letters past ASCII; the plugin then runs it with `LC_ALL=C.UTF-8`. Windows is untried. A journal with Windows's line endings (CRLF) reads as any other, and what the plugin writes in it (a transaction, a balance assertion, a block's results) ends its lines the same way. A path a tool prints with `\` names the same file as the editor's with `/`, in any case. A Beancount block (Run the Journal Block) gives its text to `bean-query` as `/dev/stdin`, which Windows lacks: there it does not run.

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

Kalem shows completion, hover, definition, references, rename, diagnostics, document symbols and formatting, and the servers' inlay hints, dimmed at the end of a posting: an elided posting's amount (both hledger servers, seen in Kalem's terminal editor), and running balances from ptimoney's once its setting is on, in `settings.toml`: `[plugins."org.kalem.pta".settings.hledgerLanguageServer.inlayHints]` with `showRunningBalances = true`. Kalem's `editor.inlay_hints` (on) and Toggle Inlay Hints show or hide them all. It does not yet ask for code lenses.

The plugin turns hledger-lsp's formatting off (`servers.hledger-lsp.ignore`, `["documentFormattingProvider"]`), since ptimoney's 0.7.0 formats a journal into one that means something else; with juev's, whose formatting hledger reads as before, set it to `[]`.

Without its main journal, beancount-language-server checks an included file alone and reports its accounts as unknown (two false errors on `2026-01.beancount` in a test); told the main journal, the file is clean and completion knows every account of the journal. Set `servers.beancount-language-server.initializationOptions.journal_file` (absolute, from `~/`, or beside the file); the plugin takes it as Beancount's main journal too while `journal` is empty.

## Tests

`cargo test -p kalem-plugin-pta`: the manifest's two halves name what exists and agree; each tool's check, run as the plugin runs it, finds the one problem of each error file at its line and none in the clean journals, and each tool's error output (recorded) is read into its problems; every corpus journal aligned whole, at 52 and at 0, changes only the blanks before its amounts, is unchanged aligned again, and reads as before to its tool (`tests/align.rs`); its servers say their versions and how to install them in a line the status bar holds, and their settings are in sections the servers read; the main journal and the tool are worked out as said above (unit tests); the corpus through each server installed (`tests/server.rs`, with `PTA_HLEDGER_LSP` and `PTA_BEANCOUNT_LSP` naming programs, `:` between them): completion, the deliberate errors, and formatting the tool reads as before, ptimoney's 0.7.0 checked to still do the harm above; the syntaxes load as Kalem loads them, take the languages' files and source blocks, parse every corpus file to its end, color a transaction of each dialect and the rules file as the tests spell out, and color as tags and accounts what `hledger tags` and `hledger accounts` read; and the corpus, one journal per dialect (`corpus/hledger`, three files with `include`, and a bank statement with its CSV rules, which `hledger print` reads; `corpus/ledger`; `corpus/beancount`, two files), each passes its tool's strictest check (`hledger check --strict --auto`, `ledger --pedantic balance`, `bean-check`), and each `errors.*` file fails it at its line. A tool not installed is skipped with a note.
