//! The plugin as Kalem's `document-viewer` and `document-editor` (D54):
//! a workbook's sheets are grid units the host draws, cells are edited as
//! typed, rows and columns inserted and deleted, edits undone by the
//! workbook's own history, macros listed and run on the user's command,
//! and the file saved as itself.

use std::collections::HashMap;

use kalem_viewer::{
    Align, AxisFont, AxisScale, Bitmap, Chart, ChartAxis, ChartKind, CompareOp, CondRule,
    CondStyle, DataLabels, Detection, ErrorStyle, FileHandle, GridCell, GridEdit, GridLayout,
    Gridlines, InfoField, LegendPosition, MacroEntry, MacroOutcome, MacroQuestion, MacroUi, Paint,
    PivotSpec, RenderRequest, Rendered, Result, SaveOutput, Structure, StyleChange, Unit, UnitKind,
    VAlign, Validation, ValidationError, ValidationKind, Viewer, ViewerDocument, ViewerError,
};

use crate::cellref::{CellRef, MAX_COL, MAX_ROW};
use crate::sheet::Value;
use crate::workbook::{SheetKind, Visibility, Workbook};

/// The extensions of SpreadsheetML workbooks.
const OOXML: [&str; 4] = ["xlsx", "xlsm", "xltx", "xltm"];

/// The viewer of Excel workbooks.
#[derive(Debug, Default, Clone, Copy)]
pub struct XlsxViewer;

fn err(e: impl std::fmt::Display) -> ViewerError {
    ViewerError(e.to_string())
}

/// The headings' height and width in twips, as Excel counts them in a
/// split's sizes.
const HEAD_TWIPS: (f64, f64) = (300.0, 400.0);

/// A row's height and a column's width in twips.
fn row_twips(l: &GridLayout, r: u32) -> f64 {
    let pt = l
        .heights
        .iter()
        .find(|(i, _)| *i == r)
        .map_or(l.default_height, |(_, h)| *h);
    f64::from(pt) * 20.0
}

fn col_twips(l: &GridLayout, c: u32) -> f64 {
    let w = l.widths.get(c as usize).copied().unwrap_or(l.default_width);
    // Characters as pixels (`7 × width + 5`), pixels as points.
    f64::from(w * 7.0 + 5.0) * 0.75 * 20.0
}

/// A split's top pane rows and left pane columns, from its sizes.
fn split_cells(l: &GridLayout, s: &crate::workbook::SplitRaw, headings: bool) -> (u32, u32) {
    let (hy, hx) = if headings { HEAD_TWIPS } else { (0.0, 0.0) };
    let count = |room: f64, from: u32, size: &dyn Fn(u32) -> f64| -> u32 {
        if room <= 0.0 {
            return 0;
        }
        let (mut used, mut n) = (0.0, 0);
        while used + size(from + n) / 2.0 < room && n < 500 {
            used += size(from + n);
            n += 1;
        }
        n.max(1)
    };
    (
        count(s.y - hy, s.top_left.row, &|r| row_twips(l, r)),
        count(s.x - hx, s.top_left.col, &|c| col_twips(l, c)),
    )
}

/// A split's sizes in twips, from its rows and columns.
fn split_twips(l: &GridLayout, [rows, cols, top, left]: [u32; 4], headings: bool) -> (f64, f64) {
    let (hy, hx) = if headings { HEAD_TWIPS } else { (0.0, 0.0) };
    let y = if rows == 0 {
        0.0
    } else {
        hy + (top..top + rows).map(|r| row_twips(l, r)).sum::<f64>()
    };
    let x = if cols == 0 {
        0.0
    } else {
        hx + (left..left + cols).map(|c| col_twips(l, c)).sum::<f64>()
    };
    (y, x)
}

fn rgb(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

impl Viewer for XlsxViewer {
    fn id(&self) -> &str {
        "xlsx"
    }

    fn name(&self) -> &str {
        "Excel workbooks"
    }

    fn extensions(&self) -> &[&str] {
        #[cfg(not(target_family = "wasm"))]
        {
            &["xlsx", "xlsm", "xltx", "xltm", "xls", "xlsb", "ods"]
        }
        #[cfg(target_family = "wasm")]
        {
            &OOXML
        }
    }

    fn detect(&self, name: &str, head: &[u8]) -> Detection {
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        let zip = head.starts_with(b"PK\x03\x04");
        let ole = head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]);
        let ods = zip
            && head
                .windows(46)
                .any(|w| w == b"application/vnd.oasis.opendocument.spreadsheet");
        let ok = match ext.as_str() {
            e if OOXML.contains(&e) => zip,
            "xlsb" => zip && cfg!(not(target_family = "wasm")),
            "xls" => ole && cfg!(not(target_family = "wasm")),
            "ods" => ods && cfg!(not(target_family = "wasm")),
            _ => false,
        };
        if ok {
            Detection::Magic
        } else if self.extensions().contains(&ext.as_str()) {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        #[cfg(target_arch = "wasm32")]
        crate::component_clock();
        let bytes = file.read_all()?;
        let ext = file.extension();
        if !OOXML.contains(&ext.as_str()) {
            #[cfg(not(target_family = "wasm"))]
            return Ok(Box::new(LegacyDoc {
                wb: crate::legacy::LegacyWorkbook::open(bytes).map_err(err)?,
            }));
            #[cfg(target_family = "wasm")]
            return Err(ViewerError(format!(".{ext} is not opened by this build")));
        }
        let wb = Workbook::open(bytes).map_err(err)?;
        Ok(Box::new(XlsxDoc {
            wb: std::sync::Mutex::new(wb),
            saved_at: 0,
            view_changed: false,
            notes: HashMap::new(),
            cf: HashMap::new(),
            charts: HashMap::new(),
            fill_lists: Vec::new(),
            name: file.name().to_owned(),
        }))
    }
}

/// An open workbook.
struct XlsxDoc {
    /// Behind a lock so that `text(&self)` can read sheets lazily.
    wb: std::sync::Mutex<Workbook>,
    /// The history's length when last saved.
    saved_at: usize,
    /// A sheet's view settings changed since the last save.
    view_changed: bool,
    /// Each sheet's notes, read once.
    notes: HashMap<usize, HashMap<CellRef, String>>,
    /// Each sheet's conditional formats and what they made of its cells,
    /// at the workbook's generation they were read.
    cf: HashMap<usize, (u64, crate::conditional::Evaluator)>,
    /// Each sheet's charts, at the generation they were read.
    charts: HashMap<usize, (u64, Vec<Chart>)>,
    /// The user's own lists a fill goes round.
    fill_lists: Vec<Vec<String>>,
    name: String,
}

fn units(names: impl Iterator<Item = (String, bool, bool)>) -> Structure {
    Structure {
        units: names
            .map(|(name, hidden, grid)| Unit {
                kind: if grid {
                    UnitKind::Sheet
                } else {
                    UnitKind::Image
                },
                label: if hidden {
                    format!("{name} (hidden)")
                } else {
                    name
                },
                duration_ms: None,
            })
            .collect(),
        outline: Vec::new(),
    }
}

/// A blank page: what a sheet renders to for a host without grids.
fn blank() -> Rendered {
    Rendered::Bitmap(Bitmap::new(1, 1, vec![255, 255, 255, 255]))
}

fn tsv(rows: Vec<Vec<String>>) -> String {
    rows.into_iter()
        .map(|r| r.join("\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The host's dialogs as the interpreter's: a dialog the host cannot
/// answer now stops the run and is kept as the question.
struct Ui<'a> {
    ui: &'a mut dyn MacroUi,
    name: String,
    question: Option<MacroQuestion>,
}

impl crate::macros::MacroHost for Ui<'_> {
    fn msg_box(&mut self, prompt: &str, buttons: i64, title: &str) -> i64 {
        match self.ui.message(prompt, buttons, title) {
            Some(a) => a,
            None => {
                self.question = Some(MacroQuestion::Message {
                    prompt: prompt.into(),
                    buttons,
                    title: title.into(),
                });
                2
            }
        }
    }

    fn input_box(&mut self, prompt: &str, title: &str, default: &str) -> Option<String> {
        match self.ui.input(prompt, title, default) {
            Some(a) => a,
            None => {
                self.question = Some(MacroQuestion::Input {
                    prompt: prompt.into(),
                    title: title.into(),
                    default: default.into(),
                });
                None
            }
        }
    }

    fn workbook_name(&mut self) -> String {
        self.name.clone()
    }

    fn stopped(&self) -> bool {
        self.question.is_some()
    }
}

impl XlsxDoc {
    fn book(&mut self) -> &mut Workbook {
        self.wb
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Workbook> {
        self.wb
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn worksheet(&self, unit: usize) -> bool {
        self.locked()
            .sheets()
            .get(unit)
            .is_some_and(|s| s.kind == SheetKind::Worksheet)
    }

    fn chartsheet(&self, unit: usize) -> bool {
        self.locked()
            .sheets()
            .get(unit)
            .is_some_and(|s| s.kind == SheetKind::Chartsheet)
    }

    /// Puts what the sheet's conditional formats make of the cells in
    /// view into `out`, adding the empty cells they color.
    fn conditional(
        &mut self,
        unit: usize,
        rows: &std::ops::Range<u32>,
        cols: &std::ops::Range<u32>,
        out: &mut Vec<(u32, u32, GridCell)>,
    ) {
        let generation = self.book().generation();
        let mut ev = match self.cf.remove(&unit) {
            Some((g, ev)) if g == generation => ev,
            _ => match self.book().conditional_formats(unit) {
                Ok(f) => crate::conditional::Evaluator::new(f),
                Err(_) => return,
            },
        };
        if ev.is_empty() {
            self.cf.insert(unit, (generation, ev));
            return;
        }
        let dxfs = self.book().dxfs().to_vec();
        let mut index: HashMap<(u32, u32), usize> = out
            .iter()
            .enumerate()
            .map(|(i, (r, c, _))| ((*r, *c), i))
            .collect();
        // The empty cells too, where a rule may color them; not past a
        // screenful, so a whole-column rule stays cheap.
        let empties: Vec<CellRef> = if ev.colors_empty() {
            rows.clone()
                .take(200)
                .flat_map(|r| cols.clone().take(60).map(move |c| CellRef::new(r, c)))
                .filter(|p| !index.contains_key(&(p.row, p.col)) && ev.touches(*p))
                .collect()
        } else {
            Vec::new()
        };
        for at in empties {
            index.insert((at.row, at.col), out.len());
            out.push((at.row, at.col, GridCell::default()));
        }
        // The formulas the rules need for these cells, and for a screenful
        // above and below, computed together: a held arrow key then costs
        // one recalculation a screenful, not one a row.
        let h = rows.end - rows.start;
        let ahead = rows.start.saturating_sub(h)..rows.end.saturating_add(h);
        let mut touched: Vec<CellRef> = index
            .keys()
            .map(|&(r, c)| CellRef::new(r, c))
            .filter(|p| ev.touches(*p))
            .collect();
        if let Ok(sheet) = self.book().sheet(unit) {
            touched.extend(
                sheet
                    .cells
                    .range(CellRef::new(ahead.start, 0)..CellRef::new(ahead.end, 0))
                    .map(|(p, _)| *p)
                    .filter(|p| cols.contains(&p.col) && !rows.contains(&p.row))
                    .take(20_000),
            );
        }
        touched.retain(|p| ev.touches(*p));
        ev.prefetch(&touched, self.book(), unit);
        let mut unused = Vec::new();
        for (&(row, col), &i) in &index {
            let at = CellRef::new(row, col);
            if !ev.touches(at) {
                continue;
            }
            let r = ev.result(at, self.book(), unit, &dxfs);
            let cell = &mut out[i].2;
            if r == crate::conditional::CfResult::default() {
                if cell == &GridCell::default() {
                    unused.push(i);
                }
                continue;
            }
            cell.bold = r.bold.unwrap_or(cell.bold);
            cell.italic = r.italic.unwrap_or(cell.italic);
            cell.underline = r.underline.unwrap_or(cell.underline);
            cell.strike = r.strike.unwrap_or(cell.strike);
            if let Some(c) = r.color {
                cell.color = Some(rgb(c));
            }
            if let Some(c) = r.fill {
                cell.fill = Some(rgb(c));
            }
            cell.bar = r.bar.map(|(w, c)| (w, rgb(c)));
            cell.icon = r.icon.map(|(g, c)| (g, rgb(c)));
        }
        // Empty cells no rule colored are not shown.
        unused.sort_unstable();
        for i in unused.into_iter().rev() {
            out.swap_remove(i);
        }
        self.cf.insert(unit, (generation, ev));
    }

    fn notes(&mut self, unit: usize) -> &HashMap<CellRef, String> {
        if !self.notes.contains_key(&unit) {
            let n = self
                .book()
                .comments(unit)
                .unwrap_or_default()
                .into_iter()
                // A thread's note is the thread's, shown as the thread.
                .filter(|c| !c.author.starts_with("tc="))
                .map(|c| (c.cell, c.text))
                .collect();
            self.notes.insert(unit, n);
        }
        &self.notes[&unit]
    }

    fn all_units(&self) -> Vec<usize> {
        (0..self.locked().sheets().len()).collect()
    }
}

impl XlsxDoc {
    /// Tables drawn in their style: the header row filled and bold, the
    /// data rows banded, the total row bold; a cell's own fill and font
    /// color first. Empty cells of a table get its look too.
    /// The sparklines drawn in the cells shown: their points scaled to
    /// 0..1000 from their data's values.
    fn sparklines(
        &mut self,
        unit: usize,
        rows: &std::ops::Range<u32>,
        cols: &std::ops::Range<u32>,
        out: &mut Vec<(u32, u32, GridCell)>,
    ) {
        use kalem_viewer::{Sparkline, SparklineKind};
        for s in self.book().sparklines(unit) {
            if !rows.contains(&s.cell.row) || !cols.contains(&s.cell.col) {
                continue;
            }
            let sheet = match &s.sheet {
                Some(name) => match self.book().sheet_index(name) {
                    Some(i) => i,
                    None => continue,
                },
                None => unit,
            };
            let mut values = Vec::new();
            for r in s.data.start.row..=s.data.end.row {
                for c in s.data.start.col..=s.data.end.col {
                    values.push(match self.book().value(sheet, CellRef::new(r, c)) {
                        Ok(Value::Number(n)) if n.is_finite() => Some(n),
                        _ => None,
                    });
                }
            }
            let known = values.iter().flatten().copied();
            let (min, max) = known.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v), b.max(v))
            });
            let mut line = Sparkline {
                kind: s.kind,
                color: s.color,
                marker: s.marker,
                ..Sparkline::default()
            };
            if min.is_finite() {
                // Columns stand on zero; a line spans its values.
                let (lo, hi) = match s.kind {
                    SparklineKind::Column => (min.min(0.0), max.max(0.0)),
                    _ => (min, max),
                };
                let scale = |v: f64| {
                    if hi > lo {
                        ((v - lo) / (hi - lo) * 1000.0).round() as u16
                    } else {
                        500
                    }
                };
                line.points = values
                    .iter()
                    .map(|v| {
                        v.map(|v| match s.kind {
                            SparklineKind::WinLoss if v > 0.0 => 1000,
                            SparklineKind::WinLoss if v < 0.0 => 0,
                            SparklineKind::WinLoss => 500,
                            _ => scale(v),
                        })
                    })
                    .collect();
                line.zero = match s.kind {
                    SparklineKind::WinLoss => Some(500),
                    _ if lo <= 0.0 && hi >= 0.0 && hi > lo => Some(scale(0.0)),
                    _ => None,
                };
                let at = |want: f64| values.iter().position(|v| *v == Some(want));
                line.high = s.high.then(|| at(max)).flatten();
                line.low = s.low.then(|| at(min)).flatten();
            } else {
                line.points = vec![None; values.len()];
            }
            match out
                .iter_mut()
                .find(|(r, c, _)| (*r, *c) == (s.cell.row, s.cell.col))
            {
                Some((_, _, cell)) => cell.sparkline = Some(line),
                None => out.push((
                    s.cell.row,
                    s.cell.col,
                    GridCell {
                        sparkline: Some(line),
                        ..GridCell::default()
                    },
                )),
            }
        }
    }

    fn table_look(
        &mut self,
        unit: usize,
        rows: &std::ops::Range<u32>,
        cols: &std::ops::Range<u32>,
        out: &mut Vec<(u32, u32, GridCell)>,
    ) {
        let tables = self.book().sheet_tables(unit);
        for t in tables {
            let (h, text, band) = crate::workbook::style_colors(&t.style);
            let r = t.range;
            for row in r.start.row.max(rows.start)..=r.end.row.min(rows.end.saturating_sub(1)) {
                for col in r.start.col.max(cols.start)..=r.end.col.min(cols.end.saturating_sub(1)) {
                    if !out.iter().any(|x| x.0 == row && x.1 == col) {
                        out.push((row, col, GridCell::default()));
                    }
                }
            }
            let data_start = r.start.row + u32::from(t.header);
            for (row, col, cell) in out.iter_mut() {
                if !(r.start.row..=r.end.row).contains(row)
                    || !(r.start.col..=r.end.col).contains(col)
                {
                    continue;
                }
                if t.header && *row == r.start.row {
                    cell.fill = cell.fill.or(Some(h));
                    cell.color = cell.color.or(Some(text));
                    cell.bold = true;
                } else if t.totals && *row == r.end.row {
                    cell.bold = true;
                } else if t.stripes && (*row - data_start) % 2 == 0 {
                    cell.fill = cell.fill.or(Some(band));
                }
            }
        }
    }
}

impl ViewerDocument for XlsxDoc {
    fn structure(&self) -> Structure {
        units(self.locked().sheets().iter().map(|s| {
            (
                s.name.clone(),
                s.visibility != Visibility::Visible,
                matches!(s.kind, SheetKind::Worksheet | SheetKind::Chartsheet),
            )
        }))
    }

    fn render(&mut self, _unit: usize, _request: RenderRequest) -> Result<Rendered> {
        Ok(blank())
    }

    fn text(&self, unit: usize) -> String {
        self.locked().grid(unit).map(tsv).unwrap_or_default()
    }

    fn info(&self) -> Vec<InfoField> {
        let wb = self.locked();
        let mut out = vec![
            InfoField::new("Format", "Excel workbook (SpreadsheetML)"),
            InfoField::new("Sheets", wb.sheets().len().to_string()),
            InfoField::new("Parts", wb.parts().len().to_string()),
        ];
        if wb.has_vba() {
            out.push(InfoField::new("Macros", "VBA project, run only on command"));
        }
        out
    }

