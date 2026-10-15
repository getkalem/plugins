# Note graphs (`graph`)

A Logseq graph or an Obsidian vault in [Kalem](https://github.com/getkalem/kalem), opened as itself: its pages, journals, links, block references, tags and tasks indexed, what links to a page shown beside it, today's journal a key away. Nothing is converted: Logseq's `logseq/` and Obsidian's `.obsidian/` are never written, and a note is written only when the user saves it or runs a command that writes one (see [What it writes](#what-it-writes)). The work list is [`graph_todo.md`](graph_todo.md).

**Status: 0.17.0**, for Kalem 0.6.31 and later (plugin API 0.2.19; the reference counts need Kalem 0.6.33 or later). 0.17.0 shows a block's number of references at the end of its first line, as Logseq does (each referring block once, an embed too, none inside an embed).

0.16.0 has everything but what 0.17.0 brings; it draws a Logseq whiteboard's text at the size Logseq gives it (its font size, else its scale level), in the window; the terminal keeps one character a cell.

0.15.0, for Kalem 0.6.30 (plugin API 0.2.18), has everything but what 0.16.0 brings; it edits an embed's blocks as an outline where they show (Enter makes a block, Tab and Shift+Tab indent and outdent it with its children, as in Logseq), shows a block's embed with the block itself first, and has the backlinks panel, the status bar and Recent Pages follow the note in front, a switch back to one open already too.

0.14.1, for Kalem 0.6.28 and later (plugin API 0.2.17), has everything but what 0.15.0 brings; it draws whiteboards as Logseq 0.10.9 draws its own (pictures kept in the file, text sizes, untitled boards), keeps a note opened while its graph indexes among the recent pages, and gives Zoom into Block Logseq's breadcrumb as its label (drawn by Kalem 0.6.29 and later). The plugin finds a graph, indexes it and gives the commands, the backlinks panel and the documents below, the graph drawn among them; it edits a graph as an outliner, runs Logseq's simple and advanced queries and searches the graph; its layer draws a note as Logseq or Obsidian draws it, embeds and query results in place, and its completer offers pages, tags, blocks and properties as the user types (below). Its list of tasks is [`graph_todo.md`](graph_todo.md).

0.13.0, for Kalem 0.6.27 (plugin API 0.2.16), has everything but what 0.14.0 brings: the pages opened last and each graph's graph view toggles and sort kept from one session to the next, and a graph indexed in slices of 30 ms so that a large one opens at once and keys answer while it indexes.

0.12.0, for Kalem 0.6.15 and later (plugin API 0.2.13; Zoom into Block from Kalem 0.6.24, typing into an embed from 0.6.26), has everything but what 0.13.0 brings: Logseq's whiteboards and Obsidian's canvases drawn (read only), a shape followed to its page, block or file.

0.11.0 has everything but what 0.12.0 brings: an embed's or a query result's blocks typed into where they show, written back into their notes (with Kalem 0.6.26 and later; a click or Edit Line Under Line, Enter to write, Escape to drop).

0.10.0 has everything but what 0.11.0 brings: Logseq's zoom into a block (Zoom into Block and Zoom Out, `SPC m s n` and `SPC b -` in Markdown notes), a collapsed block's children shown, with Kalem 0.6.24 and later.

0.9.0 has everything but what 0.10.0 brings: Logseq's flashcards (Review Flashcards, `SPC n r c`, scored and scheduled as Logseq does), unlinked mentions found in a block's every line, `(sample n)`, Logseq's clock kept on a task started and done, Insert Date (`SPC m d t`), pages for properties' names and Obsidian's tags in the graph view, and `((` and block ids in Org notes (`:ID:` in the headline's drawer).

0.8.0 has everything but what 0.9.0 brings: pages sorted by when Logseq last changed them and queries by created-at and updated-at (Logseq's pages-metadata.edn), settings files reloaded on change, `/Template` completion, every block offered after `((`, and `J` and `O` in the graph view for the journals and the orphans.

0.7.0 has everything but what 0.8.0 brings: the Graph button and menu on a graph's folder in the file manager (with Kalem 0.6.19 and later), queries and references checked against Logseq 0.10.9 on an Org graph too (headline tags, `#+title:` lines, a link to an alias counted as the alias's in queries and the graph view), and the backlinks and local graph of a page opened before its file is written.

0.6.0 has everything but what 0.7.0 brings: Graph View drawing the pages and links Logseq's graph view draws (aliases, whiteboards, a namespace's pages linked to their parent, Logseq's `:graph/settings` or Obsidian's `graph.json` followed), checked against Logseq 0.10.9's, and the large graph's time budgets.

0.5.1 has everything but what 0.6.0 brings: the graph's menu behind a Graph button (`SPC n r m`, the status bar item), with its favorites, recent pages, whiteboards and other graphs; queries and references answered as Logseq 0.10.9 answers them (a page's properties a block of their own, a namespace's parents referred to, a Logseq graph's notes read anywhere in its folder); a folded block's bullet drawn as ▸; and a PDF's highlights page named for its PDF in a drawn graph.

0.4.0, for Kalem 0.6.14, has everything but what 0.5.0 and plugin API 0.2.13 bring: embeds and query results drawn under their line, Logseq's advanced queries, a graph indexed once its folder is added to the projects, and the pages that Logseq, Obsidian, a sync or git write, rename or remove while Kalem is open (0.4.0 sees them only if Kalem has them open, else once Kalem starts again).

