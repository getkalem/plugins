# graph: Logseq graphs and Obsidian vaults opened as themselves

The list for the `graph` plugin of getkalem/plugins (`plugins/graph`,
crate `kalem-plugin-graph`, id `org.kalem.graph`): a folder of notes
that Logseq or Obsidian keeps, opened in Kalem as the graph it is, with
its pages, journals, links, block references, embeds, tags and tasks,
nothing converted and nothing of Kalem's written into it. Written
2026-10-10 before the folder existed, at the repository's root, and
moved here with the crate the same day; each task says what was done and
what is open.
It rests on Kalem's decisions D29 (a small core: Markdown and Org stay
in it, every other mode is a plugin), D25 and §11.11 (the contract a
built-in mode uses is the contract a plugin mode uses), D55's rule
carried to text (a note is written as its own application writes it),
and on Kalem's task T2.7c.12 (an Obsidian vault opens as itself), whose
Obsidian half this plugin takes over: the owner decided on 2026-10-10
that Obsidian's constructs (callouts, embeds, block ids, aliases) live
in the plugin, not in the core's Markdown mode, which keeps CommonMark,
GFM and the wiki links of T2.7c.9 (D29). The letters are GR. A roadmap
item waits on the owner: R5.5 and T2.7c.12 re-pointed at this plugin,
and T3.1.9g, the mode binding, brought forward as its gate.

The state this list starts from was read from Kalem's code and Book on
2026-10-10, not from the roadmap's checkboxes: releases v0.6.2 to
v0.6.6 are tagged; every built-in mode (Org, Markdown, CSV, LaTeX) is on
the mode contract and both editors draw every mode through
`kalem_core::mode_view` (R3.1 and R3.2, done 2026-10-04); the Markdown
mode parses `[[Page|title]]` wiki links, hides their brackets away from
the cursor, resolves them in the folder, then anywhere in the project
ignoring case, offers the project's Markdown files after `[[`, folds
front matter as a drawer and edits it as Obsidian's properties panel
does; Search Notes (`SPC n s`) searches a notes folder; the plugin API
is 0.2.8 with `documents`, `styled-documents`, `process`, `decorations`
(gutter marks), `ui` (panels of trees, texts, buttons), `settings`,
`fs` (read, write and list inside the projects, by permission),
`editor` (the current document read and edited) and the events
`document-open`, `document-changed`, `document-after-save` and
`workspace-file-changed`. What is not there: a mode registered by a
plugin (`modes.wit` does not exist; the Book: "still to come, the
plugin side, `kalem::modes::register`, T3.1.9g"), completers or link
types from plugins, backlinks anywhere in the core, callouts, embeds,
block anchors, aliases, virtual text, a consent prompt for permissions.

## Why this plugin, and why now

Logseq split on 2026-04-24: the file-based application became "Logseq
OG", in maintenance (security and Electron updates, no features), and
the database application took the name; its 2.0 beta of July keeps the
notes in SQLite, mirrors them to Markdown one way, and dropped Org
files altogether. The people who chose Logseq for local plain files,
many of them on Org, sit in an application that will not change again,
and nothing opens their folder in place: Obsidian imports and rewrites,
SilverBullet has no blocks, no community fork has appeared (checked
2026-10-10). Obsidian's own users ask for the same thing from the other
side: a native or terminal editor for the vault they have (obsidian.nvim
and its fork, basalt, clin and Emeraldian in 2026 alone). Both groups
keep daily notes, link them, and live in their files every day; a user
who writes today's journal in Kalem makes Kalem the editor they open
first. That is the loyalty the plain-text accounting plugin
(`pta_todo.md`) cannot offer on its own, and the reason this plugin
comes first.

The files are Kalem's own formats. A Logseq graph is Markdown or Org
in `pages/` and `journals/` with a `logseq/config.edn`; an Obsidian
vault is Markdown with a `.obsidian/` folder. What the plugin adds is
the graph layer: every bullet a block, properties written as `key::
value` lines (two of them, `id::` and `collapsed::`, hidden by the
application), block references `((uuid))`, page and block embeds, tags
that are pages, journals named by date, TODO and SCHEDULED in Org's
spelling inside Markdown, callouts, aliases, and an index of all of it
across the folder so that a page shows what links to it.

## The shape: a component today, a mode when Kalem binds modes

The plugin is one component in two layers, and the order of the list
follows what the API allows when.

The **first layer needs nothing new** (GR1 to GR3): the graph found by
its marker, the index built from the files through `fs`, a backlinks
panel through `ui`, the navigation commands (follow the reference at
the cursor, today's journal, find a page, insert a link) through
`editor` and `file.open`, and the read-only documents (backlinks, all
pages, journals, tags, tasks) through `documents`, as the git plugin
writes its status. The notes themselves still open in the core's
Markdown or Org mode: wiki links already hide their brackets, but
`id::` lines and `((uuid))` stay visible. Useful and shippable, not yet
a replacement.

The **second layer is the mode** (GR5 to GR8): the graph's Markdown and
Org drawn as Logseq and Obsidian draw them. A plugin cannot register a
mode today; GR4 specifies the binding, Kalem's T3.1.9g, as work in
Kalem's repository (a sibling worktree on a branch, never the shared
`main`), written for every later mode plugin (Typst, R5.10, waits on
the same binding), and with one general addition this plugin needs:
**a mode that layers on a core mode**. The graph mode does not reparse
Markdown; it asks the host for the Markdown (or Org) tree of the text
and adds and changes nodes: a property line becomes a hidden marker, a
`((uuid))` a link, a `{{embed}}` an embed. Obsidian-flavoured,
Logseq-flavoured, Pandoc-, MyST- and Quarto-flavoured Markdown are all
"Markdown plus", and this is how each of them becomes a plugin without
a second parser and without breaking the CommonMark suites the core
passes.

A task is done when it works in both editors, leaves every file byte
for byte as it was unless the user edited it (a save without edits is
identical; an edit rewrites the lines it touches; `logseq/` is read for
`config.edn` and never written; of `.obsidian/` only `daily-notes.json`,
`app.json` and `templates.json` are read, read-only, and nothing under
it is ever written (owner, 2026-10-10); nothing is written that Logseq
or Obsidian would not write themselves),
undoes in one step, and has its tests: the conformance test on three
corpus graphs (a Logseq Markdown graph, a Logseq Org graph, an Obsidian
vault), the index's counts and every resolution rule checked, every
corpus file through the mode's byte-exact round trip and `modes::check`
once the mode exists, snapshots in both editors, and the documents
printed under `kalem run`. Every command is in the palette, its keys
can be bound again. The keys are Doom's `SPC n r` map, which Kalem's
leader table lists as "Org Roam is not part of Kalem": the plugin takes
it as org-roam has it (`r` the backlinks panel, org-roam's buffer
toggle, `R` the backlinks as a document, `f` find a page, `i` insert a
link, `s` index again, org-roam's sync, `g` the graph, `d t` today's
journal, `d y` yesterday, `d m` tomorrow, `d d` a date, `d f` and `d b`
the next and previous journal) with the plugin's own beside them (`o`
follow the reference at the cursor, `b` insert a block reference, `p`
all pages, `j` journals, `t` tasks, `T` tags), and in a graph's
document its own `SPC m` keys for the outliner (GR8). The Word-like
profile gets a **Graph** menu with the same commands.

## GR1. The spike: a graph recognized, its index built