    fn grid(&mut self, unit: usize) -> Option<GridLayout> {
        // A chart sheet: empty cells under its chart, not edited.
        if self.chartsheet(unit) {
            return Some(GridLayout {
                rows: 33,
                cols: 14,
                max_rows: 33,
                max_cols: 14,
                default_width: 8.43,
                default_height: 15.0,
                editable: false,
                ..GridLayout::default()
            });
        }
        if !self.worksheet(unit) {
            return None;
        }
        let sheet = self.book().sheet(unit).ok()?;
        let used = sheet.used_range();
        let mut widths = Vec::new();
        for c in &sheet.cols {
            if let Some(w) = c.width {
                let last = c.max.min(used.map_or(64, |u| u.end.col + 64));
                if widths.len() <= last as usize {
                    widths.resize(last as usize + 1, f32::NAN);
                }
                for col in c.min..=last {
                    widths[col as usize] = w as f32;
                }
            }
        }
        let default_width = sheet.default_col_width.unwrap_or(8.43) as f32;
        for w in &mut widths {
            if w.is_nan() {
                *w = default_width;
            }
        }
        Some(GridLayout {
            rows: used.map_or(0, |u| u.end.row + 1),
            cols: used.map_or(0, |u| u.end.col + 1),
            max_rows: MAX_ROW,
            max_cols: MAX_COL,
            widths,
            default_width,
            hidden_rows: sheet
                .rows
                .iter()
                .filter(|(_, r)| r.hidden)
                .map(|(i, _)| *i)
                .collect(),
            hidden_cols: sheet
                .cols
                .iter()
                .filter(|c| c.hidden)
                .flat_map(|c| c.min..=c.max.min(c.min + 1024))
                .collect(),
            merged: sheet
                .merged
                .iter()
                .map(|m| [m.start.row, m.start.col, m.end.row, m.end.col])
                .collect(),
            frozen: sheet.frozen.unwrap_or((0, 0)),
            editable: true,
            heights: sheet
                .rows
                .iter()
                .filter_map(|(i, r)| r.height.map(|h| (*i, h as f32)))
                .collect(),
            default_height: sheet.default_row_height.unwrap_or(15.0) as f32,
            filter: sheet.auto_filter.as_ref().map(|f| {
                [
                    f.range.start.row,
                    f.range.start.col,
                    f.range.end.row,
                    f.range.end.col,
                ]
            }),
            filtered: sheet
                .auto_filter
                .as_ref()
                .map(|f| {
                    f.columns
                        .iter()
                        .map(|c| f.range.start.col + c.col_id)
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    fn outline(&mut self, unit: usize) -> (Vec<(u32, u8)>, Vec<(u32, u8)>) {
        self.book().outline(unit).unwrap_or_default()
    }

    fn set_outline(
        &mut self,
        unit: usize,
        rows: bool,
        from: u32,
        to: u32,
        deeper: bool,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_outline(unit, rows, from, to, deeper)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_detail_shown(
        &mut self,
        unit: usize,
        rows: bool,
        at: u32,
        shown: bool,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_detail_shown(unit, rows, at, shown)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn subtotal(
        &mut self,
        unit: usize,
        range: [u32; 4],
        by: u32,
        function: u32,
        columns: &[u32],
    ) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .subtotal(unit, r, by, function, columns)
            .map_err(err)?;
        Ok(self.all_units())
    }

    fn begin_batch(&mut self) {
        let _ = self.book().begin_batch();
    }

    fn end_batch(&mut self) {
        let _ = self.book().end_batch();
    }

    fn conditional_ranges(&mut self, unit: usize) -> Vec<[u32; 4]> {
        self.book()
            .conditional_formats(unit)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|f| f.ranges)
            .map(|r| [r.start.row, r.start.col, r.end.row, r.end.col])
            .collect()
    }

    fn evaluate_formulas(&mut self, unit: usize, formulas: &[String]) -> Vec<Option<String>> {
        let Ok(values) = self.book().evaluate_formulas(unit, formulas) else {
            return vec![None; formulas.len()];
        };
        values
            .into_iter()
            .map(|v| {
                v.map(|v| match v {
                    Value::Number(n) => {
                        if n.fract() == 0.0 && n.abs() < 1e15 {
                            format!("{}", n as i64)
                        } else {
                            format!("{n}")
                        }
                    }
                    Value::Text(t) => format!("\"{}\"", t.replace('"', "\"\"")),
                    Value::Bool(b) => if b { "TRUE" } else { "FALSE" }.into(),
                    Value::Error(e) => e,
                    Value::Empty => "0".into(),
                })
            })
            .collect()
    }

    fn sheet_protection(&mut self, unit: usize) -> Option<kalem_viewer::SheetProtection> {
        self.book().sheet_protection(unit)
    }

    fn protect_sheet(
        &mut self,
        unit: usize,
        protection: Option<kalem_viewer::SheetProtection>,
        password: Option<&str>,
    ) -> Result<Vec<usize>> {
        self.book()
            .protect_sheet(unit, protection.as_ref(), password)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn workbook_protected(&mut self) -> bool {
        self.book().workbook_protected()
    }

    fn protect_workbook(&mut self, on: bool, password: Option<&str>) -> Result<Vec<usize>> {
        self.book().protect_workbook(on, password).map_err(err)?;
        Ok(self.all_units())
    }

    fn page_setup(&mut self, unit: usize) -> Option<kalem_viewer::PageSetup> {
        self.book().page_setup(unit).ok()
    }

    fn set_page_setup(
        &mut self,
        unit: usize,
        setup: &kalem_viewer::PageSetup,
    ) -> Result<Vec<usize>> {
        self.book().set_page_setup(unit, setup).map_err(err)?;
        Ok(vec![unit])
    }

    fn drawings(&mut self, unit: usize) -> Vec<kalem_viewer::Drawing> {
        self.book().drawings(unit)
    }

    fn drawing_image(&mut self, unit: usize, index: usize) -> Option<Vec<u8>> {
        self.book().drawing_image(unit, index)
    }

    fn insert_picture(
        &mut self,
        unit: usize,
        anchor: [u32; 4],
        bytes: &[u8],
        extension: &str,
    ) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(anchor[0], anchor[1]),
            end: CellRef::new(anchor[2], anchor[3]),
        };
        self.book()
            .insert_picture(unit, r, bytes, extension)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn insert_shape(
        &mut self,
        unit: usize,
        anchor: [u32; 4],
        preset: &str,
        text: &str,
        text_box: bool,
    ) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(anchor[0], anchor[1]),
            end: CellRef::new(anchor[2], anchor[3]),
        };
        self.book()
            .insert_shape(unit, r, preset, text, text_box)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn move_drawing(&mut self, unit: usize, index: usize, anchor: [u32; 4]) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(anchor[0], anchor[1]),
            end: CellRef::new(anchor[2], anchor[3]),
        };
        self.book().move_drawing(unit, index, r).map_err(err)?;
        Ok(vec![unit])
    }

    fn set_shape_text(&mut self, unit: usize, index: usize, text: &str) -> Result<Vec<usize>> {
        self.book().set_shape_text(unit, index, text).map_err(err)?;
        Ok(vec![unit])
    }

    fn delete_drawing(&mut self, unit: usize, index: usize) -> Result<Vec<usize>> {
        self.book().delete_drawing(unit, index).map_err(err)?;
        Ok(vec![unit])
    }

    fn tables(&mut self, unit: usize) -> Vec<kalem_viewer::TableInfo> {
        self.book()
            .sheet_tables(unit)
            .into_iter()
            .map(|t| kalem_viewer::TableInfo {
                name: t.name,
                range: [
                    t.range.start.row,
                    t.range.start.col,
                    t.range.end.row,
                    t.range.end.col,
                ],
                totals: t.totals,
                style: t.style,
            })
            .collect()
    }

    fn create_table(
        &mut self,
        unit: usize,
        range: [u32; 4],
        header: bool,
        style: &str,
    ) -> Result<String> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .create_table(unit, r, header, style)
            .map_err(err)
    }

    fn set_table_totals(&mut self, unit: usize, name: &str, on: bool) -> Result<Vec<usize>> {
        self.book().set_table_totals(unit, name, on).map_err(err)?;
        Ok(self.all_units())
    }

    fn remove_table(&mut self, unit: usize, name: &str) -> Result<Vec<usize>> {
        self.book().remove_table(unit, name).map_err(err)?;
        Ok(self.all_units())
    }

    fn grid_cells(
        &mut self,
        unit: usize,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32, GridCell)> {
        if !self.worksheet(unit) || rows.is_empty() || cols.is_empty() {
            return Vec::new();
        }
        let positions: Vec<CellRef> = match self.book().sheet(unit) {
            Ok(s) => s
                .cells
                .range(CellRef::new(rows.start, 0)..CellRef::new(rows.end, 0))
                .map(|(p, _)| *p)
                .filter(|p| cols.contains(&p.col))
                .collect(),
            Err(_) => return Vec::new(),
        };
        let notes: Vec<CellRef> = self.notes(unit).keys().copied().collect();
        let base = self.book().style(0);
        let mut out = Vec::with_capacity(positions.len());
        for at in positions {
            let text = self.book().display(unit, at).unwrap_or_default();
            let value = self.book().value(unit, at).unwrap_or(Value::Empty);
            let Ok(sheet) = self.book().sheet(unit) else {
                break;
            };
            let Some(cell) = sheet.cells.get(&at) else {
                continue;
            };
            let (formula, style) = (cell.formula.is_some(), cell.style);
            let style = self.book().style(style);
            let align = match style.align.as_deref() {
                Some("left") => Align::Left,
                Some("center" | "centerContinuous") => Align::Center,
                Some("right") => Align::Right,
                _ => Align::General,
            };
            #[allow(clippy::needless_update)]
            out.push((
                at.row,
                at.col,
                GridCell {
                    text,
                    numeric: matches!(value, Value::Number(_)),
                    bold: style.bold,
                    italic: style.italic,
                    underline: style.underline,
                    strike: style.strike,
                    color: style.color.map(rgb),
                    fill: style.fill.map(rgb),
                    align,
                    wrap: style.wrap,
                    formula,
                    note: notes.contains(&at),
                    bar: None,
                    icon: None,
                    // Size and typeface only where they differ from the
                    // workbook's default font.
                    font_size: style
                        .size
                        .filter(|s| Some(*s) != base.size)
                        .map(|s| (s * 10.0).round() as u16),
                    face: style.font.clone().filter(|f| Some(f) != base.font.as_ref()),
                    indent: style.indent,
                    rotation: style.rotation,
                    shrink: style.shrink,
                    center_across: style.align.as_deref() == Some("centerContinuous"),
                    unlocked: style.unlocked,
                    borders: style.sides.map(|s| s.map(|(c, _)| rgb(c))),
                    border_thick: style.sides.map(|s| s.is_some_and(|(_, t)| t)),
                    border_styles: style.lines,
                    fill_pattern: style.pattern.clone(),
                    valign: match style.valign.as_deref() {
                        Some("top") => VAlign::Top,
                        Some("center" | "justify" | "distributed") => VAlign::Middle,
                        _ => VAlign::Bottom,
                    },
                    // Fields the contract gains later start empty.
                    ..GridCell::default()
                },
            ));
        }
        // Cells not there that their row's or column's style paints.
        let styled = self.book().sheet(unit).is_ok_and(|s| {
            s.rows.values().any(|r| r.style.is_some()) || s.cols.iter().any(|c| c.style.is_some())
        });
        if styled {
            let have: std::collections::HashSet<(u32, u32)> =
                out.iter().map(|c| (c.0, c.1)).collect();
            let book = self.book();
            for r in rows.clone() {
                for c in cols.clone() {
                    if have.contains(&(r, c)) {
                        continue;
                    }
                    let s = book.row_or_col_style(unit, CellRef::new(r, c));
                    if s == 0 {
                        continue;
                    }
                    let style = book.style(s);
                    if style.fill.is_none()
                        && style.pattern.is_none()
                        && style.sides.iter().all(Option::is_none)
                    {
                        continue;
                    }
                    out.push((
                        r,
                        c,
                        GridCell {
                            fill: style.fill.map(rgb),
                            fill_pattern: style.pattern.clone(),
                            borders: style.sides.map(|s| s.map(|(c, _)| rgb(c))),
                            border_thick: style.sides.map(|s| s.is_some_and(|(_, t)| t)),
                            border_styles: style.lines,
                            ..GridCell::default()
                        },
                    ));
                }
            }
        }
        self.table_look(unit, &rows, &cols, &mut out);
        self.conditional(unit, &rows, &cols, &mut out);
        // Notes on cells that hold nothing still show their mark.
        for at in notes {
            if rows.contains(&at.row)
                && cols.contains(&at.col)
                && !out.iter().any(|(r, c, _)| (*r, *c) == (at.row, at.col))
            {
                out.push((
                    at.row,
                    at.col,
                    GridCell {
                        note: true,
                        ..GridCell::default()
                    },
                ));
            }
        }
        self.sparklines(unit, &rows, &cols, &mut out);
        for t in self.book().threads(unit) {
            let at = t.cell;
            if !rows.contains(&at.row) || !cols.contains(&at.col) {
                continue;
            }
            match out
                .iter_mut()
                .find(|(r, c, _)| (*r, *c) == (at.row, at.col))
            {
                Some((_, _, cell)) => cell.thread = true,
                None => out.push((
                    at.row,
                    at.col,
                    GridCell {
                        thread: true,
                        ..GridCell::default()
                    },
                )),
            }
        }
        out
    }

    fn calc_options(&mut self) -> kalem_viewer::CalcOptions {
        self.book().calc_options()
    }

    fn set_calc_options(&mut self, options: kalem_viewer::CalcOptions) -> Result<Vec<usize>> {
        self.book().set_calc_options(options).map_err(err)?;
        Ok(self.all_units())
    }

    fn circular_references(&mut self) -> Vec<(usize, u32, u32)> {
        self.book()
            .circular_references()
            .into_iter()
            .map(|(u, at)| (u, at.row, at.col))
            .collect()
    }

    fn sheet_view(&mut self, unit: usize) -> kalem_viewer::SheetView {
        let raw = self.book().view_raw(unit);
        let split = raw.split.and_then(|sp| {
            let l = self.grid(unit)?;
            let (rows, cols) = split_cells(&l, &sp, raw.headings);
            (rows > 0 || cols > 0).then_some([rows, cols, sp.top_left.row, sp.top_left.col])
        });
        kalem_viewer::SheetView {
            zoom: raw.zoom,
            gridlines: raw.gridlines,
            headings: raw.headings,
            page_break_preview: raw.preview,
            split,
        }
    }

    fn set_sheet_view(&mut self, unit: usize, view: kalem_viewer::SheetView) -> Result<Vec<usize>> {
        if !(10..=400).contains(&view.zoom) {
            return Err(ViewerError("The zoom is 10% to 400%".into()));
        }
        let split = match view.split {
            Some([rows, cols, top, left]) if rows > 0 || cols > 0 => {
                let l = self
                    .grid(unit)
                    .ok_or_else(|| ViewerError("Not a sheet".into()))?;
                let (y, x) = split_twips(&l, [rows, cols, top, left], view.headings);
                Some(crate::workbook::SplitRaw {
                    y,
                    x,
                    top_left: CellRef::new(top, left),
                    pane_top_left: CellRef::new(top + rows, left + cols),
                })
            }
            _ => None,
        };
        let raw = crate::workbook::ViewRaw {
            zoom: view.zoom,
            gridlines: view.gridlines,
            headings: view.headings,
            preview: view.page_break_preview,
            split,
        };
        if self.book().set_view_raw(unit, raw).map_err(err)? {
            self.view_changed = true;
        }
        Ok(vec![unit])
    }

    fn cell_styles(&mut self) -> Vec<String> {
        self.book().cell_styles()
    }

    fn apply_cell_style(&mut self, unit: usize, range: [u32; 4], name: &str) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book().apply_cell_style(unit, r, name).map_err(err)?;
        Ok(vec![unit])
    }

    fn new_cell_style(
        &mut self,
        name: &str,
        unit: usize,
        row: u32,
        col: u32,
    ) -> Result<Vec<usize>> {
        self.book()
            .new_cell_style(name, unit, CellRef::new(row, col))
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn theme_name(&mut self) -> Option<String> {
        self.book().theme_name()
    }

    fn theme_names(&mut self) -> Vec<String> {
        crate::Workbook::theme_names()
    }

    fn set_theme(&mut self, name: &str) -> Result<Vec<usize>> {
        self.book().set_theme(name).map_err(err)?;
        Ok(self.all_units())
    }

    fn tab_color(&mut self, unit: usize) -> Option<[u8; 3]> {
        self.book().tab_color(unit).map(rgb)
    }

    fn set_tab_color(&mut self, unit: usize, color: Option<[u8; 3]>) -> Result<Vec<usize>> {
        self.book().set_tab_color(unit, color).map_err(err)?;
        Ok(self.all_units())
    }

    fn threads(&mut self, unit: usize) -> Vec<kalem_viewer::CommentThread> {
        self.book()
            .threads(unit)
            .into_iter()
            .map(|t| kalem_viewer::CommentThread {
                row: t.cell.row,
                col: t.cell.col,
                done: t.done,
                comments: t
                    .comments
                    .into_iter()
                    .map(|c| kalem_viewer::ThreadComment {
                        author: c.author,
                        text: c.text,
                        time: c.time,
                    })
                    .collect(),
            })
            .collect()
    }

    fn add_thread_comment(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        author: &str,
        text: &str,
        time: &str,
    ) -> Result<Vec<usize>> {
        self.book()
            .add_thread_comment(unit, CellRef::new(row, col), author, text, time)
            .map_err(err)?;
        self.notes.remove(&unit);
        Ok(vec![unit])
    }

    fn resolve_thread(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        done: bool,
    ) -> Result<Vec<usize>> {
        self.book()
            .resolve_thread(unit, CellRef::new(row, col), done)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn delete_thread_comment(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        index: usize,
    ) -> Result<Vec<usize>> {
        self.book()
            .delete_thread_comment(unit, CellRef::new(row, col), index)
            .map_err(err)?;
        self.notes.remove(&unit);
        Ok(vec![unit])
    }

    fn add_sparklines(
        &mut self,
        unit: usize,
        data: [u32; 4],
        location: [u32; 4],
        kind: kalem_viewer::SparklineKind,
        mark: bool,
    ) -> Result<Vec<usize>> {
        let range = |a: [u32; 4]| crate::Range {
            start: CellRef::new(a[0], a[1]),
            end: CellRef::new(a[2], a[3]),
        };
        self.book()
            .add_sparklines(unit, range(data), range(location), kind, mark)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn clear_sparklines(&mut self, unit: usize, range: [u32; 4]) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book().clear_sparklines(unit, r).map_err(err)?;
        Ok(vec![unit])
    }

    fn goal_seek(
        &mut self,
        unit: usize,
        set: (u32, u32),
        target: f64,
        by: (u32, u32),
    ) -> Result<Option<f64>> {
        self.book()
            .goal_seek(
                unit,
                CellRef::new(set.0, set.1),
                target,
                CellRef::new(by.0, by.1),
            )
            .map_err(err)
    }

    fn create_data_table(
        &mut self,
        unit: usize,
        range: [u32; 4],
        row_input: Option<(u32, u32)>,
        col_input: Option<(u32, u32)>,
    ) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        let cell = |c: Option<(u32, u32)>| c.map(|(r, c)| CellRef::new(r, c));
        self.book()
            .create_data_table(unit, r, cell(row_input), cell(col_input))
            .map_err(err)?;
        Ok(self.all_units())
    }

    fn scenarios(&mut self, unit: usize) -> Vec<kalem_viewer::Scenario> {
        self.book()
            .scenarios(unit)
            .into_iter()
            .map(|s| kalem_viewer::Scenario {
                name: s.name,
                comment: s.comment,
                cells: s
                    .cells
                    .into_iter()
                    .map(|(at, v)| (at.row, at.col, v))
                    .collect(),
            })
            .collect()
    }

    fn add_scenario(
        &mut self,
        unit: usize,
        name: &str,
        cells: &[(u32, u32)],
        comment: &str,
    ) -> Result<Vec<usize>> {
        let cells: Vec<CellRef> = cells.iter().map(|&(r, c)| CellRef::new(r, c)).collect();
        self.book()
            .add_scenario(unit, name, &cells, comment)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn show_scenario(&mut self, unit: usize, name: &str) -> Result<Vec<usize>> {
        self.book().show_scenario(unit, name).map_err(err)?;
        Ok(self.all_units())
    }

    fn delete_scenario(&mut self, unit: usize, name: &str) -> Result<Vec<usize>> {
        self.book().delete_scenario(unit, name).map_err(err)?;
        Ok(vec![unit])
    }

    fn cell_input(&mut self, unit: usize, row: u32, col: u32) -> String {
        self.book()
            .edit_text(unit, CellRef::new(row, col))
            .unwrap_or_default()
    }

    fn paste_cells(
        &mut self,
        from: (usize, [u32; 4]),
        to: (usize, u32, u32),
        kind: kalem_viewer::PasteKind,
        transpose: bool,
    ) -> Result<Vec<usize>> {
        let r = from.1;
        let src = crate::Range {
            start: CellRef::new(r[0], r[1]),
            end: CellRef::new(r[2], r[3]),
        };
        self.book()
            .paste_cells(
                (from.0, src),
                (to.0, CellRef::new(to.1, to.2)),
                kind,
                transpose,
            )
            .map_err(err)?;
        Ok(self.all_units())
    }

    fn clear_range(
        &mut self,
        unit: usize,
        range: [u32; 4],
        contents: bool,
        formats: bool,
    ) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .clear_parts(unit, r, contents, formats)
            .map_err(err)?;
        self.notes.remove(&unit);
        Ok(self.all_units())
    }

    fn fill_formats(
        &mut self,
        from: (usize, [u32; 4]),
        to: (usize, [u32; 4]),
    ) -> Result<Vec<usize>> {
        let range = |r: [u32; 4]| crate::Range {
            start: CellRef::new(r[0], r[1]),
            end: CellRef::new(r[2], r[3]),
        };
        self.book()
            .fill_formats((from.0, range(from.1)), (to.0, range(to.1)))
            .map_err(err)?;
        Ok(vec![to.0])
    }

    fn remove_duplicates(
        &mut self,
        unit: usize,
        range: [u32; 4],
        columns: &[u32],
        header: bool,
    ) -> Result<usize> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .remove_duplicates(unit, r, columns, header)
            .map_err(err)
    }

    fn formula_functions(&mut self) -> Vec<(String, String)> {
        crate::functions::list()
    }

    fn cell_link(&mut self, unit: usize, row: u32, col: u32) -> Option<String> {
        self.book()
            .link(unit, CellRef::new(row, col))
            .ok()
            .flatten()
    }

    fn set_link(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        target: Option<String>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_link(unit, CellRef::new(row, col), target.as_deref())
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn defined_names(&mut self) -> Vec<(String, String)> {
        self.book()
            .defined_names()
            .iter()
            .filter(|d| !d.hidden && !d.name.starts_with("_xlnm."))
            .map(|d| (d.name.clone(), d.refers_to.clone()))
            .collect()
    }

    fn set_defined_name(&mut self, name: &str, refers_to: Option<&str>) -> Result<Vec<usize>> {
        self.book().set_defined_name(name, refers_to).map_err(err)?;
        Ok(self.all_units())
    }

    fn recalculate(&mut self) -> Result<Vec<usize>> {
        self.book().recalculate().map_err(err)?;
        Ok(self.all_units())
    }

    fn set_note(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        text: Option<String>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_comment(unit, CellRef::new(row, col), text.as_deref())
            .map_err(err)?;
        self.notes.remove(&unit);
        Ok(vec![unit])
    }

    fn set_frozen(&mut self, unit: usize, rows: u32, cols: u32) -> Result<Vec<usize>> {
        self.book().set_frozen(unit, rows, cols).map_err(err)?;
        Ok(vec![unit])
    }

    fn set_hidden(
        &mut self,
        unit: usize,
        rows: bool,
        from: u32,
        to: u32,
        hidden: bool,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_hidden(unit, rows, from, to, hidden)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn edit_sheets(&mut self, edit: kalem_viewer::SheetEdit) -> Result<usize> {
        let shown = self.book().edit_sheets(&edit).map_err(err)?;
        // What was kept by sheet number.
        self.notes.clear();
        self.cf.clear();
        self.charts.clear();
        Ok(shown)
    }

    fn hidden_units(&mut self) -> Vec<usize> {
        self.book().hidden_sheets()
    }

    fn range_numbers(&mut self, unit: usize, range: [u32; 4]) -> (Vec<f64>, usize) {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book().range_summary(unit, r).unwrap_or_default()
    }

    fn cell_format(&mut self, unit: usize, row: u32, col: u32) -> Option<String> {
        let style = self
            .book()
            .sheet(unit)
            .ok()?
            .cells
            .get(&CellRef::new(row, col))
            .map_or(0, |c| c.style);
        Some(self.book().style(style).num_fmt.clone())
    }

    fn cell_note(&mut self, unit: usize, row: u32, col: u32) -> Option<String> {
        self.notes(unit).get(&CellRef::new(row, col)).cloned()
    }

    fn set_cell(&mut self, unit: usize, row: u32, col: u32, input: &str) -> Result<Vec<usize>> {
        self.book()
            .set_cell(unit, CellRef::new(row, col), input)
            .map_err(err)?;
        Ok(self.all_units())
    }

    fn enter_in_range(
        &mut self,
        unit: usize,
        range: [u32; 4],
        at: (u32, u32),
        input: &str,
    ) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .enter_in_range(unit, r, CellRef::new(at.0, at.1), input)
            .map_err(err)?;
        Ok(self.all_units())
    }

    fn grid_edit(&mut self, unit: usize, edit: GridEdit) -> Result<Vec<usize>> {
        match edit {
            GridEdit::InsertRows { at, count } => self.book().insert_rows(unit, at, count),
            GridEdit::DeleteRows { at, count } => self.book().delete_rows(unit, at, count),
            GridEdit::InsertCols { at, count } => self.book().insert_cols(unit, at, count),
            GridEdit::DeleteCols { at, count } => self.book().delete_cols(unit, at, count),
        }
        .map_err(err)?;
        self.notes.clear();
        Ok(self.all_units())
    }

    fn set_cells(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        values: &[Vec<String>],
    ) -> Result<Vec<usize>> {
        self.book()
            .set_cells(unit, CellRef::new(row, col), values)
            .map_err(err)?;
        Ok(self.all_units())
    }

    fn set_cell_list(&mut self, unit: usize, cells: &[(u32, u32, String)]) -> Result<Vec<usize>> {
        let cells: Vec<(CellRef, String)> = cells
            .iter()
            .map(|(r, c, v)| (CellRef::new(*r, *c), v.clone()))
            .collect();
        self.book().set_cell_list(unit, &cells).map_err(err)?;
        Ok(self.all_units())
    }

    fn move_cells(
        &mut self,
        unit: usize,
        range: [u32; 4],
        row: u32,
        col: u32,
    ) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .move_range(unit, r, CellRef::new(row, col))
            .map_err(err)?;
        self.notes.clear();
        Ok(self.all_units())
    }

    fn set_fill_lists(&mut self, lists: Vec<Vec<String>>) {
        self.fill_lists = lists;
    }

    fn fill(
        &mut self,
        unit: usize,
        source: [u32; 4],
        target: [u32; 4],
        series: bool,
    ) -> Result<Vec<usize>> {
        let r = |a: [u32; 4]| crate::cellref::Range {
            start: CellRef::new(a[0], a[1]),
            end: CellRef::new(a[2], a[3]),
        };
        let lists = self.fill_lists.clone();
        self.book()
            .fill(unit, r(source), r(target), series, &lists)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn sort_range(
        &mut self,
        unit: usize,
        range: [u32; 4],
        key: u32,
        descending: bool,
        header: bool,
    ) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .sort_range(unit, r, key, descending, header)
            .map_err(err)?;
        self.notes.clear();
        Ok(self.all_units())
    }

