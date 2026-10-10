# pta: plain-text accounting, the whole job in one plugin

The list for the `pta` plugin of getkalem/plugins (`plugins/pta`, crate
`kalem-plugin-pta`, id `org.kalem.pta`): hledger, Ledger and Beancount
journals written, checked, imported from bank statements, reconciled
against them and reported, inside Kalem, the same in the window and in
the terminal. Written 2026-10-10 before the folder existed, and moved in
with the crate (PT1); each task says what was done and what is open. It rests on Kalem's decisions D57 (one
language server client in the core, the plugins declare the servers),
D16 (Sublime syntaxes for the highlighter), D28 and D29 (a sandboxed
component, nothing added to the core) and on the rule of D55 carried
to a text format: a journal is written as its own tool writes it, and
nothing of Kalem's goes into it. A roadmap item waits on the owner
(R5.17 is proposed; R5.15, the git plugin, is the model for a plugin
that runs a program and writes documents). The letters are PT.

## Why this plugin, and why one

The audience is Kalem's own. The people who keep a journal in hledger,
Ledger or Beancount keep their notes in Org, use Doom Emacs's keys and
git, and stay in Emacs for ledger-mode, the one place that reconciles a
journal against a bank statement. The rest of their work is scattered:
the language servers written for VS Code validate and complete, Fava and
Paisa report and offer a small editor in the browser, `hledger import`
and the Beancount importers run in a terminal, and nothing joins them.
The plugin is that one place: the journal edited with completion and
checks, the statement opened beside it in Kalem's CSV grid, imported
through the user's rules, reconciled posting by posting, and the balance
and register read as documents in the editor, foldable and searchable
as the git plugin's status is. Nothing else to install but the tool the
user already has.

