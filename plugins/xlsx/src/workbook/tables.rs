//! Tables (Excel's Format as Table): a table part per table with its
//! name, range, columns and style, named by the sheet's `<tableParts>`; a
//! total row; the table growing when a row is typed under it; turned back
//! into a range with its structured references made plain ones.

use super::*;

const TABLE_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";

/// A table as read from its part.
#[derive(Debug, Clone, PartialEq)]
pub struct TableDef {
    /// Its name (`displayName`, which formulas use).
    pub name: String,
    /// Its `id`, unique in the workbook.
    pub id: u32,
    /// The sheet it is on.
    pub sheet: usize,
    /// Its part.
    pub part: String,
    /// Its range, the header and total rows included.
    pub range: Range,
    /// It has a header row.
    pub header: bool,
    /// It has a total row.
    pub totals: bool,
    /// Its columns' names.
    pub columns: Vec<String>,
    /// Its style's name.
    pub style: String,
    /// Its rows are banded.
    pub stripes: bool,
}

fn parse_table(text: &str, sheet: usize, part: &str) -> Option<TableDef> {
    let mut r = Reader::new(text);
    let mut def: Option<TableDef> = None;
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        match tag.name {
            "table" => {
                let count = |k: &str, d: u32| tag.attr(k).and_then(|v| v.parse().ok()).unwrap_or(d);
                def = Some(TableDef {
                    name: tag
                        .attr("displayName")
                        .or_else(|| tag.attr("name"))?
                        .into_owned(),
                    id: count("id", 0),
                    sheet,
                    part: part.to_owned(),
                    range: Range::parse(&tag.attr("ref")?)?,
                    header: count("headerRowCount", 1) > 0,
                    totals: count("totalsRowCount", 0) > 0,
                    columns: Vec::new(),
                    style: String::new(),
                    stripes: true,
                });
            }
            "tableColumn" => {
                if let (Some(d), Some(n)) = (def.as_mut(), tag.attr("name")) {
                    d.columns.push(n.into_owned());
                }
            }
            "tableStyleInfo" => {
                if let Some(d) = def.as_mut() {
                    d.style = tag.attr("name").map(|v| v.into_owned()).unwrap_or_default();
                    d.stripes = tag
                        .attr("showRowStripes")
                        .as_deref()
                        .is_none_or(|v| v == "1" || v == "true");
                }
            }
            _ => {}
        }
    }
    def
}

/// The colors a table's style draws: the header's fill and text, and the
/// banded rows' fill (Excel's medium styles; the others as Medium 2).
pub fn style_colors(style: &str) -> ([u8; 3], [u8; 3], [u8; 3]) {
    let white = [0xFF, 0xFF, 0xFF];
    match style {
        "TableStyleMedium3" => ([0xED, 0x7D, 0x31], white, [0xFB, 0xE2, 0xD5]),
        "TableStyleMedium4" => ([0xA5, 0xA5, 0xA5], white, [0xED, 0xED, 0xED]),
        "TableStyleMedium5" => ([0xFF, 0xC0, 0x00], white, [0xFF, 0xF2, 0xCC]),
        "TableStyleMedium6" => ([0x5B, 0x9B, 0xD5], white, [0xDD, 0xEB, 0xF7]),
        "TableStyleMedium7" => ([0x70, 0xAD, 0x47], white, [0xE2, 0xEF, 0xDA]),
        _ => ([0x44, 0x72, 0xC4], white, [0xD9, 0xE1, 0xF2]),
    }
}

impl Workbook {
    /// Every table of the workbook, sheet by sheet.
    pub fn tables(&self) -> Result<Vec<TableDef>> {
        let mut out = Vec::new();
        for i in 0..self.sheets.len() {
            if self.sheets[i].kind != SheetKind::Worksheet {
                continue;
            }
            for (kind, part) in self.sheet_parts(i)? {
                if kind != "table" {
                    continue;
                }
                let text = text_of(self.pkg.part(&part)?, &part)?;
                if let Some(t) = parse_table(&text, i, &part) {
                    out.push(t);
                }
            }
        }
        Ok(out)
    }