    fn set_filter(&mut self, unit: usize, range: Option<[u32; 4]>) -> Result<Vec<usize>> {
        let r = range.map(|r| crate::cellref::Range {
            start: CellRef::new(r[0], r[1]),
            end: CellRef::new(r[2], r[3]),
        });
        self.book().set_filter(unit, r).map_err(err)?;
        Ok(vec![unit])
    }

    fn filter_column(
        &mut self,
        unit: usize,
        col: u32,
        values: Option<Vec<String>>,
    ) -> Result<Vec<usize>> {
        self.book().filter_column(unit, col, values).map_err(err)?;
        Ok(vec![unit])
    }

    fn sort_range_by(
        &mut self,
        unit: usize,
        range: [u32; 4],
        keys: &[kalem_viewer::SortKey],
        header: bool,
    ) -> Result<Vec<usize>> {
        let r = crate::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .sort_range_keys(unit, r, keys, header)
            .map_err(err)?;
        Ok(self.all_units())
    }

    fn filter_column_by(
        &mut self,
        unit: usize,
        col: u32,
        rule: Option<kalem_viewer::FilterRule>,
    ) -> Result<Vec<usize>> {
        self.book()
            .filter_column_rule(unit, col, rule)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn column_filter(&mut self, unit: usize, col: u32) -> Option<kalem_viewer::FilterRule> {
        self.book().column_filter(unit, col)
    }

    fn reapply_filter(&mut self, unit: usize) -> Result<Vec<usize>> {
        self.book().reapply_filter(unit).map_err(err)?;
        Ok(vec![unit])
    }

    fn move_cells_between(
        &mut self,
        from: usize,
        range: [u32; 4],
        to: usize,
        row: u32,
        col: u32,
    ) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .move_range_to(from, r, to, CellRef::new(row, col))
            .map_err(err)?;
        self.notes.clear();
        Ok(self.all_units())
    }

