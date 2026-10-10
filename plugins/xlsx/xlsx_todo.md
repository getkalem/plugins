# xlsx: what is still missing

The list for the `xlsx` plugin of getkalem/plugins (`plugins/xlsx`,
crate `kalem-plugin-xlsx`, id `org.kalem.xlsx`, 0.0.9): Kalem's tasks
T3.7.4 and T3.7.4b, decisions D54, D55 and D56. Kalem's own Excel lists
say what the editor and the plugin do together:
`docs/history/excel_todo.md` and `excel_todo2.md` (E1–E36, done),
`docs/excel_todo3.md` (E37–E47 done, E48–E52 open). This list, written
2026-10-10, is what is left on the plugin's side and in the menus, found
by reading the plugin's source and README, the grid interface
(`grid.wit` and the `spreadsheet-viewer` world), the 278 `viewer.grid.*`
commands of Kalem's core, its menus (`crates/kalem-core/src/menus.rs`),
its toolbar and the grid's right-click menus, and the open workbook
items of `docs/publish_todo.md`. Where a task is Kalem's E48–E52, it is
named and not repeated.

As before, a task is done when it works in both editors, is written into
the file as Excel writes it, undoes in one step, and has its tests; every
command is in the palette and its keys can be bound again. A change to
the grid contract also goes into its interface, and a new release of the
plugin follows when the contract moves. Tests today: 173, passing, two
measurements ignored.

## In the file

### XL1. Dynamic arrays (Kalem's E48)

- [ ] XL1 Formulas whose result spills into the cells around them
  (`FILTER`, `SORT`, `SORTBY`, `UNIQUE`, `SEQUENCE`, `RANDARRAY`,
  `XLOOKUP` over a range, array arithmetic), the spill range drawn and
  referred to as `A1#`, `#SPILL!` where it is blocked, written as Excel
  365 writes them; `LET` and `LAMBDA` the engine knows, their spilled
  results it does not. Today a dynamic or CSE array result is never
  recalculated and the anchor cell of a dynamic array cannot be edited;
  a workbook on the 1904 date system gets no recalculation at all, so a
  formula typed into one shows blank (`publish_todo.md`).

### XL2. Links between workbooks (Kalem's E52)

- [ ] XL2 `externalLink` parts read: formulas referring to other
  workbooks (`[Sales.xlsx]Q1!B2`) kept, their last values shown and
  updated from the other file; Edit Links (update, change source,
  break); 3D references (`Sheet1:Sheet3!A1`) computed; `INDIRECT`
  across sheets; a link to another workbook followed open. Today the
  parts survive byte for byte and such formulas keep their stored
  results, untrusted.

### XL3. Functions the engine lacks

- [~] XL3 Of 286 functions an everyday workbook may use, the engine
  (494 functions, `functions.rs`) lacks 11: `HYPERLINK` (every list of
  links), `GETPIVOTDATA` (written by a click on a pivot table),
  `AGGREGATE`, `BAHTTEXT`, `ENCODEURL`; `GROUPBY` and `PIVOTBY` (365);
  and `WEBSERVICE`, `FILTERXML`, `IMAGE` and `STOCKHISTORY`, which reach
  the network and are never to compute. The legacy names (`RANK`,
  `MODE`, `VAR`, `PERCENTILE`, `QUARTILE`, `PERCENTRANK`, `STDEV`) are
  the engine's. A formula calling an unknown function computed
  `#NAME?`, and so did every formula reading it: after an edit of their
  other cells, those kept results no longer true. (Done 2026-10-10: a
  formula calling a function the engine lacks, and not one of the
  workbook's names (a `LAMBDA`), enters the engine as its stored
  result, so the formulas reading it compute from that result and the
  cell keeps it; `HYPERLINK(link, [name])` is given to the engine as
  `CHOOSE(n, link, [name])`, the value the cell shows; tests in
  `calc.rs`, `formula.rs` and `tests/workbook.rs`. Open: `AGGREGATE`,
  `GETPIVOTDATA`, `ENCODEURL`, `BAHTTEXT`, `GROUPBY` and `PIVOTBY`
  computed, the engine's work in getkalem/ironcalc.)

### XL4. Rich text in cells

- [ ] XL4 The runs of a shared or inline string (a bold word, a colored
  figure, two sizes in one cell) are joined into plain text on read and
  drawn in the cell's one font (`sheet.rs`, `parse_shared_strings`);
  an edit of such a cell writes plain text, so its runs are lost;
  phonetic runs are dropped. Read with their looks, drawn, kept through
  an edit of another part of the text, and made (a look given to part
  of the text while the cell is edited, written as `<r>` runs).

### XL5. Number formats, the rest

- [~] XL5 Colors in brackets (`[Red]` for the negative section, `[Blue]`)
  and locale tags (`[$-tr-TR]`, `[$€-x-euro2]`) were skipped on display
  (`numfmt.rs`); the `*` fill of Accounting (`_(* #,##0_)`, the sign
  pushed to the left edge, the number to the right) is drawn as a skip,
  not as a fill to the cell's width. (Done 2026-10-10: the eight named
  colors and `[Color1]` to `[Color56]` of the legacy palette, taken
  from the section that shows the value (by sign or condition; a text's
  from the text section, or from a code of one section), drawn over the
  font's color and a conditional format's, as Excel does; tests in
  `numfmt.rs` and `viewer.rs`. Open: the `*` fill, which needs the grid
  contract to say where a cell's text repeats a character to its width,
  for every format with such a fill (Calc's and Numbers' too); locale
  tags choosing the date names and the marks.)