    /// The tables of one sheet.
    pub fn sheet_tables(&self, idx: usize) -> Vec<TableDef> {
        self.tables()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.sheet == idx)
            .collect()
    }

    /// Format as Table: `range` made a table in `style`, its first row the
    /// headers (else a row of `Column1`… put over it, the cells under it
    /// moved down); a filter on the sheet over it taken off. One undo step;
    /// the table's name.
    pub fn create_table(
        &mut self,
        idx: usize,
        range: Range,
        header: bool,
        style: &str,
    ) -> Result<String> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let overlaps = |a: &Range, b: &Range| {
            a.start.row <= b.end.row
                && b.start.row <= a.end.row
                && a.start.col <= b.end.col
                && b.start.col <= a.end.col
        };
        let tables = self.tables()?;
        if let Some(t) = tables
            .iter()
            .find(|t| t.sheet == idx && overlaps(&t.range, &range))
        {
            return Err(Error::Refused(format!(
                "the table {} is in the way",
                t.name
            )));
        }
        if let Some(m) = self.loaded[&idx]
            .1
            .merged
            .iter()
            .find(|m| overlaps(m, &range))
        {
            return Err(Error::Refused(format!(
                "the merged cell {m} is in the range; unmerge it first"
            )));
        }
        let taken: Vec<String> = tables
            .iter()
            .map(|t| t.name.to_lowercase())
            .chain(self.defined_names.iter().map(|d| d.name.to_lowercase()))
            .collect();
        let name = (1..)
            .map(|n| format!("Table{n}"))
            .find(|n| !taken.contains(&n.to_lowercase()))
            .expect("a free name");
        let id = tables.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let style = if style.is_empty() {
            "TableStyleMedium2"
        } else {
            style
        };
        let made = name.clone();
        self.in_one_step(|wb| {
            let mut range = range;
            if !header {
                // A header row over the table: its cells move down a row.
                let last = wb.loaded[&idx]
                    .1
                    .used_range()
                    .map_or(range.end.row, |u| u.end.row.max(range.end.row));
                let block = Range {
                    start: range.start,
                    end: CellRef::new(last, range.end.col),
                };
                wb.move_range(idx, block, CellRef::new(range.start.row + 1, range.start.col))?;
                range.end.row += 1;
            }
            // Column names: the headers, empty and repeated ones made unique,
            // written as text.
            let mut columns: Vec<String> = Vec::new();
            for (k, c) in (range.start.col..=range.end.col).enumerate() {
                let at = CellRef::new(range.start.row, c);
                let shown = if header {
                    wb.display(idx, at)?.trim().to_owned()
                } else {
                    String::new()
                };
                let base = if shown.is_empty() {
                    format!("Column{}", k + 1)
                } else {
                    shown.clone()
                };
                let mut n = base.clone();
                let mut i = 2;
                while columns.iter().any(|x| x.to_lowercase() == n.to_lowercase()) {
                    n = format!("{base}{i}");
                    i += 1;
                }
                let is_text = matches!(wb.value(idx, at)?, Value::Text(_));
                if !is_text || n != shown {
                    wb.set_input(idx, at, Input::Text(n.clone()))?;
                }
                columns.push(n);
            }
            // A sheet filter over the table gives way to the table's own.
            if let Some(af) = wb.loaded[&idx].1.auto_filter.clone()
                && overlaps(&af.range, &range)
            {
                wb.set_filter(idx, None)?;
            }
            let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
            let cols: String = columns
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    format!(
                        "<tableColumn id=\"{}\" name=\"{}\"/>",
                        i + 1,
                        xml::escape(n)
                    )
                })
                .collect();
            let text = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<table xmlns=\"{main}\" id=\"{id}\" name=\"{name}\" displayName=\"{name}\" ref=\"{range}\" totalsRowShown=\"0\"><autoFilter ref=\"{range}\"/><tableColumns count=\"{}\">{cols}</tableColumns><tableStyleInfo name=\"{}\" showFirstColumn=\"0\" showLastColumn=\"0\" showRowStripes=\"1\" showColumnStripes=\"0\"/></table>",
                columns.len(),
                xml::escape(style)
            );
            let part = wb.free_part("xl/tables/table");
            wb.add_part(&part, text, TABLE_TYPE)?;
            let sheet_part = wb.sheets[idx].part.clone();
            let rid = wb.add_rel(&sheet_part, "table", &part)?;
            wb.add_table_part(idx, &rid);
            wb.engine = None;
            wb.batch_changed = true;
            Ok(())
        })?;
        Ok(made)
    }

    /// The sheet's `<tablePart>` for relationship `rid`, `<tableParts>`
    /// made or counted again.
    fn add_table_part(&mut self, idx: usize, rid: &str) {
        let text = self.loaded[&idx].0.clone();
        let p = self.loaded[&idx].1.prefix.clone();
        let ns = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let head_end = text.find("<sheetData").unwrap_or(text.len());
        let rp = text[..head_end].split("xmlns:").skip(1).find_map(|d| {
            let (name, rest) = d.split_once("=\"")?;
            rest.starts_with(ns).then(|| name.to_owned())
        });
        let el = match &rp {
            Some(r) => format!("<{p}tablePart {r}:id=\"{rid}\"/>"),
            None => format!("<{p}tablePart xmlns:r=\"{ns}\" r:id=\"{rid}\"/>"),
        };
        let mut r = Reader::new(&text);
        let mut list: Option<(std::ops::Range<usize>, bool, usize)> = None;
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "tableParts"
            {
                let count = tag.attr("count").and_then(|v| v.parse().ok()).unwrap_or(0);
                list = Some((tag.span.clone(), tag.empty, count));
                break;
            }
        }
        let new = match list {
            Some((span, empty, count)) => {
                let open = xml::set_attr(&text[span.clone()], "count", &(count + 1).to_string());
                if empty {
                    let o = open.trim_end_matches("/>").trim_end().to_owned();
                    splice(&text, vec![(span, format!("{o}>{el}</{p}tableParts>"))])
                } else {
                    let end = text[span.end..]
                        .find(&format!("</{p}tableParts>"))
                        .map_or(span.end, |e| span.end + e);
                    splice(&text, vec![(span, open), (end..end, el)])
                }
            }
            None => insert_top_level(
                &text,
                &["extLst"],
                &format!("<{p}tableParts count=\"1\">{el}</{p}tableParts>"),
            ),
        };
        self.replace_sheet_text(idx, new);
    }

    /// A table by its name.
    fn table_named(&self, idx: usize, name: &str) -> Result<TableDef> {
        self.sheet_tables(idx)
            .into_iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| Error::Refused(format!("No table named {name}")))
    }

    /// A table part with its `<table ref>` and `<autoFilter ref>` (the rows
    /// above the total row) set again, and `totals` rows counted.
    fn set_table_range(&mut self, t: &TableDef, range: Range, totals: bool) -> Result<()> {
        let text = text_of(self.pkg.part(&t.part)?, &t.part)?;
        let data_end = if totals {
            range.end.row - 1
        } else {
            range.end.row
        };
        let filter = Range {
            start: range.start,
            end: CellRef::new(data_end, range.end.col),
        };
        let mut r = Reader::new(&text);
        let mut splices = Vec::new();
        while let Some(tok) = r.next_token() {
            let Token::Start(tag) = tok else { continue };
            match tag.name {
                "table" => {
                    let mut h = xml::set_attr(&text[tag.span.clone()], "ref", &range.to_string());
                    h = if totals {
                        xml::set_attr(&h, "totalsRowCount", "1")
                    } else {
                        xml::remove_attr(&h, "totalsRowCount")
                    };
                    h = xml::set_attr(&h, "totalsRowShown", if totals { "1" } else { "0" });
                    splices.push((tag.span.clone(), h));
                }
                "autoFilter" => splices.push((
                    tag.span.clone(),
                    xml::set_attr(&text[tag.span.clone()], "ref", &filter.to_string()),
                )),
                _ => {}
            }
        }
        self.pkg
            .set_part(&t.part, splice(&text, splices).into_bytes());
        Ok(())
    }

    /// A total row under a table (`Total` under its first column, the sum
    /// of its last as `SUBTOTAL(109,…)`), or taken away. One undo step.
    pub fn set_table_totals(&mut self, idx: usize, name: &str, on: bool) -> Result<()> {
        self.load(idx)?;
        let t = self.table_named(idx, name)?;
        if t.totals == on {
            return Ok(());
        }
        let below = t.range.end.row + 1;
        if on {
            for c in t.range.start.col..=t.range.end.col {
                if !self.display(idx, CellRef::new(below, c))?.is_empty() {
                    return Err(Error::Refused(
                        "the row under the table is not empty".into(),
                    ));
                }
            }
        }
        self.in_one_step(|wb| {
            let mut range = t.range;
            let text = text_of(wb.pkg.part(&t.part)?, &t.part)?;
            let (first, last) = (t.columns.first().cloned(), t.columns.last().cloned());
            let mut r = Reader::new(&text);
            let mut splices = Vec::new();
            let mut k = 0;
            while let Some(tok) = r.next_token() {
                let Token::Start(tag) = tok else { continue };
                if tag.name != "tableColumn" {
                    continue;
                }
                let mut h = text[tag.span.clone()].to_owned();
                if on && k == 0 {
                    h = xml::set_attr(&h, "totalsRowLabel", "Total");
                } else if on && k + 1 == t.columns.len() {
                    h = xml::set_attr(&h, "totalsRowFunction", "sum");
                } else if !on {
                    h = xml::remove_attr(
                        &xml::remove_attr(&h, "totalsRowLabel"),
                        "totalsRowFunction",
                    );
                }
                splices.push((tag.span.clone(), h));
                k += 1;
            }
            wb.pkg
                .set_part(&t.part, splice(&text, splices).into_bytes());
            if on {
                range.end.row += 1;
                let row = range.end.row;
                if let Some(f) = &first {
                    let _ = f;
                    wb.set_input(
                        idx,
                        CellRef::new(row, range.start.col),
                        Input::Text("Total".into()),
                    )?;
                }
                if let Some(l) = &last
                    && t.columns.len() > 1
                {
                    let f = format!("SUBTOTAL(109,{}[{}])", t.name, escape_column(l));
                    wb.set_input(idx, CellRef::new(row, range.end.col), Input::Formula(f))?;
                }
            } else {
                for c in range.start.col..=range.end.col {
                    wb.set_input(idx, CellRef::new(range.end.row, c), Input::Clear)?;
                }
                range.end.row -= 1;
            }
            wb.set_table_range(&t, range, on)?;
            wb.engine = None;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Convert to Range: the table's part, relationship and `<tablePart>`
    /// gone, its cells kept, formulas naming its columns given plain
    /// references. One undo step.
    pub fn remove_table(&mut self, idx: usize, name: &str) -> Result<()> {
        self.load(idx)?;
        let t = self.table_named(idx, name)?;
        self.in_one_step(|wb| {
            // Structured references made plain, on every sheet.
            let data = (
                t.range.start.row + u32::from(t.header),
                t.range.end.row - u32::from(t.totals),
            );
            let plain = |f: &str| -> String { plain_references(f, &t, data) };
            for i in 0..wb.sheets.len() {
                if wb.sheets[i].kind != SheetKind::Worksheet {
                    continue;
                }
                wb.load(i)?;
                let cells: Vec<(CellRef, String)> = wb.loaded[&i]
                    .1
                    .cells
                    .iter()
                    .filter_map(|(p, c)| c.formula.as_ref().map(|f| (*p, f.text.clone())))
                    .filter(|(_, f)| {
                        f.to_lowercase()
                            .contains(&format!("{}[", t.name.to_lowercase()))
                    })
                    .collect();
                for (p, f) in cells {
                    wb.set_input(i, p, Input::Formula(plain(&f)))?;
                }
            }
            // The part and what names it.
            let sheet_part = wb.sheets[idx].part.clone();
            let rels_path = rels::rels_path(&sheet_part);
            let rels = rels::parse(&text_of(wb.pkg.part(&rels_path)?, &rels_path)?);
            let rid = rels
                .iter()
                .find(|r| rels::resolve(&sheet_part, &r.target) == t.part)
                .map(|r| r.id.clone());
            if let Some(rid) = rid {
                let text = wb.loaded[&idx].0.clone();
                let mut r = Reader::new(&text);
                let mut splices = Vec::new();
                let mut count = 0;
                let mut list = None;
                while let Some(tok) = r.next_token() {
                    let Token::Start(tag) = tok else { continue };
                    match tag.name {
                        "tableParts" => list = Some(tag.span.clone()),
                        "tablePart" => {
                            if tag.attr("id").as_deref() == Some(rid.as_str()) {
                                splices.push((tag.span.clone(), String::new()));
                            } else {
                                count += 1;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(span) = list {
                    let h = xml::set_attr(&text[span.clone()], "count", &count.to_string());
                    splices.push((span, h));
                }
                let mut new = splice(&text, splices);
                if count == 0 {
                    let p = wb.loaded[&idx].1.prefix.clone();
                    if let (Some(a), Some(b)) = (
                        new.find(&format!("<{p}tableParts")),
                        new.find(&format!("</{p}tableParts>")),
                    ) {
                        new = format!(
                            "{}{}",
                            &new[..a],
                            &new[b + format!("</{p}tableParts>").len()..]
                        );
                    }
                }
                wb.replace_sheet_text(idx, new);
                wb.remove_rel_of(&sheet_part, &rid)?;
            }
            wb.remove_part_and_type(&t.part)?;
            wb.engine = None;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// A sheet's relationship taken away.
    fn remove_rel_of(&mut self, source: &str, rid: &str) -> Result<()> {
        let path = rels::rels_path(source);
        let text = text_of(self.pkg.part(&path)?, &path)?;
        let mut r = Reader::new(&text);
        let mut splices = Vec::new();
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "Relationship"
                && tag.attr("Id").as_deref() == Some(rid)
            {
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                splices.push((tag.span.start..end, String::new()));
            }
        }
        self.pkg
            .set_part(&path, splice(&text, splices).into_bytes());
        Ok(())
    }

    /// A part out of the package and its content types.
    fn remove_part_and_type(&mut self, name: &str) -> Result<()> {
        self.pkg.remove_part(name);
        let ct = "[Content_Types].xml";
        let types = text_of(self.pkg.part(ct)?, ct)?;
        let needle = format!("PartName=\"/{name}\"");
        let new = match types.find(&needle) {
            Some(at) => {
                let start = types[..at].rfind('<').unwrap_or(at);
                let end = types[at..].find("/>").map_or(types.len(), |e| at + e + 2);
                format!("{}{}", &types[..start], &types[end..])
            }
            None => types,
        };
        self.pkg.set_part(ct, new.into_bytes());
        Ok(())
    }

    /// After a cell under a table was given an entry: the table grows by
    /// that row, as Excel's AutoExpansion, when it has no total row.
    pub(crate) fn grow_table(&mut self, idx: usize, at: CellRef) -> Result<()> {
        for t in self.sheet_tables(idx) {
            if !t.totals
                && at.row == t.range.end.row + 1
                && (t.range.start.col..=t.range.end.col).contains(&at.col)
            {
                let mut range = t.range;
                range.end.row += 1;
                self.set_table_range(&t, range, false)?;
                self.engine = None;
            }
        }
        Ok(())
    }
}

/// A column's name as a structured reference writes it: `'`, `#`, `[` and
/// `]` escaped with `'`.
fn escape_column(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if matches!(c, '\'' | '#' | '[' | ']') {
            out.push('\'');
        }
        out.push(c);
    }
    out
}

/// A formula with `Table[Column]` and `Table[]` references to table `t`
/// made plain absolute ranges of its data rows `data`.
fn plain_references(f: &str, t: &TableDef, data: (u32, u32)) -> String {
    let lower = f.to_lowercase();
    let key = format!("{}[", t.name.to_lowercase());
    let mut out = String::new();
    let mut i = 0;
    while let Some(p) = lower[i..].find(&key) {
        let start = i + p;
        out.push_str(&f[i..start]);
        let open = start + key.len();
        let Some(close) = f[open..].find(']').map(|c| open + c) else {
            out.push_str(&f[start..]);
            return out;
        };
        let col = f[open..close].replace('\'', "");
        let cols: Vec<u32> = if col.is_empty() {
            vec![t.range.start.col, t.range.end.col]
        } else {
            match t.columns.iter().position(|c| c.eq_ignore_ascii_case(&col)) {
                Some(k) => vec![t.range.start.col + k as u32],
                None => {
                    out.push_str(&f[start..=close]);
                    i = close + 1;
                    continue;
                }
            }
        };
        let a = CellRef::new(data.0, cols[0]);
        let b = CellRef::new(data.1, *cols.last().unwrap_or(&cols[0]));
        let abs = |c: CellRef| {
            let s = c.to_string();
            let split = s.find(|ch: char| ch.is_ascii_digit()).unwrap_or(s.len());
            format!("${}${}", &s[..split], &s[split..])
        };
        out.push_str(&format!("{}:{}", abs(a), abs(b)));
        i = close + 1;
    }
    out.push_str(&f[i..]);
    out
}
