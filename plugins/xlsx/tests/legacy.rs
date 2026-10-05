//! `.xls` and `.ods` shown through calamine (T3.7.7): never written.

use std::path::PathBuf;

use kalem_plugin_xlsx::CellRef;
use kalem_plugin_xlsx::legacy::{LegacyFormat, LegacyWorkbook};

fn corpus(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/corpus")
            .join(name),
    )
    .unwrap()
}

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

#[test]
fn xls_and_ods_show_the_same_workbook() {
    for (f, format) in [
        ("libreoffice-budget.xls", LegacyFormat::Xls),
        ("libreoffice-budget.ods", LegacyFormat::Ods),
    ] {
        let wb = LegacyWorkbook::open(corpus(f)).unwrap();
        assert_eq!(wb.format, format, "{f}");
        let names: Vec<&str> = wb.sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Budget", "Dates", "Hidden"], "{f}");
        assert!(wb.sheets[2].hidden, "{f}");
        assert_eq!(wb.display(0, at("A3")), "Food", "{f}");
        assert_eq!(wb.display(0, at("D5")), "4293.75", "{f}");
        // calamine reads `.ods` formulas well; of `.xls` ones it misreads
        // areas, which are then not shown (the value is).
        let shown = wb.edit_text(0, at("D5"));
        assert!(
            shown == "=SUM(D2:D4)" || (format == LegacyFormat::Xls && shown == "4293.75"),
            "{f}: {shown}"
        );
        assert_eq!(wb.display(1, at("A1")), "2026-10-03", "{f}");
        // A date and time, and a time: numbers in both formats (an `.ods`
        // file's came as ISO text).
        assert_eq!(wb.display(1, at("A2")), "2026-10-03 14:30:00", "{f}");
        assert_eq!(wb.display(1, at("A3")), "09:15:00", "{f}");
        assert_eq!(wb.display(1, at("A7")), "Türkçe ğüşıöç İ", "{f}");
        assert_eq!(wb.grid(0)[0][0], "Item", "{f}");
    }
}
