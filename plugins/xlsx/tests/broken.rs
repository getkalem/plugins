//! Workbooks whose sheet XML is broken (a row numbered 0 or past the
//! sheet's last, a part cut off inside a formula) take rows and columns
//! inserted and deleted without a panic, which would take the plugin down
//! with the workbook.

use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_plugin_xlsx::Workbook;

/// openpyxl's budget workbook with its first sheet's XML changed by
/// `change`.
fn broken(change: impl Fn(&str) -> String) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/openpyxl-budget.xlsx");
    let mut pkg = Package::read(std::fs::read(p).unwrap()).unwrap();
    let xml = String::from_utf8(pkg.part("xl/worksheets/sheet1.xml").unwrap()).unwrap();
    pkg.set_part("xl/worksheets/sheet1.xml", change(&xml).into_bytes());
    pkg.write().unwrap()
}

/// Two rows and two columns inserted and deleted at the start of the
/// first sheet, each undone: the sheet still reads after each.
fn rows_and_columns_moved(input: Vec<u8>) {
    let mut wb = Workbook::open(input).unwrap();
    for kind in 0..4 {
        let done = match kind {
            0 => wb.insert_rows(0, 0, 2),
            1 => wb.delete_rows(0, 0, 2),
            2 => wb.insert_cols(0, 0, 2),
            _ => wb.delete_cols(0, 0, 2),
        };
        if done.is_ok() {
            wb.undo();
        }
        wb.grid(0).unwrap();
    }
}

#[test]
fn a_row_numbered_0() {
    rows_and_columns_moved(broken(|x| {
        x.replacen(r#"<row r="1">"#, r#"<row r="0">"#, 1)
    }));
}

#[test]
fn a_row_numbered_past_the_last() {
    rows_and_columns_moved(broken(|x| {
        x.replacen(r#"<row r="5">"#, r#"<row r="4294967295">"#, 1)
    }));
}

#[test]
fn a_sheet_cut_off_inside_a_formula() {
    rows_and_columns_moved(broken(|x| {
        x[..x.find("<f>B2+C2").unwrap() + "<f>B2".len()].to_owned()
    }));
}
