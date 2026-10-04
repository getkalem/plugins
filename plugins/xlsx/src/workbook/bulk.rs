//! Many cells written at once: the sheet's text changed in one pass and
//! read once, the engine told of every cell and computing once, instead
//! of a pass and a reading a cell (a paste of 36,000 cells took minutes).
//! A cell that needs more than its own `<c>` (the first cell of a shared
//! formula overwritten) is written the cell-by-cell way after the others.

use super::*;

impl Workbook {
    /// Writes `edits` into sheet `idx`, each as `set_input` would, in one
    /// undo step; a cell `set_input` would refuse refuses them all.
    pub(crate) fn set_inputs(&mut self, idx: usize, edits: Vec<(CellRef, Input)>) -> Result<()> {
        self.set_inputs_styled(idx, edits.into_iter().map(|(a, i)| (a, i, None)).collect())
    }

    /// `set_inputs` with each cell's style given (`Some`), as a sort or a
    /// move carries it, instead of the one the cell has.
    pub(crate) fn set_inputs_styled(
        &mut self,
        idx: usize,
        edits: Vec<(CellRef, Input, Option<u32>)>,
    ) -> Result<()> {
        self.load(idx)?;
        if edits.is_empty() {
            return Ok(());
        }
        // The last entry for a cell wins.
        let mut by_cell: BTreeMap<CellRef, (Input, Option<u32>)> = BTreeMap::new();
        for (at, input, style) in edits {
            by_cell.insert(at, (input, style));
        }
        let mut slow: Vec<(CellRef, Input, Option<u32>)> = Vec::new();
        let mut fast: Vec<(CellRef, Input, Option<u32>)> = Vec::new();
        {
            for (&at, (input, style)) in &by_cell {
                self.check_cell_edit(idx, at)?;
                let model = &self.loaded[&idx].1;
                if !self.filling_table && model.data_tables.iter().any(|t| t.range.contains(at)) {
                    // A data table's own rule (cleared whole) is the
                    // cell-by-cell way's.
                    slow.push((at, input.clone(), *style));
                    continue;
                }
                if let Some(m) = model.merge_at(at).filter(|m| m.start != at) {
                    return Err(Error::Refused(format!(
                        "{at} is inside the merged cell {m}; edit {}",
                        m.start
                    )));
                }
                let old = model.cells.get(&at);
                if let Some(c) = old {
                    if let Some(anchor) = c.in_array_of {
                        return Err(Error::Refused(format!(
                            "{at} is part of the array formula at {anchor}; an array is changed as a whole"
                        )));
                    }
                    match c.formula.as_ref().map(|f| &f.kind) {
                        Some(FormulaKind::Array { range }) if range.start != range.end => {
                            return Err(Error::Refused(format!(
                                "{at} holds an array formula over {range}; an array is changed as a whole"
                            )));
                        }
                        Some(FormulaKind::DataTable) if !self.filling_table => {
                            return Err(Error::Refused(format!("{at} is part of a data table")));
                        }
                        Some(FormulaKind::Shared { master: true, .. }) => {
                            slow.push((at, input.clone(), *style));
                            continue;
                        }
                        _ => {}
                    }
                }
                fast.push((at, input.clone(), *style));
            }
        }
        self.ensure_engine()?;
        self.in_one_step(|wb| {
            wb.write_many(idx, &fast)?;
            for (at, input, style) in slow {
                wb.set_input(idx, at, input)?;
                if let Some(s) = style {
                    wb.apply_style(idx, at, s)?;
                }
            }
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// The cells' `<c>`s written in one pass over the sheet's text.
    fn write_many(&mut self, idx: usize, cells: &[(CellRef, Input, Option<u32>)]) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }
        // Each cell's style: its own, else its row's or column's; a number
        // typed with a format gets it where the cell's is General.
        let mut xml: Vec<(CellRef, Option<String>)> = Vec::with_capacity(cells.len());
        let mut formulas = false;
        let mut removes_formula = false;
        for (at, input, given) in cells {
            let old = self.loaded[&idx].1.cells.get(at).cloned();
            let mut style = given.unwrap_or_else(|| {
                old.as_ref()
                    .map_or_else(|| self.row_or_col_style(idx, *at), |c| c.style)
            });
            if let Input::Number(_, Some(fmt)) = input
                && self.styles.get(style).num_fmt == "General"
            {
                style = self.style_with_numfmt(style, *fmt)?;
            }
            let is_formula = matches!(input, Input::Formula(_));
            formulas |= is_formula;
            removes_formula |= old.as_ref().is_some_and(|c| c.formula.is_some()) && !is_formula;
            let prefix = self.loaded[&idx].1.prefix.clone();
            let gone = matches!(input, Input::Clear) && style == 0;
            xml.push((*at, (!gone).then(|| cell_xml(&prefix, *at, style, input))));
        }
        let (text, model) = &self.loaded[&idx];
        let p = model.prefix.clone();
        let mut splices: Vec<(Span<usize>, String)> = Vec::new();
        let mut news: Vec<(CellRef, String)> = Vec::new();
        let mut grown: Option<Range> = None;
        for (at, c) in &xml {
            if c.is_some() {
                grown = Some(match grown {
                    None => Range {
                        start: *at,
                        end: *at,
                    },
                    Some(g) => Range {
                        start: CellRef::new(g.start.row.min(at.row), g.start.col.min(at.col)),
                        end: CellRef::new(g.end.row.max(at.row), g.end.col.max(at.col)),
                    },
                });
            }
            match (model.cells.get(at), c) {
                (Some(old), Some(c)) => splices.push((old.span.clone(), c.clone())),
                (Some(old), None) => splices.push((old.span.clone(), String::new())),
                (None, None) => {}
                (None, Some(c)) => news.push((*at, c.clone())),
            }
        }
        splices.extend(new_cells(text, model, &p, &news)?);
        if let (Some((span, dim)), Some(g)) = (&model.dimension, grown) {
            let whole = match Range::parse(dim) {
                Some(r) => Range {
                    start: CellRef::new(r.start.row.min(g.start.row), r.start.col.min(g.start.col)),
                    end: CellRef::new(r.end.row.max(g.end.row), r.end.col.max(g.end.col)),
                },
                None => g,
            };
            if Range::parse(dim) != Some(whole) {
                splices.push((
                    span.clone(),
                    xml::set_attr(&text[span.clone()], "ref", &whole.to_string()),
                ));
            }
        }
        let new_text = splice(text, splices);
        self.replace_sheet_text(idx, new_text);
        // The engine told of every cell; in the step's batch, computed once.
        for (at, ..) in cells {
            self.recompute_after(idx, *at)?;
        }
        if formulas || self.any_formula()? {
            self.set_full_calc_on_load();
        }
        if removes_formula {
            self.drop_calc_chain()?;
        }
        Ok(())
    }

    /// Cells given styles (an index each) in one pass over the sheet's text:
    /// a cell there gets its `s`, a missing one is made empty with it.
    pub(crate) fn style_many(&mut self, idx: usize, cells: &[(CellRef, u32)]) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }
        let (text, model) = &self.loaded[&idx];
        let p = model.prefix.clone();
        let mut splices: Vec<(Span<usize>, String)> = Vec::new();
        let mut news: Vec<(CellRef, String)> = Vec::new();
        let mut sorted: Vec<(CellRef, u32)> = cells.to_vec();
        sorted.sort_by_key(|c| c.0);
        sorted.dedup_by_key(|c| c.0);
        for (at, style) in &sorted {
            match model.cells.get(at) {
                Some(c) => {
                    let el = &text[c.span.clone()];
                    let tag_end = el.find('>').map_or(el.len(), |q| q + 1);
                    let head = if *style == 0 {
                        xml::remove_attr(&el[..tag_end], "s")
                    } else {
                        xml::set_attr(&el[..tag_end], "s", &style.to_string())
                    };
                    splices.push((c.span.start..c.span.start + tag_end, head));
                }
                None if *style == 0 => {}
                None => news.push((*at, format!("<{p}c r=\"{at}\" s=\"{style}\"/>"))),
            }
        }
        splices.extend(new_cells(text, model, &p, &news)?);
        let new_text = splice(text, splices);
        self.replace_sheet_text(idx, new_text);
        Ok(())
    }
}

