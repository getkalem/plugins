//! Workbooks of the corpus opened, shown, edited and saved.
//!
//! The rules under test are those of Kalem's task T3.7.4: a save without
//! edits is byte-identical; after an edit only the touched parts differ,
//! and every other entry of the ZIP is copied byte for byte.

use std::path::PathBuf;

use kalem_plugin_xlsx::package::Package;
use kalem_plugin_xlsx::{CellRef, Error, Value, Workbook};

fn corpus(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

const FILES: [&str; 3] = [
    "openpyxl-budget.xlsx",
    "libreoffice-budget.xlsx",
    "openpyxl-empty.xlsx",
];

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

/// The parts whose inflated bytes differ between two packages, and the
/// parts present in one only.
fn part_diff(a: &[u8], b: &[u8]) -> Vec<String> {
    let (pa, pb) = (
        Package::read(a.to_vec()).unwrap(),
        Package::read(b.to_vec()).unwrap(),
    );
    let mut out = Vec::new();
    for n in pa.names() {
        if !pb.contains(&n) {
            out.push(format!("-{n}"));
        } else if pa.part(&n).unwrap() != pb.part(&n).unwrap() {
            out.push(n);
        }
    }
    for n in pb.names() {
        if !pa.contains(&n) {
            out.push(format!("+{n}"));
        }
    }
    out.sort();
    out
}

/// The cells whose `<c>` differs between two saves of a sheet part, after
/// checking that everything outside the cells is byte-identical.
fn changed_cells(a: &[u8], b: &[u8], part: &str) -> Vec<String> {
    let text = |bytes: &[u8]| {
        String::from_utf8(Package::read(bytes.to_vec()).unwrap().part(part).unwrap()).unwrap()
    };
    let (ta, tb) = (text(a), text(b));
    let cells = |t: &str| -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut rest = t;
        while let Some(p) = rest.find("<c r=\"") {
            let after = &rest[p..];
            let end = match (after.find("/>"), after.find("</c>")) {
                (Some(e), Some(c)) if e < after.find('>').unwrap_or(0) + 1 => e.min(c) + 2,
                (_, Some(c)) => c + 4,
                (Some(e), None) => e + 2,
                (None, None) => after.len(),
            };
            let el = &after[..end];
            let r = el[6..].split('"').next().unwrap().to_owned();
            out.push((r, el.to_owned()));
            rest = &after[end..];
        }
        out
    };
    let skeleton = |t: &str| -> String {
        let mut s = t.to_owned();
        for (_, el) in cells(t) {
            s = s.replacen(&el, "<c/>", 1);
        }
        s
    };
    assert_eq!(skeleton(&ta), skeleton(&tb), "only cells change");
    let (ca, cb) = (cells(&ta), cells(&tb));
    ca.iter()
        .zip(&cb)
        .filter(|(x, y)| x != y)
        .map(|(x, _)| x.0.clone())
        .collect()
}

#[test]
fn unedited_save_is_byte_identical() {
    for f in FILES {
        let bytes = corpus(f);
        let mut wb = Workbook::open(bytes.clone()).unwrap();
        for i in 0..wb.sheets().len() {
            let _ = wb.grid(i);
        }
        assert_eq!(wb.save().unwrap(), bytes, "{f}");
        assert!(!wb.is_dirty());
    }
}

#[test]
fn values_are_shown_through_their_formats() {
    for f in ["openpyxl-budget.xlsx", "libreoffice-budget.xlsx"] {
        let mut wb = Workbook::open(corpus(f)).unwrap();
        let names: Vec<&str> = wb.sheets().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Budget", "Dates", "Hidden"], "{f}");
        assert_eq!(wb.display(0, at("B3")).unwrap(), "431.50", "{f}");
        assert_eq!(wb.display(0, at("B2")).unwrap(), "1,200.00", "{f}");
        assert_eq!(wb.edit_text(0, at("D2")).unwrap(), "=B2+C2", "{f}");
        assert_eq!(wb.display(1, at("A1")).unwrap(), "2026-10-03", "{f}");
        assert_eq!(wb.edit_text(1, at("A1")).unwrap(), "2026-10-03", "{f}");
        assert_eq!(wb.display(1, at("A4")).unwrap(), "25.6%", "{f}");
        assert_eq!(wb.display(1, at("A5")).unwrap(), "TRUE", "{f}");
        assert_eq!(wb.display(1, at("A7")).unwrap(), "Türkçe ğüşıöç İ", "{f}");
        assert_eq!(wb.display(1, at("A8")).unwrap(), "  spaced  ", "{f}");
        let comments = wb.comments(0).unwrap();
        assert_eq!(comments.len(), 1, "{f}");
        assert_eq!(comments[0].cell, at("A2"));
        assert!(comments[0].text.contains("Paid on the first"), "{f}");
        let sheet = wb.sheet(0).unwrap();
        assert!(sheet.has_drawing, "{f}");
        assert_eq!(sheet.merged.len(), 1, "{f}");
        assert_eq!(sheet.col_width(0), Some(18.0), "{f}");
        let s = wb.sheet(0).unwrap().cells[&at("A1")].style;
        let style = wb.style(s);
        assert!(style.bold, "{f}");
        assert_eq!(style.fill, Some(0x4472C4), "{f}");
    }
    // LibreOffice wrote the cached results.
    let mut wb = Workbook::open(corpus("libreoffice-budget.xlsx")).unwrap();
    assert_eq!(wb.display(0, at("D5")).unwrap(), "4,293.75");
    assert_eq!(wb.display(1, at("A6")).unwrap(), "#DIV/0!");
}

