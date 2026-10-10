//! Outlines: rows and columns grouped as Excel's Group writes them
//! (`outlineLevel` on `<row>` and `<col>`, the sheet's
//! `outlineLevelRow`/`outlineLevelCol`), collapsed and expanded (`hidden`,
//! and `collapsed` on the summary row or column after a group), and
//! Subtotal, which puts a total under each group of a table and outlines it.

use super::*;

/// An attribute of a `<row>` set (`Some`) or taken away (`None`).
type RowEdit = (&'static str, Option<String>);

/// The rows and the columns grouped, each with its level.
pub type Outline = (Vec<(u32, u8)>, Vec<(u32, u8)>);

impl Workbook {
    /// The rows and the columns grouped, each with its level.
    pub fn outline(&mut self, idx: usize) -> Result<Outline> {
        self.load(idx)?;
        let m = &self.loaded[&idx].1;
        let rows = m
            .rows
            .iter()
            .filter(|(_, r)| r.level > 0)
            .map(|(i, r)| (*i, r.level))
            .collect();
        let cols = m
            .cols
            .iter()
            .filter(|c| c.level > 0)
            .flat_map(|c| (c.min..=c.max.min(c.min + 1024)).map(move |i| (i, c.level)))
            .collect();
        Ok((rows, cols))
    }

    fn row_level(&self, idx: usize, r: u32) -> u8 {
        self.loaded[&idx].1.rows.get(&r).map_or(0, |x| x.level)
    }

    fn col_level(&self, idx: usize, c: u32) -> u8 {
        self.loaded[&idx]
            .1
            .cols
            .iter()
            .find(|x| (x.min..=x.max).contains(&c))
            .map_or(0, |x| x.level)
    }

    /// Rows' attributes set or taken away, rows made for those that need
    /// one.
    fn edit_rows(&mut self, idx: usize, edits: &BTreeMap<u32, Vec<RowEdit>>) -> Result<()> {
        let (text, model) = &*self.loaded[&idx];
        let p = model.prefix.clone();
        let Some((sd_start, sd_end)) = model.sheet_data.clone() else {
            return Err(Error::Refused("the sheet part has no sheetData".into()));
        };
        let mut splices: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        let mut inserts: BTreeMap<usize, String> = BTreeMap::new();
        let mut fresh = String::new();
        for (r, attrs) in edits {
            match model.rows.get(r) {
                Some(row) => {
                    let mut tag = text[row.start.clone()].to_owned();
                    for (k, v) in attrs {
                        tag = match v {
                            Some(v) => xml::set_attr(&tag, k, v),
                            None => xml::remove_attr(&tag, k),
                        };
                    }
                    if tag != text[row.start.clone()] {
                        splices.push((row.start.clone(), tag));
                    }
                }
                None => {
                    let set: String = attrs
                        .iter()
                        .filter_map(|(k, v)| v.as_ref().map(|v| format!(" {k}=\"{v}\"")))
                        .collect();
                    if set.is_empty() {
                        continue;
                    }
                    let el = format!("<{p}row r=\"{}\"{set}/>", r + 1);
                    if sd_end.is_none() {
                        fresh.push_str(&el);
                        continue;
                    }
                    let at = model
                        .rows
                        .range(..r)
                        .next_back()
                        .map_or(sd_start.end, |(_, row)| {
                            row.end.as_ref().map_or(row.start.end, |e| e.end)
                        });
                    inserts.entry(at).or_default().push_str(&el);
                }
            }
        }
        if !fresh.is_empty() {
            let tag = &text[sd_start.clone()];
            let open = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
            splices.push((sd_start.clone(), format!("{open}>{fresh}</{p}sheetData>")));
        }
        splices.extend(inserts.into_iter().map(|(at, s)| (at..at, s)));
        splices.sort_by_key(|s| (s.0.start, s.0.end));
        let new = splice(text, splices);
        self.replace_sheet_text(idx, new);
        Ok(())
    }

    /// The sheet's deepest row or column level written on its
    /// `<sheetFormatPr>`.
    fn note_outline_levels(&mut self, idx: usize) {
        let m = &self.loaded[&idx].1;
        let rows = m.rows.values().map(|r| r.level).max().unwrap_or(0);
        let cols = m.cols.iter().map(|c| c.level).max().unwrap_or(0);
        let text = self.loaded[&idx].0.clone();
        let p = m.prefix.clone();
        let mut r = Reader::new(&text);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            if tag.name == "sheetFormatPr" {
                let mut h = text[tag.span.clone()].to_owned();
                for (k, v) in [("outlineLevelRow", rows), ("outlineLevelCol", cols)] {
                    h = if v > 0 {
                        xml::set_attr(&h, k, &v.to_string())
                    } else {
                        xml::remove_attr(&h, k)
                    };
                }
                let new = splice(&text, vec![(tag.span.clone(), h)]);
                self.replace_sheet_text(idx, new);
                return;
            }
            if tag.name == "sheetData" {
                break;
            }
        }
        if rows == 0 && cols == 0 {
            return;
        }
        let mut el = format!("<{p}sheetFormatPr defaultRowHeight=\"15\"");
        if rows > 0 {
            el.push_str(&format!(" outlineLevelRow=\"{rows}\""));
        }
        if cols > 0 {
            el.push_str(&format!(" outlineLevelCol=\"{cols}\""));
        }
        el.push_str("/>");
        let new = insert_top_level(&text, &["cols", "sheetData"], &el);
        self.replace_sheet_text(idx, new);
    }

    /// Group (`deeper`) or Ungroup rows or columns `from..=to`: their
    /// outline level one deeper (seven at most) or one less. One undo step.
    pub fn set_outline(
        &mut self,
        idx: usize,
        rows: bool,
        from: u32,
        to: u32,
        deeper: bool,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let (from, to) = (from.min(to), from.max(to));
        if to - from >= 100_000 {
            return Err(Error::Refused(
                "Group 100,000 rows or columns at most".into(),
            ));
        }
        self.in_one_step(|wb| {
            let next = |l: u8| {
                if deeper {
                    (l + 1).min(7)
                } else {
                    l.saturating_sub(1)
                }
            };
            if rows {
                let mut edits = BTreeMap::new();
                for r in from..=to {
                    let l = next(wb.row_level(idx, r));
                    edits.insert(r, vec![("outlineLevel", (l > 0).then(|| l.to_string()))]);
                }
                wb.edit_rows(idx, &edits)?;
            } else {
                let mut text = wb.loaded[&idx].0.clone();
                for c in from..=to {
                    let l = next(wb.col_level(idx, c));
                    if l == 0 && wb.col_level(idx, c) == 0 {
                        continue;
                    }
                    text = col_attrs_text(&text, c + 1, &|tag: &str| {
                        if l > 0 {
                            xml::set_attr(tag, "outlineLevel", &l.to_string())
                        } else {
                            xml::remove_attr(tag, "outlineLevel")
                        }
                    });
                }
                wb.replace_sheet_text(idx, text);
            }
            wb.note_outline_levels(idx);
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// The group `at` is in, or sums up (the row or column right after a
    /// group): its first and last, and the summary row or column after it.
    fn group_of(&self, idx: usize, rows: bool, at: u32) -> Option<(u32, u32, u32)> {
        let level = |i: u32| {
            if rows {
                self.row_level(idx, i)
            } else {
                self.col_level(idx, i)
            }
        };
        // A summary row (after a deeper group) stands for the group above
        // it; else the group the row is in.
        let (l, start) = if at > 0 && level(at - 1) > level(at) {
            (level(at) + 1, at - 1)
        } else if level(at) > 0 {
            (level(at), at)
        } else {
            return None;
        };
        let mut a = start;
        while a > 0 && level(a - 1) >= l {
            a -= 1;
        }
        let mut b = start;
        while level(b + 1) >= l {
            b += 1;
        }
        Some((a, b, b + 1))
    }

    /// Hide Detail (`shown` off) or Show Detail of the group `at` is in or
    /// sums up: its rows or columns hidden or shown, its summary row or
    /// column marked `collapsed`. One undo step.
    pub fn set_detail_shown(&mut self, idx: usize, rows: bool, at: u32, shown: bool) -> Result<()> {
        self.load(idx)?;
        let Some((a, b, summary)) = self.group_of(idx, rows, at) else {
            return Err(Error::Refused("The cursor is in no group".into()));
        };
        self.in_one_step(|wb| {
            if rows {
                let mut edits: BTreeMap<u32, Vec<RowEdit>> = BTreeMap::new();
                for r in a..=b {
                    edits.insert(r, vec![("hidden", (!shown).then(|| "1".to_owned()))]);
                }
                edits
                    .entry(summary)
                    .or_default()
                    .push(("collapsed", (!shown).then(|| "1".to_owned())));
                wb.edit_rows(idx, &edits)?;
            } else {
                let mut text = wb.loaded[&idx].0.clone();
                for c in a..=b {
                    text = col_attrs_text(&text, c + 1, &|tag: &str| {
                        if shown {
                            xml::remove_attr(tag, "hidden")
                        } else {
                            xml::set_attr(tag, "hidden", "1")
                        }
                    });
                }
                text = col_attrs_text(&text, summary + 1, &|tag: &str| {
                    if shown {
                        xml::remove_attr(tag, "collapsed")
                    } else {
                        xml::set_attr(tag, "collapsed", "1")
                    }
                });
                wb.replace_sheet_text(idx, text);
            }
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Subtotal, as Excel's: under each run of rows of `range` (its first
    /// row the headers) with the same value in column `by`, a row with
    /// "… Total" and `SUBTOTAL(function, …)` of each of `columns`; a Grand
    /// Total at the end; the rows outlined (details 2, totals 1). One undo
    /// step.
    pub fn subtotal(
        &mut self,
        idx: usize,
        range: Range,
        by: u32,
        function: u32,
        columns: &[u32],
    ) -> Result<()> {
        self.load(idx)?;
        if !(range.start.col..=range.end.col).contains(&by) {
            return Err(Error::Refused("the column is outside the range".into()));
        }
        let first = range.start.row + 1;
        if first > range.end.row {
            return Err(Error::Refused(
                "A subtotal needs rows under the headers".into(),
            ));
        }
        let cols: Vec<u32> = columns
            .iter()
            .copied()
            .filter(|c| (range.start.col..=range.end.col).contains(c) && *c != by)
            .collect();
        if cols.is_empty() {
            return Err(Error::Refused("Add the subtotal to which column?".into()));
        }
        // The runs of the same value, top to bottom.
        let mut groups: Vec<(u32, u32, String)> = Vec::new();
        for r in first..=range.end.row {
            let v = self.display(idx, CellRef::new(r, by))?;
            match groups.last_mut() {
                Some(g) if g.2.to_lowercase() == v.to_lowercase() => g.1 = r,
                _ => groups.push((r, r, v)),
            }
        }
        let letter = |c: u32| crate::cellref::column_name(c);
        self.in_one_step(|wb| {
            let sum = |wb: &mut Self, at: u32, label: String, a: u32, b: u32| -> Result<()> {
                wb.set_input(idx, CellRef::new(at, by), Input::Text(label))?;
                for &c in &cols {
                    let f = format!(
                        "SUBTOTAL({function},{l}{}:{l}{})",
                        a + 1,
                        b + 1,
                        l = letter(c)
                    );
                    wb.set_input(idx, CellRef::new(at, c), Input::Formula(f))?;
                }
                Ok(())
            };
            // The grand total first, then each group's from the bottom up:
            // rows put in above it move its range with them.
            wb.insert_rows(idx, range.end.row + 1, 1)?;
            sum(
                wb,
                range.end.row + 1,
                "Grand Total".into(),
                first,
                range.end.row,
            )?;
            for (a, b, v) in groups.iter().rev() {
                wb.insert_rows(idx, b + 1, 1)?;
                sum(wb, b + 1, format!("{v} Total"), *a, *b)?;
            }
            // The outline: details at 2, the totals at 1, from the top.
            let mut edits: BTreeMap<u32, Vec<RowEdit>> = BTreeMap::new();
            let mut r = first;
            for (a, b, _) in &groups {
                for _ in *a..=*b {
                    edits.insert(r, vec![("outlineLevel", Some("2".into()))]);
                    r += 1;
                }
                edits.insert(r, vec![("outlineLevel", Some("1".into()))]);
                r += 1;
            }
            wb.edit_rows(idx, &edits)?;
            wb.note_outline_levels(idx);
            wb.batch_changed = true;
            Ok(())
        })
    }
}
