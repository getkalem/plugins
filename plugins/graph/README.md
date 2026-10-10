# Note graphs (`graph`)

A Logseq graph or an Obsidian vault in [Kalem](https://github.com/getkalem/kalem), opened as itself: its pages, journals, links, block references, tags and tasks indexed, what links to a page shown beside it, today's journal a key away. Nothing is converted: Logseq's `logseq/` and Obsidian's `.obsidian/` are never written, and a note is written only when the user saves it or asks for a new journal. The work list is [`graph_todo.md`](graph_todo.md).

**Status: early** (GR1 to GR4 and GR9 to GR11 of the list, GR5 to GR8 in part). The plugin finds a graph, indexes it and gives the commands, the backlinks panel and the documents below. The notes open in Kalem's own Markdown and Org modes; with a Kalem that has layers (plugin API 0.2.10's `layer`, on Kalem's branch `graph-mode` until it is merged) the plugin's layer draws them as Logseq and Obsidian do, below. Without it, wiki links show as links but Logseq's `id::` and `collapsed::` lines and a bare `((uuid))` stay visible.

## What it needs

- **The graph's folder is one of Kalem's projects** (Projects: Add Project). Kalem lets a plugin read only its projects' folders (`fs:read:workspace`); a note opened from a folder that is no project is not seen as a graph's.
- A Logseq graph is found by its `logseq/config.edn`, an Obsidian vault by its `.obsidian/` folder, from the folder of any note opened in it upwards. A folder without either is a graph when the setting `graphs` names it.
- A Kalem with plugin API 0.2.8 (checked with Kalem 0.6.6).

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
| `SPC n s` (in a graph) | Search the Graph | Kalem's search over the graph's folder, without `logseq/`, `.obsidian/`, `.trash/` and the hidden folders; elsewhere `SPC n s` searches Kalem's notes folder |
| `SPC n S` (in a graph) | Find Heading | The graph's headings, and the top-level blocks of its pages (not its journals'), each with its page |
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

In a note of a graph (a document the plugin's layer serves, Kalem's when-clause key `editorLayer`):

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

The layer is built only against a Kalem whose plugin API has it (0.2.10): `RUSTFLAGS="--cfg kalem_layer"` and a `kalem-plugin` with the feature `layer`. Built against Kalem's `main` today, the plugin has no layer and draws nothing differently; the editing commands work, their keys in notes need the same Kalem (`editorLayer`).

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

- Without layers (a Kalem before them) the notes are drawn by Kalem's Markdown and Org alone: Logseq's property lines and block ids show, embeds show their source, callouts are quotes. With them, embeds show one line, not the block or page in full, and callouts are quotes with their title.
- Advanced queries (`#+BEGIN_QUERY`, Datalog) are not run. Of the simple ones, `(sample n)` takes the first n, not n at random; `created-at` and `updated-at` sort by a journal's day only, which file graphs keep; a clause Kalem does not read finds nothing and says so.
- Whiteboards, canvases, flashcards, PDF highlights and the database version's SQLite graphs are out of scope.
- The index follows saves and changes on disk, not unsaved typing.
- Recent Pages remembers the pages of this session only; Logseq keeps its list across sessions.
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
