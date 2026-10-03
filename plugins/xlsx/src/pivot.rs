//! Pivot tables (ECMA-376 part 1, 18.10): computed from a source range and
//! written as Excel writes them — a cache definition, its records, the
//! table's definition, and its cells — in compact form, the layout Excel
//! gives a new pivot table. The cache asks to be refreshed on load, so
//! Excel and LibreOffice compute it again themselves.

use std::collections::HashMap;

use kalem_viewer::Aggregate;

use crate::cellref::{CellRef, Range};
use crate::sheet::Value;
use crate::xml;

/// The source: field names, and each record's values and their text.
#[derive(Debug, Clone, Default)]
pub struct Source {
    /// The fields' names (the first row).
    pub names: Vec<String>,
    /// The records' values.
    pub rows: Vec<Vec<Value>>,
    /// The records' values as shown, for item labels.
    pub shown: Vec<Vec<String>>,
}

/// What a pivot table shows, by source field index.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    /// Row fields, outermost first.
    pub rows: Vec<usize>,
    /// Column fields: none or one.
    pub cols: Vec<usize>,
    /// Value fields.
    pub values: Vec<(usize, Aggregate)>,
}

/// A value an item stands for: text without regard to case, as Excel
/// groups it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Number(u64),
    Text(String),
    Bool(bool),
    Error(String),
    Blank,
}

fn key(v: &Value) -> Key {
    match v {
        Value::Number(n) => Key::Number((n + 0.0).to_bits()),
        Value::Text(t) => Key::Text(t.to_lowercase()),
        Value::Bool(b) => Key::Bool(*b),
        Value::Error(e) => Key::Error(e.clone()),
        _ => Key::Blank,
    }
}

/// A field's distinct values: in the order first met (the cache's shared
/// items), and their sorted order (the table's items).
#[derive(Debug, Clone, Default)]
pub struct Items {
    /// Each value and its label.
    pub shared: Vec<(Value, String)>,
    /// Shared indexes, ascending: numbers, text, logical values, errors,
    /// then the blank.
    pub order: Vec<usize>,
    /// Each shared index's place in `order`.
    pub place: Vec<usize>,
    /// Each record's shared index.
    pub of_record: Vec<usize>,
}

