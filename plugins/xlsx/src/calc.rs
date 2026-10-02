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

use std::collections::HashMap;

use ironcalc_base::Model;
use ironcalc_base::cell::CellValue;
use ironcalc_base::types::CellType;

use crate::cellref::CellRef;
use crate::sheet::{FormulaKind, Sheet, Value};

/// A workbook's cells as the engine holds them.
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
/// prefixes dropped.
fn engine_formula(text: &str) -> String {
    let t = text
        .replace("_xlfn._xlws.", "")
        .replace("_xlfn.", "")
        .replace("_xlws.", "");
    format!("={t}")
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
    /// formulas; a formula the engine cannot read, an array formula or a
    /// data table enters as its stored result.
    pub fn load(
        sheets: &[(String, Option<&Sheet>)],
        names: &[(String, Option<usize>, String)],
    ) -> Result<Self, String> {
        let mut model = Model::new_empty("workbook", "en", "UTC", "en")?;
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
        for ((_, sheet), idx) in sheets.iter().zip(&map) {
            let (Some(sheet), Some(idx)) = (sheet, idx) else {
                continue;
            };
            for (at, cell) in &sheet.cells {
                let (r, c) = rc(*at);
                let as_formula = match &cell.formula {
                    Some(f)
                        if matches!(f.kind, FormulaKind::Normal | FormulaKind::Shared { .. }) =>
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

    /// Computes a formula as if it were in a cell of `sheet` that is not
    /// used: `Application.Evaluate` and `WorksheetFunction` in macros.
    pub fn eval_scratch(&mut self, sheet: usize, formula: &str) -> Option<Value> {
        let idx = (*self.map.get(sheet)?)?;
        // The last cell of the sheet; cleared again afterwards.
        let (r, c) = (
            crate::cellref::MAX_ROW as i32,
            crate::cellref::MAX_COL as i32,
        );
        self.model
            .update_cell_with_formula(idx, r, c, engine_formula(formula))
            .ok()?;
        self.model.evaluate();
        let v = self.value(sheet, CellRef::new(r as u32 - 1, c as u32 - 1));
        let _ = self.model.set_user_input(idx, r, c, String::new());
        v
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
        let mut e = Engine::load(&sheets, &names).unwrap();
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
}
