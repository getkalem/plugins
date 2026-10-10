//! Formula results computed by IronCalc (D56, task T3.7.4a), used only where
//! they agree with the file.
//!
//! A workbook's formula cells carry their last results (`<v>`), written by
//! the application that saved the file; Excel recalculates on open when told
//! to, LibreOffice by default does not. After an edit Kalem therefore writes
//! the new results of the formulas the edit changed. The engine is trusted
//! cell by cell: before any edit it computes every formula of the workbook,
//! and a cell whose result equals the result stored in the file is trusted;
//! after an edit, a trusted cell whose result changed gets the new result,
//! and an untrusted cell whose result changed loses its stored result (the
//! cell is stale, and Excel and LibreOffice compute it on open).

use std::collections::{HashMap, HashSet};

use ironcalc_base::Model;
use ironcalc_base::cell::CellValue;
use ironcalc_base::types::CellType;

use crate::cellref::CellRef;
use crate::sheet::{FormulaKind, Sheet, Value};

/// A workbook's cells as the engine holds them.
/// A table as the engine reads structured references through it.
#[derive(Debug, Clone)]
pub struct EngineTable {
    /// Its name.
    pub name: String,
    /// The workbook sheet it is on.
    pub sheet: usize,
    /// Its range in A1 notation, header and total rows included.
    pub reference: String,
    /// Its columns' names.
    pub columns: Vec<String>,
    /// It has a header row.
    pub header: bool,
    /// It has a total row.
    pub totals: bool,
}

pub struct Engine {
    model: Model<'static>,
    /// The engine's sheet index of each workbook sheet, `None` for chart sheets.
    map: Vec<Option<u32>>,
    /// The last computed result of each formula cell, by workbook sheet.
    results: HashMap<(usize, CellRef), Value>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("map", &self.map)
            .finish_non_exhaustive()
    }
}

/// A formula's text as the engine reads it: the file's future-function
/// prefixes dropped, and `HYPERLINK`, which the engine lacks, as the value
/// it shows.
fn engine_formula(text: &str) -> String {
    let t = text
        .replace("_xlfn._xlws.", "")
        .replace("_xlfn.", "")
        .replace("_xlws.", "");
    format!("={}", crate::formula::hyperlink_as_value(&t))
}

/// Whether `formula` calls a function the engine does not compute (not
/// one of the workbook's names, which may hold a `LAMBDA`): `GETPIVOTDATA`,
/// `AGGREGATE`, `WEBSERVICE`, `IMAGE`, `GROUPBY`, a function newer than the
/// engine.
fn calls_unknown(formula: &str, names: &HashSet<String>) -> bool {
    static KNOWN: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
    let known = KNOWN.get_or_init(|| {
        let mut k: HashSet<String> = crate::functions::list()
            .into_iter()
            .map(|(n, _)| n.to_ascii_uppercase())
            .collect();
        // Given to the engine as the value it shows.
        k.insert("HYPERLINK".into());
        k
    });
    crate::formula::called_functions(formula)
        .iter()
        .any(|f| !known.contains(f) && !names.contains(f))
}

fn rc(at: CellRef) -> (i32, i32) {
    (at.row as i32 + 1, at.col as i32 + 1)
}

/// Whether two results are the same, numbers to within the last digits.
pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x == y || (x - y).abs() <= 1e-12 * x.abs().max(y.abs()).max(1.0)
        }
        // An empty text result is stored as `t="str"` with an empty `<v>`.
        (Value::Text(s), Value::Empty) | (Value::Empty, Value::Text(s)) => s.is_empty(),
        _ => a == b,
    }
}