fn rank(v: &Value) -> u8 {
    match v {
        Value::Number(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        Value::Error(_) => 3,
        _ => 4,
    }
}

fn items(src: &Source, field: usize) -> Items {
    let mut map: HashMap<Key, usize> = HashMap::new();
    let mut out = Items::default();
    for (i, row) in src.rows.iter().enumerate() {
        let v = row.get(field).cloned().unwrap_or(Value::Empty);
        let k = key(&v);
        let idx = *map.entry(k).or_insert_with(|| {
            let label = src.shown[i].get(field).cloned().unwrap_or_default();
            out.shared.push((v.clone(), label));
            out.shared.len() - 1
        });
        out.of_record.push(idx);
    }
    let mut order: Vec<usize> = (0..out.shared.len()).collect();
    order.sort_by(|&a, &b| {
        let (va, vb) = (&out.shared[a].0, &out.shared[b].0);
        rank(va).cmp(&rank(vb)).then_with(|| match (va, vb) {
            (Value::Number(x), Value::Number(y)) => {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Value::Text(x), Value::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        })
    });
    out.place = vec![0; order.len()];
    for (p, &i) in order.iter().enumerate() {
        out.place[i] = p;
    }
    out.order = order;
    out
}

/// A cell of the table.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// A label.
    Text(String),
    /// A summary.
    Number(f64),
    /// A summary that cannot be computed (`#DIV/0!`).
    Error(String),
}

#[derive(Debug, Clone, Copy, Default)]
struct Acc {
    sum: f64,
    count: usize,
    numbers: usize,
    max: f64,
    min: f64,
}

impl Acc {
    fn add(&mut self, v: &Value) {
        if matches!(v, Value::Empty) {
            return;
        }
        self.count += 1;
        if let Value::Number(n) = v {
            if self.numbers == 0 {
                (self.max, self.min) = (*n, *n);
            } else {
                self.max = self.max.max(*n);
                self.min = self.min.min(*n);
            }
            self.numbers += 1;
            self.sum += n;
        }
    }

    fn get(&self, agg: Aggregate) -> Out {
        match agg {
            Aggregate::Sum => Out::Number(self.sum),
            Aggregate::Count => Out::Number(self.count as f64),
            Aggregate::Average if self.numbers == 0 => Out::Error("#DIV/0!".into()),
            Aggregate::Average => Out::Number(self.sum / self.numbers as f64),
            Aggregate::Max => Out::Number(if self.numbers == 0 { 0.0 } else { self.max }),
            Aggregate::Min => Out::Number(if self.numbers == 0 { 0.0 } else { self.min }),
        }
    }
}

/// A value field's caption, as Excel names it.
pub fn caption(name: &str, agg: Aggregate) -> String {
    let what = match agg {
        Aggregate::Sum => "Sum",
        Aggregate::Count => "Count",
        Aggregate::Average => "Average",
        Aggregate::Max => "Max",
        Aggregate::Min => "Min",
    };
    format!("{what} of {name}")
}

/// A computed pivot table.
#[derive(Debug, Clone)]
pub struct Pivot {
    /// What it shows.
    pub layout: Layout,
    /// The items of each row and column field.
    pub items: HashMap<usize, Items>,
    /// The rows: depth and the item's place in its field's order; the
    /// grand total last, as `None`.
    pub row_items: Vec<Option<(usize, usize)>>,
    /// The columns' item places (with a column field), the grand total
    /// last as `None`.
    pub col_items: Vec<Option<usize>>,
    /// The cells, from the table's first cell.
    pub cells: Vec<Vec<Option<Out>>>,
    /// The rows above the first row of items.
    pub header_rows: usize,
}

/// Computes a pivot table; `Err` with what is wrong with the layout.
pub fn compute(src: &Source, layout: &Layout) -> Result<Pivot, String> {
    if layout.rows.is_empty() {
        return Err("A pivot table needs a row field".into());
    }
    if layout.values.is_empty() {
        return Err("A pivot table needs a value field".into());
    }
    if layout.cols.len() > 1 || (!layout.cols.is_empty() && layout.values.len() > 1) {
        return Err("Kalem's pivot tables take one column field, with one value field".into());
    }
    let mut axis: Vec<usize> = layout.rows.clone();
    axis.extend(&layout.cols);
    let mut seen = std::collections::HashSet::new();
    if !axis.iter().all(|f| seen.insert(*f)) {
        return Err("A field is used twice".into());
    }
    if axis
        .iter()
        .chain(layout.values.iter().map(|v| &v.0))
        .any(|&f| f >= src.names.len())
    {
        return Err("No such field".into());
    }
    let items: HashMap<usize, Items> = axis.iter().map(|&f| (f, items(src, f))).collect();
    let place = |f: usize, rec: usize| items[&f].place[items[&f].of_record[rec]];
    // Every record's row path and column, as places.
    let paths: Vec<Vec<usize>> = (0..src.rows.len())
        .map(|i| layout.rows.iter().map(|&f| place(f, i)).collect())
        .collect();
    let col_of = |i: usize| layout.cols.first().map(|&f| place(f, i));
    let mut full: Vec<Vec<usize>> = paths.clone();
    full.sort();
    full.dedup();
    // The rows, depth first: a new prefix makes a row.
    let mut row_items = Vec::new();
    let mut prefixes: Vec<Vec<usize>> = Vec::new();
    let mut prev: Option<&Vec<usize>> = None;
    for t in &full {
        for d in 0..t.len() {
            if prev.is_none_or(|p| p[..=d] != t[..=d]) {
                row_items.push(Some((d, t[d])));
                prefixes.push(t[..=d].to_vec());
            }
        }
        prev = Some(t);
    }
    row_items.push(None);
    prefixes.push(Vec::new());
    let mut col_items: Vec<Option<usize>> = Vec::new();
    if let Some(&f) = layout.cols.first() {
        let mut present: Vec<usize> = (0..src.rows.len()).map(|i| place(f, i)).collect();
        present.sort_unstable();
        present.dedup();
        col_items.extend(present.into_iter().map(Some));
        col_items.push(None);
    }
    // Sums by row prefix and column (None: all columns).
    let mut acc: HashMap<(Vec<usize>, Option<usize>), Vec<Acc>> = HashMap::new();
    let n = layout.values.len();
    for (i, path) in paths.iter().enumerate() {
        let cols: Vec<Option<usize>> = match col_of(i) {
            Some(c) => vec![Some(c), None],
            None => vec![None],
        };
        for d in 0..=path.len() {
            for c in &cols {
                let a = acc
                    .entry((path[..d].to_vec(), *c))
                    .or_insert_with(|| vec![Acc::default(); n]);
                for (k, (f, _)) in layout.values.iter().enumerate() {
                    a[k].add(src.rows[i].get(*f).unwrap_or(&Value::Empty));
                }
            }
        }
    }
    let label = |f: usize, p: usize| -> Out {
        let it = &items[&f];
        let (v, shown) = &it.shared[it.order[p]];
        match v {
            Value::Empty => Out::Text("(blank)".into()),
            Value::Number(x) if shown.parse::<f64>().is_ok() => Out::Number(*x),
            _ => Out::Text(shown.clone()),
        }
    };
    let value = |prefix: &Vec<usize>, c: Option<usize>, k: usize| -> Option<Out> {
        acc.get(&(prefix.clone(), c))
            .map(|a| a[k].get(layout.values[k].1))
    };
    let mut cells: Vec<Vec<Option<Out>>> = Vec::new();
    let header_rows;
    let name = |f: usize| src.names[f].clone();
    if let Some(&cf) = layout.cols.first() {
        header_rows = 2;
        let (vf, agg) = layout.values[0];
        cells.push(vec![
            Some(Out::Text(caption(&name(vf), agg))),
            Some(Out::Text("Column Labels".into())),
        ]);
        let mut head = vec![Some(Out::Text("Row Labels".into()))];
        for c in &col_items {
            head.push(Some(match c {
                Some(p) => label(cf, *p),
                None => Out::Text("Grand Total".into()),
            }));
        }
        cells.push(head);
    } else {
        header_rows = 1;
        let mut head = vec![Some(Out::Text("Row Labels".into()))];
        for (f, agg) in &layout.values {
            head.push(Some(Out::Text(caption(&name(*f), *agg))));
        }
        cells.push(head);
    }
    for (item, prefix) in row_items.iter().zip(&prefixes) {
        let mut line = vec![Some(match item {
            Some((d, p)) => label(layout.rows[*d], *p),
            None => Out::Text("Grand Total".into()),
        })];
        if layout.cols.is_empty() {
            for k in 0..n {
                line.push(value(prefix, None, k));
            }
        } else {
            for c in &col_items {
                line.push(value(prefix, *c, 0));
            }
        }
        cells.push(line);
    }
    Ok(Pivot {
        layout: layout.clone(),
        items,
        row_items,
        col_items,
        cells,
        header_rows,
    })
}

impl Pivot {
    /// The cells' range from its first cell.
    pub fn range(&self, at: CellRef) -> Range {
        let width = self.cells.iter().map(Vec::len).max().unwrap_or(1) as u32;
        Range {
            start: at,
            end: CellRef::new(at.row + self.cells.len() as u32 - 1, at.col + width - 1),
        }
    }
}

fn num(n: f64) -> String {
    format!("{n}")
}

fn item_xml(v: &Value) -> String {
    match v {
        Value::Number(n) => format!("<n v=\"{}\"/>", num(*n)),
        Value::Text(t) => format!("<s v=\"{}\"/>", xml::escape(t)),
        Value::Bool(b) => format!("<b v=\"{}\"/>", u8::from(*b)),
        Value::Error(e) => format!("<e v=\"{}\"/>", xml::escape(e)),
        _ => "<m/>".into(),
    }
}

/// The `<sharedItems>` of a field: what kinds of value it holds, and its
/// items when it is a row or column field.
fn shared_items(src: &Source, field: usize, items: Option<&Items>) -> String {
    let (mut text, mut number, mut int, mut blank, mut boolean, mut error, mut long) =
        (false, false, true, false, false, false, false);
    let (mut min, mut max) = (f64::MAX, f64::MIN);
    for row in &src.rows {
        match row.get(field).unwrap_or(&Value::Empty) {
            Value::Number(n) => {
                number = true;
                int &= n.fract() == 0.0;
                min = min.min(*n);
                max = max.max(*n);
            }
            Value::Text(t) => {
                text = true;
                long |= t.chars().count() > 255;
            }
            Value::Bool(_) => boolean = true,
            Value::Error(_) => error = true,
            _ => blank = true,
        }
    }
    let mut a = String::new();
    if !text {
        a.push_str(" containsSemiMixedTypes=\"0\" containsString=\"0\"");
    }
    if [text, number, boolean, error]
        .iter()
        .filter(|x| **x)
        .count()
        > 1
    {
        a.push_str(" containsMixedTypes=\"1\"");
    }
    if number {
        a.push_str(" containsNumber=\"1\"");
        if int {
            a.push_str(" containsInteger=\"1\"");
        }
        a.push_str(&format!(
            " minValue=\"{}\" maxValue=\"{}\"",
            num(min),
            num(max)
        ));
    }
    if blank {
        a.push_str(" containsBlank=\"1\"");
    }
    if long {
        a.push_str(" longText=\"1\"");
    }
    match items {
        Some(it) => {
            let body: String = it.shared.iter().map(|(v, _)| item_xml(v)).collect();
            format!(
                "<sharedItems{a} count=\"{}\">{body}</sharedItems>",
                it.shared.len()
            )
        }
        None => format!("<sharedItems{a}/>"),
    }
}

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// The cache definition: the source, and its fields.
pub fn cache_definition(
    p: &Pivot,
    src: &Source,
    sheet: &str,
    range: Range,
    records_rid: &str,
) -> String {
    let fields: String = src
        .names
        .iter()
        .enumerate()
        .map(|(f, name)| {
            format!(
                "<cacheField name=\"{}\" numFmtId=\"0\">{}</cacheField>",
                xml::escape(name),
                shared_items(src, f, p.items.get(&f))
            )
        })
        .collect();
    format!(
        "{DECL}<pivotCacheDefinition xmlns=\"{MAIN}\" xmlns:r=\"{RELS}\" r:id=\"{records_rid}\" refreshOnLoad=\"1\" refreshedBy=\"Kalem\" createdVersion=\"8\" refreshedVersion=\"8\" minRefreshableVersion=\"3\" recordCount=\"{}\"><cacheSource type=\"worksheet\"><worksheetSource ref=\"{range}\" sheet=\"{}\"/></cacheSource><cacheFields count=\"{}\">{fields}</cacheFields></pivotCacheDefinition>",
        src.rows.len(),
        xml::escape(sheet),
        src.names.len()
    )
}

/// The cache's records: row and column fields by item, the rest as values.
pub fn cache_records(p: &Pivot, src: &Source) -> String {
    let mut body = String::new();
    for (i, row) in src.rows.iter().enumerate() {
        body.push_str("<r>");
        for f in 0..src.names.len() {
            match p.items.get(&f) {
                Some(it) => body.push_str(&format!("<x v=\"{}\"/>", it.of_record[i])),
                None => body.push_str(&item_xml(row.get(f).unwrap_or(&Value::Empty))),
            }
        }
        body.push_str("</r>");
    }
    format!(
        "{DECL}<pivotCacheRecords xmlns=\"{MAIN}\" xmlns:r=\"{RELS}\" count=\"{}\">{body}</pivotCacheRecords>",
        src.rows.len()
    )
}

/// The table's definition.
pub fn table_definition(
    p: &Pivot,
    src: &Source,
    name: &str,
    cache_id: u32,
    at: CellRef,
    style: &str,
) -> String {
    let l = &p.layout;
    let fields: String = (0..src.names.len())
        .map(|f| {
            let axis = if l.rows.contains(&f) {
                Some("axisRow")
            } else if l.cols.contains(&f) {
                Some("axisCol")
            } else {
                None
            };
            let data = if l.values.iter().any(|v| v.0 == f) {
                " dataField=\"1\""
            } else {
                ""
            };
            match axis {
                Some(a) => {
                    let it = &p.items[&f];
                    let list: String =
                        it.order.iter().map(|x| format!("<item x=\"{x}\"/>")).collect();
                    format!(
                        "<pivotField axis=\"{a}\"{data} showAll=\"0\"><items count=\"{}\">{list}<item t=\"default\"/></items></pivotField>",
                        it.order.len() + 1
                    )
                }
                None => format!("<pivotField{data} showAll=\"0\"/>"),
            }
        })
        .collect();
    let x = |v: usize| {
        if v == 0 {
            "<x/>".to_owned()
        } else {
            format!("<x v=\"{v}\"/>")
        }
    };
    let row_fields: String = l
        .rows
        .iter()
        .map(|f| format!("<field x=\"{f}\"/>"))
        .collect();
    let row_items: String = p
        .row_items
        .iter()
        .map(|i| match i {
            Some((0, v)) => format!("<i>{}</i>", x(*v)),
            Some((d, v)) => format!("<i r=\"{d}\">{}</i>", x(*v)),
            None => "<i t=\"grand\"><x/></i>".into(),
        })
        .collect();
    let (col_fields, col_items) = if let Some(&cf) = l.cols.first() {
        let items: String = p
            .col_items
            .iter()
            .map(|c| match c {
                Some(v) => format!("<i>{}</i>", x(*v)),
                None => "<i t=\"grand\"><x/></i>".into(),
            })
            .collect();
        (
            format!("<colFields count=\"1\"><field x=\"{cf}\"/></colFields>"),
            format!(
                "<colItems count=\"{}\">{items}</colItems>",
                p.col_items.len()
            ),
        )
    } else if l.values.len() > 1 {
        let items: String = (0..l.values.len())
            .map(|k| {
                if k == 0 {
                    "<i><x/></i>".to_owned()
                } else {
                    format!("<i i=\"{k}\"><x v=\"{k}\"/></i>")
                }
            })
            .collect();
        (
            "<colFields count=\"1\"><field x=\"-2\"/></colFields>".to_owned(),
            format!("<colItems count=\"{}\">{items}</colItems>", l.values.len()),
        )
    } else {
        (
            String::new(),
            "<colItems count=\"1\"><i/></colItems>".to_owned(),
        )
    };
    let data_fields: String = l
        .values
        .iter()
        .map(|(f, agg)| {
            let sub = match agg {
                Aggregate::Sum => "",
                Aggregate::Count => " subtotal=\"count\"",
                Aggregate::Average => " subtotal=\"average\"",
                Aggregate::Max => " subtotal=\"max\"",
                Aggregate::Min => " subtotal=\"min\"",
            };
            format!(
                "<dataField name=\"{}\" fld=\"{f}\"{sub} baseField=\"0\" baseItem=\"0\"/>",
                xml::escape(&caption(&src.names[*f], *agg))
            )
        })
        .collect();
    let first_header = if l.cols.is_empty() && l.values.len() > 1 {
        0
    } else {
        1
    };
    format!(
        "{DECL}<pivotTableDefinition xmlns=\"{MAIN}\" name=\"{}\" cacheId=\"{cache_id}\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" applyWidthHeightFormats=\"1\" dataCaption=\"Values\" updatedVersion=\"8\" minRefreshableVersion=\"3\" useAutoFormatting=\"1\" itemPrintTitles=\"1\" createdVersion=\"8\" indent=\"0\" outline=\"1\" outlineData=\"1\" multipleFieldFilters=\"0\"><location ref=\"{}\" firstHeaderRow=\"{first_header}\" firstDataRow=\"{}\" firstDataCol=\"1\"/><pivotFields count=\"{}\">{fields}</pivotFields><rowFields count=\"{}\">{row_fields}</rowFields><rowItems count=\"{}\">{row_items}</rowItems>{col_fields}{col_items}<dataFields count=\"{}\">{data_fields}</dataFields><pivotTableStyleInfo name=\"{}\" showRowHeaders=\"1\" showColHeaders=\"1\" showRowStripes=\"0\" showColStripes=\"0\" showLastColumn=\"1\"/></pivotTableDefinition>",
        xml::escape(name),
        p.range(at),
        p.header_rows,
        src.names.len(),
        l.rows.len(),
        p.row_items.len(),
        l.values.len(),
        xml::escape(style)
    )
}

/// What Kalem reads of an existing table: its name, cache, first cell,
/// style and layout by cache field; `Err` naming what it cannot refresh.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableDef {
    /// The table's name.
    pub name: String,
    /// The cache it reads.
    pub cache_id: u32,
    /// Where it stands.
    pub location: Option<Range>,
    /// Its style's name.
    pub style: String,
    /// Row fields, by cache field.
    pub rows: Vec<usize>,
    /// Column fields, by cache field (the values' `-2` left out).
    pub cols: Vec<usize>,
    /// Value fields, by cache field.
    pub values: Vec<(usize, Aggregate)>,
    /// Page (filter) fields: not refreshed.
    pub pages: usize,
}

