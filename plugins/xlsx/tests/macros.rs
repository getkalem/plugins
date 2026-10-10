//! VBA macros run on the corpus workbook (T3.7.4b).

use std::path::PathBuf;

use kalem_plugin_xlsx::macros::{self, Limits, QuietHost};
use kalem_plugin_xlsx::vba::{Module, ModuleKind, Project};
use kalem_plugin_xlsx::{CellRef, Workbook};

fn book() -> Workbook {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/libreoffice-budget.xlsx");
    Workbook::open(std::fs::read(p).unwrap()).unwrap()
}

fn project(src: &str) -> Project {
    Project {
        name: "VBAProject".into(),
        modules: vec![Module {
            name: "Module1".into(),
            kind: ModuleKind::Standard,
            source: src.replace('\n', "\r\n"),
        }],
    }
}

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

fn run(wb: &mut Workbook, src: &str, name: &str) -> Result<macros::RunReport, macros::MacroError> {
    macros::run_macro(
        wb,
        &project(src),
        name,
        &mut QuietHost::default(),
        Limits::default(),
    )
}

#[test]
fn a_macro_fills_cells_and_formulas_recalculate() {
    let mut wb = book();
    let src = r#"
Sub Raise()
    Dim r As Long
    For r = 2 To 4
        Cells(r, 2).Value = Cells(r, 2).Value * 2
    Next r
    Range("A7").Value = "Total Q1"
    Range("B7").Formula = "=SUM(B2:B4)"
    Debug.Print Range("B7").Value, Worksheets("Budget").Range("D5").Text
End Sub
"#;
    let report = run(&mut wb, src, "Raise").unwrap();
    assert!(report.changed);
    assert_eq!(wb.display(0, at("B2")).unwrap(), "2,400.00");
    assert_eq!(wb.display(0, at("D2")).unwrap(), "3,600.00");
    assert_eq!(wb.display(0, at("A7")).unwrap(), "Total Q1");
    assert_eq!(wb.edit_text(0, at("B7")).unwrap(), "=SUM(B2:B4)");
    assert_eq!(report.output, ["3263 5,925.25"]);
    // One undo step for the whole run.
    assert!(wb.undo());
    assert_eq!(wb.display(0, at("B2")).unwrap(), "1,200.00");
    assert_eq!(wb.display(0, at("A7")).unwrap(), "");
    assert!(!wb.undo());
}

#[test]
fn the_language() {
    let mut wb = book();
    let src = r#"
Option Explicit
Private count As Long
Const BASE = 10

Function Fact(n As Long) As Double
    If n <= 1 Then Fact = 1 Else Fact = n * Fact(n - 1)
End Function

Sub Bump(ByRef x As Long, Optional ByVal by As Long = 1)
    x = x + by
End Sub

Sub Main()
    Dim i As Long, s As String, a() As Variant, d As Object, c As New Collection, k
    ReDim a(1 To 3)
    For i = 1 To 3: a(i) = i * BASE: Next
    ReDim Preserve a(1 To 4)
    a(4) = "x"
    s = Join(Array(a(1), a(2), a(3), a(4)), "-")
    Debug.Print s
    count = 5
    Bump count
    Bump count, by:=10
    Debug.Print count, Fact(10)
    Set d = CreateObject("Scripting.Dictionary")
    d("tr") = "Türkiye"
    d.Add "de", "Deutschland"
    For Each k In d.Keys
        c.Add k & "=" & d(k)
    Next
    Debug.Print c.Count, c(2), d.Exists("fr")
    Select Case count
        Case 1 To 10: s = "small"
        Case Is > 10: s = "big"
    End Select
    i = 0
    Do
        i = i + 1
        If i = 3 Then Exit Do
    Loop While True
    Debug.Print s, i, UCase(Left("kalem", 3)), Format(0.256, "0.0%"), 7 \ 2, 7 Mod 3, 2 ^ 10, "a" & 1 + 2
    On Error Resume Next
    i = 1 / 0
    Debug.Print Err.Number, Err.Description
    On Error GoTo Handler
    Err.Raise 1000, , "custom"
    Debug.Print "not reached"
    Exit Sub
Handler:
    Debug.Print "handled", Err.Number, Err.Description
End Sub
"#;
    let report = run(&mut wb, src, "Main").unwrap();
    assert_eq!(
        report.output,
        [
            "10-20-30-x",
            "16 3628800",
            "2 de=Deutschland False",
            "big 3 KAL 25.6% 3 1 1024 a3",
            "11 Division by zero",
            "handled 1000 custom",
        ]
    );
    assert!(!report.changed);
}