#[test]
fn a_value_edit_touches_its_sheet_and_the_calc_flag_only() {
    for f in ["openpyxl-budget.xlsx", "libreoffice-budget.xlsx"] {
        let bytes = corpus(f);
        let mut wb = Workbook::open(bytes.clone()).unwrap();
        wb.set_cell(0, at("B2"), "1300").unwrap();
        let out = wb.save().unwrap();
        let diff = part_diff(&bytes, &out);
        // openpyxl sets the flag itself; then workbook.xml stays as it was.
        let expected: &[&str] = if f.starts_with("openpyxl") {
            &["xl/worksheets/sheet1.xml"]
        } else {
            &["xl/workbook.xml", "xl/worksheets/sheet1.xml"]
        };
        assert_eq!(diff, expected, "{f}");
        // The edited sheet differs in the edited cell and in the cells whose
        // results it changed (LibreOffice stored results; openpyxl did not).
        let changed = changed_cells(&bytes, &out, "xl/worksheets/sheet1.xml");
        let expected: &[&str] = if f.starts_with("openpyxl") {
            &["B2"]
        } else {
            &["B2", "D2", "B5", "D5"]
        };
        assert_eq!(changed, expected, "{f}");
        let mut again = Workbook::open(out).unwrap();
        assert_eq!(again.display(0, at("B2")).unwrap(), "1,300.00", "{f}");
        assert_eq!(again.edit_text(0, at("D2")).unwrap(), "=B2+C2", "{f}");
        assert_eq!(again.display(0, at("D2")).unwrap(), "2,500.00", "{f}");
        assert_eq!(again.display(0, at("D5")).unwrap(), "4,393.75", "{f}");
        let wbxml = String::from_utf8(
            Package::read(again.save().unwrap())
                .unwrap()
                .part("xl/workbook.xml")
                .unwrap(),
        )
        .unwrap();
        assert!(wbxml.contains("fullCalcOnLoad=\"1\""), "{f}");
    }
}

#[test]
fn new_cells_rows_and_entries() {
    let bytes = corpus("libreoffice-budget.xlsx");
    let mut wb = Workbook::open(bytes.clone()).unwrap();
    // A new cell in an existing row, between two others.
    wb.set_cell(0, at("E2"), "between").unwrap();
    // A new row past the end, and one between existing rows does not exist
    // here; a row far below.
    wb.set_cell(0, at("B10"), "=SUM(B2:B4)").unwrap();
    wb.set_cell(0, at("C8"), "2026-10-03").unwrap();
    wb.set_cell(0, at("A9"), "50%").unwrap();
    wb.set_cell(0, at("A7"), "a < b & \"c\"").unwrap();
    wb.set_cell(0, at("A6"), "'0042").unwrap();
    let out = wb.save().unwrap();
    let mut again = Workbook::open(out.clone()).unwrap();
    assert_eq!(again.display(0, at("E2")).unwrap(), "between");
    assert_eq!(again.edit_text(0, at("B10")).unwrap(), "=SUM(B2:B4)");
    assert_eq!(again.display(0, at("C8")).unwrap(), "10/3/2026");
    assert_eq!(again.display(0, at("A9")).unwrap(), "50%");
    assert_eq!(again.display(0, at("A7")).unwrap(), "a < b & \"c\"");
    assert_eq!(again.display(0, at("A6")).unwrap(), "0042");
    assert_eq!(again.edit_text(0, at("A6")).unwrap(), "'0042");
    let sheet = again.sheet(0).unwrap();
    let rows: Vec<u32> = sheet.rows.keys().copied().collect();
    assert_eq!(rows, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let cols: Vec<u32> = sheet
        .cells
        .keys()
        .filter(|c| c.row == 1)
        .map(|c| c.col)
        .collect();
    assert_eq!(cols, [0, 1, 2, 3, 4]);
    // The date and the percent needed number formats: styles.xml changed too.
    let diff = part_diff(&bytes, &out);
    assert_eq!(
        diff,
        [
            "xl/styles.xml",
            "xl/workbook.xml",
            "xl/worksheets/sheet1.xml"
        ]
    );
    // The untouched sheet keeps every other cell.
    assert_eq!(again.display(0, at("D5")).unwrap(), "4,293.75");
}

#[test]
fn clearing_a_formula_drops_nothing_else() {
    let bytes = corpus("libreoffice-budget.xlsx");
    let mut wb = Workbook::open(bytes).unwrap();
    wb.set_cell(0, at("D2"), "").unwrap();
    let out = wb.save().unwrap();
    let mut again = Workbook::open(out).unwrap();
    // The style stays on an emptied cell.
    let c = again.sheet(0).unwrap().cells[&at("D2")].clone();
    assert_eq!(c.value, Value::Empty);
    assert!(c.formula.is_none());
    assert_ne!(c.style, 0);
}

#[test]
fn edits_excel_would_refuse_are_refused() {
    let mut wb = Workbook::open(corpus("libreoffice-budget.xlsx")).unwrap();
    assert!(matches!(
        wb.set_cell(0, at("G1"), "x"),
        Err(Error::Refused(_))
    ));
    assert!(wb.set_cell(0, at("F1"), "x").is_ok());
}

#[test]
fn the_empty_sheet_takes_a_first_cell() {
    let bytes = corpus("openpyxl-empty.xlsx");
    let mut wb = Workbook::open(bytes).unwrap();
    wb.set_cell(0, at("C3"), "42").unwrap();
    let mut again = Workbook::open(wb.save().unwrap()).unwrap();
    assert_eq!(again.display(0, at("C3")).unwrap(), "42");
    assert_eq!(
        again.sheet(0).unwrap().used_range().unwrap().to_string(),
        "C3"
    );
}

fn part_text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(Package::read(bytes.to_vec()).unwrap().part(name).unwrap()).unwrap()
}

