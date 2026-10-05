//! Workbooks the plugin shows but does not write: Excel 97–2003 (`.xls`,
//! BIFF8), Excel's binary workbook (`.xlsb`) and OpenDocument spreadsheets
//! (`.ods`), read through calamine (Kalem's task T3.7.7).
//!
//! There is no faithful writer for these formats here, so nothing is ever
//! saved to them: the view offers no edits, and Kalem never converts the
//! file to open it.

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, SheetType, SheetVisible};

use crate::cellref::{CellRef, Range};
use crate::numfmt;
use crate::sheet::Value;

/// Which format was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyFormat {
    /// Excel 97–2003.
    Xls,
    /// Excel binary workbook.
    Xlsb,
    /// OpenDocument spreadsheet.
    Ods,
}

/// A cell as read.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyCell {
    /// The value.
    pub value: Value,
    /// Whether the number is a date or time, and which.
    pub date: Option<DateKind>,
    /// The formula, without `=`.
    pub formula: Option<String>,
}

/// What a date cell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateKind {
    /// A day.
    Date,
    /// A time of day.
    Time,
    /// A day and a time.
    DateTime,
    /// A duration.
    Duration,
}

/// A sheet as read.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacySheet {
    /// The tab's name.
    pub name: String,
    /// Hidden or very hidden.
    pub hidden: bool,
    /// Whether it holds cells (a chart sheet does not).
    pub worksheet: bool,
    /// The cells that hold something.
    pub cells: BTreeMap<CellRef, LegacyCell>,
}

impl LegacySheet {
    /// The used range, `None` for an empty sheet.
    pub fn used_range(&self) -> Option<Range> {
        let first = *self.cells.keys().next()?;
        let (mut r1, mut c0, mut c1) = (first.row, first.col, first.col);
        for k in self.cells.keys() {
            r1 = r1.max(k.row);
            c0 = c0.min(k.col);
            c1 = c1.max(k.col);
        }
        Some(Range {
            start: CellRef::new(first.row, c0),
            end: CellRef::new(r1, c1),
        })
    }
}

/// A workbook read for viewing.
#[derive(Debug, Clone)]
pub struct LegacyWorkbook {
    /// The format.
    pub format: LegacyFormat,
    /// The sheets in order.
    pub sheets: Vec<LegacySheet>,
    /// An OpenDocument spreadsheet's cell styles.
    pub looks: Option<crate::ods_style::Looks>,
}

/// An ISO 8601 date, or date and time (`2026-10-03`,
/// `2026-10-03T14:30:00`, with fractions of a second), as a serial in the
/// 1900 date system and its kind.
fn iso_date_time(s: &str) -> Option<(f64, DateKind)> {
    let (date, time) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut parts = date.splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let day = crate::numfmt::date_to_serial(y, m, d, false) as f64;
    match time {
        None => Some((day, DateKind::Date)),
        Some(t) => Some((day + clock(t)?, DateKind::DateTime)),
    }
}

/// `hh:mm:ss` (seconds with a fraction, optional) as a fraction of a day.
fn clock(t: &str) -> Option<f64> {
    let mut parts = t.splitn(3, ':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next().map_or(Some(0.0), |s| s.parse().ok())?;
    Some((h * 3600.0 + m * 60.0 + s) / 86_400.0)
}

/// An ISO 8601 duration as OpenDocument writes a time (`PT09H15M00S`,
/// `P1DT2H`, `-PT30M`) in days.
fn iso_duration(s: &str) -> Option<f64> {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1.0, r),
        None => (1.0, s),
    };
    let rest = rest.strip_prefix('P')?;
    let (days, time) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };
    let mut total = 0.0;
    if !days.is_empty() {
        total += days.strip_suffix('D')?.parse::<f64>().ok()?;
    }
    let mut number = String::new();
    for ch in time.chars() {
        let scale = match ch {
            'H' => 1.0 / 24.0,
            'M' => 1.0 / 1440.0,
            'S' => 1.0 / 86_400.0,
            _ => {
                number.push(ch);
                continue;
            }
        };
        total += number.parse::<f64>().ok()? * scale;
        number.clear();
    }
    number.is_empty().then_some(sign * total)
}

fn date_kind(v: f64, duration: bool) -> DateKind {
    if duration {
        DateKind::Duration
    } else if v < 1.0 {
        DateKind::Time
    } else if v.fract() == 0.0 {
        DateKind::Date
    } else {
        DateKind::DateTime
    }
}