### XL6. Charts, the rest (beside Kalem's E46)

- [ ] XL6 Office 2016 chart parts (`cx:chartSpace`, `chart.rs`
  `parse_chartex`): histograms and waterfalls are drawn, treemap,
  sunburst, funnel, box and whisker, Pareto and map charts are
  placeholders, and none of these kinds is made by Insert Chart.
  Drawn, and the ones Excel makes from a range (histogram, Pareto, box
  and whisker, waterfall, funnel, treemap, sunburst) made and written
  as `cx:` parts with their relationships and content types.

### XL7. Other formats (Kalem's E50)

- [~] XL7 Number formats and cell styles into and out of `.ods` (the
  Book says number formats are lost on conversion); a `.xls` file's
  formulas kept (only their results today, and calamine 0.36 misreads
  some BIFF8 formulas, which are then not shown) and `.xlsb` read the
  same way; workbooks encrypted with RC4 (Excel 97–2003, and 2007's
  CryptoAPI RC4) not read; Save as CSV with a chosen delimiter and
  encoding, a sheet or each sheet to its own file; export as HTML.
  (Done 2026-10-10 in Kalem's branch `xlsx-menus`: Export as CSV or
  Text, CSV UTF-8 with commas or semicolons, Text with tabs, CSV in
  Windows-1254 or Windows-1252 with `?` for what the code page lacks,
  the sheet shown or every visible worksheet to its own file. Open:
  the rest.)

### XL8. Macros, the rest (Kalem's T3.7.4b)

- [~] XL8 In the interpreter (`macros/excel.rs`, `interp.rs`):
  formatting statements applied (`Font`, `Interior`, `NumberFormat`,
  `ColumnWidth`, `RowHeight`: skipped and listed in the run's report
  today); `Worksheets.Add`, `Copy`, `Move` and `Delete`, a sheet
  renamed or hidden (`Worksheet.Name`, `Visible`), `Workbook.Names`,
  `FormulaR1C1`, `Range.Copy` without a destination (the clipboard),
  `Sort` and `AutoFilter`, `Resume` and `Resume Next` in handlers,
  labels inside blocks, user-defined `Type`s, class modules as objects,
  UserForms, events. In Kalem: a macro's run off the editor's thread
  (it blocks the editor for up to 60 s); View Code on a sheet tab and in
  the palette, the modules' source shown as the command line's `vba`
  shows it (Kalem has Run Macro only); the corpus of `.xlsm` files
  compared with Excel's runs. Editing the source means writing the
  project ([MS-OVBA]), which the plugin never rewrites: a decision for
  the owner, recorded either way. (Done 2026-10-10: formatting applied
  through the plugin's Format Cells, `Font` (bold, italic, underline,
  strikethrough, color, `ColorIndex`, size, name), `Interior` (color,
  `ColorIndex`, `Pattern = xlNone`), `Borders(side)` and
  `BorderAround`, `NumberFormat`, the alignments, `WrapText`,
  `IndentLevel`, `Orientation`, `ShrinkToFit`, `Locked`, `ColumnWidth`
  and `RowHeight` (on `Rows` and `Columns` too), `Merge`, `UnMerge` and
  `MergeCells`, each readable back; `RGB()` and Excel's alignment,
  border and font constants; `Worksheet.Name` and `Visible` set; one
  undo step for the run as before; tests in `tests/macros.rs`. Open:
  sheets added, deleted, copied and moved (their indexes shift under
  the macro's variables), and the rest of the list above.) (Done
  2026-10-10 too: `Resume` and `Resume Next` in handlers, the handler
  run where the error was so that a loop goes on, `Resume without
  error` (20) outside one; `Empty = ""` true, as VBA compares Empty
  with a string; `FormulaR1C1` read and written, and a formula set on
  several cells filled into them, relative references following.)

