//! What-if analysis: Goal Seek, data tables of one and two variables
//! (`<f t="dataTable">`, their results computed by putting each input
//! value into the input cells), and scenarios (`<scenarios>`).

use super::*;
use crate::sheet::DataTableDef;

/// A scenario as kept in the sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioDef {
    /// Its name.
    pub name: String,
    /// What it is about.
    pub comment: String,
    /// Its cells and their values, as typed.
    pub cells: Vec<(CellRef, String)>,
}

/// A sheet part's scenarios and the bytes of its `<scenarios>`.
fn parse_scenarios(text: &str) -> (Vec<ScenarioDef>, Option<Span<usize>>) {
    let mut r = Reader::new(text);
    let mut out: Vec<ScenarioDef> = Vec::new();
    let mut span: Option<Span<usize>> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => match tag.name {
                "scenarios" => {
                    span = Some(tag.span.clone());
                }
                "scenario" => out.push(ScenarioDef {
                    name: tag.attr("name").map(|v| v.into_owned()).unwrap_or_default(),
                    comment: tag
                        .attr("comment")
                        .map(|v| v.into_owned())
                        .unwrap_or_default(),
                    cells: Vec::new(),
                }),
                "inputCells" => {
                    if let (Some(s), Some(at)) = (
                        out.last_mut(),
                        tag.attr("r").and_then(|v| CellRef::parse(v.trim())),
                    ) {
                        s.cells.push((
                            at,
                            tag.attr("val").map(|v| v.into_owned()).unwrap_or_default(),
                        ));
                    }
                }
                _ => {}
            },
            Token::End { name, span: end } => {
                if name == "scenarios"
                    && let Some(s) = span.as_mut()
                {
                    s.end = end.end;
                }
            }
            Token::Text { .. } => {}
        }
    }
    (out, span)
}

/// A value cell, with a data table's `<f>` when it is the table's first.
fn value_cell(p: &str, at: CellRef, style: u32, v: &Value, f: &str) -> String {
    let s = if style == 0 {
        String::new()
    } else {
        format!(" s=\"{style}\"")
    };
    let (t, v) = match v {
        Value::Number(n) => ("", Some(number_text(*n))),
        Value::Text(x) => (" t=\"str\"", Some(xml::escape(x))),
        Value::Bool(b) => (" t=\"b\"", Some(u8::from(*b).to_string())),
        Value::Error(e) => (" t=\"e\"", Some(xml::escape(e))),
        Value::Empty => ("", None),
    };
    let v = v.map_or(String::new(), |v| format!("<{p}v>{v}</{p}v>"));
    format!("<{p}c r=\"{at}\"{s}{t}>{f}{v}</{p}c>")
}

/// A data table's `<f>`.
fn table_f(p: &str, def: &DataTableDef) -> String {
    let mut a = format!("<{p}f t=\"dataTable\" ref=\"{}\"", def.range);
    if def.two {
        a.push_str(" dt2D=\"1\" dtr=\"1\"");
    } else {
        a.push_str(if def.row {
            " dt2D=\"0\" dtr=\"1\""
        } else {
            " dt2D=\"0\" dtr=\"0\""
        });
    }
    if let Some(r1) = def.r1 {
        a.push_str(&format!(" r1=\"{r1}\""));
    }
    if let Some(r2) = def.r2 {
        a.push_str(&format!(" r2=\"{r2}\""));
    }
    a.push_str("/>");
    a
}

fn input_of(v: &Value) -> Input {
    match v {
        Value::Number(n) => Input::Number(*n, None),
        Value::Text(t) => Input::Text(t.clone()),
        Value::Bool(b) => Input::Bool(*b),
        Value::Error(e) => Input::Error(e.clone()),
        Value::Empty => Input::Clear,
    }
}