The shape is one plugin with two halves in one manifest. The declarative
half (D57) names the three languages, their syntaxes and their servers,
hledger-lsp and beancount-language-server, which Kalem's one client
runs: completion of accounts, payees, tags and dates, diagnostics,
formatting, references and rename come from them, since a completer of
a plugin's own has no binding yet (design §11.12). The component half
(`main`) does what no server does: it runs the user's tool through
`process` (`subprocess:hledger`, `subprocess:ledger`,
`subprocess:bean-check`, `subprocess:bean-query`) and writes documents
through `documents` and `styled-documents`, as the git plugin does with
git. **The tool does the accounting** (the git plugin's G1): the plugin
computes no balance in this list; hledger, Ledger and Beancount parse,
balance and check, and the plugin asks and shows. Whether a Rust engine
later makes even the tool unnecessary is PT12, a decision for the owner.
hledger comes first in every task, since the gap is widest there
(ledger-mode's reconciliation fails with hledger, and Fava is
Beancount's); Beancount second; Ledger where its syntax differs.

A task is done when it works in both editors, writes into the journal
only what its tool writes (the dialect's own syntax, the user's
alignment, decimal mark and comments kept, the file otherwise untouched
byte for byte), undoes in one step, and has its tests: the conformance
test on a corpus of three journals, one per dialect, each checked clean
by its tool where the tool is installed and skipped with a note where
it is not, as the rust plugin's tests do; and under `kalem run` every
document prints to standard output. Every command is in the palette,
its keys can be bound again, and the journal's commands sit under
Doom's local leader `SPC m` in a journal, named after ledger-mode's
where it has one. The tasks are in the order they are done, the spike
first.

## PT1. The spike: a journal served

- [x] PT1 `plugins/pta` from `template/`: `Cargo.toml`
  (`kalem-plugin-pta`; `kalem-plugin` for the component; `serde_json`
  and `kalem-highlight` for the tests), `src/lib.rs`,
  `tests/conformance.rs`, and a manifest with both halves: `id`
  `org.kalem.pta`, `name` "Plain text accounting", `main`
  `dist/pta.wasm`, `api` `^0.2.8`, `activation` `onLanguage:hledger`,
  `onLanguage:ledger`, `onLanguage:beancount`; `permissions`
  `subprocess` (the servers, which the core runs) and
  `subprocess:hledger`, `subprocess:ledger`, `subprocess:bean-check`,
  `subprocess:bean-query`; three languages, `hledger` (`extensions`
  `journal`, `hledger`, `j`; `filenames` `.hledger.journal`), `ledger`
  (`ledger`, `ldg`; `dat` is not claimed, too many files are `.dat`),
  `beancount` (`beancount`, `bean`), each with `comment` `;` (hledger
  and Ledger take `#` and `*` at the start of a line too, the
  syntax's work, not the comment command's) and no brackets; a fourth,
  `hledger-rules` (`rules`), for the CSV import rules of PT9, with no
  server; hledger's `timeclock` and `timedot` files are not claimed
  yet. The first check is Kalem's loader: `plugin_store.rs` reads
  `languages` beside `main` (`languages.rs` reads them from any
  manifest), but no plugin has shipped with both; if the component
  and the languages do not both load, that is the first ask of the
  core (K1) and the spike ships the declarative half alone. The
  corpus: `corpus/hledger/` (`2026.journal` including
  `accounts.journal` and `2025.journal`: `account` and `commodity`
  directives, `decimal-mark ,`, `include`, transactions with every
  status, tags, balance assertions `=` and `==`, a cost `@` and `@@`,
  a periodic `~ monthly` and an auto posting `=`, rooted by
  `LEDGER_FILE` since the layout has no marker file),
  `corpus/ledger/` (`business.ledger`: `apply account`, `alias`, `P`
  prices, `D`, virtual postings `(…)` and `[…]`, `assert`, `check`),
  `corpus/beancount/` (`household.beancount`: `option`, `plugin`,
  `open`, `close`, `commodity`, `pad`, `balance`, `price`, `note`,
  `document`, `event`, `custom`, `include`, lots `{…}`, prices `@`,
  tags, links, metadata); and beside each a file with one deliberate
  error (an unbalanced transaction, an undeclared account, a failed
  assertion) for the diagnostics tests. Done when `kalem lsp status
  corpus/hledger/2026.journal` names the plugin and a server,
  completion after an account's first letters lists the declared
  accounts in both editors, the deliberate error shows as a
  diagnostic, and `hledger check --strict`, `ledger bal --pedantic`
  and `bean-check` pass on the clean files and fail on the others in
  the conformance test.
  (Done 2026-10-10, against Kalem 0.6.9 built from its tag, with
  hledger 1.52.4, Ledger 3.4.1, Beancount 3.2.3 and hledger-lsp 0.7.0.
  The crate follows the graph plugin rather than `template/`, which is
  a viewer's. K1 holds: the installer lists the languages, the
  component and the programs together, `kalem lsp status` names the
  plugin and its servers, and the component runs beside the
  languages. Changed from the plan: no `activation`, since Kalem 0.6.9
  starts a plugin that declares files with the first of them
  (`onFile`) and 0.6.8 at its start, while `onLanguage:` starts no
  component; `api` `^0.2.9`, what the component imports. The component
  has one command for the spike, Tool Version, which runs the
  dialect's tool through `process` and says what it is, or how to
  install it and the setting `programs.NAME` that names where it is
  (each program has that setting, as the git plugin's). The corpus as
  planned, each clean journal passing its tool's strictest check
  (`hledger check --strict --auto`, `ledger --pedantic balance`,
  `bean-check`; Ledger's `--pedantic` wants `tag` declared too) and
  each error file failing at its line: hledger's a failed balance
  assertion (13:49), Ledger's an unbalanced transaction (lines 7 to
  9), Beancount's an account never opened (line 7). Checked in the
  terminal editor and through `kalem lsp`: completion after `exp`
  lists the declared accounts of the included `accounts.journal`; the
  failed assertion is a diagnostic at its line; Tool Version answers
  for the three tools and says how to install a missing one. Found:
  hledger-lsp warns that the automated posting's `(budget:food)` is
  not declared, taking the virtual posting's parentheses for the
  account's name (hledger accepts the file): the tool's check, PT5,
  is the truth. Found too: a plain text file's `textType` is its
  extension through a fixed table of the core's (`journal` for
  `2026.journal`, `bean` for `.bean`), not the plugin language's id,
  so the commands are scoped by both (`TEXT_TYPES`) until K7, and the
  status bar names the mode `journal`. Not driven: the graphical
  editor, which shares the language server client. Open: the servers'
  measurements (PT3), highlighting (PT2).)

## PT2. The highlighter: three dialects, one base

- [ ] PT2 Sublime syntaxes written for the plugin, since none can be
  taken: LedgerTools's `Ledger.sublime-syntax` is CC BY-NC-SA, which
  MIT OR Apache-2.0 cannot carry, and archived (2026-10-01); Ledger3's
  license is unstated; no Beancount or hledger `.sublime-syntax` is
  published. One base, `Journal (Base).sublime-syntax`, for what the
  dialects share: the date (and the secondary date after `=`), the
  status mark, the code in parentheses, the description with its
  payee and note split at `|` for hledger, the posting's account (its
  segments as a path, `entity.name.namespace`, the leaf
  `entity.name.label`, so the leaf stands out), the amount with its
  sign, decimal mark and digit groups, and its commodity (quoted or
  not, before or after), the cost (`@`, `@@`) and the balance
  assertion (`=`, `==`, `=*`), comments (`;`, and for hledger and
  Ledger `#` and `*` at the start of a line), `tag:value` pairs inside
  comments as `entity.name.tag`; and three files extending it:
  `hledger` (its directives `account`, `commodity`, `decimal-mark`,
  `include`, `payee`, `tag`, `alias`, `apply account`, `D`, `P`, `Y`,
  the periodic `~` and auto `=` transaction lines, the queries in
  their rules, virtual postings), `Ledger` (`!` and `@` directive
  forms, `check`, `assert`, `expr`, `define`, `bucket`, `fixed`,
  `comment` and `test` blocks), `Beancount` (`option`, `plugin`,
  `include`, `pushtag`, `poptag`, `pushmeta`, `popmeta`, the dated
  directives `open`, `close`, `commodity`, `balance`, `pad`, `note`,
  `document`, `price`, `event`, `query`, `custom`, `txn`, the quoted
  payee and narration, `#tag` and `^link`, `key: value` metadata, lots
  `{…}` and `{{…}}`, the `txn` flags). Scope names follow
  sublimehq/Packages' current ones, so Kalem's mapping of scopes to
  kinds colors them as the rust plugin's README lists. `hledger`,
  `ledger` and `beancount` fenced and `#+begin_src` blocks in Markdown
  and Org are highlighted by the plugin's syntaxes (a plugin's syntax
  takes the languages it names). The conformance test registers the
  syntaxes as Kalem does (`kalem_highlight::register`), parses every
  corpus file to its end, and checks the colors of one transaction
  per dialect span by span, with the cases that go wrong easily named
  (an account containing a space, a commodity with a space in quotes,
  a negative amount in parentheses in Ledger, a comment that is not a
  tag). The syntaxes are licensed as the repository is.