- [~] GR1 `plugins/graph` from `template/`: `Cargo.toml`
  (`kalem-plugin-graph`; `kalem-plugin` with the `extension` world; a
  small EDN reader, written in the crate or a dependency without
  `std::fs`, for `config.edn`), `src/lib.rs`, `tests/conformance.rs`;
  the manifest: `id` `org.kalem.graph`, `name` "Note graphs", `main`
  `dist/graph.wasm`, `api` `^0.2.8`, `activation` `onStartup`,
  `permissions` `fs:read:workspace` (the index reads every note;
  `fs:write:workspace` arrives with GR8, decided by the owner on
  2026-10-10 with GR8c's safety rules). On `app-ready` and when a
  project is added, the plugin
  looks at each project folder of the workspace: `logseq/config.edn`
  makes it a Logseq graph, `.obsidian/` an Obsidian vault, the
  setting `graphs` (a list of folders with a kind) forces either; a
  folder that is both is Logseq's (the owner may want the reverse). A
  graph's root, kind, format (`:preferred-format` of `config.edn`:
  `:markdown` or `:org`; Obsidian is Markdown) and its folders
  (`:pages-directory`, `:journals-directory`, defaults `pages` and
  `journals`; Obsidian's are the whole vault, journals where
  `.obsidian/daily-notes.json` says, else the setting
  `obsidian.daily_notes`) are kept per project. A status
  bar item says `⌬ my-notes · 1,240 pages` and a click opens the
  "All pages" document (GR3). Nothing else happens until a command
  runs. The corpus, written for this repository under CC0 (a vault
  under a licence that allows it may join later, as T2.7c.12 asks):
  `corpus/logseq-md/` (a `config.edn` with the defaults and
  `:preferred-format :markdown`; `pages/` with properties in a first
  block, `title::` overriding a file name, `alias::`, `tags::`,
  namespaces (`a/b` as `a___b.md`, and one legacy `a.b.md`), blocks
  with `id::` and `collapsed::`, `((uuid))` references with and
  without labels, `{{embed ((uuid))}}` and `{{embed [[page]]}}`,
  `#tag` and `#[[two words]]`, TODO, DOING, LATER, NOW, DONE, WAITING
  and CANCELED blocks with `[#A]` priorities, `SCHEDULED:` and
  `DEADLINE:` lines with a repeater, a `:LOGBOOK:` with `CLOCK:`
  lines, `#+BEGIN_QUOTE` and `#+BEGIN_NOTE` admonitions, a numbered
  list by `logseq.order-list-type:: number`, a heading in a block (`-
  # Title`), a `{{query (and [[page]] (task TODO))}}`, a `template::`
  block; `journals/2026_10_0N.md` for ten days, one referencing
  another by its title `[[Oct 3rd, 2026]]`; `assets/` with a picture
  linked `![](../assets/pic.png)`; `logseq/bak/` and
  `logseq/pages-metadata.edn` present and ignored),
  `corpus/logseq-org/` (`:preferred-format :org`: the same graph in
  Org, headings as blocks, `:PROPERTIES:` drawers with `:id:`,
  `#+title:`, `#+tags:` and `#+alias:` first lines, `[[page]]` and
  `[[file:./x.org][label]]` links), `corpus/obsidian/` (a `.obsidian/`
  folder with files the plugin never reads; notes with `[[Page]]`,
  `[[Page|alias]]`, `[[Page#Heading]]`, `[[Page#^block]]`, a `^block`
  id at a paragraph's end, `![[Page]]`, `![[Page#Heading]]`,
  `![[pic.png|300]]`, callouts of several types with `+` and `-`,
  front matter with `aliases:` and `tags:`, `#tag` and `#nested/tag`,
  `==highlight==`, `%%comment%%`, Dataview's `key:: value` inline
  fields, two notes of the same name in different folders for the
  shortest-path rule, daily notes in `Daily/2026-10-0N.md`, a
  `.canvas` file). Done when the status bar item names each corpus
  graph with the right page count, and the conformance test checks
  the manifest and, for each corpus, the kind, format and folders
  found and the counts of pages, blocks, references and journals the
  index (GR2) reports.
  (Done 2026-10-10: the crate (`src/` with `config`, `date`, `edn`,
  `files`, `names`, `scan`, `index`, `template`, `views`, `content`,
  `app` and the component), the manifest with seven settings and a Graph
  menu, the three corpora (41 files: `logseq-md` with ten journals, a
  namespace, a percent-encoded file name, the backup, a hidden folder
  and assets that are never read; `logseq-org`; `obsidian` with two
  notes of one name, a trash, a canvas and the three settings files), a
  line in `CODEOWNERS` and the entry in `index.json`, no download until
  a tag. Three changes from the plan. A plugin cannot list Kalem's
  projects, so a graph is found when a note is opened, from its folder
  upwards, and by the setting `graphs`, not on `app-ready` (K10). Kalem
  lets a plugin read only its projects' folders, so the graph's folder
  must be a project; the README says so. `fs:write:workspace` comes now,
  not with GR8, for one write: a new journal made from its template when
  no file is there. The corpus has no legacy `a.b.md` name, the graph
  being `:triple-lowbar`; the legacy reading has its unit test. Checked
  in Kalem 0.6.6 through a pseudo-terminal with its own configuration
  folder and the corpus as a project: the status bar shows `⌬ logseq-md
  · 17 pages`, the Backlinks document lists Kalem's five linked pages,
  its blocks' two references and two unlinked mentions, Enter on a line
  opens the journal at that line, the Backlinks panel fills, and on a
  copy of the vault Today's Journal asks the date and writes
  `Daily/2026-10-11.md` from the vault's template. Open: the graphical
  editor, by hand.)

## GR2. The index