impl Engine {
    /// Loads every worksheet of a workbook: values as values, formulas as
    /// formulas; a formula the engine cannot read or calls a function it
    /// does not compute, an array formula or a data table enters as its
    /// stored result, so that the formulas reading the cell compute from
    /// it. Such a cell keeps its stored result: the engine never changes
    /// it.
    pub fn load(
        sheets: &[(String, Option<&Sheet>)],
        names: &[(String, Option<usize>, String)],
        tables: &[EngineTable],
    ) -> Result<Self, String> {
        // In the user's time zone, as Excel computes NOW() and TODAY().
        let mut model = Model::new_empty("workbook", "en", local_timezone(), "en")
            .or_else(|_| Model::new_empty("workbook", "en", "UTC", "en"))?;
        let mut map = Vec::new();
        let mut count = 0u32;
        for (name, sheet) in sheets {
            if sheet.is_none() {
                map.push(None);
                continue;
            }
            if count == 0 {
                model.rename_sheet_by_index(0, name)?;
            } else {
                model.add_sheet(name)?;
            }
            map.push(Some(count));
            count += 1;
        }
        for (name, scope, formula) in names {
            let scope = scope.and_then(|s| map.get(s).copied().flatten());
            // Names come before the cells whose formulas use them. Names the
            // engine cannot read are left out; formulas using them compute
            // #NAME? and are not trusted.
            let _ = model.new_defined_name(name, scope, formula.trim_start_matches('='));
        }
        // Tables before the formulas, so that their structured references
        // parse: the model made again from a workbook that has them.
        if !tables.is_empty() {
            let mut wb = model.workbook.clone();
            for t in tables {
                let Some((sheet_name, _)) = sheets.get(t.sheet) else {
                    continue;
                };
                let columns = t
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(i, n)| ironcalc_base::types::TableColumn {
                        id: i as u32 + 1,
                        name: n.clone(),
                        ..Default::default()
                    })
                    .collect();
                wb.tables.insert(
                    t.name.clone(),
                    ironcalc_base::types::Table {
                        name: t.name.clone(),
                        display_name: t.name.clone(),
                        sheet_name: sheet_name.clone(),
                        reference: t.reference.clone(),
                        totals_row_count: u32::from(t.totals),
                        header_row_count: u32::from(t.header),
                        header_row_dxf_id: None,
                        data_dxf_id: None,
                        totals_row_dxf_id: None,
                        columns,
                        style_info: Default::default(),
                        has_filters: true,
                    },
                );
            }
            model = Model::from_workbook(wb, "en")?;
        }
        let name_set: HashSet<String> =
            names.iter().map(|(n, ..)| n.to_ascii_uppercase()).collect();
        for ((_, sheet), idx) in sheets.iter().zip(&map) {
            let (Some(sheet), Some(idx)) = (sheet, idx) else {
                continue;
            };
            for (at, cell) in &sheet.cells {
                let (r, c) = rc(*at);
                let as_formula = match &cell.formula {
                    Some(f)
                        if matches!(f.kind, FormulaKind::Normal | FormulaKind::Shared { .. })
                            && !calls_unknown(&f.text, &name_set) =>
                    {
                        Some(&f.text)
                    }
                    _ => None,
                };
                if let Some(text) = as_formula
                    && model
                        .update_cell_with_formula(*idx, r, c, engine_formula(text))
                        .is_ok()
                {
                    continue;
                }
                set_value(&mut model, *idx, r, c, &cell.value)?;
            }
        }
        Ok(Self {
            model,
            map,
            results: HashMap::new(),
        })
    }

    /// Computes every formula and records the results of the given cells.
    pub fn evaluate(
        &mut self,
        formula_cells: &[(usize, CellRef)],
    ) -> &HashMap<(usize, CellRef), Value> {
        self.model.evaluate();
        self.results.clear();
        for &(sheet, at) in formula_cells {
            if let Some(v) = self.value(sheet, at) {
                self.results.insert((sheet, at), v);
            }
        }
        &self.results
    }

    /// The engine's current value of a cell.
    pub fn value(&self, sheet: usize, at: CellRef) -> Option<Value> {
        let idx = (*self.map.get(sheet)?)?;
        let (r, c) = rc(at);
        let ty = self.model.get_cell_type(idx, r, c).ok()?;
        Some(match self.model.get_cell_value_by_index(idx, r, c).ok()? {
            CellValue::None => Value::Empty,
            CellValue::Number(n) => Value::Number(n),
            CellValue::Boolean(b) => Value::Bool(b),
            CellValue::String(s) if ty == CellType::ErrorValue => Value::Error(s),
            CellValue::String(s) => Value::Text(s),
        })
    }

    /// Formulas computed together, as if each were in an unused cell of the
    /// sheet: one recalculation of the workbook for all of them, which is
    /// what a recalculation costs.
    pub fn eval_scratch_many(&mut self, sheet: usize, formulas: &[String]) -> Vec<Option<Value>> {
        let Some(Some(idx)) = self.map.get(sheet).copied() else {
            return vec![None; formulas.len()];
        };
        let (last_row, c) = (
            crate::cellref::MAX_ROW as i32,
            crate::cellref::MAX_COL as i32,
        );
        let mut out = Vec::with_capacity(formulas.len());
        // The sheet's last column, from its last row up, a chunk at a time.
        for chunk in formulas.chunks(4096) {
            let mut placed = Vec::with_capacity(chunk.len());
            for (k, f) in chunk.iter().enumerate() {
                let r = last_row - k as i32;
                let ok = self
                    .model
                    .update_cell_with_formula(idx, r, c, engine_formula(f.trim_start_matches('=')))
                    .is_ok();
                placed.push((r, ok));
            }
            self.model.evaluate();
            for (r, ok) in placed {
                out.push(if ok {
                    self.value(sheet, CellRef::new(r as u32 - 1, c as u32 - 1))
                } else {
                    None
                });
                let _ = self.model.set_user_input(idx, r, c, String::new());
            }
        }
        out
    }

    /// Enters a cell's new content: a formula, a value, or nothing.
    pub fn set(
        &mut self,
        sheet: usize,
        at: CellRef,
        formula: Option<&str>,
        value: &Value,
    ) -> Result<(), String> {
        let Some(Some(idx)) = self.map.get(sheet).copied() else {
            return Ok(());
        };
        let (r, c) = rc(at);
        if let Some(f) = formula
            && self
                .model
                .update_cell_with_formula(idx, r, c, engine_formula(f))
                .is_ok()
        {
            return Ok(());
        }
        set_value(&mut self.model, idx, r, c, value)
    }
}

