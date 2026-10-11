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
balance and check, and the plugin asks and shows. One plugin, one
manifest, one install: the user adds the plugin and the hledger,
Ledger or Beancount they already use, and needs nothing else (no Fava,
no VS Code, no ledger-mode, no import command in a terminal); the
language servers, where installed, add completion and their own
checks. No engine inside makes the tool unnecessary: the owner decided
so on 2026-10-11 (PT12), the tool being the reference implementation,
as git is the git plugin's.
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

- [x] PT2 Sublime syntaxes written for the plugin, since none can be
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
  (Done 2026-10-11, checked in Kalem 0.6.13's terminal editor with the
  plugin installed: the three journals, a `.rules` file, and
  `hledger` and `beancount` source blocks in Org. Five files under
  `syntaxes/`: `Journal (Base)` (a `syntaxBases` entry, hidden: the
  header and postings, accounts, amounts, costs, lots, assertions,
  comments and their tags, the periodic and automated transactions,
  and the directives hledger and Ledger share, Ledger's that hledger
  reads and ignores included), `hledger` (payee and note at `|`,
  `decimal-mark`), `Ledger` (`!` and `@` before a directive, the payee
  ended only by a `;` after a tab or two spaces as Ledger reads it,
  `:tag:` lists and `Key: value`, value expressions in parentheses,
  timeclock lines, `python`), `Beancount` (over the base for postings,
  amounts and lots; its own `main`: the dated directives, flags,
  strings, `#tags`, `^links`, metadata, arithmetic, Org headings), and
  `hledger CSV rules` (standing alone: directives, field assignments
  and references, `if` blocks, and tables split by the character
  after `if`). Changed from the plan: the scopes are chosen for
  Kalem's colors (kalem-highlight's kinds), which have none for
  `entity.name.namespace` or `entity.name.label`: an account is one
  `support.type.account` (a type's color; `entity.name.type.account`
  where declared), its leaf not set apart; dates and amounts are
  numbers, commodities constants (the same color), descriptions,
  payees and paths strings, statuses and directives keywords, tag
  names with their colon tags, a tag's value stays the comment's,
  queries and the rules' matchers regular expressions. Tags are read
  only where the tools read them: a comment line of the file's own
  carries none, nor does a Beancount comment. `tests/syntaxes.rs`
  loads the files as Kalem does (`kalem_highlight::register`, the
  languages' ids and extensions as aliases), checks that each id,
  extension and syntax name finds the plugin's syntax and the base
  none, parses every corpus file to its end through syntect with
  `extends` resolved and checks the line after it, checks one
  transaction of each dialect and the rules file span by span, the
  easy mistakes (an account with a space, `"FUND A"`, Ledger's
  `(-$132.00)`, a top-level comment with a colon, Ledger's `;` inside
  a payee, a posting's status, Ledger's lot price and date), and that
  the tags and accounts colored in the hledger corpus are those
  `hledger tags` and `hledger accounts` list. The corpus gained those
  cases (each tool still passes it) and `statement.csv` with its
  rules, which `hledger print` reads into the right accounts. Not
  driven: the graphical editor, which loaded the syntaxes (its log)
  and colors through the same highlighter.)

## PT3. The servers: found, in the right root, told their settings

