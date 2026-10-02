//! A command line over the plugin's library, for trying it before Kalem's
//! plugin host exists:
//!
//! ```sh
//! cargo run -p kalem-plugin-xlsx --example xlsx -- info book.xlsx
//! cargo run -p kalem-plugin-xlsx --example xlsx -- show book.xlsx [SHEET]
//! cargo run -p kalem-plugin-xlsx --example xlsx -- get book.xlsx 'Sheet1!B2'
//! cargo run -p kalem-plugin-xlsx --example xlsx -- set book.xlsx 'Sheet1!B2' '=SUM(A1:A3)' [-o out.xlsx]
//! cargo run -p kalem-plugin-xlsx --example xlsx -- insert-rows book.xlsx Sheet1 3 2
//! cargo run -p kalem-plugin-xlsx --example xlsx -- delete-cols book.xlsx Sheet1 B 1
//! cargo run -p kalem-plugin-xlsx --example xlsx -- vba book.xlsm
//! ```
//!
//! `set` without `-o` writes the file in place, atomically.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kalem_plugin_xlsx::{CellRef, SheetKind, Visibility, Workbook, vba};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn open(path: &Path) -> Res<Workbook> {
    Ok(Workbook::open(std::fs::read(path)?)?)
}

/// `Sheet!A1`, `'My sheet'!A1` or `A1` on the first sheet.
fn address(wb: &Workbook, s: &str) -> Res<(usize, CellRef)> {
    let (sheet, cell) = match s.rsplit_once('!') {
        Some((sh, c)) => {
            let name = sh.trim_matches('\'').replace("''", "'");
            (wb.sheet_index(&name).ok_or(format!("no sheet {name}"))?, c)
        }
        None => (0, s),
    };
    Ok((
        sheet,
        CellRef::parse(cell).ok_or(format!("not a cell: {cell}"))?,
    ))
}

fn info(path: &Path) -> Res<()> {
    let mut wb = open(path)?;
    println!("{}", path.display());
    for i in 0..wb.sheets().len() {
        let s = wb.sheets()[i].clone();
        let vis = match s.visibility {
            Visibility::Visible => "",
            Visibility::Hidden => " (hidden)",
            Visibility::VeryHidden => " (very hidden)",
        };
        match s.kind {
            SheetKind::Worksheet => {
                let sheet = wb.sheet(i)?;
                let used = sheet
                    .used_range()
                    .map_or("empty".to_owned(), |r| r.to_string());
                let extra = if sheet.has_drawing {
                    ", drawings kept"
                } else {
                    ""
                };
                println!(
                    "  {i}: {}{vis}: {used}, {} cells{extra}",
                    s.name,
                    sheet.cells.len()
                );
            }
            k => println!("  {i}: {}{vis}: {k:?}, kept as it is", s.name),
        }
    }
    for n in wb.defined_names() {
        println!("  name {} = {}", n.name, n.refers_to);
    }
    if wb.has_vba() {
        println!("  VBA project: yes (`vba` lists its modules)");
    }
    println!("  {} parts", wb.parts().len());
    Ok(())
}

fn show(path: &Path, sheet: Option<&str>) -> Res<()> {
    let mut wb = open(path)?;
    let idx = match sheet {
        Some(n) => wb.sheet_index(n).ok_or(format!("no sheet {n}"))?,
        None => 0,
    };
    let grid = wb.grid(idx)?;
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0; width];
    for row in &grid {
        for (c, v) in row.iter().enumerate() {
            widths[c] = widths[c].max(v.chars().count()).min(30);
        }
    }
    print!("    ");
    for (c, w) in widths.iter().enumerate() {
        print!(
            "| {:<w$} ",
            kalem_plugin_xlsx::cellref::column_name(c as u32),
            w = (*w).max(1)
        );
    }
    println!("|");
    for (r, row) in grid.iter().enumerate() {
        print!("{:>4}", r + 1);
        for (c, w) in widths.iter().enumerate() {
            let v: String = row
                .get(c)
                .map_or("", String::as_str)
                .chars()
                .take(30)
                .collect();
            print!("| {v:<w$} ", w = (*w).max(1));
        }
        println!("|");
    }
    for c in wb.comments(idx)? {
        println!("note {}: {} ({})", c.cell, c.text, c.author);
    }
    Ok(())
}

