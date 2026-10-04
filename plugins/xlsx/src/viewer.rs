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
    PivotSpec, RenderRequest, Rendered, Result, SaveOutput, Structure, Unit, UnitKind, Validation,
    ValidationError, ValidationKind, Viewer, ViewerDocument, ViewerError,
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
            notes: HashMap::new(),
            cf: HashMap::new(),
            charts: HashMap::new(),
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
    /// Each sheet's notes, read once.
    notes: HashMap<usize, HashMap<CellRef, String>>,
    /// Each sheet's conditional formats and what they made of its cells,
    /// at the workbook's generation they were read.
    cf: HashMap<usize, (u64, crate::conditional::Evaluator)>,
    /// Each sheet's charts, at the generation they were read.
    charts: HashMap<usize, (u64, Vec<Chart>)>,
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
        // The formulas the rules need for these cells, computed together.
        let touched: Vec<CellRef> = index
            .keys()
            .map(|&(r, c)| CellRef::new(r, c))
            .filter(|p| ev.touches(*p))
            .collect();
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

impl ViewerDocument for XlsxDoc {
    fn structure(&self) -> Structure {
        units(self.locked().sheets().iter().map(|s| {
            (
                s.name.clone(),
                s.visibility != Visibility::Visible,
                s.kind == SheetKind::Worksheet,
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
                },
            ));
        }
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
        out
    }

    fn cell_input(&mut self, unit: usize, row: u32, col: u32) -> String {
        self.book()
            .edit_text(unit, CellRef::new(row, col))
            .unwrap_or_default()
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
        self.book()
            .fill(unit, r(source), r(target), series)
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
        if !self.worksheet(unit) {
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

    fn insert_pivot(&mut self, unit: usize, spec: PivotSpec) -> Result<usize> {
        let r = spec.range;
        let range = crate::cellref::Range {
            start: CellRef::new(r[0], r[1]),
            end: CellRef::new(r[2], r[3]),
        };
        let layout = crate::pivot::Layout {
            rows: spec.rows.iter().map(|&f| f as usize).collect(),
            cols: spec.cols.iter().map(|&f| f as usize).collect(),
            values: spec.values.iter().map(|&(f, a)| (f as usize, a)).collect(),
        };
        self.book().insert_pivot(unit, range, &layout).map_err(err)
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
        wb.history_len() != self.saved_at || (self.saved_at == 0 && wb.is_dirty())
    }

    fn save(&mut self) -> Result<SaveOutput> {
        let bytes = self.book().save().map_err(err)?;
        self.saved_at = self.book().history_len();
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
        Some(GridLayout {
            rows: used.map_or(0, |u| u.end.row + 1),
            cols: used.map_or(0, |u| u.end.col + 1),
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
        s.cells
            .range(CellRef::new(rows.start, 0)..CellRef::new(rows.end, 0))
            .filter(|(p, _)| cols.contains(&p.col))
            .map(|(p, c)| {
                (
                    p.row,
                    p.col,
                    GridCell {
                        text: self.wb.display(unit, *p),
                        numeric: matches!(c.value, Value::Number(_)),
                        formula: c.formula.is_some(),
                        ..GridCell::default()
                    },
                )
            })
            .collect()
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
            values: vec![(1, Aggregate::Sum), (2, Aggregate::Max)],
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
            values: vec![(1, Aggregate::Count)],
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
    }
}
