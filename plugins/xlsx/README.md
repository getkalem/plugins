# Excel workbooks (`xlsx`)

Opens, edits and saves Excel workbooks (SpreadsheetML, ECMA-376 part 1: `.xlsx`, `.xlsm`, `.xltx`, `.xltm`) **as themselves**. Nothing is converted: the file is a ZIP package of XML parts, and the plugin reads and writes those parts. Kalem's task T3.7.4; decisions D54, D55 and D56 of its design document.

## What it does

- **Reads** sheets, cell values, shared and inline strings, formulas (shared formulas expanded), number formats (dates, times, percent, currency, fractions, scientific, sections and conditions, and the colors they give, `[Red]` or `[Color10]`, drawn over the font's), fonts, fills, alignment, borders, theme colors, merged cells, frozen panes, column widths, hidden rows, columns and sheets, defined names and comments.
- **Edits** a cell the way typing in Excel does: `=` starts a formula, `'` forces text, `1,234.5`, `50%`, `2026-10-03` and `14:30` are numbers (and a General cell gets the number format Excel would give it). Only the cell's `<c>` element is rewritten; its style is kept. Edits Excel refuses (inside a merged cell, part of an array formula or a data table) are refused.
- **Recalculates** with [IronCalc](https://www.ironcalc.com) and writes the new results of the formulas an edit changed, so LibreOffice, which does not recalculate on open, and every viewer that shows stored results agree with Excel. The engine is trusted cell by cell: before an edit it computes the whole workbook, and only cells whose result equals the one stored in the file get new results; the others lose their stored result and are computed by Excel or LibreOffice on open. A formula calling a function the engine does not compute (`GETPIVOTDATA`, `AGGREGATE`, `WEBSERVICE`, `IMAGE`, `GROUPBY`) keeps its stored result, and the formulas reading it compute from that result; `HYPERLINK` shows its friendly name, or its address. `calcPr fullCalcOnLoad` is set either way.
- **Keeps everything else**: a save without edits returns the input byte for byte; after an edit only the edited parts are written, and every other ZIP entry (charts, pivot tables, drawings, images, the VBA project) is copied byte for byte, local header included. When a formula is removed, the calculation chain is dropped, as the specification allows and Excel rebuilds it.
- **Rows and columns** are inserted and deleted as Excel does it: every reference to the sheet moves (formulas on every sheet, absolute ones too, ranges grown and shrunk, `#REF!` where cells are gone), and so do merged cells, hyperlinks, conditional formats, validations, the filter, column widths, defined names, chart series, comments and their note shapes, drawing anchors, tables and pivot sources. Excel's refusals are kept. Every edit undoes and redoes; undoing everything saves the file as it was read.
- **A password to open** ([MS-OFFCRYPTO]): a workbook encrypted by Excel 2010 or later (agile encryption, AES with SHA-512) or Excel 2007 (standard encryption, AES-128 with SHA-1) opens with its password, which Kalem asks for; one encrypted with Excel's default password opens unasked. It is saved encrypted again with the password it opened with: agile encryption, AES-256 and SHA-512 spun 100,000 times, with the data integrity check and the data spaces Excel writes, in a compound file of version 3 (LibreOffice opens such files, and none of version 4). The key and salts come from Kalem's `clock.random`. Saved as `.ods`, it is not encrypted, and the save says so. RC4 encryption is not read.
- **`.xls`, `.xlsb` and `.ods`** are read through calamine, in the component too, and never written by the plugin: Kalem converts such a file to edit it, and asks before saving it back in its own format. calamine 0.36 misreads some BIFF8 formulas; those are not shown, their values are.
- **Macros**: `xl/vbaProject.bin` is kept byte for byte, its modules' source is read ([MS-OVBA]), and macros **run** inside the plugin (`macros::run_macro`): VBA's language (procedures and functions, recursion, `ByRef`/`ByVal`, optional and named arguments, `ParamArray`, arrays with `ReDim Preserve`, `If`, `For`, `For Each`, `Do`, `While`, `Select Case`, `With`, `On Error Resume Next` and `On Error GoTo` with `Resume`, `Resume Next` and `Resume label` (the handler runs where the error was, inside its loops), `Enum`, constants) and about 120 library functions; Excel's object model for data (`Application`, `ThisWorkbook`, `Worksheets`, `Range`, `Cells`, `Rows`, `Columns`, `Value`, `Formula` (filled into a range as Excel fills it), `FormulaR1C1`, `Text`, `Address`, `Offset`, `Resize`, `End`, `CurrentRegion`, `UsedRange`, `Find`, `Copy`, `ClearContents`, `EntireRow.Insert/Delete`, `Intersect`, `Evaluate`, `[A1]`), every Excel function through `WorksheetFunction` and `Application` (computed by IronCalc), `Collection` and `Scripting.Dictionary`, `MsgBox` and `InputBox` through the host, `Debug.Print`. A run is one undo step. A macro runs only when the user asks for it by name; event handlers (`Workbook_Open`, `Auto_Open`, `Worksheet_Change`) are listed and never run by themselves. Files, network, `Shell`, `Declare`, `CreateObject` beyond `Scripting.Dictionary`, other workbooks and `SendKeys` stop the macro with a message naming the call and the line; a step and time budget stops endless loops. Formatting is applied as Format Cells applies it: `Font` (bold, italic, underline, strikethrough, color, `ColorIndex`, size, name), `Interior` (color, `ColorIndex`, no fill), `Borders` and `BorderAround`, `NumberFormat`, the alignments, `WrapText`, `IndentLevel`, `Orientation`, `ShrinkToFit`, `Locked`, `ColumnWidth` and `RowHeight`, `Merge` and `UnMerge`, with `RGB` and Excel's constants; a sheet is renamed and hidden through its `Name` and `Visible`. What is not modelled (`Characters`, `FormatConditions`, theme colors, `AutoFit`) is skipped and listed in the run's report.

## Try it

Until Kalem's plugin host and the `document-viewer` contract (T3.1, T3.7.1) are published, the library comes with a command line:

```sh
cargo run -p kalem-plugin-xlsx --example xlsx -- info  book.xlsx
cargo run -p kalem-plugin-xlsx --example xlsx -- show  book.xlsx [SHEET]
cargo run -p kalem-plugin-xlsx --example xlsx -- get   book.xlsx 'Sheet1!D2'
cargo run -p kalem-plugin-xlsx --example xlsx -- set   book.xlsx 'Sheet1!B2' 1300 [-o out.xlsx]
cargo run -p kalem-plugin-xlsx --example xlsx -- insert-rows book.xlsx Sheet1 3 2
cargo run -p kalem-plugin-xlsx --example xlsx -- vba   book.xlsm
cargo run -p kalem-plugin-xlsx --example xlsx -- macros book.xlsm
cargo run -p kalem-plugin-xlsx --example xlsx -- run   book.xlsm Module1.Fill [-o out.xlsm]
cargo run -p kalem-plugin-xlsx --example xlsx -- show  old.xls
```

Without Excel at hand, `attach-vba book.xlsx examples/Budget.bas out.xlsm` puts a module into a copy of a workbook to try `macros` and `run` on (the project it writes is enough for Kalem, not for Excel).

## Tests

`cargo test -p kalem-plugin-xlsx`. The corpus in `tests/corpus/` is generated by `tests/corpus/make.py` (openpyxl, then the same workbook saved by LibreOffice Calc) and licensed as this repository. The tests check byte-identical round trips, which parts and which cells an edit changes, values through their formats, and the engine. LibreOffice was used by hand as the oracle for recalculated files (`soffice --convert-to csv`); making that a CI step is open.

## Not yet

- Cells inserted and deleted on part of a row or column as an edit of their own: Kalem moves the cells below or to the right, so a range crossing the edge does not grow or shrink as in Excel, and macros' `Range.Insert Shift:=xlDown` is refused; rich text in cells (its runs are read as plain text, and an edit of the cell drops them); a threaded comment's text edited (comments are added, answered, resolved and deleted, notes edited).
- In macros: sheets added, deleted, copied and moved, sorting and filtering, `AutoFit`, labels inside blocks, user-defined `Type`s, class modules as objects, UserForms, events.
- Dynamic arrays (`FILTER`, `SORT`, `UNIQUE`, `SEQUENCE` spilling, `A1#`), links to other workbooks, and the functions the engine lacks computed; the full list is [`xlsx_todo.md`](xlsx_todo.md).
- A test in Microsoft Excel itself that edited files open without a repair prompt (the exit criterion of T3.7.4), and macros compared with Excel's runs on a corpus of `.xlsm` files.
