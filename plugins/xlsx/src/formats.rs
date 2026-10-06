//! Files of other formats (the plugin API's `formats` interface, 0.2.3):
//! a new workbook made from sheets of entries as typed, a workbook
//! declared as another of its kinds (`.xlsm` as `.xlsx`, a template as a
//! workbook), and a workbook written as an OpenDocument spreadsheet.
//! Kalem held this code until the interface let the plugin write its own
//! formats.

use std::collections::BTreeMap;

use kalem_viewer::{GridCell, NewSheet, ViewerDocument};

use crate::package::Package;

/// The workbook kinds: their extensions.
pub const KINDS: [&str; 4] = ["xlsx", "xlsm", "xltx", "xltm"];

/// A new workbook of `extension` holding `sheets`, every cell written at
/// once: entries read as Excel reads them typed.
pub fn new_workbook(extension: &str, sheets: &[NewSheet]) -> Result<Vec<u8>, String> {
    let Some(main) = main_content_type(extension) else {
        return Err(format!("A workbook is not a .{extension} file"));
    };
    let sheets: Vec<SheetEntries> = sheets
        .iter()
        .map(|s| {
            let mut cells = BTreeMap::new();
            for (r, row) in s.rows.iter().enumerate() {
                for (c, v) in row.iter().enumerate() {
                    if !v.is_empty() {
                        cells.insert((r as u32, c as u32), v.clone());
                    }
                }
            }
            (s.name.clone(), cells)
        })
        .collect();
    Ok(build_xlsx(&sheets, main))
}

/// The main part's content type of a workbook saved as `extension`.
fn main_content_type(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
        "xlsm" => "application/vnd.ms-excel.sheet.macroEnabled.main+xml",
        "xltx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.template.main+xml",
        "xltm" => "application/vnd.ms-excel.template.macroEnabled.main+xml",
        _ => return None,
    })
}

/// The workbook `bytes` as a file of `extension` declares itself: the
/// workbook part's content type of a workbook, a macro-enabled workbook,
/// a template or a macro-enabled template, and, where macros are not
/// allowed, without its VBA project (Excel refuses an `.xlsx` that keeps
/// one). Every other part is copied byte for byte; the bytes are as they
/// were when nothing changes.
pub fn retype(bytes: Vec<u8>, extension: &str) -> Result<Vec<u8>, String> {
    let Some(target) = main_content_type(extension) else {
        return Ok(bytes);
    };
    let mut p = Package::read(bytes).map_err(|e| e.to_string())?;
    let macros = matches!(extension, "xlsm" | "xltm");
    let is_vba = |name: &str| {
        let n = name.trim_start_matches('/');
        n.starts_with("xl/vbaProject") && n.ends_with(".bin")
    };
    if !macros {
        for name in p.names().into_iter().filter(|n| is_vba(n)) {
            p.remove_part(&name);
        }
    }
    let text = |p: &Package, name: &str| {
        p.part(name)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    if let Some(t) = text(&p, "[Content_Types].xml") {
        let mut new = t.clone();
        for ext in KINDS {
            let from = main_content_type(ext).unwrap_or_default();
            if from != target {
                new = new.replace(from, target);
            }
        }
        if !macros {
            // The VBA project's own overrides, and its extension's default.
            new = drop_elements(&new, "Override", |e| e.contains("/xl/vbaProject"));
            new = drop_elements(&new, "Default", |e| {
                e.contains("application/vnd.ms-office.vbaProject")
            });
        }
        if new != t {
            p.set_part("[Content_Types].xml", new.into_bytes());
        }
    }
    if !macros && let Some(t) = text(&p, "xl/_rels/workbook.xml.rels") {
        let new = drop_elements(&t, "Relationship", |e| {
            e.contains("/relationships/vbaProject")
        });
        if new != t {
            p.set_part("xl/_rels/workbook.xml.rels", new.into_bytes());
        }
    }
    p.write().map_err(|e| e.to_string())
}

/// `xml` without the empty elements `<NAME …/>` for which `drop` is true.
fn drop_elements(xml: &str, name: &str, drop: impl Fn(&str) -> bool) -> String {
    let open = format!("<{name} ");
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let Some(len) = rest[i..].find("/>").map(|k| k + 2) else {
            break;
        };
        out.push_str(&rest[..i]);
        let element = &rest[i..i + len];
        if !drop(element) {
            out.push_str(element);
        }
        rest = &rest[i + len..];
    }
    out.push_str(rest);
    out
}