- [x] PT3a hledger-lsp. Two programs answer to the name: ptimoney's
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
  (Done 2026-10-11, with ptimoney's 0.7.0 from npm and juev's 0.2.64
  built from its module, each through Kalem 0.6.14's `kalem lsp ask`
  and `check`, and asked directly for what Kalem cannot ask yet
  (rename, code actions, inlay hints, code lenses); the README's table
  has every answer. Neither is the clear choice. ptimoney's: completion
  of the declared accounts, the failed assertion at its line, but its
  formatting changes what the journal means (an automated posting's
  `*-1` dropped, so hledger books 0 to the budget, and `==` written
  `=`), its definition and hover put an account's declaration on the
  main journal's first line, its rename changes the open file only,
  and it takes `(budget:food)` for undeclared. juev's: definition,
  rename across files and payee completion right, formatting that
  hledger reads as before, but an error on the automated posting's
  `*-1` and a hover balance that misses the amounts hledger infers.
  The manifest: one entry, `hledger-lsp`, `--version` for `kalem lsp
  status`, `candidates` the PATH's, npm's (`~/.npm-global/bin`,
  Homebrew's), Go's (`~/go/bin`); `rootMarkers` with
  `.hledger-lsp.json`, ptimoney's configuration file; `settings` with
  both programs' sections (`hledgerLanguageServer`, `hledger`), a few
  of each surfaced (`maxNumberOfProblems`, `validation.…`,
  `completion.onlyDeclaredAccounts`; `diagnostics.accountCheck`,
  `completion.accountScope`), checked to reach each program; not
  `hledgerPath`, which ptimoney's reserves and does not read. `install`
  names one way, the README both: Kalem shows `install` in the status
  bar when the server is missing, and a long one pushed the plugin's
  own item out of it. Found: Kalem formats with a server whenever it
  offers formatting, before the plugin's `commands.format`, and the
  user's `formatters.TYPE = "off"` does not stop it, so nothing keeps
  ptimoney's from harming a journal but the README's warning and undo:
  K9, built in Kalem the same day (branch `server-options`); the
  manifest's `hledger-lsp` has `"ignore": ["documentFormattingProvider"]`
  and the setting `servers.hledger-lsp.ignore` describes it, `[]` for
  juev's. `tests/server.rs` talks to each program installed through
  `kalem-lsp` and checks a formatter by the tool (`hledger print`
  before and after), ptimoney's 0.7.0 checked to still do its harm, so
  that a fixed release is noticed. Ledger has no server; hledger-lsp
  is not given Ledger's files, since hledger cannot read their value
  expressions.)
- [~] PT3b beancount-language-server (Rust, `cargo install
  beancount-language-server` or Homebrew, 1.9.x): `command`
  `["beancount-language-server"]`, `candidates` it and
  `~/.cargo/bin/beancount-language-server`; it must be told the main
  journal (`initializationOptions` `{"journal_file": "…"}`), so the
  plugin's setting `journal` (PT3c) is passed; its diagnostics need
  Python's `beancount` (PyO3 embedded by default, the system Python as
  the fallback), said in `install` and in the README; its formatter is
  bean-format's, with `prefix-width`, `num-width` and
  `currency-column` as settings. Measured on the corpus as PT3a.
  (Done 2026-10-11: 1.9.2, from crates.io and in Homebrew, measured as
  PT3a: completion of accounts (an included file's too), payees, tags
  and currencies, definition, references and rename across files,
  `bean-check`'s errors (run as a program, `bean-check` on the PATH;
  not Python embedded), its formatting read as before by `bean-query`;
  no hover, no code actions. Without a main journal it checks the open
  file alone: a file included from another gets its accounts reported
  unknown (two false errors in a test of three files); told the main
  journal, the file is clean and completion knows every account. The
  manifest has `--version`, the Homebrew and Cargo places, a short
  `install`. Open, waiting on Kalem: telling it the main journal and
  its formatting widths, which it reads only from
  `initializationOptions`, fixed in the manifest today: K8.
  Later the same day: K8 built in Kalem (branch `server-options`), and
  the manifest describes
  `servers.beancount-language-server.initializationOptions.journal_file`,
  which the component takes as Beancount's main journal too while
  `journal` is empty, so one setting serves both; checked with a
  Kalem built from the branch: the included file clean, the item
  `beancount · main.beancount`. Done once that Kalem is released.)