#[test]
fn ranges_rows_and_worksheet_functions() {
    let mut wb = book();
    let src = r#"
Sub Tidy()
    Dim last As Long, ws As Worksheet, cell As Range, total As Double
    Set ws = ThisWorkbook.Worksheets("Budget")
    last = ws.Cells(ws.Rows.Count, 1).End(xlUp).Row
    Debug.Print last, ws.Range("A1").CurrentRegion.Address, ws.UsedRange.Rows.Count
    For Each cell In ws.Range("B2:B4")
        total = total + cell.Value
    Next cell
    Debug.Print total, WorksheetFunction.Sum(ws.Range("B2:C4")), Application.WorksheetFunction.Max(1, 5, 3)
    Debug.Print Application.VLookup("Food", ws.Range("A2:D4"), 4, False)
    ws.Rows(3).Insert
    ws.Range("A3").Value = "Books"
    ws.Range("B3").Value = 100
    Debug.Print ws.Range("B6").Formula, ws.Range("B6").Value
    ws.Columns("C").Delete
    Debug.Print ws.Range("C2").Formula
    With ws.Range("A1")
        .Offset(0, 6).Value = "note"
        .Font.Bold = True
    End With
    Debug.Print ws.Range("G1").Value, ws.Range("A1").Address(False, False), ws.Cells(2, "B").Address
End Sub
"#;
    let report = run(&mut wb, src, "Tidy").unwrap();
    assert_eq!(
        report.output,
        [
            "5 $A$1:$D$5 5",
            "1631.5 4293.75 5",
            "943.75",
            "=SUM(B2:B5) 1731.5",
            "=B2+#REF!",
            "note A1 $B$2",
        ]
    );
    // `.Font.Bold = True` applied, nothing skipped.
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(style(&mut wb, "A1").bold);
}

