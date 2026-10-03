//! Rows and columns inserted and deleted: every part that names a place on
//! the changed sheet rewritten as Excel rewrites it, every other byte kept.
//!
//! The sheet's own part moves its rows, cells, merged cells, hyperlinks,
//! conditional formats, validations, the filter, the view and the column
//! widths, and adjusts its formulas; the other sheets', the defined names',
//! the charts' formulas follow; comments, their note shapes, drawings'
//! anchors, tables and pivot sources move with the cells.

use std::collections::HashSet;

use crate::cellref::{CellRef, Range};
use crate::formula::{self, Op};
use crate::sheet::{FormulaKind, Sheet};
use crate::xml::{self, Reader, Tag, Token};

/// Writes a part again: untouched bytes copied, changes spliced in order.
struct Out<'a> {
    src: &'a str,
    out: String,
    at: usize,
}

impl<'a> Out<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            out: String::with_capacity(src.len() + 64),
            at: 0,
        }
    }

    /// Replaces `start..end` with `with`.
    fn replace(&mut self, start: usize, end: usize, with: &str) {
        self.out.push_str(&self.src[self.at..start]);
        self.out.push_str(with);
        self.at = end;
    }

    fn finish(mut self) -> String {
        self.out.push_str(&self.src[self.at..]);
        self.out
    }
}

/// The span of an element's content and its end, for the start tag just read.
fn content(src: &str, r: &mut Reader<'_>, tag: &Tag<'_>) -> (usize, usize, usize) {
    if tag.empty {
        return (tag.span.end, tag.span.end, tag.span.end);
    }
    let end = r.skip_element();
    let close = src[..end].rfind('<').unwrap_or(end);
    (tag.span.end, close, end)
}

fn adjust_attr(
    out: &mut Out<'_>,
    tag: &Tag<'_>,
    src: &str,
    name: &str,
    f: impl Fn(&str) -> Option<String>,
) -> bool {
    let Some(v) = tag.attr(name) else { return true };
    match f(&v) {
        Some(nv) if nv != *v => {
            out.replace(
                tag.span.start,
                tag.span.end,
                &xml::set_attr(&src[tag.span.clone()], name, &nv),
            );
            true
        }
        Some(_) => true,
        None => false,
    }
}

/// The formula text of an element (`<f>`, `<formula>`, `<c:f>`) adjusted.
fn adjust_text(
    out: &mut Out<'_>,
    src: &str,
    start: usize,
    close: usize,
    f: impl Fn(&str) -> String,
) {
    let raw = &src[start..close];
    let text = xml::unescape(raw);
    let new = f(&text);
    if new != *text {
        out.replace(start, close, &xml::escape(&new));
    }
}

fn range_attr(op: Op) -> impl Fn(&str) -> Option<String> {
    move |v: &str| {
        Range::parse(v)
            .and_then(|r| op.range(r))
            .map(|r| r.to_string())
    }
}

fn sqref_attr(op: Op) -> impl Fn(&str) -> Option<String> {
    move |v: &str| {
        let s = formula::adjust_sqref(v, op);
        (!s.is_empty()).then_some(s)
    }
}

