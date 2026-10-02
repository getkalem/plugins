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
//! cargo run -p kalem-plugin-xlsx --example xlsx -- macros book.xlsm
//! cargo run -p kalem-plugin-xlsx --example xlsx -- run book.xlsm Module1.Fill [-o out.xlsm]
//! cargo run -p kalem-plugin-xlsx --example xlsx -- attach-vba book.xlsx Module1.bas out.xlsm
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

/// `.xls`, `.xlsb` and `.ods`: shown, never written.
fn legacy(cmd: &str, path: &Path, arg: Option<&str>) -> Res<()> {
    let wb = kalem_plugin_xlsx::legacy::LegacyWorkbook::open(std::fs::read(path)?)?;
    match cmd {
        "info" => {
            println!("{} ({:?}, shown only)", path.display(), wb.format);
            for (i, s) in wb.sheets.iter().enumerate() {
                let used = s.used_range().map_or("empty".to_owned(), |r| r.to_string());
                println!(
                    "  {i}: {}{}: {used}",
                    s.name,
                    if s.hidden { " (hidden)" } else { "" }
                );
            }
        }
        "show" => {
            let idx = match arg {
                Some(n) => wb
                    .sheets
                    .iter()
                    .position(|s| s.name == n)
                    .ok_or(format!("no sheet {n}"))?,
                None => 0,
            };
            for (r, row) in wb.grid(idx).iter().enumerate() {
                println!("{:>4}| {}", r + 1, row.join(" | "));
            }
        }
        "get" => {
            let at = arg.ok_or("missing cell")?;
            let (sheet, cell) = at
                .rsplit_once('!')
                .map_or((None, at), |(s, c)| (Some(s.trim_matches('\'')), c));
            let idx = sheet
                .map_or(Some(0), |n| wb.sheets.iter().position(|s| s.name == n))
                .ok_or("no such sheet")?;
            let cell = CellRef::parse(cell).ok_or("not a cell")?;
            println!("shown:   {}", wb.display(idx, cell));
            println!("entered: {}", wb.edit_text(idx, cell));
        }
        _ => return Err("this format is shown only: info, show and get".into()),
    }
    Ok(())
}

/// A terminal host: `MsgBox` prints and waits for Enter (or y/n),
/// `InputBox` reads a line.
struct Terminal;

impl kalem_plugin_xlsx::macros::MacroHost for Terminal {
    fn msg_box(&mut self, prompt: &str, buttons: i64, title: &str) -> i64 {
        let yes_no = matches!(buttons & 7, 3 | 4);
        println!("[{title}] {prompt}{}", if yes_no { " (y/n)" } else { "" });
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        if yes_no {
            if line.trim().eq_ignore_ascii_case("y") {
                6
            } else {
                7
            }
        } else {
            1
        }
    }

    fn input_box(&mut self, prompt: &str, title: &str, default: &str) -> Option<String> {
        println!("[{title}] {prompt} [{default}]");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok()?;
        let t = line.trim_end_matches(['\r', '\n']);
        Some(if t.is_empty() {
            default.to_owned()
        } else {
            t.to_owned()
        })
    }

    fn print(&mut self, line: &str) {
        println!("{line}");
    }
}

fn list_macros(path: &Path) -> Res<()> {
    let wb = open(path)?;
    let Some(project) = wb.vba_project()? else {
        println!("no VBA project");
        return Ok(());
    };
    for m in kalem_plugin_xlsx::macros::list_macros(&project)? {
        println!(
            "{}{}",
            m.qualified,
            if m.event {
                "  (event: never run by itself)"
            } else {
                ""
            }
        );
    }
    Ok(())
}

fn run_macro(path: &Path, name: &str, out: Option<PathBuf>) -> Res<()> {
    let mut wb = open(path)?;
    let project = wb.vba_project()?.ok_or("no VBA project")?;
    let result = kalem_plugin_xlsx::macros::run_macro(
        &mut wb,
        &project,
        name,
        &mut Terminal,
        Default::default(),
    );
    let report = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("macro stopped: {e}");
            *e.report
        }
    };
    for s in &report.skipped {
        eprintln!("skipped: {s}");
    }
    if wb.is_dirty() {
        let target = out.unwrap_or_else(|| path.to_path_buf());
        std::fs::write(&target, wb.save()?)?;
        println!(
            "written: {} ({})",
            target.display(),
            wb.changed_parts().join(", ")
        );
    }
    Ok(())
}

/// Puts a module's source into a copy of a workbook as its VBA project: for
/// trying macros without Excel. The result opens in Kalem; Excel wants a
/// fuller project than this writes.
fn attach_vba(path: &Path, module: &Path, out: &Path) -> Res<()> {
    use kalem_plugin_xlsx::package::Package;
    let source = std::fs::read_to_string(module)?
        .replace("\r\n", "\n")
        .replace('\n', "\r\n");
    let name = module
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Module1");
    let bin = kalem_plugin_xlsx::vba::write_project(
        &[(
            name,
            kalem_plugin_xlsx::vba::ModuleKind::Standard,
            source.as_bytes(),
        )],
        1252,
    );
    let mut pkg = Package::read(std::fs::read(path)?)?;
    pkg.set_part("xl/vbaProject.bin", bin);
    let rels = String::from_utf8(pkg.part("xl/_rels/workbook.xml.rels")?)?;
    let rels = rels.replace(
        "</Relationships>",
        "<Relationship Id=\"rIdVba\" Type=\"http://schemas.microsoft.com/office/2006/relationships/vbaProject\" Target=\"vbaProject.bin\"/></Relationships>",
    );
    pkg.set_part("xl/_rels/workbook.xml.rels", rels.into_bytes());
    let ct = String::from_utf8(pkg.part("[Content_Types].xml")?)?;
    let ct = ct
        .replace("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml", "application/vnd.ms-excel.sheet.macroEnabled.main+xml")
        .replace("</Types>", "<Override PartName=\"/xl/vbaProject.bin\" ContentType=\"application/vnd.ms-office.vbaProject\"/></Types>");
    pkg.set_part("[Content_Types].xml", ct.into_bytes());
    std::fs::write(out, pkg.write()?)?;
    println!("written: {}", out.display());
    Ok(())
}

fn run(args: &[String]) -> Res<()> {
    let arg = |i: usize| args.get(i).map(String::as_str);
    let path = PathBuf::from(arg(1).ok_or("missing file")?);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "xls" | "xlsb" | "ods") {
        return legacy(arg(0).unwrap_or(""), &path, arg(2));
    }
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
        Some("macros") => list_macros(&path),
        Some("run") => {
            let out = args.iter().position(|a| a == "-o").and_then(|p| args.get(p + 1)).map(PathBuf::from);
            run_macro(&path, arg(2).ok_or("missing macro name")?, out)
        }
        Some("attach-vba") => attach_vba(&path, Path::new(arg(2).ok_or("missing module file")?), Path::new(arg(3).ok_or("missing output")?)),
        Some(c @ ("insert-rows" | "delete-rows" | "insert-cols" | "delete-cols")) => structural(
            c,
            &path,
            arg(2).ok_or("missing sheet")?,
            arg(3).ok_or("missing row or column")?,
            arg(4).unwrap_or("1"),
        ),
        _ => Err("commands: info, show, get, set, insert-rows, delete-rows, insert-cols, delete-cols, vba, macros, run, attach-vba".into()),
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