    fn clear_cells(&mut self, unit: usize, range: [u32; 4]) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book().clear_range(unit, r).map_err(err)?;
        Ok(self.all_units())
    }

    fn merge_cells(&mut self, unit: usize, range: [u32; 4], center: bool) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book().merge_cells(unit, r, center).map_err(err)?;
        Ok(self.all_units())
    }

    fn unmerge_cells(&mut self, unit: usize, row: u32, col: u32) -> Result<Vec<usize>> {
        self.book()
            .unmerge_cells(unit, CellRef::new(row, col))
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn charts(&mut self, unit: usize) -> Vec<Chart> {
        if !self.worksheet(unit) && !self.chartsheet(unit) {
            return Vec::new();
        }
        let generation = self.book().generation();
        if let Some((g, c)) = self.charts.get(&unit)
            && *g == generation
        {
            return c.clone();
        }
        let c = self.book().charts(unit).unwrap_or_default();
        self.charts.insert(unit, (generation, c.clone()));
        c
    }

    fn insert_chart(
        &mut self,
        unit: usize,
        range: [u32; 4],
        kind: ChartKind,
        title: Option<String>,
    ) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .insert_chart(unit, r, kind, title.as_deref())
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn move_chart(&mut self, unit: usize, index: usize, anchor: [u32; 4]) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(anchor[0].min(anchor[2]), anchor[1].min(anchor[3])),
            end: CellRef::new(anchor[0].max(anchor[2]), anchor[1].max(anchor[3])),
        };
        self.book().move_chart(unit, index, r).map_err(err)?;
        Ok(vec![unit])
    }

    fn set_chart_title(
        &mut self,
        unit: usize,
        index: usize,
        title: Option<String>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_chart_title(unit, index, title.as_deref())
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_axis_title(
        &mut self,
        unit: usize,
        index: usize,
        axis: ChartAxis,
        title: Option<String>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_axis_title(unit, index, axis == ChartAxis::Vertical, title.as_deref())
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_legend(
        &mut self,
        unit: usize,
        index: usize,
        position: Option<LegendPosition>,
    ) -> Result<Vec<usize>> {
        self.book().set_legend(unit, index, position).map_err(err)?;
        Ok(vec![unit])
    }

    fn set_data_labels(
        &mut self,
        unit: usize,
        index: usize,
        labels: DataLabels,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_data_labels(unit, index, labels)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_axis_scale(
        &mut self,
        unit: usize,
        index: usize,
        scale: AxisScale,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_axis_scale(unit, index, scale)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_chart_kind(&mut self, unit: usize, index: usize, kind: ChartKind) -> Result<Vec<usize>> {
        self.book().set_chart_kind(unit, index, kind).map_err(err)?;
        Ok(vec![unit])
    }

    fn set_series_color(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        color: Option<[u8; 3]>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_series_color(unit, index, series, color)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_point_color(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        point: usize,
        color: Option<[u8; 3]>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_point_color(unit, index, series, point, color)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_explosion(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        point: Option<usize>,
        percent: u32,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_explosion(unit, index, series, point, percent)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_chart_area(
        &mut self,
        unit: usize,
        index: usize,
        background: Paint,
        border: Paint,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_chart_area(unit, index, background, border)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_plot_area(
        &mut self,
        unit: usize,
        index: usize,
        background: Paint,
        border: Paint,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_plot_area(unit, index, background, border)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_gridlines(&mut self, unit: usize, index: usize, lines: Gridlines) -> Result<Vec<usize>> {
        self.book().set_gridlines(unit, index, lines).map_err(err)?;
        Ok(vec![unit])
    }

    fn set_axis_format(
        &mut self,
        unit: usize,
        index: usize,
        format: Option<String>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_axis_format(unit, index, format.as_deref())
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_axis_font(
        &mut self,
        unit: usize,
        index: usize,
        axis: ChartAxis,
        font: AxisFont,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_axis_font(unit, index, axis == ChartAxis::Vertical, &font)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_title_font(&mut self, unit: usize, index: usize, font: AxisFont) -> Result<Vec<usize>> {
        self.book()
            .set_title_font(unit, index, &font)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_legend_font(&mut self, unit: usize, index: usize, font: AxisFont) -> Result<Vec<usize>> {
        self.book()
            .set_legend_font(unit, index, &font)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn delete_chart(&mut self, unit: usize, index: usize) -> Result<Vec<usize>> {
        self.book().delete_chart(unit, index).map_err(err)?;
        Ok(vec![unit])
    }

    fn set_series_kind(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        kind: Option<ChartKind>,
        secondary: bool,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_series_kind(unit, index, series, kind, secondary)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_trendline(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        trendline: Option<kalem_viewer::Trendline>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_trendline(unit, index, series, trendline)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_error_bars(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        bars: Option<kalem_viewer::ErrorBars>,
    ) -> Result<Vec<usize>> {
        self.book()
            .set_error_bars(unit, index, series, bars)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_label_cells(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        range: Option<[u32; 4]>,
    ) -> Result<Vec<usize>> {
        let range = range.map(|r| crate::cellref::Range {
            start: CellRef::new(r[0], r[1]),
            end: CellRef::new(r[2], r[3]),
        });
        self.book()
            .set_label_cells(unit, index, series, range)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn move_chart_to_sheet(&mut self, unit: usize, index: usize, name: &str) -> Result<usize> {
        self.book()
            .move_chart_to_sheet(unit, index, name)
            .map_err(err)
    }

    fn move_chart_to_grid(
        &mut self,
        unit: usize,
        target: usize,
        anchor: [u32; 4],
    ) -> Result<usize> {
        let anchor = crate::cellref::Range {
            start: CellRef::new(anchor[0], anchor[1]),
            end: CellRef::new(anchor[2], anchor[3]),
        };
        self.book()
            .move_chart_to_grid(unit, target, anchor)
            .map_err(err)
    }

    fn chart_template(&mut self, unit: usize, index: usize) -> Result<Vec<u8>> {
        self.book().chart_template(unit, index).map_err(err)
    }

    fn apply_chart_template(
        &mut self,
        unit: usize,
        index: usize,
        template: &[u8],
    ) -> Result<Vec<usize>> {
        self.book()
            .apply_chart_template(unit, index, template)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn insert_pivot(&mut self, unit: usize, spec: PivotSpec) -> Result<usize> {
        let r = spec.range;
        let range = crate::cellref::Range {
            start: CellRef::new(r[0], r[1]),
            end: CellRef::new(r[2], r[3]),
        };
        let layout = crate::pivot::layout_from_spec(&spec);
        self.book().insert_pivot(unit, range, &layout).map_err(err)
    }

    fn pivots(&mut self, unit: usize) -> Vec<kalem_viewer::PivotInfo> {
        if !self.worksheet(unit) {
            return Vec::new();
        }
        self.book().pivots(unit)
    }

    fn set_pivot(&mut self, unit: usize, index: usize, spec: PivotSpec) -> Result<Vec<usize>> {
        self.book().set_pivot(unit, index, &spec).map_err(err)?;
        Ok(vec![unit])
    }

    fn insert_pivot_chart(
        &mut self,
        unit: usize,
        index: usize,
        kind: ChartKind,
    ) -> Result<Vec<usize>> {
        self.book()
            .insert_pivot_chart(unit, index, kind)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn slicers(&mut self, unit: usize) -> Vec<kalem_viewer::Slicer> {
        if !self.worksheet(unit) {
            return Vec::new();
        }
        self.book().slicers(unit)
    }

    fn insert_slicer(
        &mut self,
        unit: usize,
        pivot: Option<usize>,
        table: Option<&str>,
        field: &str,
        anchor: [u32; 4],
    ) -> Result<Vec<usize>> {
        let anchor = crate::cellref::Range {
            start: CellRef::new(anchor[0], anchor[1]),
            end: CellRef::new(anchor[2], anchor[3]),
        };
        let name = pivot.and_then(|i| self.book().pivots(unit).get(i).map(|p| p.name.clone()));
        self.book()
            .insert_slicer(unit, name.as_deref(), table, field, anchor)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn select_slicer(
        &mut self,
        unit: usize,
        index: usize,
        selected: &[String],
    ) -> Result<Vec<usize>> {
        self.book()
            .select_slicer(unit, index, selected)
            .map_err(err)?;
        Ok((0..self.book().sheets().len()).collect())
    }

    fn delete_slicer(&mut self, unit: usize, index: usize) -> Result<Vec<usize>> {
        self.book().delete_slicer(unit, index).map_err(err)?;
        Ok((0..self.book().sheets().len()).collect())
    }

    fn refresh_pivots(&mut self) -> Result<Vec<usize>> {
        self.book().refresh_pivots().map_err(err)
    }

    fn validation(&mut self, unit: usize, row: u32, col: u32) -> Option<Validation> {
        let at = CellRef::new(row, col);
        let dv = self.book().validation_at(unit, at).ok()??;
        let kind = match dv.kind.as_str() {
            "whole" => ValidationKind::Whole,
            "decimal" => ValidationKind::Decimal,
            "list" => ValidationKind::List,
            "date" => ValidationKind::Date,
            "time" => ValidationKind::Time,
            "textLength" => ValidationKind::TextLength,
            "custom" => ValidationKind::Custom,
            _ => ValidationKind::Any,
        };
        let op = match dv.operator.as_str() {
            "greaterThan" => CompareOp::Greater,
            "lessThan" => CompareOp::Less,
            "greaterThanOrEqual" => CompareOp::GreaterOrEqual,
            "lessThanOrEqual" => CompareOp::LessOrEqual,
            "equal" => CompareOp::Equal,
            "notEqual" => CompareOp::NotEqual,
            "notBetween" => CompareOp::NotBetween,
            _ => CompareOp::Between,
        };
        // A value as typed: a number as it is, anything else a formula.
        let typed = |f: &str| {
            if f.trim().parse::<f64>().is_ok() {
                f.trim().to_owned()
            } else {
                format!("={f}")
            }
        };
        let value = match (&dv.formula1, kind) {
            (Some(f), ValidationKind::List) => {
                crate::validation::literal_list(f).map_or_else(|| format!("={f}"), |l| l.join(","))
            }
            (Some(f), ValidationKind::Custom) => format!("={f}"),
            (Some(f), _) => typed(f),
            (None, _) => String::new(),
        };
        let list = if kind == ValidationKind::List {
            self.book().list_values(unit, &dv, at)
        } else {
            Vec::new()
        };
        let style = match dv.error_style.as_str() {
            "warning" => ErrorStyle::Warning,
            "information" => ErrorStyle::Information,
            _ => ErrorStyle::Stop,
        };
        Some(Validation {
            kind,
            op,
            value,
            value2: dv.formula2.as_deref().map(typed),
            allow_blank: dv.allow_blank,
            dropdown: dv.dropdown,
            prompt: (dv.show_input && !(dv.prompt.is_empty() && dv.prompt_title.is_empty()))
                .then(|| (dv.prompt_title.clone(), dv.prompt.clone())),
            error: dv
                .show_error
                .then(|| (style, dv.error_title.clone(), dv.error.clone())),
            list,
        })
    }

    fn set_validation(
        &mut self,
        unit: usize,
        range: [u32; 4],
        validation: Option<Validation>,
    ) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .set_validation(unit, r, validation.as_ref())
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn check_input(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        input: &str,
    ) -> Option<ValidationError> {
        let dv = self
            .book()
            .check_entry(unit, CellRef::new(row, col), input)
            .ok()??;
        let style = match dv.error_style.as_str() {
            "warning" => ErrorStyle::Warning,
            "information" => ErrorStyle::Information,
            _ => ErrorStyle::Stop,
        };
        Some(ValidationError {
            style,
            title: dv.error_title,
            message: if dv.error.is_empty() {
                "This value doesn't match the data validation restrictions defined for this cell."
                    .into()
            } else {
                dv.error
            },
        })
    }

    fn invalid_cells(
        &mut self,
        unit: usize,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32)> {
        if !self.worksheet(unit) {
            return Vec::new();
        }
        let Ok(dvs) = self.book().validations(unit) else {
            return Vec::new();
        };
        let dvs: Vec<_> = dvs.into_iter().filter(|d| d.kind != "none").collect();
        if dvs.is_empty() {
            return Vec::new();
        }
        let positions: Vec<CellRef> = match self.book().sheet(unit) {
            Ok(s) => s
                .cells
                .range(CellRef::new(rows.start, 0)..CellRef::new(rows.end, 0))
                .map(|(p, _)| *p)
                .filter(|p| cols.contains(&p.col))
                .collect(),
            Err(_) => return Vec::new(),
        };
        let pairs: Vec<(CellRef, &crate::validation::DataValidation)> = positions
            .iter()
            .filter_map(|&at| dvs.iter().find(|d| d.covers(at)).map(|d| (at, d)))
            .collect();
        // Their formulas computed together, then each cell judged.
        self.book().prefetch_validations(unit, &pairs);
        let mut out = Vec::new();
        for (at, dv) in pairs {
            if !self.book().accepts(unit, at, dv).unwrap_or(true) {
                out.push((at.row, at.col));
            }
        }
        out
    }

    fn add_conditional_format(
        &mut self,
        unit: usize,
        range: [u32; 4],
        rule: CondRule,
        style: CondStyle,
    ) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0], range[1]),
            end: CellRef::new(range[2], range[3]),
        };
        self.book()
            .add_conditional_format(unit, r, &rule, &style)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn clear_conditional_formats(
        &mut self,
        unit: usize,
        range: Option<[u32; 4]>,
    ) -> Result<Vec<usize>> {
        let r = range.map(|r| crate::cellref::Range {
            start: CellRef::new(r[0], r[1]),
            end: CellRef::new(r[2], r[3]),
        });
        let any = self
            .book()
            .clear_conditional_formats(unit, r)
            .map_err(err)?;
        Ok(if any { vec![unit] } else { Vec::new() })
    }

    fn change_style(
        &mut self,
        unit: usize,
        range: [u32; 4],
        change: StyleChange,
    ) -> Result<Vec<usize>> {
        let r = crate::cellref::Range {
            start: CellRef::new(range[0].min(range[2]), range[1].min(range[3])),
            end: CellRef::new(range[0].max(range[2]), range[1].max(range[3])),
        };
        self.book().change_style(unit, r, &change).map_err(err)?;
        Ok(vec![unit])
    }

    fn set_wrap(&mut self, unit: usize, row: u32, col: u32, wrap: bool) -> Result<Vec<usize>> {
        self.book()
            .set_wrap(unit, CellRef::new(row, col), wrap)
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_row_height(&mut self, unit: usize, row: u32, height: f32) -> Result<Vec<usize>> {
        self.book()
            .set_row_height(unit, row, f64::from(height))
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn set_col_width(&mut self, unit: usize, col: u32, width: f32) -> Result<Vec<usize>> {
        self.book()
            .set_col_width(unit, col, f64::from(width))
            .map_err(err)?;
        Ok(vec![unit])
    }

    fn has_history(&self) -> bool {
        true
    }

    fn undo(&mut self) -> Result<bool> {
        self.notes.clear();
        Ok(self.book().undo())
    }

    fn redo(&mut self) -> Result<bool> {
        self.notes.clear();
        Ok(self.book().redo())
    }

    fn modified(&self) -> bool {
        let wb = self.locked();
        self.view_changed
            || wb.history_len() != self.saved_at
            || (self.saved_at == 0 && wb.is_dirty())
    }

    fn save(&mut self) -> Result<SaveOutput> {
        let bytes = self.book().save().map_err(err)?;
        self.saved_at = self.book().history_len();
        self.view_changed = false;
        Ok(SaveOutput {
            bytes,
            losses: Vec::new(),
        })
    }

    fn macros(&mut self) -> Vec<MacroEntry> {
        let Ok(Some(project)) = self.book().vba_project() else {
            return Vec::new();
        };
        crate::macros::list_macros(&project)
            .map(|l| {
                l.into_iter()
                    .map(|m| MacroEntry {
                        name: m.qualified,
                        event: m.event,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn run_macro(&mut self, name: &str, ui: &mut dyn MacroUi) -> Result<MacroOutcome> {
        let project = self
            .book()
            .vba_project()
            .map_err(err)?
            .ok_or_else(|| ViewerError("The workbook has no macros".into()))?;
        let mut host = Ui {
            ui,
            name: self.name.clone(),
            question: None,
        };
        let result = crate::macros::run_macro(
            self.book(),
            &project,
            name,
            &mut host,
            crate::macros::Limits::default(),
        );
        let question = host.question.take();
        self.notes.clear();
        let (report, error) = match result {
            Ok(r) => (r, None),
            Err(e) => (*e.report.clone(), Some(e.to_string())),
        };
        if question.is_some() && report.changed {
            // Taken back: the run is made again with the answer.
            self.book().rollback();
        }
        let mut output = report.messages.clone();
        output.extend(report.output);
        Ok(MacroOutcome {
            output,
            skipped: report.skipped,
            error,
            changed: report.changed && question.is_none(),
            question,
        })
    }
}

/// An `.xls`, `.xlsb` or `.ods` workbook: shown, never edited.
#[cfg(not(target_family = "wasm"))]
struct LegacyDoc {
    wb: crate::legacy::LegacyWorkbook,
}

#[cfg(not(target_family = "wasm"))]
impl ViewerDocument for LegacyDoc {
    fn structure(&self) -> Structure {
        units(
            self.wb
                .sheets
                .iter()
                .map(|s| (s.name.clone(), s.hidden, s.worksheet)),
        )
    }

    fn render(&mut self, _unit: usize, _request: RenderRequest) -> Result<Rendered> {
        Ok(blank())
    }

    fn text(&self, unit: usize) -> String {
        tsv(self.wb.grid(unit))
    }

    fn info(&self) -> Vec<InfoField> {
        vec![
            InfoField::new(
                "Format",
                format!(
                    "{:?}, shown only (never written or converted)",
                    self.wb.format
                ),
            ),
            InfoField::new("Sheets", self.wb.sheets.len().to_string()),
        ]
    }

    fn grid(&mut self, unit: usize) -> Option<GridLayout> {
        let s = self.wb.sheets.get(unit).filter(|s| s.worksheet)?;
        let used = s.used_range();
        // As far as the cells' colors go, too.
        let (paint_rows, paint_cols) = self
            .wb
            .looks
            .as_ref()
            .map_or((0, 0), |l| l.extent(unit, (10_000, 1_024)));
        Some(GridLayout {
            rows: used.map_or(0, |u| u.end.row + 1).max(paint_rows),
            cols: used.map_or(0, |u| u.end.col + 1).max(paint_cols),
            max_rows: MAX_ROW,
            max_cols: MAX_COL,
            default_width: 8.43,
            editable: false,
            ..GridLayout::default()
        })
    }

    fn grid_cells(
        &mut self,
        unit: usize,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32, GridCell)> {
        let Some(s) = self.wb.sheets.get(unit) else {
            return Vec::new();
        };
        let looks = self.wb.looks.as_ref();
        let look = |r: u32, c: u32| looks.map(|l| l.at(unit, r, c)).unwrap_or_default();
        let mut out: Vec<(u32, u32, GridCell)> = s
            .cells
            .range(CellRef::new(rows.start, 0)..CellRef::new(rows.end, 0))
            .filter(|(p, _)| cols.contains(&p.col))
            .map(|(p, c)| {
                let l = look(p.row, p.col);
                (
                    p.row,
                    p.col,
                    GridCell {
                        text: self.wb.display(unit, *p),
                        numeric: matches!(c.value, Value::Number(_)),
                        formula: c.formula.is_some(),
                        fill: l.fill,
                        color: l.color,
                        bold: l.bold,
                        italic: l.italic,
                        underline: l.underline,
                        ..GridCell::default()
                    },
                )
            })
            .collect();
        // Empty cells their style paints.
        if looks.is_some() {
            let have: std::collections::HashSet<(u32, u32)> =
                out.iter().map(|c| (c.0, c.1)).collect();
            for r in rows.clone() {
                for c in cols.clone() {
                    if have.contains(&(r, c)) {
                        continue;
                    }
                    let l = look(r, c);
                    if l.paints() {
                        out.push((
                            r,
                            c,
                            GridCell {
                                fill: l.fill,
                                ..GridCell::default()
                            },
                        ));
                    }
                }
            }
        }
        out
    }

    fn cell_input(&mut self, unit: usize, row: u32, col: u32) -> String {
        self.wb.edit_text(unit, CellRef::new(row, col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kalem_viewer::Aggregate;

    struct Yes;

    impl MacroUi for Yes {
        fn message(&mut self, _: &str, _: i64, _: &str) -> Option<i64> {
            Some(6)
        }
        fn input(&mut self, _: &str, _: &str, d: &str) -> Option<Option<String>> {
            Some(Some(d.to_owned()))
        }
    }

    /// Answers only plain messages: every question stops the run.
    struct Later;

    impl MacroUi for Later {
        fn message(&mut self, _: &str, buttons: i64, _: &str) -> Option<i64> {
            (buttons & 7 == 0).then_some(1)
        }
        fn input(&mut self, _: &str, _: &str, _: &str) -> Option<Option<String>> {
            None
        }
    }

    fn open(name: &str) -> Box<dyn ViewerDocument> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/corpus")
            .join(name);
        let head = std::fs::read(&p).unwrap();
        assert_eq!(
            XlsxViewer.detect(name, &head[..head.len().min(8192)]),
            Detection::Magic,
            "{name}"
        );
        XlsxViewer.open(FileHandle::new(p)).unwrap()
    }

    #[test]
    fn a_workbook_as_grid_units() {
        let mut d = open("libreoffice-budget.xlsx");
        let s = d.structure();
        assert_eq!(s.units.len(), 3);
        assert_eq!(s.units[2].label, "Hidden (hidden)");
        let g = d.grid(0).unwrap();
        assert_eq!((g.rows, g.cols), (5, 7));
        assert_eq!(g.widths[0], 18.0);
        assert_eq!(g.merged, vec![[0, 5, 0, 6]]);
        let cells = d.grid_cells(0, 0..3, 0..4);
        let a1 = cells.iter().find(|(r, c, _)| (*r, *c) == (0, 0)).unwrap();
        assert!(a1.2.bold && a1.2.fill == Some([0x44, 0x72, 0xC4]));
        let d2 = cells.iter().find(|(r, c, _)| (*r, *c) == (1, 3)).unwrap();
        assert!(d2.2.formula && d2.2.numeric && d2.2.text == "2,400.00");
        assert!(cells.iter().any(|(r, c, x)| (*r, *c) == (1, 0) && x.note));
        assert_eq!(d.cell_input(0, 1, 3), "=B2+C2");
        assert!(d.text(0).starts_with("Item\tQ1\tQ2\tTotal"));
    }

    #[test]
    fn edits_undo_and_save() {
        let mut d = open("libreoffice-budget.xlsx");
        assert!(!d.modified());
        d.set_cell(0, 1, 1, "1300").unwrap();
        assert!(d.modified());
        assert_eq!(d.grid_cells(0, 1..2, 3..4)[0].2.text, "2,500.00");
        d.grid_edit(0, GridEdit::InsertRows { at: 1, count: 1 })
            .unwrap();
        assert_eq!(d.cell_input(0, 2, 3), "=B3+C3");
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert!(!d.modified());
        assert!(d.redo().unwrap());
        let out = d.save().unwrap();
        assert!(!d.modified());
        let mut again = Workbook::open(out.bytes).unwrap();
        assert_eq!(again.display(0, CellRef::new(1, 1)).unwrap(), "1,300.00");
    }

    #[test]
    fn column_widths_are_edits() {
        let mut d = open("libreoffice-budget.xlsx");
        d.set_col_width(0, 1, 20.0).unwrap();
        assert_eq!(d.grid(0).unwrap().widths[1], 20.0);
        // Column A keeps its width.
        assert_eq!(d.grid(0).unwrap().widths[0], 18.0);
        assert!(d.modified());
        let saved = d.save().unwrap().bytes;
        let mut wb = Workbook::open(saved).unwrap();
        assert_eq!(wb.sheet(0).unwrap().col_width(1), Some(20.0));
        assert!(d.undo().unwrap());
        assert_ne!(d.grid(0).unwrap().widths.get(1).copied(), Some(20.0));
    }

    #[test]
    fn row_heights_are_edits() {
        let mut d = open("libreoffice-budget.xlsx");
        assert_eq!(d.grid(0).unwrap().default_height, 15.0);
        // A row that exists, and one past the data.
        d.set_row_height(0, 2, 30.0).unwrap();
        d.set_row_height(0, 20, 8.5).unwrap();
        let g = d.grid(0).unwrap();
        assert!(
            g.heights.contains(&(2, 30.0)) && g.heights.contains(&(20, 8.5)),
            "{:?}",
            g.heights
        );
        assert_eq!(d.cell_input(0, 2, 0), "Food");
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        let s = wb.sheet(0).unwrap();
        assert_eq!(s.rows[&2].height, Some(30.0));
        assert_eq!(s.rows[&20].height, Some(8.5));
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert!(!d.grid(0).unwrap().heights.contains(&(2, 30.0)));
    }

    #[test]
    fn wrap_text_is_a_style() {
        let mut d = open("libreoffice-budget.xlsx");
        // A cell with a style (bold, filled) and an empty one.
        d.set_wrap(0, 0, 1, true).unwrap();
        d.set_wrap(0, 9, 9, true).unwrap();
        let c = d.grid_cells(0, 0..1, 1..2).remove(0).2;
        assert!(c.wrap && c.bold && c.fill.is_some(), "{c:?}");
        assert!(d.grid_cells(0, 9..10, 9..10)[0].2.wrap);
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        let s = wb.sheet(0).unwrap().cells[&CellRef::new(0, 1)].style;
        assert!(wb.style(s).wrap && wb.style(s).bold);
        d.set_wrap(0, 0, 1, false).unwrap();
        assert!(!d.grid_cells(0, 0..1, 1..2).remove(0).2.wrap);
        assert!(d.undo().unwrap() && d.undo().unwrap() && d.undo().unwrap());
        assert!(!d.grid_cells(0, 0..1, 1..2).remove(0).2.wrap);
        // Undone past the save: different from the file on disk.
        assert!(d.modified());
    }

    #[test]
    fn moving_cells() {
        let mut d = open("libreoffice-budget.xlsx");
        // B2:C3 (Rent and Food's quarters) moved to H10.
        d.move_cells(0, [1, 1, 2, 2], 9, 7).unwrap();
        assert_eq!(d.cell_input(0, 1, 1), "");
        assert_eq!(d.cell_input(0, 9, 7), "1200");
        assert_eq!(d.cell_input(0, 10, 8), "512.25");
        // The format went with them.
        assert_eq!(d.grid_cells(0, 9..10, 7..8)[0].2.text, "1,200.00");
        // The formulas that read them follow; their results stand.
        assert_eq!(d.cell_input(0, 1, 3), "=H10+I10");
        assert_eq!(d.cell_input(0, 4, 1), "=SUM(B2:B4)");
        assert_eq!(d.grid_cells(0, 1..2, 3..4)[0].2.text, "2,400.00");
        assert_eq!(d.grid_cells(0, 4..5, 1..2)[0].2.text, "0.00");
        // One step back.
        assert!(d.undo().unwrap());
        assert_eq!(d.cell_input(0, 1, 1), "1200");
        assert_eq!(d.cell_input(0, 1, 3), "=B2+C2");
        // A merged cell in the way is refused.
        assert!(d.move_cells(0, [1, 1, 1, 1], 0, 5).is_err());
    }

    #[test]
    fn moving_cells_to_another_sheet() {
        let mut d = open("libreoffice-budget.xlsx");
        // Budget!B3:D3 (Food: Q1, Q2 and its total =B3+C3) to Dates!C10.
        d.move_cells_between(0, [2, 1, 2, 3], 1, 9, 2).unwrap();
        assert_eq!(d.cell_input(0, 2, 1), "");
        assert_eq!(d.cell_input(1, 9, 2), "431.5");
        // The moved formula names the cells it read: they moved with it.
        assert_eq!(d.cell_input(1, 9, 4), "=Dates!C10+Dates!D10");
        assert_eq!(d.grid_cells(1, 9..10, 4..5)[0].2.text, "943.75");
        // Budget's sums now read Dates where the cells went.
        assert_eq!(d.cell_input(0, 4, 3), "=SUM(D2:D4)");
        assert_eq!(d.grid_cells(0, 4..5, 3..4)[0].2.text, "3,350.00");
        assert!(d.undo().unwrap());
        assert_eq!(d.cell_input(0, 2, 3), "=B3+C3");
        assert_eq!(d.cell_input(1, 9, 2), "");
    }

    #[test]
    fn sorting_rows() {
        let mut d = open("libreoffice-budget.xlsx");
        // A1:D4 by Q1 (B), the header kept: Travel 0, Food 431.5, Rent 1200.
        d.sort_range(0, [0, 0, 3, 3], 1, false, true).unwrap();
        let names: Vec<String> = (1..4).map(|r| d.cell_input(0, r, 0)).collect();
        assert_eq!(names, ["Travel", "Food", "Rent"]);
        assert_eq!(d.cell_input(0, 0, 0), "Item");
        // Each row's formula moved with it, reading its own row.
        assert_eq!(d.cell_input(0, 1, 3), "=B2+C2");
        assert_eq!(d.grid_cells(0, 1..2, 3..4)[0].2.text, "950.00");
        assert_eq!(
            d.grid_cells(0, 3..4, 1..2)[0].2.text,
            "1,200.00",
            "formats move too"
        );
        d.sort_range(0, [0, 0, 3, 3], 0, true, true).unwrap();
        let names: Vec<String> = (1..4).map(|r| d.cell_input(0, r, 0)).collect();
        assert_eq!(names, ["Travel", "Rent", "Food"]);
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert_eq!(d.cell_input(0, 1, 0), "Rent");
    }

    #[test]
    fn filtering_rows() {
        let mut d = open("libreoffice-budget.xlsx");
        d.set_filter(0, Some([0, 0, 3, 3])).unwrap();
        assert_eq!(d.grid(0).unwrap().filter, Some([0, 0, 3, 3]));
        // Q2 (C) to 512.25 and 950.00: Rent's row hides.
        d.filter_column(0, 2, Some(vec!["512.25".into(), "950.00".into()]))
            .unwrap();
        let g = d.grid(0).unwrap();
        assert_eq!(g.hidden_rows, vec![1]);
        assert_eq!(g.filtered, vec![2]);
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        let s = wb.sheet(0).unwrap();
        assert!(s.rows[&1].hidden);
        let af = s.auto_filter.clone().unwrap();
        assert_eq!(
            af.columns[0].values.as_deref(),
            Some(&["512.25".to_string(), "950.00".into()][..])
        );
        // Cleared: shown again; the filter off.
        d.filter_column(0, 2, None).unwrap();
        assert!(d.grid(0).unwrap().hidden_rows.is_empty());
        d.filter_column(0, 0, Some(vec!["Food".into()])).unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![1, 3]);
        d.set_filter(0, None).unwrap();
        let g = d.grid(0).unwrap();
        assert!(g.filter.is_none() && g.hidden_rows.is_empty());
        assert!(d.undo().unwrap());
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![1, 3]);
    }

    #[test]
    fn pasting_rows() {
        let mut d = open("libreoffice-budget.xlsx");
        let rows = vec![
            vec!["Books".to_string(), "1,000.50".into(), "=B8*2".into()],
            vec!["Tea".into(), "20".into()],
        ];
        d.set_cells(0, 7, 0, &rows).unwrap();
        assert_eq!(d.cell_input(0, 7, 0), "Books");
        assert_eq!(d.cell_input(0, 7, 1), "1000.5");
        assert_eq!(d.grid_cells(0, 7..8, 2..3)[0].2.text, "2001");
        assert_eq!(d.cell_input(0, 8, 1), "20");
        // One step.
        assert!(d.undo().unwrap());
        assert_eq!(d.cell_input(0, 7, 0), "");
        // Into part of a merged cell: refused whole, nothing written.
        assert!(
            d.set_cells(0, 0, 4, &[vec!["a".into(), "b".into(), "c".into()]])
                .is_err()
        );
        assert_eq!(d.cell_input(0, 0, 4), "");
    }

    #[test]
    fn clearing_a_range() {
        let mut d = open("libreoffice-budget.xlsx");
        // B2:C3: four numbers, gone in one step, their formats kept.
        d.clear_cells(0, [1, 1, 2, 2]).unwrap();
        assert_eq!(d.cell_input(0, 1, 1), "");
        assert_eq!(d.cell_input(0, 2, 2), "");
        assert_eq!(d.grid_cells(0, 1..2, 3..4)[0].2.text, "0.00");
        d.set_cell(0, 1, 1, "5").unwrap();
        assert_eq!(
            d.grid_cells(0, 1..2, 1..2)[0].2.text,
            "5.00",
            "the format stayed"
        );
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert_eq!(d.cell_input(0, 2, 2), "512.25");
    }

    #[test]
    fn merging_cells() {
        let mut d = open("libreoffice-budget.xlsx");
        // B2:C3 holds four numbers: only B2's stays, centered; one undo step.
        d.merge_cells(0, [1, 1, 2, 2], true).unwrap();
        let g = d.grid(0).unwrap();
        assert!(g.merged.contains(&[1, 1, 2, 2]), "{:?}", g.merged);
        assert_eq!(d.cell_input(0, 1, 1), "1200");
        assert_eq!(d.cell_input(0, 1, 2), "");
        assert_eq!(d.cell_input(0, 2, 1), "");
        assert_eq!(d.grid_cells(0, 1..2, 1..2)[0].2.align, Align::Center);
        // The sums follow the cleared cells.
        assert_eq!(d.grid_cells(0, 4..5, 1..2)[0].2.text, "1,200.00");
        // Overlapping an existing merge is refused.
        assert!(d.merge_cells(0, [0, 5, 1, 6], false).is_err());
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        assert!(
            wb.sheet(0)
                .unwrap()
                .merged
                .iter()
                .any(|m| m.to_string() == "B2:C3")
        );
        d.unmerge_cells(0, 2, 2).unwrap();
        assert!(!d.grid(0).unwrap().merged.contains(&[1, 1, 2, 2]));
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert_eq!(d.cell_input(0, 1, 2), "1200");
        assert!(!d.grid(0).unwrap().merged.contains(&[1, 1, 2, 2]));
    }

    #[test]
    fn legacy_formats_are_shown() {
        for name in ["libreoffice-budget.xls", "libreoffice-budget.ods"] {
            let mut d = open(name);
            assert!(!d.grid(0).unwrap().editable);
            assert!(d.grid_cells(0, 0..1, 0..1)[0].2.text == "Item");
            assert!(d.set_cell(0, 0, 0, "x").is_err());
        }
    }

    #[test]
    fn macros_through_the_contract() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/corpus/libreoffice-budget.xlsx");
        let mut d = XlsxViewer.open(FileHandle::new(&p)).unwrap();
        assert!(d.macros().is_empty());
        assert!(d.run_macro("X", &mut Yes).is_err());
        // A workbook with a project.
        let src = "Sub Ask()\r\n  Range(\"A9\") = 1\r\n  MsgBox \"hi\"\r\n  If MsgBox(\"Go on?\", vbYesNo) = vbYes Then Range(\"A10\") = 2\r\nEnd Sub\r\n";
        let mut pkg = crate::package::Package::read(std::fs::read(&p).unwrap()).unwrap();
        pkg.set_part(
            "xl/vbaProject.bin",
            crate::vba::write_project(
                &[("Module1", crate::vba::ModuleKind::Standard, src.as_bytes())],
                1252,
            ),
        );
        let rels = String::from_utf8(pkg.part("xl/_rels/workbook.xml.rels").unwrap()).unwrap().replace(
            "</Relationships>",
            "<Relationship Id=\"rIdV\" Type=\"http://schemas.microsoft.com/office/2006/relationships/vbaProject\" Target=\"vbaProject.bin\"/></Relationships>",
        );
        pkg.set_part("xl/_rels/workbook.xml.rels", rels.into_bytes());
        let dir = std::env::temp_dir().join(format!("kalem-xlsx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.xlsm");
        std::fs::write(&file, pkg.write().unwrap()).unwrap();
        let mut d = XlsxViewer.open(FileHandle::new(&file)).unwrap();
        assert_eq!(
            d.macros(),
            vec![MacroEntry {
                name: "Module1.Ask".into(),
                event: false
            }]
        );
        // Asked: stopped and taken back.
        let o = d.run_macro("Module1.Ask", &mut Later).unwrap();
        assert!(
            matches!(o.question, Some(MacroQuestion::Message { buttons: 4, .. })),
            "{o:?}"
        );
        assert!(!o.changed && !d.modified());
        assert_eq!(d.cell_input(0, 8, 0), "");
        // Answered: runs through.
        let o = d.run_macro("Module1.Ask", &mut Yes).unwrap();
        assert!(o.question.is_none() && o.changed);
        assert_eq!(o.output, ["hi", "Go on?"]);
        assert_eq!(d.cell_input(0, 9, 0), "2");
        assert!(d.undo().unwrap());
        assert_eq!(d.cell_input(0, 8, 0), "");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn conditional_formats_color_the_grid() {
        let mut d = open("openpyxl-budget.xlsx");
        let rule = CondRule::Formula("=ROW()=7".into());
        let style = CondStyle {
            fill: Some([1, 2, 3]),
            color: None,
            bold: true,
        };
        d.add_conditional_format(0, [1, 0, 9, 3], rule, style)
            .unwrap();
        d.add_conditional_format(
            0,
            [1, 2, 4, 2],
            CondRule::IconSet("3Arrows".into()),
            CondStyle::default(),
        )
        .unwrap();
        let cells = d.grid_cells(0, 0..20, 0..6);
        let get = |r: u32, c: u32| {
            cells
                .iter()
                .find(|(a, b, _)| (*a, *b) == (r, c))
                .map(|x| x.2.clone())
        };
        // An empty cell the formula colors is in the grid; others are not.
        let empty = get(6, 3).unwrap();
        assert_eq!((empty.fill, empty.bold), (Some([1, 2, 3]), true));
        assert!(get(8, 3).is_none_or(|c| c.fill.is_none()));
        assert!(get(1, 2).unwrap().icon.is_some());
        assert_eq!(
            d.clear_conditional_formats(0, Some([15, 5, 15, 5]))
                .unwrap(),
            Vec::<usize>::new()
        );
        assert_eq!(d.clear_conditional_formats(0, None).unwrap(), vec![0]);
        assert!(
            d.grid_cells(0, 0..20, 0..6)
                .iter()
                .all(|(_, _, c)| c.icon.is_none())
        );
        assert!(d.undo().unwrap());
        assert!(
            d.grid_cells(0, 0..20, 0..6)
                .iter()
                .any(|(_, _, c)| c.icon.is_some())
        );
    }

    #[test]
    fn data_validation() {
        let mut d = open("openpyxl-budget.xlsx");
        let list = Validation {
            kind: ValidationKind::List,
            value: "Food, Rent ,Travel".into(),
            prompt: Some(("Item".into(), "Pick one".into())),
            ..Validation::default()
        };
        d.set_validation(0, [1, 0, 4, 0], Some(list)).unwrap();
        let v = d.validation(0, 2, 0).unwrap();
        assert_eq!(v.list, ["Food", "Rent", "Travel"]);
        assert_eq!(v.value, "Food,Rent,Travel");
        assert_eq!(v.prompt, Some(("Item".into(), "Pick one".into())));
        assert!(d.validation(0, 5, 0).is_none());
        assert!(d.check_input(0, 1, 0, "rent").is_none());
        let e = d.check_input(0, 1, 0, "Cinema").unwrap();
        assert_eq!(e.style, ErrorStyle::Stop);
        // Checking left the cell and the history as they were.
        let before = d.cell_input(0, 1, 0);
        assert!(d.check_input(0, 1, 0, "Cinema").is_some());
        assert_eq!(d.cell_input(0, 1, 0), before);
        assert!(!d.redo().unwrap_or(false));
        // Whole numbers over 1000 in B2:B5, a warning.
        let whole = Validation {
            kind: ValidationKind::Whole,
            op: CompareOp::Greater,
            value: "1000".into(),
            error: Some((ErrorStyle::Warning, "Small".into(), "Under 1000".into())),
            ..Validation::default()
        };
        d.set_validation(0, [1, 1, 4, 1], Some(whole)).unwrap();
        let e = d.check_input(0, 2, 1, "5").unwrap();
        assert_eq!(
            (e.style, e.message.as_str()),
            (ErrorStyle::Warning, "Under 1000")
        );
        assert!(d.check_input(0, 2, 1, "1500").is_none());
        assert!(d.check_input(0, 2, 1, "1500.5").is_some());
        // The cells already there that break it, circled.
        let bad = d.invalid_cells(0, 0..10, 0..4);
        assert!(bad.iter().all(|(_, c)| *c == 1 || *c == 0), "{bad:?}");
        // A list from a range, read through the cells.
        let from_range = Validation {
            kind: ValidationKind::List,
            value: "=$A$2:$A$4".into(),
            ..Validation::default()
        };
        d.set_validation(0, [7, 3, 7, 3], Some(from_range)).unwrap();
        let names: Vec<String> = (1..4).map(|r| d.cell_input(0, r, 0)).collect();
        assert_eq!(d.validation(0, 7, 3).unwrap().list, names);
        // Saved and read again: Excel's markup, one block.
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        assert_eq!(wb.validations(0).unwrap().len(), 3);
        // Cleared from part of a range: the rest kept.
        d.set_validation(0, [2, 0, 2, 0], None).unwrap();
        assert!(d.validation(0, 2, 0).is_none());
        assert!(d.validation(0, 3, 0).is_some() && d.validation(0, 1, 0).is_some());
        d.set_validation(0, [0, 0, 20, 5], None).unwrap();
        let text = String::from_utf8(d.save().unwrap().bytes).unwrap_or_default();
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        assert!(wb.validations(0).unwrap().is_empty());
        let _ = text;
        assert!(d.undo().unwrap());
        assert!(d.validation(0, 3, 0).is_some());
    }

    #[test]
    fn pivot_tables() {
        let mut d = open("openpyxl-budget.xlsx");
        let sheets = d.structure().units.len();
        // Item by Q1, over A1:C5.
        let spec = PivotSpec {
            range: [0, 0, 4, 2],
            rows: vec![0],
            cols: vec![],
            values: vec![
                PivotSpec::value(1, Aggregate::Sum),
                PivotSpec::value(2, Aggregate::Max),
            ],
            ..PivotSpec::default()
        };
        let unit = d.insert_pivot(0, spec).unwrap();
        assert_eq!(unit, sheets);
        assert_eq!(d.structure().units.len(), sheets + 1);
        assert_eq!(d.cell_input(unit, 2, 0), "Row Labels");
        assert_eq!(d.cell_input(unit, 2, 1), "Sum of Q1");
        let last = (3..20)
            .find(|&r| d.cell_input(unit, r, 0) == "Grand Total")
            .unwrap();
        assert_eq!(last, 3 + 4, "four items, then the total");
        let bytes = d.save().unwrap().bytes;
        let wb = Workbook::open(bytes).unwrap();
        assert_eq!(wb.pivot_tables().len(), 1);
        assert_eq!(wb.sheets()[unit].name, "Pivot1");
        // A source value changed, then Refresh All.
        d.set_cell(0, 1, 1, "=1000000").unwrap();
        let total = d.cell_input(unit, last, 1);
        assert_eq!(d.refresh_pivots().unwrap(), vec![unit]);
        assert_ne!(d.cell_input(unit, last, 1), total);
        assert!(d.undo().unwrap());
        assert_eq!(d.cell_input(unit, last, 1), total);
        // Undone, the new sheet goes too.
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert_eq!(d.structure().units.len(), sheets);
        // Rows by item and columns by Q2, counted.
        let spec = PivotSpec {
            range: [0, 0, 4, 2],
            rows: vec![0],
            cols: vec![2],
            values: vec![PivotSpec::value(1, Aggregate::Count)],
            ..PivotSpec::default()
        };
        let unit = d.insert_pivot(0, spec).unwrap();
        assert_eq!(d.cell_input(unit, 2, 1), "Column Labels");
        assert_eq!(d.cell_input(unit, 3, 0), "Row Labels");
        if let Ok(dir) = std::env::var("KALEM_PIVOT_OUT") {
            std::fs::write(
                format!("{dir}/kalem-pivot-test.xlsx"),
                d.save().unwrap().bytes,
            )
            .unwrap();
        }
    }

    #[test]
    fn colored_cells_columns_and_rows_read() {
        // Excel's (openpyxl's) and LibreOffice's OpenDocument: a red cell,
        // column B yellow, row 5 blue, empty cells far off too.
        for name in ["openpyxl-colors.xlsx", "libreoffice-colors.ods"] {
            let mut d = open(name);
            let at = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
                d.grid_cells(0, r..r + 1, c..c + 1)
                    .first()
                    .and_then(|x| x.2.fill)
            };
            assert_eq!(at(&mut d, 1, 3), Some([255, 0, 0]), "{name}");
            assert_eq!(at(&mut d, 0, 1), Some([255, 255, 0]), "{name}");
            assert_eq!(at(&mut d, 500, 1), Some([255, 255, 0]), "{name}");
            assert_eq!(at(&mut d, 4, 2), Some([0, 176, 240]), "{name}");
            assert_eq!(at(&mut d, 4, 40), Some([0, 176, 240]), "{name}");
            assert_eq!(at(&mut d, 2, 2), None, "{name}");
        }
    }

    #[test]
    fn whole_columns_and_rows_colored() {
        let mut d = open("openpyxl-budget.xlsx");
        let fill = |c: [u8; 3]| kalem_viewer::StyleChange {
            fill: Some(Some(c)),
            ..Default::default()
        };
        // Column F yellow, row 12 blue: their own styles.
        d.change_style(0, [0, 5, 1_048_575, 5], fill([255, 255, 0]))
            .unwrap();
        d.change_style(0, [11, 0, 11, 16_383], fill([0, 176, 240]))
            .unwrap();
        let at = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1)
                .first()
                .and_then(|x| x.2.fill)
        };
        assert_eq!(at(&mut d, 500, 5), Some([255, 255, 0]));
        assert_eq!(at(&mut d, 11, 30), Some([0, 176, 240]));
        assert_eq!(
            at(&mut d, 11, 5),
            Some([0, 176, 240]),
            "the row's over the column's"
        );
        assert_eq!(at(&mut d, 1, 1), None);
        // A value typed in the column keeps its color.
        d.set_cell(0, 40, 5, "7").unwrap();
        assert_eq!(at(&mut d, 40, 5), Some([255, 255, 0]));
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-whole.xlsx"), &saved).unwrap();
        }
        let back = std::env::temp_dir().join(format!("kalem-whole-{}.xlsx", std::process::id()));
        std::fs::write(&back, &saved).unwrap();
        let mut reopened = XlsxViewer.open(FileHandle::new(back.clone())).unwrap();
        assert_eq!(at(&mut reopened, 900, 5), Some([255, 255, 0]));
        let _ = std::fs::remove_file(back);
        assert!(d.undo().unwrap() && d.undo().unwrap() && d.undo().unwrap());
        assert_eq!(at(&mut d, 500, 5), None);
    }

    #[test]
    fn pivot_tables_the_rest() {
        use kalem_viewer::{
            CalculatedField, PivotFilter, PivotFilterKind, PivotSort, PivotValue, ReportForm,
            ShowAs,
        };
        let mut d = open("openpyxl-budget.xlsx");
        let spec = PivotSpec {
            range: [0, 0, 4, 2],
            rows: vec![0],
            values: vec![PivotSpec::value(1, Aggregate::Sum)],
            ..PivotSpec::default()
        };
        let unit = d.insert_pivot(0, spec).unwrap();
        let info = d.pivots(unit);
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].name, "PivotTable1");
        assert_eq!(info[0].fields[..3], ["Item", "Q1", "Q2"]);
        let mut spec = info[0].spec.clone();
        // Q1 and a calculated field, a share of the total; the largest
        // first; one item hidden; the tabular layout.
        spec.calculated = vec![CalculatedField {
            name: "Both".into(),
            formula: "Q1 + Q2".into(),
        }];
        spec.values.push(PivotValue {
            field: 3,
            show_as: ShowAs::PercentOfTotal,
            ..PivotValue::default()
        });
        spec.sorts = vec![PivotSort {
            field: 0,
            descending: true,
            by_value: Some(0),
        }];
        let first = d.cell_input(unit, 3, 0);
        spec.filters = vec![PivotFilter {
            field: 0,
            kind: PivotFilterKind::Items,
            hidden: vec![first.clone()],
            ..PivotFilter::default()
        }];
        spec.form = ReportForm::Tabular;
        d.set_pivot(unit, 0, spec.clone()).unwrap();
        assert_eq!(d.pivots(unit)[0].spec, spec);
        assert_eq!(d.cell_input(unit, 2, 0), "Item");
        assert_eq!(d.cell_input(unit, 2, 2), "Sum of Both");
        let items: Vec<String> = (3..7).map(|r| d.cell_input(unit, r, 0)).collect();
        assert!(!items.contains(&first), "{items:?}");
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_PIVOT_OUT") {
            std::fs::write(format!("{dir}/kalem-pivot-rest.xlsx"), &saved).unwrap();
        }
        let mut back = Workbook::open(saved).unwrap();
        assert_eq!(back.pivots(unit)[0].spec, spec);
        assert!(d.undo().unwrap());
        assert_eq!(d.pivots(unit)[0].spec.form, ReportForm::Compact);
        // A PivotChart beside it.
        d.insert_pivot_chart(unit, 0, ChartKind::Column).unwrap();
        assert_eq!(d.charts(unit).len(), 1);
        // A slicer of the items: one chosen, the table filtered so.
        d.insert_slicer(unit, Some(0), None, "Item", [2, 6, 12, 8])
            .unwrap();
        let s = d.slicers(unit);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].caption, "Item");
        assert!(s[0].items.iter().all(|i| i.1));
        let pick = s[0].items[0].0.clone();
        d.select_slicer(unit, 0, std::slice::from_ref(&pick))
            .unwrap();
        let s = d.slicers(unit);
        assert_eq!(s[0].items.iter().filter(|i| i.1).count(), 1);
        assert_eq!(d.cell_input(unit, 3, 0), pick);
        assert_eq!(d.cell_input(unit, 4, 0), "Grand Total");
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_PIVOT_OUT") {
            std::fs::write(format!("{dir}/kalem-slicer.xlsx"), &saved).unwrap();
        }
        let mut back = Workbook::open(saved).unwrap();
        assert_eq!(back.slicers(unit).len(), 1);
        d.select_slicer(unit, 0, &[]).unwrap();
        assert!(d.slicers(unit)[0].items.iter().all(|i| i.1));
        d.delete_slicer(unit, 0).unwrap();
        assert!(d.slicers(unit).is_empty());
        // A table's slicer: its rows filtered.
        let t = d
            .create_table(0, [0, 0, 4, 2], true, "TableStyleMedium2")
            .unwrap();
        d.insert_slicer(0, None, Some(&t), "Item", [10, 5, 18, 7])
            .unwrap();
        let s = d.slicers(0);
        let first = s[0].items[0].0.clone();
        d.select_slicer(0, 0, std::slice::from_ref(&first)).unwrap();
        let hidden = d.grid(0).unwrap().hidden_rows;
        assert_eq!(hidden.len(), 3, "{hidden:?}");
    }

    #[test]
    fn charts_the_rest() {
        let mut d = open("openpyxl-budget.xlsx");
        let n = d.charts(0).len();
        d.insert_chart(0, [0, 0, 4, 2], ChartKind::Column, Some("Spending".into()))
            .unwrap();
        // Q2 a line on the secondary axis: a combo chart.
        d.set_series_kind(0, n, 1, Some(ChartKind::Line), true)
            .unwrap();
        let c = d.charts(0)[n].clone();
        assert_eq!(c.kind, ChartKind::Column);
        assert_eq!(c.series[1].kind, Some(ChartKind::Line));
        assert!(c.series[1].secondary && !c.series[0].secondary);
        // A trendline with its equation, error bars, labels from cells.
        let t = kalem_viewer::Trendline {
            kind: kalem_viewer::TrendKind::Linear,
            equation: true,
            r_squared: true,
            ..Default::default()
        };
        d.set_trendline(0, n, 0, Some(t)).unwrap();
        let b = kalem_viewer::ErrorBars {
            kind: kalem_viewer::ErrorKind::Percent,
            value: 10.0,
        };
        d.set_error_bars(0, n, 0, Some(b)).unwrap();
        d.set_label_cells(0, n, 0, Some([1, 0, 4, 0])).unwrap();
        let c = d.charts(0)[n].clone();
        assert_eq!(c.series[0].trendline, Some(t));
        assert_eq!(c.series[0].error_bars, Some(b));
        assert_eq!(c.series[0].cell_labels.len(), 4);
        // Saved as a template and given to a chart of another kind.
        let crtx = d.chart_template(0, n).unwrap();
        d.insert_chart(0, [0, 0, 4, 1], ChartKind::Line, None)
            .unwrap();
        d.apply_chart_template(0, n + 1, &crtx).unwrap();
        assert_eq!(d.charts(0)[n + 1].kind, ChartKind::Column);
        // Moved to a chart sheet of its own, and back.
        let units = d.structure().units.len();
        let sheet = d.move_chart_to_sheet(0, n, "Chart1").unwrap();
        assert_eq!(sheet, units);
        assert_eq!(d.structure().units.len(), units + 1);
        assert_eq!(d.charts(0).len(), n + 1);
        let on_sheet = d.charts(sheet);
        assert_eq!(on_sheet.len(), 1);
        assert_eq!(on_sheet[0].series[1].kind, Some(ChartKind::Line));
        assert!(d.grid(sheet).is_some_and(|g| !g.editable));
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-charts-rest.xlsx"), &saved).unwrap();
        }
        let back = d.move_chart_to_grid(sheet, 0, [20, 0, 34, 7]).unwrap();
        assert_eq!(back, 0);
        assert_eq!(d.structure().units.len(), units);
        assert_eq!(d.charts(0).len(), n + 2);
        assert_eq!(d.charts(0)[n + 1].anchor, [20, 0, 34, 7]);
        assert!(d.undo().unwrap());
        assert_eq!(d.structure().units.len(), units + 1);
    }

    #[test]
    fn histograms_and_waterfalls_read() {
        let x = r#"<cx:chartSpace xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"><cx:chartData><cx:data id="0"><cx:numDim type="val"><cx:f>Sheet1!$B$2:$B$7</cx:f><cx:lvl ptCount="6"><cx:pt idx="0">1</cx:pt><cx:pt idx="1">2</cx:pt><cx:pt idx="2">2</cx:pt><cx:pt idx="3">3</cx:pt><cx:pt idx="4">9</cx:pt><cx:pt idx="5">10</cx:pt></cx:lvl></cx:numDim></cx:data></cx:chartData><cx:chart><cx:title><cx:tx><cx:txData><cx:v>Ages</cx:v></cx:txData></cx:tx></cx:title><cx:plotArea><cx:plotAreaRegion><cx:series layoutId="clusteredColumn" uniqueId="{1}"><cx:dataId val="0"/><cx:layoutPr><cx:binning intervalClosed="r"><cx:binCount val="3"/></cx:binning></cx:layoutPr></cx:series></cx:plotAreaRegion></cx:plotArea></cx:chart></cx:chartSpace>"#;
        let def = crate::chart::parse_chartex(x, &[]);
        assert_eq!(def.kind, ChartKind::Histogram);
        assert_eq!(def.title.as_deref(), Some("Ages"));
        assert_eq!(def.series[0].val.0.as_deref(), Some("Sheet1!$B$2:$B$7"));
        assert_eq!(def.series[0].binning, Some((Some(3), None)));
        let (labels, counts) = crate::chart::histogram(&def.series[0].val.1, (Some(3), None));
        assert_eq!(labels[0], "[1, 4]");
        assert_eq!(counts, vec![4, 0, 2]);
        let w = x.replace("clusteredColumn", "waterfall").replace(
            "<cx:binning intervalClosed=\"r\"><cx:binCount val=\"3\"/></cx:binning>",
            "<cx:subtotals><cx:idx val=\"5\"/></cx:subtotals>",
        );
        let def = crate::chart::parse_chartex(&w, &[]);
        assert_eq!(def.kind, ChartKind::Waterfall);
        assert_eq!(def.series[0].subtotals, vec![5]);
    }

    #[test]
    fn charts() {
        let mut d = open("openpyxl-budget.xlsx");
        let before = d.charts(0).len();
        // Q1 and Q2 by item: two series, four categories.
        d.insert_chart(0, [0, 0, 4, 2], ChartKind::Column, Some("Spending".into()))
            .unwrap();
        let charts = d.charts(0);
        assert_eq!(charts.len(), before + 1);
        let c = charts.last().unwrap();
        assert_eq!(c.kind, ChartKind::Column);
        assert_eq!(c.title.as_deref(), Some("Spending"));
        assert_eq!(c.series.len(), 2);
        assert_eq!(c.series[0].name, "Q1");
        assert_eq!(c.categories.len(), 4);
        assert_eq!(c.series[0].values[0], Some(1200.0));
        assert_eq!(c.anchor, [0, 4, 14, 11]);
        // Values follow the cells.
        d.set_cell(0, 1, 1, "5000").unwrap();
        assert_eq!(
            d.charts(0).last().unwrap().series[0].values[0],
            Some(5000.0)
        );
        // A sheet with no drawing gets one; a pie takes one series.
        d.insert_chart(1, [0, 0, 3, 0], ChartKind::Pie, None)
            .unwrap();
        assert_eq!(d.charts(1).len(), 1);
        let bytes = d.save().unwrap().bytes;
        let mut wb = Workbook::open(bytes.clone()).unwrap();
        assert_eq!(wb.charts(0).unwrap().len(), before + 1);
        assert_eq!(wb.charts(1).unwrap()[0].kind, ChartKind::Pie);
        // Moved and resized, then back with undo.
        d.move_chart(0, before, [20, 1, 30, 6]).unwrap();
        assert_eq!(d.charts(0)[before].anchor, [20, 1, 30, 6]);
        assert!(d.undo().unwrap());
        assert_eq!(d.charts(0)[before].anchor, [0, 4, 14, 11]);
        // Retitled, then untitled: no series name put in its place.
        d.set_chart_title(0, before, Some("Costs".into())).unwrap();
        assert_eq!(d.charts(0)[before].title.as_deref(), Some("Costs"));
        d.set_chart_title(0, before, None).unwrap();
        assert_eq!(d.charts(0)[before].title, None);
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert_eq!(d.charts(0)[before].title.as_deref(), Some("Spending"));
        // Axis titles, saved as Excel reads them.
        d.set_axis_title(0, before, ChartAxis::Horizontal, Some("Item".into()))
            .unwrap();
        d.set_axis_title(0, before, ChartAxis::Vertical, Some("TRY".into()))
            .unwrap();
        let c = &d.charts(0)[before];
        assert_eq!(
            (c.horizontal_title.as_deref(), c.vertical_title.as_deref()),
            (Some("Item"), Some("TRY"))
        );
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        assert_eq!(
            wb.charts(0).unwrap()[before].vertical_title.as_deref(),
            Some("TRY")
        );
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert_eq!(d.charts(0)[before].vertical_title, None);
        // Two series: a legend at the bottom, moved left, then none.
        assert_eq!(d.charts(0)[before].legend, Some(LegendPosition::Bottom));
        d.set_legend(0, before, Some(LegendPosition::Left)).unwrap();
        assert_eq!(d.charts(0)[before].legend, Some(LegendPosition::Left));
        d.set_legend(0, before, None).unwrap();
        assert_eq!(d.charts(0)[before].legend, None);
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        assert_eq!(wb.charts(0).unwrap()[before].legend, None);
        assert!(d.undo().unwrap() && d.undo().unwrap());
        // Values labeled, saved, then no labels.
        let values = DataLabels {
            value: true,
            ..DataLabels::default()
        };
        d.set_data_labels(0, before, values).unwrap();
        assert_eq!(d.charts(0)[before].labels, values);
        let labeled = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-labels-test.xlsx"), &labeled).unwrap();
        }
        let mut wb = Workbook::open(labeled).unwrap();
        assert_eq!(wb.charts(0).unwrap()[before].labels, values);
        d.set_data_labels(0, before, DataLabels::default()).unwrap();
        assert!(!d.charts(0)[before].labels.any());
        assert!(d.undo().unwrap() && d.undo().unwrap());
        // The value axis from 0 to 5000 in steps of 1000; refused upside
        // down; saved; back to automatic with undo.
        let scale = AxisScale {
            min: Some(0.0),
            max: Some(5000.0),
            major: Some(1000.0),
            log: false,
        };
        d.set_axis_scale(0, before, scale).unwrap();
        assert_eq!(d.charts(0)[before].scale, scale);
        let wrong = AxisScale {
            min: Some(10.0),
            max: Some(1.0),
            ..scale
        };
        assert!(d.set_axis_scale(0, before, wrong).is_err());
        let scaled = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-scale-test.xlsx"), &scaled).unwrap();
        }
        let mut wb = Workbook::open(scaled).unwrap();
        assert_eq!(wb.charts(0).unwrap()[before].scale, scale);
        assert!(d.undo().unwrap());
        assert_eq!(d.charts(0)[before].scale, AxisScale::default());
        // A cream background and a gray border, kept by the change of kind
        // below.
        d.set_chart_area(
            0,
            before,
            Paint::Color([0xFF, 0xF2, 0xCC]),
            Paint::Color([0x40; 3]),
        )
        .unwrap();
        let c = d.charts(0)[before].clone();
        assert_eq!(
            (c.background, c.border),
            (Paint::Color([0xFF, 0xF2, 0xCC]), Paint::Color([0x40; 3]))
        );
        // A light gray plot area, kept by the change of kind below.
        d.set_plot_area(0, before, Paint::Color([0xF2; 3]), Paint::Automatic)
            .unwrap();
        assert_eq!(d.charts(0)[before].plot_background, Paint::Color([0xF2; 3]));
        // Minor horizontal lines too, kept by the change of kind below
        // (turned with the value axis).
        let lines = Gridlines {
            horizontal_major: true,
            horizontal_minor: true,
            ..Gridlines::default()
        };
        d.set_gridlines(0, before, lines).unwrap();
        assert_eq!(d.charts(0)[before].gridlines, lines);
        // Its value axis in thousands of lira, kept by the change of kind.
        d.set_axis_format(0, before, Some("#,##0 \"TL\"".into()))
            .unwrap();
        assert_eq!(
            d.charts(0)[before].axis_format.as_deref(),
            Some("#,##0 \"TL\"")
        );
        // Its category labels bold and blue, kept by the change of kind
        // (on a bar chart's vertical axis).
        let bold = AxisFont {
            bold: true,
            color: Some([0x1F, 0x4E, 0x79]),
            ..AxisFont::default()
        };
        d.set_axis_font(0, before, ChartAxis::Horizontal, bold.clone())
            .unwrap();
        assert_eq!(d.charts(0)[before].horizontal_font, bold);
        assert!(
            d.set_axis_font(
                0,
                before,
                ChartAxis::Vertical,
                AxisFont {
                    size: Some(0.0),
                    ..AxisFont::default()
                }
            )
            .is_err()
        );
        // The title in a big green font, kept when its words change and by
        // the change of kind below.
        let big = AxisFont {
            size: Some(18.0),
            bold: true,
            color: Some([0, 0xB0, 0x50]),
            ..AxisFont::default()
        };
        d.set_title_font(0, before, big.clone()).unwrap();
        assert_eq!(d.charts(0)[before].title_font, big);
        d.set_chart_title(0, before, Some("Spending".into()))
            .unwrap();
        assert_eq!(d.charts(0)[before].title_font, big);
        // The legend small and gray, kept by the change of kind below.
        let small = AxisFont {
            size: Some(8.0),
            color: Some([0x59; 3]),
            ..AxisFont::default()
        };
        d.set_legend_font(0, before, small.clone()).unwrap();
        assert_eq!(d.charts(0)[before].legend_font, small);
        // Q2 colored red, kept by the change of kind below.
        d.set_series_color(0, before, 1, Some([0xFF, 0, 0]))
            .unwrap();
        assert_eq!(d.charts(0)[before].series[1].color, Some([0xFF, 0, 0]));
        assert_eq!(d.charts(0)[before].series[0].color, None);
        // Made a bar chart: the series, title, legend, labels and scale
        // kept, the category axis's title going to the side with the
        // categories; then a line chart; undone.
        d.set_axis_title(0, before, ChartAxis::Horizontal, Some("Item".into()))
            .unwrap();
        d.set_data_labels(0, before, values).unwrap();
        d.set_axis_scale(0, before, scale).unwrap();
        let was = d.charts(0)[before].clone();
        d.set_chart_kind(0, before, ChartKind::Bar).unwrap();
        let bar = d.charts(0)[before].clone();
        assert_eq!(bar.kind, ChartKind::Bar);
        assert_eq!(bar.series, was.series);
        assert_eq!(bar.categories, was.categories);
        assert_eq!((bar.background, bar.border), (was.background, was.border));
        assert_eq!(bar.plot_background, was.plot_background);
        assert_eq!(bar.axis_format, was.axis_format);
        assert_eq!(bar.title_font, was.title_font);
        assert_eq!(bar.legend_font, was.legend_font);
        assert_eq!(
            bar.vertical_font, was.horizontal_font,
            "the categories' font"
        );
        assert_eq!(
            bar.gridlines,
            Gridlines {
                vertical_major: true,
                vertical_minor: true,
                ..Gridlines::default()
            }
        );
        assert_eq!(
            (bar.title.clone(), bar.legend, bar.labels, bar.scale),
            (was.title.clone(), was.legend, was.labels, was.scale)
        );
        assert_eq!(
            (
                bar.vertical_title.as_deref(),
                bar.horizontal_title.as_deref()
            ),
            (Some("Item"), None)
        );
        d.set_chart_kind(0, before, ChartKind::Line).unwrap();
        let line = d.charts(0)[before].clone();
        assert_eq!(
            (line.kind, line.horizontal_title.as_deref()),
            (ChartKind::Line, Some("Item"))
        );
        // A pie made of it shows a legend for its slices.
        d.set_legend(0, before, None).unwrap();
        d.set_chart_kind(0, before, ChartKind::Pie).unwrap();
        assert!(d.charts(0)[before].legend.is_some());
        let pie = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-kind-test.xlsx"), &pie).unwrap();
        }
        let mut wb = Workbook::open(pie).unwrap();
        assert_eq!(wb.charts(0).unwrap()[before].kind, ChartKind::Pie);
        assert!(
            d.set_series_color(0, before, 0, Some([0, 0, 0])).is_err(),
            "a pie"
        );
        // Its second slice red, kept through a change of kind, then its
        // series' color again.
        d.set_point_color(0, before, 0, 1, Some([0xFF, 0, 0]))
            .unwrap();
        assert_eq!(
            d.charts(0)[before].series[0].point_colors,
            vec![(1, [0xFF, 0, 0])]
        );
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(
                format!("{dir}/kalem-slice-test.xlsx"),
                d.save().unwrap().bytes,
            )
            .unwrap();
        }
        assert!(
            d.set_point_color(0, before, 0, 99, Some([1, 1, 1]))
                .is_err()
        );
        // Its first slice pulled out a quarter of the radius, kept in a
        // doughnut; refused past 400 percent.
        d.set_explosion(0, before, 0, Some(0), 25).unwrap();
        assert_eq!(
            d.charts(0)[before].series[0].point_explosions,
            vec![(0, 25)]
        );
        assert!(d.set_explosion(0, before, 0, Some(0), 500).is_err());
        d.set_chart_kind(0, before, ChartKind::Doughnut).unwrap();
        assert_eq!(
            d.charts(0)[before].series[0].point_colors,
            vec![(1, [0xFF, 0, 0])]
        );
        assert_eq!(
            d.charts(0)[before].series[0].point_explosions,
            vec![(0, 25)]
        );
        assert!(d.undo().unwrap() && d.undo().unwrap());
        d.set_point_color(0, before, 0, 1, None).unwrap();
        assert!(d.charts(0)[before].series[0].point_colors.is_empty());
        assert!(d.undo().unwrap() && d.undo().unwrap());
        for _ in 0..8 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(d.charts(0)[before].kind, ChartKind::Column);
        // Removed, then back with undo.
        d.delete_chart(0, before).unwrap();
        assert_eq!(d.charts(0).len(), before);
        assert!(d.undo().unwrap());
        assert_eq!(d.charts(0).len(), before + 1);
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-chart-test.xlsx"), bytes).unwrap();
        }
    }

    #[test]
    fn fill_handle() {
        let mut d = open("openpyxl-budget.xlsx");
        // A10 "Item 1", B10:B11 10 and 20, C10 a formula reading B10.
        d.set_cell(0, 9, 0, "Item 1").unwrap();
        d.set_cell(0, 9, 1, "10").unwrap();
        d.set_cell(0, 10, 1, "20").unwrap();
        d.set_cell(0, 9, 2, "=B10*2").unwrap();
        // The text and its number down four rows.
        d.fill(0, [9, 0, 9, 0], [9, 0, 12, 0], true).unwrap();
        assert_eq!(d.cell_input(0, 12, 0), "Item 4");
        // 10, 20, … goes on: 30, 40.
        d.fill(0, [9, 1, 10, 1], [9, 1, 12, 1], true).unwrap();
        assert_eq!(
            (d.cell_input(0, 11, 1), d.cell_input(0, 12, 1)),
            ("30".into(), "40".into())
        );
        // The formula moves its reference.
        d.fill(0, [9, 2, 9, 2], [9, 2, 12, 2], true).unwrap();
        assert_eq!(d.cell_input(0, 12, 2), "=B13*2");
        assert_eq!(d.grid_cells(0, 12..13, 2..3)[0].2.text, "80");
        // Copied, not a series (Fill Down): 10, 20, 10, 20.
        d.fill(0, [9, 1, 10, 1], [9, 1, 12, 1], false).unwrap();
        assert_eq!(d.cell_input(0, 11, 1), "10");
        // Right, with the header's style: Q1's bold header to the next cell.
        d.fill(0, [0, 1, 0, 1], [0, 1, 0, 4], true).unwrap();
        assert_eq!(d.cell_input(0, 0, 4), "Q4");
        assert!(d.grid_cells(0, 0..1, 4..5)[0].2.bold);
        // One undo step each; refused two ways at once.
        assert!(d.undo().unwrap());
        assert_eq!(d.cell_input(0, 0, 4), "");
        assert!(d.fill(0, [9, 1, 10, 1], [9, 1, 12, 3], true).is_err());
        // Scattered cells in one undo step.
        let cells = vec![(30, 0, "a".to_string()), (32, 1, "b".to_string())];
        d.set_cell_list(0, &cells).unwrap();
        assert_eq!(
            (d.cell_input(0, 30, 0), d.cell_input(0, 32, 1)),
            ("a".into(), "b".into())
        );
        assert!(d.undo().unwrap());
        assert_eq!(
            (d.cell_input(0, 30, 0), d.cell_input(0, 32, 1)),
            (String::new(), String::new())
        );
        // The user's own list goes round.
        d.set_fill_lists(vec![vec!["Low".into(), "Mid".into(), "High".into()]]);
        d.set_cell(0, 20, 0, "Mid").unwrap();
        d.fill(0, [20, 0, 20, 0], [20, 0, 22, 0], true).unwrap();
        assert_eq!(
            (d.cell_input(0, 21, 0), d.cell_input(0, 22, 0)),
            ("High".into(), "Low".into())
        );
    }

    #[test]
    fn font_formatting() {
        let mut d = open("openpyxl-budget.xlsx");
        // A2:B3 bold, red on yellow, 14 point Arial; C9 (empty) too.
        let change = StyleChange {
            bold: Some(true),
            color: Some(Some([0xC0, 0, 0])),
            fill: Some(Some([0xFF, 0xFF, 0])),
            size: Some(14.0),
            face: Some("Arial".into()),
            ..StyleChange::default()
        };
        d.change_style(0, [1, 0, 2, 1], change.clone()).unwrap();
        d.change_style(0, [8, 2, 8, 2], change.clone()).unwrap();
        let c = d.grid_cells(0, 1..2, 1..2).remove(0).2;
        assert!(c.bold);
        assert_eq!(
            (c.color, c.fill),
            (Some([0xC0, 0, 0]), Some([0xFF, 0xFF, 0]))
        );
        assert_eq!((c.font_size, c.face.as_deref()), (Some(140), Some("Arial")));
        assert!(
            d.grid_cells(0, 8..9, 2..3)[0].2.bold,
            "an empty cell takes the style"
        );
        // The number format stays: B2 still shows 1,200.00.
        assert_eq!(c.text, "1,200.00");
        // The same change again makes no new style.
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-font-test.xlsx"), &saved).unwrap();
        }
        let mut wb = Workbook::open(saved).unwrap();
        let s = wb.sheet(0).unwrap().cells[&CellRef::new(1, 1)].style;
        assert!(wb.style(s).bold && wb.style(s).font.as_deref() == Some("Arial"));
        let xfs = |d: &mut Box<dyn ViewerDocument>| {
            let b = d.save().unwrap().bytes;
            let pkg = crate::package::Package::read(b).unwrap();
            String::from_utf8(pkg.part("xl/styles.xml").unwrap())
                .unwrap()
                .matches("<xf ")
                .count()
        };
        let before = xfs(&mut d);
        d.change_style(0, [1, 0, 2, 1], change).unwrap();
        assert_eq!(xfs(&mut d), before);
        // Bold off, then the whole change undone: as the file was.
        d.change_style(
            0,
            [1, 1, 1, 1],
            StyleChange {
                bold: Some(false),
                ..StyleChange::default()
            },
        )
        .unwrap();
        assert!(!d.grid_cells(0, 1..2, 1..2)[0].2.bold);
        for _ in 0..4 {
            assert!(d.undo().unwrap());
        }
        let c = d.grid_cells(0, 1..2, 1..2).remove(0).2;
        assert!(!c.bold && c.fill.is_none() && c.font_size.is_none());
    }

    #[test]
    fn alignment() {
        let mut d = open("openpyxl-budget.xlsx");
        // A2:B3 centered at the top; then B2 right, the top kept.
        d.change_style(
            0,
            [1, 0, 2, 1],
            StyleChange {
                align: Some(Align::Center),
                valign: Some(VAlign::Top),
                ..StyleChange::default()
            },
        )
        .unwrap();
        d.change_style(
            0,
            [1, 1, 1, 1],
            StyleChange {
                align: Some(Align::Right),
                ..StyleChange::default()
            },
        )
        .unwrap();
        let c = |d: &mut Box<dyn ViewerDocument>, r: u32, col: u32| {
            d.grid_cells(0, r..r + 1, col..col + 1).remove(0).2
        };
        let (a2, b2) = (c(&mut d, 1, 0), c(&mut d, 1, 1));
        assert_eq!((a2.align, a2.valign), (Align::Center, VAlign::Top));
        assert_eq!((b2.align, b2.valign), (Align::Right, VAlign::Top));
        // B2's number format stays.
        assert_eq!(b2.text, "1,200.00");
        // Written in the cell's <alignment>, as Excel reads it.
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        let s = wb.sheet(0).unwrap().cells[&CellRef::new(1, 0)].style;
        let st = wb.style(s);
        assert_eq!(
            (st.align.as_deref(), st.valign.as_deref()),
            (Some("center"), Some("top"))
        );
        // Middle, then General; each one undo step.
        d.change_style(
            0,
            [1, 0, 1, 0],
            StyleChange {
                align: Some(Align::General),
                valign: Some(VAlign::Middle),
                ..StyleChange::default()
            },
        )
        .unwrap();
        let a2 = c(&mut d, 1, 0);
        assert_eq!((a2.align, a2.valign), (Align::General, VAlign::Middle));
        for _ in 0..3 {
            assert!(d.undo().unwrap());
        }
        let a2 = c(&mut d, 1, 0);
        assert_eq!((a2.align, a2.valign), (Align::General, VAlign::Bottom));
    }

    #[test]
    fn borders() {
        use kalem_viewer::BorderSet;
        let mut d = open("openpyxl-budget.xlsx");
        let set = |d: &mut Box<dyn ViewerDocument>, r: [u32; 4], b, c| {
            d.change_style(
                0,
                r,
                StyleChange {
                    borders: Some((b, c)),
                    ..StyleChange::default()
                },
            )
            .unwrap();
        };
        let cell = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1)
                .into_iter()
                .next()
                .map(|c| c.2)
                .unwrap_or_default()
        };
        // Blue around B2:D4: its corners have two sides, its middle none.
        let blue = Some([0, 0, 0xFF]);
        set(&mut d, [1, 1, 3, 3], BorderSet::Outside, blue);
        let b2 = cell(&mut d, 1, 1);
        assert_eq!(b2.borders, [blue, None, None, blue]);
        let d4 = cell(&mut d, 3, 3);
        assert_eq!(d4.borders, [None, blue, blue, None]);
        assert_eq!(cell(&mut d, 2, 2).borders, [None; 4]);
        assert_eq!(b2.border_thick, [false; 4]);
        // The bottom of B2:D4 thick: D4 keeps its right side.
        set(&mut d, [1, 1, 3, 3], BorderSet::ThickOutside, None);
        let d4 = cell(&mut d, 3, 3);
        assert_eq!(d4.border_thick, [false, true, true, false]);
        assert_eq!(d4.borders, [None, Some([0, 0, 0]), Some([0, 0, 0]), None]);
        // Every side of A2, kept through saving as Excel reads it.
        set(&mut d, [1, 0, 1, 0], BorderSet::All, None);
        assert!(cell(&mut d, 1, 0).borders.iter().all(Option::is_some));
        set(&mut d, [5, 0, 5, 0], BorderSet::Bottom, blue);
        assert_eq!(cell(&mut d, 5, 0).borders, [None, None, blue, None]);
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-border-test.xlsx"), &saved).unwrap();
        }
        let mut wb = Workbook::open(saved).unwrap();
        let s = wb.sheet(0).unwrap().cells[&CellRef::new(3, 3)].style;
        assert_eq!(wb.style(s).sides[2], Some((0, true)));
        // None takes them away; each change one undo step.
        set(&mut d, [1, 0, 3, 3], BorderSet::None, None);
        assert_eq!(cell(&mut d, 3, 3).borders, [None; 4]);
        assert_eq!(cell(&mut d, 1, 0).borders, [None; 4]);
        for _ in 0..5 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(cell(&mut d, 1, 1).borders, [None; 4]);
        assert_eq!(cell(&mut d, 5, 0).borders, [None; 4]);
    }

    #[test]
    fn number_formats() {
        let mut d = open("openpyxl-budget.xlsx");
        let format = |d: &mut Box<dyn ViewerDocument>, r: [u32; 4], code: &str| {
            d.change_style(
                0,
                r,
                StyleChange {
                    number_format: Some(code.into()),
                    ..StyleChange::default()
                },
            )
            .unwrap();
        };
        let text = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1).remove(0).2.text
        };
        assert_eq!(d.cell_format(0, 1, 1).as_deref(), Some("#,##0.00"));
        // B2:C2 as a percent, scientific, one decimal, in lira.
        format(&mut d, [1, 1, 1, 2], "0%");
        assert_eq!(text(&mut d, 1, 1), "120000%");
        assert_eq!(d.cell_format(0, 1, 2).as_deref(), Some("0%"));
        format(&mut d, [1, 1, 1, 1], "0.00E+00");
        assert_eq!(text(&mut d, 1, 1), "1.20E+03");
        format(&mut d, [1, 1, 1, 1], "0.0");
        assert_eq!(text(&mut d, 1, 1), "1200.0");
        format(&mut d, [1, 1, 1, 1], "#,##0.00 \"₺\"");
        assert_eq!(text(&mut d, 1, 1), "1,200.00 ₺");
        // Kept through saving; the font and fill the cell had stay.
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        let s = wb.sheet(0).unwrap().cells[&CellRef::new(1, 1)].style;
        assert_eq!(wb.style(s).num_fmt, "#,##0.00 \"₺\"");
        for _ in 0..4 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(text(&mut d, 1, 1), "1,200.00");
    }

    #[test]
    fn range_numbers() {
        let mut d = open("openpyxl-budget.xlsx");
        // A1:D3: four headers and Rent, Food as text, six numbers.
        let (numbers, count) = d.range_numbers(0, [0, 0, 2, 3]);
        assert_eq!(count, 12);
        assert_eq!(numbers.len(), 6);
        // D2:D3 are formulas: their values count.
        let (n, _) = d.range_numbers(0, [1, 3, 2, 3]);
        assert_eq!(n.iter().sum::<f64>(), 2400.0 + 943.75);
        // Empty cells are not counted; a whole column is quick.
        assert_eq!(d.range_numbers(0, [10, 10, 20, 20]), (Vec::new(), 0));
        assert_eq!(d.range_numbers(0, [0, 1, 1_048_575, 1]).1, 5);
    }

    #[test]
    fn sheets_edited() {
        use kalem_viewer::SheetEdit;
        let mut d = open("openpyxl-budget.xlsx");
        let names = |d: &mut Box<dyn ViewerDocument>| -> Vec<String> {
            d.structure().units.into_iter().map(|u| u.label).collect()
        };
        let first = names(&mut d);
        assert_eq!(first[0], "Budget");
        // A formula on another sheet naming Budget follows its new name.
        d.edit_sheets(SheetEdit::Insert(1)).unwrap();
        let n = names(&mut d);
        assert_eq!(n.len(), first.len() + 1);
        assert_eq!(n[1], "Sheet1");
        d.set_cell(1, 0, 0, "=Budget!D2*2").unwrap();
        d.edit_sheets(SheetEdit::Rename(0, "Bütçe 2026".into()))
            .unwrap();
        assert_eq!(d.cell_input(1, 0, 0), "='Bütçe 2026'!D2*2");
        assert_eq!(d.grid_cells(1, 0..1, 0..1)[0].2.text, "4800");
        // Names Excel refuses.
        for bad in ["", "a/b", "Sheet1", "'x", &"x".repeat(32)] {
            assert!(
                d.edit_sheets(SheetEdit::Rename(0, bad.into())).is_err(),
                "{bad}"
            );
        }
        // Moved last: the formula still finds it.
        let last = names(&mut d).len() - 1;
        d.edit_sheets(SheetEdit::Move(0, last)).unwrap();
        assert_eq!(names(&mut d)[last], "Bütçe 2026");
        assert_eq!(d.grid_cells(0, 0..1, 0..1)[0].2.text, "4800");
        // Hidden and shown again; the last visible sheet cannot be hidden.
        let hidden = d.hidden_units();
        d.edit_sheets(SheetEdit::Hide(0, true)).unwrap();
        assert!(d.hidden_units().contains(&0));
        d.edit_sheets(SheetEdit::Hide(0, false)).unwrap();
        assert_eq!(d.hidden_units(), hidden);
        // Deleted: the formula naming it becomes #REF!.
        d.edit_sheets(SheetEdit::Delete(last)).unwrap();
        assert_eq!(names(&mut d).len(), first.len());
        assert_eq!(d.cell_input(0, 0, 0), "=#REF!D2*2");
        // Saved and read again as Excel would; each change undone in turn.
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-sheets-test.xlsx"), &saved).unwrap();
        }
        let pkg = crate::package::Package::read(saved.clone()).unwrap();
        assert!(!pkg.contains("xl/calcChain.xml"));
        let wb = Workbook::open(saved).unwrap();
        assert_eq!(wb.sheets()[0].name, "Sheet1");
        for _ in 0..7 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(names(&mut d), first);
        assert_eq!(d.grid_cells(0, 1..2, 3..4)[0].2.text, "2,400.00");
    }

    #[test]
    fn rows_and_columns_hidden() {
        let mut d = open("openpyxl-budget.xlsx");
        let layout = |d: &mut Box<dyn ViewerDocument>| d.grid(0).unwrap();
        // Rows 2-3, and rows 20-21 the part has not got yet.
        d.set_hidden(0, true, 1, 2, true).unwrap();
        d.set_hidden(0, true, 19, 20, true).unwrap();
        assert_eq!(layout(&mut d).hidden_rows, vec![1, 2, 19, 20]);
        // Columns B:C, then C shown again.
        d.set_hidden(0, false, 1, 2, true).unwrap();
        assert_eq!(layout(&mut d).hidden_cols, vec![1, 2]);
        d.set_hidden(0, false, 2, 2, false).unwrap();
        assert_eq!(layout(&mut d).hidden_cols, vec![1]);
        // Saved as Excel reads it; the values stay.
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-hidden-test.xlsx"), &saved).unwrap();
        }
        let mut wb = Workbook::open(saved).unwrap();
        let sheet = wb.sheet(0).unwrap();
        assert!(sheet.rows[&1].hidden && sheet.rows[&20].hidden);
        assert_eq!(d.grid_cells(0, 1..2, 3..4)[0].2.text, "2,400.00");
        // Rows shown again; each change one undo step.
        d.set_hidden(0, true, 0, 30, false).unwrap();
        assert!(layout(&mut d).hidden_rows.is_empty());
        for _ in 0..5 {
            assert!(d.undo().unwrap());
        }
        let l = layout(&mut d);
        assert!(l.hidden_rows.is_empty() && l.hidden_cols.is_empty());
    }

    #[test]
    fn panes_frozen() {
        let mut d = open("openpyxl-budget.xlsx");
        let frozen = |d: &mut Box<dyn ViewerDocument>| d.grid(0).unwrap().frozen;
        // The budget's header row is frozen already.
        let first = frozen(&mut d);
        assert_eq!(first, (1, 0));
        d.set_frozen(0, 0, 3).unwrap();
        assert_eq!(frozen(&mut d), (0, 3));
        d.set_frozen(0, 2, 1).unwrap();
        assert_eq!(frozen(&mut d), (2, 1));
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-frozen-test.xlsx"), &saved).unwrap();
        }
        let pkg = crate::package::Package::read(saved).unwrap();
        let sheet = String::from_utf8(pkg.part("xl/worksheets/sheet1.xml").unwrap()).unwrap();
        assert_eq!(sheet.matches("<pane ").count(), 1, "{sheet}");
        assert!(sheet.contains(r#"<pane xSplit="1" ySplit="2" topLeftCell="B3" activePane="bottomRight" state="frozen"/>"#), "{sheet}");
        d.set_frozen(0, 0, 0).unwrap();
        assert_eq!(frozen(&mut d), (0, 0));
        for _ in 0..3 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(frozen(&mut d), first);
    }

    #[test]
    fn notes_written() {
        use kalem_viewer::SheetEdit;
        let mut d = open("openpyxl-budget.xlsx");
        let first = d.cell_note(0, 1, 0);
        assert!(first.is_some());
        // Edited, added beside it, then the first taken away.
        d.set_note(0, 1, 0, Some("Paid <monthly> & on time".into()))
            .unwrap();
        assert_eq!(
            d.cell_note(0, 1, 0).as_deref(),
            Some("Paid <monthly> & on time")
        );
        d.set_note(0, 2, 2, Some("Check".into())).unwrap();
        assert_eq!(d.cell_note(0, 2, 2).as_deref(), Some("Check"));
        assert!(d.grid_cells(0, 2..3, 2..3)[0].2.note);
        d.set_note(0, 1, 0, None).unwrap();
        assert!(d.cell_note(0, 1, 0).is_none());
        // A sheet without notes gets its comments part and drawing.
        d.edit_sheets(SheetEdit::Insert(1)).unwrap();
        d.set_note(1, 4, 1, Some("Yeni not".into())).unwrap();
        assert_eq!(d.cell_note(1, 4, 1).as_deref(), Some("Yeni not"));
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-notes-test.xlsx"), &saved).unwrap();
        }
        let wb = Workbook::open(saved.clone()).unwrap();
        let notes = wb.comments(1).unwrap();
        assert_eq!(
            (notes[0].cell, notes[0].text.as_str()),
            (CellRef::new(4, 1), "Yeni not")
        );
        let pkg = crate::package::Package::read(saved).unwrap();
        let sheet = String::from_utf8(pkg.part(&wb.sheets()[1].part).unwrap()).unwrap();
        assert!(sheet.contains("<legacyDrawing r:id="), "{sheet}");
        // Each change one undo step.
        for _ in 0..5 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(d.cell_note(0, 1, 0), first);
        assert!(d.cell_note(0, 2, 2).is_none());
    }

    #[test]
    fn paste_special() {
        use kalem_viewer::PasteKind;
        let mut d = open("openpyxl-budget.xlsx");
        let text = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1)
                .first()
                .map(|x| x.2.text.clone())
                .unwrap_or_default()
        };
        let d2 = d.cell_input(0, 1, 3);
        assert!(d2.starts_with('='), "{d2}");
        // Values: D2's result, not its formula.
        d.paste_cells((0, [1, 1, 1, 3]), (0, 9, 5), PasteKind::Values, false)
            .unwrap();
        assert_eq!(d.cell_input(0, 9, 7), "2400");
        // Formulas: D2's, moved down eight rows (to empty cells: 0).
        d.paste_cells((0, [1, 3, 1, 3]), (0, 9, 3), PasteKind::Formulas, false)
            .unwrap();
        let moved = d.cell_input(0, 9, 3);
        assert!(moved.contains("10") && !moved.contains('2'), "{moved}");
        // Formats: B2's number format on F12.
        d.paste_cells((0, [1, 1, 1, 1]), (0, 11, 5), PasteKind::Formats, false)
            .unwrap();
        d.set_cell(0, 11, 5, "5").unwrap();
        assert_eq!(text(&mut d, 11, 5), "5.00");
        // Transposed: the header row down column A from A20.
        d.paste_cells((0, [0, 0, 0, 3]), (0, 19, 0), PasteKind::All, true)
            .unwrap();
        let col: Vec<String> = (19..23).map(|r| text(&mut d, r, 0)).collect();
        assert_eq!(col, ["Item", "Q1", "Q2", "Total"]);
        // Each paste one undo step.
        for _ in 0..5 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(d.cell_input(0, 9, 7), "");
        assert_eq!(text(&mut d, 19, 0), "");
    }

    #[test]
    fn clear_and_paint_formats() {
        let mut d = open("openpyxl-budget.xlsx");
        let text = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1)
                .first()
                .map(|x| x.2.clone())
                .unwrap_or_default()
        };
        // B2's format cleared: the number shows as General, still there.
        assert_eq!(text(&mut d, 1, 1).text, "1,200.00");
        d.clear_range(0, [1, 1, 1, 1], false, true).unwrap();
        assert_eq!(text(&mut d, 1, 1).text, "1200");
        // Painted back from B3 over B2:C2.
        d.fill_formats((0, [2, 1, 2, 1]), (0, [1, 1, 1, 2]))
            .unwrap();
        assert_eq!(text(&mut d, 1, 1).text, "1,200.00");
        // A header's bold painted over an empty cell, which keeps it.
        d.fill_formats((0, [0, 0, 0, 0]), (0, [9, 9, 9, 9]))
            .unwrap();
        d.set_cell(0, 9, 9, "x").unwrap();
        assert!(text(&mut d, 9, 9).bold);
        // Clear All: A2's value, format and note gone.
        assert!(d.cell_note(0, 1, 0).is_some());
        d.clear_range(0, [1, 0, 1, 0], true, true).unwrap();
        assert_eq!(d.cell_input(0, 1, 0), "");
        assert!(d.cell_note(0, 1, 0).is_none());
        // One undo step each.
        for _ in 0..5 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(text(&mut d, 1, 1).text, "1,200.00");
        assert!(d.cell_note(0, 1, 0).is_some());
    }

    #[test]
    fn duplicates_removed() {
        let mut d = open("openpyxl-budget.xlsx");
        // Rows 7-11: three kinds, one repeated twice in another case.
        for (r, row) in [
            ["Kira", "10"],
            ["Yemek", "20"],
            ["kira", "10"],
            ["Yol", "30"],
            ["KIRA", "11"],
        ]
        .iter()
        .enumerate()
        {
            for (c, v) in row.iter().enumerate() {
                d.set_cell(0, 6 + r as u32, 6 + c as u32, v).unwrap();
            }
        }
        // By the name only: the two other "kira" rows go.
        let removed = d.remove_duplicates(0, [6, 6, 10, 7], &[6], false).unwrap();
        assert_eq!(removed, 2);
        let col: Vec<String> = (6..11).map(|r| d.cell_input(0, r, 6)).collect();
        assert_eq!(col, ["Kira", "Yemek", "Yol", "", ""]);
        assert_eq!(d.cell_input(0, 8, 7), "30");
        assert!(d.undo().unwrap());
        // By both columns: only the exact repeat goes.
        let removed = d.remove_duplicates(0, [6, 6, 10, 7], &[], false).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(d.cell_input(0, 9, 6), "KIRA");
        assert_eq!(d.remove_duplicates(0, [6, 6, 9, 7], &[], false).unwrap(), 0);
    }

    #[test]
    fn hyperlinks() {
        let mut d = open("openpyxl-budget.xlsx");
        // An address on an empty cell: its text, drawn as a link.
        d.set_link(0, 9, 1, Some("https://kalem.app/?a=1&b=2".into()))
            .unwrap();
        assert_eq!(
            d.cell_link(0, 9, 1).as_deref(),
            Some("https://kalem.app/?a=1&b=2")
        );
        let c = d.grid_cells(0, 9..10, 1..2).remove(0).2;
        assert_eq!(c.text, "https://kalem.app/?a=1&b=2");
        assert!(c.underline && c.color == Some([0x05, 0x63, 0xC1]));
        // A place in the workbook on A2, its text kept.
        d.set_link(0, 1, 0, Some("#Dates!A1".into())).unwrap();
        assert_eq!(d.cell_link(0, 1, 0).as_deref(), Some("#Dates!A1"));
        assert_eq!(d.grid_cells(0, 1..2, 0..1)[0].2.text, "Rent");
        // Changed, then taken away with its look.
        d.set_link(0, 9, 1, Some("mailto:a@b.c".into())).unwrap();
        assert_eq!(d.cell_link(0, 9, 1).as_deref(), Some("mailto:a@b.c"));
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-links-test.xlsx"), &saved).unwrap();
        }
        let pkg = crate::package::Package::read(saved).unwrap();
        let rels =
            String::from_utf8(pkg.part("xl/worksheets/_rels/sheet1.xml.rels").unwrap()).unwrap();
        assert_eq!(rels.matches("TargetMode=\"External\"").count(), 1, "{rels}");
        d.set_link(0, 1, 0, None).unwrap();
        assert!(d.cell_link(0, 1, 0).is_none());
        assert!(!d.grid_cells(0, 1..2, 0..1)[0].2.underline);
        for _ in 0..4 {
            assert!(d.undo().unwrap());
        }
        assert!(d.cell_link(0, 9, 1).is_none() && d.cell_link(0, 1, 0).is_none());
    }

    #[test]
    fn names_defined() {
        let mut d = open("openpyxl-budget.xlsx");
        let before = d.defined_names();
        // A name for B2:B4, used by a formula.
        d.set_defined_name("Q1Values", Some("Budget!$B$2:$B$4"))
            .unwrap();
        assert!(
            d.defined_names()
                .contains(&("Q1Values".into(), "Budget!$B$2:$B$4".into()))
        );
        d.set_cell(0, 9, 1, "=SUM(Q1Values)").unwrap();
        assert_eq!(d.grid_cells(0, 9..10, 1..2)[0].2.text, "1631.5");
        // Names Excel refuses.
        for bad in ["A1", "R1C1", "1x", "a b", "c"] {
            assert!(
                d.set_defined_name(bad, Some("Budget!$A$1")).is_err(),
                "{bad}"
            );
        }
        // Defined again elsewhere, then deleted.
        d.set_defined_name("q1values", Some("Budget!$B$2")).unwrap();
        assert_eq!(d.grid_cells(0, 9..10, 1..2)[0].2.text, "1200");
        let saved = d.save().unwrap().bytes;
        let wb = Workbook::open(saved).unwrap();
        assert!(wb.defined_names().iter().any(|n| n.name == "q1values"));
        d.set_defined_name("Q1VALUES", None).unwrap();
        assert_eq!(d.defined_names(), before);
        assert!(d.set_defined_name("Q1VALUES", None).is_err());
        for _ in 0..4 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(d.defined_names(), before);
    }

    #[test]
    fn calculate_now() {
        let mut d = open("openpyxl-budget.xlsx");
        d.set_cell(0, 9, 1, "=RAND()").unwrap();
        let first = d.cell_input(0, 9, 1);
        assert_eq!(first, "=RAND()");
        let value =
            |d: &mut Box<dyn ViewerDocument>| d.grid_cells(0, 9..10, 1..2)[0].2.text.clone();
        let a = value(&mut d);
        d.recalculate().unwrap();
        let b = value(&mut d);
        assert_ne!(a, b, "a new random number");
        // Other results stay.
        assert_eq!(d.grid_cells(0, 1..2, 3..4)[0].2.text, "2,400.00");
    }

    #[test]
    fn entered_in_a_range() {
        let mut d = open("openpyxl-budget.xlsx");
        // Typed in F2 with F2:G3 selected: each cell's own references.
        d.enter_in_range(0, [1, 5, 2, 6], (1, 5), "=B2*2").unwrap();
        assert_eq!(d.cell_input(0, 1, 5), "=B2*2");
        assert_eq!(d.cell_input(0, 2, 6), "=C3*2");
        assert_eq!(d.grid_cells(0, 2..3, 6..7)[0].2.text, "1024.5");
        d.enter_in_range(0, [8, 0, 9, 0], (8, 0), "Yok").unwrap();
        assert_eq!(d.cell_input(0, 9, 0), "Yok");
        assert!(d.undo().unwrap());
        assert!(d.undo().unwrap());
        assert_eq!(d.cell_input(0, 1, 5), "");
    }

    #[test]
    fn line_breaks_wrap() {
        let mut d = open("openpyxl-budget.xlsx");
        d.set_cell(0, 9, 0, "Kira\nOcak").unwrap();
        let c = d.grid_cells(0, 9..10, 0..1).remove(0).2;
        assert!(c.wrap && c.text == "Kira\nOcak", "{c:?}");
        assert!(d.undo().unwrap());
        assert_eq!(d.cell_input(0, 9, 0), "");
    }

    #[test]
    fn custom_sort_and_filters() {
        use kalem_viewer::{FilterOp, FilterRule, SortKey};
        let mut d = open("openpyxl-budget.xlsx");
        // A table at F1:G6: month and amount.
        let rows = [
            ["Ay", "Tutar"],
            ["Mart", "30"],
            ["Ocak", "10"],
            ["Şubat", "20"],
            ["Ocak", "5"],
            ["Mart", "40"],
        ];
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                d.set_cell(0, r as u32, 8 + c as u32, v).unwrap();
            }
        }
        let col = |d: &mut Box<dyn ViewerDocument>, c: u32| -> Vec<String> {
            (1..6).map(|r| d.cell_input(0, r, c)).collect()
        };
        // By month in the calendar's order, then amount largest first.
        let months: Vec<String> = ["Ocak", "Şubat", "Mart"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let keys = [
            SortKey {
                col: 8,
                descending: false,
                list: Some(months),
                color: None,
            },
            SortKey {
                col: 9,
                descending: true,
                list: None,
                color: None,
            },
        ];
        d.sort_range_by(0, [0, 8, 5, 9], &keys, true).unwrap();
        assert_eq!(col(&mut d, 8), ["Ocak", "Ocak", "Şubat", "Mart", "Mart"]);
        assert_eq!(col(&mut d, 9), ["10", "5", "20", "40", "30"]);
        // A filter: amounts over 15 and under 35.
        d.set_filter(0, Some([0, 8, 5, 9])).unwrap();
        let rule = FilterRule::Custom {
            first: (FilterOp::Greater, "15".into()),
            second: Some((true, FilterOp::Less, "35".into())),
        };
        d.filter_column_by(0, 9, Some(rule.clone())).unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![1, 2, 4]);
        assert_eq!(d.column_filter(0, 9), Some(rule));
        // Months beginning with "o"; the top two amounts; above the average.
        d.filter_column_by(0, 9, None).unwrap();
        let begins = FilterRule::Custom {
            first: (FilterOp::BeginsWith, "o".into()),
            second: None,
        };
        d.filter_column_by(0, 8, Some(begins.clone())).unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![3, 4, 5]);
        assert_eq!(d.column_filter(0, 8), Some(begins));
        d.filter_column_by(0, 8, None).unwrap();
        d.filter_column_by(
            0,
            9,
            Some(FilterRule::Top {
                count: 2,
                percent: false,
                bottom: false,
            }),
        )
        .unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![1, 2, 3]);
        d.filter_column_by(0, 9, Some(FilterRule::Average { above: true }))
            .unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![1, 2, 3]);
        // Reapply after an edit: 5 becomes 50.
        d.set_cell(0, 2, 9, "50").unwrap();
        d.reapply_filter(0).unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![1, 3, 5]);
        // A fill color.
        d.change_style(
            0,
            [3, 9, 3, 9],
            StyleChange {
                fill: Some(Some([0xFF, 0xFF, 0])),
                ..StyleChange::default()
            },
        )
        .unwrap();
        d.filter_column_by(0, 9, Some(FilterRule::Fill([0xFF, 0xFF, 0])))
            .unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![1, 2, 4, 5]);
        assert_eq!(
            d.column_filter(0, 9),
            Some(FilterRule::Fill([0xFF, 0xFF, 0]))
        );
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-filters-test.xlsx"), &saved).unwrap();
        }
    }

    #[test]
    fn tables_made() {
        let mut d = open("openpyxl-budget.xlsx");
        // A1:D4 a table: its headers, banding, a name.
        let name = d.create_table(0, [0, 0, 3, 3], true, "").unwrap();
        assert_eq!(name, "Table1");
        let t = d.tables(0);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].range, [0, 0, 3, 3]);
        let a1 = d.grid_cells(0, 0..1, 0..1).remove(0).2;
        assert!(a1.bold && a1.fill == Some([0x44, 0x72, 0xC4]), "{a1:?}");
        assert_eq!(
            d.grid_cells(0, 1..2, 2..3)[0].2.fill,
            Some([0xD9, 0xE1, 0xF2])
        );
        // A structured reference computes.
        d.set_cell(0, 9, 9, "=SUM(Table1[Q1])").unwrap();
        assert_eq!(d.grid_cells(0, 9..10, 9..10)[0].2.text, "1631.5");
        // A row typed under it: it grows, the sum with it.
        d.set_cell(0, 4, 0, "Extra").unwrap();
        assert_eq!(d.tables(0)[0].range, [0, 0, 4, 3]);
        d.set_cell(0, 4, 1, "100").unwrap();
        assert_eq!(d.grid_cells(0, 9..10, 9..10)[0].2.text, "1731.5");
        // A total row; then taken away.
        d.set_cell(0, 5, 0, "").unwrap();
        let err = d.set_table_totals(0, "Table1", true);
        assert!(err.is_ok(), "{err:?}");
        assert_eq!(d.cell_input(0, 5, 0), "Total");
        assert_eq!(d.cell_input(0, 5, 3), "=SUBTOTAL(109,Table1[Total])");
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-table-test.xlsx"), &saved).unwrap();
        }
        d.set_table_totals(0, "Table1", false).unwrap();
        assert_eq!(d.cell_input(0, 5, 0), "");
        // Turned into a range: the formula's reference made plain.
        d.remove_table(0, "Table1").unwrap();
        assert!(d.tables(0).is_empty());
        assert_eq!(d.cell_input(0, 9, 9), "=SUM($B$2:$B$5)");
        assert_eq!(d.grid_cells(0, 9..10, 9..10)[0].2.text, "1731.5");
    }

    #[test]
    fn page_set_up() {
        use kalem_viewer::PageSetup;
        let mut d = open("openpyxl-budget.xlsx");
        let first = d.page_setup(0).unwrap();
        const PNG: [u8; 69] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92,
            0xEF, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let s = PageSetup {
            landscape: true,
            paper: 8,
            margins: [0.25, 0.25, 0.5, 0.5],
            fit: Some((1, 0)),
            scale: 100,
            print_area: Some([0, 0, 4, 3]),
            title_rows: Some((0, 0)),
            title_cols: Some((0, 1)),
            header: "&L&G&C&A".into(),
            footer: "&CPage &P of &N".into(),
            row_breaks: vec![3],
            col_breaks: vec![2],
            gridlines: true,
            headings: true,
            pictures: vec![kalem_viewer::HeaderPicture {
                place: "LH".into(),
                data: PNG.to_vec(),
                size: (36.0, 18.0),
            }],
        };
        d.set_page_setup(0, &s).unwrap();
        assert_eq!(d.page_setup(0).unwrap(), s);
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-page-test.xlsx"), &saved).unwrap();
        }
        let mut wb = Workbook::open(saved).unwrap();
        let names: Vec<&str> = wb.defined_names().iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"_xlnm.Print_Area") && names.contains(&"_xlnm.Print_Titles"));
        assert_eq!(wb.page_setup(0).unwrap(), s);
        // Scaled to a percentage, not fitted.
        let half = PageSetup {
            fit: None,
            scale: 50,
            ..s.clone()
        };
        d.set_page_setup(0, &half).unwrap();
        assert_eq!(d.page_setup(0).unwrap(), half);
        assert!(d.undo().unwrap());
        // Back to none of it; undone.
        d.set_page_setup(0, &PageSetup::default()).unwrap();
        assert_eq!(d.page_setup(0).unwrap().print_area, None);
        assert!(d.undo().unwrap() && d.undo().unwrap());
        assert_eq!(d.page_setup(0).unwrap(), first);
    }

    #[test]
    fn outlines_and_subtotals() {
        let mut d = open("openpyxl-budget.xlsx");
        // Rows 2-4 grouped, then 3 one deeper; columns B:C grouped.
        d.set_outline(0, true, 1, 3, true).unwrap();
        d.set_outline(0, true, 2, 2, true).unwrap();
        d.set_outline(0, false, 1, 2, true).unwrap();
        let (rows, cols) = d.outline(0);
        assert_eq!(rows, vec![(1, 1), (2, 2), (3, 1)]);
        assert_eq!(cols, vec![(1, 1), (2, 1)]);
        // Collapsed from its summary row 5, then shown again.
        d.set_detail_shown(0, true, 4, false).unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_rows, vec![1, 2, 3]);
        d.set_detail_shown(0, true, 4, true).unwrap();
        assert!(d.grid(0).unwrap().hidden_rows.is_empty());
        d.set_detail_shown(0, false, 1, false).unwrap();
        assert_eq!(d.grid(0).unwrap().hidden_cols, vec![1, 2]);
        // Ungrouped one level.
        d.set_outline(0, true, 1, 3, false).unwrap();
        assert_eq!(d.outline(0).0, vec![(2, 1)]);
        for _ in 0..7 {
            assert!(d.undo().unwrap());
        }
        assert!(d.outline(0).0.is_empty());
        // A subtotal of a small table by its first column.
        for (r, row) in [
            ["Ay", "Tutar"],
            ["Ocak", "10"],
            ["Ocak", "5"],
            ["Şubat", "20"],
        ]
        .iter()
        .enumerate()
        {
            for (c, v) in row.iter().enumerate() {
                d.set_cell(0, 20 + r as u32, 8 + c as u32, v).unwrap();
            }
        }
        d.subtotal(0, [20, 8, 23, 9], 8, 9, &[9]).unwrap();
        let col: Vec<String> = (21..27).map(|r| d.cell_input(0, r, 8)).collect();
        assert_eq!(
            col,
            [
                "Ocak",
                "Ocak",
                "Ocak Total",
                "Şubat",
                "Şubat Total",
                "Grand Total"
            ]
        );
        assert_eq!(d.cell_input(0, 23, 9), "=SUBTOTAL(9,J22:J23)");
        let shown = |d: &mut Box<dyn ViewerDocument>, r: u32| {
            d.grid_cells(0, r..r + 1, 9..10)[0].2.text.clone()
        };
        assert_eq!(
            (shown(&mut d, 23), shown(&mut d, 25), shown(&mut d, 26)),
            ("15".into(), "20".into(), "35".into())
        );
        let (rows, _) = d.outline(0);
        assert_eq!(rows, vec![(21, 2), (22, 2), (23, 1), (24, 2), (25, 1)]);
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-outline-test.xlsx"), &saved).unwrap();
        }
    }

    #[test]
    fn indent_rotation_shrink_across() {
        let mut d = open("openpyxl-budget.xlsx");
        let cell = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1).remove(0).2
        };
        d.change_style(
            0,
            [1, 0, 1, 0],
            StyleChange {
                indent: Some(2),
                ..StyleChange::default()
            },
        )
        .unwrap();
        let a2 = cell(&mut d, 1, 0);
        assert_eq!((a2.indent, a2.align), (2, Align::Left));
        d.change_style(
            0,
            [2, 0, 2, 0],
            StyleChange {
                rotation: Some(45),
                shrink: Some(true),
                ..StyleChange::default()
            },
        )
        .unwrap();
        let a3 = cell(&mut d, 2, 0);
        assert_eq!((a3.rotation, a3.shrink), (45, true));
        d.change_style(
            0,
            [0, 0, 0, 2],
            StyleChange {
                center_across: Some(true),
                ..StyleChange::default()
            },
        )
        .unwrap();
        assert!(cell(&mut d, 0, 0).center_across);
        let mut wb = Workbook::open(d.save().unwrap().bytes).unwrap();
        let s = wb.sheet(0).unwrap().cells[&CellRef::new(2, 0)].style;
        assert_eq!((wb.style(s).rotation, wb.style(s).shrink), (45, true));
        for _ in 0..3 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(cell(&mut d, 1, 0).indent, 0);
    }

    #[test]
    fn protection() {
        use kalem_viewer::{SheetEdit, SheetProtection};
        let mut d = open("openpyxl-budget.xlsx");
        // B2:B3 unlocked; the sheet protected with a password, sorting
        // allowed.
        d.change_style(
            0,
            [1, 1, 2, 1],
            StyleChange {
                locked: Some(false),
                ..StyleChange::default()
            },
        )
        .unwrap();
        assert!(d.grid_cells(0, 1..2, 1..2)[0].2.unlocked);
        let p = SheetProtection {
            sort: true,
            ..SheetProtection::default()
        };
        d.protect_sheet(0, Some(p), Some("gizli")).unwrap();
        let read = d.sheet_protection(0).unwrap();
        assert!(read.has_password && read.sort && !read.format_cells);
        // Locked cells, formats and rows refused; unlocked cells edited.
        assert!(d.set_cell(0, 1, 0, "x").is_err());
        assert!(d.set_cell(0, 1, 1, "5").is_ok());
        assert!(
            d.change_style(
                0,
                [1, 1, 1, 1],
                StyleChange {
                    bold: Some(true),
                    ..StyleChange::default()
                }
            )
            .is_err()
        );
        assert!(
            d.grid_edit(0, GridEdit::InsertRows { at: 1, count: 1 })
                .is_err()
        );
        // Saved: Excel's SHA-512 protection.
        let saved = d.save().unwrap().bytes;
        let pkg = crate::package::Package::read(saved).unwrap();
        let text = String::from_utf8(pkg.part("xl/worksheets/sheet1.xml").unwrap()).unwrap();
        assert!(
            text.contains("algorithmName=\"SHA-512\"") && text.contains("sort=\"0\""),
            "{text}"
        );
        // The wrong password refused, the right one unprotects.
        assert!(d.protect_sheet(0, None, Some("yanlış")).is_err());
        d.protect_sheet(0, None, Some("gizli")).unwrap();
        assert!(d.sheet_protection(0).is_none());
        assert!(d.set_cell(0, 1, 0, "x").is_ok());
        // The workbook's structure.
        d.protect_workbook(true, None).unwrap();
        assert!(d.workbook_protected());
        assert!(d.edit_sheets(SheetEdit::Insert(0)).is_err());
        d.protect_workbook(false, None).unwrap();
        assert!(d.edit_sheets(SheetEdit::Insert(0)).is_ok());
    }

    #[test]
    fn formulas_evaluated() {
        let mut d = open("openpyxl-budget.xlsx");
        let out = d.evaluate_formulas(
            0,
            &[
                "B2+1".into(),
                "A2".into(),
                "1/0".into(),
                "B2>1000".into(),
                "B9".into(),
            ],
        );
        assert_eq!(
            out,
            vec![
                Some("1201".into()),
                Some("\"Rent\"".into()),
                Some("#DIV/0!".into()),
                Some("TRUE".into()),
                Some("0".into())
            ]
        );
    }

    #[test]
    fn batches_undo_as_one() {
        let mut d = open("openpyxl-budget.xlsx");
        d.begin_batch();
        d.set_cell(0, 9, 0, "a").unwrap();
        d.set_cell(0, 9, 1, "b").unwrap();
        d.change_style(
            0,
            [9, 0, 9, 1],
            StyleChange {
                bold: Some(true),
                ..StyleChange::default()
            },
        )
        .unwrap();
        d.end_batch();
        assert!(d.undo().unwrap());
        assert_eq!(
            (d.cell_input(0, 9, 0), d.cell_input(0, 9, 1)),
            (String::new(), String::new())
        );
        // A conditional format's range is among the sheet's.
        let rule = CondRule::Formula("=TRUE".into());
        let style = CondStyle {
            fill: Some([1, 2, 3]),
            color: None,
            bold: false,
        };
        d.add_conditional_format(0, [2, 2, 5, 3], rule, style)
            .unwrap();
        assert!(d.conditional_ranges(0).contains(&[2, 2, 5, 3]));
    }

    #[test]
    fn pictures_and_shapes() {
        use kalem_viewer::DrawingKind;
        let mut d = open("openpyxl-budget.xlsx");
        let charts = d.charts(0).len();
        // A PNG of one pixel.
        let png: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0,
            0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 13, 0x49, 0x44,
            0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0, 5, 0, 1, 0xFF, 0x89,
            0x99, 0x3D, 0x1D, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        d.insert_picture(0, [10, 1, 14, 3], &png, "png").unwrap();
        d.insert_shape(0, [10, 5, 12, 7], "ellipse", "Hedef\n2026", false)
            .unwrap();
        d.insert_shape(0, [15, 1, 16, 4], "rect", "Not: taslak", true)
            .unwrap();
        let all = d.drawings(0);
        assert_eq!(all.len(), 3);
        assert_eq!(
            (all[0].kind.clone(), all[0].anchor),
            (DrawingKind::Picture, [10, 1, 14, 3])
        );
        assert_eq!(d.drawing_image(0, 0).unwrap(), png);
        let DrawingKind::Shape {
            preset,
            text,
            text_box,
            fill,
            ..
        } = all[1].kind.clone()
        else {
            panic!()
        };
        assert_eq!(
            (preset.as_str(), text.as_str(), text_box, fill),
            ("ellipse", "Hedef\n2026", false, Some([0x44, 0x72, 0xC4]))
        );
        // Moved, its text changed; the picture deleted.
        d.move_drawing(0, 1, [20, 5, 23, 8]).unwrap();
        d.set_shape_text(0, 1, "Yeni").unwrap();
        let s = &d.drawings(0)[1];
        assert_eq!(s.anchor, [20, 5, 23, 8]);
        assert!(matches!(&s.kind, DrawingKind::Shape { text, .. } if text == "Yeni"));
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-pictures-test.xlsx"), &saved).unwrap();
        }
        d.delete_drawing(0, 0).unwrap();
        assert_eq!(d.drawings(0).len(), 2);
        // The charts are still the charts.
        assert_eq!(d.charts(0).len(), charts);
        for _ in 0..6 {
            assert!(d.undo().unwrap());
        }
        assert!(d.drawings(0).is_empty());
    }

    #[test]
    fn sparklines() {
        use kalem_viewer::SparklineKind;
        let mut d = open("openpyxl-budget.xlsx");
        let rows: Vec<Vec<String>> = [["3", "-1", "4", "1"], ["2", "7", "1", "8"]]
            .iter()
            .map(|r| r.iter().map(|v| v.to_string()).collect())
            .collect();
        d.set_cells(0, 30, 0, &rows).unwrap();
        // A line for each row in the column on its right, high and low
        // marked; win/loss for the first row in one cell.
        d.add_sparklines(0, [30, 0, 31, 3], [30, 4, 31, 4], SparklineKind::Line, true)
            .unwrap();
        d.add_sparklines(
            0,
            [30, 0, 30, 3],
            [33, 0, 33, 0],
            SparklineKind::WinLoss,
            false,
        )
        .unwrap();
        let cells = d.grid_cells(0, 30..34, 0..5);
        let at = |r, c| {
            cells
                .iter()
                .find(|(a, b, _)| (*a, *b) == (r, c))
                .and_then(|(_, _, g)| g.sparkline.clone())
                .unwrap()
        };
        let first = at(30, 4);
        assert_eq!(first.kind, SparklineKind::Line);
        assert_eq!(
            first.points,
            vec![Some(800), Some(0), Some(1000), Some(400)]
        );
        assert_eq!(
            (first.high, first.low, first.zero),
            (Some(2), Some(1), Some(200))
        );
        assert_eq!(at(31, 4).points[3], Some(1000));
        let wl = at(33, 0);
        assert_eq!(
            (wl.kind, wl.points),
            (
                SparklineKind::WinLoss,
                vec![Some(1000), Some(0), Some(1000), Some(1000)]
            )
        );
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-sparklines-test.xlsx"), &saved).unwrap();
        }
        // Read back from the saved file.
        let mut back = crate::Workbook::open(saved).unwrap();
        assert_eq!(back.sparklines(0).len(), 3);
        // One cleared, then the rest; then undone.
        d.clear_sparklines(0, [31, 4, 31, 4]).unwrap();
        assert!(
            d.grid_cells(0, 31..32, 4..5)
                .iter()
                .all(|(_, _, g)| g.sparkline.is_none())
        );
        assert!(d.grid_cells(0, 30..31, 4..5)[0].2.sparkline.is_some());
        d.clear_sparklines(0, [0, 0, 100, 10]).unwrap();
        assert!(
            d.grid_cells(0, 30..34, 0..5)
                .iter()
                .all(|(_, _, g)| g.sparkline.is_none())
        );
        let mut back = crate::Workbook::open(d.save().unwrap().bytes).unwrap();
        assert!(back.sparklines(0).is_empty());
        for _ in 0..2 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(
            d.grid_cells(0, 30..34, 0..5)
                .iter()
                .filter(|(_, _, g)| g.sparkline.is_some())
                .count(),
            3
        );
    }

    #[test]
    fn what_if() {
        let mut d = open("openpyxl-budget.xlsx");
        let at = |d: &mut Box<dyn ViewerDocument>, r, c| d.cell_input(0, r, c);
        let shown = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1)
                .first()
                .map(|x| x.2.text.clone())
                .unwrap_or_default()
        };
        // A loan: rate in K1, months in K2, amount in K3, payment in K4.
        for (r, v) in ["0.01", "12", "1000", "=-PMT(K1,K2,K3)"].iter().enumerate() {
            d.set_cell(0, r as u32, 10, v).unwrap();
        }
        // Goal Seek: the amount whose payment is 100.
        let x = d.goal_seek(0, (3, 10), 100.0, (2, 10)).unwrap().unwrap();
        assert!((x - 1125.5077).abs() < 0.01, "{x}");
        assert!(
            shown(&mut d, 3, 10).starts_with("100"),
            "{}",
            shown(&mut d, 3, 10)
        );
        assert!(d.undo().unwrap());
        assert_eq!(at(&mut d, 2, 10), "1000");
        // Refused: a formula as the changing cell, no formula to set.
        assert!(d.goal_seek(0, (3, 10), 1.0, (3, 10)).is_err());
        assert!(d.goal_seek(0, (2, 10), 1.0, (1, 10)).is_err());
        // One variable down a column: rates in M2:M4, the payment in N1.
        d.set_cell(0, 0, 13, "=K4").unwrap();
        for (r, v) in ["0", "0.01", "0.02"].iter().enumerate() {
            d.set_cell(0, 1 + r as u32, 12, v).unwrap();
        }
        d.create_data_table(0, [0, 12, 3, 13], None, Some((0, 10)))
            .unwrap();
        let pay = |rate: f64| 1000.0 * rate / (1.0 - (1.0 + rate).powi(-12));
        let n = |s: String| s.replace(',', "").parse::<f64>().unwrap();
        assert!((n(shown(&mut d, 1, 13)) - 1000.0 / 12.0).abs() < 0.01);
        assert!((n(shown(&mut d, 3, 13)) - pay(0.02)).abs() < 0.01);
        // Its cells are changed only as a whole.
        assert!(d.set_cell(0, 2, 13, "5").is_err());
        // Two variables: months along P1:R1, rates down O2:O3, K4 at O1.
        d.set_cell(0, 0, 14, "=K4").unwrap();
        for (c, v) in ["6", "12", "24"].iter().enumerate() {
            d.set_cell(0, 0, 15 + c as u32, v).unwrap();
        }
        d.set_cell(0, 1, 14, "0.01").unwrap();
        d.set_cell(0, 2, 14, "0.02").unwrap();
        d.create_data_table(0, [0, 14, 2, 17], Some((1, 10)), Some((0, 10)))
            .unwrap();
        assert!((n(shown(&mut d, 2, 16)) - pay(0.02)).abs() < 0.01);
        // Calculate Now after the amount changed: both tables follow.
        d.set_cell(0, 2, 10, "2000").unwrap();
        d.recalculate().unwrap();
        assert!((n(shown(&mut d, 3, 13)) - 2.0 * pay(0.02)).abs() < 0.01);
        assert!((n(shown(&mut d, 2, 16)) - 2.0 * pay(0.02)).abs() < 0.01);
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-whatif-test.xlsx"), &saved).unwrap();
        }
        // Cleared whole, from any of its cells.
        d.set_cell(0, 2, 13, "").unwrap();
        assert_eq!(shown(&mut d, 1, 13), "");
        d.set_cell(0, 2, 13, "5").unwrap();
        // Scenarios: kept, shown, deleted.
        d.add_scenario(0, "Low", &[(0, 10), (1, 10)], "cheap")
            .unwrap();
        d.set_cell(0, 0, 10, "0.05").unwrap();
        d.set_cell(0, 1, 10, "36").unwrap();
        d.add_scenario(0, "High", &[(0, 10), (1, 10)], "").unwrap();
        let all = d.scenarios(0);
        assert_eq!(all.len(), 2);
        assert_eq!(
            all[0].cells,
            vec![(0, 10, "0.01".into()), (1, 10, "12".into())]
        );
        d.show_scenario(0, "low").unwrap();
        assert_eq!(
            (at(&mut d, 0, 10), at(&mut d, 1, 10)),
            ("0.01".into(), "12".into())
        );
        assert!(d.undo().unwrap());
        assert_eq!(at(&mut d, 1, 10), "36");
        d.delete_scenario(0, "High").unwrap();
        assert_eq!(d.scenarios(0).len(), 1);
        let mut back = crate::Workbook::open(d.save().unwrap().bytes).unwrap();
        assert_eq!(back.scenarios(0)[0].comment, "cheap");
    }

    #[test]
    fn threads_and_tab_colors() {
        let mut d = open("openpyxl-budget.xlsx");
        // A thread on E2: a comment, a reply; marked on the cell.
        d.add_thread_comment(
            0,
            1,
            4,
            "Ayşe",
            "Bu tutar doğru mu?",
            "2026-10-04T10:00:00.00",
        )
        .unwrap();
        d.add_thread_comment(
            0,
            1,
            4,
            "Mehmet",
            "Evet, faturaya baktım.",
            "2026-10-04T10:05:00.00",
        )
        .unwrap();
        let t = d.threads(0);
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].row, t[0].col, t[0].done), (1, 4, false));
        let who: Vec<&str> = t[0].comments.iter().map(|c| c.author.as_str()).collect();
        assert_eq!(who, ["Ayşe", "Mehmet"]);
        assert!(d.grid_cells(0, 1..2, 4..5)[0].2.thread);
        // Its note for older readers is not a note of its own.
        assert!(d.cell_note(0, 1, 4).is_none());
        // Refused where a note is.
        d.set_note(0, 1, 5, Some("not".into())).unwrap();
        assert!(
            d.add_thread_comment(0, 1, 5, "A", "x", "2026-10-04T10:00:00")
                .is_err()
        );
        assert!(d.undo().unwrap());
        d.resolve_thread(0, 1, 4, true).unwrap();
        assert!(d.threads(0)[0].done);
        d.set_tab_color(0, Some([0xC0, 0x50, 0x4D])).unwrap();
        assert_eq!(d.tab_color(0), Some([0xC0, 0x50, 0x4D]));
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-threads-test.xlsx"), &saved).unwrap();
        }
        let mut back = crate::Workbook::open(saved).unwrap();
        let t = back.threads(0);
        assert_eq!((t[0].comments.len(), t[0].done), (2, true));
        assert!(
            back.comments(0)
                .unwrap()
                .iter()
                .any(|c| c.author.starts_with("tc=")
                    && c.text.contains("Bu tutar doğru mu?")
                    && c.text.contains("Reply:"))
        );
        // The reply deleted, then the thread; then all undone.
        d.delete_thread_comment(0, 1, 4, 1).unwrap();
        assert_eq!(d.threads(0)[0].comments.len(), 1);
        d.delete_thread_comment(0, 1, 4, 0).unwrap();
        assert!(d.threads(0).is_empty());
        assert!(!d.grid_cells(0, 1..2, 4..5).iter().any(|c| c.2.thread));
        d.set_tab_color(0, None).unwrap();
        assert_eq!(d.tab_color(0), None);
        for _ in 0..6 {
            assert!(d.undo().unwrap());
        }
        assert_eq!(d.threads(0)[0].comments.len(), 1);
    }

    #[test]
    fn sheet_views() {
        use kalem_viewer::SheetView;
        let mut d = open("openpyxl-budget.xlsx");
        assert_eq!(d.sheet_view(0), SheetView::default());
        assert!(!d.modified());
        let v = SheetView {
            zoom: 150,
            gridlines: false,
            headings: false,
            page_break_preview: true,
            split: Some([3, 2, 4, 1]),
        };
        d.set_sheet_view(0, v).unwrap();
        assert_eq!(d.sheet_view(0), v);
        // An edit to save, which undo leaves alone.
        assert!(d.modified());
        d.set_cell(0, 0, 10, "x").unwrap();
        assert!(d.undo().unwrap());
        assert_eq!(d.sheet_view(0), v);
        let saved = d.save().unwrap().bytes;
        assert!(!d.modified());
        let mut back = crate::Workbook::open(saved).unwrap();
        let raw = back.view_raw(0);
        assert_eq!(
            (raw.zoom, raw.gridlines, raw.headings, raw.preview),
            (150, false, false, true)
        );
        assert!(raw.split.is_some());
        // Back to normal; the frozen pane the file had is kept meanwhile.
        d.set_sheet_view(0, SheetView::default()).unwrap();
        let mut back = crate::Workbook::open(d.save().unwrap().bytes).unwrap();
        assert_eq!(back.view_raw(0), crate::workbook::ViewRaw::default());
        assert_eq!(back.sheet(0).unwrap().frozen, Some((1, 0)));
    }

    #[test]
    fn a_sheet_copied() {
        use kalem_viewer::SheetEdit;
        let mut d = open("openpyxl-budget.xlsx");
        let name = d.structure().units[0].label.clone();
        let png: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0,
            0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 13, 0x49, 0x44,
            0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0, 5, 0, 1, 0xFF, 0x89,
            0x99, 0x3D, 0x1D, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        d.insert_picture(0, [20, 1, 22, 2], &png, "png").unwrap();
        let table = d
            .create_table(0, [30, 0, 32, 1], false, "TableStyleMedium2")
            .unwrap();
        d.add_thread_comment(0, 1, 4, "A", "Bak", "2026-10-04T10:00:00")
            .unwrap();
        let charts = d.charts(0).len();
        assert!(charts > 0);
        // Copied next to it, named as Excel names a copy.
        let at = d.edit_sheets(SheetEdit::Copy(0, 1)).unwrap();
        assert_eq!(at, 1);
        assert_eq!(d.structure().units[1].label, format!("{name} (2)"));
        assert_eq!(d.cell_input(1, 1, 0), d.cell_input(0, 1, 0));
        assert_eq!(d.charts(1).len(), charts);
        assert_eq!(d.drawings(1).len(), d.drawings(0).len());
        assert!(d.cell_note(1, 1, 0).is_some() == d.cell_note(0, 1, 0).is_some());
        assert_eq!(d.threads(1).len(), 1);
        let tables = d.tables(1);
        assert_eq!(tables.len(), 1);
        assert_ne!(tables[0].name, table);
        // The copy's chart reads the copy.
        let before = d.charts(0)[0].series[0].values.clone();
        d.set_cell(1, 1, 1, "9999").unwrap();
        assert_eq!(d.charts(0)[0].series[0].values, before);
        assert_ne!(d.charts(1)[0].series[0].values, before);
        // Saved and read again: the same.
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-copy-sheet-test.xlsx"), &saved).unwrap();
        }
        let back = crate::Workbook::open(saved).unwrap();
        assert_eq!(back.sheets()[1].name, format!("{name} (2)"));
        assert_eq!(back.sheet_tables(1).len(), 1);
        // Undone, whole.
        assert!(d.undo().unwrap());
        assert!(d.undo().unwrap());
        assert_eq!(d.structure().units.len(), back.sheets().len() - 1);
    }

    #[test]
    fn calculation_options() {
        use kalem_viewer::{CalcMode, CalcOptions};
        let mut d = open("openpyxl-budget.xlsx");
        let shown = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1)
                .first()
                .map(|x| x.2.text.clone())
                .unwrap_or_default()
        };
        assert_eq!(d.calc_options(), CalcOptions::default());
        // A circle: K1 reads L1, L1 reads K1.
        d.set_cell(0, 0, 10, "=L1+1").unwrap();
        d.set_cell(0, 0, 11, "=K1*0.5").unwrap();
        assert_eq!(d.circular_references(), vec![(0, 0, 10), (0, 0, 11)]);
        // Iterated, it settles: K1 = 2, L1 = 1.
        let it = CalcOptions {
            iterate: true,
            max_iterations: 100,
            max_change: 0.000_001,
            ..CalcOptions::default()
        };
        d.set_calc_options(it).unwrap();
        assert!(d.circular_references().is_empty());
        let num = |d: &mut Box<dyn ViewerDocument>, r, c| shown(d, r, c).parse::<f64>().unwrap();
        assert!((num(&mut d, 0, 10) - 2.0).abs() < 1e-4);
        assert!((num(&mut d, 0, 11) - 1.0).abs() < 1e-4);
        // Edited again, iterated again.
        d.set_cell(0, 0, 11, "=K1*0.75").unwrap();
        assert!(
            (num(&mut d, 0, 10) - 4.0).abs() < 1e-3,
            "{}",
            shown(&mut d, 0, 10)
        );
        // Manual: what reads a changed cell waits for Calculate Now.
        d.set_calc_options(CalcOptions {
            mode: CalcMode::Manual,
            ..CalcOptions::default()
        })
        .unwrap();
        d.set_cell(0, 4, 10, "3").unwrap();
        d.set_cell(0, 4, 11, "=K5*2").unwrap();
        assert_eq!(shown(&mut d, 4, 11), "6");
        d.set_cell(0, 4, 10, "10").unwrap();
        assert_eq!(shown(&mut d, 4, 11), "6");
        d.recalculate().unwrap();
        assert_eq!(shown(&mut d, 4, 11), "20");
        // Automatic: a data table follows its input at once.
        d.set_calc_options(CalcOptions::default()).unwrap();
        d.set_cell(0, 7, 10, "=K5*2").unwrap();
        d.set_cell(0, 8, 9, "1").unwrap();
        d.set_cell(0, 9, 9, "2").unwrap();
        d.create_data_table(0, [7, 9, 9, 10], None, Some((4, 10)))
            .unwrap();
        assert_eq!(shown(&mut d, 9, 10), "4");
        d.set_cell(0, 9, 9, "5").unwrap();
        assert_eq!(shown(&mut d, 9, 10), "10");
        // Kept in the file.
        d.set_calc_options(CalcOptions {
            mode: CalcMode::AutomaticExceptTables,
            ..it
        })
        .unwrap();
        let back = crate::Workbook::open(d.save().unwrap().bytes).unwrap();
        let o = back.calc_options();
        assert_eq!(
            (o.mode, o.iterate, o.max_iterations),
            (CalcMode::AutomaticExceptTables, true, 100)
        );
        assert!(d.undo().unwrap());
        assert_eq!(d.calc_options().mode, CalcMode::Automatic);
    }

    #[test]
    fn sorted_by_color() {
        use kalem_viewer::{SortColor, SortKey, StyleChange};
        let mut d = open("openpyxl-budget.xlsx");
        for (r, v) in ["a", "b", "c", "d"].iter().enumerate() {
            d.set_cell(0, 20 + r as u32, 0, v).unwrap();
        }
        let red = [0xC0, 0x00, 0x00];
        for r in [21, 23] {
            d.change_style(
                0,
                [r, 0, r, 0],
                StyleChange {
                    fill: Some(Some(red)),
                    ..StyleChange::default()
                },
            )
            .unwrap();
        }
        let key = |descending| SortKey {
            col: 0,
            descending,
            color: Some(SortColor {
                font: false,
                rgb: red,
            }),
            ..SortKey::default()
        };
        d.sort_range_by(0, [20, 0, 23, 0], &[key(false)], false)
            .unwrap();
        let col: Vec<String> = (20..24).map(|r| d.cell_input(0, r, 0)).collect();
        assert_eq!(col, ["b", "d", "a", "c"]);
        d.sort_range_by(0, [20, 0, 23, 0], &[key(true)], false)
            .unwrap();
        let col: Vec<String> = (20..24).map(|r| d.cell_input(0, r, 0)).collect();
        assert_eq!(col, ["a", "c", "b", "d"]);
    }

    #[test]
    fn formatting_more() {
        use kalem_viewer::{BorderSet, FillPattern, LineStyle, StyleChange};
        let mut d = open("openpyxl-budget.xlsx");
        let cell = |d: &mut Box<dyn ViewerDocument>, r: u32, c: u32| {
            d.grid_cells(0, r..r + 1, c..c + 1)
                .into_iter()
                .next()
                .map(|x| x.2)
                .unwrap_or_default()
        };
        // A double bottom line.
        d.change_style(
            0,
            [10, 1, 10, 1],
            StyleChange {
                borders: Some((BorderSet::Bottom, Some([0, 0, 0]))),
                border_style: Some(LineStyle::Double),
                ..StyleChange::default()
            },
        )
        .unwrap();
        assert_eq!(
            cell(&mut d, 10, 1).border_styles[2],
            Some(LineStyle::Double)
        );
        // A pattern, and a gradient.
        let grid = FillPattern::Pattern {
            kind: "darkGrid".into(),
            color: [0x44, 0x72, 0xC4],
            background: [0xFF, 0xFF, 0xFF],
        };
        let fade = FillPattern::Gradient {
            angle: 90,
            from: [0xFF, 0xFF, 0xFF],
            to: [0x44, 0x72, 0xC4],
        };
        for (c, f) in [(2, &grid), (3, &fade)] {
            d.change_style(
                0,
                [10, c, 10, c],
                StyleChange {
                    fill_pattern: Some(Some(f.clone())),
                    ..StyleChange::default()
                },
            )
            .unwrap();
        }
        assert_eq!(cell(&mut d, 10, 2).fill_pattern, Some(grid.clone()));
        assert_eq!(cell(&mut d, 10, 3).fill_pattern, Some(fade.clone()));
        // A named style, made once.
        d.apply_cell_style(0, [12, 1, 13, 2], "Good").unwrap();
        d.apply_cell_style(0, [14, 1, 14, 1], "good").unwrap();
        let good = cell(&mut d, 12, 1);
        assert_eq!(
            (good.fill, good.color),
            (Some([0xC6, 0xEF, 0xCE]), Some([0x00, 0x61, 0x00]))
        );
        assert_eq!(d.cell_styles().iter().filter(|n| *n == "Good").count(), 1);
        assert!(d.apply_cell_style(0, [12, 1, 12, 1], "Nonesuch").is_err());
        // One of the user's own, from a cell.
        d.new_cell_style("Mine", 0, 10, 2).unwrap();
        assert!(d.cell_styles().contains(&"Mine".to_string()));
        assert!(d.new_cell_style("Mine", 0, 10, 2).is_err());
        // The theme.
        let before = d.theme_name();
        d.set_theme("Blue").unwrap();
        assert_eq!(d.theme_name().as_deref(), Some("Blue"));
        let saved = d.save().unwrap().bytes;
        if let Ok(dir) = std::env::var("KALEM_CHART_OUT") {
            std::fs::write(format!("{dir}/kalem-formatting-test.xlsx"), &saved).unwrap();
        }
        let back = crate::Workbook::open(saved).unwrap();
        assert_eq!(back.theme_name().as_deref(), Some("Blue"));
        assert!(back.cell_styles().contains(&"Good".to_string()));
        assert!(d.undo().unwrap());
        assert_eq!(d.theme_name(), before);
        // A workbook without a theme gets one.
        let mut d = open("libreoffice-budget.xlsx");
        assert_eq!(d.theme_name(), None);
        d.set_theme("Green").unwrap();
        let back = crate::Workbook::open(d.save().unwrap().bytes).unwrap();
        assert_eq!(back.theme_name().as_deref(), Some("Green"));
        assert!(d.undo().unwrap());
        assert_eq!(d.theme_name(), None);
    }
}
