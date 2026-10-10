# docx: a Word document opened, edited and saved as itself

The list for the `docx` plugin of getkalem/plugins (`plugins/docx`,
crate `kalem-plugin-docx`, id `org.kalem.docx`): Kalem's task T3.7.5,
roadmap item R5.16, decisions D54 (files that are not text open through
plugins) and D55 (opened as itself: a Word file is edited as
WordprocessingML, ECMA-376 part 1, and never converted). Written
2026-10-09 before the crate existed, at the repository's root, and
moved here with the crate the same day; each task says what was done
and what is open. The xlsx plugin is the model: what it does for SpreadsheetML
(the package read as parts, a cell edit rewriting one `<c>`, every other
byte kept, a save without edits byte-identical) this plugin does for
WordprocessingML, where the cell is the run.

## Its own plugin, on a layer shared with xlsx

Of the xlsx plugin's 78,000 lines, the part a Word document needs as it
is comes to about 2,100: the OPC package (`package.rs`, the ZIP read and
written part by part, 609 lines), the span-keeping XML reader and the
attribute splicing (`xml.rs`, 515), relationships and content types
(`rels.rs`, 177), a password to open ([MS-OFFCRYPTO], `crypto.rs`, 735:
Word writes the same compound file as Excel), and the theme's colors
(`styles::parse_theme`, 90). Later and partly: the VBA project reader
and the language half of the macro interpreter (`vba.rs` and
`macros/` without `excel.rs`, about 4,700 lines, for `.docm`), the
chart parts (`chart.rs` and its companions, about 4,300: a chart in
Word is the same `c:chart` part, anchored differently), and the
picture's `a:blip` in `pictures.rs`. Everything else (sheets, formulas,
IronCalc, number formats, pivot tables, calamine, the grid viewer) is
Excel's and of no use to Word, and the pptx plugin (T3.7.6) will need
exactly the shared part again.

So the docx plugin is a crate of its own, not a mode of the xlsx crate:
one plugin, one crate, one directory, one tag (CONTRIBUTING; T3.7.11
names `plugins/docx`); the manifests differ in every field (`opens`,
`name`, the 4 GB memory limit a million-cell sheet needs); a component
exports one world, and a workbook's is `spreadsheet-viewer` while a
document's will be the flow world of WP5; and a Word user should not
download a formula engine and a VBA interpreter to read a letter. The
shared part becomes a library crate the three Office plugins depend on
by path (WP0). The one thing that would change this: if the owner wants
the shared crate avoided for now, docx starts with copies of the four
modules (2,000 lines) and the extraction waits for pptx.

Each task is done when it works in both editors, is written into the
file as Word writes it, undoes in one step, and has its tests. The
three rules of 11.13 hold throughout: parts, never the package (an edit
rewrites the one element it touches; every other byte of the part and
every other entry of the ZIP is copied as it was); no extension (nothing
written that ECMA-376 does not define, no `rsid` invented, no property
Word would not write); unknown constructs stay visible (a placeholder
naming what it is, kept in the file). "As Word writes it" means Word
2016 and later (Microsoft 365): the transitional namespace, the
`w14`/`w15`/`w16` additions read and kept, element order as the
schema's sequence, which Word checks and repairs. The tasks are in the
order they are done, the spike first.

## WP0. The shared OOXML crate