### XL9. Memory and speed (`publish_todo.md`, E38's last part)

- [~] XL9 Every edit pushes a snapshot of every loaded sheet's XML and
  cell model with no limit on the undo stack (`workbook.rs`, about
  150–200 MB per edit on a million-cell sheet); the first edit loads the
  whole workbook into IronCalc and each edit recalculates all of it
  (`calc.rs`); a style change over a sheet formatted row by row costs
  41 s; a million-cell workbook opens and recalculates in about a second
  each, in the editor's thread. The undo depth capped by bytes, the
  snapshot the changed part only; the dependents recalculated, not the
  workbook; the long work in the background with progress. (Done
  2026-10-10: the loaded sheets are shared between the states
  (`Arc`), so a snapshot holds only the sheets an edit changed, cloned
  when edited; the history keeps a hundred edits, as Excel, and no
  more than a gigabyte of what the states do not share, the oldest let
  go first (`set_undo_budget`); tested in `tests/workbook.rs`. Open:
  the results and trust maps copied with each state,
  the dependents recalculated instead of the workbook, and the work off
  the editor's thread, which is Kalem's.)

### XL10. Edits the interface allows and Excel has, not yet made

- [~] XL10 Insert Cells and Delete Cells with a shift as the plugin's
  own edit: Kalem emulates them by moving the block below or right
  (`move_cells`, a cut and paste), which is what the README's "Not yet"
  names and what macros' `Range.Insert Shift:=xlDown` needs; a row
  height or column width typed as a number and the default column width
  (the interface has `set-row-height` and `set-col-width`; Kalem's
  commands step by 3 points or fit); Create Names from Selection;
  conditional formats' Manage Rules (a rule edited, reordered, Stop If
  True; today a rule is added or every rule cleared); grouped sheets
  (Select All Sheets, one entry typed into every grouped sheet, a
  format given to all); Allow Users to Edit Ranges on a protected
  sheet; Insert Symbol; a sheet's background picture; timelines beside
  slicers; form controls and the checkbox cells of Excel 2024 shown,
  if only as placeholders; pictures placed in cells and the `IMAGE`
  function; Min, Max and Numerical Count in the status line beside
  Sum, Average and Count (done 2026-10-10 in Kalem's branch
  `xlsx-menus`: after Excel's three, so that eighty columns cut them
  first). (Done 2026-10-10 in the same branch too: Row Height and
  Column Width typed, every row or column selected in one undo step;
  Create from Selection, Ctrl+Shift+F3, names made as Excel makes
  them.)

### XL11. The generic halves of the contract

- [~] XL11 The `spreadsheet-viewer` world exports `annotations`, but
  the plugin keeps the trait's defaults for it (`annotations`,
  `comment`, `reply`, `resolve`, `remove_comment`, `set_comment_text`,
  `set_author`) and offers threaded comments through the grid's
  `threads` instead; the Review category's commands are scoped to flow
  documents, so a workbook's comments and notes are reached from
  `viewer.grid.*` only. Either the plugin implements `annotations` over
  its threads, so Word and Excel share one Review (its menu, panel,
  next and previous), or the world stops exporting it. `search`,
  `links`, `text_at` and `text_rects` are defaults too: the grid has
  its own find and links, which is fine, and should be recorded as the
  decision. (Done 2026-10-10: the plugin gives its threaded comments
  as annotations, each comment and answer by its ID on its cell
  (`Anchor::Cell`), and adds, answers, edits (the workbook's new
  `set_thread_comment_text`), resolves and removes them through the
  contract, by the author `set_author` gives; Kalem's branch
  `xlsx-menus` edits a comment's text from the grid's Comments through
  them. Open: the Review commands and their panel for workbooks in
  Kalem, and the decision on `search` and `links`.)