- [x] GR2a The scanner. Not a Markdown parser: a line scanner that
  finds what the graph layer needs and nothing more, so that a
  10,000-page graph indexes in a second. For Logseq Markdown: a block
  starts at a line whose first non-blank is `- ` (tabs or spaces, the
  file's own); its depth is its indentation; its continuation lines
  are the indented lines after it until the next bullet; its
  properties are the `key:: value` lines right after its first line
  (values split at commas into page references when the key is
  `alias`, `tags` or one of `:property/separated-by-commas`); the
  page's properties are the first block when it holds only properties
  (Logseq's pre-block) or a `---` front matter; `id::` is the block's
  UUID, `collapsed::` its fold, `heading::` or `- # ` its heading; the
  first line's keyword (`TODO`, `DOING`, `DONE`, `LATER`, `NOW`,
  `WAITING`, `WAIT`, `CANCELED`, `CANCELLED`, `IN-PROGRESS`), priority
  (`[#A]` to `[#C]`), `SCHEDULED:` and `DEADLINE:` lines with their
  Org timestamps and repeaters, the `:LOGBOOK:` drawer's range;
  references in the text: `[[page]]`, `[label]([[page]])`, `#tag`,
  `#[[tag]]`, `((uuid))`, `[label](((uuid)))`, `{{embed ((uuid))}}`,
  `{{embed [[page]]}}`, `{{query …}}` (kept as text for GR9), and the
  references inside property values. For Logseq Org: a heading is a
  block, its `:PROPERTIES:` drawer's `:id:` its UUID, `#+title:`,
  `#+alias:`, `#+tags:` and `#+property:` lines the page's properties,
  Org's own keywords, priorities, planning lines and `LOGBOOK`;
  `[[page]]`, `[[page][label]]`, `[[file:…]]` and `((uuid))`. For
  Obsidian: paragraphs and list items as blocks, a `^id` at a line's
  end as the block's id, front matter's `aliases:`, `tags:` and other
  keys as the page's properties, `key:: value` inline fields as block
  properties, `[[Page]]`, `[[Page|alias]]`, `[[Page#Heading]]`,
  `[[Page#^block]]`, `![[…]]` embeds, `#tag` and `#nested/tag`,
  `%%…%%` comments skipped. Headings and their ranges in every format,
  for `#Heading` targets and the outline. Unit tests per construct on
  small texts; the corpus's counts in the conformance test.
  (Done 2026-10-10, `src/scan.rs`: the three flavours, line by line,
  with every reference's line and columns and its block. Beyond the
  plan: references inside a page property's value at their columns
  (`type:: [[Program]]`), `[[a [[b]]]]` naming both, `(((uuid)))`, a
  fence opened on a bullet's own line, a `#+BEGIN_NOTE` block shown by
  its first line of content, Obsidian's aliases left out of the
  references (they link nowhere in Obsidian), Org's headline tags left
  out of the title. Fourteen unit tests and the corpus's.)

- [x] GR2b Pages and their names. A page is a file under the pages
  folder, a journal, or a name only referenced (Logseq's tag and
  reference pages exist without a file until written). Logseq's
  rules: titles compare ignoring case; the file name encodes the
  title by `:file/name-format` (`:triple-lowbar`: `/` as `___`,
  other characters URL-encoded; the legacy format: `.` and `%2F` for
  `/`, read but never written); `title::` in the pre-block wins over
  the file name; `alias::` names resolve to the page; a namespace
  `a/b/c` lists under `a/b`. Obsidian's rules: a bare `[[Name]]`
  resolves to the one file called `Name.md` anywhere in the vault,
  and when several exist, to the one whose path the link gives
  (shortest path, Obsidian's default); `aliases:` in front matter
  resolve; `[[Name#Heading]]` to the heading, `[[Name#^id]]` to the
  block. Journals: Logseq's file name from `:journal/file-name-format`
  (default `yyyy_MM_dd`) and title from `:journal/page-title-format`
  (default `MMM do, yyyy`), so `[[Oct 3rd, 2026]]` is a journal
  reference; Obsidian's from `.obsidian/daily-notes.json` (`folder`,
  `format` in Moment's syntax, `template`), read read-only, else the
  plugin's settings `obsidian.daily_notes` and `obsidian.daily_format`,
  else Obsidian's own defaults (the vault's root, `YYYY-MM-DD`); the
  same for `app.json` (`attachmentFolderPath`, `newFileLocation` and
  `newFileFolderPath`, `useMarkdownLinks`, `newLinkFormat`) and
  `templates.json` (`folder`): a vault works with no settings, a
  missing or unreadable file falls back to the settings and then the
  defaults, nothing under `.obsidian/` is ever written (owner,
  2026-10-10). Every rule is a test with a
  fixture in the corpus, including the case-only difference, the
  namespace, the legacy file name, the alias, the two same-named
  notes, and the journal reference by title.
  (Done 2026-10-10, `src/names.rs`, `src/date.rs` and `Index::resolve`:
  Logseq's titles, aliases and file names (also the file name's own
  title when `title::` differs), journals by both formats; Obsidian's
  path, the linking note's folder first and then the shortest path,
  never an alias; the date patterns in both dialects (ordinals, month
  and weekday names, quoted text) formatting and parsing. Every rule has
  a fixture in a corpus. Obsidian's daily notes' folder, format and
  template come from `.obsidian/daily-notes.json` as decided.)

- [~] GR2c The index itself and its upkeep. In memory, per graph:
  pages (title, path or none, aliases, namespace, properties, kind:
  page, journal with its date, or virtual), blocks (page, line range,
  depth, id, parent, first line, keyword, priority, scheduled and
  deadline dates, properties, collapsed), references (from block to
  page or block, kind: link, tag, embed, property, with the range in
  the file), headings. Built at the first command that needs it, and
  on `app-ready` when the setting `index_on_start` is on (the
  default), from `fs.list` and `fs.read` over the graph's folders,
  skipping `logseq/`, `.obsidian/`, `.trash/`, `.recycle/`, `assets/`
  (listed for pictures, not read), `:hidden` folders of `config.edn`,
  files over 16 MB (`fs.read`'s limit) with a notice; kept current by
  `document-after-save` (that file rescanned) and
  `workspace-file-changed` (the path rescanned or removed, debounced
  100 ms); the unsaved text of the current document is read through
  `editor.text` when a command runs there, so the panel and the
  commands see what the user typed. Memory kept under 300 MB for a
  million blocks (strings interned, ranges as `u32`); the time to
  build measured and written in the README. `graph.reindex` (`SPC n r
  r`) rebuilds. Tests: the counts; a save changing one file changes
  only its entries; a file removed removes its page and its outgoing
  references and keeps the incoming ones pointing at a virtual page.
  (Done 2026-10-10, `src/index.rs`: pages, files, names, block ids,
  backlinks, block backlinks, tags and tasks, built from the files and
  kept by `document-after-save` and `workspace-file-changed`; the maps
  are derived again after each file, which is simple and enough for the
  corpus; a page only referenced is kept as one. Open: the unsaved text
  of the current document (`Index::update_text` exists, not yet called).
  GR11 measured the large graph, made a save change only its file's
  entries, and lists the problems beyond the one notice in All pages.)

## GR3. What works without the mode

- [x] GR3a Follow and open. `graph.follow` (`Enter` on a reference in
  the Vim profile's normal mode, `SPC n r o`; Alt+Enter offers the
  choices): the reference at the cursor, found by the scanner's rules
  in the current line (`editor.text` of the line), opened with
  `file.open` at the line of the target: a page by its file, a page
  without a file created as the application creates it (Logseq:
  `pages/NAME.md` by the file-name format with no content, or the
  `default-templates` page template when `config.edn` names one;
  Obsidian: `Name.md` where `app.json`'s `newFileLocation` says, the
  vault's root, the current note's folder or `newFileFolderPath`, else
  the setting `obsidian.new_notes`, default the root), a `((uuid))` or
  `#^id` at its block's
  line, a `#Heading` at the heading, a tag at its page, an embed at
  what it embeds, a journal reference at the journal (created for a
  future or missing date, as Logseq does). The core's own Open Link
  (`markdown.openLink`) keeps handling plain `[[Page]]` wiki links as
  before; the plugin's command adds what the core does not know, and
  for `[[Page]]` applies the graph's resolution (aliases, `title::`,
  namespaces, case) before the core's, so that the two agree where
  they overlap. Tests on the corpus for every target kind.
  (Done 2026-10-10: `graph.follow` on `SPC n r o`, for pages, tags,
  block references, embeds, Obsidian's headings and block ids; a page
  not written yet opens as a new empty document at the path the graph
  would give it, written when saved; a journal from its template. The
  core's Open Link is left as it is.)

- [~] GR3b Journals. `graph.today` (`SPC n r d t`), `graph.yesterday`
  (`d y`), `graph.tomorrow` (`d m`), `graph.journalOn` (`d d`, a date
  prompt taking `2026-10-03`, `oct 3`, `-3`, `monday`): opens the
  journal for the date by the graph's naming, created when missing
  with the journal template of `config.edn`'s `:default-templates
  {:journals "name"}` (the template page's blocks copied, `<%today%>`
  and the other dynamic variables Logseq expands written as their
  values); `graph.nextJournal` and `graph.previousJournal` (`Alt+]`,
  `Alt+[` in a journal) move by date over existing journals. The
  "Journals" document (GR3d) is the stream. Tests: the created file's
  name and content; a template with variables; the Org format's
  journal.
  (Done 2026-10-10: the journal commands, the template of either
  application filled in and written only when no file is there, a date
  typed in the forms listed. Kalem gives an extension plugin no clock
  (the `extension` world imports no `clock`), so the first journal
  command of a session asks today's date, offering the newest journal's,
  and keeps it: K9 removes the question. Open: `Alt+[` and `Alt+]`,
  which need a context key for "in a journal"; the journals as `d n` and
  `d p` meanwhile.)

- [~] GR3c Find, insert, backlinks. `graph.findPage` (`SPC n r f`): a
  quick-pick of every page by title and alias, journals last, the
  filter as the user types, the first item "Create NAME" when nothing
  matches, opening or creating as GR3a does. `graph.insertLink` (`SPC
  n r i`): the same pick, inserting `[[Title]]` at the cursor through
  `editor.insert` (Obsidian: as `app.json`'s `newLinkFormat` and
  `useMarkdownLinks` say, shortest, relative or absolute, wiki or
  Markdown link, so that Obsidian leaves the link as written; else the
  setting `obsidian.link_style`; Org: `[[Title]]` or the `file:` form
  `:org-mode/insert-file-link?` asks for).
  `graph.insertBlockRef` (`SPC n r (`): a quick-pick over blocks'
  first lines across the graph, full-text filtered, inserting
  `((uuid))`; when the chosen block has no id, one is written into
  its file (GR8c; until then the pick offers only blocks that have
  one, and says so). The **backlinks panel** (`SPC n r b` toggles it;
  the `ui` panel `graph.backlinks`): for the current document's page,
  "Linked references" grouped by page (a tree entry per page with its
  count, under it an entry per referencing block with its first line,
  the journal pages first by date), then "Unlinked references" (pages
  whose text holds the page's title or an alias without a link, found
  by a case-insensitive search over the index's first lines and the
  files on demand), refreshed on `document-open` and
  `document-after-save`; a click on an entry opens the file at the
  block (`clicked` of `panel-event`, `file.open` with the line). The
  same for the block at the cursor when the setting
  `backlinks.block` is on (`selection-changed`, debounced). Tests: the
  panel's tree for a corpus page with three referencing pages and one
  unlinked mention; the pick's items and the inserted text per format.
  (Done 2026-10-10: Find Page, Insert Link (Logseq's `[[Title]]` or
  Org's file link as `config.edn` asks; Obsidian's shortest, relative or
  absolute path, wiki or Markdown link, as `app.json` asks), Insert
  Block Reference over the blocks that have an id, and the backlinks
  panel. Kalem refuses a panel whose clickable entries do not have
  unique keys, so the entries are keyed by their place in the tree and
  the plugin keeps where each goes. A quick-pick returns indices only,
  so "Create a page…" is the list's first item and asks the title after.
  Inserting after a pick goes through the plugin's own command
  `graph.insertText`, which `kalem.run` queues, since `editor` is
  reachable only while a command runs. Open: the block-level backlinks
  of `backlinks.block`; unlinked mentions are searched in blocks' first
  lines only.)

- [~] GR3d The documents, through `documents` and `styled-documents`
  as the git plugin writes its status, each refreshed when the index
  changes and printed under `kalem run`: **Backlinks: PAGE** (kind
  `graph-backlinks`, the panel's content as a document, for the
  terminal user who wants it full width and searchable); **All
  pages** (`graph-pages`: title, namespace, backlink count, block
  count, last journal mention; sorted by title, `s` cycles the sort;
  Enter opens); **Journals** (`graph-journals`: the last thirty
  journals newest first, each page's blocks under its title as Logseq's
  Journals view shows them, read-only, Enter opens the day for
  editing, `PageDown` at the end loads thirty more); **Tags**
  (`graph-tags`: every tag page with its count, Enter shows its
  backlinks); **Tasks** (`graph-tasks`, GR9b); **Graph**
  (`graph-graph`: pages as a tree by namespace, each with its
  in-degree and out-degree, orphans in a section of their own; no
  drawing of nodes, which no interface of Kalem's offers and a tree
  serves better in a terminal). Styled: journals' dates muted, counts
  muted, titles bold, keywords in Org's colors. The `▸ ▾` folds and
  the `glyphs` setting as the git plugin's.
  (Done 2026-10-10, `src/views.rs`: Backlinks (with the references to
  the page's blocks), All pages, Journals, Tags (an Obsidian tag opens a
  "Tag:" document, tags not being notes there), Tasks (template blocks
  left out) and Graph, styled, with Enter on every line that names a
  place, written anew when a file of their graph changes. Open: the `s`
  sort of All pages, "PageDown loads thirty more", and the dates of
  tasks before today's date is known.)

## GR4. Kalem's core: layers over a core mode's view

Kalem's side, in a sibling worktree on a branch from `main`
(`org-graph`, branch `graph-mode`), written for every later plugin and
reviewed as an API change (the rule of `kalem-api-general`): nothing of
it names this plugin.

**What the code says (read 2026-10-10), and why GR4 changed.** The
editors do not draw a document from the mode contract's tree:
`kalem_core::mode_view` picks, by `DocumentMode`, each mode's own
drawing (`markdown::line_view`, `latex_view::line_view`, Org's syntax
tree); the contract's `ModeSpec::parse` serves `modes::check` and the
batch commands. A plugin mode returning a tree, even one layered on
Markdown's, would need a generic drawer of trees, and would draw
Markdown worse than Markdown's own (its tables, front matter, wiki
links, formulas, pictures). So the graph does not become a mode: it
becomes a **layer** over the core mode's view. The core draws the line
as it does; the layer's overlays then change it, each by source range:
hidden away from the cursor (a `^id`, an `id::` line's text, the
brackets of `((uuid))`), shown as other text away from the cursor
(`((uuid))` as its block's text), drawn in a style (a tag, a task's
keyword, a highlight), and whole lines hidden or folded to their first
line away from the cursor (a block's `id::` and `collapsed::` lines, a
`:LOGBOOK:`). A line is made of runs mapped to source ranges, and
hiding, replacing and styling are what Markdown's own drawing does to
runs already, so a layer needs no new drawing in either editor; the
lines it hides go through the blocks' visibility as drawers do. The
same contract serves Obsidian's callouts and `%%comments%%`, Pandoc's
and MyST's extensions, Hugo's shortcodes, Dataview's fields: every
"Markdown plus" a plugin, without a second Markdown parser and without
touching the suites the core passes. A standalone plugin mode with a
parser of its own (Typst, R5.10) still needs the tree's binding and a
drawer of trees, T3.1.9g proper; it is left to that item. The tasks
below replace the earlier GR4a to GR4d.

- [x] GR4a The core's contract, `kalem_core::layers`: `Overlays` (spans
  with an effect: `Hide`, `Replace { text, style }`, `Style(style)`;
  lines with an effect: `Hidden`, `Folded`), a registry of layers by
  plugin (an ID, the root markers a document's folder or an ancestor
  must hold, the modes it serves), the overlays of a document computed
  by its layer's provider and kept by the document's version
  (`DocumentState::overlays`), and `mode_view` applying them to the
  base view's lines and blocks when the document is shown as it reads,
  never in the source view. Spans of the cursor's line show as source,
  as markers do; a replaced or hidden span never splits a run the base
  already replaced (a wiki link), it is dropped there. `BlockKind::Hidden`
  for lines hidden away from the cursor, handled by `view::visible`
  beside drawers. Tests in the core with a layer written in Rust.
  (Done 2026-10-10 in Kalem's worktree `org-graph`, branch `graph-mode`,
  rebased on `origin/main`: `kalem_core::layers` (`Overlays`,
  `LayerSpec`, the registry, markers looked for upwards and remembered,
  the provider, `Outcome` with `Busy` asked again at the next drawing,
  `apply_line` splitting verbatim runs and leaving a span that would cut
  a replaced run, `apply_blocks` cutting the mode's blocks, or one block
  of the whole text when the mode gives none, which Markdown does
  without front matter or formulas), `BlockKind::Hidden` in
  `view::visible`, `DocumentState::overlays` kept by the text's version,
  the layers' generation and the path, `mode_view::view_key` so that
  both editors keep their blocks by both. Six tests, one over a real
  Markdown document with a layer written in Rust; Kalem's 566 core tests
  pass.)

- [x] GR4b The plugin API, 0.2.10: the interface `layer` exported (one
  function, `overlays(layer, path, text)`), in a world `extension-layer`
  (the `extension` world and that export) that `kalem-plugin`'s feature
  `layer` builds, its `Plugin` trait gaining a default `overlays`; the
  host binding `plugin` and, when the component exports it, `layer`,
  so components of 0.2.8 load unchanged; the manifest's `layers`
  (`[{"id", "markers", "modes"}]`) read when a plugin loads and
  registered in the core, the bridge calling the plugin for a
  document's overlays under the plugin's time and memory budget, a
  failure leaving the document as the core draws it. The `clock`
  interface in the `extension` world (K9) in the same release. The
  Book's Part III gets "Layers over a mode" and the version's line.
  (Done 2026-10-10, the same branch: `layer.wit` (the export `layer`
  with `overlays(layer, path, text) -> overlay-set`, the import
  `layers.refresh`), the `extension` world importing `clock` and
  `layers`, the world `extension-layer`, `kalem-plugin`'s feature
  `layer` and `clock` module, `Plugin::overlays`, the export macro; the
  host binding `plugin` alone and `layer` when exported, `clock` and
  `layers` linked without a permission; the manifest's `layers` read and
  registered when the plugin runs, the bridge asking the plugin under
  its budget with a lock it only tries, a trap stopping the plugin as
  any call's does. Kalem 0.6.8 was released with 0.2.9 (`flow-3`), so
  the layers are 0.2.10 (the owner's decision of 2026-10-10), and
  `flow-3.wit` of 0.2.9 is frozen with them. A
  test plugin, `tests/plugins/layered`, and a host test of the overlays,
  the refresh and the clock; an extension without the export loads with
  no layer (tested, and checked in the terminal editor with this plugin
  built against 0.2.8). The Book's Part III has "Layers over a mode" and
  the version's line; CHANGELOG. The plugin builds its layer with
  `RUSTFLAGS="--cfg kalem_layer"` against the branch's `kalem-plugin`
  (feature `layer`) until Kalem's `main` has it; built against `main` it
  has no layer, as before.)

- [x] GR4c The leader table: the `SPC n r` rows become the graph
  plugin's (their `reason` "Org Roam is not part of Kalem" replaced by
  the plugin's commands, as the `SPC g` rows were the git plugin's),
  which-key naming `+roam` as Doom does.
  (Done 2026-10-10 on the branch `graph-mode`: the one row became Doom's
  roam map, `a`, `f`, `F`, `g`, `i`, `n`, `r`, `R`, `s` and the `d`
  group's `b`, `d`, `D`, `f`, `m`, `M`, `n`, `t`, `T`, `y`, `Y`, `-`,
  those the plugin serves marked "Needs the graph plugin", the capture
  keys "Planned" with R5.2, random and references "Not yet"; which-key
  names nested groups by their path (`+roam`, `+by date`), tested. The
  plugin's keys follow Doom's: the previous and next journal are `d b`
  and `d f`. Beside it, the when-clause key `editorLayer` (the layer
  serving the document, `graph.logseq`), so that a plugin binds keys in
  its layer's documents only.)

## GR5. The layer: a Logseq Markdown graph drawn as Logseq draws it

- [~] GR5 The layer `logseq` over Kalem's Markdown (`src/layer.rs`),
  construct by construct, each with its test and checked in the
  terminal editor of the branch:

  | In the file | Becomes |
  |---|---|
  | A block's `key:: value` lines Logseq hides (`id`, `collapsed`, `heading`, `created-at`, `updated-at`, `query-*`, `card-*`, `logseq.*`, `:block-hidden-properties`) | lines hidden away from the cursor |
  | Other properties, the page's properties | the key dimmed |
  | `((uuid))` | the block's first line, as a link; dimmed when the block is not found or the graph not indexed |
  | `{{embed ((uuid))}}`, `{{embed [[page]]}}` | `↳` and the block's first line, or the page's title |
  | `TODO`, `DOING`, `LATER`, `NOW`, `DONE`, `WAITING`, `CANCELED` at a block's start, `[#A]` | the keyword and the priority as Org draws them |
  | `SCHEDULED: <…>`, `DEADLINE: <…>` | a date, dimmed |
  | `:LOGBOOK:` … `:END:` | folded to its first line |
  | `#tag`, `#[[two words]]` | a tag |
  | `#+BEGIN_NOTE` … `#+END_NOTE` and the others | their lines dimmed |
  | `[[page]]`, `$$…$$`, pictures, tables | Kalem's Markdown, unchanged |

  (Done 2026-10-10 but for what follows. Open: an embed's block or
  page drawn in full under its line, which needs lines a layer adds
  (virtual lines, K6's kin); an admonition drawn as a callout box;
  `logseq.order-list-type:: number`'s numbers; `^^highlight^^`;
  `collapsed:: true` folding the block on open, which needs a layer's
  folds (a `folded` effect on lines); `{{query}}`'s results (GR9a).)

## GR6. The layer: an Obsidian vault drawn as Obsidian draws it

- [~] GR6 The layer `obsidian` over Kalem's Markdown, T2.7c.12's
  Obsidian half, here by the owner's decision of 2026-10-10: a callout's
  `[!type]` shown as its icon and name (every type Obsidian names, an
  unknown one by its own name), `-` folding it to its first line; a
  `^id` at a line's end hidden; `%%comments%%` hidden, over lines too;
  `==highlight==` bold, its markers hidden; tags styled; `![[note]]`,
  `![[note#Heading]]` and `![[note#^id]]` shown as `↳` and the note's
  title or the block's text; Dataview's `key:: value` key dimmed; front
  matter as Kalem folds it. (Done 2026-10-10. Open: a callout drawn as a
  box with its color; embeds drawn in full; `![[pic.png|300]]` at its
  width, Kalem's Markdown drawing `![[…]]` pictures being the core's
  T2.7c.12 part that stays there.)

## GR7. The layer: a Logseq Org graph

- [~] GR7 The layer `logseq` over Kalem's Org for `.org` files: Org's
  own headlines, drawers, keywords, priorities and planning lines need
  nothing; the layer shows `((uuid))` as its block's text and `{{embed}}`
  as `↳` and the block's text, and styles `#tags`. (Done 2026-10-10.
  Open: `[[page]]` resolved by the graph's rules before Org's own (Org
  takes `[[page]]` as a fuzzy link inside the file), the TODO keywords
  of `:preferred-workflow` added to the document's keyword set.)

## GR8. Editing as an outliner

- [~] GR8a Blocks. In a graph's Markdown, the mode's `edit` hook: Enter
  at a block's end makes a sibling block at the same depth (Markdown's
  list Enter does this; checked, and the keyword, priority and
  properties of the block not copied), Enter in the middle splits the
  block as Logseq does, Tab and BackTab change the depth of the block
  with its children (Markdown nests one item today; the children must
  move too). Plugin commands, each one `editor.replace` and one undo
  step, under `SPC m` in a graph's document and in the Graph menu:
  `graph.moveBlockUp` and `Down` (`Alt+Up`, `Alt+Down`: the block
  with its children past its sibling), `graph.indent` and `outdent`
  (`Alt+Right`, `Alt+Left`, for the Word-like profile), `graph.toggleFold`
  (`SPC m f`, and a click on the bullet in the window): folds the block
  in the view (`view.fold` run through `kalem.run`) and writes or
  removes `collapsed:: true` exactly where Logseq writes it (after the
  block's other properties, before its children) when the setting
  `persist_folds` is on: on by default in a Logseq graph, Logseq's own
  behaviour, so that a block folded in Kalem is folded in Logseq and on
  every synced device; off keeps the file untouched; and never in an
  Obsidian vault, which keeps folds in its own data and would take the
  line for content (owner, 2026-10-10), `graph.zoom` (`SPC m z`,
  `view.narrowToSubtree`
  on the block, Logseq's zoom-in; Escape widens), `graph.cycleTodo`
  (`SPC m t`, Org's `t`: the `:preferred-workflow` cycle, `TODO →
  DOING → DONE` or `LATER → NOW → DONE`, writing the keyword as
  Logseq writes it; DONE adds nothing else unless `:feature/enable-
  timetracking?` is on, when a `:LOGBOOK:` clock is closed as Logseq
  does), `graph.setPriority` (`SPC m p`), `graph.schedule` and
  `graph.deadline` (`SPC m s`, `SPC m d`: a date prompt, the planning
  line written in Org's form under the first line), `graph.toggleCheckbox`
  for `- [ ]` items (Markdown's own). Byte-exact tests: each command on
  a corpus block, the rest of the file identical.
  (Done 2026-10-10, `src/edit.rs` and `src/app/outline.rs`, as plugin
  commands since a layer has no `edit` hook: Cycle Task, Set Priority,
  Schedule, Deadline, Move Block Up and Down with the blocks under it,
  Indent and Outdent, Fold Block writing `collapsed:: true` with the
  layer hiding the folded block's children; Logseq's Markdown and Org,
  and Obsidian's lists (check boxes for tasks). Keys in notes through
  `editorLayer`: `SPC m t`, `SPC m p`, `SPC m d s`, `SPC m d d`, `SPC m
  z`, `Alt+Shift+Up`/`Down`/`Right`/`Left`, and `Tab`, `Shift+Tab`
  (indent while typing, fold in Vim's normal mode). Each one undo step,
  the cursor taken to a moved block by `edit.gotoLine`. Checked in the
  terminal editor of the branch: fold, cycle, move, save. The scanner
  learned that a block's lines end at its last line that is not blank
  and include its code and drawers. Open: Enter splitting a block as
  Logseq does (Markdown's own Enter continues the list); zoom, which
  needs narrowing a Markdown document to a range; the Word-like
  profile's `tab`, which its keymap gives to `view.fold`; a done task's
  clock. Corrected with GR9: the cycle is Logseq's own, each keyword to
  its next whatever the workflow (`TODO → DOING`, `LATER → NOW`, both
  `→ DONE`, `DONE →` none), and none, `WAITING` or `CANCELED` to the
  workflow's first keyword.)

- [ ] GR8b Completion, through GR4a's `complete` hook: `[[` offers the
  graph's pages and aliases by title (the core's wiki completer offers
  file names; the layer's items replace them in a graph), closing
  `]]`; `((` offers blocks by their first line, full-text, inserting
  `((uuid))` and writing the id when missing (GR8c); `#` offers tags
  and pages; `::` at a block's start offers property keys the graph
  uses and, after a key, its values seen; `/` offers Logseq's slash
  commands that the plugin implements (TODO, DOING, LATER, NOW,
  SCHEDULED, DEADLINE, Today, Tomorrow, Yesterday, Embed block, Embed
  page, Query, Template, A, B, C) when the setting `slash_commands` is
  on; `<` offers the `#+BEGIN_*` blocks. Each item tested through
  `kalem complete FILE:LINE:COL`.
  (Open: a plugin cannot give completions yet (T3.1.9c). Insert Link and
  Insert Block Reference cover `[[` and `((` from the palette and `SPC n
  r i`, `SPC n r b` meanwhile.)

- [~] GR8c Writing into another file: a block referenced for the first
  time gets its `id:: uuid` (a v4 UUID from `clock.random`) written
  into its file after its first line and other properties, as Logseq
  does; a page renamed (`graph.renamePage`, `SPC n r R`) rewrites
  `[[old]]`, `#old`, `alias::` and `tags::` mentions in every file of
  the graph and renames the file by the name format, as both
  applications do; a page or block moved likewise. The file that is
  open in the editor is edited through the editor (the owner's K5, an
  edit to a named open document, shared with `pta_todo.md`'s K3);
  until it exists, a file not open is written with `fs.write`
  (`fs:write:workspace` added to the manifest here; decided by the
  owner on 2026-10-10, since without it `((` offers only blocks that
  already have an id, pages cannot be renamed and new pages get no
  template) and a file that is open is left alone with a notice naming
  it, so that no edit is lost. The safety rules of every write: the
  file is read again right before, only the lines touched are
  rewritten, nothing under `logseq/` or `.obsidian/` is ever written,
  a file the editor holds modified is refused, and the write is said
  in the process log with the file and the lines. Tests: the id
  written where
  Logseq writes it; a rename across three files; a modified open file
  refused.
  (Done 2026-10-10 in part. Insert Block Reference offers every block;
  one without an id gets a UUID (the clock's random bits with API
  0.2.10, else a hash and a counter) written as `id::` where Logseq
  writes it: in its file with `fs.write`, the file read again first, or
  by the editor with the reference itself when the block is in the
  document being edited. Rename Page sets `title::` (`#+title:` in Org)
  and rewrites every reference by the page's title, `[[…]]`, `#tag`,
  `#[[…]]`, embeds and comma properties, in every file, aliases kept; a
  page whose new name another page has is refused. The plugin follows
  which files Kalem holds with unsaved changes (`document-open`,
  `document-changed`, `document-close`, saves) and writes none of them;
  Kalem reloads an open file that has none. Open: the file renamed with
  the page, and a vault's note renamed at all, which need a way to
  rename a file (K11); a block moved to another page.)

## GR9. Queries and tasks

- [x] GR9a Logseq's simple queries over the index: `{{query (and [[page]]
  (task TODO DOING) (between -7d today) (property type book)
  (page-property type book) (page-tags tag) (namespace ns)
  (priority A B) (not …) (or …) (page [[x]]) (full-text-search "x")
  (sort-by created-at desc) (sample 5))}}`, the result rendered as the
  block `Embed`'s text: the matching blocks grouped by page with their
  first lines, or a table
  when `query-table:: true` with the columns `query-properties::`
  names; `(between …)` over journal dates and `SCHEDULED`/`DEADLINE`;
  recomputed when the index changes. Advanced queries
  (`#+BEGIN_QUERY` with Datalog) are shown as source with a note that
  Kalem does not run them (a non-goal: no Datascript). Tests: each
  clause on the corpus; a table result's columns.
  (Done 2026-10-10, `src/query.rs`: every clause listed, `[[page]]` and
  `#tag` over Logseq's path references (the block's page, its own
  references and those of the blocks it is under), a query of page
  clauses finding pages, `(between …)` over `today`, `±Nd/w/m/y`, a
  journal's title and ISO dates, results ordered as the backlinks are
  unless sorted. As a layer has no virtual lines (K2), the block shows
  `{{query …}}` as one line away from the cursor (`⌕ 4 blocks: …`, the
  first two), kept by the index's version, and the results open as a
  document (`graph-query`) by Follow Reference on the line or by the new
  Run Query (`SPC n r q`): grouped by page, or the table that
  `query-table::`, `query-properties::`, `query-sort-by::` and
  `query-sort-desc::` on the query's block ask for, Enter opening the
  block. `#+BEGIN_QUERY` is marked as not run, and Follow Reference
  inside one says so. `(sample n)` takes the first n. The corpus's
  queries moved to where Logseq writes the table's properties, on the
  query's block. Checked in the terminal editor of the branch: the
  lines, the table, Enter.)
- [~] GR9b The **Tasks** document (`graph-tasks`, `SPC n r t`): NOW and
  DOING first, then LATER and TODO by priority, WAITING, then the
  scheduled and deadline items of the coming week with their dates
  (Logseq's "SCHEDULED AND DEADLINE" section of a journal), each
  with its page, Enter opening the block, `t` cycling the keyword from
  the document (through GR8c's path for a file not open). When
  Kalem's agenda (R5.2) lands, the plugin offers its items to it
  through whatever interface R5.2 gives plugins, so one agenda shows
  Org files and the graph's blocks together; the shapes (a block with
  a keyword, a priority, dates and a page) are the same.
  (Done 2026-10-10 but the agenda: `t` in the Tasks and Query documents
  (`textType == graph-tasks || textType == graph-query`) cycles the
  line's task in its file as GR8c writes one (read again, found again
  by its line and text, refused when Kalem holds it with unsaved
  changes); the documents and the notes' query lines follow. The day
  the documents count from is the clock's in UTC until the user gives
  the date (K9). Checked in the terminal editor of the branch. Open: the
  agenda, with R5.2.)

## GR10. Search and views

- [x] GR10 Search Notes (`notes.search`, `SPC n s`) over the graph's
  root when the notes folder setting is empty and a graph is open;
  "Search the notes' headings" (`SPC n S`, "not yet" in the leader
  table) as the plugin's `graph.findHeading` over the index's
  headings and block titles; unlinked references and orphan pages in
  the Graph document; `graph.recent` (`SPC n r l`): the pages opened
  last; whiteboards (`whiteboards/*.edn`) and `.canvas` files listed in
  All pages with "opens in Logseq" or "opens in Obsidian" and the
  system-open offered; `hls__*` pages (Logseq's PDF highlights) listed
  as pages, their highlights not drawn; the status bar item's click;
  `kalem run graph.pages ROOT`, `graph.backlinks PAGE`, `graph.tasks
  ROOT` printing the documents.
  (Done 2026-10-10. Search the Graph (`SPC n s` in a graph's notes and
  documents) runs Kalem's Search in Folder over the root with `ignore`
  (`/logseq/`, `/.obsidian/`, `/.trash/`, the hidden folders), which
  Kalem's branch `graph-mode` adds (K8's first half); a Kalem without it
  searches the folder whole. Find Heading (`SPC n S`): the headings and
  the top-level blocks of pages, not of journals, at most 20,000, by
  page. The Graph document's unlinked mentions, counted in one pass over
  the blocks (the names by their first word), equal to each page's own
  list on the three corpora. Recent Pages (`SPC n r l`) for the session,
  and Random Page (`SPC n r a`, the leader table's row, the clock's
  random bits). Whiteboards and canvases are pages without blocks,
  linked as their applications link them (`[[Design]]`,
  `[[Board.canvas]]`), listed in All pages; Enter or a link shows the
  file in the file manager (`file.reveal` with a `path`, also added on
  the branch; built without it, a notice names the file), as a plugin
  opening a file with its application could start programs, which is
  for the owner to decide (K12). `hls__` pages are pages, named in All
  pages as a PDF's highlights, their `ls-type::`, `hl-*::` properties
  hidden by the layer. The status bar's click was there (All pages).
  `kalem run` is T3.1.16, built on the branch: `kalem run graph
  graph.pages ROOT`, `graph.tasks ROOT`, `graph.backlinksDocument
  PAGE`; a folder is a graph's root from itself. Found on the way: `#+`
  directives (`#+BEGIN_NOTE`) were read as tags. Checked in the
  terminal editor of the branch: the search without `logseq/`, Find
  Heading; and `kalem run` on the corpus.)

## GR11. Speed, limits and the tests

- [x] GR11 A generated large graph in the tests (`tests/large.rs`,
  ignored by default, run on demand and in CI's nightly): 10,000
  pages and 1,000,000 blocks (Logseq's own scale target for the
  database version), 50 references a page; the index built under 2 s
  and under 300 MB, a save re-indexed under 10 ms, the backlinks
  panel for a page with 1,000 references drawn under 50 ms, a parse of
  a 5,000-block journal within the mode budget through the layer (the
  layer walks the base tree's nodes once per parse: O(nodes)), the
  Embed cache hit on every keystroke that does not touch a reference.
  The conformance test: the manifest; each corpus graph's kind,
  format, folders and counts; every rule of GR2b with its fixture;
  every file of the three corpora through the mode's byte-exact round
  trip (`modes::check`, and saved unchanged after folding and
  unfolding) once GR5 to GR7 exist; the documents' snapshots in both
  editors; `kalem run` output. A graph that is not UTF-8, a file over
  16 MB, a `config.edn` that does not parse (the defaults used, one
  notice), a circular embed (cut at depth 3 with a note) each give one
  notice and never a panic (the `diagnostics` interface's `panicked`
  checked in a test).
  (Done 2026-10-10. `tests/large.rs` generates the graph in memory and
  measures with a counting allocator; the README has the numbers. The
  first run missed five budgets and led to: the scanner's vectors
  trimmed and a task's keyword borrowed from the scanner's list
  (362 to 287 MB), a backlink's path shared by its file's references; a
  save that keeps the page's names withdrawing and adding again only
  its file's entries (514 ms to 0.4 ms), checked against a fresh build
  after four kinds of edit of every corpus file; the panel's unlinked
  references searched for when opened, as Logseq does (286 to 2 ms);
  the layer reading a line's references once instead of at each column
  (58 to 8 ms); the Tasks document's sort without copies (697 to
  350 ms); a query's `[[page]]` from the index's backlinks by file (496
  to 20 ms). In Kalem itself the plugin was stopped at the first note
  of the large graph: an extension's limits are 64 MB and 100 ms a
  call, so the manifest asks for a viewer's, 1 GB and 10 s; Kalem's
  release build indexes the graph in 1.9 s (`kalem run`), and the
  editor shows its status within four seconds of the note opening.
  The layers have no `Embed` nodes to cache: Kalem keeps a note's
  overlays for each version of its text, and the queries' lines for each
  version of the index. The conformance test folds and unfolds every
  Logseq block with children back to the same bytes and checks the
  overlays of every corpus file; `tests/snapshots.rs` holds the
  documents, which `kalem run` prints byte for byte (ignored test,
  `KALEM=`); `tests/robust.rs` covers the configuration that does not
  parse, the file not in UTF-8 and the one over 16 MB (one notice; the
  rest listed under "Not read" in All pages, since a plugin has no
  process log), embeds of themselves (one line, never followed: the
  depth-3 cut has nothing to cut), and every corpus note broken
  thousands of ways through the scanner, the layer, the queries and the
  editing commands, which found no panic. Found on the way: the Graph
  document hung a namespace's pages under the line above when the
  namespace itself had no file. Open: the first index of a large graph
  holds Kalem's thread while it is built (K13).)

## GR12. Release, the README and the Book

- [~] GR12 Two releases. `graph-v0.1.0` after GR3: "your Logseq graph
  or Obsidian vault in Kalem: backlinks, journals, find and insert,
  the pages and tasks as documents; nothing converted, `logseq/` and
  `.obsidian/` untouched", with the README saying plainly that the
  notes still open in Kalem's Markdown and Org modes and what that
  shows (`id::` lines, raw `((uuid))`) until the mode. `graph-v0.2.0`
  after GR8: the mode, "opened as itself". The README: what is read and
  what is shown per application, the keys (`SPC n r`, `SPC m`), the
  settings, known differences from Logseq (no whiteboards, no advanced
  queries, no flashcards, no plugins, no PDF highlights drawn, no
  real-time sync: Syncthing or git as Logseq OG users already do) and
  from Obsidian (no canvas, no community plugins' syntax beyond
  Dataview's inline fields, no `.obsidian/` settings read), a
  migration note for Logseq OG users (open the same folder; keep
  Logseq OG installed as long as you like, both read the same files)
  and for the database version (a Markdown Mirror folder opens as a
  vault with the mirror's block-id comments hidden, if that proves
  true in a test; the database itself does not open); `index.json`,
  `CODEOWNERS`; the Book's Part III page for the plugin and, with
  GR4, "Writing a mode"; R5.5's text in the roadmap re-pointed here.
  (Done 2026-10-10 but the releases themselves. 0.1.0 is everything to
  GR11 without the layer, built as the release workflow builds it: it
  imports plugin API 0.2.9, so the manifest says `^0.2.9` (it said
  `^0.2.8`, which the build had outgrown); checked on Kalem 0.6.8 built
  from its tag, and on 0.6.7, which runs it too. The README says what
  0.1.0 does on Kalem 0.6.8 and what waits for Kalem's next release
  (the layer, the outliner's keys in notes, a query's line, the search
  leaving out `logseq/`, the reveal, the clock, `kalem run`), how to
  move over from Logseq's file version, its database version and
  Obsidian, and the differences from each. The database version: its
  Markdown Mirror (Logseq's ADR 0016, `docs/logseq-markdown-syntax.md`)
  writes no block-id comments, contrary to what this item supposed, but
  Logseq Markdown with a page `id::` line and properties as `* key::`
  items; the plugin finds it by `mirror/markdown/.index.edn`, reads it
  as a Logseq graph with the items as properties, never writes it, and
  `corpus/logseq-db-mirror` tests it as Logseq's own tests write it.
  `index.json` regenerated (its layers' markers), CODEOWNERS as it was.
  On Kalem's branch `graph-mode`: the Book's Part III page "Logseq
  graphs and Obsidian vaults"; "Layers over a mode" was written with
  GR4; R5.5 re-pointed here and T2.7c.12's Obsidian half recorded as
  this plugin's layer. Open: pushing main and the tag `graph-v0.1.0`,
  for the owner; 0.2.0 with the layer once Kalem releases API 0.2.10,
  the manifest then saying `^0.2.10` and the release built with the
  feature `layer`.)

## What Kalem's core must gain

Each is the plugin's gate or its ask, written for every plugin.

- K1 Layers over a mode's view (GR4a, GR4b): done on Kalem's branch
  `graph-mode`, waiting to be merged. It replaces the earlier asks of a
  plugin mode with a tree layered on a core mode's (`base-parse`) and
  new kinds (`Embed`, `Callout`, `Highlight`, `Drawer`): the editors
  draw from each mode's own code, not from the contract's tree, so a
  layer over that drawing serves where a tree would not.
- K2 Lines a layer adds under a line, drawn and not part of the text
  (an embed's block in full, a query's results): the editors' virtual
  lines, beside K6's virtual text. Not blocking.
- K3 Folds a layer gives (a block with `collapsed:: true` folded on
  open, a callout's `-`): a `folded` effect on lines that the editors'
  folds start from. Not blocking.
- K4 The `SPC n r` rows handed to the plugin, `+roam` in which-key
  (GR4d), as `SPC g` was handed to the git plugin.
- K5 An edit to a named open document, or an event when a file the
  plugin wrote is re-read (GR8c); the same ask as `pta_todo.md`'s K3.
  Until then `fs.write` for files not open, a notice for open ones.
- K6 Virtual text in `decorations` (the git design's 9.3): backlink
  and reference counts at a block's end, as Logseq shows them. Not
  blocking.
- K7 The permission consent on first use (R5.11): a plugin that reads
  every file of a project should be consented to in the editor, not
  only at install, before `graph-v0.1.0` reaches users. Recommended,
  not blocking.
- K8 `notes.search` over a folder a plugin names, and headings search
  from a plugin (GR10). Not blocking. (The first half done on the
  branch `graph-mode`: Search in Folder's `ignore`; headings are the
  plugin's Find Heading.)

- K9 The `clock` interface in the `extension` world (the viewers'
  worlds import it already): done on Kalem's branch `graph-mode` with
  K1. Until it is merged and the plugin built against it, this plugin
  asks the date once a session (GR3b) and leaves Logseq's `<% time %>`
  empty. Built against it, the plugin takes block IDs from
  `clock.random` and the Tasks and Query documents' day from
  `clock.now` in UTC; the journals still ask, as `clock.timezone` names
  the zone without its offset and the plugin carries no time zone
  database. Open: the local date or the zone's offset from the clock
  (a function added to `clock`, for every plugin that shows a date).
- K12 Opening a file with its application, or an application's link
  (`obsidian://open?vault=…&file=…`, `logseq://graph/…?page=…`), from a
  plugin: a whiteboard or a canvas opened where it is drawn. A command a
  plugin may run that opens a path for the system can start programs
  (a `.command` file, an application), past the `process` permission:
  for the owner to decide, with a confirmation or a permission. Until
  then the plugin shows the file in the file manager (`file.reveal`'s
  `path`). Not blocking.
- K13 Work a plugin does without holding the editor: a call that may
  take seconds (the first index of a graph of a million blocks, 1.9 s in
  Kalem's release build) run off the editor's thread, or in slices with
  the editor drawing between them, the plugin's other calls waiting.
  Until then the first command or note of a large graph waits. Not
  blocking.
- K11 Renaming (and deleting) a file through `fs`, under
  `fs:write:workspace`: a page renamed with its file as Logseq does, a
  vault's note renamed at all (GR8c). Not blocking.
- K10 A plugin learns the folders of Kalem's projects (an `editor` or
  `kalem` function, or an event when one is added), so that the graphs
  among them are found at start, and a notice can say which folder to
  add when a note's folder is no project's. Not blocking.

## Decisions, and what waits in Kalem's repository

- Decided (2026-10-10): the name stays `graph`, id `org.kalem.graph`,
  manifest name "Note graphs", description naming both applications.
  Not `obsidian`: the plugin opens Logseq graphs first and Obsidian
  vaults second, and a product's name as a plugin's name would claim an
  affiliation and tie the plugin to one application; the repository
  names plugins after what they open (`docx`, `xlsx`) or run (`git`),
  and what this one opens both applications call a graph or a vault.
  Users find it by the description and the README, which say Logseq
  and Obsidian in their first line.
- Decided (owner, 2026-10-10): T2.7c.12's Obsidian half lives in this
  plugin's layer (GR6), not in the core's Markdown mode, so that the
  core stays CommonMark, GFM and wiki links (D29) and every flavour of
  Markdown is a plugin on `base-parse`. T2.7c.12 in Kalem's `todo.md`
  is to be re-pointed here, its wiki-link half (T2.7c.9) staying in
  the core.
- Decided (owner, 2026-10-10): of `.obsidian/`, `daily-notes.json`,
  `app.json` and `templates.json` are read, read-only, with the
  plugin's settings and then Obsidian's defaults as the fallbacks;
  nothing else under it is read and nothing under it is written. The
  cost is a dependency on three undocumented but long-stable files;
  the gain is a vault that works with no settings and links and daily
  notes Obsidian will not rewrite (GR2b, GR3a, GR3c). T2.7c.12's
  "never read `.obsidian/`" is narrowed to this.
- Decided (owner, 2026-10-10): folds are written to the file as
  `collapsed:: true` by default in a Logseq graph, never in an
  Obsidian vault; the setting `persist_folds` turns it off (GR8a).
- Decided (owner, 2026-10-10): `fs:write:workspace` is granted with
  GR8, under GR8c's safety rules; K5 is its complement for open files,
  not its replacement.
- Decided (owner, 2026-10-10): this plugin's mode is the first released
  against the mode binding, ahead of Typst (R5.10); the binding is
  written once for both the layered and the standalone case (GR4).
- Decided (owner, 2026-10-10): API 0.2.10 for layers and the clock,
  one release (GR4a, GR4b); 0.2.9 went to `flow-3` with Kalem 0.6.8.
- Confirmed out of scope: Logseq's database graphs (SQLite),
  whiteboards and canvases, advanced Datalog queries, flashcards, PDF
  highlights, real-time sync, Logseq plugins' and Obsidian community
  plugins' syntax beyond Dataview's inline fields.
- Still to do, in Kalem's repository by the owner: R5.5 and T2.7c.12
  re-pointed at this plugin; a roadmap item for T3.1.9g with GR4's
  scope, ahead of R5.10; the `SPC n r` rows handed to the plugin.