/// The changed sheet's own part. `model` is the part as read, for the
/// shared formula groups whose first cell is deleted: their other cells
/// get their own text.
pub fn rewrite_sheet(src: &str, op: Op, sheet: &str, model: &Sheet) -> String {
    let rows_op = matches!(op, Op::InsertRows { .. } | Op::DeleteRows { .. });
    let orphaned: HashSet<u32> = model
        .cells
        .iter()
        .filter_map(|(p, c)| match &c.formula {
            Some(f) => match f.kind {
                FormulaKind::Shared { si, master: true } if op.cell(*p).is_none() => Some(si),
                _ => None,
            },
            None => None,
        })
        .collect();
    let adjust_f = |t: &str| formula::adjust(t, op, sheet, Some(sheet));
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    let mut cell: Option<CellRef> = None;
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        match tag.name {
            "dimension" => {
                let ok = adjust_attr(&mut out, &tag, src, "ref", range_attr(op));
                if !ok {
                    out.replace(
                        tag.span.start,
                        tag.span.end,
                        &xml::set_attr(&src[tag.span.clone()], "ref", "A1"),
                    );
                }
            }
            "pane" => {
                let ok = adjust_attr(&mut out, &tag, src, "topLeftCell", |v| {
                    CellRef::parse(v)
                        .and_then(|c| op.cell(c))
                        .map(|c| c.to_string())
                });
                if !ok {
                    out.replace(
                        tag.span.start,
                        tag.span.end,
                        &xml::remove_attr(&src[tag.span.clone()], "topLeftCell"),
                    );
                }
            }
            "selection" => {
                let mut t = src[tag.span.clone()].to_owned();
                if let Some(a) = tag.attr("activeCell") {
                    let n = CellRef::parse(&a)
                        .and_then(|c| op.cell(c))
                        .map_or("A1".into(), |c| c.to_string());
                    t = xml::set_attr(&t, "activeCell", &n);
                }
                if let Some(s) = tag.attr("sqref") {
                    let n = formula::adjust_sqref(&s, op);
                    t = xml::set_attr(&t, "sqref", if n.is_empty() { "A1" } else { &n });
                }
                if t != src[tag.span.clone()] {
                    out.replace(tag.span.start, tag.span.end, &t);
                }
            }
            "col" if !rows_op => {
                let get = |k| tag.attr(k).and_then(|v| v.parse::<u32>().ok());
                if let (Some(min), Some(max)) = (get("min"), get("max")) {
                    match op.span(min.saturating_sub(1), max.saturating_sub(1)) {
                        Some((a, b)) => {
                            let t =
                                xml::set_attr(&src[tag.span.clone()], "min", &(a + 1).to_string());
                            let t = xml::set_attr(&t, "max", &(b + 1).to_string());
                            if t != src[tag.span.clone()] {
                                out.replace(tag.span.start, tag.span.end, &t);
                            }
                        }
                        None => {
                            let (_, _, end) = content(src, &mut r, &tag);
                            out.replace(tag.span.start, end, "");
                        }
                    }
                }
            }
            "row" => {
                let Some(row) = tag.attr("r").and_then(|v| v.parse::<u32>().ok()) else {
                    continue;
                };
                let new = if rows_op {
                    op.coord(row - 1)
                } else {
                    Some(row - 1)
                };
                match new {
                    None => {
                        let (_, _, end) = content(src, &mut r, &tag);
                        out.replace(tag.span.start, end, "");
                    }
                    Some(n) => {
                        let mut t =
                            xml::set_attr(&src[tag.span.clone()], "r", &(n + 1).to_string());
                        if !rows_op {
                            t = xml::remove_attr(&t, "spans");
                        }
                        if t != src[tag.span.clone()] {
                            out.replace(tag.span.start, tag.span.end, &t);
                        }
                    }
                }
            }
            "c" => {
                let Some(pos) = tag.attr("r").and_then(|v| CellRef::parse(&v)) else {
                    continue;
                };
                match op.cell(pos) {
                    None => {
                        let (_, _, end) = content(src, &mut r, &tag);
                        out.replace(tag.span.start, end, "");
                        cell = None;
                    }
                    Some(n) => {
                        if n != pos {
                            out.replace(
                                tag.span.start,
                                tag.span.end,
                                &xml::set_attr(&src[tag.span.clone()], "r", &n.to_string()),
                            );
                        }
                        cell = if tag.empty { None } else { Some(pos) };
                    }
                }
            }
            "f" => {
                let shared_si = (tag.attr("t").as_deref() == Some("shared"))
                    .then(|| tag.attr("si").and_then(|v| v.parse::<u32>().ok()))
                    .flatten();
                let is_master = tag.attr("ref").is_some();
                if let (Some(si), false, Some(pos)) = (shared_si, is_master, cell)
                    && orphaned.contains(&si)
                {
                    // The group's first cell is gone: this cell keeps its formula.
                    let text = model
                        .cells
                        .get(&pos)
                        .and_then(|c| c.formula.as_ref())
                        .map(|f| f.text.clone())
                        .unwrap_or_default();
                    let (_, _, end) = content(src, &mut r, &tag);
                    let p = xml::prefix(tag.qname);
                    out.replace(
                        tag.span.start,
                        end,
                        &format!("<{p}f>{}</{p}f>", xml::escape(&adjust_f(&text))),
                    );
                    continue;
                }
                adjust_attr(&mut out, &tag, src, "ref", range_attr(op));
                let (s, c, _) = content(src, &mut r, &tag);
                adjust_text(&mut out, src, s, c, adjust_f);
            }
            "formula" | "formula1" | "formula2" => {
                let (s, c, _) = content(src, &mut r, &tag);
                adjust_text(&mut out, src, s, c, adjust_f);
            }
            "mergeCell" | "hyperlink" | "autoFilter" | "sortState"
                if !adjust_attr(&mut out, &tag, src, "ref", range_attr(op)) =>
            {
                let (_, _, end) = content(src, &mut r, &tag);
                out.replace(tag.span.start, end, "");
            }
            "conditionalFormatting" | "dataValidation" | "ignoredError" | "protectedRange"
                if !adjust_attr(&mut out, &tag, src, "sqref", sqref_attr(op)) =>
            {
                let (_, _, end) = content(src, &mut r, &tag);
                out.replace(tag.span.start, end, "");
            }
            _ => {}
        }
    }
    out.finish()
}