## In the menus (Kalem's side)

### XL12. The menu bar

- [x] XL12 No menu of the bar names a workbook: Kalem's ten menus
  (Kalem, File, Project, Edit, Format, Review, Insert, Table for CSV,
  BibTeX, View) carry none of the 278 `viewer.grid.*` commands, the
  plugin's manifest declares no `menus` (the git plugin's does), and
  the terminal's F10 list is the same list; the right-click menus, the
  keys and the palette are the only way to the commands. Menus are
  already filtered to the commands the open document offers
  (`menus_for`), so the CSV Table menu's way works: menus of
  `viewer.grid.*` items in `menus.rs`, shown only where offered, their
  labels the commands' titles (all 278 are in the `en` and `tr`
  locales already). Excel's order kept, so an Excel user finds things
  where they are:
  - File: Save Sheet as CSV, Export PDF, Page Setup, Print Preview,
    Print.
  - Edit: Paste Special, Paste as Text, Insert Copied Cells; Fill
    (Down, Right, Series, Justify, Flash Fill); Clear (Contents,
    Formats, All); Find, Replace, Find All, Find Next and Previous, in
    formulas, in notes, in the workbook; Go To, Go To Special; Select
    Row, Column, Visible Cells.
  - Format: a workbook block beside Org's, LaTeX's and the flow
    document's: Bold, Italic, Underline, Strikethrough, Font, Font
    Size, Font Color, Fill Color; the alignments, Indent, Text
    Rotation, Wrap Text, Shrink to Fit, Center Across Selection;
    Borders, Border Line, Border Color; Number Format, Increase and
    Decrease Decimal; Merge and Center, Merge, Unmerge; Cell Style,
    Format as Table, Conditional Formatting (the highlight rules, top
    and bottom, data bars, color scales, icon sets, clear); Format
    Cells; Format Painter; Row Height to Fit, Taller and Shorter Row,
    Column Width to Fit, Wider and Narrower Column, Autofit Columns;
    Hide and Unhide Rows and Columns; Theme; Lock Cells.
  - Insert: Sheet, Rows, Columns, Cells; Function, AutoSum; Chart,
    PivotTable, Sparklines, Slicer; Picture, Shape; Link; Note,
    Comment; Date, Time; Name.
  - Data: Sort Ascending and Descending, Custom Sort, Sort by Color;
    Filter, Filter by Condition, Filter by Color, Clear Filters,
    Reapply, Advanced Filter; Text to Columns, Flash Fill, Remove
    Duplicates; Data Validation (list, number, formula, message, alert,
    Circle Invalid, clear); Group, Ungroup, Show and Hide Detail,
    Subtotal; Goal Seek, Data Table, Scenarios; Refresh Pivots, Pivot
    Options; Total Row, Convert to Range.
  - Formulas: Insert Function, AutoSum; Define Name, Name Manager,
    Delete Name; Trace Precedents, Trace Dependents, Remove Arrows;
    Show Formulas, Error Checking, Evaluate Formula, Watch Window,
    Circular References; Calculate Now, Calculation Options.
  - Review: New Comment, Comments, Edit Note, Delete Note; Spelling;
    Protect Sheet, Protect Workbook (today the Review menu is the flow
    document's only).
  - View: Freeze Top Row, Freeze First Column, Freeze Panes, Unfreeze;
    Split; Zoom In, Out, 100 %, to a percentage; Gridlines, Headings,
    Show Formulas, Page Break Preview; Watch Window; Next and Previous
    Sheet, All Sheets, Hide and Unhide Sheet.
  - Chart: Chart Title, Axis Titles, Legend, Data Labels, Labels from
    Cells; Chart Kind, Series Kind, Trendline, Error Bars; Axis Scale,
    Axis Format, the fonts; Series Color, Point Color, Explode Slice,
    Chart Area, Plot Area, Gridlines; Move and Resize, Move to a Sheet,
    Save and Apply Template; Delete Chart.
  - Macro: Run Macro.

  (Done 2026-10-10 in Kalem's branch `xlsx-menus`: the workbook's
  items in File, Edit, Format, Insert, Review and View, the new menus
  Data, Formulas and Chart (`menu-data`, `menu-formulas`,
  `menu-chart` in both locales), Run Macro in View; the document
  context the menus are built from gains `viewerGrid` and
  `hasComments`, so a PDF's or a picture's menus carry none of them,
  and the text's Find, Find and Replace and Select All leave a grid
  (`!viewerGrid`) for the grid's own; tests in `menus.rs` and both
  editors' workbook tests. The commands on a text (the line commands,
  Go to Matching Bracket, Trim Trailing Whitespace, Go to Line, Source
  View, the encodings) leave every viewer's menus and keys, through
  the when-clause `editorMode != viewer`; Paste as Plain Text stays,
  as it pastes into the cells.)