## PT3. The servers: found, in the right root, told their settings

- [ ] PT3a hledger-lsp. Two programs answer to the name: ptimoney's
  (TypeScript, `npm install -g hledger-lsp`, 0.7.x, MIT: completion
  that learns the accounts a payee uses, fifteen validations, running
  balances as code lenses and inlay hints, rename across files, code
  actions adding missing declarations, `include` followed from a root
  it detects) and juev's (Go, binaries on its releases: transaction
  templates, balance checks on save). Both measured on the corpus
  with `kalem lsp ask`; the manifest's `servers` names one entry,
  `hledger-lsp`, `command` `["hledger-lsp", "--stdio"]`, `candidates`
  the PATH's and npm's global bin, `install` the two ways, and the
  README says which program was measured and what the other lacks.
  The root: a journal has no marker file; `rootMarkers`
  `[".hledger.journal", "hledger.conf"]`, then the file's folder, and
  the server's own detection follows `include` upward. Its settings
  (`maxNumberOfProblems`, `hledgerPath`) surfaced as the rust plugin
  surfaces rust-analyzer's. What of its answers Kalem's client shows
  today (completion, hover, definition, references, rename,
  diagnostics, formatting) and what it does not (code lenses and
  inlay hints: the client answers their refresh requests with nothing
  and asks for neither, `kalem-lsp/src/client.rs`) measured and
  written down: the running balances in the margin are K4.