/// Another sheet's part: only its formulas, whose references to the changed
/// sheet move.
pub fn rewrite_formulas(src: &str, op: Op, op_sheet: &str, sheet: &str) -> String {
    let adjust_f = |t: &str| formula::adjust(t, op, op_sheet, Some(sheet));
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if matches!(tag.name, "f" | "formula" | "formula1" | "formula2") {
            let (s, c, _) = content(src, &mut r, &tag);
            adjust_text(&mut out, src, s, c, adjust_f);
        }
    }
    out.finish()
}

/// Every formula of a sheet part (`<f>`, conditional formats' and
/// validations') passed through `f`.
pub fn map_formula_texts(src: &str, f: impl Fn(&str) -> String) -> String {
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if matches!(
            tag.name,
            "f" | "formula" | "formula1" | "formula2" | "definedName"
        ) {
            let (s, c, _) = content(src, &mut r, &tag);
            adjust_text(&mut out, src, s, c, &f);
        }
    }
    out.finish()
}

/// `workbook.xml`: the defined names' formulas.
pub fn rewrite_defined_names(src: &str, op: Op, op_sheet: &str, sheet_names: &[String]) -> String {
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name == "definedName" {
            // A local name's unqualified references are to its own sheet.
            let local = tag
                .attr("localSheetId")
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|i| sheet_names.get(i).cloned());
            let (s, c, _) = content(src, &mut r, &tag);
            adjust_text(&mut out, src, s, c, |t| {
                formula::adjust(t, op, op_sheet, local.as_deref())
            });
        }
    }
    out.finish()
}

/// A chart part: series and labels' formulas (`<c:f>`), always qualified.
pub fn rewrite_chart(src: &str, op: Op, op_sheet: &str) -> String {
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name == "f" {
            let (s, c, _) = content(src, &mut r, &tag);
            adjust_text(&mut out, src, s, c, |t| {
                formula::adjust(t, op, op_sheet, None)
            });
        }
    }
    out.finish()
}

/// A comments part of the changed sheet: each comment's cell; comments on
/// deleted cells go.
pub fn rewrite_comments(src: &str, op: Op) -> String {
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name == "comment" {
            let ok = adjust_attr(&mut out, &tag, src, "ref", |v| {
                CellRef::parse(v)
                    .and_then(|c| op.cell(c))
                    .map(|c| c.to_string())
            });
            if !ok {
                let (_, _, end) = content(src, &mut r, &tag);
                out.replace(tag.span.start, end, "");
            }
        }
    }
    out.finish()
}

/// The legacy drawing of notes (VML): each note's `x:Row` and `x:Column`
/// (zero-based) and its box's anchor; shapes of deleted notes go.
pub fn rewrite_vml(src: &str, op: Op) -> String {
    let rows_op = matches!(op, Op::InsertRows { .. } | Op::DeleteRows { .. });
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != "shape" || tag.empty {
            continue;
        }
        let start = tag.span.start;
        let end = r.skip_element();
        let shape = &src[start..end];
        let num = |name: &str| -> Option<(usize, usize, u32)> {
            let open = shape
                .find(&format!(":{name}>"))
                .or_else(|| shape.find(&format!("<{name}>")))?;
            let s = shape[open..].find('>')? + open + 1;
            let e = shape[s..].find('<')? + s;
            Some((s, e, shape[s..e].trim().parse().ok()?))
        };
        let (Some(row), Some(col)) = (num("Row"), num("Column")) else {
            continue;
        };
        let moved = op.cell(CellRef::new(row.2, col.2));
        let Some(moved) = moved else {
            out.replace(start, end, "");
            continue;
        };
        let mut new = shape.to_owned();
        let mut edits: Vec<(usize, usize, String)> = vec![
            (row.0, row.1, moved.row.to_string()),
            (col.0, col.1, moved.col.to_string()),
        ];
        // `x:Anchor`: left column, offset, top row, offset, right column, offset, bottom row, offset.
        if let Some(a) = num("Anchor").map(|(s, e, _)| (s, e)).or_else(|| {
            let open = shape.find(":Anchor>")?;
            let s = open + ":Anchor>".len();
            let e = shape[s..].find('<')? + s;
            Some((s, e))
        }) {
            let parts: Vec<String> = shape[a.0..a.1]
                .split(',')
                .map(|p| p.trim().to_owned())
                .collect();
            if parts.len() == 8 {
                let mut p: Vec<u32> = parts.iter().filter_map(|x| x.parse().ok()).collect();
                if p.len() == 8 {
                    let idx = if rows_op { [2, 6] } else { [0, 4] };
                    for i in idx {
                        p[i] = op.coord(p[i]).unwrap_or_else(|| match op {
                            Op::DeleteRows { at, .. } | Op::DeleteCols { at, .. } => at,
                            _ => p[i],
                        });
                    }
                    edits.push((
                        a.0,
                        a.1,
                        p.iter().map(u32::to_string).collect::<Vec<_>>().join(", "),
                    ));
                }
            }
        }
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        for (s, e, v) in edits {
            new.replace_range(s..e, &v);
        }
        if new != shape {
            out.replace(start, end, &new);
        }
    }
    out.finish()
}