#[test]
fn inserting_a_row_moves_everything_that_points_at_the_sheet() {
    for f in ["libreoffice-budget.xlsx", "openpyxl-budget.xlsx"] {
        let mut wb = Workbook::open(corpus(f)).unwrap();
        // Before row 3 (Food).
        wb.insert_rows(0, 2, 1).unwrap();
        wb.set_cell(0, at("A3"), "Books").unwrap();
        wb.set_cell(0, at("B3"), "100").unwrap();
        let out = wb.save().unwrap();
        let mut again = Workbook::open(out.clone()).unwrap();
        assert_eq!(again.display(0, at("A4")).unwrap(), "Food", "{f}");
        assert_eq!(again.edit_text(0, at("D4")).unwrap(), "=B4+C4", "{f}");
        assert_eq!(again.edit_text(0, at("B6")).unwrap(), "=SUM(B2:B5)", "{f}");
        assert_eq!(again.display(0, at("B6")).unwrap(), "1,731.50", "{f}");
        let names = again.defined_names().to_vec();
        assert_eq!(names[0].refers_to, "Budget!$B$2:$B$5", "{f}");
        assert!(
            part_text(&out, "xl/charts/chart1.xml").contains("$B$2:$B$5"),
            "{f}"
        );
        assert_eq!(again.comments(0).unwrap()[0].cell, at("A2"), "{f}");
        assert_eq!(
            again.sheet(0).unwrap().merged[0].to_string(),
            "F1:G1",
            "{f}"
        );
        assert!(!Package::read(out).unwrap().contains("xl/calcChain.xml"));
    }
}

#[test]
fn deleting_rows_and_columns_recalculates() {
    let mut wb = Workbook::open(corpus("libreoffice-budget.xlsx")).unwrap();
    // Food goes.
    wb.delete_rows(0, 2, 1).unwrap();
    assert_eq!(wb.display(0, at("A3")).unwrap(), "Travel");
    assert_eq!(wb.edit_text(0, at("B4")).unwrap(), "=SUM(B2:B3)");
    assert_eq!(wb.display(0, at("B4")).unwrap(), "1,200.00");
    assert_eq!(wb.display(0, at("D4")).unwrap(), "3,350.00");
    // Column A goes: its note too.
    wb.delete_cols(0, 0, 1).unwrap();
    assert_eq!(wb.edit_text(0, at("C2")).unwrap(), "=A2+B2");
    assert!(wb.comments(0).unwrap().is_empty());
    let out = wb.save().unwrap();
    let text = part_text(&out, "xl/worksheets/sheet1.xml");
    assert!(text.contains("<mergeCell ref=\"E1:F1\"/>"), "{text}");
    let mut again = Workbook::open(out).unwrap();
    assert_eq!(again.display(0, at("C4")).unwrap(), "3,350.00");
}

#[test]
fn undo_returns_to_the_file_as_read() {
    let bytes = corpus("libreoffice-budget.xlsx");
    let mut wb = Workbook::open(bytes.clone()).unwrap();
    wb.set_cell(0, at("B2"), "5").unwrap();
    wb.insert_cols(0, 1, 2).unwrap();
    wb.delete_rows(0, 0, 1).unwrap();
    assert_eq!(wb.can_undo_redo(), (true, false));
    assert!(wb.undo() && wb.undo() && wb.undo());
    assert!(!wb.undo());
    assert_eq!(wb.save().unwrap(), bytes);
    assert!(wb.redo());
    assert_eq!(wb.display(0, at("B2")).unwrap(), "5.00");
    assert_eq!(wb.display(0, at("D2")).unwrap(), "1,205.00");
}

#[test]
fn structural_edits_excel_refuses() {
    let mut wb = Workbook::open(corpus("libreoffice-budget.xlsx")).unwrap();
    assert!(matches!(
        wb.insert_rows(0, 0, 1_048_576),
        Err(Error::Refused(_))
    ));
}