- [x] PT3c The main journal and the tool. Includes make one file the
  root: the tool, the servers and every report must be given it.
  The plugin's setting `journal` (a path relative to the project; the
  default is `LEDGER_FILE` of the environment, else the file the
  command runs in) and `tool` (`auto`, `hledger`, `ledger`,
  `beancount`: `auto` is the dialect of the file's language, and for
  `.ledger` files, which hledger users name so too, hledger when it is
  installed, else Ledger). A status bar item shows the tool, the
  journal and its last check (`hledger · 2026.journal · ✓`, or the
  error count); a click runs the check (PT5). `kalem lsp status` and
  the process log say which program ran for what.
  (Done 2026-10-11, in Kalem 0.6.14's terminal editor: the settings
  `journal` and `tool`, and a status bar item `hledger · 2026.journal`
  whose tooltip says why that journal and which program reads it; its
  click runs Tool Version until PT5's check takes it, with the item's
  ✓ or count. The logic is `src/journal.rs`, tested natively; the
  component follows the documents opened, closed, saved and the one
  the cursor is in (`selection-changed`), reads the settings again
  each time (Kalem may start the plugin before it has read them), and
  drops an answer a newer refresh made stale. Changed from the plan:
  `LEDGER_FILE` is read through hledger (`hledger files` without `-f`
  lists that journal and its includes; the main journal when it lists
  the file), since a component sees no environment, and for Ledger not
  at all (it lists no included files that hold no postings); a
  relative `journal` is beside the file, as a component cannot list
  the projects (graph's K10); a `.ledger` file under `auto` goes to
  Ledger where Ledger is installed and to hledger only where Ledger is
  not, since hledger refuses the corpus's Ledger value expression.
  Found: Kalem runs a plugin's programs only in a project's folder (or
  the first project's when none is given), so a journal outside the
  projects is read from there by absolute path, and with no project
  at all not at all; the README says to add the journal's folder.
  `kalem lsp status` names each server's program and version.)

## PT4. Formatting as the tool writes it

- [~] PT4 Format (Kalem's `SPC c f`): the server's formatting where it
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
  (Done 2026-10-11 but Enter in Vim's insert mode; checked in Kalem's
  terminal editor. Format, changed from the plan: no `commands.format`,
  since a language plugin's `commands` are the plugin's, one `format`
  for its three languages, and `bean-format` would run on hledger's
  files (K11); hledger-lsp's formatting is left unused (PT3a, K9) and
  `hledger print` rewrites too much, so Format Document in an hledger
  or Ledger journal says there is no formatter (with K9's Kalem;
  before it, hledger-lsp's), and Align Amounts is the journal's
  formatter. Beancount's: its server's (bean-format's widths, read as
  before by `bean-query`, PT3b), else, without the server, the user's
  `formatters.beancount = "bean-format -"`, as the README says.
  Align Amounts (`pta.align`, `SPC m a` in Vim's command mode, the
  palette): `src/align.rs`, the anchor an amount's last digit (the sign
  and a left commodity before it, Beancount's sum `800.00 + 42.35`
  taken whole, a cost, a lot and a comment after it), a value
  expression's closing parenthesis (`(-$132.00)`), or, for a posting
  with a balance assertion alone, the assertion's amount; two blanks
  at the least between account and amount (hledger's and Ledger's
  rule); a posting without an amount, a comment line and Beancount's
  metadata left alone; characters counted, not bytes; the edits the
  gaps only, one undo step named "Align Amounts". `amount_column` 52
  or 0 as planned. `align_on_enter`: `pta.newline`, bound to Enter in
  a journal outside Vim's command mode, aligns the cursor's posting
  (the transaction's column) and runs Kalem's `edit.newline`; Kalem
  put the profile's Enter before a plugin's in a journal, so it ran
  only once Kalem gave a plugin's keys in its languages' files the
  place its keys in its own documents have (K10, built on Kalem's
  branch `language-keys`; checked: Enter aligns, then the new line
  indented). Open: Vim's insert mode, whose layer takes Enter in a
  plain text file before any key (K10's other half); `SPC m a` from
  command mode aligns there. Tests: 10 unit tests of the anchors and
  the cases (quoted commodity, cost, lot, assertion, comment, a tab,
  the column too near, the cursor's transaction only, a selection,
  Beancount's metadata, wide characters, Enter's line only), and
  `tests/align.rs`: every corpus journal aligned whole at 52 and at 0
  changes only gaps, is unchanged by a second alignment, and reads as
  before to its tool (`hledger print`, `ledger register`, `bean-query`;
  an error file's error the same).)

## PT5. Checked by the tool

- [x] PT5 `pta.check` (`SPC m k`; the status bar item's click; on save
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
  (Done 2026-10-11, checked in Kalem's terminal editor: Check Journal
  (`SPC m k`, the palette, the status bar item's click, `r` in the
  document) on the main journal of PT3c; the document "Check:
  errors.journal" with the tool's check line, each problem's place
  (beside the main journal by its name), its message and the lines
  the tool quoted, muted; Enter on a problem opens its file at its
  line and column (`file.open`'s column a byte from 0: the tools'
  less one, exact on an ASCII line), `q` and Escape close it; on save
  (`check_on_save`) the item gets `✓` or `1 problem`, and an open
  document is written again; a tool missing: one notice with its
  install line, once a session on save, each time asked; Kalem's
  refusal (no project) said when asked. `src/check.rs`: the commands
  (`hledger … check --strict` and the setting `checks`, names only;
  `ledger … --pedantic --explicit balance`; `bean-check`), each tool's
  error form parsed from its standard error (hledger's one problem,
  its quoted lines, its message; Ledger's every `While parsing file
  …`/`Error:`; bean-check's every `FILE:LINE: message`, sorted by
  line), and the report. Measured: hledger stops at its first
  problem, Ledger and bean-check report them all (bean-check not in
  line order). Tests: unit tests on recorded output of each tool (one
  problem, two, none, a failure without a place, one in no known
  form) and the report; the conformance test runs each tool's check as
  the plugin does on the corpus: each error file one problem at its
  line (hledger 13, Ledger 9, Beancount 7), each clean journal none.
  K2 stands: `diagnostics` is a plugin's panic report, and
  `decorations` marks only a diff's lines.)

## PT6. Writing transactions

- [x] PT6a `pta.newTransaction` (`SPC m t`; ledger-mode's `C-c C-a`):
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
  (Done 2026-10-11, checked in Kalem's terminal editor: the date asked
  with today's offered (the clock's local day) and hledger's smart
  dates taken (`yesterday`, `-3`, `5`, `10/5`, a weekday, `last` and
  `next` ones), resolved by `src/write.rs`; the payees the journal's
  text names, the last written first, then the tool's (`hledger
  payees`, `ledger payees`, `bean-query`'s `SELECT DISTINCT payee`),
  and "Another payee…" asked as text; the draft `hledger print
  --match`, `ledger xact` or `bean-query`'s `PRINT FROM payee = …`
  (its last entry), only for a payee of the journal, since hledger and
  Ledger match loosely and draft from another's otherwise; the draft
  dated, without its code, status (Beancount's flag kept), second date
  and balance assertions; written at the date's place (`insert_at`)
  by the plugin's own `pta.insertTransaction`, queued, since a
  question's answer comes when no command runs; the cursor on its
  first amount through `file.open`.)
- [x] PT6b `pta.toggleStatus` (`SPC m c`; ledger-mode's `C-c C-c`):
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
  (Done 2026-10-11, checked in the terminal editor. Changed from the
  plan: `toggle_postings` off by default (hledger's users mark
  transactions); the selection's transactions all to the first one's
  next; Alt+Down and Alt+Up as planned (free in a plain text file);
  the date a day later or earlier on `ctrl+shift+l` and `ctrl+shift+h`
  in Vim's command mode, Doom's keys for an Org timestamp, not
  Shift+Up and Shift+Down, which extend the selection, and the date of
  the transaction at the cursor, not only with the cursor on it; Sort
  Transactions keeps the cursor in its transaction, refuses across
  `apply`, `end`, `alias`, `Y`, `D`, `decimal-mark`, `include`,
  `bucket`, `fixed`, comment blocks, and Beancount's `pushtag`,
  `pushmeta`, `include` and `option`, and takes a last line without
  its line feed. Tests: 10 unit tests (dates and their arithmetic,
  smart dates, finding transactions and Beancount's directives apart,
  toggling a transaction, a posting and a selection, Beancount's
  `txn`, moving, copying, placing, sorting with comments and a
  refusal, drafts of the three tools, payees).)
- [x] PT6c The outline (Kalem's outline and sidebar): the server's
  document symbols, which both servers give (transactions by date and
  description, directives), checked in both editors; nothing of the
  plugin's own until a plugin can give an outline.
  (Done: both hledger-lsp programs and beancount-language-server give
  document symbols, measured through `kalem lsp ask symbols` in PT3:
  ptimoney's lists transactions and their postings, juev's groups the
  transactions by month, Beancount's lists entries and postings.)

## PT7. The balance and the register as documents

- [~] PT7a `pta.balance` (`SPC m b`): the balance sheet of the main
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
  (Done 2026-10-11 but `kalem run`'s printing, the depth on `SPC m p`,
  and the negative sections' colors as planned; checked in the
  terminal editor. The tool computes every amount: hledger's `balance
  --tree --no-elide --format '%2(depth_spacer)%(account)|%(total)'`
  (the journal's own amount style kept, full names from the indents),
  Ledger's `balance --balance-format '%(account)|%(scrub(display_total))'`
  (a parent with one child one line, `Budget:Travel`, as Ledger shows
  it), Beancount's `bean-query` at each level of the tree
  (`root(account, n)`, until a level adds none) and one for the total;
  `src/report.rs` reads them, `layout` draws the tree. Two sections,
  not `balancesheet` and `incomestatement`: assets, liabilities and
  equity (hledger's `type:ALE`, Ledger's and Beancount's root names) at
  the period's end, the others (income, expenses, budgets) within it,
  each with the tool's total. Tab, Right and Left fold and unfold (the
  folded kept by account across refreshes; Left on a leaf folds its
  parent), Enter opens the Register, `p` the period (this month, last
  month, this quarter, this year, last year, all dates), `d` the depth,
  `r` again, `q` and Escape close, `?` the pick; written again on the
  journal's save. Changed: the period and depth keys are `p` and `d` in
  the document, not `SPC m p`; a typed period waits (Ledger and
  Beancount would need it parsed). Styled: the header, the sections and
  totals bold, marks muted, amounts below zero red. Tests: unit tests
  of each tool's rows, the tree, the folds, the periods, the commands;
  `tests/reports.rs` runs each tool on the corpus (a subtotal, a
  section's accounts, a period's change against the balance at its
  end).)
- [~] PT7b `pta.register` (`SPC m R`; Enter on an account in PT7a; the
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
  (Done 2026-10-11 but the typed query (`SPC m /`); checked in the
  terminal editor: from the Balance's Enter, `SPC m R` on a posting's
  account (`write::account_at`), or an account picked from the tool's
  (`hledger accounts`, `ledger accounts`, `bean-query`'s); the account
  with its subaccounts (`^ACCOUNT(:|$)`); hledger's `register -O csv`
  (its CSV writes amounts without digit groups and no status), Ledger's
  `register --register-format` (fields apart by U+001F, a payee may
  hold `|`; its state, file and line), `bean-query`'s `date, flag,
  payee, narration, position, balance, filename, lineno`; the last
  2,000 shown, the earlier counted. Enter opens the journal at the
  posting: Ledger's and Beancount's place, hledger's transaction found
  in `hledger print -O json` (`tsourcepos`) when asked. `p` the period,
  `r` again, `q` closes. Tests: the three parsers and the layout, and
  each tool's register on the corpus, its first running total and the
  first posting's place.)
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

- [~] PT8a `pta.reconcile` (`SPC m r`; ledger-mode's `C-c C-r`), in a
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
  (Done 2026-10-11 but the pane beside, the refresh 500 ms after a
  change, one block per commodity; checked in the terminal editor on
  the corpus. The tool totals: hledger's `balance` with `status:*`,
  `status:!` and both (`-O json`), Ledger's `--cleared`, `--pending`
  and `--limit 'cleared|pending'`, Beancount's flags (`!` pending,
  anything else cleared: Beancount has no unmarked posting); the
  postings not cleared from hledger's `register status:! status: -O
  json`, Ledger's `--uncleared`, Beancount's `flag = '!'`. The plugin
  takes the tool's cleared-and-pending total from the target typed in
  the journal's style (the tool's total offered), exact decimals
  (`src/reconcile.rs`); hledger's amounts read from its JSON and
  written in their style, which the Register now uses too (digit
  groups, the status). The document: the five figures, the difference
  green at zero and red otherwise, the postings not cleared; Enter
  opens the journal at one, `t` the target, `C` finish, `r` again, `q`
  closes; written again when the journal is saved. Found: Ledger's
  query language takes parentheses for grouping, so an account and its
  subaccounts are two terms, `^A$ or ^A:` (the Register's Ledger
  filter was wrong before, its test now checks the last total). The
  account picked: the cursor's posting's, else among the asset and
  liability accounts. Opened as a document, not yet in a pane beside
  the journal; the commodity the target names, or the first.)
- [~] PT8b `pta.finishReconcile` (`SPC m C`; ledger-mode's `C-c C-c` in
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
  (Done 2026-10-11 but Space: Finish asks the statement's date (today
  offered, smart dates taken), opens the journal the reconciliation
  started in and runs `pta.finishReconcileWrite` there, queued, since
  a command edits only the document it runs in (K3): the account's
  pending marks (a transaction's or a posting's) made `*`, and the
  assertion written at its date's place, hledger's and Ledger's
  `DATE * Balance assertion` with `ACCOUNT  0,00 TRY = TARGET`, the
  zero and the target in the commodity's style, Beancount's `balance`
  the next day; one undo step. Checked: the corpus's pending Market
  transaction cleared, the assertion written, `hledger check
  --strict` passing. Pending marks in an included file are not
  cleared (only the open journal is edited).)
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

- [x] PT9a hledger: `pta.import` (`SPC m i`), in a CSV document: the
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
  (Done 2026-10-11, checked in the terminal editor on the corpus's
  `statement.csv`: `SPC m i` in a CSV statement (or Import Statement in
  the palette) shows `hledger import --dry-run` as "Import:
  statement.csv", in hledger's syntax and highlighting, with a comment
  header (the count; `i` import, `e` the rules, `r` again, `q`); `i`
  runs the import, which hledger writes and remembers, the journal
  then opened at the first transaction imported (the last of `hledger
  print -O json`). The journal: the setting `journal` (beside the
  statement when relative), else `LEDGER_FILE`'s (`hledger files`),
  else the journal last opened in the session. A statement without
  rules (hledger's error read): offered rules from a template of its
  header row (`src/import.rs`: the usual names in English, Turkish,
  German, Spanish, French; the separator, the date format and the
  decimal mark guessed from the first data row; `account1` picked
  among the journal's asset accounts), written in the new rules file
  by the plugin's own command, queued; a test has hledger import the
  statement through them. Changed: the plugin starts with any
  document (`activation` `onEvent:document:open`), since a CSV file is
  not one it claims and claiming CSV would suggest it for every CSV.
  Not done: the yellow of the postings to `expenses:unknown` (the
  document is highlighted as hledger's journal).)
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
  (Waits on the owner: `subprocess:python3` is the open question
  below; until then Import in a Beancount journal's statement says so.)
- [x] PT9c Ledger: `ledger convert FILE` with the account and the input
  date format from settings, through PT9a's document, appended as PT9b
  appends.
  (Done 2026-10-11: the settings `ledger_account` and
  `ledger_date_format`; Ledger's conversion shown in the Import
  document, `i` appending it at the end of the journal, opened and
  edited by the plugin's own command, queued: one undo step. Ledger's
  `convert` reads a header of its own names (`date`, `payee`,
  `amount`…); a test converts such a statement.)

## PT10. Prices and values

- [x] PT10 `pta.prices` (`SPC m $`): `bean-price` for Beancount
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
  (Done 2026-10-11, checked in the terminal editor: Fetch Prices (`SPC
  m p`, which-key's row reading "p → Fetch Prices", rather than `SPC m
  $`) in a Ledger copy of the corpus fetched EUR/USD from the ECB
  through `pricehist` after the journal's last price (Feb 1) and
  appended the `P` lines as the journal writes them (`P 2026/10/09 EUR
  $1.1206`), one undo step; again, "No new prices". hledger and Ledger
  fetch the pairs of the setting `price_sources` (`ecb EUR/USD`, a
  third word for the journal's symbol of the quote: `ecb EUR/USD $`),
  each after its last price as `hledger prices` or `ledger prices`
  lists them (`-sx`), a pair without one from 30 days back, with the
  journal's date separator and decimal mark (hledger's `decimal-mark
  ,`); Beancount runs `bean-price --update` on the main journal, which
  reads its commodities' `price:` metadata. Written at the end of the
  setting `prices` (beside the main journal when relative: a file the
  journal includes) or the main journal, opened for it and written by
  the plugin's own command `pta.append`, which Ledger's import now
  shares. A program missing, refused or failing gives one notice
  (`pip install pricehist`, `pip install beanprice`, the settings
  `programs.pricehist` and `programs.bean-price`). Market value: `v` in
  the Balance and the Register, the header then saying "market value":
  hledger `-V --infer-market-prices` (its costs as prices too), Ledger
  `-V`, Beancount's queries through `value()`; tests value the corpus
  with each tool (Ledger's EUR expenses at $1.12, hledger's cash at
  its costs, Beancount's fund at its last price) and have Ledger and
  hledger read prices written the plugin's way, the network left out.
  Not done: `--value=then` (the value at each posting's date) as a
  third state of `v`.)

## PT11. The rest of Kalem

- [~] PT11a A **Journal** menu in the menu bar and the F10 list while
  the current file is a journal or one of the plugin's documents
  (`when` `textType == hledger || textType == ledger || textType ==
  beancount || textType == pta-…`): New transaction, Toggle status,
  Align, Sort; Check; Balance, Register, Reconcile, Report…; Import,
  Edit rules; Prices; the tool and the journal (PT3c's settings
  through a quick-pick); the leader table's `SPC m` rows named for
  which-key, as the git plugin's `+git` rows are. A toolbar button
  "Journal" beside Git, opening the balance.
  (Done 2026-10-11 at the owner's asking, after the git plugin's Git
  menu and button and the graph plugin's: the manifest's `menus` (the
  menu bar's and F10's "Journal", the journal's commands in
  `kalem_plugin_pta::MENU`'s order with its separators, then
  Commands…) and `buttons` ("Journal", the window's toolbar and the
  foot of the terminal's sidebar), both where a journal or the Check
  document is; the button runs `pta.menu` (Commands…, `SPC m m`, `?` in
  the Check document), a pick of the journal's commands with their
  keys, run in the journal once chosen, since a button runs a command
  and most of the journal's act at the cursor (the graph plugin's menu
  document suits its commands, which act on no note). Checked in the
  terminal editor: F10 lists "Journal › …" with the keys, `SPC m m`'s
  pick runs Toggle Status on the journal. The which-key group under
  `SPC m` is named by Kalem from the commands' category. Open: the
  tool and the journal through a pick (the settings page has them);
  the window's toolbar not looked at.)
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

## PT12. An engine inside: not built (the owner, 2026-10-11)

- [x] PT12 No Rust engine for the journals inside the component: the
  tool stays the reference implementation, as git is the git
  plugin's, in D55's spirit; the plugin answers only where hledger,
  Ledger or Beancount is installed, and says how to install it where
  it is not. (Decided 2026-10-11: weighed was a parser, balancing, the
  register, the balance report and the checks in Rust, and
  rustledger for Beancount, so that the plugin answered with no tool
  installed; the owner chose the tool. Its one argument kept for
  later: the reconcile document's refresh 500 ms after a change
  (PT8a), if a process round trip proves too slow there, is a
  question for PT13's measurements, not a reason for an engine.)

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

- [~] PT14 The README: what it does, the feature table per dialect
  (what the server gives, what the tool gives, what the plugin
  itself does), the install lines (hledger, Ledger, Beancount and
  Python's `beancount`, the two servers, `bean-price`, `pricehist`),
  the keys under `SPC m`, the settings, the known differences from
  ledger-mode (what its reconcile does that PT8 does not yet, K3) and
  from Fava (charts, the browser editor); a line in `CODEOWNERS`; the
  entry in `index.json`; the tag `pta-v0.1.0` once PT1 to PT8 are
  done; the Book's Part IV page for the journal formats (D53), which
  waits on Kalem.
  (0.1.0 prepared 2026-10-11, the owner deciding that it goes out with
  what is done rather than after all of PT1 to PT8: version 0.1.0 in
  `plugin.json` and `Cargo.toml`; `api` `^0.2.13`, what the component
  imports with the workspace's kalem-plugin (every interface at 0.2.13,
  read with `wasm-tools component wit`), for Kalem 0.6.16 and later
  (the servers' options and the plugin's keys in its languages' files,
  K8 to K10a); the description says what the plugin does. The README
  for the release: install lines (the plugin, each tool, the servers,
  `pricehist` and `beanprice`), what each dialect gets and from whom
  (server, tool, plugin), the keys under `SPC m` and in the documents,
  the settings, what waits on Kalem (K3's marking from the
  reconciliation, the statement in a pane beside) and what is not done
  (PT7c, PT8c, PT9b, PT11b, PT11c, PT13), and the differences from
  ledger-mode and Fava. `CODEOWNERS` had its line since PT1;
  `index.json` regenerated (download empty until the tag). Left: the
  tag `pta-v0.1.0` and its release, the git session's; the Book's page,
  which waits on Kalem.)

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
  journal whose server is missing. The completer came with Kalem 0.6.13
  (plugin API 0.2.11, a component's `completer` export); the outline
  provider has not.
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
- K8 A server's `initializationOptions` from the user's settings: the
  user's `servers.KEY.initializationOptions` merged over the
  manifest's, as `settings` and `servers.KEY.command` already are,
  and shown in the plugin's page; a plugin's setting then describes
  it (`servers.beancount-language-server.initializationOptions.journal_file`).
  beancount-language-server reads its main journal and its formatting
  widths only there (PT3b); any server configured only at `initialize`
  needs it. Built 2026-10-11 on Kalem's branch `server-options`
  (3e5c45e1, from Kalem 0.6.15), unreleased.
- K9 A server's feature turned off: a list of the server's
  capabilities Kalem does not use, in the manifest's server
  (`"ignore": ["documentFormattingProvider"]`) and the user's
  `servers.KEY.ignore`; and `formatters.TYPE = "off"` stopping a
  server's formatting too. Kalem formats with a server whenever it
  offers formatting, and ptimoney's hledger-lsp 0.7.0 changes what a
  journal means when it does (PT3a): Format Document would then take
  the plugin's formatter (PT4) or say there is none. Built with K8
  (3e5c45e1): the manifest's `ignore`, the user's in its place (`[]`
  for all used, so that juev's formatting can be had back), and
  `formatters.TYPE = "off"` stopping every formatter; unreleased.
- K10 A plugin's keys in the files of a language it declares
  (`textType == journal`) before the profile's, as its keys in its own
  documents are (PT4's Enter): built on Kalem's branch `language-keys`
  (05177b87), unreleased. Its other half, open: Vim's insert mode
  leaving a key to a plugin's binding where one applies; its layer
  takes Enter in a plain text file before the keys
  (`kalem-core/src/vim/insert.rs`), so ledger-mode's align on Enter
  is not had there.
- K11 A language plugin's `commands` per language, or a component's
  command as a language's formatter: one `commands.format` serves all
  of a plugin's languages (`bean-format` would run on an hledger
  journal), and Align Amounts cannot be Format Document's in a journal.

## Open for the owner

- The name: `pta` (the family) against `ledger` (one tool's name, and
  the Emacs mode's) or `journal`; the id and the letters follow.
- Which dialect first: hledger is proposed (the widest gap, the
  fastest tool, the server with the most to show); the owner may want
  Beancount first for its stricter format and Fava's users.
- `subprocess:python3` for the Beancount importers (PT9b): a broad
  program to grant; the alternative waits on a permission that names
  a script.
- `.dat` and `.j` claimed or not; hledger's `timeclock` and `timedot`
  as later languages.
- The roadmap item (R5.17 proposed). The plugin is installed from
  `index.json` as the git plugin is, never built in (the owner's rule
  of 2026-10-10: Kalem builds in Org, Markdown, CSV and LaTeX only).
