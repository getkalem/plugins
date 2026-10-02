Attribute VB_Name = "Budget"
Option Explicit

Sub AddVat()
    Dim r As Long, last As Long
    last = Cells(Rows.Count, 1).End(xlUp).Row
    Range("E1").Value = "With VAT"
    For r = 2 To last
        Cells(r, 5).Formula = "=D" & r & "*1.2"
    Next r
    Debug.Print "rows:", last - 1, "total with VAT:", Format(WorksheetFunction.Sum(Range("E2:E" & last)), "#,##0.00")
    If MsgBox("Add a row for Books?", vbYesNo + vbQuestion, "Budget") = vbYes Then
        Rows(3).Insert
        Range("A3:C3").Value = Array("Books", 100, 50)
        Range("D3").Formula = "=B3+C3"
    End If
End Sub

Private Sub Workbook_Open()
End Sub