impl LegacyWorkbook {
    /// Reads a workbook from its bytes.
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let format = if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
            LegacyFormat::Xls
        } else if bytes.windows(8).take(4096).any(|w| w == b"mimetype") {
            LegacyFormat::Ods
        } else {
            LegacyFormat::Xlsb
        };
        let looks = if format == LegacyFormat::Ods {
            crate::ods_style::Looks::read(&bytes)
        } else {
            None
        };
        let mut book =
            calamine::open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let meta: Vec<calamine::Sheet> = book.sheets_metadata().to_vec();
        let mut sheets = Vec::new();
        for m in meta {
            let worksheet = matches!(m.typ, SheetType::WorkSheet);
            let mut cells = BTreeMap::new();
            if worksheet {
                let range = book.worksheet_range(&m.name).map_err(|e| e.to_string())?;
                let (r0, c0) = range.start().unwrap_or((0, 0));
                for (r, c, d) in range.used_cells() {
                    let at = CellRef::new(r0 + r as u32, c0 + c as u32);
                    let (value, date) = match d {
                        Data::Empty => continue,
                        Data::Int(i) => (Value::Number(*i as f64), None),
                        Data::Float(f) => (Value::Number(*f), None),
                        Data::String(s) => (Value::Text(s.clone()), None),
                        Data::Bool(b) => (Value::Bool(*b), None),
                        Data::Error(e) => (Value::Error(e.to_string()), None),
                        Data::DateTime(dt) => {
                            let v = dt.as_f64();
                            (Value::Number(v), Some(date_kind(v, dt.is_duration())))
                        }
                        // OpenDocument's dates and times come as ISO 8601
                        // text: as numbers with their kind, as Excel's do.
                        Data::DateTimeIso(s) => match iso_date_time(s) {
                            Some((v, kind)) => (Value::Number(v), Some(kind)),
                            None => (Value::Text(s.clone()), None),
                        },
                        Data::DurationIso(s) => match iso_duration(s) {
                            Some(v) if (0.0..1.0).contains(&v) => {
                                (Value::Number(v), Some(DateKind::Time))
                            }
                            Some(v) => (Value::Number(v), Some(DateKind::Duration)),
                            None => (Value::Text(s.clone()), None),
                        },
                    };
                    cells.insert(
                        at,
                        LegacyCell {
                            value,
                            date,
                            formula: None,
                        },
                    );
                }
                if let Ok(formulas) = book.worksheet_formula(&m.name) {
                    let (r0, c0) = formulas.start().unwrap_or((0, 0));
                    for (r, c, f) in formulas.used_cells() {
                        if f.is_empty() {
                            continue;
                        }
                        let at = CellRef::new(r0 + r as u32, c0 + c as u32);
                        let f = if format == LegacyFormat::Ods {
                            odf_to_a1(f)
                        } else {
                            f.clone()
                        };
                        let f = f.strip_prefix('=').unwrap_or(&f).to_owned();
                        if format == LegacyFormat::Xls && !plausible_xls_formula(&f) {
                            // calamine 0.36 decodes some BIFF8 references
                            // wrongly (an area's relative-flag bits taken as
                            // column bits); such a formula is not shown, its
                            // value is.
                            continue;
                        }
                        cells
                            .entry(at)
                            .or_insert(LegacyCell {
                                value: Value::Empty,
                                date: None,
                                formula: None,
                            })
                            .formula = Some(f);
                    }
                }
            }
            sheets.push(LegacySheet {
                name: m.name,
                hidden: !matches!(m.visible, SheetVisible::Visible),
                worksheet,
                cells,
            });
        }
        Ok(Self {
            format,
            sheets,
            looks,
        })
    }

    /// A cell as the grid shows it. The files' number formats are not
    /// carried by the reader, so numbers show in General and dates in ISO form.
    pub fn display(&self, idx: usize, at: CellRef) -> String {
        let Some(c) = self.sheets.get(idx).and_then(|s| s.cells.get(&at)) else {
            return String::new();
        };
        show(c)
    }

    /// A cell as the formula bar shows it.
    pub fn edit_text(&self, idx: usize, at: CellRef) -> String {
        let Some(c) = self.sheets.get(idx).and_then(|s| s.cells.get(&at)) else {
            return String::new();
        };
        match &c.formula {
            Some(f) => format!("={f}"),
            None => show(c),
        }
    }

    /// The used range as display strings, row by row.
    pub fn grid(&self, idx: usize) -> Vec<Vec<String>> {
        let Some(s) = self.sheets.get(idx) else {
            return Vec::new();
        };
        let Some(used) = s.used_range() else {
            return Vec::new();
        };
        let width = (used.end.col + 1) as usize;
        let mut rows = vec![vec![String::new(); width]; (used.end.row + 1) as usize];
        for (p, c) in &s.cells {
            rows[p.row as usize][p.col as usize] = show(c);
        }
        rows
    }
}

