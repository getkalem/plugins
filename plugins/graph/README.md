# Note graphs (`graph`)

A Logseq graph or an Obsidian vault in [Kalem](https://github.com/getkalem/kalem), opened as itself: its pages, journals, links, block references, tags and tasks indexed, what links to a page shown beside it, today's journal a key away. Nothing is converted: Logseq's `logseq/` and Obsidian's `.obsidian/` are never written, and a note is written only when the user saves it or asks for a new journal. The work list is [`graph_todo.md`](graph_todo.md).

**Status: 0.1.0**, for Kalem 0.6.8 and later (0.6.7 runs it too). The plugin finds a graph, indexes it and gives the commands, the backlinks panel and the documents below; it edits a graph as an outliner, runs Logseq's simple queries and searches the graph. Its list of tasks is [`graph_todo.md`](graph_todo.md).

With Kalem 0.6.8 the notes open in Kalem's own Markdown and Org modes and show as they are written: Logseq's `id::` and `collapsed::` lines, a bare `((uuid))`, `{{embed …}}` and `{{query …}}` as text, an Obsidian callout as a quote. Wiki links are links. The plugin's layer, which draws a note as Logseq or Obsidian draws it (below), and a few other parts wait for Kalem's next release, from its branch `graph-mode` (plugin API 0.2.10): the outliner's keys inside notes (the commands are in the Graph menu and the palette meanwhile), a query's line in its note, Search the Graph leaving out `logseq/` and `.obsidian/` (0.6.8 searches the whole folder), a whiteboard or canvas shown in the file manager (0.6.8 names the file in a notice), the clock (0.6.8 asks today's date once a session), and `kalem run`. The sections below say which is which.

## Moving over

- **From Logseq (the file version, "OG").** Add the graph's folder as a project of Kalem's and open any page: nothing is converted, and `logseq/` is never written. Keep Logseq installed for as long as you like: both read and write the same files, and each reads again a file the other saved (save in one before editing the same page in the other). Sync stays as it was: Syncthing, git or iCloud on the folder.
- **From Logseq's database version.** Its SQLite database does not open in Kalem. Its Markdown Mirror does: turn it on in Logseq's settings (the desktop app), and the folder `mirror/markdown/` in the graph's folder opens as a graph, read only: pages, journals, links, tags, tasks, properties (written there as `* key:: value`), queries and search. Logseq writes the mirror from its database and writes over any change, so Kalem never writes it and its editing commands say so.
- **From Obsidian.** Add the vault's folder as a project and open a note. Of `.obsidian/`, only `daily-notes.json`, `app.json` and `templates.json` are read, for the daily notes, where new notes go, how links are written and the templates' folder; nothing under it is written.

## What it needs

- **The graph's folder is one of Kalem's projects** (Projects: Add Project). Kalem lets a plugin read only its projects' folders (`fs:read:workspace`); a note opened from a folder that is no project is not seen as a graph's.
- A Logseq graph is found by its `logseq/config.edn`, an Obsidian vault by its `.obsidian/` folder, a Logseq database graph's mirror by its `mirror/markdown/.index.edn`, from the folder of any note opened in it upwards. A folder without any is a graph when the setting `graphs` names it.
- Kalem 0.6.8 or later (plugin API 0.2.9); the layer and the parts above need Kalem's next release.
- Room to index a large graph: the plugin asks Kalem for 1 GB and 10 s a call (see Speed and size).

## Commands and keys

The keys are Doom Emacs's org-roam map, `SPC n r`, in the Vim profile; every command is in the palette under **Graph** and in the **Graph** menu.

| Keys | Command | What it does |
|---|---|---|
| `SPC n r r` | Backlinks Panel | The side panel: the page's linked references grouped by page, then its unlinked mentions; a click opens the line |
| `SPC n r R` | Backlinks | The same as a document, with the references to the page's blocks; Enter on a line opens it |
| `SPC n r o` | Follow Reference | The `[[page]]`, `#tag`, `((uuid))`, `{{embed}}`, `[[note#Heading]]` or `[[note#^id]]` under the cursor; a page not written yet opens as a new file, written when saved. On a `{{query …}}`'s line, its results (below) |
| `SPC n r f` | Find Page | Every page by title, aliases in the detail; "Create a page…" first |
| `SPC n r i` | Insert Link | A link to the chosen page, written as the graph writes links (Logseq's `[[Title]]`; Obsidian's shortest path, or as `.obsidian/app.json` says) |
| `SPC n r b` | Insert Block Reference | `((uuid))` or `[[note#^id]]` of a block that has an id |
| `SPC n r d t`, `d y`, `d m` | Today's, Yesterday's, Tomorrow's Journal | Opens the journal; one not written yet is made from the graph's journal template (Logseq's `:default-templates {:journals …}`, Obsidian's daily notes template) |
| `SPC n r d d` | Journal of a Date | `2026-10-03`, `oct 3`, `yesterday`, `-3`, `monday` |
| `SPC n r d f`, `d b` | Next, Previous Journal | The next and previous journal that exists |
| `SPC n r p` | All Pages | Every page with its links and blocks (a PDF's highlights named for the PDF), the whiteboards or canvases, then the pages only referenced |
| `SPC n r j` | Journals | The last thirty journals, newest first, with their blocks |
| `SPC n r T` | Tags | Every tag with how often it is used |
| `SPC n r t` | Tasks | NOW and DOING, then LATER and TODO by priority, then WAITING; what is scheduled or due within a week. Enter opens the task, `t` cycles its keyword in its file |
| `SPC n r q` | Run Query | A Logseq simple query typed, its results as a document |
| `SPC n r l` | Recent Pages | The graph's pages opened last in this session |
| `SPC n r a` | Random Page | A page of the graph at random |
| `SPC n s` (in a graph, Kalem's next release) | Search the Graph | Kalem's search over the graph's folder, without `logseq/`, `.obsidian/`, `.trash/` and the hidden folders; elsewhere `SPC n s` searches Kalem's notes folder |
| `SPC n S` (in a graph, Kalem's next release) | Find Heading | The graph's headings, and the top-level blocks of its pages (not its journals'), each with its page |
| `SPC n r g` | Graph | The pages as a tree of namespaces with their links in and out, then the pages named without a link and how often, then the orphans |
| `SPC n r s` | Index Again | Reads the graph again |

The status bar shows the graph of the current note and its pages (`⌬ notes · 1,240 pages`); a click lists them.

**Whiteboards and canvases.** A Logseq whiteboard (`whiteboards/*.edn`) and an Obsidian canvas (`*.canvas`) are pages: links to them resolve, and they show in All pages with the application that draws them. Kalem does not draw them. Following one shows the file in the file manager, to be opened there (with a Kalem whose plugin API is 0.2.10; before, the notice names the file).

**From the command line.** With a Kalem that has batch mode, the documents print without a window:

```bash
kalem run graph graph.pages ~/notes
```

`graph.tasks`, `graph.tags`, `graph.journals` and `graph.graph` take the graph's folder too, `graph.backlinksDocument` a page's file; `--format json` gives the title, kind and text.

**Today's date.** Kalem's plugin API 0.2.10 gives a plugin the time in UTC and the time zone's name, not its offset. The Tasks document and the queries count from the UTC day until the user gives the date. The first journal command of a session asks for today's date, offering the clock's day (the newest journal's without the clock), and keeps it until Kalem quits.

## Editing as an outliner

In a note of a graph (a document the plugin's layer serves, Kalem's when-clause key `editorLayer`, from Kalem's next release; with Kalem 0.6.8 the commands run from the Graph menu and the palette):

| Keys | Command | What it does |
|---|---|---|
| `SPC m t` | Cycle Task | The block's keyword as Logseq cycles it: `TODO` to `DOING`, `LATER` to `NOW`, both to `DONE`, `DONE` to none, and none (or `WAITING`, `CANCELED`) to `LATER` (`TODO` with `:preferred-workflow :todo`); in a vault the check box: none, `[ ]`, `[x]`, none |
| `SPC m p` | Set Priority | `[#A]`, `[#B]`, `[#C]` or none, after the keyword |
| `SPC m d s`, `SPC m d d` | Schedule, Deadline | `SCHEDULED: <2026-10-12 Mon>` after the block's first line and properties (after an Org headline), replaced where it is, removed when the answer is empty |
| `Alt+Shift+Up`, `Alt+Shift+Down` | Move Block Up, Down | The block with the blocks under it past its sibling |
| `Tab`, `Shift+Tab` (typing, in the Vim profile), `Alt+Shift+Right`, `Alt+Shift+Left` | Indent, Outdent Block | The block with the blocks under it a level deeper (under its previous sibling) or out (the blocks after it at its level becoming its children, as Logseq outdents); a tab or the note's own spaces |
| `Tab` (Vim's normal mode), `SPC m z` | Fold Block | `collapsed:: true` written or taken away, as Logseq folds; the layer hides the blocks under a folded block until the cursor goes into them |
| | Rename Page | `title::` set and every reference to the page by its title rewritten in every file of the graph (aliases kept); a file Kalem holds with unsaved changes stops it before anything is written. A vault's note is renamed with its file, which plugins cannot do yet |

Insert Block Reference offers every block: one without an id gets its `id::` written where Logseq writes it, in its file, or by the editor when it is in the document being edited; a file Kalem holds with unsaved changes is not written. Each command is one undo step.

## How notes are drawn (layers)

Kalem's Markdown and Org draw the note; the plugin's layer changes what Logseq and Obsidian show differently, away from the cursor (the cursor's line shows the source, as Kalem's markers do; Show Source shows the file as it is):

| | Logseq | Obsidian |
|---|---|---|
| Hidden | a block's `id::`, `collapsed::`, `heading::` and the other properties Logseq hides | a `^id` at a line's end, `%%comments%%` |
| Shown as other text | `((uuid))` as its block's text; `{{embed …}}` as `↳` and the block or the page; `{{query …}}` as `⌕`, how many blocks or pages it finds and the first two; `#+BEGIN_QUERY` marked as not run | a callout's `[!note]` as its icon and name; `![[note]]` as `↳` and the note or the block |
| Styled | task keywords and priorities as Org's, dates, `#tags`, property keys dimmed | `==highlights==`, `#tags`, Dataview's keys dimmed |
| Folded | `:LOGBOOK:` to its first line; the blocks under a block with `collapsed:: true` hidden | a callout with `-` to its first line |

The layer comes with Kalem's next release (plugin API 0.2.10's `layer`) and this plugin's 0.2.0. Until then it is built only by hand against Kalem's branch `graph-mode`: `RUSTFLAGS="--cfg kalem_layer"` and a `kalem-plugin` with the feature `layer`.

## Queries

Logseq's simple queries run over the index: `[[page]]` and `#tag` (the page's blocks, the blocks referring to it and the blocks under them, as Logseq's path references count them), `"text"` and `(full-text-search "text")`, `(and …)`, `(or …)`, `(not …)`, `(task NOW DOING)`, `(priority A B)`, `(between -7d today)` (a journal's day, `SCHEDULED:` or `DEADLINE:`; `today`, `yesterday`, `tomorrow`, `±Nd`, `±Nw`, `±Nm`, `±Ny`, `[[Oct 3rd, 2026]]`, `2026-10-03`), `(property key value)`, `(page-property key value)`, `(page-tags tag)`, `(page name)`, `(namespace name)`, `(sort-by key desc)` and `(sample n)`. A query of page clauses only finds pages.

The results are grouped by page, journals newest first, with each block's first line; with `query-table:: true` on the query's block they are a table of the columns `query-properties::` names (`[:block :page]` by default), sorted by `query-sort-by::` and `query-sort-desc::`. Enter opens a block, `t` cycles its keyword. The document and the line in the note are computed again when the graph changes.

## What is read

| | Logseq | Obsidian |
|---|---|---|
| Settings | `logseq/config.edn`: format, workflow, folders, journal file and title formats, file name format, hidden folders, journal template, hidden and comma properties | `.obsidian/daily-notes.json`, `app.json`, `templates.json`, read-only; the plugin's settings and then Obsidian's defaults when they are missing |
| Notes | `.md` and `.org` under the pages and journals folders, but `logseq/`, hidden folders and `:hidden` | Every `.md` but `.obsidian/`, `.trash/` and hidden folders |
| Pages | Title from `title::` (or `#+title:`), else the file name decoded (`a___b` is `a/b`, `%3F` is `?`); journals by date; aliases resolve to the page; names compare ignoring case | A note per file, named by its path; `[[Name]]` resolves to the note in the linking note's folder first, else the shortest path; aliases name the note but link nowhere, as in Obsidian |
| Blocks | Every `- ` bullet (every headline in Org) with its depth, `id::`, `collapsed::`, task keyword, priority, `SCHEDULED:` and `DEADLINE:`, properties | Paragraphs, list items and headings; `^id` at a line's end; check boxes as tasks; Dataview's `key:: value` |
| References | `[[page]]`, `[label]([[page]])`, `#tag`, `#[[two words]]`, `((uuid))`, `{{embed …}}`, page properties' values (`alias::`, `tags::`, `:property/separated-by-commas`), `[[file:…][…]]` in Org | `[[note]]`, `[[note\|text]]`, `[[note#Heading]]`, `[[note#^id]]`, `![[note]]`, Markdown links to `.md` files, `#tag` and `#nested/tag` (not `#2026`) |
| Not references | Code spans and code blocks, `#+BEGIN_SRC` in Org, a block of a template (`template::`) as a task | Code, `%%comments%%`, embeds of pictures |

A Logseq database graph's Markdown Mirror is read as a Logseq graph with Logseq's defaults, as Logseq's own description of it writes it (its ADR 0016 and `docs/logseq-markdown-syntax.md`): each file's first line, the page's `id::`, hidden; properties as `* key:: value` items, an open property's values as the items under it; a page's title as its file's name (a title's `:` or `/` written as `_` there stays so).

## Settings

| Setting | Default | |
|---|---|---|
| `graphs` | `[]` | Folders that are graphs without a marker: a path, or `{"path": …, "kind": "logseq"}` / `"obsidian"` |
| `obsidian.daily_notes` | `""` | The daily notes' folder when `.obsidian/daily-notes.json` does not say |
| `obsidian.daily_format` | `YYYY-MM-DD` | Their names, in Moment's tokens |
| `obsidian.new_notes` | `""` | The folder of new notes when `.obsidian/app.json` does not say |
| `obsidian.link_style` | `shortest` | `shortest`, `relative`, `absolute` or `markdown`, when `app.json` does not say |
| `glyphs` | `unicode` | `▾ •` or `v *` for a terminal font without them |
| `journals_shown` | `30` | The journals the Journals document shows |

## What it writes

With `fs:write:workspace`: a new journal made from a template, and only when no file is there; a block's `id::` when it is first referred to; the references to a page renamed; a task's keyword cycled from the Tasks or a Query document. Each file is read again right before, only the lines touched change, and a file Kalem holds with unsaved changes is never written. Every other new page opens as an empty document and is written when the user saves it. Nothing under `logseq/` or `.obsidian/`.

## Known differences

From Logseq:

- Whiteboards are listed with the pages and their links resolve, but Kalem does not draw them; following one shows the file to open in Logseq. Flashcards, Logseq's plugins and their syntax, and PDF highlights drawn over the PDF are not there; a PDF's highlights page (`hls__…`) is a page like another.
- Advanced queries (`#+BEGIN_QUERY`, Datalog) are not run. Of the simple ones, `(sample n)` takes the first n, not n at random; `created-at` and `updated-at` sort by a journal's day only, which file graphs keep; a clause Kalem does not read finds nothing and says so.
- An embed shows one line, not the block or page in full; a query shows one line in its note and its results as a document (Kalem's layers add no lines yet).
- No real-time sync: Syncthing or git on the folder, as Logseq's file version is used.
- The database version's graph itself does not open; its Markdown Mirror does, read only.
- The index follows saves and changes on disk, not unsaved typing. Recent Pages remembers this session's pages only.

From Obsidian:

- Canvases are listed with the notes and their links resolve; following one shows the file to open in Obsidian.
- Community plugins' syntax is not read, but Dataview's inline fields (`key:: value`), whose keys are dimmed.
- `.obsidian/` is read for three files only (above), never written.
- A note is not renamed with its file yet: Kalem gives plugins no way to rename a file.

Both:

- The graph's folder must be a project of Kalem's (see above).

## Speed and size

Measured on a graph generated at the scale Logseq aims at (`tests/large.rs`): 10,000 pages, 1,000,000 blocks, 579,000 references, 49 MB of text, one page referred to 1,000 times. Native numbers are from an optimized build on an Apple M1 Max; "in Kalem" is the plugin as WebAssembly in Kalem's release build, through `kalem run`.

| | Native | Budget |
|---|---|---|
| The index built | 0.8 s | 2 s |
| The memory it holds (64-bit; less in WebAssembly's 32 bits) | 287 MB | 300 MB |
| A saved page indexed again | 0.4 ms | 10 ms |
| The backlinks panel of the page with 1,000 references | 2 ms | 50 ms |
| Its unlinked references, when opened | 280 ms | 500 ms |
| The layer over a journal of 5,000 blocks, at each keystroke | 8 ms | 50 ms |
| The Tasks document, 200,000 tasks | 350 ms | 500 ms |
| A query over the graph | 20 ms | 500 ms |
| In Kalem: the graph indexed and All pages printed | 1.9 s, 340 MB for the whole process | |

The plugin asks Kalem for a viewer's limits, 1 GB and 10 s a call (`limits` in `plugin.json`): an extension's own, 64 MB and 100 ms, stop it at the first note of a graph of some thousand pages. The first command or note of a large graph waits for its index while Kalem runs the call; a save then changes only the saved file's entries in the index (a page renamed, or a file added or removed, derives the names again, as fast as the first build's last step). The panel's unlinked references are searched for when they are opened, as Logseq does.

## Tests

`cargo test -p kalem-plugin-graph`:

- the library's unit tests;
- `tests/conformance.rs` over three corpus graphs written for this repository and licensed as it is (`corpus/logseq-md`, `corpus/logseq-org`, `corpus/obsidian`): settings, page names and every resolution rule, blocks, tasks, references, templates, queries, the documents, every corpus file scanned whole, every block folded and unfolded back to the same bytes, and an index kept by saves equal to one built afresh after every kind of edit of every file;
- `tests/snapshots.rs`: each document of each corpus as written, against `tests/snapshots/` (`UPDATE_SNAPSHOTS=1` writes them anew); with `KALEM=path/to/kalem`, the ignored `kalem_run_prints_them` compares what `kalem run` prints;
- `tests/robust.rs`: a configuration that does not parse, a file not in UTF-8 and one over 16 MB give one notice; embeds of themselves; every note of the corpora broken a few thousand ways through the scanner, the layer, the queries and every editing command without a panic (`ROBUST_ROUNDS` and `ROBUST_SEED` search longer);
- `tests/large.rs`, ignored: the measurements above, `cargo test --release -p kalem-plugin-graph --test large -- --ignored --nocapture`; `GRAPH_DIR=DIR` with `write_the_large_graph` writes the graph for Kalem.