/// The user's time zone by its IANA name; UTC when it cannot be told.
/// Asked once: the engine keeps it for as long as it lives.
fn local_timezone() -> &'static str {
    static TZ: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TZ.get_or_init(|| {
        #[cfg(target_arch = "wasm32")]
        {
            kalem_plugin::viewer::kalem::plugin::clock::timezone()
        }
        #[cfg(all(not(target_arch = "wasm32"), not(target_family = "wasm")))]
        {
            iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into())
        }
        #[cfg(all(not(target_arch = "wasm32"), target_family = "wasm"))]
        {
            "UTC".to_string()
        }
    })
}

impl Engine {
    /// Circular references computed over and over (iterative
    /// calculation): each cell of `circ` (sheet, cell, formula) starts from
    /// its value in `start`, and each round every formula is computed with
    /// the others' values from the round before, until none changes by more
    /// than `max_change` or `max_iterations` rounds. The cells are left in
    /// the engine as those values, for the formulas reading them.
    pub fn iterate(
        &mut self,
        circ: &[(usize, CellRef, String)],
        start: Vec<f64>,
        max_iterations: u32,
        max_change: f64,
    ) -> Vec<f64> {
        let mut vals = start;
        let mut by_sheet: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
        for (i, (s, _, _)) in circ.iter().enumerate() {
            by_sheet.entry(*s).or_default().push(i);
        }
        for _ in 0..max_iterations.max(1) {
            for ((s, at, _), v) in circ.iter().zip(&vals) {
                let _ = self.set(*s, *at, None, &Value::Number(*v));
            }
            let mut new = vals.clone();
            for (sheet, idxs) in &by_sheet {
                let formulas: Vec<String> = idxs.iter().map(|&i| circ[i].2.clone()).collect();
                for (&i, r) in idxs.iter().zip(self.eval_scratch_many(*sheet, &formulas)) {
                    if let Some(Value::Number(n)) = r {
                        new[i] = n;
                    }
                }
            }
            let change = vals
                .iter()
                .zip(&new)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max);
            vals = new;
            if change <= max_change {
                break;
            }
        }
        for ((s, at, _), v) in circ.iter().zip(&vals) {
            let _ = self.set(*s, *at, None, &Value::Number(*v));
        }
        vals
    }
}