- [x] WP0 `crates/ooxml` (crate `kalem-ooxml`, `"crates/*"` added to the
  workspace members; rlib only, no `plugin.json`, so `build-index.py`
  and the release workflow pass it by): `package` (the ZIP layer),
  `xml` (the reader, `escape`, `set_attr`, `remove_attr`; the
  SpreadsheetML `_xHHHH_` escapes behind a flag, since Word writes
  none), `rels` (parse, resolve, remove, the content types; each
  plugin keeps its own `kind` constants), `crypto` (agile and standard
  encryption, read and written) and `theme` (`clrScheme` as today,
  plus `fontScheme`'s major and minor Latin, East Asian and complex
  script fonts, which Word's `w:asciiTheme` names), each with the
  tests it has now. The xlsx plugin depends on it and re-exports the
  modules under their old paths (`kalem_plugin_xlsx::package::Package`
  is what its tests and the example use); its corpus tests stay
  byte-identical; xlsx 0.0.9 is released with no change in behavior.
  Left in xlsx: everything else. Noted for later: the VBA project
  reader (WP12) and the chart reader (WP7) move when docx needs them.
  (Done 2026-10-09: `crates/ooxml` with `package` (and
  `restore_part`, so that an edit undone saves the input byte for
  byte), `xml` (plain `escape`; `escape_st_xstring` and
  `set_attr_escaped` for SpreadsheetML), `rels` (and `content_type`,
  `next_id`, `add`, `add_override`, `add_default`), `crypto` (its
  messages naming the file as the caller says) and `theme` (colors and
  font scheme); xlsx's modules re-export them under their old paths,
  xlsx's own `xml::escape` and `xml::set_attr` stay the ST_Xstring ones,
  and all its tests pass unchanged. Open: xlsx 0.0.9 released.)

## WP1. The spike: one run changed, Word opens it

- [~] WP1 `kalem plugin new docx`; the package opened through the shared
  crate; `[Content_Types].xml` and `_rels/.rels` followed to the main
  document part (`officeDocument`), `word/document.xml` read into
  paragraphs and runs with the byte span of each, the text of one
  `<w:t>` replaced, the part spliced, the package written. On six files
  (Word 2016, Word 365, LibreOffice, python-docx, pandoc, Google Docs):
  a save without edits is byte-identical; after the edit the part diff
  is `word/document.xml` alone; LibreOffice opens the result; Word
  opens it without a repair prompt and shows the edit. Measured and
  written down: a 1,000-page document (a 40 MB `document.xml`) opened,
  the time and the memory. The spike's verdict decides whether the
  editor half of WP8 onward is committed to (the note in Kalem's
  `todo.md` under group 3.7: "a spike on docx alone should precede
  committing to three editors").
  (Done 2026-10-09 but for Word: on six files of three producers
  (python-docx, LibreOffice, the handmade one, each saved again by
  LibreOffice) an unedited save is byte-identical and after an edit
  only `word/document.xml` differs, in the runs touched; LibreOffice
  opens every edited file and shows the edit. A document of 20,000
  paragraphs (4.4 MB of `document.xml`) opens in 0.18 s in 82 MB, an
  edit and its save take 0.17 s (M1 Max, release build). Open: Word
  opening the edited files without a repair prompt, by hand. Driving
  Word with AppleScript stopped at its file access prompt, and Word
  files saved by Word itself are not in the corpus yet. The verdict for
  the editor half is yes: the edits of WP8 were written.)

## WP2. The package and the document read

- [x] WP2 The parts, by relationship from the main document part and
  never by name: `styles`, `numbering`, `settings`, `fontTable`,
  `theme`, `footnotes`, `endnotes`, `comments` (with `commentsExtended`,
  `commentsIds`, `people`), `header` and `footer` parts, `glossaryDocument`
  (building blocks: kept, not shown), `vbaProject`, the `media` parts,
  `docProps/core.xml` and `app.xml`; the strict namespace
  (`purl.oclc.org`) read as the transitional one, as `rels` keys on
  the type's last segment and the reader drops prefixes. The body
  (`w:body`) as a list of blocks with spans: paragraphs (`w:p` with
  `w:pPr`: `pStyle`, `numPr`, `jc`, `ind`, `spacing`, `outlineLvl`,
  `pBdr`, `shd`, `tabs`, the mark's `rPr`, a `sectPr` ending a section)
  of runs (`w:r` with `w:rPr`) whose content is `w:t` (with
  `xml:space="preserve"`), `w:tab`, `w:br` (text wrapping, page,
  column), `w:cr`, `w:sym`, `w:noBreakHyphen`, `w:softHyphen`,
  `w:footnoteReference`, `w:endnoteReference`, `w:commentReference`,
  `w:fldChar` and `w:instrText`, `w:drawing`, `w:pict`, `w:object`,
  `w:lastRenderedPageBreak` (skipped) and `w:delText`; around runs
  `w:hyperlink`, `w:fldSimple`, `w:ins`, `w:del`, `w:moveFrom`,
  `w:moveTo`, `w:sdt` (its `w:sdtContent` read in place),
  `w:smartTag`, `w:bookmarkStart` and `End`, `w:commentRangeStart`
  and `End`, `w:proofErr` (skipped); tables (`w:tbl`, WP6); and
  `mc:AlternateContent`, whose `mc:Choice` is taken when its
  `Requires` names a namespace the plugin knows (`wps`, `wpg`, `w14`,
  `w15`, `w16se`) and whose `mc:Fallback` otherwise. Parts are read
  when first needed and kept; a part the plugin does not read is never
  inflated. Every cut of a file (the pdf-viewer's test) fails without a
  panic.
  (Done 2026-10-09, `story.rs` and `document.rs`: every listed
  part followed by relationship, the strict namespace by the types'
  last segments, paragraphs, runs, every listed run child and inline
  group with its span, `mc:AlternateContent` through `wps`, `wpg`,
  `w14`… else the fallback, a choice read whole so that an edit removes
  the whole element; text without `xml:space="preserve"` loses its
  outer white space as in Word. Open: `glossaryDocument` and
  `commentsExtended`, `commentsIds`, `people` are kept, not read;
  UTF-16 parts are refused with a message.)

## WP3. Styles: what the text looks like

- [~] WP3 Formatting resolved as 17.7.2 says, in this order: the
  document defaults (`w:docDefaults`), the table style and its
  conditional parts, the numbering level's properties, the paragraph
  style and its `w:basedOn` chain, the paragraph's own `w:pPr`, the
  run style (`w:rStyle`) and its chain, the run's own `w:rPr`; toggle
  properties (bold, italic, caps, small caps, strike, double strike,
  outline, shadow, emboss, imprint, hidden) combined as 17.7.3 says,
  each style level that sets one toggling it; `w:default="1"` styles;
  sizes in half-points (`w:sz`, `w:szCs`), indents and spacing in
  twentieths of a point (`w:ind`, `w:spacing`, `w:line` in 240ths of a
  line under `auto`); theme fonts (`w:asciiTheme`, `w:hAnsiTheme`,
  `w:eastAsiaTheme`, `w:cstheme` through the theme's major and minor
  fonts) and theme colors (`w:themeColor` with `w:themeTint` and
  `w:themeShade`), `auto` as the theme's foreground (the workbook fix
  of Kalem 0.3.0 on the dark theme); `w:highlight` by name, `w:shd`
  fills, `w:u` kinds, `w:vertAlign`, `w:color`, `w:rFonts` by script
  (`ascii`, `hAnsi`, `eastAsia`, `cs`, chosen by the character's
  script as 17.3.2.26 says), `w:lang`, `w:rtl` and `w:bidi`; `w:jc`
  (`start` and `left`, `end` and `right`, `center`, `both`,
  `distribute`), `w:outlineLvl` for the outline; `w:latentStyles` kept
  untouched. Resolution memoized per style and per paragraph, so a
  typed character re-resolves one paragraph. The resolved look of
  every paragraph and run of the corpus compared with LibreOffice's
  (`soffice --convert-to html` read back) and with python-docx's
  reading of the same files.
  (Done 2026-10-09, `props.rs` and `styles.rs`, but for the
  comparison with LibreOffice's and python-docx's reading of the
  corpus, which the tests do by hand-written expectations instead; the
  theme colors' tint and shade agree with the values Word writes, to
  one level of a channel for some tints (Word's own rounding); fonts
  are chosen by the ASCII slot, not by each character's script yet.)

## WP4. Lists and numbering

- [~] WP4 `numbering.xml`: `w:abstractNum` with its levels (`w:lvl`:
  `w:start`, `w:numFmt` of 17.18.59 from `decimal` and `bullet` to
  `lowerRoman`, `upperLetter`, `decimalZero`, `ordinal`, `cardinalText`
  and `none`, `w:lvlText` with `%1.%2`, `w:lvlJc`, `w:lvlRestart`,
  `w:isLgl`, the level's `w:pPr` indents and `w:rPr` for the label's
  font), `w:num` with `w:lvlOverride` and `w:startOverride`,
  `w:numId="0"` as no list, a style's `w:numPr` and a level's
  `w:pStyle` (list styles); the counters computed in document order
  per `numId` and level with restarts, so each paragraph shows the
  label Word shows (`1.`, `a)`, `iv.`, `•`); Symbol and Wingdings
  bullets mapped to Unicode (`U+F0B7` is `•`); the label drawn with the
  level's indent and tab. Lists compared with pandoc's reading (list
  item or not, level) and with LibreOffice's labels.
  (Done 2026-10-09, `numbering.rs` and `chars.rs`, but for the
  comparison with pandoc's and LibreOffice's labels on a corpus: pandoc
  is not installed here; LibreOffice's save of the handmade file gives
  the same labels.)

## WP5. The view: web layout as blocks, and Kalem's flow interface

- [~] WP5a The plugin's half (done 2026-10-09, `flow.rs`): a story
  shown as blocks Kalem will lay out, Word's web layout: paragraphs
  with their style's name, list label, alignment, indents, spacing,
  shading, outline level and runs; runs with their resolved look,
  link, tracked change, comments, field, and the range of the
  paragraph's edit text they stand for; tables with their cells'
  spans, merges, fills, borders and blocks; text boxes as frames after
  their anchor's paragraph; placeholders naming what is not shown;
  notes in order of reference with their marks; comments; the first
  section's header and footer. `text(unit)` gives Kalem the text
  (paragraphs a line, labels first, table rows a line with tabs
  between cells, notes after the body), the outline comes from the
  headings, and `render` says that Kalem lays the document out (WP5b).
  The plugin builds as a `flow-viewer` component (plugin API 0.2.7)
  of 1.0 MB as released.

- [x] WP5b Kalem's half, a **flow interface for every plugin of flowing
  documents**, not for Word alone (the owner's direction, 2026-10-09:
  an addition to the plugin API is made as general as it can be, so
  that later plugins use it as they are). Its users from the start:
  Word (`docx`), OpenDocument text (`odt`, T3.7.7), RTF, e-books
  (`epub`, one unit a chapter), web pages and e-mail bodies (`html`,
  `eml`), and the text bodies of slides (`pptx`, T3.7.6) as frames.
  (Done 2026-10-09, plugin API 0.2.7, Kalem's `flow.wit`,
  `annotations.wit`, `kalem-viewer`'s `flow.rs`, `kalem-core`'s
  `flow.rs`; this plugin's half in `contract.rs`:)
  - **The unit.** `viewer.wit`'s unit kinds are frozen, so a unit is of
    flowing text when its viewer answers `flow.layout(unit)`, as a
    grid's does; the world `flow-viewer` exports `flow` beside
    `viewer`, `password`, `formats` and `annotations`, and
    `kalem-plugin`'s feature `flow` adapts the Rust contract to it.
    One unit is a document's body, its first section's header and
    footer and its notes as asides. (Open: a section break given as a
    rule; the plugin does not mark it yet.)
  - **Items**, fetched a range at a time (`items(unit, from, count)`)
    with a version: WIT has no recursive types, so a table, a row, a
    cell and an aside (a header, a footer, a footnote, an endnote, a
    frame, a sidebar) are a start and an end around their paragraphs.
    A paragraph has a role any format has (body, title, subtitle,
    heading, list item, quote, code, caption) with its level, the
    format's style name, its label with the label's marks, alignment,
    indents, spacing, background, its edit text, its runs and the
    annotations on its mark; a paragraph without an index is shown and
    not edited. A run has its text, a piece (text, tab, a line, page
    or column break, a note's mark, a picture, a placeholder), marks by
    generic names (bold, italic, underline with its kind, strike,
    double strike, caps, small caps, hidden, raised or lowered, color,
    highlight, size, typeface), its link, the IDs of the annotations it
    is in, its range of the edit text, and the reason it is locked when
    it is.
  - **Edits** in edit coordinates (a paragraph's index and a byte of
    its edit text): `replace`, `split`, `join`, `delete` from one place
    to another, `set-marks`, `set-style`; the document's own history
    (`begin-batch`, `end-batch`, `undo`, `redo`), as the grid's.
    `styles` lists the styles for pickers; `render-picture` gives a
    picture as a bitmap.
  - **Annotations** are an interface of their own (the owner's
    decision, 2026-10-09), exported by every viewer world, so that the
    PDF viewer's notes and a workbook's comments use it too: a comment
    (author, date, text, the comment it answers, resolved) or a tracked
    change (insertion, deletion, formatting, moved from, moved to),
    anchored to flowing text, a page's text, a cell, an area or a
    whole unit; adding, answering, resolving and removing comments,
    accepting and rejecting one change or all, tracking turned on and
    off, and the author's name from Kalem's setting `user.name`.
  - **Kalem shows** a flow unit in both editors as a document of the
    editor: its text is the paragraphs' edit texts a line each (a row
    of one-paragraph cells a line, its cells between tabs), drawn with
    the runs' marks, sizes in proportion to the body text's, list
    labels, the asides' and rows' marks before their lines; typing,
    Enter, Backspace and deleting across paragraphs become `replace`,
    `split`, `join` and `delete` in one batch, refused edits undone and
    said in a notice; undo and redo are the plugin's. Comments are
    highlighted, insertions underlined and deletions struck through in
    their colors, the status bar says the comment or change at the
    cursor, and the commands of the Review category act on them.
  Not yet in Kalem: pictures drawn (`render-picture` is in the
  interface, the plugin does not answer it), the comments' margin,
  formatting and style edits from the editor (`set-marks`,
  `set-style`: WP9), and a page's text anchors for the PDF viewer.

## WP6. Tables

- [~] WP6 `w:tbl`: the grid (`w:tblGrid`, `w:gridCol` in twentieths of a
  point), widths (`w:tblW`, `w:tcW` as `auto`, `dxa` or `pct`),
  `w:gridSpan`, `w:vMerge` (`restart` and continued), `w:tblBorders`
  and `w:tcBorders` with `insideH` and `insideV`, `w:shd` fills, cell
  margins, the table's `w:jc` and `w:tblInd`, nested tables, `w:tblHeader`
  rows noted; table styles (`w:tblStyle`) with `w:tblLook`'s first and
  last row, first and last column and banding choosing the
  `w:tblStylePr` parts (17.7.6); `w:cantSplit` and `w:tblpPr` (floating
  tables) ignored in web layout, the table shown in place. Drawn by
  the host as its Org tables are, with the cells' own paragraphs
  wrapping inside them; in the terminal as box lines, as the CSV grid.
  Cells compared with pandoc's reading of the corpus.
  (Shown, done 2026-10-09: the grid, spans and merges, widths,
  the table style's whole and conditional formats chosen by
  `w:tblLook` and the band sizes, borders as the table and the cells
  give them, fills, nested tables, header rows marked; rows and cells
  inside content controls read. Open: drawn by Kalem (WP5b), widths
  given as percentages, and the comparison with pandoc.)

## WP7. Fields, links, notes, comments, changes and the rest, shown

- [~] WP7a Fields: `w:fldSimple` and `w:fldChar` ranges (begin,
  separate, end; nested) show their cached result, the instruction
  (`PAGE`, `NUMPAGES`, `DATE`, `REF`, `SEQ`, `TOC`, `HYPERLINK`,
  `MERGEFIELD`) kept for the hover text and the field marked as a
  field; a `TOC`'s result is its paragraphs, shown as they are;
  `w:dirty` fields marked stale; `HYPERLINK` fields and `w:hyperlink`
  (external through its relationship, internal through `w:anchor` and
  the bookmark) as links; bookmarks as the targets of `#` links.
  (Done 2026-10-09 but for `w:dirty`, which is not marked.)
- [~] WP7b Footnotes and endnotes: `w:footnoteReference` and
  `w:endnoteReference` as superscript marks numbered as `w:footnotePr`
  and `w:endnotePr` say (`w:numFmt`, `w:numStart`, `w:numRestart`),
  custom marks (`w:customMarkFollows`), the notes from `footnotes.xml`
  and `endnotes.xml` with the separator notes (`w:type="separator"`
  and `continuationSeparator`, ids `-1` and `0`) left out, each note's
  own paragraphs shown after the body with a link back.
  (Done 2026-10-09 but for custom marks
  (`w:customMarkFollows`) and section-level note properties.)
- [~] WP7c Comments: `comments.xml` (`w:id`, `w:author`, `w:date`,
  `w:initials`, the comment's paragraphs), the ranges in the body,
  replies and `done` from `commentsExtended.xml` (`w15:paraId`,
  `w15:paraIdParent`), authors from `people.xml`; shown in the margin
  panel with the range marked, a reply under its comment.
  (Read 2026-10-09: comments with their author, date and
  blocks, the runs in their ranges. Open: replies and `done` from
  `commentsExtended.xml`, authors from `people.xml`, the margin panel.)
- [~] WP7d Tracked changes: `w:ins`, `w:del` (its `w:delText`),
  `w:moveFrom` and `w:moveTo`, `w:rPrChange`, `w:pPrChange`,
  `w:sectPrChange`, `w:tblPrChange`, `w:cellIns` and `w:cellDel`, a
  paragraph mark inserted or deleted (`w:ins` and `w:del` in the
  mark's `w:rPr`), `w:trackRevisions` in the settings; insertions
  underlined and deletions struck, each in its author's color, with
  the author and date on hover; a view of the final text (markup
  hidden) and of the original; Next Change and Previous Change.
  (Read 2026-10-09: insertions, deletions, moves (shown as an
  insertion and a deletion), formatting changes, paragraph marks
  inserted and deleted, with authors and dates; deleted text left out
  of the text read. Open: table rows and cells inserted or deleted, the
  final and original views, Next Change.)
- [~] WP7e Headers, footers and sections: `w:headerReference` and
  `w:footerReference` (`default`, `first`, `even`), `w:titlePg` and
  `w:evenAndOddHeaders`, shown once at the top and the bottom of the
  body; `w:sectPr` (page size, margins, `w:cols`, `w:pgNumType`,
  `w:type` next page, continuous, even, odd, column) as a marker line
  naming the kind of break; page breaks (`w:br w:type="page"`,
  `w:pageBreakBefore`) and column breaks as marker lines; columns
  noted, not laid out.
  (Done 2026-10-09 for the first section's default header and
  footer and each section break's kind. Open: first-page and even
  headers, page and column breaks drawn as marker lines by Kalem.)
- [~] WP7f Pictures and drawings: `w:drawing` with `wp:inline` (the
  picture from the media part through `a:blip r:embed`, its size from
  `wp:extent`, `a:srcRect` cropping honored, effects ignored) decoded in
  the plugin as the image viewer decodes (PNG, JPEG, GIF, BMP, TIFF,
  WebP; SVG through `resvg` when the size budget allows; EMF, WMF and
  WDP as placeholders naming the format); `wp:anchor` (floating) shown
  inline where it is anchored, with a marker saying it floats and how
  the text wraps (T3.7.5); `w:pict` VML pictures (`v:imagedata`) read
  the same way; alt text from `wp:docPr`; a picture's link
  (`a:hlinkClick`).
  (Read 2026-10-09: inline and floating drawings, their size,
  name and alternative text, the picture's part through its
  relationship (embedded or linked), VML pictures. Open: decoding the
  picture for Kalem (WP5b's `render-picture`), cropping, links on pictures.)
- [~] WP7g Placeholders, kept in the file and shown by name: text boxes
  (`wps:txbx`, their content shown inline inside a frame marker),
  shapes (`wps:wsp` with their text), groups, SmartArt (`dgm:`),
  charts (`c:chart`: shown as a placeholder first; then, through the
  chart reader shared with xlsx, drawn by the host as it draws a
  workbook's charts, from the chart part's cached values), embedded
  objects (`w:object`: its `v:imagedata` preview when there is one,
  else the object's program name from the relationship's content
  type), content controls (`w:sdt`: the content, placeholder text
  muted, a checkbox as `☐` or `☒`, a date or dropdown's value), OMML
  math (`m:oMath`: its linear text; later, through Kalem's math
  rendering, when an OMML-to-LaTeX pass exists), `w:ruby`,
  `w:framePr`, `w:fitText`, line numbering. The chapter's
  known-differences table lists every construct shown as a
  placeholder (T3.7.5's "done when").
  (Done 2026-10-09 as placeholders: text boxes (their content
  shown after the anchor's paragraph, from `wps` or VML), shapes,
  groups, canvases, charts, SmartArt, ink, embedded objects by program,
  content controls (a check box shows its state), equations as their
  linear text, unknown elements by name. Open: charts drawn from their
  parts, ruby, frames, line numbering.)

## WP8. Typing: text edited in runs

- [x] WP8 The editor half, in the format's own vocabulary and nothing
  else: a character typed goes into the run before the cursor (its
  `w:rPr` inherited, as Word does; at a paragraph's start, into its
  first run, or a new run with the mark's `w:rPr`), rewriting that
  run's `<w:t>` alone, escaped, with `xml:space="preserve"` when a
  space leads or trails; a selection deleted across runs shortens,
  removes or splits them; Enter splits the paragraph (the `w:pPr`
  copied; a heading's next paragraph takes the style's `w:next`);
  Backspace at a paragraph's start joins it to the one before (its
  runs appended, the first's `w:pPr` kept); Tab writes `w:tab`,
  Shift+Enter `w:br`; a paragraph deleted; typing inside a `w:del`
  refused, inside a field's result refused until the field is
  unlinked; the same machinery in a header, footer, footnote, endnote
  or comment part, which is then the part rewritten; control
  characters XML 1.0 forbids refused. Undo and redo from snapshots
  (the package shared by `Arc` as the workbook's is; undoing
  everything saves the input byte for byte); edits batched by the host
  undo as one step; `modified`; the save writes the parts that changed
  and nothing else (the test: the part diff after typing is
  `word/document.xml` alone; after a footnote edit, `footnotes.xml`
  alone), `docProps/core.xml` untouched (Word would bump
  `dcterms:modified` and `cp:revision`; whether Kalem does waits for
  the owner, under the no-extension rule either way). A run-level diff
  in the tests (the `changed_cells` of the workbook tests, for runs):
  after typing one word, exactly one `<w:r>` differs.
  (Done 2026-10-09, `edit.rs` and `Document::replace`,
  `split`, `join`, `delete_between`, in every story: the body, cells,
  headers, footers, notes and comments; a run-level diff in the tests.
  The core properties are left untouched, as the open question below
  says. Open: text boxes (their choice and fallback both), typing into
  a field's result, the paragraph style's `w:next` applied only at the
  end, as Word does.)

## WP9. Formatting edited

- [~] WP9 Bold, italic, underline, strikethrough (Ctrl+B, Ctrl+I,
  Ctrl+U, as in Word), font color, highlight, size (Ctrl+Shift+> and
  <), face, superscript and subscript, caps, Clear Formatting
  (Ctrl+Space) over the selection: the runs split at the selection's
  ends so that only the selected text changes (a run cut into up to
  three, each with the original `w:rPr`), the property written against
  the resolved value (a toggle on in the style is turned off by
  `<w:b w:val="0"/>`), `w:sz` with `w:szCs`, `w:rFonts` for every
  script, the `w:rPr` children in the schema's sequence (17.3.2.28:
  `rStyle`, `rFonts`, `b`, `bCs`, `i`, `iCs`, `caps`, `smallCaps`,
  `strike`, `dstrike`, `outline`, `shadow`, `emboss`, `imprint`,
  `noProof`, `snapToGrid`, `vanish`, `webHidden`, `color`, `spacing`,
  `w`, `kern`, `position`, `sz`, `szCs`, `highlight`, `u`, `effect`,
  `bdr`, `shd`, `fitText`, `vertAlign`, `rtl`, `cs`, `em`, `lang`,
  `eastAsianLayout`, `specVanish`, `oMath`), since Word repairs a file
  whose children are out of order; adjacent runs that end up alike
  left as they are (Word merges them on its next save, and merging is
  not an edit the user made). Paragraph formatting: alignment (Ctrl+L,
  Ctrl+E, Ctrl+R, Ctrl+J), indents (Increase and Decrease Indent),
  spacing before and after, line spacing, borders and shading, written
  into `w:pPr` in its sequence (17.3.1.26); the paragraph style
  (Ctrl+Alt+1 to 3 for Heading 1 to 3, Ctrl+Shift+N for Normal; Title,
  Subtitle, Quote, List Paragraph and every style the document has, in
  a picker) as `w:pStyle` by style id; a built-in style the document
  lacks (Heading 3 never used) added to `styles.xml` as Word adds it
  from its own definitions, which the plugin carries as a table of
  Word's built-in styles with their properties; lists: Bullets and
  Numbering on paragraphs (`w:numPr` with an existing `w:num` of the
  kind when there is one, else a new `w:abstractNum` and `w:num`
  written as Word's standard bullet and decimal definitions), list
  levels by Tab and Shift+Tab (`w:ilvl`), a list ended; table cell
  text and formatting through the same commands. Every command in the
  palette, Word's keys where the editor has them free.
  (Done 2026-10-10 for the look of characters and the paragraph style,
  `format.rs`, `Document::set_marks` and `set_paragraph_style`, through
  the `flow` interface's `set-marks` and `set-style`: bold, italic,
  underline (a kind), strikethrough, superscript and subscript, color,
  highlight (`w:highlight` for Word's sixteen colors, else `w:shd`),
  size (`w:sz` and `w:szCs`), typeface (`w:ascii` and `w:hAnsi`, the
  theme's fonts that would win taken away; other scripts' left as they
  are) and Clear (the direct look taken away, `w:rStyle` and `w:lang`
  kept) over a range of one or more paragraphs: the runs cut at the
  range's ends (a cut run's `w:rPrChange` given its own ID), each
  property where the schema's sequence puts it, a toggle turned off
  taken away and written `w:val="0"` where a style still turns it on;
  `w:pStyle` first in `w:pPr`, none for the default style; while the
  document tracks changes, `w:rPrChange` and `w:pPrChange` with the old
  properties, which Accept and Reject keep or undo. Kalem's Format menu
  and toolbar (Bold, Italic, Underline, Strike Through, Superscript,
  Subscript, Font, Font Size, Text Color, Highlight Color, Paragraph
  Style, Clear Formatting) send them. LibreOffice 7.3 reads the result.
  Paragraphs and lists done 2026-10-10 through plugin API 0.2.8's
  `flow-2` (`set-paragraphs`, proposed and added for every format of
  flowing text: alignment, indents, spacing, line spacing, a list of
  bullets or of numbers in a CSS numbering style, its level, cleared),
  `lists.rs`, `Document::set_paragraph_format`: `w:jc`, `w:ind` and
  `w:spacing` attributes, `w:numPr`, each child of `w:pPr` where
  `CT_PPr`'s sequence puts it, an alignment a style overrides written
  explicitly; a list continuing the one before it when it is of the
  kind, else a new instance (counting from 1) of the document's
  definition of the kind or of Word's own nine-level bullet or numbered
  definition, the definitions before the instances; the List Paragraph
  style given as Word gives it, and taken away with the list; a list a
  style makes ended by `w:numId` 0; `w:pPrChange` while tracking.
  LibreOffice 7.3 reads the lists, the alignment and the spacing.
  Open: caps and small caps, sizes up and down, borders, shading and
  tabs of paragraphs, a built-in style the document lacks added, and
  Word's keys for headings.)

## WP10. Comments and tracked changes edited

- [x] WP10a Comments: New Comment on the selection (a `w:comment` in
  `comments.xml`, made with its relationship and content type override
  when the document has none; `w:commentRangeStart` and `End` around
  the runs and a run with `w:commentReference` in the `CommentReference`
  style; a `w15:commentEx` with a fresh `w14:paraId`, unique and below
  `0x80000000`, in `commentsExtended.xml`), Reply (`w15:paraIdParent`),
  Resolve (`w15:done`), Delete (the three markers and the comment
  gone; the parts removed when empty, as Word leaves them); the author
  and initials from Kalem's user name (`set-author`). Kalem's half is
  done (the `annotations` interface's `comment`, `reply`, `resolve`,
  `remove` and the Review commands, 2026-10-09).
  (Done 2026-10-09: `comments.rs`, `Document::add_comment`,
  `reply_comment`, `resolve_comment`, `remove_comment` and
  `set_comment_text`. Editing a comment (Kalem's Edit Comment, the
  `annotations` interface's `set-text`) writes its paragraphs anew, a
  line each, each taking an old paragraph's start tag and properties in
  order and the last line the last paragraph's, so that its
  `w14:paraId`, which answers and the done mark name, stays; the mark
  run kept at the start, the author and date as they were; a comment
  holding a table refused.
  New Comment on a range in the body or a note (headers and footers
  refused, as Word keeps none there), a run split where the range
  starts or ends inside it, the reference run after the end and, when
  the end is inside a link or an insertion, after that at the
  paragraph's level; an empty range a comment at a point, its
  reference alone. The comment's paragraphs each with a fresh
  `w14:paraId` (the root declaring `w14` and listing it as ignorable
  when it did not), `CommentText` and `CommentReference` only when the
  document has those styles, Word's `w14:textId` `77777777`; its
  `w15:commentEx` written when the document has the extended part. A
  reply anchored after the thread's last range start and reference,
  its parent's last paragraph given an ID when it had none. The
  `commentsExtended` part read too, so Kalem shows answers under their
  comment and done comments as resolved. Checked with LibreOffice 7.3:
  it reads the comments with their authors, texts and ranges, and
  shows an answer as a comment of its own, as it reads no threads.
  Resolve marks the thread's first comment `w15:done` (an answer
  resolves its thread, as in Word), the extended part made when there
  is none; Kalem shows the answers resolved with it. Delete takes a
  comment with its answers (an answer alone when it is one): the range
  markers, the reference run when it holds nothing else, the comment,
  and its entries in `commentsExtended`, `commentsIds` and, by durable
  ID, `commentsExtensible`; the runs a range split stay split, and an
  empty comments part stays, valid as it is. LibreOffice 7.3 reads the
  resolved mark and finds nothing of a deleted comment. Word by hand
  still.)
- [x] WP10b Tracked changes: Accept and Reject on the change under the
  cursor, Accept All and Reject All, as Word rewrites them (an
  accepted `w:ins` unwrapped, a rejected one removed; an accepted
  `w:del` removed, a rejected one unwrapped with `w:delText` back to
  `w:t`; a property change accepted drops its `w:rPrChange` or
  `w:pPrChange`, rejected restores the properties inside it; paragraph
  marks and moves likewise); Track Changes turned on and off
  (`w:trackRevisions` in `settings.xml`), and while it is on the
  plugin's own typing and formatting written as `w:ins`, `w:del` and
  `w:rPrChange` with the author and the date, so a reviewer's edits in
  Kalem look in Word as Word's own do.
  (Done 2026-10-09: Track Changes turned on and off in the settings,
  and while it is on typing, deleting, Enter and Backspace written as
  `w:ins`, `w:del` with `w:delText` and the paragraph mark's `w:ins`
  and `w:del`, each with the author (Kalem's setting `user.name`, given
  through `set-author`; "Kalem" without it), the date and a fresh ID.
  LibreOffice keeps them on its own save, but for a paragraph mark
  inserted. Accepting and rejecting (`review.rs`, `Document::decide`
  and `decide_all`): runs inserted or deleted, moves, paragraph marks
  (accepting a deleted mark joins the paragraphs, in the body), run and
  paragraph formatting changes, one by its ID or all in every story;
  from Kalem's Review commands through the `annotations` interface.
  Formatting changes are made by the plugin only once WP9 is.)

## WP11. Pictures, links, tables and structure edited; Find

- [ ] WP11a Insert Picture (the file's bytes as `word/media/imageN.EXT`
  with its relationship and a content type default, a `w:drawing` with
  `wp:inline`, `wp:docPr`, `a:graphic` and `pic:pic` as Word writes
  them, sized from the picture's pixels at 96 dpi and fitted to the
  text width), Delete, Resize (`wp:extent` and `a:ext` together), Alt
  Text; Insert Hyperlink (external: a relationship with
  `TargetMode="External"` and a `w:hyperlink` whose run has the
  `Hyperlink` style; internal: to a bookmark, Insert Bookmark), Remove
  Hyperlink.
- [ ] WP11b Tables: Insert Table (rows by columns, `w:tblGrid` from
  the text width, the `TableGrid` style when the document has it, else
  the borders written), Insert and Delete Row and Column (`w:gridSpan`
  and `w:vMerge` adjusted), Merge Cells, Delete Table, a table style
  applied (`w:tblStyle` with `w:tblLook`), column widths dragged.
- [ ] WP11c Structure: Insert Footnote and Endnote (the reference run
  in the `FootnoteReference` style, the note in its part, the part
  made with its two separator notes when absent), Delete; Page Break,
  Section Break (the `w:sectPr` copied from the section's), Edit Header
  and Footer (the part rewritten; a header part made with its
  `w:headerReference`, relationship and content type when the section
  has none); Document Properties (title, subject, author, keywords in
  `docProps/core.xml`) in the information panel with the word, character
  and paragraph counts computed, not `app.xml`'s stale ones.
- [ ] WP11d Find (Ctrl+F) and Replace (Ctrl+H), the host's search over
  `text(unit)`, a match across run boundaries replaced by rewriting
  the runs it spans with the first run's `w:rPr`; whole word, case;
  Replace All one undo step; Spelling over the text with the editor's
  dictionaries, the document's `w:lang` choosing them.

## WP12. New documents, other formats, passwords, macros

- [~] WP12a New Document (`Document1.docx`: the parts Word writes for a
  blank document, `[Content_Types].xml`, `_rels/.rels`,
  `word/document.xml`, `styles.xml` with Word's defaults, `settings.xml`,
  `fontTable.xml`, `webSettings.xml`, the Office theme, `docProps`),
  New from Template (`.dotx` and `.dotm` opened as a new `.docx` or
  `.docm` with the template's styles, no `w:attachedTemplate` written);
  `.docm` opened and saved as itself, the VBA project kept byte for
  byte and its modules listed through the shared reader; Save As
  `.docx` from `.docm` (the project removed, the main part's content
  type changed, as xlsx does for `.xlsm`) through `formats.save-as`;
  conversion only as an explicit export (D55): Kalem's pandoc bridge
  already imports `.docx` to Org, and the plugin's `text` is the plain
  text export. `.doc` is not read (no pure-Rust reader; the fallback
  of T3.7.9 offers the system application); `.odt` is a group of its
  own (T3.7.7); `.rtf` neither.
  (Done 2026-10-09 for the plugin's half of New Document:
  `Viewer::new_file` writes a blank `.docx`, `.docm`, `.dotx` or
  `.dotm` with those parts (`blank.rs`), Word 365's document defaults
  and built-in styles (Normal, Heading 1 to 9, Title, Subtitle, Quote,
  Intense Quote, List Paragraph, their character styles), compatibility
  mode 15, and the Office theme of Aptos, which the shared layer now
  holds (`kalem_ooxml::theme::OFFICE`, with `Package::new` for a
  package made from nothing); a sheet of entries the contract hands
  over becomes a table. Not Word's: A4 with margins of 2.5 cm where
  Word takes Letter or A4 from the locale, and no `w:lang`, so that
  Word proofs in its own editing language. LibreOffice opens the files
  and saves them again with the text and the page. Open: Kalem's
  command, which today makes only workbooks (`app.newWorkbook` is
  bound to `.xlsx`): a New command for every plugin that makes files,
  as the manifest's `opens` lists what a plugin opens; New from
  Template; Save As between the kinds; `.docm` macros listed; Word
  itself opening a new file, checked by hand.)
- [~] WP12b A password to open (the shared `crypto`: agile and standard
  encryption, `password.open-with-password`, saved encrypted again
  with the password it opened with, Word's default password opened
  unasked); document protection (`w:documentProtection`: read only,
  comments only, tracked changes only, forms, with `w:enforcement`,
  the SHA-512 hash and salt spun as 17.15.1.29 says, the same hashing
  as a sheet's protection) read and kept, edits refused as it says,
  Protect Document and Unprotect with a password.
  (Done 2026-10-09 for a password to open, read and written
  with the shared layer; enforced protection is read and edits are
  refused while it holds. Open: Protect and Unprotect.)
- [ ] WP12c Macros run (later, if asked, as T3.7.4b was): the macro
  interpreter's language half shared from xlsx, Word's object model
  (`ActiveDocument`, `Paragraphs`, `Range`, `Selection`, `Find`,
  `Tables`, `Bookmarks`, `Content`) as the plugin's edits, one undo
  step; `Document_Open`, `AutoOpen` and `AutoExec` listed and never
  run; everything outside the document stops the macro with a message.

## WP13. Pages, printing and the terminal (later)

- [ ] WP13 Page view: a layout engine (line breaking with the fonts'
  real metrics, widows and orphans, headers and footers per page,
  footnotes at the page's foot, floating pictures and tables with text
  wrapping, columns, `PAGE` and `NUMPAGES` computed) rendering pages
  as bitmaps through `render(unit)` of the viewer contract; its fonts:
  the sandbox has none, so either a `fonts` interface of the host (the
  system's font files by name, a new permission shown at install) or
  metric-compatible substitutes bundled (Liberation for Times, Arial
  and Courier, under the component's size budget). Before that:
  printing and PDF of a document through Kalem's print path as a
  workbook prints (E45), the flow as LaTeX or Typst. Terminal parity
  (T3.7.8): the flow as styled text in the terminal editor, pictures
  as terminal images where kitty, iTerm2 or sixel are detected and as
  their alt text otherwise, tables as box lines; recorded in
  `book/part-4/terminal-parity.org`.

## WP14. Speed, size and the tests

- [~] WP14a Budgets, measured and written in the README: a 1,000-page
  document opens and shows its first screen in under a second, a
  typed character re-resolves one paragraph and rewrites one run, the
  save of a 40 MB `document.xml` splices in well under a second, the
  memory stays under the default 1 GB (the manifest asks for more only
  with a reason); the `.wasm` under 3 MB without picture decoders and
  under 6 MB with them (T3.7.11's size budget in `plugin.json`).
  (Measured 2026-10-09 on 20,000 paragraphs, 4.4 MB of
  `document.xml`: shown in 0.18 s in 82 MB, an edit and its save in
  0.17 s; the component is 710 kB. Open: a paragraph's edit re-reads
  its whole story, which a cache of the story's layout will end; a 40
  MB `document.xml` is not measured yet.)
- [~] WP14b The corpus (`tests/corpus/make.py`, as xlsx's): documents
  written by python-docx (MIT) with every construct of WP2 to WP7,
  then the same saved again by LibreOffice Writer headless as `.docx`
  (`MS Word 2007 XML`) and `.odt`, so two producers' ways of writing
  the same content are met; Word's own saves of them added by hand
  from the owner's Word; pandoc's docx test files fetched by the
  script and not committed (pandoc is GPL); every license recorded.
  The tests: byte-identical round trip of every file; the part diff
  and the run diff after each kind of edit; the resolved styles, the
  list labels, the tables' cells and the text against pandoc's docx
  reader (`pandoc -t json`; the `kalem diff-pandoc` pattern) and
  LibreOffice's text (`--convert-to txt`); LibreOffice headless opens
  every saved file (`--convert-to pdf` succeeds); the saved parts
  validated against the ECMA-376 transitional schemas in CI
  (`xmllint`, or the Open XML SDK's validator where `dotnet` is at
  hand), which catches the element-order mistakes Word repairs; Word
  itself opening the saved corpus without a repair prompt, by hand, as
  the exit criterion; every cut of a file failing without a panic; the
  conformance suite against the fake host (T3.1.17) when it exists.
  (Done 2026-10-09 in part: python-docx's file, the handmade
  file with every construct, and LibreOffice's saves of both, 53 tests;
  LibreOffice as the oracle by hand. Open: Word's own saves, pandoc's
  test files, the schemas' validation in CI, Word opening every saved
  file.)
- [x] WP14c The command line example (`examples/docx.rs`), until the
  flow world is in Kalem: `info` (parts, styles, sections, counts),
  `show` (the text with its styles as marks, the lists labeled),
  `styles`, `set FILE N TEXT` (a paragraph's text replaced), `vba`.
  (Done 2026-10-09: `info`, `show`, `text`, `outline`,
  `styles`, `paras`, `set`, `type`, `split`, `join`, `track`.)

## WP15. Release and the Book

- [x] WP15 `plugin.json` (`org.kalem.docx`, "Word documents", `opens`
  `.docx`, `.docm`, `.dotx`, `.dotm`, `activation` `onDocument`, `api`
  the flow interface's version, no permissions), the README in the
  shape of xlsx's (what it does, try it, tests, not yet), a line in
  the repository's README table and in `CODEOWNERS`, `index.json`
  regenerated, the tag `docx-v0.0.1` once Kalem's flow world is
  released; the chapter in Part III of the Book from
  `TEMPLATE-format.org` (the standard and the oracle, which files, what
  is read, shown, edited and written, export, known differences, not
  implemented, limits, code and tests), changed with the code in the
  same pull request (D53); R5.16's docx half ticked.
  (Done 2026-10-09: `plugin.json` on plugin API
  0.2.7's `flow-viewer` (`"api": "^0.2.7"`), the README, the line in
  the repository's README, in `CODEOWNERS` and in `index.json`; the
  Book's chapter "Word documents" in Part III, a pointer in Part I,
  and T3.7.5 and R5.16 with this plugin's progress (Kalem 5e22a05).
  The release build checked as the release workflow makes it (SIMD,
  the functions' names kept): 1.0 MB, the `flow-viewer` world's
  exports, no WASI import; installed into a Kalem of `main` from its
  folder, `kalem plugin check` says it runs, and Kalem's
  `flow_component` test opens, edits, undoes, saves and opens again
  each file of the corpus through it. Released 2026-10-09 as
  `docx-v0.0.1`: the component signed and attached to the GitHub
  release, its hash in `releases/` and its download in `index.json`.
  It runs with Kalem 0.6.0 (released 2026-10-09, plugin API 0.2.7) and
  later; an older Kalem refuses it. `docx-v0.0.2` the same day: comments
  added and answered, WP10a, and new documents, WP12a; `docx-v0.0.3`:
  comments resolved and deleted; `docx-v0.0.4` the next day: a
  comment's text edited, and formatting, WP9.)

## Open for the owner

- The shared crate's home: `crates/ooxml`, as proposed and done; or
  `plugins/ooxml` (no change to the workspace globs, but a library
  among the plugins).
- Whether a save updates `dcterms:modified` and `cp:revision` as Word
  does (WP8).
- Web layout only, or the page view of WP13 on the roadmap now.
