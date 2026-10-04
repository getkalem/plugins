//! How long large edits take: a measurement, run with
//! `cargo test --release -p kalem-plugin-xlsx --test speed -- --ignored --nocapture`.
#![allow(clippy::print_stdout)]

use std::time::Instant;

use kalem_plugin_xlsx::{CellRef, Range, Workbook};

fn book() -> Workbook {
    let p =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/openpyxl-budget.xlsx");
    Workbook::open(std::fs::read(p).unwrap()).unwrap()
}

fn rows(n: usize, cols: usize) -> Vec<Vec<String>> {
    (0..n)
        .map(|r| {
            (0..cols)
                .map(|c| match c {
                    0 => format!("Item {r}"),
                    _ if c == cols - 1 => format!("=SUM(B{}:K{})", r + 10, r + 10),
                    _ => format!("{}", (r * 7 + c * 13) % 1000),
                })
                .collect()
        })
        .collect()
}

#[test]
#[ignore = "a measurement"]
fn large_edits() {
    let mut wb = book();
    let time = |label: &str, f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        println!("{label}: {} ms", t.elapsed().as_millis());
    };
    let data = rows(3000, 12);
    time("paste 36,000 cells", &mut || {
        wb.set_cells(0, CellRef::new(9, 0), &data).unwrap();
    });
    let big = rows(10_000, 10);
    time("paste 100,000 cells", &mut || {
        wb.set_cells(0, CellRef::new(9, 20), &big).unwrap();
    });
    let r = |r0, c0, r1, c1| Range {
        start: CellRef::new(r0, c0),
        end: CellRef::new(r1, c1),
    };
    time("clear 100,000 cells", &mut || {
        wb.clear_range(0, r(9, 20, 10_008, 29)).unwrap();
    });
    time("enter in 30,000 cells", &mut || {
        wb.enter_in_range(0, r(9, 20, 3008, 29), CellRef::new(9, 20), "=A10*2")
            .unwrap();
    });
    time("sort 3,000 rows", &mut || {
        wb.sort_range(0, r(9, 0, 3008, 11), 1, false, false)
            .unwrap();
    });
    time("insert a row at the top", &mut || {
        wb.insert_rows(0, 10, 1).unwrap();
    });
    time("fill 3,000 rows down", &mut || {
        wb.fill(0, r(9, 40, 9, 40), r(9, 40, 3009, 40), true, &[])
            .unwrap();
    });
    time("undo", &mut || {
        assert!(wb.undo());
    });
    time("save", &mut || {
        wb.save().unwrap();
    });
}

#[test]
#[ignore = "a measurement"]
fn a_million_cells() {
    let mut wb = book();
    // 100,000 rows of ten cells, the last a formula of the row.
    let rows: Vec<Vec<String>> = (0..100_000)
        .map(|r| {
            (0..10)
                .map(|c| match c {
                    9 => format!("=SUM(A{}:I{})", r + 20, r + 20),
                    _ => format!("{}", (r * 7 + c * 13) % 1000),
                })
                .collect()
        })
        .collect();
    let t = Instant::now();
    wb.set_cells(0, CellRef::new(19, 0), &rows).unwrap();
    println!("enter 1,000,000 cells: {} ms", t.elapsed().as_millis());
    let t = Instant::now();
    let bytes = wb.save().unwrap();
    println!(
        "save: {} ms, {} KB",
        t.elapsed().as_millis(),
        bytes.len() / 1024
    );
    let t = Instant::now();
    let mut wb = Workbook::open(bytes).unwrap();
    let s = wb.sheet(0).unwrap().cells.len();
    println!(
        "open and read the sheet: {} ms ({s} cells)",
        t.elapsed().as_millis()
    );
    let t = Instant::now();
    for r in (0..100_000).step_by(10_000) {
        for c in 0..10 {
            let _ = wb.display(0, CellRef::new(r + 19, c));
        }
    }
    println!("a screenful at ten places: {} ms", t.elapsed().as_millis());
    let t = Instant::now();
    wb.recalculate().unwrap();
    println!("recalculate: {} ms", t.elapsed().as_millis());
    for k in 0..5 {
        let t = Instant::now();
        wb.set_cell(0, CellRef::new(19 + k, 0), "5").unwrap();
        println!("a cell typed: {} ms", t.elapsed().as_millis());
    }
}
