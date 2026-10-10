# Note graphs (`graph`)

A Logseq graph or an Obsidian vault in [Kalem](https://github.com/getkalem/kalem), opened as itself: its pages, journals, links, block references, tags and tasks indexed, what links to a page shown beside it, today's journal a key away. Nothing is converted: Logseq's `logseq/` and Obsidian's `.obsidian/` are never written, and a note is written only when the user saves it or asks for a new journal. The work list is [`graph_todo.md`](graph_todo.md).

**Status: early** (GR1 to GR3 of the list). The plugin finds a graph, indexes it and gives the commands, the backlinks panel and the documents below. The notes themselves still open in Kalem's own Markdown and Org modes: wiki links show as links, but Logseq's `id::` and `collapsed::` lines and a bare `((uuid))` stay visible, and blocks do not fold as Logseq folds them. Drawing a note as Logseq and Obsidian draw it needs a mode from a plugin, which Kalem's plugin API does not have yet (GR4).

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
| `SPC n r o` | Follow Reference | The `[[page]]`, `#tag`, `((uuid))`, `{{embed}}`, `[[note#Heading]]` or `[[note#^id]]` under the cursor; a page not written yet opens as a new file, written when saved |
| `SPC n r f` | Find Page | Every page by title, aliases in the detail; "Create a page…" first |
| `SPC n r i` | Insert Link | A link to the chosen page, written as the graph writes links (Logseq's `[[Title]]`; Obsidian's shortest path, or as `.obsidian/app.json` says) |
| `SPC n r b` | Insert Block Reference | `((uuid))` or `[[note#^id]]` of a block that has an id |
| `SPC n r d t`, `d y`, `d m` | Today's, Yesterday's, Tomorrow's Journal | Opens the journal; one not written yet is made from the graph's journal template (Logseq's `:default-templates {:journals …}`, Obsidian's daily notes template) |
| `SPC n r d d` | Journal of a Date | `2026-10-03`, `oct 3`, `yesterday`, `-3`, `monday` |
| `SPC n r d n`, `d p` | Next, Previous Journal | The next and previous journal that exists |
| `SPC n r p` | All Pages | Every page with its links and blocks, then the pages only referenced |
| `SPC n r j` | Journals | The last thirty journals, newest first, with their blocks |
| `SPC n r T` | Tags | Every tag with how often it is used |
| `SPC n r t` | Tasks | NOW and DOING, then LATER and TODO by priority, then WAITING; what is scheduled or due within a week |
| `SPC n r g` | Graph | The pages as a tree of namespaces with their links in and out, then the orphans |
| `SPC n r s` | Index Again | Reads the graph again |

The status bar shows the graph of the current note and its pages (`⌬ notes · 1,240 pages`); a click lists them.

**Today's date.** Kalem gives a plugin no clock yet, so the first journal command of a session asks for today's date (the newest journal's date offered) and keeps it until Kalem quits. Asked once; the list's K9 removes the question.

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

Only a new journal made from a template, and only when no file is there (`fs:write:workspace`); every other new page opens as an empty document and is written when the user saves it. Nothing under `logseq/` or `.obsidian/`.

## Known differences

- The notes are drawn by Kalem's Markdown and Org modes until the plugin's mode (GR4 to GR8): Logseq's property lines and block ids show, embeds show their source, callouts are quotes.
- Logseq's queries are not run yet (GR9); whiteboards, canvases, flashcards, PDF highlights, Datalog queries and the database version's SQLite graphs are out of scope.
- The index follows saves and changes on disk, not unsaved typing.
- The graph's folder must be a project of Kalem's (see above).

## Tests

`cargo test -p kalem-plugin-graph`: the library's unit tests, and `tests/conformance.rs` over three corpus graphs written for this repository and licensed as it is (`corpus/logseq-md`, `corpus/logseq-org`, `corpus/obsidian`): settings, page names and every resolution rule, blocks, tasks, references, templates, the documents, and every corpus file scanned whole.