impl Workbook {
    /// Puts each set of values into the input cells of sheet `idx`, and
    /// reads the outputs after each; the inputs are as they were after.
    fn probe(
        &mut self,
        idx: usize,
        inputs: &[CellRef],
        sets: &[Vec<f64>],
        outputs: &[(usize, CellRef)],
    ) -> Result<Vec<Vec<Value>>> {
        self.load(idx)?;
        self.ensure_engine()?;
        self.flush()?;
        let originals: Vec<(Option<String>, Value)> = inputs
            .iter()
            .map(|at| match self.loaded[&idx].1.cells.get(at) {
                Some(c) => (
                    c.formula
                        .as_ref()
                        .filter(|f| {
                            matches!(f.kind, FormulaKind::Normal | FormulaKind::Shared { .. })
                        })
                        .map(|f| f.text.clone()),
                    c.value.clone(),
                ),
                None => (None, Value::Empty),
            })
            .collect();
        let engine = match self.engine.as_mut() {
            Some(Ok(e)) => e,
            Some(Err(e)) => {
                return Err(Error::Refused(format!(
                    "The formula engine cannot compute this workbook: {e}"
                )));
            }
            None => return Err(Error::Refused("No formula engine".into())),
        };
        let fail = |e: String| Error::Refused(format!("the formula engine: {e}"));
        let mut out = Vec::with_capacity(sets.len());
        let mut result = Ok(());
        for set in sets {
            for (at, v) in inputs.iter().zip(set) {
                if let Err(e) = engine.set(idx, *at, None, &Value::Number(*v)) {
                    result = Err(fail(e));
                }
            }
            if result.is_err() {
                break;
            }
            engine.evaluate(&[]);
            out.push(
                outputs
                    .iter()
                    .map(|(s, at)| engine.value(*s, *at).unwrap_or(Value::Empty))
                    .collect(),
            );
        }
        for (at, (f, v)) in inputs.iter().zip(&originals) {
            if let Err(e) = engine.set(idx, *at, f.as_deref(), v) {
                result = Err(fail(e));
            }
        }
        engine.evaluate(&[]);
        result.map(|()| out)
    }

    /// Goal Seek: the value of `by` that makes formula cell `set` come to
    /// `target`, by the secant method from `by`'s value; put into `by` (an
    /// undo step) when found.
    pub fn goal_seek(
        &mut self,
        idx: usize,
        set: CellRef,
        target: f64,
        by: CellRef,
    ) -> Result<Option<f64>> {
        self.load(idx)?;
        let cells = &self.loaded[&idx].1.cells;
        if cells.get(&set).is_none_or(|c| c.formula.is_none()) {
            return Err(Error::Refused(format!("{set} has to hold a formula")));
        }
        let start = match cells.get(&by) {
            Some(c) if c.formula.is_some() => {
                return Err(Error::Refused(format!(
                    "{by} has to hold a value, not a formula"
                )));
            }
            Some(Cell {
                value: Value::Number(n),
                ..
            }) => *n,
            Some(Cell {
                value: Value::Empty,
                ..
            })
            | None => 0.0,
            Some(_) => return Err(Error::Refused(format!("{by} has to hold a number"))),
        };
        self.check_cell_edit(idx, by)?;
        let f = |wb: &mut Self, x: f64| -> Result<Option<f64>> {
            Ok(
                match wb.probe(idx, &[by], &[vec![x]], &[(idx, set)])?[0][0] {
                    Value::Number(v) if v.is_finite() => Some(v - target),
                    _ => None,
                },
            )
        };
        let close = 1e-9 * target.abs().max(1.0);
        let (mut x0, mut x1) = (start, if start == 0.0 { 1.0 } else { start * 1.1 });
        let Some(mut f0) = f(self, x0)? else {
            return Ok(None);
        };
        let mut best = (f0.abs(), x0);
        for _ in 0..100 {
            if best.0 <= close {
                break;
            }
            let Some(f1) = f(self, x1)? else {
                // No result there: back towards the last good point.
                x1 = (x0 + x1) / 2.0;
                continue;
            };
            if f1.abs() < best.0 {
                best = (f1.abs(), x1);
            }
            let d = f1 - f0;
            let x2 = if d == 0.0 {
                x1 + (x1 - x0) * 2.0
            } else {
                x1 - f1 * (x1 - x0) / d
            };
            if !x2.is_finite() {
                break;
            }
            (x0, f0, x1) = (x1, f1, x2);
        }
        if best.0 > 1e-6 * target.abs().max(1.0) {
            return Ok(None);
        }
        // Fifteen digits, as Excel keeps.
        let x = format!("{:.14e}", best.1).parse().unwrap_or(best.1);
        self.set_input(idx, by, Input::Number(x, None))?;
        Ok(Some(x))
    }

