//! The plugin as Kalem's `document-viewer` and `document-editor` (D54):
//! a workbook's sheets are grid units the host draws, cells are edited as
//! typed, rows and columns inserted and deleted, edits undone by the
//! workbook's own history, macros listed and run on the user's command,
//! and the file saved as itself.

use std::collections::HashMap;

use kalem_viewer::{
    Align, Bitmap, Detection, FileHandle, GridCell, GridEdit, GridLayout, InfoField, MacroEntry,
    MacroOutcome, MacroQuestion, MacroUi, RenderRequest, Rendered, Result, SaveOutput, Structure,
    Unit, UnitKind, Viewer, ViewerDocument, ViewerError,
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
                },
            ));
        }
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
}