#[test]
fn what_a_macro_cannot_do() {
    let mut wb = book();
    let e = run(&mut wb, "Sub A()\n  Shell \"rm -rf /\"\nEnd Sub", "A").unwrap_err();
    assert!(
        e.message.contains("Shell") || e.message.contains("shell"),
        "{e}"
    );
    assert_eq!(e.line, 2);
    let e = run(
        &mut wb,
        "Sub A()\n  Set o = CreateObject(\"WScript.Shell\")\nEnd Sub",
        "A",
    )
    .unwrap_err();
    assert!(e.message.contains("wscript.shell"), "{e}");
    let e = macros::run_macro(
        &mut wb,
        &project("Sub A()\n  Do\n  Loop\nEnd Sub"),
        "A",
        &mut QuietHost::default(),
        Limits {
            steps: 10_000,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(e.message.contains("steps"), "{e}");
    // A runtime error stops the macro; its edits so far are kept and undo as one.
    let e = run(
        &mut wb,
        "Sub A()\n  Range(\"A9\") = 1\n  x = 1 / 0\nEnd Sub",
        "A",
    )
    .unwrap_err();
    assert_eq!(e.line, 3);
    assert!(e.message.contains("Division by zero"));
    assert_eq!(wb.display(0, at("A9")).unwrap(), "1");
    assert!(wb.undo());
    assert_eq!(wb.display(0, at("A9")).unwrap(), "");
}

/// The style of a cell, as a macro's formatting leaves it.
fn style(wb: &mut Workbook, cell: &str) -> kalem_plugin_xlsx::styles::CellStyle {
    let s = wb
        .sheet(0)
        .unwrap()
        .cells
        .get(&at(cell))
        .map_or(0, |c| c.style);
    wb.style(s)
}

#[test]
fn a_macro_formats_cells() {
    let mut wb = book();
    let src = r#"
Sub Dress()
    With Range("A1:D1")
        .Font.Bold = True
        .Font.Color = RGB(255, 0, 0)
        .Font.Size = 14
        .Interior.Color = RGB(0, 0, 255)
        .HorizontalAlignment = xlCenter
        .Borders(xlEdgeBottom).LineStyle = xlContinuous
    End With
    Range("B2:B4").NumberFormat = "0.0%"
    Range("A2").Font.ColorIndex = 3
    Range("A3").Interior.ColorIndex = 6
    Range("A3").Interior.Pattern = xlNone
    Range("A4").WrapText = True
    Columns("C").ColumnWidth = 20
    Rows(3).RowHeight = 30
    Range("A6:B6").Merge
    Range("C2").BorderAround xlContinuous, xlThick
    Worksheets(1).Name = "Plan"
    Debug.Print Range("A1").Font.Bold, Range("A1").Font.Color, Range("B2").NumberFormat, _
        Range("A1").HorizontalAlignment = xlCenter, Range("A6").MergeCells, _
        Columns("C").ColumnWidth, Range("A1").Interior.ColorIndex
End Sub
"#;
    let report = run(&mut wb, src, "Dress").unwrap();
    assert!(report.changed);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let a1 = style(&mut wb, "A1");
    assert!(a1.bold);
    assert_eq!(a1.color, Some(0xFF0000));
    assert_eq!(a1.size, Some(14.0));
    assert_eq!(a1.fill, Some(0x0000FF));
    assert_eq!(a1.align.as_deref(), Some("center"));
    assert!(a1.sides[2].is_some(), "the bottom border");
    assert!(style(&mut wb, "D1").bold);
    assert_eq!(style(&mut wb, "B3").num_fmt, "0.0%");
    assert_eq!(wb.display(0, at("B3")).unwrap(), "43150.0%");
    assert_eq!(style(&mut wb, "A2").color, Some(0xFF0000));
    assert_eq!(style(&mut wb, "A3").fill, None);
    assert!(style(&mut wb, "A4").wrap);
    let sheet = wb.sheet(0).unwrap();
    assert!(
        sheet
            .cols
            .iter()
            .any(|c| c.min <= 2 && c.max >= 2 && c.width == Some(20.0))
    );
    assert_eq!(sheet.rows.get(&2).and_then(|r| r.height), Some(30.0));
    assert!(
        sheet
            .merged
            .iter()
            .any(|m| m.start == at("A6") && m.end == at("B6"))
    );
    assert!(
        style(&mut wb, "C2")
            .sides
            .iter()
            .all(|s| s.is_some_and(|(_, thick)| thick))
    );
    assert_eq!(wb.sheets()[0].name, "Plan");
    assert_eq!(report.output, ["True 255 0.0% True True 20 5"]);
    // The whole run is one undo step.
    assert!(wb.undo());
    assert_ne!(style(&mut wb, "A1").fill, Some(0x0000FF));
    assert_ne!(style(&mut wb, "B3").num_fmt, "0.0%");
    assert!(
        wb.sheet(0)
            .unwrap()
            .merged
            .iter()
            .all(|m| m.start != at("A6"))
    );
    assert_eq!(wb.sheets()[0].name, "Budget");
    // A sheet hidden by its Visible property.
    run(
        &mut wb,
        "Sub H()\n  Worksheets(2).Visible = xlSheetHidden\nEnd Sub",
        "H",
    )
    .unwrap();
    assert_ne!(
        wb.sheets()[1].visibility,
        kalem_plugin_xlsx::workbook::Visibility::Visible
    );
}

#[test]
fn error_handlers_resume() {
    let mut wb = book();
    let src = r#"
Function SafeDiv(a, b)
    On Error GoTo Fail
    SafeDiv = a / b
    Exit Function
Fail:
    SafeDiv = -1
    Resume Next
End Function

Function Retry(ByVal n As Long)
    On Error GoTo Again
    Retry = 12 / (2 - n)
    Exit Function
Again:
    n = n + 1
    Resume
End Function

Sub Sweep()
    Dim r As Long, bad As Long
    On Error GoTo Oops
    For r = 1 To 4
        Cells(r, 8).Value = 10 / (r - 2)
    Next r
    Debug.Print bad, Range("H1").Value, Range("H2").Value = "", Range("H4").Value, _
        SafeDiv(1, 0), SafeDiv(6, 3), Retry(2)
    Exit Sub
Oops:
    bad = bad + 1
    Resume Next
End Sub

Sub Falls()
    On Error GoTo H
    x = 1 / 0
    Range("H9").Value = "not reached"
H:
    Range("H8").Value = "handled " & Err.Number
End Sub

Sub Stray()
    Resume Next
End Sub
"#;
    // The loop goes on after the cell that failed; Resume tries again.
    let report = run(&mut wb, src, "Sweep").unwrap();
    assert_eq!(report.output, ["1 -10 True 5 -1 2 -12"]);
    // A handler that reaches End Sub ends the procedure.
    run(&mut wb, src, "Falls").unwrap();
    assert_eq!(wb.display(0, at("H8")).unwrap(), "handled 11");
    assert_eq!(wb.display(0, at("H9")).unwrap(), "");
    // Resume where no error was taken.
    let e = run(&mut wb, src, "Stray").unwrap_err();
    assert!(e.message.contains("Resume without error"), "{e}");
}

#[test]
fn formulas_in_r1c1_and_filled() {
    let mut wb = book();
    let src = r#"
Sub Recorded()
    Range("E2:E4").FormulaR1C1 = "=RC[-3]+RC[-2]"
    Range("F2:F4").Formula = "=B2*2"
    Range("E5").FormulaR1C1 = "=SUM(R[-3]C:R[-1]C)"
    Debug.Print Range("E3").Formula, Range("F4").Formula, Range("E5").FormulaR1C1, _
        Range("D2").FormulaR1C1, Range("E5").Value
End Sub
"#;
    let report = run(&mut wb, src, "Recorded").unwrap();
    assert_eq!(
        report.output,
        ["=B3+C3 =B4*2 =SUM(R[-3]C:R[-1]C) =RC[-2]+RC[-1] 4293.75"]
    );
    assert_eq!(wb.edit_text(0, at("E2")).unwrap(), "=B2+C2");
    assert_eq!(wb.edit_text(0, at("F2")).unwrap(), "=B2*2");
    assert_eq!(wb.display(0, at("F3")).unwrap(), "863");
}

#[test]
fn listing_macros() {
    let p = Project {
        name: "VBAProject".into(),
        modules: vec![
            Module { name: "ThisWorkbook".into(), kind: ModuleKind::Class, source: "Private Sub Workbook_Open()\nEnd Sub\nSub Workbook_BeforeClose(Cancel As Boolean)\nEnd Sub".into() },
            Module {
                name: "Module1".into(),
                kind: ModuleKind::Standard,
                source: "Sub Public1()\nEnd Sub\nPrivate Sub Hidden()\nEnd Sub\nSub WithArg(x)\nEnd Sub\nFunction F()\nEnd Function\nSub Auto_Open()\nEnd Sub".into(),
            },
        ],
    };
    let list = macros::list_macros(&p).unwrap();
    let names: Vec<(&str, bool)> = list
        .iter()
        .map(|m| (m.qualified.as_str(), m.event))
        .collect();
    assert_eq!(
        names,
        [("Module1.Public1", false), ("Module1.Auto_Open", true)]
    );
}