- [ ] PT3b beancount-language-server (Rust, `cargo install
  beancount-language-server` or Homebrew, 1.9.x): `command`
  `["beancount-language-server"]`, `candidates` it and
  `~/.cargo/bin/beancount-language-server`; it must be told the main
  journal (`initializationOptions` `{"journal_file": "…"}`), so the
  plugin's setting `journal` (PT3c) is passed; its diagnostics need
  Python's `beancount` (PyO3 embedded by default, the system Python as
  the fallback), said in `install` and in the README; its formatter is
  bean-format's, with `prefix-width`, `num-width` and
  `currency-column` as settings. Measured on the corpus as PT3a.
- [ ] PT3c The main journal and the tool. Includes make one file the
  root: the engine, the servers and every report must be given it.
  The plugin's setting `journal` (a path relative to the project; the
  default is `LEDGER_FILE` of the environment, else the file the
  command runs in) and `tool` (`auto`, `hledger`, `ledger`,
  `beancount`: `auto` is the dialect of the file's language, and for
  `.ledger` files, which hledger users name so too, hledger when it is
  installed, else Ledger). A status bar item shows the tool, the
  journal and its last check (`hledger · 2026.journal · ✓`, or the
  error count); a click runs the check (PT5). `kalem lsp status` and
  the process log say which program ran for what.

## PT4. Formatting as the tool writes it

- [ ] PT4 Format (Kalem's `SPC c f`): the server's formatting where it
  formats (both do), else `commands.format`: `bean-format` for
  Beancount, nothing for hledger and Ledger (`hledger print` rewrites
  too much: it normalizes amounts, drops directives and reorders).
  The plugin's own `pta.align` (`SPC m a`, ledger-mode's auto-align):
  the amounts of the transaction at the cursor, or of the selection,
  aligned so that their last digits meet at the column of the setting
  `amount_column` (52, ledger-mode's default; 0 for two spaces after
  the longest account), the user's decimal mark, digit groups,
  commodity side and comments kept, through `editor.replace` of the
  changed lines only; `align_on_enter` (on by default) aligns the
  posting just typed when Enter is pressed on it, as ledger-mode does.
  Nothing else of the file moves. Tests: a transaction with a quoted
  commodity, a cost, a balance assertion and a trailing comment,
  aligned, and unchanged by a second alignment; the file otherwise
  identical byte for byte.

## PT5. Checked by the tool

- [ ] PT5 `pta.check` (`SPC m k`; the status bar item's click; on save
  when the setting `check_on_save` is on, the default): `hledger check
  --strict` with the checks the setting `checks` adds
  (`ordereddates`, `payees`, `recentassertions`, `uniqueleafnames`,
  hledger's names), `ledger … --pedantic --explicit` (a balance report
  for its errors on standard error), `bean-check`, each on the main
  journal of PT3c, through `process`, never waiting. Their errors
  (`file:line:` and the message; hledger's with the transaction
  quoted) become a document "Check: 2026.journal" (kind `pta-check`),
  one entry per error, the message under it, Enter opening the file at
  the line (`file.open` with `line`), refreshed on every save while
  open, closed by Escape; and the status bar item's count. The
  server's own diagnostics stay; the tool's are the truth, and the
  README says the two may disagree (hledger-lsp validates with rules
  of its own, not by running hledger). A plugin publishing diagnostics
  of its own, shown beside the server's in the gutter, is K2; the
  document is what the API allows today. Tests: each dialect's error
  file gives one entry at the right line; a clean file gives none and
  the item shows ✓; a tool not installed gives one notice naming the
  install line and nothing else.

## PT6. Writing transactions