/// A zip file of `entries` (name, bytes, deflated), as Office files are.
fn zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, deflate) in entries {
        let crc = crc32fast::hash(data);
        let body = if *deflate {
            miniz_oxide::deflate::compress_to_vec(data, 6)
        } else {
            data.to_vec()
        };
        let method: u16 = if *deflate { 8 } else { 0 };
        let offset = out.len() as u32;
        let header = |sig: u32, central: bool| -> Vec<u8> {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend(method.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend(0x21u16.to_le_bytes());
            h.extend(crc.to_le_bytes());
            h.extend((body.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central {
                // Comment length, disk, internal and external attributes.
                h.extend([0u8; 6]);
                h.extend(0u32.to_le_bytes());
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(&body);
        central.extend(header(0x0201_4b50, true));
    }
    let at = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A sheet's name and its entries as typed, by row and column.
type SheetEntries = (String, BTreeMap<(u32, u32), String>);

/// An entry as a cell holds it.
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Number(f64),
    Text(String),
    Bool(bool),
    /// A number shown with a built-in format: a serial date or time (14,
    /// 22, 21), thousands (3, 4) or a percentage (9, 10).
    Date(f64, u32),
    Formula(String),
}

/// An entry as typed (as a cell's entry reads back: plain numbers, ISO
/// dates and times, `=` formulas, `TRUE`, `'` before text that would
/// read otherwise).
fn entry(input: &str) -> Option<Entry> {
    if input.is_empty() {
        return None;
    }
    if let Some(t) = input.strip_prefix('\'') {
        return Some(Entry::Text(t.to_owned()));
    }
    if let Some(f) = input.strip_prefix('=') {
        return Some(Entry::Formula(f.to_owned()));
    }
    let t = input.trim();
    if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false") {
        return Some(Entry::Bool(t.eq_ignore_ascii_case("true")));
    }
    if let Ok(n) = t.parse::<f64>()
        && n.is_finite()
    {
        return Some(Entry::Number(n));
    }
    // `1,234.5` and `12.5%`, as a sheet reads them typed.
    if let Some(p) = t.strip_suffix('%')
        && let Some(n) = grouped_number(p.trim())
    {
        return Some(Entry::Date(n / 100.0, if p.contains('.') { 10 } else { 9 }));
    }
    if t.contains(',')
        && let Some(n) = grouped_number(t)
    {
        return Some(Entry::Date(n, if t.contains('.') { 4 } else { 3 }));
    }
    // A date that is none (`2026-02-30`) stays text.
    if let Some((kind, v)) = date_value(t)
        && let Some((n, f)) = serial(kind, &v)
    {
        return Some(Entry::Date(n, f));
    }
    Some(Entry::Text(input.to_owned()))
}

/// A number written with a point for decimals and commas between groups
/// of three digits (`-1,234.5`).
fn grouped_number(t: &str) -> Option<f64> {
    let body = t.strip_prefix('-').unwrap_or(t);
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let groups: Vec<&str> = int.split(',').collect();
    let digits = |g: &str| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit());
    let ok = digits(groups[0])
        && (groups.len() == 1 || groups[0].len() <= 3)
        && groups[1..].iter().all(|g| g.len() == 3 && digits(g))
        && (frac.is_empty() || digits(frac));
    if !ok {
        return None;
    }
    let n: f64 = format!(
        "{}.{}",
        groups.concat(),
        if frac.is_empty() { "0" } else { frac }
    )
    .parse()
    .ok()?;
    Some(if t.starts_with('-') { -n } else { n })
}

/// An OpenDocument date or time value as a serial number and the format
/// that shows it.
fn serial(kind: &str, v: &str) -> Option<(f64, u32)> {
    let time = |t: &str| -> Option<f64> {
        let p: Vec<f64> = t
            .split(':')
            .map(|x| x.parse().ok())
            .collect::<Option<_>>()?;
        Some((p[0] * 3600.0 + p.get(1).unwrap_or(&0.0) * 60.0 + p.get(2).unwrap_or(&0.0)) / 86400.0)
    };
    if kind == "time" {
        // `PT10H30M00S`.
        let t = v
            .trim_start_matches("PT")
            .replace(['H', 'M'], ":")
            .replace('S', "");
        return Some((time(&t)?, 21));
    }
    let (d, t) = match v.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (v, None),
    };
    let p: Vec<i64> = d
        .split('-')
        .map(|x| x.parse().ok())
        .collect::<Option<_>>()?;
    let (y, m, day) = (p[0], u32::try_from(p[1]).ok()?, u32::try_from(p[2]).ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || day == 0 || day > month_days[m as usize - 1] {
        return None;
    }
    let days = (crate::numfmt::days_from_civil(y, m, day)
        - crate::numfmt::days_from_civil(1899, 12, 30)) as f64;
    match t {
        Some(t) => Some((days + time(t)?, 22)),
        None => Some((days, 14)),
    }
}

/// An Excel workbook of sheets (name, entries by row and column as typed),
/// written straight, every cell at once.
fn build_xlsx(sheets: &[SheetEntries], main_type: &str) -> Vec<u8> {
    let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType=""#,
    );
    types.push_str(main_type);
    types.push_str(
        r#""/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
    );
    let mut list = String::new();
    let mut rels = String::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, (name, cells)) in sheets.iter().enumerate() {
        let k = i + 1;
        types.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{k}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
        list.push_str(&format!(
            r#"<sheet name="{}" sheetId="{k}" r:id="rId{k}"/>"#,
            esc(name)
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{k}" Type="{rel}/worksheet" Target="worksheets/sheet{k}.xml"/>"#
        ));
        let mut data = String::new();
        let mut row: Option<u32> = None;
        for (&(r, c), input) in cells {
            let Some(e) = entry(input) else { continue };
            if row != Some(r) {
                if row.is_some() {
                    data.push_str("</row>");
                }
                data.push_str(&format!(r#"<row r="{}">"#, r + 1));
                row = Some(r);
            }
            let at = format!("{}{}", crate::cellref::column_name(c), r + 1);
            data.push_str(&match e {
                Entry::Number(n) => format!(r#"<c r="{at}"><v>{n}</v></c>"#),
                Entry::Bool(b) => format!(r#"<c r="{at}" t="b"><v>{}</v></c>"#, u8::from(b)),
                Entry::Date(n, f) => {
                    let s = match f {
                        14 => 1,
                        22 => 2,
                        21 => 3,
                        3 => 4,
                        4 => 5,
                        9 => 6,
                        _ => 7,
                    };
                    format!(r#"<c r="{at}" s="{s}"><v>{n}</v></c>"#)
                }
                Entry::Formula(f) => format!(
                    r#"<c r="{at}"><f>{}</f></c>"#,
                    esc(&crate::formula::with_future_prefixes(&f))
                ),
                Entry::Text(t) => format!(
                    r#"<c r="{at}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                    esc(&t)
                ),
            });
        }
        if row.is_some() {
            data.push_str("</row>");
        }
        parts.push((
            format!("xl/worksheets/sheet{k}.xml"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{main}" xmlns:r="{rel}"><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><sheetData>{data}</sheetData></worksheet>"#
            ),
        ));
    }
    let n = sheets.len() + 1;
    rels.push_str(&format!(
        r#"<Relationship Id="rId{n}" Type="{rel}/styles" Target="styles.xml"/>"#
    ));
    types.push_str("</Types>");
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{main}" xmlns:r="{rel}"><bookViews><workbookView/></bookViews><sheets>{list}</sheets><calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>"#
    );
    // The formats dates and times show with (Excel's built-in 14, 22, 21),
    // thousands (3, 4) and percentages (9, 10).
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="{main}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="8"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="22" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="21" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="3" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="4" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="9" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="10" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
    );
    let root = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{rel}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let wb_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    );
    let mut entries: Vec<(&str, &[u8], bool)> = vec![
        ("[Content_Types].xml", types.as_bytes(), true),
        ("_rels/.rels", root.as_bytes(), true),
        ("xl/workbook.xml", workbook.as_bytes(), true),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes(), true),
        ("xl/styles.xml", styles.as_bytes(), true),
    ];
    for (name, text) in &parts {
        entries.push((name.as_str(), text.as_bytes(), true));
    }
    zip(&entries)
}

/// A sheet of a grid document as read: its name, and each cell's entry
/// as typed and how it looks, by row and column.
struct SheetData {
    name: String,
    hidden: bool,
    cells: BTreeMap<(u32, u32), (String, GridCell, Option<String>)>,
    layout: kalem_viewer::GridLayout,
}

/// The grid sheets of `doc`.
fn sheets(doc: &mut dyn ViewerDocument) -> Result<Vec<(usize, SheetData)>, String> {
    let units = doc.structure().units;
    let mut out = Vec::new();
    for (u, unit) in units.iter().enumerate() {
        let Some(layout) = doc.grid(u) else { continue };
        let (rows, cols) = (layout.rows, layout.cols);
        if u64::from(rows) * u64::from(cols) > 5_000_000 {
            return Err(format!("{} is too large to convert", unit.label));
        }
        let mut cells = BTreeMap::new();
        // A band of rows at a time.
        let mut r = 0;
        while r < rows {
            let to = (r + 500).min(rows);
            for (row, col, cell) in doc.grid_cells(u, r..to, 0..cols) {
                let input = doc.cell_input(u, row, col);
                if input.is_empty() && cell == GridCell::default() {
                    continue;
                }
                let fmt = doc.cell_format(u, row, col).filter(|f| f != "General");
                cells.insert((row, col), (input, cell, fmt));
            }
            r = to;
        }
        let hidden = unit.label.ends_with(" (hidden)");
        let name = unit.label.trim_end_matches(" (hidden)").to_owned();
        out.push((
            u,
            SheetData {
                name,
                hidden,
                cells,
                layout,
            },
        ));
    }
    if out.is_empty() {
        return Err("No sheet to convert".into());
    }
    Ok(out)
}

/// An Excel formula (without `=`) in OpenFormula: references in brackets
/// (`[.A1:.B2]`, `['Other sheet'.A1]`), arguments apart by `;`, array
/// rows by `|`.
pub fn open_formula(f: &str) -> String {
    let chars: Vec<char> = f.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_array = false;
    let cell_at = |s: &[char], mut j: usize| -> Option<usize> {
        // `$A$1`, `A1`, `A:A`'s halves, `1:1`'s halves: a cell here.
        let start = j;
        if j < s.len() && s[j] == '$' {
            j += 1;
        }
        let letters = j;
        while j < s.len() && s[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j == letters || j - letters > 3 {
            return None;
        }
        if j < s.len() && s[j] == '$' {
            j += 1;
        }
        let digits = j;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        (j > digits && j > start).then_some(j)
    };
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '"' => {
                // A string, as it is.
                out.push(ch);
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    if chars[i] == '"' {
                        if chars.get(i + 1) == Some(&'"') {
                            out.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            '{' => in_array = true,
            '}' => in_array = false,
            ',' => {
                out.push(';');
                i += 1;
                continue;
            }
            ';' if in_array => {
                out.push('|');
                i += 1;
                continue;
            }
            _ => {}
        }
        let boundary = i == 0
            || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '.');
        if boundary && (ch.is_ascii_alphabetic() || ch == '$' || ch == '\'' || ch == '_') {
            // A sheet's name, then `!`.
            let mut j = i;
            let mut sheet: Option<String> = None;
            if ch == '\'' {
                let mut k = i + 1;
                let mut name = String::new();
                while k < chars.len() {
                    if chars[k] == '\'' {
                        if chars.get(k + 1) == Some(&'\'') {
                            name.push('\'');
                            k += 2;
                            continue;
                        }
                        break;
                    }
                    name.push(chars[k]);
                    k += 1;
                }
                if chars.get(k + 1) == Some(&'!') {
                    sheet = Some(format!("'{}'", name.replace('\'', "''")));
                    j = k + 2;
                }
            } else {
                let mut k = i;
                while k < chars.len()
                    && (chars[k].is_alphanumeric() || chars[k] == '_' || chars[k] == '.')
                {
                    k += 1;
                }
                if chars.get(k) == Some(&'!') {
                    sheet = Some(chars[i..k].iter().collect());
                    j = k + 1;
                }
            }
            // Whole columns: `A:C`.
            let col_end = |mut k: usize| -> Option<usize> {
                let st = k;
                if chars.get(k) == Some(&'$') {
                    k += 1;
                }
                let l = k;
                while k < chars.len() && chars[k].is_ascii_alphabetic() {
                    k += 1;
                }
                (k > l && k - l <= 3 && k > st).then_some(k)
            };
            if let Some(e1) = col_end(j)
                && chars.get(e1) == Some(&':')
                && let Some(e2) = col_end(e1 + 1)
                && !chars
                    .get(e2)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '(')
            {
                let a: String = chars[j..e1].iter().collect();
                let b: String = chars[e1 + 1..e2].iter().collect();
                out.push_str(&format!("[{}.{a}:.{b}]", sheet.clone().unwrap_or_default()));
                i = e2;
                continue;
            }
            if let Some(end) = cell_at(&chars, j)
                && chars.get(end) != Some(&'(')
                && !chars
                    .get(end)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                let first: String = chars[j..end].iter().collect();
                let (second, next) = match chars.get(end) {
                    Some(':') => match cell_at(&chars, end + 1) {
                        Some(e2) => (Some(chars[end + 1..e2].iter().collect::<String>()), e2),
                        None => (None, end),
                    },
                    _ => (None, end),
                };
                let sh = sheet.unwrap_or_default();
                out.push('[');
                out.push_str(&sh);
                out.push('.');
                out.push_str(&first);
                if let Some(s2) = second {
                    out.push_str(":.");
                    out.push_str(&s2);
                }
                out.push(']');
                i = next;
                continue;
            }
            // A function or a name: as it is, Excel's future prefixes out.
            let mut k = i;
            while k < chars.len()
                && (chars[k].is_alphanumeric() || chars[k] == '_' || chars[k] == '.')
            {
                k += 1;
            }
            let word: String = chars[i..k.max(i + 1)].iter().collect();
            let word = word
                .strip_prefix("_xlfn._xlws.")
                .or_else(|| word.strip_prefix("_xlfn."))
                .or_else(|| word.strip_prefix("_xlws."))
                .unwrap_or(&word)
                .to_owned();
            out.push_str(&word);
            i = k.max(i + 1);
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// A width in characters as centimeters.
fn cm(chars: f32) -> f32 {
    (chars * 7.0 + 5.0) / 96.0 * 2.54
}

/// An ISO date or date and time (`2026-10-04`, `2026-10-04 10:30:00`) as
/// an OpenDocument date value; a time (`10:30:00`) as a duration.
fn date_value(s: &str) -> Option<(&'static str, String)> {
    let t = s.trim();
    let date_ok = |d: &str| {
        let p: Vec<&str> = d.split('-').collect();
        p.len() == 3
            && p[0].len() == 4
            && p.iter()
                .all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()))
    };
    let time_ok = |x: &str| {
        let p: Vec<&str> = x.split(':').collect();
        (2..=3).contains(&p.len())
            && p.iter()
                .all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit() || c == '.'))
    };
    if let Some((d, tm)) = t.split_once(' ')
        && date_ok(d)
        && time_ok(tm)
    {
        let tm = if tm.matches(':').count() == 1 {
            format!("{tm}:00")
        } else {
            tm.to_owned()
        };
        return Some(("date", format!("{d}T{tm}")));
    }
    if date_ok(t) {
        return Some(("date", t.to_owned()));
    }
    if time_ok(t) {
        let p: Vec<&str> = t.split(':').collect();
        return Some((
            "time",
            format!("PT{}H{}M{}S", p[0], p[1], p.get(2).unwrap_or(&"0")),
        ));
    }
    None
}