/// Where new cells' `<c>`s (in order) go in a sheet's text: in a row that
/// is there after the cell before them (its `spans` widened), a `<row/>`
/// opened up, or new rows after the row before them.
fn new_cells(
    text: &str,
    model: &Sheet,
    p: &str,
    news: &[(CellRef, String)],
) -> Result<Vec<(Span<usize>, String)>> {
    let mut splices = Vec::new();
    let mut in_rows: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut open_rows: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut new_rows: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut widest: BTreeMap<u32, u32> = BTreeMap::new();
    for (at, c) in news {
        match model.rows.get(&at.row) {
            Some(row) if row.end.is_some() => {
                let before = model
                    .cells
                    .range(CellRef::new(at.row, 0)..*at)
                    .next_back()
                    .map_or(row.start.end, |(_, cell)| cell.span.end);
                in_rows.entry(before).or_default().push(c.clone());
                let w = widest.entry(at.row).or_insert(at.col);
                *w = (*w).max(at.col);
            }
            Some(_) => {
                open_rows.entry(at.row).or_default().push(c.clone());
                let w = widest.entry(at.row).or_insert(at.col);
                *w = (*w).max(at.col);
            }
            None => new_rows.entry(at.row).or_default().push(c.clone()),
        }
    }
    for (pos, cs) in in_rows {
        splices.push((pos..pos, cs.concat()));
    }
    for (r, w) in &widest {
        let Some(row) = model.rows.get(r) else {
            continue;
        };
        let tag = &text[row.start.clone()];
        let widened = widen_spans(tag, *w);
        match open_rows.remove(r) {
            // `<row …/>` opens up to hold its cells.
            Some(cs) => {
                let open = widened
                    .trim_end_matches('>')
                    .trim_end_matches('/')
                    .trim_end();
                splices.push((
                    row.start.clone(),
                    format!("{open}>{}</{p}row>", cs.concat()),
                ));
            }
            None if widened != tag => splices.push((row.start.clone(), widened)),
            None => {}
        }
    }
    if !new_rows.is_empty() {
        let Some((sd_start, sd_end)) = &model.sheet_data else {
            return Err(Error::Refused("the sheet part has no sheetData".into()));
        };
        let mut at_pos: BTreeMap<usize, String> = BTreeMap::new();
        for (r, cs) in &new_rows {
            let row = format!("<{p}row r=\"{}\">{}</{p}row>", r + 1, cs.concat());
            let pos = match sd_end {
                None => usize::MAX,
                Some(_) => model
                    .rows
                    .range(..*r)
                    .next_back()
                    .map_or(sd_start.end, |(_, x)| {
                        x.end.as_ref().map_or(x.start.end, |e| e.end)
                    }),
            };
            at_pos.entry(pos).or_default().push_str(&row);
        }
        for (pos, rows) in at_pos {
            if pos == usize::MAX {
                // `<sheetData/>` opens up.
                let tag = &text[sd_start.clone()];
                let open = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
                splices.push((sd_start.clone(), format!("{open}>{rows}</{p}sheetData>")));
            } else {
                splices.push((pos..pos, rows));
            }
        }
    }
    Ok(splices)
}