0.3.1, for Kalem 0.6.13, has everything but what 0.4.0 and plugin API 0.2.12 bring: the graph drawn (Graph View, Local Graph), Rename Page renaming the page's file, a Ctrl-click and `Alt+Enter` opening links by the graph's rules, completion after `::`, `/` and a vault's `[[note#`, and the time in journal templates.

0.2.0, for Kalem 0.6.11 and 0.6.12, has everything but what 0.3.0 and plugin API 0.2.11 bring: completion, Enter making blocks as Logseq's, Open Link by the graph's rules, `Alt+]` and `Alt+[`, the keys in a note before the Vim profile's (`SPC m t`, `SPC m p p`, `SPC m d s`, `SPC m d d`, `SPC n s`), All pages sorted, Block References, highlights, numbered lists and admonitions drawn, Logseq's keywords styled in Org notes, the commands seeing unsaved typing; and today's date is asked once a session.

0.1.0, for Kalem 0.6.8 to 0.6.10, has everything but what plugin API 0.2.10 brings: with 0.1.0 the notes show as they are written (Logseq's `id::` and `collapsed::` lines, a bare `((uuid))`, `{{embed …}}` and `{{query …}}` as text, an Obsidian callout as a quote), the outliner's commands run from the Graph menu and the palette rather than from keys in a note, Search the Graph searches `logseq/` and `.obsidian/` too, a whiteboard or canvas is named in a notice rather than shown in the file manager, and today's date is asked once a session.

## Getting started

1. Open a page of the graph in Kalem: `kalem gui ~/notes/pages/Kalem.md`, or File › Open. Without the plugin, Kalem says in the status bar that the Note graphs plugin serves the folder; **Plugins: Install Suggested Plugin** in the palette installs it (or the Kalem menu's Browse Plugins, or `kalem plugin install graph` in a terminal). With Kalem 0.6.18 and later it starts at once, the note drawn as Logseq or Obsidian draws it; with an earlier Kalem, start Kalem again.
2. Kalem offers the graph's folder as a project, since the plugin reads only projects: Enter adds it, and the graph is indexed (the status bar says `⌬ notes · 20 pages`).
3. The **Graph** button, `SPC n r m` or a click on that status bar item opens the graph's menu, as Logseq's left sidebar: the journals, today's journal, the graph drawn, all pages, tags, tasks, search, the favorites and the pages opened last, each a key away. The rest is under **Graph** in the palette and the menu bar, and below.

## Moving over

- **From Logseq (the file version, "OG").** Add the graph's folder as a project of Kalem's and open any page: nothing is converted, and `logseq/` is never written. Keep Logseq installed for as long as you like: both read and write the same files, and each reads again a file the other saved (save in one before editing the same page in the other). Kalem hears every file Logseq writes, renames or removes in the graph while both are open (Kalem 0.6.15 and later; before, a page Logseq made was seen once Kalem started again or after Index Again); a page open in Kalem is read again when it has no changes of Kalem's; with unsaved changes, Logseq's are merged in when they are to other blocks (Kalem 0.6.17 and later), and Kalem asks which to keep only when both changed the same lines. Tested with Logseq 0.10.9 open on the same graph, both ways: blocks, pages made, renamed (by either) and folded, task keywords, priorities and block references written by Kalem, typing in both at once. Logseq writes a file again in its own way after another program renames a page it names (no line feed at the end, tabs before nested blocks), and keeps a block folded that another program unfolded until the graph is indexed again. Sync stays as it was: Syncthing, git or iCloud on the folder.
- **From Logseq's database version.** Its SQLite database does not open in Kalem. Its Markdown Mirror does: turn it on in Logseq's settings (the desktop app), and the folder `mirror/markdown/` in the graph's folder opens as a graph, read only: pages, journals, links, tags, tasks, properties (written there as `* key:: value`), queries and search. Logseq writes the mirror from its database and writes over any change, so Kalem never writes it and its editing commands say so.
- **From Obsidian.** Add the vault's folder as a project and open a note. Of `.obsidian/`, only `daily-notes.json`, `app.json`, `templates.json`, `bookmarks.json` and `graph.json` are read, for the daily notes, where new notes go, how links are written, the templates' folder, the bookmarks in the graph's menu and what the graph view shows; nothing under it is written.

## What it needs

- **The graph's folder is one of Kalem's projects.** Kalem lets a plugin read only its projects' folders (`fs:read:workspace`). Opening a note of a graph outside the projects, Kalem offers to add its folder (Enter adds it, and the graph is indexed at once; with Kalem before 0.6.15, Projects: Add Project).
- A Logseq graph is found by its `logseq/config.edn`, an Obsidian vault by its `.obsidian/` folder, a Logseq database graph's mirror by its `mirror/markdown/.index.edn`, from the folder of any note opened in it upwards. A folder without any is a graph when the setting `graphs` names it: a Logseq graph when it has `pages/` or `journals/`, else a vault, unless the setting gives its kind.
- Kalem 0.6.31 or later (plugin API 0.2.19), 0.6.33 or later for the reference counts; 0.15.0 runs on Kalem 0.6.30, 0.14.1 on Kalem 0.6.28 and 0.6.29, 0.13.0 on Kalem 0.6.27, 0.12.0 on Kalem 0.6.15 and later (0.6.24 or later for Zoom into Block, 0.6.26 or later for typing into an embed); 0.4.0 runs on Kalem 0.6.14, 0.3.1 on 0.6.13, 0.2.0 on 0.6.11 and 0.6.12, 0.1.0 on 0.6.8 to 0.6.10, without the layer.
- Room to index a large graph: the plugin asks Kalem for 1 GB and 10 s a call (see Speed and size).

## Commands and keys

The keys are Doom Emacs's org-roam map, `SPC n r`, in the Vim profile; every command is in the palette under **Graph**, and most are in the **Graph** menu.

| Keys | Command | What it does |
|---|---|---|
| `SPC n r m`; the **Graph** button; a click on the status bar item | Menu | The graph's menu, as Logseq's and Obsidian's left sidebar: Journals, Today's journal, Graph view, All pages, Tags, Tasks, Flashcards (a Logseq graph's), Search and Find or create a page, each a key away in it (`j`, `.`, `g`, `p`, `T`, `t`, `c`, `/`, `f`) and Enter or a click on its line; then the favorites (Logseq's `:favorites` in `logseq/config.edn`, Obsidian's bookmarks), the pages opened last, the whiteboards or canvases, and the other graphs, each opened by Enter. The Graph button stands in the window's toolbar and at the foot of the terminal's sidebar in a graph's notes and the plugin's documents, and on the graph's folder in the file manager with Kalem 0.6.19 and later (`folderLayer`) |
| `SPC n r r` | Backlinks Panel | The side panel: the page's linked references grouped by page, then its unlinked mentions, each line as its note draws it (links and tags without brackets, a block reference as its block's text, an embed as what it embeds); a click opens the line |
| `SPC n r R` | Backlinks | The same as a document, with the references to the page's blocks; Enter on a line opens it |
| `SPC o l` in a note; Enter (Vim's normal mode) and `SPC m g x` in a Logseq Org note; `Alt+Enter` while typing in Markdown; a Ctrl-click in the window (Command-click on macOS) | Open Link | What Follow Reference follows, by the graph's rules (`[[page]]` in Org opens the page, not a heading of the file); any other link as the mode opens it |
| `SPC n r o` | Follow Reference | The `[[page]]`, `#tag`, `((uuid))`, `{{embed}}`, `[[note#Heading]]` or `[[note#^id]]` under the cursor; a page not written yet opens as a new file, written when saved. On a `{{query …}}`'s line, its results (below) |
| `SPC n r f` | Find Page | Every page by title, aliases in the detail; "Create a page…" first |
| `SPC n r i` | Insert Link | A link to the chosen page, written as the graph writes links (Logseq's `[[Title]]`; Obsidian's shortest path, or as `.obsidian/app.json` says) |
| `SPC n r b` | Insert Block Reference | `((uuid))` or `[[note#^id]]` of a block: a Logseq block without an id gets one (below); a vault's block needs its `^id` |
| `SPC n r d t`, `d y`, `d m` | Today's, Yesterday's, Tomorrow's Journal | Opens the journal; one not written yet is made from the graph's journal template (Logseq's `:default-templates {:journals …}`, Obsidian's daily notes template) |
| `SPC n r d d` | Journal of a Date | `2026-10-03`, `oct 3`, `yesterday`, `-3`, `monday` |
| `SPC n r d f`, `d b`; `Alt+]`, `Alt+[` in a note | Next, Previous Journal | The next and previous journal that exists (from a page, counting from today). A terminal without the kitty keyboard protocol cannot send `Alt+[` |
| `SPC n r p` | All Pages | Every page with its links and blocks (a PDF's highlights named for the PDF), the whiteboards or canvases, then the pages only referenced; `s` sorts it by title, links, blocks or when Logseq last changed the page (`logseq/pages-metadata.edn`) |
| | Block References | What refers to the block at the cursor, as a document |
| `SPC n r j` | Journals | The last thirty journals, newest first, with their blocks |
| `SPC n r T` | Tags | Every tag with how often it is used |
| `SPC n r t` | Tasks | NOW and DOING, then LATER and TODO by priority, then WAITING; what is scheduled or due within a week. Enter opens the task, `t` cycles its keyword in its file |
| `SPC n r c` | Review Flashcards | Logseq's flashcards: the blocks tagged `#card` that are due, new ones first, one at a time. `a` shows the answer (the block's children, its `{{cloze …}}`); `f`, `t` and `r` score the card as forgotten, remembered with effort or remembered, and write Logseq's `card-*` properties into its block, its next review worked out as Logseq works it out (SM-5, with `:srs/learning-fraction`, `:srs/initial-interval` and the factors Logseq learnt in `logseq/srs-of-matrix.edn`); `n` passes over it. Enter opens the card |
| `SPC n r q` | Run Query | A Logseq simple query typed, its results as a document |
| `SPC n r l` | Recent Pages | The graph's pages opened last, kept from one session to the next with Kalem 0.6.28 and later (plugin API 0.2.17), as are each graph's graph view toggles (`J`, `O`) and its sort of All pages (`s`) |
| `SPC n r a` | Random Page | A page of the graph at random |
| `SPC n s` (in a graph) | Search the Graph | Kalem's search over the graph's folder, without `logseq/`, `.obsidian/`, `.trash/` and the hidden folders; elsewhere `SPC n s` searches Kalem's notes folder |
| `SPC n S` (in a graph) | Find Heading | The graph's headings, and the top-level blocks of its pages (not its journals'), each with its page |
| `SPC n r v` | Graph View | The graph drawn: its pages as dots colored by namespace and sized by their links, the links as lines (Logseq's journals left out, as Logseq leaves them out). Kalem lays it out and draws it; drag to pan, the wheel or `+` and `-` to zoom, a click or Tab to choose a page, a double click or Enter to open it, `0` to fit. The terminal draws it with Braille dots; Show Source lists the pages |
| `SPC n r V` | Local Graph | The note's page in the middle and the pages two links from it, journals too |
| `SPC n r g` | Graph | The pages as a tree of namespaces with their links in and out, then the pages named without a link and how often, then the orphans |
| `SPC n r s` | Index Again | Reads the graph again |

The status bar shows the graph of the current note and its pages (`⌬ notes · 1240 pages`; `graph:` for `⌬` with the setting `glyphs = "ascii"`); a click opens the graph's menu.

**Whiteboards and canvases.** A Logseq whiteboard (`whiteboards/*.edn`) and an Obsidian canvas (`*.canvas`) are pages: links to them resolve, and they show in All pages. With Kalem 0.6.27 and later (plugin API 0.2.16) following one draws it, read only, as its application places its shapes: boxes, ellipses, frames, text, pen strokes, pictures from the graph's assets, a Logseq portal as its page's title or its block's text, an Obsidian note, link or group, and the connectors between them with their labels and arrows. Drag or the arrows pan, the wheel or `+` and `-` zoom, `0` fits; Tab chooses the next shape that leads somewhere and Enter (or a double click) opens it: a portal's page or block, a canvas's note, a picture's file. Show Source lists the shapes' text in reading order, each opening the same way. Changing a whiteboard is done in its application, which Kalem reads again.

**From the command line.** With a Kalem that has batch mode, the documents print without a window:

```bash
kalem run graph graph.pages ~/notes
```

`graph.tasks`, `graph.tags`, `graph.journals` and `graph.graph` take the graph's folder too, `graph.backlinksDocument` a page's file; `--format json` gives the title, kind and text.

**Today's date.** Kalem's plugin API 0.2.11 gives the plugin the local time (`clock-2`): Today's Journal opens the user's day, the Tasks document and the queries count from it, and a journal template's `<% time %>` (Logseq) or `{{time}}` (Obsidian, in its format) is the local time.

## Editing as an outliner

In a note of a graph (a document the plugin's layer serves, Kalem's when-clause key `editorLayer`), whose keys come before the profile's (Kalem 0.6.13):

| Keys | Command | What it does |
|---|---|---|
| Enter (typing, in a Logseq note in Markdown) | New Block | As Logseq's Enter: a new block after the block's properties and dates, the first child when its children are shown, the block split at the cursor (its properties staying with the first part), an empty child outdented; elsewhere Markdown's Enter |
| `SPC m t` in Org, `SPC m t t` in Markdown | Cycle Task | The block's keyword as Logseq cycles it: `TODO` to `DOING`, `LATER` to `NOW`, both to `DONE`, `DONE` to none, and none (or `WAITING`, `CANCELED`) to `LATER` (`TODO` with `:preferred-workflow :todo`); in a vault the check box: none, `[ ]`, `[x]`, none. In Markdown, `SPC m t` is Doom's toggles (`SPC m t x` the check box, `SPC m t m` the markup); in a Logseq graph that tracks time (`:feature/enable-timetracking?`, Logseq's default), a task started (`NOW`, `DOING`) gets a `CLOCK:` line in its `:LOGBOOK:` and one done (or back to `LATER`, `TODO`) its clock closed with the time it ran, as Logseq writes them |
| `SPC m p p` | Set Priority | `[#A]`, `[#B]`, `[#C]` or none, after the keyword (Doom's Org key; `SPC m p u` and `SPC m p d` stay Org's); not in a vault, whose notes have no priorities |
| `SPC m d s`, `SPC m d d` | Schedule, Deadline | `SCHEDULED: <2026-10-12 Mon>` after the block's first line and properties (after an Org headline), replaced where it is, removed when the answer is empty; not in a vault |
| `SPC m d t` in Markdown | Insert Date | Logseq's date picker: a day typed (`today`, `friday`, `+3`, `oct 12`, `2026-10-20`), its journal's link written at the cursor (`[[Oct 13th, 2026]]`) |
| `Alt+Shift+Up`, `Alt+Shift+Down` | Move Block Up, Down | The block with the blocks under it past its sibling |
| `Tab`, `Shift+Tab` (typing, in the Vim profile), `Alt+Shift+Right`, `Alt+Shift+Left` | Indent, Outdent Block | The block with the blocks under it a level deeper (under its previous sibling) or out (the blocks after it at its level becoming its children, as Logseq outdents); a tab or the note's own spaces |
| `Tab` (Vim's normal mode), `SPC m z` | Fold Block | In a Logseq note in Markdown, `collapsed:: true` written or taken away, as Logseq folds (elsewhere, Kalem's own fold); the layer hides the blocks under a folded block until the cursor goes into them |
| `SPC m s n`, `SPC b -` in Markdown | Zoom into Block | Logseq's zoom into a block: the note narrowed to the block at the cursor and the blocks under it, the rest hidden and out of the cursor's reach (Kalem 0.6.24 and later, its Narrow to Lines; an older Kalem says "Unknown command"), under a line naming the page and the blocks above it, as Logseq's breadcrumb (Kalem 0.6.29 and later); in Org notes Kalem's own `SPC m s n` narrows to the headline |
| `SPC m s N`, `SPC b -` | Zoom Out | The whole note again (Kalem's Widen) |
| | Rename Page | Every reference to the page by its name rewritten in every file of the graph (aliases kept), and the page's file renamed as the application renames it: a vault's note always, a Logseq page named by its file (one named by `title::` keeps its file, its `title::` changed). Kalem's document of the file follows it; a file Kalem holds with unsaved changes stops it before anything is written |

Insert Block Reference offers every block: one without an id gets its `id::` written where Logseq writes it, in its file, or by the editor when it is in the document being edited; a file Kalem holds with unsaved changes is not written. Each command is one undo step.

## How notes are drawn (layers)

Kalem's Markdown and Org draw the note; the plugin's layer changes what Logseq and Obsidian show differently, away from the cursor (the cursor's line shows the source, as Kalem's markers do; Show Source shows the file as it is):

| | Logseq | Obsidian |
|---|---|---|
| Hidden | a block's `id::`, `collapsed::`, `heading::` and the other properties Logseq hides | a `^id` at a line's end, `%%comments%%` |
| Shown as other text | `((uuid))` as its block's text; `{{embed …}}` as `↳` and the block or the page; `{{query …}}` and `#+BEGIN_QUERY` as `⌕`, how many blocks or pages the query finds, and its results under the line; `#+BEGIN_NOTE`, `TIP`, `IMPORTANT`, `CAUTION`, `WARNING`, `PINNED` as the admonition's icon and name, its `#+END_` line hidden; a numbered block's bullet (`logseq.order-list-type:: number`) as `1.`, nested `a.` and `I.` | a callout's `[!note]` as its icon and name; `![[note]]` as `↳` and the note or the block |
| Styled | task keywords and priorities as Org's (in Org notes `LATER`, `NOW`, `DOING`, `WAITING`, `CANCELED` too), dates, `#tags`, `^^highlights^^`, property keys dimmed | `==highlights==`, `#tags`, Dataview's keys dimmed |
| Folded | `:LOGBOOK:` to its first line; the blocks under a block with `collapsed:: true` hidden | a callout with `-` to its first line |
| Added | a block's number of references at the end of its first line, each block referring to it once, as Logseq shows it beside the block (with Kalem's plugin API 0.2.20) | |

The layer uses plugin API 0.2.20's `layer-5`, which the plugin is built with (`kalem-plugin`'s feature `layer-5`, which brings `layer`, `completer` and the layers between too); an earlier Kalem asks the layer it knows, without what came after it.

## Completion

As the user types in a note of a graph, Kalem's completion menu offers (plugin API 0.2.11's `completer`; Tab or Enter takes an item, Ctrl+Space asks anywhere):

| After | Offers | Writes |
|---|---|---|
| `[[` | The pages and aliases whose title holds what is typed, journals last | The link as the graph writes links (Logseq's `[[Title]]`, Org's `file:` form when `config.edn` asks for it; Obsidian's shortest path, or as `.obsidian/app.json` says), closed unless `]]` follows |
| `#` (after a space or at a line's start) | Logseq's pages, the most used first; Obsidian's tags | `#name`, `#[[two words]]` |
| `((`, in a Logseq graph | The blocks that have an id, by their text; once something is typed, as Logseq offers them, every block holding it too, those with an id first | `((uuid))`; a block without an id is given one, written into its file as Logseq writes one (`id::` in Markdown, `:ID:` in an Org headline's `:PROPERTIES:` drawer) when the note with the reference is saved (a file Kalem holds with unsaved changes, at its own save) |
| `<`, in a Logseq note in Markdown | Logseq's blocks: Quote, Src, Query, Note, Tip, Important, Caution, Pinned, Warning, Example, Export, Verse, Center, Comment… | `#+BEGIN_NOTE` … `#+END_NOTE` with the block's indentation, the cursor inside |
| `::` at a block's start | The properties the graph uses, the most used first | `key:: ` |
| `key::` | The values that property has, one by one for `tags::`, `alias::` and the comma properties | the value |
| `/` (after a space or at a line's start), in a Logseq note in Markdown | Logseq's commands that write text: TODO, LATER, NOW… and A, B, C and h1 to h6 at a block's start; Page and Block reference and embed, Link, Today, Yesterday, Tomorrow, Current time, Query, Code block…; at a block's start, the graph's templates (`Template: NAME`, Logseq's `/Template`) | what Logseq writes, the cursor where Logseq puts it; a template's blocks in the block's place, under its indent, its variables (`<% today %>`, `<% current page %>`…) filled in |
| `[[note#`, `[[note#^`, in a vault | The note's headings, its blocks with an id | `[[note#Heading]]`, `[[note#^id]]` |

Where it offers something, Kalem's own completion of Markdown files after `[[` and of the document's words is left out. The items come from the index: before the graph is indexed, Kalem's own completion answers.

## Queries

Logseq's simple queries run over the index: `[[page]]` and `#tag` (the page's blocks, the blocks referring to it and the blocks under them, as Logseq's path references count them), `"text"` and `(full-text-search "text")`, `(and …)`, `(or …)`, `(not …)`, `(task NOW DOING)`, `(priority A B)`, `(between -7d today)` (a journal's day, `SCHEDULED:` or `DEADLINE:`; `today`, `yesterday`, `tomorrow`, `±Nd`, `±Nw`, `±Nm`, `±Ny`, `[[Oct 3rd, 2026]]`, `2026-10-03`), `(property key value)`, `(page-property key value)`, `(page-tags tag)`, `(page name)`, `(namespace name)` (its children, not theirs), `(sort-by key desc)` and `(sample n)`. A query of page clauses only finds pages. As in Logseq, a page's properties at its top are a block of their own (its pre-block): `{{query [[book]]}}` finds the pages with `type:: [[book]]`.

The answers are Logseq's: a test (`tests/logseq_parity.rs`) runs simple and advanced queries and reads each page's references on the corpus, and compares them with what Logseq 0.10.9 answered there through its API (`tests/logseq/answers-0.10.9.json`).

The results are grouped by page, journals newest first, with each block's first line; with `query-table:: true` on the query's block they are a table of the columns `query-properties::` names (`[:block :page]` by default), sorted by `query-sort-by::` and `query-sort-desc::`. Enter opens a block, `t` cycles its keyword. The document and the line in the note are computed again when the graph changes.

Advanced queries (`#+BEGIN_QUERY` … `#+END_QUERY`, Datalog) run too, as Logseq's file version runs them: the note shows the query's title and how many it finds, its results under it, its EDN hidden until the cursor goes into it, and Follow Reference on it opens the results. The graph is seen as Logseq's schema has it (`:block/name`, `:block/original-name`, `:block/journal?`, `:block/journal-day`, `:block/properties`, `:block/tags`, `:block/alias`, `:block/namespace` of a page; `:block/content`, `:block/marker`, `:block/priority`, `:block/page`, `:block/parent`, `:block/refs`, `:block/path-refs`, `:block/scheduled`, `:block/deadline`, `:block/uuid`, `:block/pre-block?` of a block, a page's pre-block among them). `:where` takes data patterns, `not`, `not-join`, `or`, `or-join`, `and`, the comparisons, `contains?`, `clojure.string/includes?`, `starts-with?`, `ends-with?` and `blank?`, `get`, `get-else`, `missing?`, `str`, and Logseq's rules (`page-ref`, `task`, `priority`, `property`, `page-property`, `page-tags`, `between`, `namespace`, `page`, `block-content`); `:inputs` takes `:today`, `:yesterday`, `:tomorrow`, `:-7d`, `:+7d`, `:7d-before`, `:7d-after` and `:current-page`; `:find` a variable, `(pull ?b [*])` or `(count ?b)`, other values as a table.

## What is read

| | Logseq | Obsidian |
|---|---|---|
| Settings | `logseq/config.edn`: format, workflow, folders, journal file and title formats, file name format, hidden folders, journal template, hidden and comma properties, `:favorites`, `:graph/settings` | `.obsidian/daily-notes.json`, `app.json`, `templates.json`, `bookmarks.json`, `graph.json` (`showOrphans`, `hideUnresolved`), read-only; the plugin's settings and then Obsidian's defaults when they are missing |
| Notes | Every `.md` and `.org` in the graph's folder, as Logseq reads them (a note in `assets/` too), but `logseq/`, hidden folders, `node_modules` and `:hidden`; journals by their folder | Every `.md` but `.obsidian/`, `.trash/`, hidden folders and `node_modules` |
| Pages | Title from `title::` (or `#+title:`), else the file name decoded (`a___b` is `a/b`, `%3F` is `?`); journals by date; aliases resolve to the page; names compare ignoring case | A note per file, named by its path; `[[Name]]` resolves to the note in the linking note's folder first, else the shortest path; aliases name the note but link nowhere, as in Obsidian |
| Blocks | Every `- ` bullet (every headline in Org) with its depth, `id::`, `collapsed::`, task keyword, priority, `SCHEDULED:` and `DEADLINE:`, properties | Paragraphs, list items and headings; `^id` at a line's end; check boxes as tasks; Dataview's `key:: value` |
| References | `[[page]]`, `[label]([[page]])`, `#tag`, `#[[two words]]`, `((uuid))`, `{{embed …}}`, page properties' values (`alias::`, `tags::`, `:property/separated-by-commas`), `[[file:…][…]]` and a headline's tags (`:a:b:`) in Org; a link to a page of a namespace refers to its parents too (`[[a/b/c]]` to `a/b` and `a`), as in Logseq. A link to a page's alias is the page's in its backlinks, as Logseq's linked references count it, and the alias's own in queries and the graph view, as Logseq's do | `[[note]]`, `[[note\|text]]`, `[[note#Heading]]`, `[[note#^id]]`, `![[note]]`, Markdown links to `.md` files, `#tag` and `#nested/tag` (not `#2026`) |
| Not references | Code spans and code blocks, `#+BEGIN_SRC` in Org, the pages a `{{query …}}` names, a block of a template (`template::`) as a task | Code, `%%comments%%`, embeds of pictures |

A Logseq database graph's Markdown Mirror is read as a Logseq graph with Logseq's defaults, as Logseq's own description of it writes it (its ADR 0016 and `docs/logseq-markdown-syntax.md`): each file's first line, the page's `id::`, hidden; properties as `* key:: value` items, an open property's values as the items under it; a page's title as its file's name (a title's `:` or `/` written as `_` there stays so).

## Settings

| Setting | Default | |
|---|---|---|
| `graphs` | `[]` | Folders that are graphs without a marker: a path, or `{"path": …, "kind": "logseq"}` / `"obsidian"`; the path whole, as Kalem opens the notes (`/Users/me/notes`, not `~/notes`). Such a folder has the commands and documents, not the layer's drawing or completion (an empty `.obsidian/` in it gives both) |
| `obsidian.daily_notes` | `""` | The daily notes' folder when `.obsidian/daily-notes.json` does not say |
| `obsidian.daily_format` | `YYYY-MM-DD` | Their names, in Moment's tokens |
| `obsidian.new_notes` | `""` | The folder of new notes when `.obsidian/app.json` does not say |
| `obsidian.link_style` | `shortest` | `shortest`, `relative`, `absolute` or `markdown`, when `app.json` does not say |
| `glyphs` | `unicode` | `▾ •` or `v *` for a terminal font without them |
| `journals_shown` | `30` | The journals the Journals document shows |

## What it writes

With `fs:write:workspace`: a new journal made from a template, and only when no file is there; a block's `id::` when it is first referred to; the references to a page renamed, and its file renamed (moved, never deleted); a task's keyword cycled from the Tasks or a Query document (with Logseq's clock when the graph tracks time); a flashcard's `card-*` properties when it is scored. Each file is read again right before, only the lines touched change, and a file Kalem holds with unsaved changes is never written. Every other new page opens as an empty document and is written when the user saves it. Nothing under `logseq/` or `.obsidian/`. What is typed into a block where an embed or a query shows it is written by Kalem itself, into that block's page, with the same care (only a file in Kalem's projects, only where it still holds that block's line).

## Known differences

From Logseq:

- Whiteboards and canvases are drawn read only (Kalem 0.6.27 and later): they are changed in their application. Of Logseq's shapes, a polygon is drawn as a box, a web page's, a video's or a tweet's shape as a box with its address (Kalem does not open addresses yet), and a picture kept in the board itself rather than in the assets (as Logseq's own first whiteboard keeps them) as a box saying a picture is there. Text is drawn at the size Logseq gives it (xs to xl, its `fontSize`) in Kalem's window, with Kalem's plugin API 0.2.19 and later; the terminal writes a character a cell. Checked against the first whiteboard Logseq 0.10.9 draws, its tour: every shape and connector where Logseq places it. Logseq's plugins and their syntax, and PDF highlights drawn over the PDF are not there; a PDF's highlights page (`hls__…`) is a page like another.
- Flashcards are reviewed in a document, not Logseq's dialog. The factors Logseq learns from its reviews (`logseq/srs-of-matrix.edn`) are read and not written, as nothing under `logseq/` is: a review in Kalem follows what Logseq learnt without teaching it. Logseq's Reset card and its `{{cards …}}` macro's query are not there.
- Of advanced queries, `:result-transform`, `:view` and `:rules` of one's own (Clojure) are not run, nor functions beyond those above; the query says what it does not run. Of the simple ones, `(sample n)` takes n at random, the same n through a day (a query drawn again as one types keeps them); `created-at` and `updated-at` sort by when Logseq made and last changed the page, as `logseq/pages-metadata.edn` keeps it (a block by its page's), else by a journal's day; a clause Kalem does not read finds nothing and says so.
- An embed shows its block's children or its page's blocks under its line, and a query its results (24 lines at most, then how many more; Follow Reference opens them all), as text in their note's style; an embed inside an embed shows as written. With Kalem 0.6.26 and later (plugin API 0.2.15), a block shown there is typed into where it shows, as in Logseq: a click on its line (or the palette's Edit Line Under Line on the embed's line) shows its first line as its page holds it, Enter writes it there, Escape drops it, Up and Down go to the block before or after. A block's embed names the block's page on its line and shows the block under it, typed into as its children are. With Kalem 0.6.30 and later (plugin API 0.2.18), Enter there makes a new block, under the embed's own block or after the one typed into (with the text after the cursor), Tab puts a block under the one before it and Shift+Tab takes it out a step, each written into the block's page, as in Logseq; Shift+Tab never takes a block out of the embed.
- No real-time sync: Syncthing or git on the folder, as Logseq's file version is used.
- The database version's graph itself does not open; its Markdown Mirror does, read only.
- Of `/`'s commands, those that write text and the templates, not the date picker (Insert Date, `SPC m d t`, in its place) or the drawings.
- The graph view draws the pages and links Logseq's draws, aliases, whiteboards and a namespace's pages linked to their parent among them, checked against Logseq 0.10.9 on the corpus (`tests/logseq_parity.rs`), and shows what Logseq's `:graph/settings` says, as Logseq's toggles set it: journals (`:journal?`), pages linked to no other (`:orphan-pages?`), Logseq's own pages such as TODO and A (`:builtin-pages?`), and pages with `exclude-from-graph-view:: true` (`:excluded-pages?`), Logseq's defaults where it says nothing (orphans only). Properties' names are pages of their own, as Logseq makes them (`type`, `related`; `:property-pages/enabled?`, `:property-pages/excludelist`). It leaves out the page Logseq 0.10.9 makes by mistake of a block `{{embed ((…))}}` embeds. In the graph view, `J` and `O` show or leave out the journals and the orphans, as Logseq's toggles do, kept from one session to the next with Kalem 0.6.28 and later in Kalem's state, never in the graph's settings; it has no groups of one's own or forces to set, and a page's local graph shows all.
- The index follows saves, changes on disk (any file of the graph, written by Logseq, Obsidian, a sync or git, with Kalem 0.6.15 and later; the files Kalem holds open before) and, when a command runs in a note, its unsaved text; the panel and the documents are written again after a save. The graph's own settings (`logseq/config.edn`, Obsidian's `.obsidian/*.json`) are followed as its app writes them: a favorite added or the graph view's settings changed in Logseq show in Kalem. Recent Pages is kept from one session to the next with Kalem 0.6.28 and later; a page opened by two spellings of its folder (a link to it, or macOS's `/tmp` and `/private/tmp`) is listed twice, as a plugin cannot follow links.
- The panel, the status bar and Recent Pages follow the note in front with Kalem 0.6.30 and later (its plugin API 0.2.18, `focus`), a switch back to a note open already included (Find Page, the list of open files, another pane or window).

From Obsidian:

- Canvases are listed with the notes and their links resolve; following one draws it, read only, with Kalem 0.6.27 and later (above); it is changed in Obsidian.
- Community plugins' syntax is not read, but Dataview's inline fields (`key:: value`), whose keys are dimmed.
- `.obsidian/` is read for five files only (above), never written. The graph view hides orphans and notes not written, and draws tags as nodes, as `graph.json` says (`showOrphans`, `hideUnresolved`, `showTags`); it has no attachments as nodes, groups or forces to set.

Both:

- The graph's folder must be a project of Kalem's (see above).

## Speed and size

Measured on a graph generated at the scale Logseq aims at (`tests/large.rs`): 10,000 pages, 1,000,000 blocks, 579,000 references, 49 MB of text, one page referred to 1,000 times. Native numbers are from an optimized build on an Apple M1 Max; "in Kalem" is the plugin as WebAssembly in Kalem's release build, through `kalem run`.

| | Native | Budget |
|---|---|---|
| The index built | 0.8 s | 2 s |
| Its longest slice, built a slice at a time as in Kalem | 30 ms | 50 ms |
| The memory it holds (64-bit; less in WebAssembly's 32 bits) | 287 MB | 300 MB |
| A saved page indexed again | 0.4 ms | 10 ms |
| The backlinks panel of the page with 1,000 references | 2 ms | 50 ms |
| Its unlinked references, when opened | 280 ms | 500 ms |
| The layer over a journal of 5,000 blocks, at each keystroke | 8 ms | 50 ms |
| The Tasks document, 200,000 tasks | 350 ms | 500 ms |
| A query over the graph | 20 ms | 500 ms |
| In Kalem: the graph indexed and All pages printed | 1.9 s, 340 MB for the whole process | |
| In Kalem's terminal editor: a note of the graph drawn, then the graph indexed, keys answered meanwhile | 0.6 s, then 3.2 to 4.4 s; keys within 40 ms | |

The plugin asks Kalem for a viewer's limits, 1 GB and 10 s a call (`limits` in `plugin.json`): an extension's own, 64 MB and 100 ms, stop it at the first note of a graph of some thousand pages. A note of a graph opened, the graph is indexed in slices of 30 ms at Kalem's ticks (its plugin API 0.2.17, `later`), Kalem drawing and taking keys between them: the status bar says how far it is, and the note's backlinks and links come once it is built. A command that needs the graph meanwhile (All pages, Journals) builds the rest at once; Index Again builds a slice at a time while the index the graph has serves. A save then changes only the saved file's entries in the index (a page renamed, or a file added or removed, derives the names again, as fast as the first build's last step). Files changed on disk together (a `git pull`, Logseq renaming a page and its references) are read together once the event that told of the first returns, the names derived and the documents written again once for them all. The panel's unlinked references are searched for when they are opened, as Logseq does.

## Tests

`cargo test -p kalem-plugin-graph`:

- the library's unit tests;
- `tests/conformance.rs` over four corpus graphs written for this repository and licensed as it is (`corpus/logseq-md`, `corpus/logseq-org`, `corpus/obsidian`, and a database graph's Markdown Mirror, `corpus/logseq-db-mirror`): settings, page names and every resolution rule, blocks, tasks, references, templates, queries, the documents, every corpus file scanned whole, every block folded and unfolded back to the same bytes, and an index kept by saves equal to one built afresh after every kind of edit of every file;
- `tests/logseq_parity.rs`: the references, the queries and the graph view against Logseq 0.10.9's own answers on `corpus/logseq-md` and `corpus/logseq-org`, kept in `tests/logseq/answers-0.10.9.json` (26 queries, 21 pages' references) and `answers-org-0.10.9.json` (28 queries, 4 pages' references);
- `tests/snapshots.rs`: each document of each corpus as written, the graph's menu among them, against `tests/snapshots/` (`UPDATE_SNAPSHOTS=1` writes them anew); with `KALEM=path/to/kalem`, the ignored `kalem_run_prints_them` compares what `kalem run` prints;
- `tests/robust.rs`: a configuration that does not parse, a file not in UTF-8 and one over 16 MB give one notice; embeds of themselves; every note of the corpora broken a few thousand ways through the scanner, the layer, the queries and every editing command without a panic (`ROBUST_ROUNDS` and `ROBUST_SEED` search longer);
- `tests/large.rs`, ignored: the measurements above, `cargo test --release -p kalem-plugin-graph --test large -- --ignored --nocapture`; `GRAPH_DIR=DIR` with `write_the_large_graph` writes the graph for Kalem.