- [ ] PT6a `pta.newTransaction` (`SPC m t`; ledger-mode's `C-c C-a`):
  asks the date (today from `clock`; the prompt takes hledger's smart
  dates, `yesterday`, `10/5`, `last friday`, which the plugin resolves
  itself to the dialect's form), then the payee from a quick-pick of
  the journal's payees (`hledger payees`, `ledger payees`, `bean-query
  "SELECT DISTINCT payee"`, the recent ones first), then inserts, at
  the date's place in the file (after the last transaction dated on
  or before it; at the end when the setting `insert_at` is `end`),
  the transaction the tool drafts from the payee's last one (`hledger
  print --match PAYEE`, `ledger xact DATE PAYEE`; for Beancount the
  plugin copies the last entry with that payee from the text), the
  cursor on its first amount; a payee not seen before gives an empty
  transaction with one posting line and the cursor on its account,
  where the server completes. One undo step, named.
- [ ] PT6b `pta.toggleStatus` (`SPC m c`; ledger-mode's `C-c C-c`):
  the transaction at the cursor, or the posting when the cursor is on
  a posting line and the setting `toggle_postings` is on, goes
  unmarked → pending `!` → cleared `*` → unmarked, written where the
  dialect puts the mark (after the date in all three; Beancount's
  unmarked form is `txn`); the selection's transactions all together.
  `pta.nextTransaction` and `pta.previousTransaction` (`Alt+Down`,
  `Alt+Up` in a journal, as the git documents move by item),
  `pta.copyTransaction` (`SPC m y`: the transaction at the cursor
  again at today's date, inserted at its place), `pta.dateUp` and
  `pta.dateDown` (`Shift+Up`, `Shift+Down` on a date, as Org's
  timestamps move), `pta.sort` (`SPC m s`; ledger-mode's
  `ledger-sort-buffer`: the transactions of the selection, or of the
  file, by date and then by their order, each with the comments above
  it, directives kept in place; refused where an `include` or `apply
  account` between them would change meaning). Each a single
  `editor.replace`, one undo step; each tested on the corpus for the
  bytes it leaves alone.
- [ ] PT6c The outline (Kalem's outline and sidebar): the server's
  document symbols, which both servers give (transactions by date and
  description, directives), checked in both editors; nothing of the
  plugin's own until a plugin can give an outline.

## PT7. The balance and the register as documents

- [ ] PT7a `pta.balance` (`SPC m b`): the balance sheet of the main
  journal as a document "Balance: 2026.journal" (kind `pta-balance`),
  written as the git plugin writes its status: a header line with the
  tool, the period and the depth; the account tree with `▸` and `▾`
  marks, each account with its balance per commodity, totals under
  each section, assets and liabilities first, then income and
  expenses, as the tool gives them (`hledger balancesheet
  incomestatement -O json`, `ledger bal --flat` parsed, `bean-query`
  over `SELECT account, sum(position)`); Right and Left unfold and
  fold (what is folded kept across refreshes, by account), Tab
  toggles, Enter on an account opens its register (PT7b), the arrows
  move; `SPC m p` picks the period (this month, last month, this
  quarter, this year, last year, all, or typed in the tool's period
  syntax) and the depth; the document refreshes when the journal is
  saved (`document-after-save`), and Escape goes back. Styled:
  negative amounts red, totals bold, the tree's inner segments muted,
  as `styled-documents` allows. Under `kalem run pta.balance FILE` the
  same text prints.
- [ ] PT7b `pta.register` (`SPC m R`; Enter on an account in PT7a; the
  account at the cursor in a journal, else asked): "Register:
  Expenses:Market" (kind `pta-register`), one line per posting: date,
  status, description, the other account, the amount, the running
  balance, the tool's own (`hledger register ACCT -O json`, `ledger
  reg`, `bean-query` with a running `balance`); Enter on a line opens
  the journal at the transaction, by the source position the tool
  gives (hledger's `tsourcepos`) or by date and description where it
  gives none; the same period and depth keys; a query typed with `SPC
  m /` in the tool's own query language (`status:! amt:>100
  date:2026-10`) filters it. Large journals: hledger answers 100,000
  postings in a second, the document shows the last 2,000 and offers
  the earlier ones by period, since `process` cuts output at 16 MB and
  a document of a million lines helps no one.
- [ ] PT7c The other reports, one quick-pick (`SPC m r r`, "Report…"):
  cash flow, income statement alone, budget (`--budget` from the
  periodic transactions), forecast (`--forecast`), `roi`, `stats`,
  `accounts`, `commodities`, `payees`, `tags`, `activity`; Beancount's
  through `bean-query`, with the saved queries of the journal's
  `query` directives listed first; Ledger's `bal`, `reg`, `budget`,
  `prices`. Each a document of the tool's own text, highlighted where
  the output is journal syntax (`print`), with the keys of PT7a where
  the output is a tree. A report the user runs often is pinned by the
  setting `reports`, a list of names and arguments, and appears in
  the pick and in the Journal menu.

## PT8. Reconciliation

- [ ] PT8a `pta.reconcile` (`SPC m r`; ledger-mode's `C-c C-r`), in a
  journal: asks the account (a quick-pick of the asset and liability
  accounts, the one at the cursor first) and the target balance (the
  statement's closing balance; the default the account's last balance
  assertion or `balance` directive), and opens "Reconcile:
  Assets:Bank:Ziraat" (kind `pta-reconcile`) in a pane beside the
  journal: a header with the cleared total, the pending total, the
  target and the difference, in the account's commodity (one block per
  commodity for a multi-commodity account, which ledger-mode cannot
  do), then the unmarked and pending postings of the account (`hledger
  register ACCT status:! status: -O json`, `ledger reg --uncleared`,
  `bean-query … WHERE flag = '!'`), one line each with its status,
  date, description and amount, and after them the cleared ones,
  folded. The journal stays the place that is edited: `SPC m c` in
  the journal (PT6b) marks a posting pending, the reconcile document
  refreshes on the journal's save and 500 ms after its last change
  (`document-changed`), and the difference moves toward zero. Enter on
  a line opens the journal at that posting. This works with today's
  API; marking from the reconcile document itself is PT8b.
- [ ] PT8b `pta.finishReconcile` (`SPC m C`; ledger-mode's `C-c C-c` in
  the reconcile buffer), in the journal or the reconcile document:
  when the difference is zero, every pending posting of the account
  becomes cleared, and a balance assertion for the target at the
  statement's date is written as the dialect writes it (hledger and
  Ledger: `= AMOUNT` on a zero-amount posting of a dated transaction
  "Balance assertion", or on the day's last posting of the account
  when the setting `assertion_style` is `inline`; Beancount: a
  `balance` directive dated the next day, since Beancount checks at
  the start of the day), at the date's place in the file, one undo
  step; when it is not zero, a notice says the difference and nothing
  is written. Marking a posting from the reconcile document (Space on
  a line, as ledger-mode's reconcile buffer) needs an edit of a
  document other than the one the command runs in, K3; until then
  Space in the reconcile document opens the journal at the posting,
  where `SPC m c` marks it.
- [ ] PT8c The statement beside: `pta.reconcileWith`, in a CSV
  document (Kalem's grid) or from the reconcile document, pairs the
  statement's rows with the account's postings by date and amount
  (the date within the setting `match_days`, 3; the amount exact
  after the rules' sign and decimal conventions), and shows the pairs
  in the reconcile document as matched (muted), the postings without
  a row (yellow) and the rows without a posting (red, the import of
  PT9 offered for them). No tool does this; the plugin reads the CSV
  through `editor.text` when the command runs there, or through `fs`
  (`fs:read:workspace`) when it runs from the reconcile document.

## PT9. Import from bank statements

- [ ] PT9a hledger: `pta.import` (`SPC m i`), in a CSV document: the
  rules file beside it (`statement.csv.rules`, hledger's convention)
  or the one the setting `rules` names, else offered to be made from a
  template with the CSV's header row as `fields` and the journal's
  accounts to pick for `account1`; then `hledger import --dry-run FILE
  -f JOURNAL`, and its output, the transactions it would add in
  journal syntax, as a document "Import: statement.csv" (kind
  `pta-import`) highlighted with the plugin's hledger syntax, the
  postings to `expenses:unknown` (or the rules' default account)
  colored yellow; `SPC m i` again, or Enter on the header, runs
  `hledger import FILE -f JOURNAL` for real: hledger appends the
  transactions to the journal and records the latest date in
  `.latest.statement.csv`, so a second import skips what was taken
  (the plugin writes nothing itself); the journal, open in the editor,
  is read again from the file (Kalem's file watcher), and the cursor
  goes to the first imported transaction. `.rules` files are
  highlighted (PT1's fourth language: `skip`, `fields`,
  `date-format`, `if` blocks with their patterns, `account1`…,
  `amount`, `balance`, `include`), and "Edit rules" (`SPC m e`) opens
  the rules file from the CSV or from the import document.
- [ ] PT9b Beancount: the user's beangulp import script (`import.py`
  at the journal's root, or the setting `importer`) run as `python3
  SCRIPT extract FILE` (the permission `subprocess:python3`, asked of
  the owner: a broad program to grant, and the alternative, the script
  made executable and named by a setting, needs a program permission
  the manifest cannot name in advance), its output shown as PT9a's
  document, and accepted by the plugin appending it to the journal:
  `fs:write:workspace` on the main journal when it is not open in the
  editor, `editor.insert` at its end when the import document is
  closed and the journal is the current document, and K3 for the clean
  way. `bean-check` runs after, as PT5.
- [ ] PT9c Ledger: `ledger convert FILE` with the account and the input
  date format from settings, through PT9a's document, appended as PT9b
  appends.

## PT10. Prices and values

- [ ] PT10 `pta.prices` (`SPC m $`): `bean-price` for Beancount
  (`subprocess:bean-price`), `pricehist` for hledger and Ledger
  (`subprocess:pricehist`, where installed; both say so in `install`),
  for the commodities the journal holds and the dates missing since
  its last price, their output (`P` lines, `price` directives)
  appended to the file the setting `prices` names (the journal by
  default), as the tool prints it; and the market value toggle of the
  reports (`-V`, `--value=then|end|now`; `bean-query`'s `convert`),
  `SPC m v` in a balance or register document, with the valuation in
  the header. Tests: a corpus with two commodities and prices,
  valued; a tool not installed gives one notice.

## PT11. The rest of Kalem

- [ ] PT11a A **Journal** menu in the menu bar and the F10 list while
  the current file is a journal or one of the plugin's documents
  (`when` `textType == hledger || textType == ledger || textType ==
  beancount || textType == pta-…`): New transaction, Toggle status,
  Align, Sort; Check; Balance, Register, Reconcile, Report…; Import,
  Edit rules; Prices; the tool and the journal (PT3c's settings
  through a quick-pick); the leader table's `SPC m` rows named for
  which-key, as the git plugin's `+git` rows are. A toolbar button
  "Journal" beside Git, opening the balance.
- [ ] PT11b Org: `pta.insertAsTable` in an Org document, the report of
  PT7c chosen as an Org table (`hledger … -O csv`, `bean-query -f
  csv`, converted as Kalem converts CSV to an Org table and inserted
  at the cursor through `editor.insert`), so that a monthly note holds
  its figures; a `hledger` or `beancount` source block run by Babel
  (`#+begin_src hledger :cmd "bal"`) with the tool's output as its
  result, when R5.6 gives plugins executors (K6). The three dialects'
  blocks are highlighted from PT2.
- [ ] PT11c Batch and the terminal: `kalem run pta.balance FILE`,
  `pta.register FILE ACCOUNT`, `pta.check FILE` print their documents;
  every document and key the same in the terminal editor, checked by
  hand and by the frontends' snapshot tests, with the `glyphs`
  setting of the git plugin (`▸ ▾` or `> v`) shared.

## PT12. An engine inside (later; the owner decides)

- [ ] PT12 A Rust engine for the hledger and Ledger journal (the
  parser with positions, balancing, the register, the balance report,
  the checks of PT5, the common directives) inside the component, so
  that the plugin answers with no tool installed, and rustledger
  (Rust, Beancount-compatible) weighed for Beancount: whether it
  builds for `wasm32-unknown-unknown` inside the sandbox, and whether
  its results match `bean-check` on the corpus. The rule stays the
  xlsx plugin's with IronCalc: the installed tool is the oracle where
  it is present (the engine's results compared with the tool's on
  every report and the differences reported), and the engine answers
  only where the tool is absent, or for the reconcile document's
  500 ms refresh where a process round trip is too slow. Weighed
  against D55's spirit (the tool computes, the plugin shows) and the
  install friction it removes; the README says which it is.

## PT13. Speed, limits and the tests

- [ ] PT13 The accountant's journal: 200,000 transactions over ten
  files. Every tool call asynchronous (`process` never waits), the
  status bar item showing the running one; the documents of PT7
  paged by period as said; outputs over `process`'s 16 MB refused
  with a notice that names a narrower period, not cut silently; the
  component's budget (1 GB, 10 s a call) kept with the JSON of a
  100,000-posting register parsed in under a second; the servers'
  time to first completion on that journal measured and written in
  the README. Tests: the conformance test (manifest with both halves;
  the syntaxes coloring the corpus; the servers found, their roots
  right for every corpus file; each tool's check clean on the clean
  files and failing on the error files, skipped with a note where the
  tool is missing); unit tests of the parsing of each tool's output
  (hledger's JSON, Ledger's text, beanquery's CSV) on recorded
  fixtures, so they run without the tools; snapshot tests of every
  document kind in both frontends; the byte-for-byte tests of PT4 and
  PT6 on every corpus file.

## PT14. Release, the README and the Book

- [ ] PT14 The README: what it does, the feature table per dialect
  (what the server gives, what the tool gives, what the plugin
  itself does), the install lines (hledger, Ledger, Beancount and
  Python's `beancount`, the two servers, `bean-price`, `pricehist`),
  the keys under `SPC m`, the settings, the known differences from
  ledger-mode (what its reconcile does that PT8 does not yet, K3) and
  from Fava (charts, the browser editor); a line in `CODEOWNERS`; the
  entry in `index.json`; the tag `pta-v0.1.0` once PT1 to PT8 are
  done; the Book's Part IV page for the journal formats (D53), which
  waits on Kalem.

## What Kalem's core must gain

Each is marked "waits on Kalem" in the task that needs it, and is
written for every later plugin, not for this one (the rule the git
plugin's section 9 followed).

- K1 A manifest with both `main` and `languages`, its component and
  its languages both loaded and both listed by `kalem plugin` and the
  installer's summary. Holds in Kalem 0.6.9 (PT1): nothing to gain.
- K2 Diagnostics published by a plugin for a document (the tool's
  check results of PT5 beside the server's, in the gutter and the
  problems list): a `diagnostics-2` or `publish` interface on the path
  the language server client's diagnostics already take.
- K3 Edits to an open document other than the one the command runs
  in, by path (the reconcile document marking a posting in the
  journal, PT8b; an import appended to the journal, PT9b): `editor`
  reaching a named document, or the editable generated documents with
  a `saved(text)` event the git design planned (its 9.2 and 9.8).
- K4 Running balances in the margin: inlay hints from the language
  server client (both servers give them; `client.rs` asks for none),
  or virtual text in `decorations` (planned in the git design's 9.3,
  not built).
- K5 An outline provider and a completer from a plugin (design §11.11
  and §11.12's bindings), for the `.rules` files' accounts and for a
  journal whose server is missing.
- K6 Babel executors from plugins (R5.6), for `hledger` and
  `beancount` source blocks in Org (PT11b).
- K7 A plugin language's id as the text type of the files it serves:
  `DocumentState::text_type` takes a plain text file's language from
  its extension through `command::canonical_type`'s fixed table (where
  Elixir's `ex` is listed), so `2026.journal` is `journal`, `x.bean`
  `bean`, and a plugin's `scope` and `when` must name the extensions;
  `languages::for_path` knows the plugin's language and could answer
  first. The design (§11.2) says plugins extend the vocabulary. Until
  then the plugin scopes by both (PT1).

## Open for the owner

- The name: `pta` (the family) against `ledger` (one tool's name, and
  the Emacs mode's) or `journal`; the id and the letters follow.
- Which dialect first: hledger is proposed (the widest gap, the
  fastest tool, the server with the most to show); the owner may want
  Beancount first for its stricter format and Fava's users.
- PT12: whether an engine inside the plugin is wanted at all, or the
  tool stays the only computer, as D55's spirit says.
- `subprocess:python3` for the Beancount importers (PT9b): a broad
  program to grant; the alternative waits on a permission that names
  a script.
- `.dat` and `.j` claimed or not; hledger's `timeclock` and `timedot`
  as later languages.
- The roadmap item (R5.17 proposed) and whether the plugin is bundled
  (D28) or installed from `index.json` as the git plugin is.