### XL13. The toolbar

- [x] XL13 The graphical editor's toolbar has buttons for Org, LaTeX,
  CSV and the flow document (bold, italic, styles, typefaces, sizes),
  none for the grid: Bold, Italic, Underline; Borders; Fill Color, Font
  Color; Currency, Percent, Comma, Increase and Decrease Decimal; Merge
  and Center, Wrap Text, the alignments; Sort, Filter; AutoSum; Insert
  Chart. The formula bar is there already. (Done 2026-10-10 in
  Kalem's branch `xlsx-menus`: those twenty buttons, shown where the
  document offers their commands, Currency, Percent and Comma Style
  through Number Format's codes; tested in the graphical editor.)

### XL14. Right-click menus, the rest (E37's remainder)

- [~] XL14 The cells menu (`viewer.rs`, `context_menu`) lacks Format
  Cells, Pick From Drop-down List, Filter by Selected Cell's Value and
  Color, Sort by Color and Custom Sort, Delete Note and the comment
  thread, Open Link and Remove Link, the table items (insert and
  delete table rows and columns, Total Row, Convert to Range), the
  pivot items (Refresh, PivotTable Options) and Show and Hide Detail
  on grouped rows; the rows and columns menus lack Paste Special,
  Format Cells, a height or width typed as a number, Ungroup and
  Standard Width; the tab menu lacks View Code and Select All Sheets.
  No menu at all on a chart (Excel's: chart kind, select data, move
  chart, format, data labels, trendline, delete), a picture or shape,
  a pivot table, a slicer, a sparkline, the formula bar, the Name Box
  or the status line (which of Sum, Average, Count, Min and Max it
  shows). (Done 2026-10-10 in Kalem's branch `xlsx-menus`: the cells
  menu built from what is at the cursor: Format Cells, Filter by
  Selected Cell's Color, Reapply, Sort by Color and Custom Sort always;
  Edit and Delete Note on a note, New Note elsewhere; Open, Edit and
  Remove Link on a link; Pick From Drop-down List on a list; a
  table's Total Row and Convert to Range; a pivot table's Refresh and
  Options; Show Comments on a thread; the rows' and columns' menus
  with Paste Special, Format Cells and Ungroup, Row Height and Column
  Width typed; tested in the terminal. Open: Standard Width, View Code,
  Select All Sheets, and the menus of charts, pictures, slicers,
  sparklines, the formula bar, the Name Box and the status line.)

## Checks and documents

### XL15. The oracle, the chapter and the README

- [~] XL15 Excel itself opening edited files without a repair prompt
  (T3.7.4's exit criterion, checked by hand only); LibreOffice as the
  CI oracle for recalculated files (used by hand); the `.xlsm` corpus
  compared with Excel's runs; the Book's workbook chapter (T3.7.10:
  the Book has a paragraph in part 1 and no chapter), where
  `fullCalcOnLoad`, the dropped calculation chain and the rewritten
  cached results are to be said (`publish_todo.md`'s Doc item). (The
  README's "Not yet" list brought up to date 2026-10-10; the Book's
  chapter "Workbooks" written the same day in Kalem's branch
  `xlsx-menus`, `book/part-3/workbooks.org`, with what a save writes
  and limits measured. Open: Excel's no-repair check, LibreOffice in
  CI, the `.xlsm` corpus.)