    /// A data table over `range` (its first row and column the values for
    /// the input cells), its results computed and written. One undo step.
    pub fn create_data_table(
        &mut self,
        idx: usize,
        range: Range,
        row_input: Option<CellRef>,
        col_input: Option<CellRef>,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if range.end.row == range.start.row || range.end.col == range.start.col {
            return Err(Error::Refused(
                "Select the table with its values and formulas: two rows and two columns at least"
                    .into(),
            ));
        }
        if row_input.is_none() && col_input.is_none() {
            return Err(Error::Refused("Give a row or a column input cell".into()));
        }
        if [row_input, col_input]
            .into_iter()
            .flatten()
            .any(|c| range.contains(c))
        {
            return Err(Error::Refused(
                "An input cell has to be outside the table".into(),
            ));
        }
        let results = Range {
            start: CellRef::new(range.start.row + 1, range.start.col + 1),
            end: range.end,
        };
        let area = u64::from(results.end.row - results.start.row + 1)
            * u64::from(results.end.col - results.start.col + 1);
        if area > 10_000 {
            return Err(Error::Refused(
                "A data table of 10,000 results at most".into(),
            ));
        }
        if let Some(t) = self.loaded[&idx].1.data_tables.iter().find(|t| {
            t.range.start.row <= results.end.row
                && results.start.row <= t.range.end.row
                && t.range.start.col <= results.end.col
                && results.start.col <= t.range.end.col
        }) {
            return Err(Error::Refused(format!(
                "The data table at {} is in the way",
                t.range
            )));
        }
        let two = row_input.is_some() && col_input.is_some();
        let def = DataTableDef {
            range: results,
            two,
            row: !two && row_input.is_some(),
            r1: row_input.or(col_input),
            r2: if two { col_input } else { None },
        };
        let values = self.data_table_values(idx, &def)?;
        self.in_one_step(|wb| {
            wb.filling_table = true;
            let written = (|| -> Result<()> {
                for (at, v) in &values {
                    wb.set_input(idx, *at, input_of(v))?;
                }
                Ok(())
            })();
            wb.filling_table = false;
            written?;
            // The table's `<f>` on its first cell.
            let anchor = def.range.start;
            let (text, model) = &wb.loaded[&idx];
            let p = model.prefix.clone();
            let Some(c) = model.cells.get(&anchor) else {
                return Err(Error::Refused(
                    "the data table's first cell was not written".into(),
                ));
            };
            let v = values
                .iter()
                .find(|(at, _)| *at == anchor)
                .map_or(Value::Empty, |(_, v)| v.clone());
            let new = splice(
                text,
                vec![(
                    c.span.clone(),
                    value_cell(&p, anchor, c.style, &v, &table_f(&p, &def)),
                )],
            );
            wb.replace_sheet_text(idx, new);
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// A data table's results: each input value (or pair) put into the
    /// input cells, the formulas read.
    fn data_table_values(
        &mut self,
        idx: usize,
        def: &DataTableDef,
    ) -> Result<Vec<(CellRef, Value)>> {
        let r = def.range;
        let (top, left) = (r.start.row - 1, r.start.col - 1);
        let number = |wb: &mut Self, at: CellRef| match wb.value(idx, at) {
            Ok(Value::Number(n)) => n,
            _ => 0.0,
        };
        let rows: Vec<u32> = (r.start.row..=r.end.row).collect();
        let cols: Vec<u32> = (r.start.col..=r.end.col).collect();
        let mut out = Vec::new();
        if def.two {
            let (Some(ri), Some(ci)) = (def.r1, def.r2) else {
                return Ok(Vec::new());
            };
            let mut sets = Vec::new();
            for &row in &rows {
                for &col in &cols {
                    let x = number(self, CellRef::new(top, col));
                    let y = number(self, CellRef::new(row, left));
                    sets.push(vec![x, y]);
                }
            }
            let got = self.probe(idx, &[ri, ci], &sets, &[(idx, CellRef::new(top, left))])?;
            let mut got = got.into_iter();
            for &row in &rows {
                for &col in &cols {
                    let v = got.next().and_then(|mut v| v.pop()).unwrap_or(Value::Empty);
                    out.push((CellRef::new(row, col), v));
                }
            }
        } else if def.row {
            // Values along the top row, formulas down the left column.
            let Some(input) = def.r1 else {
                return Ok(Vec::new());
            };
            let sets: Vec<Vec<f64>> = cols
                .iter()
                .map(|&c| vec![number(self, CellRef::new(top, c))])
                .collect();
            let outputs: Vec<(usize, CellRef)> = rows
                .iter()
                .map(|&row| (idx, CellRef::new(row, left)))
                .collect();
            let got = self.probe(idx, &[input], &sets, &outputs)?;
            for (ci, &col) in cols.iter().enumerate() {
                for (ri, &row) in rows.iter().enumerate() {
                    out.push((CellRef::new(row, col), got[ci][ri].clone()));
                }
            }
        } else {
            // Values down the left column, formulas along the top row.
            let Some(input) = def.r1 else {
                return Ok(Vec::new());
            };
            let sets: Vec<Vec<f64>> = rows
                .iter()
                .map(|&row| vec![number(self, CellRef::new(row, left))])
                .collect();
            let outputs: Vec<(usize, CellRef)> =
                cols.iter().map(|&c| (idx, CellRef::new(top, c))).collect();
            let got = self.probe(idx, &[input], &sets, &outputs)?;
            for (ri, &row) in rows.iter().enumerate() {
                for (ci, &col) in cols.iter().enumerate() {
                    out.push((CellRef::new(row, col), got[ri][ci].clone()));
                }
            }
        }
        out.sort_by_key(|(at, _)| *at);
        Ok(out)
    }

    /// Every data table computed again (Calculate Now): their cells
    /// rewritten in place, the formulas reading them computed after.
    pub(crate) fn refill_data_tables(&mut self) -> Result<()> {
        let tables: Vec<(usize, DataTableDef)> = self
            .loaded
            .iter()
            .flat_map(|(i, (_, m))| m.data_tables.iter().map(|t| (*i, t.clone())))
            .collect();
        if tables.is_empty() {
            return Ok(());
        }
        let mut changed = Vec::new();
        for (idx, def) in tables {
            let values = self.data_table_values(idx, &def)?;
            let (text, model) = &self.loaded[&idx];
            let p = model.prefix.clone();
            let mut splices = Vec::new();
            for (at, v) in &values {
                let Some(c) = model.cells.get(at) else {
                    continue;
                };
                let xml = if *at == def.range.start {
                    value_cell(&p, *at, c.style, v, &table_f(&p, &def))
                } else {
                    cell_xml(&p, *at, c.style, &input_of(v))
                };
                splices.push((c.span.clone(), xml));
            }
            let new = splice(text, splices);
            self.replace_sheet_text(idx, new);
            changed.extend(values.into_iter().map(|(at, v)| (idx, at, v)));
        }
        let cells = self.formula_cells();
        if let Some(Ok(engine)) = self.engine.as_mut() {
            for (idx, at, v) in &changed {
                let _ = engine.set(*idx, *at, None, v);
            }
            let after = engine.evaluate(&cells).clone();
            let before = std::mem::take(&mut self.computed);
            self.write_results(&before, after, &cells, &[]);
        }
        Ok(())
    }

    /// Clears a data table whole: its results and its `<f>`.
    pub(crate) fn clear_data_table(&mut self, idx: usize, range: Range) -> Result<()> {
        let cells: Vec<CellRef> = self.loaded[&idx]
            .1
            .cells
            .keys()
            .filter(|at| range.contains(**at))
            .copied()
            .collect();
        self.in_one_step(|wb| {
            wb.filling_table = true;
            let r = (|| -> Result<()> {
                for at in cells {
                    wb.set_input(idx, at, Input::Clear)?;
                }
                Ok(())
            })();
            wb.filling_table = false;
            wb.batch_changed = true;
            r
        })
    }

    /// The scenarios of sheet `idx`.
    pub fn scenarios(&mut self, idx: usize) -> Vec<ScenarioDef> {
        if self.load(idx).is_err() {
            return Vec::new();
        }
        parse_scenarios(&self.loaded[&idx].0).0
    }

    /// Writes sheet `idx`'s scenarios (none: `<scenarios>` taken away).
    fn write_scenarios(&mut self, idx: usize, list: &[ScenarioDef]) -> Result<()> {
        let text = self.loaded[&idx].0.clone();
        let p = self.loaded[&idx].1.prefix.clone();
        let element = (!list.is_empty()).then(|| {
            let items: String = list
                .iter()
                .map(|s| {
                    let cells: String = s
                        .cells
                        .iter()
                        .map(|(at, v)| {
                            format!("<{p}inputCells r=\"{at}\" val=\"{}\"/>", xml::escape(v))
                        })
                        .collect();
                    let comment = if s.comment.is_empty() {
                        String::new()
                    } else {
                        format!(" comment=\"{}\"", xml::escape(&s.comment))
                    };
                    format!(
                        "<{p}scenario name=\"{}\" locked=\"1\" count=\"{}\" user=\"Kalem\"{comment}>{cells}</{p}scenario>",
                        xml::escape(&s.name),
                        s.cells.len()
                    )
                })
                .collect();
            format!("<{p}scenarios current=\"0\" show=\"0\">{items}</{p}scenarios>")
        });
        let new = match (parse_scenarios(&text).1, element) {
            (Some(span), Some(e)) => splice(&text, vec![(span, e)]),
            (Some(span), None) => splice(&text, vec![(span, String::new())]),
            (None, Some(e)) => {
                let mut before = vec!["autoFilter"];
                before.extend(AFTER_AUTO_FILTER);
                insert_top_level(&text, &before, &e)
            }
            (None, None) => return Ok(()),
        };
        self.in_one_step(|wb| {
            wb.replace_sheet_text(idx, new);
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Keeps the cells' values now as scenario `name` (one of that name
    /// replaced). One undo step.
    pub fn add_scenario(
        &mut self,
        idx: usize,
        name: &str,
        cells: &[CellRef],
        comment: &str,
    ) -> Result<()> {
        self.load(idx)?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 255 {
            return Err(Error::Refused(
                "A scenario's name is 1 to 255 characters".into(),
            ));
        }
        if cells.is_empty() || cells.len() > 32 {
            return Err(Error::Refused("A scenario changes 1 to 32 cells".into()));
        }
        let mut kept = Vec::new();
        for at in cells {
            let v = match self.value(idx, *at)? {
                Value::Number(n) => number_text(n),
                Value::Text(t) => t,
                Value::Bool(b) => if b { "TRUE" } else { "FALSE" }.into(),
                Value::Error(e) => e,
                Value::Empty => String::new(),
            };
            kept.push((*at, v));
        }
        let mut list = self.scenarios(idx);
        let new = ScenarioDef {
            name: name.to_owned(),
            comment: comment.to_owned(),
            cells: kept,
        };
        match list.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name)) {
            Some(s) => *s = new,
            None => list.push(new),
        }
        self.write_scenarios(idx, &list)
    }

    /// Puts scenario `name`'s values into its cells. One undo step.
    pub fn show_scenario(&mut self, idx: usize, name: &str) -> Result<()> {
        let Some(s) = self
            .scenarios(idx)
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
        else {
            return Err(Error::Refused(format!("No scenario {name}")));
        };
        self.in_one_step(|wb| {
            for (at, v) in &s.cells {
                // Kept as values: a text that reads as a formula stays text.
                let input = match Input::parse(v, wb.date1904) {
                    Input::Formula(_) => Input::Text(v.clone()),
                    i => i,
                };
                wb.set_input(idx, *at, input)?;
            }
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Deletes scenario `name`. One undo step.
    pub fn delete_scenario(&mut self, idx: usize, name: &str) -> Result<()> {
        let mut list = self.scenarios(idx);
        let before = list.len();
        list.retain(|s| !s.name.eq_ignore_ascii_case(name));
        if list.len() == before {
            return Err(Error::Refused(format!("No scenario {name}")));
        }
        self.write_scenarios(idx, &list)
    }
}