/// `src` written as an OpenDocument spreadsheet.
pub fn to_ods(src: &mut dyn ViewerDocument) -> Result<Vec<u8>, String> {
    let data = sheets(src)?;
    let mut styles: Vec<String> = Vec::new();
    let mut style_names: BTreeMap<String, String> = BTreeMap::new();
    let mut cell_style = |cell: &GridCell, date: Option<&str>| -> Option<String> {
        let mut text = String::new();
        let mut props = String::new();
        if cell.bold {
            text.push_str(r#" fo:font-weight="bold""#);
        }
        if cell.italic {
            text.push_str(r#" fo:font-style="italic""#);
        }
        if cell.underline {
            text.push_str(r#" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color""#);
        }
        if let Some([r, g, b]) = cell.color {
            text.push_str(&format!(r##" fo:color="#{r:02x}{g:02x}{b:02x}""##));
        }
        if let Some([r, g, b]) = cell.fill {
            props.push_str(&format!(
                r##" fo:background-color="#{r:02x}{g:02x}{b:02x}""##
            ));
        }
        let align = match cell.align {
            kalem_viewer::Align::Left => Some("start"),
            kalem_viewer::Align::Center => Some("center"),
            kalem_viewer::Align::Right => Some("end"),
            kalem_viewer::Align::General => None,
        };
        let data_style = match date {
            Some("date") => r#" style:data-style-name="Ndate""#,
            Some("time") => r#" style:data-style-name="Ntime""#,
            _ => "",
        };
        if text.is_empty() && props.is_empty() && align.is_none() && data_style.is_empty() {
            return None;
        }
        let mut body = String::new();
        if !props.is_empty() {
            body.push_str(&format!("<style:table-cell-properties{props}/>"));
        }
        if let Some(a) = align {
            body.push_str(&format!(
                r#"<style:paragraph-properties fo:text-align="{a}"/>"#
            ));
        }
        if !text.is_empty() {
            body.push_str(&format!("<style:text-properties{text}/>"));
        }
        let key = format!("{data_style}|{body}");
        if let Some(n) = style_names.get(&key) {
            return Some(n.clone());
        }
        let name = format!("ce{}", style_names.len() + 1);
        styles.push(format!(
            r#"<style:style style:name="{name}" style:family="table-cell" style:parent-style-name="Default"{data_style}>{body}</style:style>"#
        ));
        style_names.insert(key, name.clone());
        Some(name)
    };
    let mut col_styles: BTreeMap<String, String> = BTreeMap::new();
    let mut tables = String::new();
    for (unit, s) in &data {
        // Formulas' results, computed together.
        let formulas: Vec<((u32, u32), String)> = s
            .cells
            .iter()
            .filter(|(_, (input, _, _))| input.starts_with('='))
            .map(|(k, (input, _, _))| (*k, input.clone()))
            .collect();
        let texts: Vec<String> = formulas.iter().map(|(_, f)| f.clone()).collect();
        let results: BTreeMap<(u32, u32), Option<String>> = formulas
            .iter()
            .map(|(k, _)| *k)
            .zip(src.evaluate_formulas(*unit, &texts))
            .collect();
        let max_c = s.cells.keys().map(|k| k.1).max().unwrap_or(0);
        let max_r = s.cells.keys().map(|k| k.0).max().unwrap_or(0);
        let mut t = format!(
            r#"<table:table table:name="{}"{}>"#,
            esc(&s.name),
            if s.hidden {
                r#" table:display="false""#
            } else {
                ""
            }
        );
        for c in 0..=max_c {
            let w = s
                .layout
                .widths
                .get(c as usize)
                .copied()
                .unwrap_or(s.layout.default_width);
            let key = format!("{:.3}cm", cm(w));
            let n = col_styles.len() + 1;
            let name = col_styles
                .entry(key)
                .or_insert_with(|| format!("co{n}"))
                .clone();
            t.push_str(&format!(
                r#"<table:table-column table:style-name="{name}"/>"#
            ));
        }
        let covered = |r: u32, c: u32| {
            s.layout.merged.iter().any(|m| {
                (m[0]..=m[2]).contains(&r) && (m[1]..=m[3]).contains(&c) && (m[0], m[1]) != (r, c)
            })
        };
        for r in 0..=max_r {
            t.push_str("<table:table-row>");
            let mut blank = 0;
            for c in 0..=max_c {
                if covered(r, c) {
                    if blank > 0 {
                        t.push_str(&format!(
                            r#"<table:table-cell table:number-columns-repeated="{blank}"/>"#
                        ));
                        blank = 0;
                    }
                    t.push_str("<table:covered-table-cell/>");
                    continue;
                }
                let Some((input, cell, _)) = s.cells.get(&(r, c)) else {
                    blank += 1;
                    continue;
                };
                if blank > 0 {
                    t.push_str(&format!(
                        r#"<table:table-cell table:number-columns-repeated="{blank}"/>"#
                    ));
                    blank = 0;
                }
                let mut attrs = String::new();
                if let Some(m) = s.layout.merged.iter().find(|m| (m[0], m[1]) == (r, c)) {
                    attrs.push_str(&format!(
                        r#" table:number-columns-spanned="{}" table:number-rows-spanned="{}""#,
                        m[3] - m[1] + 1,
                        m[2] - m[0] + 1
                    ));
                }
                let value = |v: &str| -> String {
                    let v = v.trim();
                    if let Ok(n) = v.parse::<f64>() {
                        format!(r#" office:value-type="float" office:value="{n}""#)
                    } else if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
                        format!(
                            r#" office:value-type="boolean" office:boolean-value="{}""#,
                            v.eq_ignore_ascii_case("true")
                        )
                    } else {
                        r#" office:value-type="string""#.to_owned()
                    }
                };
                let mut kind = None;
                if let Some(f) = input.strip_prefix('=') {
                    attrs.push_str(&format!(
                        r#" table:formula="of:={}""#,
                        esc(&open_formula(f))
                    ));
                    match results.get(&(r, c)).cloned().flatten() {
                        Some(v) if v.starts_with('"') => {
                            attrs.push_str(r#" office:value-type="string""#)
                        }
                        Some(v) => attrs.push_str(&value(&v)),
                        None => {}
                    }
                } else if cell.numeric
                    && let Some((k, v)) = date_value(input)
                {
                    kind = Some(k);
                    if k == "date" {
                        attrs.push_str(&format!(
                            r#" office:value-type="date" office:date-value="{v}""#
                        ));
                    } else {
                        attrs.push_str(&format!(
                            r#" office:value-type="time" office:time-value="{v}""#
                        ));
                    }
                } else if cell.numeric || !input.starts_with('\'') && input.parse::<f64>().is_ok() {
                    attrs.push_str(&value(input));
                } else {
                    attrs.push_str(r#" office:value-type="string""#);
                }
                if let Some(st) = cell_style(cell, kind) {
                    attrs.push_str(&format!(r#" table:style-name="{st}""#));
                }
                let shown = if cell.text.is_empty() && !input.starts_with('=') {
                    input.trim_start_matches('\'').to_owned()
                } else {
                    cell.text.clone()
                };
                t.push_str(&format!(
                    "<table:table-cell{attrs}><text:p>{}</text:p></table:table-cell>",
                    esc(&shown)
                ));
            }
            t.push_str("</table:table-row>");
        }
        t.push_str("</table:table>");
        tables.push_str(&t);
    }
    let cols: String = col_styles
        .iter()
        .map(|(w, n)| {
            format!(
                r#"<style:style style:name="{n}" style:family="table-column"><style:table-column-properties style:column-width="{w}"/></style:style>"#
            )
        })
        .collect();
    let ns = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2""#;
    let data_styles = r#"<number:date-style style:name="Ndate"><number:year number:style="long"/><number:text>-</number:text><number:month number:style="long"/><number:text>-</number:text><number:day number:style="long"/></number:date-style><number:time-style style:name="Ntime"><number:hours number:style="long"/><number:text>:</number:text><number:minutes number:style="long"/><number:text>:</number:text><number:seconds number:style="long"/></number:time-style>"#;
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {ns} office:version="1.2"><office:automatic-styles>{data_styles}{cols}{}</office:automatic-styles><office:body><office:spreadsheet>{tables}</office:spreadsheet></office:body></office:document-content>"#,
        styles.concat()
    );
    let styles_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {ns} office:version="1.2"><office:styles><style:style style:name="Default" style:family="table-cell"/></office:styles></office:document-styles>"#
    );
    let meta = r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" office:version="1.2"><office:meta><meta:generator>Kalem</meta:generator></office:meta></office:document-meta>"#;
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2"><manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    Ok(zip(&[
        (
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
            false,
        ),
        ("content.xml", content.as_bytes(), true),
        ("styles.xml", styles_xml.as_bytes(), true),
        ("meta.xml", meta.as_bytes(), true),
        ("META-INF/manifest.xml", manifest.as_bytes(), true),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip file's entries, through the plugin's own reader.
    fn parts(bytes: &[u8]) -> Package {
        Package::read(bytes.to_vec()).unwrap()
    }

    fn text(p: &Package, name: &str) -> Option<String> {
        p.part(name).ok().map(|b| String::from_utf8(b).unwrap())
    }

    #[test]
    fn entries_read_as_typed() {
        assert_eq!(entry("1,234.5"), Some(Entry::Date(1234.5, 4)));
        assert_eq!(entry("12.5%"), Some(Entry::Date(0.125, 10)));
        assert_eq!(entry("50%"), Some(Entry::Date(0.5, 9)));
        assert_eq!(entry("1,23"), Some(Entry::Text("1,23".into())));
        assert_eq!(entry("2026-10-04"), Some(Entry::Date(46299.0, 14)));
        assert_eq!(entry("2026-02-30"), Some(Entry::Text("2026-02-30".into())));
        assert_eq!(entry("'007"), Some(Entry::Text("007".into())));
        assert_eq!(entry("=A1"), Some(Entry::Formula("A1".into())));
        assert_eq!(entry("1.5"), Some(Entry::Number(1.5)));
        assert_eq!(
            date_value("2026-10-04 10:30"),
            Some(("date", "2026-10-04T10:30:00".into()))
        );
        assert_eq!(date_value("10:30:00"), Some(("time", "PT10H30M00S".into())));
        assert_eq!(date_value("1200"), None);
    }

    #[test]
    fn a_new_workbook_of_each_kind() {
        let sheets = [NewSheet {
            name: "Data & more".into(),
            rows: vec![
                vec!["Item".into(), "Total".into()],
                vec!["a".into(), "=TEXTJOIN(\",\",TRUE,A1:A2)".into()],
            ],
        }];
        let book = new_workbook("xlsx", &sheets).unwrap();
        let p = parts(&book);
        assert!(
            text(&p, "[Content_Types].xml")
                .unwrap()
                .contains("spreadsheetml.sheet.main+xml")
        );
        assert!(
            text(&p, "xl/workbook.xml")
                .unwrap()
                .contains(r#"name="Data &amp; more""#)
        );
        let sheet = text(&p, "xl/worksheets/sheet1.xml").unwrap();
        assert!(sheet.contains("_xlfn.TEXTJOIN("), "{sheet}");
        let tpl = new_workbook("xltx", &sheets).unwrap();
        assert!(
            text(&parts(&tpl), "[Content_Types].xml")
                .unwrap()
                .contains("template.main+xml")
        );
        assert!(new_workbook("csv", &sheets).is_err());
    }

    #[test]
    fn a_workbook_saved_as_another_kind_says_so() {
        // A macro-enabled workbook: its content type, its VBA project.
        let book = new_workbook(
            "xlsm",
            &[NewSheet {
                name: "S".into(),
                rows: Vec::new(),
            }],
        )
        .unwrap();
        let mut p = parts(&book);
        let types = text(&p, "[Content_Types].xml").unwrap().replace(
            "</Types>",
            r#"<Default Extension="bin" ContentType="application/vnd.ms-office.vbaProject"/></Types>"#,
        );
        p.set_part("[Content_Types].xml", types.into_bytes());
        let rels = text(&p, "xl/_rels/workbook.xml.rels").unwrap().replace(
            "</Relationships>",
            r#"<Relationship Id="rIdV" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/></Relationships>"#,
        );
        p.set_part("xl/_rels/workbook.xml.rels", rels.into_bytes());
        p.set_part("xl/vbaProject.bin", b"VBA".to_vec());
        let xlsm = p.write().unwrap();
        // Kept as it is in its own kind.
        assert_eq!(retype(xlsm.clone(), "xlsm").unwrap(), xlsm);
        let xlsx = parts(&retype(xlsm, "xlsx").unwrap());
        let types = text(&xlsx, "[Content_Types].xml").unwrap();
        assert!(types.contains("spreadsheetml.sheet.main+xml"), "{types}");
        assert!(
            !types.contains("macroEnabled") && !types.contains("vbaProject"),
            "{types}"
        );
        assert!(text(&xlsx, "xl/vbaProject.bin").is_none());
        assert!(
            !text(&xlsx, "xl/_rels/workbook.xml.rels")
                .unwrap()
                .contains("vbaProject")
        );
    }

    #[test]
    fn formulas_in_open_formula() {
        assert_eq!(open_formula("SUM(A1:B2,C3)"), "SUM([.A1:.B2];[.C3])");
        assert_eq!(
            open_formula("'My Sheet'!$A$1*Data!B2"),
            "['My Sheet'.$A$1]*[Data.B2]"
        );
        assert_eq!(
            open_formula("IF(A1=\"a,b\",LOG10(2),1)"),
            "IF([.A1]=\"a,b\";LOG10(2);1)"
        );
        assert_eq!(open_formula("_xlfn.CONCAT(A:A)"), "CONCAT([.A:.A])");
        assert_eq!(open_formula("SUM({1,2;3,4})"), "SUM({1;2|3;4})");
    }

    #[test]
    fn a_workbook_as_an_opendocument_spreadsheet() {
        use kalem_viewer::{FileHandle, Viewer as _};
        let book = new_workbook(
            "xlsx",
            &[NewSheet {
                name: "S".into(),
                rows: vec![vec!["2".into(), "=A1*3".into(), "2026-10-04".into()]],
            }],
        )
        .unwrap();
        let dir = std::env::temp_dir().join(format!("xlsx-ods-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("book.xlsx");
        std::fs::write(&f, book).unwrap();
        let mut doc = crate::viewer::XlsxViewer.open(FileHandle::new(&f)).unwrap();
        let ods = doc.save_as("ods").unwrap().bytes;
        assert!(ods[30..].starts_with(b"mimetypeapplication/vnd.oasis.opendocument.spreadsheet"));
        let content = text(&parts(&ods), "content.xml").unwrap();
        assert!(
            content.contains(r#"table:formula="of:=[.A1]*3""#),
            "{content}"
        );
        assert!(
            content.contains(r#"office:date-value="2026-10-04""#),
            "{content}"
        );
        assert!(doc.save_as("csv").is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