/// An OpenFormula expression (ODF 1.2 part 2) in Excel's notation, for
/// the formula bar: `of:=SUM([.A1:.B2];[Sheet2.C3])` reads
/// `=SUM(A1:B2,Sheet2!C3)`.
pub fn odf_to_a1(f: &str) -> String {
    let f = f.strip_prefix("of:").unwrap_or(f);
    let mut out = String::with_capacity(f.len());
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                for d in chars.by_ref() {
                    out.push(d);
                    if d == '"' {
                        break;
                    }
                }
            }
            ';' => out.push(','),
            '[' => {
                let mut inner = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    inner.push(d);
                }
                // Each side is `Sheet.A1`, `.A1` or `$Sheet.$A$1`.
                let side = |s: &str, first: bool| -> String {
                    let s = s.trim();
                    match s.rfind('.') {
                        Some(p) => {
                            let sheet = s[..p].trim_start_matches('$');
                            let cell = &s[p + 1..];
                            if sheet.is_empty() || !first {
                                cell.to_owned()
                            } else {
                                format!("{sheet}!{cell}")
                            }
                        }
                        None => s.to_owned(),
                    }
                };
                match inner.split_once(':') {
                    Some((a, b)) => {
                        out.push_str(&side(a, true));
                        out.push(':');
                        out.push_str(&side(b, false));
                    }
                    None => out.push_str(&side(&inner, true)),
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Whether every reference of an `.xls` formula lies on a BIFF8 sheet
/// (256 columns, 65,536 rows).
fn plausible_xls_formula(f: &str) -> bool {
    let mut ok = true;
    crate::formula::map_refs(f, |_, r| {
        for e in [r.start, r.end] {
            if e.col.is_some_and(|(c, _)| c > 255) || e.row.is_some_and(|(r, _)| r > 65_535) {
                ok = false;
            }
        }
        Some(r)
    });
    ok
}

fn show(c: &LegacyCell) -> String {
    match (&c.value, c.date) {
        (Value::Number(v), Some(kind)) => {
            let code = match kind {
                DateKind::Date => "yyyy-mm-dd",
                DateKind::Time => "hh:mm:ss",
                DateKind::DateTime => "yyyy-mm-dd hh:mm:ss",
                DateKind::Duration => "[h]:mm:ss",
            };
            numfmt::format_number(*v, code, false)
        }
        (Value::Number(v), None) => numfmt::format_general(*v),
        (Value::Text(t), _) => t.clone(),
        (Value::Bool(b), _) => if *b { "TRUE" } else { "FALSE" }.into(),
        (Value::Error(e), _) => e.clone(),
        (Value::Empty, _) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::DateKind;

    #[test]
    fn iso_dates_and_times_are_numbers() {
        // 2026-10-03 is serial 46298 in Excel's 1900 system.
        assert_eq!(
            super::iso_date_time("2026-10-03"),
            Some((46298.0, DateKind::Date))
        );
        let (v, kind) = super::iso_date_time("2026-10-03T14:30:00").unwrap();
        assert_eq!(kind, DateKind::DateTime);
        assert!((v - (46298.0 + 14.5 / 24.0)).abs() < 1e-9);
        assert!(super::iso_date_time("2026-13-03").is_none());
        assert!(super::iso_date_time("tomorrow").is_none());
        let t = super::iso_duration("PT09H15M00S").unwrap();
        assert!((t - 9.25 / 24.0).abs() < 1e-9);
        let d = super::iso_duration("P1DT2H30M").unwrap();
        assert!((d - (1.0 + 2.5 / 24.0)).abs() < 1e-9);
        assert!((super::iso_duration("-PT30M").unwrap() + 0.5 / 24.0).abs() < 1e-9);
        assert!(super::iso_duration("PT9X").is_none());
        assert!(super::iso_duration("9H").is_none());
    }

    #[test]
    fn open_formula_reads_as_a1() {
        assert_eq!(super::odf_to_a1("of:=SUM([.D2:.D4])"), "=SUM(D2:D4)");
        assert_eq!(
            super::odf_to_a1("of:=[$Data.$A$1]+IF([.B1]>0;\"a;b\";[.C1])"),
            "=Data!$A$1+IF(B1>0,\"a;b\",C1)"
        );
        assert_eq!(
            super::odf_to_a1("of:=['My Sheet'.A1:.B2]"),
            "='My Sheet'!A1:B2"
        );
    }
}