fn set_value(
    model: &mut Model<'static>,
    idx: u32,
    r: i32,
    c: i32,
    v: &Value,
) -> Result<(), String> {
    match v {
        Value::Empty => model.set_user_input(idx, r, c, String::new()),
        Value::Number(n) => model.update_cell_with_number(idx, r, c, *n),
        Value::Text(t) => model.update_cell_with_text(idx, r, c, t),
        Value::Bool(b) => model.update_cell_with_bool(idx, r, c, *b),
        // An error literal is read as one by the input parser.
        Value::Error(e) => model.set_user_input(idx, r, c, e.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet;

    #[test]
    fn computes_across_sheets_and_names() {
        let a = sheet::parse(
            r#"<worksheet><sheetData><row r="1"><c r="A1"><v>2</v></c><c r="B1"><f>A1*3</f><v>6</v></c><c r="C1"><f>_xlfn.CONCAT("a","b")</f></c></row></sheetData></worksheet>"#,
            &[],
            false,
        );
        let b = sheet::parse(
            r#"<worksheet><sheetData><row r="1"><c r="A1"><f>Data!B1+Rate</f><v>8</v></c><c r="A2"><f>1/0</f></c><c r="A3"><f>Twice</f></c></row></sheetData></worksheet>"#,
            &[],
            false,
        );
        let sheets = vec![
            ("Data".to_owned(), Some(&a)),
            ("Chart".to_owned(), None),
            ("Calc".to_owned(), Some(&b)),
        ];
        // IronCalc takes names that are references; `Twice` is an expression
        // it refuses, so formulas using it are #NAME? and never trusted.
        let names = vec![
            ("Rate".to_owned(), None, "Data!$A$1".to_owned()),
            ("Twice".to_owned(), None, "Data!$A$1*2".to_owned()),
        ];
        let mut e = Engine::load(&sheets, &names, &[]).unwrap();
        let cells = [
            (0, CellRef::new(0, 1)),
            (0, CellRef::new(0, 2)),
            (2, CellRef::new(0, 0)),
            (2, CellRef::new(1, 0)),
            (2, CellRef::new(2, 0)),
        ];
        let r = e.evaluate(&cells).clone();
        assert_eq!(r[&cells[0]], Value::Number(6.0));
        assert_eq!(r[&cells[1]], Value::Text("ab".into()));
        assert_eq!(r[&cells[2]], Value::Number(8.0));
        assert_eq!(r[&cells[3]], Value::Error("#DIV/0!".into()));
        assert_eq!(r[&cells[4]], Value::Error("#NAME?".into()));
        e.set(0, CellRef::new(0, 0), None, &Value::Number(10.0))
            .unwrap();
        let r = e.evaluate(&cells).clone();
        assert_eq!(r[&cells[0]], Value::Number(30.0));
        assert_eq!(r[&cells[2]], Value::Number(40.0));
    }

    #[test]
    fn functions_the_engine_lacks_keep_their_results() {
        // A1 asks the network, B1 a pivot table; C1 and D1 read them; E1
        // is a link showing its friendly name.
        let a = sheet::parse(
            r#"<worksheet><sheetData><row r="1"><c r="A1"><f>_xlfn.WEBSERVICE("https://example.com/rate")</f><v>4</v></c><c r="B1"><f>GETPIVOTDATA("Sum",$H$1)</f><v>10</v></c><c r="C1"><f>A1*B1+F1</f><v>40</v></c><c r="D1"><f>Twice(A1)</f><v>8</v></c><c r="E1" t="str"><f>HYPERLINK("https://example.com","Rate "&amp;A1)</f><v>Rate 4</v></c><c r="F1"><v>0</v></c></row></sheetData></worksheet>"#,
            &[],
            false,
        );
        let sheets = vec![("Data".to_owned(), Some(&a))];
        let names = vec![("Twice".to_owned(), None, "LAMBDA(x,x*2)".to_owned())];
        let mut e = Engine::load(&sheets, &names, &[]).unwrap();
        let cells: Vec<_> = (0..5).map(|c| (0, CellRef::new(0, c))).collect();
        let r = e.evaluate(&cells).clone();
        assert_eq!(r[&cells[0]], Value::Number(4.0));
        assert_eq!(r[&cells[1]], Value::Number(10.0));
        assert_eq!(r[&cells[2]], Value::Number(40.0));
        assert_eq!(r[&cells[3]], Value::Number(8.0));
        assert_eq!(r[&cells[4]], Value::Text("Rate 4".into()));
        // An edit of F1 reaches C1, which reads the stored results.
        e.set(0, CellRef::new(0, 5), None, &Value::Number(2.0))
            .unwrap();
        let r = e.evaluate(&cells).clone();
        assert_eq!(r[&cells[2]], Value::Number(42.0));
        // A link typed in shows its friendly name, or its address.
        e.set(
            0,
            CellRef::new(0, 4),
            Some("HYPERLINK(\"https://x.y\")"),
            &Value::Empty,
        )
        .unwrap();
        let r = e.evaluate(&cells).clone();
        assert_eq!(r[&cells[4]], Value::Text("https://x.y".into()));
    }
}
