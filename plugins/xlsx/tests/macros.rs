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
    assert_eq!(report.skipped.len(), 2, "{:?}", report.skipped);
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