/// Reads a table definition.
pub fn parse_table(text: &str) -> TableDef {
    let mut t = TableDef::default();
    let mut r = xml::Reader::new(text);
    let mut list = "";
    while let Some(tok) = r.next_token() {
        let xml::Token::Start(tag) = tok else {
            continue;
        };
        match tag.name {
            "pivotTableDefinition" => {
                t.name = tag.attr("name").unwrap_or_default().into_owned();
                t.cache_id = tag
                    .attr("cacheId")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
            "location" => t.location = tag.attr("ref").and_then(|v| Range::parse(&v)),
            "pivotTableStyleInfo" => t.style = tag.attr("name").unwrap_or_default().into_owned(),
            "rowFields" | "colFields" => {
                list = if tag.name == "rowFields" {
                    "rows"
                } else {
                    "cols"
                }
            }
            "field" => {
                if let Some(x) = tag
                    .attr("x")
                    .and_then(|v| v.parse::<i64>().ok())
                    .filter(|x| *x >= 0)
                {
                    if list == "rows" {
                        t.rows.push(x as usize);
                    } else {
                        t.cols.push(x as usize);
                    }
                }
            }
            "pageField" => t.pages += 1,
            "dataField" => {
                let agg = match tag.attr("subtotal").as_deref() {
                    Some("count" | "countNums") => Aggregate::Count,
                    Some("average") => Aggregate::Average,
                    Some("max") => Aggregate::Max,
                    Some("min") => Aggregate::Min,
                    _ => Aggregate::Sum,
                };
                if let Some(f) = tag.attr("fld").and_then(|v| v.parse().ok()) {
                    t.values.push((f, agg));
                }
            }
            _ => {}
        }
    }
    t
}

/// What Kalem reads of a cache definition: its source and field names;
/// `None` for a source it does not refresh (a named range, another file,
/// grouped or calculated fields).
pub fn parse_cache(text: &str) -> Option<(String, Range, Vec<String>, Option<String>)> {
    let mut r = xml::Reader::new(text);
    let (mut sheet, mut range, mut names, mut rid) = (None, None, Vec::new(), None);
    while let Some(tok) = r.next_token() {
        let xml::Token::Start(tag) = tok else {
            continue;
        };
        match tag.name {
            "pivotCacheDefinition" => rid = tag.attr("id").map(|v| v.into_owned()),
            "worksheetSource" => {
                sheet = tag.attr("sheet").map(|v| v.into_owned());
                range = tag
                    .attr("ref")
                    .and_then(|v| Range::parse(&v.replace('$', "")));
            }
            "cacheField" => {
                if tag.attr("formula").is_some() {
                    return None;
                }
                names.push(tag.attr("name").unwrap_or_default().into_owned());
            }
            "fieldGroup" | "calculatedItems" | "consolidation" => return None,
            _ => {}
        }
    }
    Some((sheet?, range?, names, rid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Source {
        let t = |s: &str| Value::Text(s.into());
        let n = Value::Number;
        let rows = vec![
            vec![t("West"), t("Pens"), n(10.0)],
            vec![t("East"), t("Pens"), n(5.0)],
            vec![t("west"), t("Ink"), n(7.0)],
            vec![t("East"), t("Ink"), Value::Empty],
        ];
        let shown = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|v| match v {
                        Value::Text(s) => s.clone(),
                        Value::Number(x) => format!("{x}"),
                        _ => String::new(),
                    })
                    .collect()
            })
            .collect();
        Source {
            names: vec!["Region".into(), "Item".into(), "Units".into()],
            rows,
            shown,
        }
    }

    #[test]
    fn rows_and_a_sum() {
        let src = source();
        let p = compute(
            &src,
            &Layout {
                rows: vec![0],
                values: vec![(2, Aggregate::Sum)],
                ..Layout::default()
            },
        )
        .unwrap();
        let text = |o: &Option<Out>| match o {
            Some(Out::Text(t)) => t.clone(),
            Some(Out::Number(n)) => format!("{n}"),
            Some(Out::Error(e)) => e.clone(),
            None => String::new(),
        };
        let grid: Vec<Vec<String>> = p
            .cells
            .iter()
            .map(|r| r.iter().map(text).collect())
            .collect();
        assert_eq!(
            grid,
            [
                ["Row Labels", "Sum of Units"],
                ["East", "5"],
                ["West", "17"],
                ["Grand Total", "22"]
            ]
        );
        assert_eq!(p.range(CellRef::new(2, 0)).to_string(), "A3:B6");
        let def = table_definition(
            &p,
            &src,
            "PivotTable1",
            1,
            CellRef::new(2, 0),
            "PivotStyleLight16",
        );
        assert!(def.contains("<rowItems count=\"3\"><i><x/></i><i><x v=\"1\"/></i><i t=\"grand\"><x/></i></rowItems>"), "{def}");
        // Shared items in the order met; the table's items sorted.
        assert!(
            def.contains(
                "<items count=\"3\"><item x=\"1\"/><item x=\"0\"/><item t=\"default\"/></items>"
            ),
            "{def}"
        );
        let t = parse_table(&def);
        assert_eq!(
            (t.rows.clone(), t.values.clone()),
            (vec![0], vec![(2, Aggregate::Sum)])
        );
        let cache = cache_definition(&p, &src, "Data", Range::parse("A1:C5").unwrap(), "rId1");
        assert!(
            cache.contains("<sharedItems count=\"2\"><s v=\"West\"/><s v=\"East\"/></sharedItems>"),
            "{cache}"
        );
        assert!(cache.contains("containsSemiMixedTypes=\"0\" containsString=\"0\" containsNumber=\"1\" containsInteger=\"1\" minValue=\"5\" maxValue=\"10\" containsBlank=\"1\""), "{cache}");
        let (sheet, range, names, _) = parse_cache(&cache).unwrap();
        assert_eq!(
            (sheet.as_str(), range.to_string(), names.len()),
            ("Data", "A1:C5".into(), 3)
        );
        assert!(
            cache_records(&p, &src).contains("<r><x v=\"0\"/><s v=\"Pens\"/><n v=\"10\"/></r>")
        );
    }

    #[test]
    fn nested_rows_and_columns() {
        let src = source();
        let p = compute(
            &src,
            &Layout {
                rows: vec![0, 1],
                values: vec![(2, Aggregate::Count)],
                ..Layout::default()
            },
        )
        .unwrap();
        // East, its Ink and Pens, West, its Ink and Pens, the total.
        let depths: Vec<Option<usize>> = p.row_items.iter().map(|i| i.map(|x| x.0)).collect();
        assert_eq!(
            depths,
            [Some(0), Some(1), Some(1), Some(0), Some(1), Some(1), None]
        );
        assert_eq!(
            p.cells[1][1],
            Some(Out::Number(1.0)),
            "East counts one value"
        );
        let p = compute(
            &src,
            &Layout {
                rows: vec![0],
                cols: vec![1],
                values: vec![(2, Aggregate::Average)],
            },
        )
        .unwrap();
        assert_eq!(p.cells[0][1], Some(Out::Text("Column Labels".into())));
        assert_eq!(
            p.cells[1][1..],
            [
                Some(Out::Text("Ink".into())),
                Some(Out::Text("Pens".into())),
                Some(Out::Text("Grand Total".into()))
            ]
        );
        // East has Ink with no number: no average.
        assert_eq!(p.cells[2][1], Some(Out::Error("#DIV/0!".into())));
        assert_eq!(p.cells[3][1], Some(Out::Number(7.0)));
        assert!(
            compute(
                &src,
                &Layout {
                    rows: vec![0],
                    cols: vec![1],
                    values: vec![(2, Aggregate::Sum), (2, Aggregate::Max)]
                }
            )
            .is_err()
        );
    }
}