/// A drawing part of the changed sheet: the anchors of charts, pictures and
/// shapes (`xdr:from` and `xdr:to`, zero-based); an anchor inside deleted
/// rows or columns moves to the deletion's edge, as Excel moves it.
pub fn rewrite_drawing(src: &str, op: Op) -> String {
    let rows_op = matches!(op, Op::InsertRows { .. } | Op::DeleteRows { .. });
    let want = if rows_op { "row" } else { "col" };
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    let mut in_anchor = false;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if matches!(tag.name, "from" | "to") => in_anchor = true,
            Token::End {
                name: "from" | "to",
                ..
            } => in_anchor = false,
            Token::Start(tag) if in_anchor && tag.name == want && !tag.empty => {
                let (s, c, _) = content(src, &mut r, &tag);
                if let Ok(v) = src[s..c].trim().parse::<u32>() {
                    let n = op.coord(v).unwrap_or(match op {
                        Op::DeleteRows { at, .. } | Op::DeleteCols { at, .. } => at,
                        _ => v,
                    });
                    if n != v {
                        out.replace(s, c, &n.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out.finish()
}

/// A table part on the changed sheet: its range and its filter's.
pub fn rewrite_table(src: &str, op: Op) -> String {
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if matches!(tag.name, "table" | "autoFilter" | "sortState") {
            adjust_attr(&mut out, &tag, src, "ref", range_attr(op));
        }
    }
    out.finish()
}

/// A pivot cache definition: its source range when it is on the changed sheet.
pub fn rewrite_pivot_cache(src: &str, op: Op, op_sheet: &str) -> String {
    let mut out = Out::new(src);
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name == "worksheetSource"
            && tag
                .attr("sheet")
                .is_some_and(|s| s.eq_ignore_ascii_case(op_sheet))
        {
            adjust_attr(&mut out, &tag, src, "ref", range_attr(op));
        }
    }
    out.finish()
}

/// The table ranges of a table part, for refusing what Excel refuses.
pub fn table_range(src: &str) -> Option<Range> {
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "table"
        {
            return tag.attr("ref").and_then(|v| Range::parse(&v));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet;

    const SHEET: &str = r#"<worksheet><dimension ref="A1:C4"/><sheetViews><sheetView><selection activeCell="B3" sqref="B3"/></sheetView></sheetViews><cols><col min="2" max="3" width="9"/></cols><sheetData><row r="1" spans="1:3"><c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B3" si="0">A1*2</f><v>2</v></c></row><row r="2"><c r="A2"><v>2</v></c><c r="B2"><f t="shared" si="0"/><v>4</v></c></row><row r="3"><c r="A3"><v>3</v></c><c r="B3"><f t="shared" si="0"/><v>6</v></c><c r="C3"><f>SUM(A1:A3)</f><v>6</v></c></row><row r="4"><c r="A4"><f>Other!A2+A2</f></c></row></sheetData><mergeCells count="1"><mergeCell ref="A2:B2"/></mergeCells><conditionalFormatting sqref="A1:A4"><cfRule type="expression"><formula>A1&gt;1</formula></cfRule></conditionalFormatting></worksheet>"#;

    #[test]
    fn inserting_a_row() {
        let model = sheet::parse(SHEET, &[], false);
        let out = rewrite_sheet(SHEET, Op::InsertRows { at: 1, n: 1 }, "S", &model);
        let m = sheet::parse(&out, &[], false);
        let rows: Vec<u32> = m.rows.keys().copied().collect();
        assert_eq!(rows, [0, 2, 3, 4]);
        assert_eq!(
            m.cells[&CellRef::parse("C4").unwrap()]
                .formula
                .as_ref()
                .unwrap()
                .text,
            "SUM(A1:A4)"
        );
        assert_eq!(
            m.cells[&CellRef::parse("B4").unwrap()]
                .formula
                .as_ref()
                .unwrap()
                .text,
            "A4*2"
        );
        assert!(out.contains(r#"<dimension ref="A1:C5"/>"#));
        assert!(out.contains(r#"<mergeCell ref="A3:B3"/>"#));
        assert!(out.contains(r#"sqref="A1:A5""#));
        assert!(out.contains(r#"activeCell="B4""#));
        assert!(out.contains(r#"ref="B1:B4""#));
    }

    #[test]
    fn deleting_the_first_row_orphans_the_shared_group() {
        let model = sheet::parse(SHEET, &[], false);
        let out = rewrite_sheet(SHEET, Op::DeleteRows { at: 0, n: 1 }, "S", &model);
        let m = sheet::parse(&out, &[], false);
        let get = |r: &str| {
            m.cells[&CellRef::parse(r).unwrap()]
                .formula
                .clone()
                .unwrap()
        };
        assert_eq!(get("B1").text, "A1*2");
        assert_eq!(get("B1").kind, FormulaKind::Normal);
        assert_eq!(get("B2").text, "A2*2");
        assert_eq!(get("C2").text, "SUM(A1:A2)");
        assert_eq!(get("A3").text, "Other!A2+A1");
        assert!(out.contains(r#"<mergeCell ref="A1:B1"/>"#));
        assert!(out.contains("<formula>#REF!&gt;1</formula>"));
    }

    #[test]
    fn deleting_a_column() {
        let model = sheet::parse(SHEET, &[], false);
        let out = rewrite_sheet(SHEET, Op::DeleteCols { at: 0, n: 1 }, "S", &model);
        let m = sheet::parse(&out, &[], false);
        assert_eq!(
            m.cells[&CellRef::parse("B3").unwrap()]
                .formula
                .as_ref()
                .unwrap()
                .text,
            "SUM(#REF!)"
        );
        assert!(out.contains(r#"<col min="1" max="2" width="9"/>"#));
        assert!(!out.contains("spans"));
        assert!(!out.contains("conditionalFormatting"));
    }

    #[test]
    fn other_parts() {
        assert_eq!(
            rewrite_formulas(
                "<c><f>S!A2+A2</f></c>",
                Op::InsertRows { at: 0, n: 1 },
                "S",
                "T"
            ),
            "<c><f>S!A3+A2</f></c>"
        );
        assert_eq!(
            rewrite_chart(
                "<c:f>'S'!$B$2:$B$4</c:f>",
                Op::DeleteRows { at: 2, n: 1 },
                "S"
            ),
            "<c:f>'S'!$B$2:$B$3</c:f>"
        );
        assert_eq!(
            rewrite_comments(
                r#"<comments><commentList><comment ref="A2"><text/></comment><comment ref="A3"><text/></comment></commentList></comments>"#,
                Op::DeleteRows { at: 1, n: 1 }
            ),
            r#"<comments><commentList><comment ref="A2"><text/></comment></commentList></comments>"#
        );
        assert_eq!(
            rewrite_drawing(
                "<xdr:from><xdr:col>5</xdr:col><xdr:row>3</xdr:row></xdr:from>",
                Op::InsertRows { at: 0, n: 2 }
            ),
            "<xdr:from><xdr:col>5</xdr:col><xdr:row>5</xdr:row></xdr:from>"
        );
        let vml = r#"<xml><v:shape id="a"><x:ClientData ObjectType="Note"><x:Anchor>1, 15, 0, 2, 3, 15, 4, 16</x:Anchor><x:Row>1</x:Row><x:Column>0</x:Column></x:ClientData></v:shape></xml>"#;
        let out = rewrite_vml(vml, Op::InsertRows { at: 0, n: 1 });
        assert!(out.contains("<x:Row>2</x:Row>"), "{out}");
        assert!(
            out.contains("<x:Anchor>1, 15, 1, 2, 3, 15, 5, 16</x:Anchor>"),
            "{out}"
        );
        assert_eq!(
            rewrite_vml(vml, Op::DeleteRows { at: 1, n: 1 }),
            "<xml></xml>"
        );
        assert_eq!(
            rewrite_defined_names(
                r#"<definedNames><definedName name="R">S!$B$2:$B$4</definedName></definedNames>"#,
                Op::InsertRows { at: 1, n: 1 },
                "S",
                &[]
            ),
            r#"<definedNames><definedName name="R">S!$B$3:$B$5</definedName></definedNames>"#
        );
    }
}