fn get(path: &Path, at: &str) -> Res<()> {
    let mut wb = open(path)?;
    let (idx, cell) = address(&wb, at)?;
    println!("shown:   {}", wb.display(idx, cell)?);
    println!("entered: {}", wb.edit_text(idx, cell)?);
    Ok(())
}

fn set(path: &Path, at: &str, entry: &str, out: Option<PathBuf>) -> Res<()> {
    let mut wb = open(path)?;
    let (idx, cell) = address(&wb, at)?;
    wb.set_cell(idx, cell, entry)?;
    let bytes = wb.save()?;
    let target = out.unwrap_or_else(|| path.to_path_buf());
    let tmp = target.with_extension("kalem-tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &target)?;
    println!("{} = {}", at, wb.display(idx, cell)?);
    println!("parts written: {}", wb.changed_parts().join(", "));
    Ok(())
}

/// `insert-rows FILE SHEET AT N`: rows are numbered from 1, columns by letter.
fn structural(cmd: &str, path: &Path, sheet: &str, at: &str, n: &str) -> Res<()> {
    let mut wb = open(path)?;
    let idx = wb.sheet_index(sheet).ok_or(format!("no sheet {sheet}"))?;
    let n: u32 = n.parse()?;
    let at = match at.parse::<u32>() {
        Ok(r) => r.checked_sub(1).ok_or("rows start at 1")?,
        Err(_) => {
            kalem_plugin_xlsx::cellref::column_index(at).ok_or(format!("not a column: {at}"))?
        }
    };
    match cmd {
        "insert-rows" => wb.insert_rows(idx, at, n)?,
        "delete-rows" => wb.delete_rows(idx, at, n)?,
        "insert-cols" => wb.insert_cols(idx, at, n)?,
        _ => wb.delete_cols(idx, at, n)?,
    }
    std::fs::write(path, wb.save()?)?;
    println!("parts written: {}", wb.changed_parts().join(", "));
    Ok(())
}

fn show_vba(path: &Path) -> Res<()> {
    let wb = open(path)?;
    let Some(bin) = wb.vba_project_bytes()? else {
        println!("no VBA project");
        return Ok(());
    };
    let project = vba::read_project(&bin)?;
    println!("project {}", project.name);
    for m in project.modules {
        println!("--- {} ({:?})", m.name, m.kind);
        println!("{}", m.source.replace("\r\n", "\n"));
    }
    Ok(())
}

fn run(args: &[String]) -> Res<()> {
    let arg = |i: usize| args.get(i).map(String::as_str);
    let path = PathBuf::from(arg(1).ok_or("missing file")?);
    match arg(0) {
        Some("info") => info(&path),
        Some("show") => show(&path, arg(2)),
        Some("get") => get(&path, arg(2).ok_or("missing cell")?),
        Some("set") => {
            let out = args
                .iter()
                .position(|a| a == "-o")
                .and_then(|p| args.get(p + 1))
                .map(PathBuf::from);
            set(
                &path,
                arg(2).ok_or("missing cell")?,
                arg(3).ok_or("missing entry")?,
                out,
            )
        }
        Some("vba") => show_vba(&path),
        Some(c @ ("insert-rows" | "delete-rows" | "insert-cols" | "delete-cols")) => structural(
            c,
            &path,
            arg(2).ok_or("missing sheet")?,
            arg(3).ok_or("missing row or column")?,
            arg(4).unwrap_or("1"),
        ),
        _ => Err("commands: info, show, get, set, insert-rows, delete-rows, insert-cols, delete-cols, vba".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xlsx: {e}");
            ExitCode::FAILURE
        }
    }
}
