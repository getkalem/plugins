//! A worksheet part (ECMA-376 part 1, 18.3) read with the byte span of every
//! row and cell, so an edit can replace one `<c>` and keep the rest.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range as Span;

use crate::cellref::{CellRef, Range};
use crate::formula;
use crate::xml::{self, Reader, Token};

/// A cell's value as the file stores it.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// No value (a styled empty cell, or a formula without a cached result).
    Empty,
    /// A number; dates are numbers with a date format.
    Number(f64),
    /// Text.
    Text(String),
    /// `TRUE` or `FALSE`.
    Bool(bool),
    /// An error such as `#DIV/0!`.
    Error(String),
}

/// How a formula is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormulaKind {
    /// Its own text.
    Normal,
    /// One cell of a shared formula group; `master` holds the text.
    Shared {
        /// The group index `si`.
        si: u32,
        /// Whether this cell holds the group's text and `ref`.
        master: bool,
    },
    /// An array formula over `range`, written on its top left cell.
    Array {
        /// The cells the array fills.
        range: Range,
    },
    /// A what-if data table (`t="dataTable"`).
    DataTable,
}

/// A formula, its text expanded for shared cells, without the leading `=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formula {
    /// The text as Excel shows it in the formula bar, without `=`.
    pub text: String,
    /// How the file stores it.
    pub kind: FormulaKind,
}

/// One `<c>` of the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The value, the cached result for formula cells.
    pub value: Value,
    /// The formula, if any.
    pub formula: Option<Formula>,
    /// The `cellXfs` index `s`.
    pub style: u32,
    /// The bytes of the `<c>` element in the part.
    pub(crate) span: Span<usize>,
    /// Whether the cell is the anchor of an array formula's other cells:
    /// part of `Array` range but not its top left.
    pub(crate) in_array_of: Option<CellRef>,
}

/// A `<row>` of the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Height in points when set.
    pub height: Option<f64>,
    /// Hidden by the user or a filter.
    pub hidden: bool,
    /// The start tag's bytes.
    pub(crate) start: Span<usize>,
    /// The end tag's bytes; `None` for `<row/>`.
    pub(crate) end: Option<Span<usize>>,
}

/// A `<col>` width and visibility for columns `min..=max` (zero-based).
#[derive(Debug, Clone, PartialEq)]
pub struct Cols {
    /// First column.
    pub min: u32,
    /// Last column.
    pub max: u32,
    /// Width in characters of the default font's digit.
    pub width: Option<f64>,
    /// Hidden.
    pub hidden: bool,
}

/// A worksheet.
#[derive(Debug, Clone, Default)]
pub struct Sheet {
    /// The cells that exist in the part, by position.
    pub cells: BTreeMap<CellRef, Cell>,
    /// The rows that exist in the part.
    pub rows: BTreeMap<u32, Row>,
    /// Column widths and visibility.
    pub cols: Vec<Cols>,
    /// Merged ranges.
    pub merged: Vec<Range>,
    /// Frozen rows and columns, from the first sheet view's pane.
    pub frozen: Option<(u32, u32)>,
    /// Default column width in characters.
    pub default_col_width: Option<f64>,
    /// Default row height in points.
    pub default_row_height: Option<f64>,
    /// Whether the sheet holds drawings (charts, pictures, shapes).
    pub has_drawing: bool,
    /// The `<dimension ref>` start tag's bytes and value.
    pub(crate) dimension: Option<(Span<usize>, String)>,
    /// `<sheetData>`'s start tag and end tag (`None` when `<sheetData/>`).
    pub(crate) sheet_data: Option<(Span<usize>, Option<Span<usize>>)>,
    /// The namespace prefix the part writes its elements with (`x:` or empty).
    pub(crate) prefix: String,
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse().ok()
}

/// Parses a shared string table: one string per `<si>`, rich text runs
/// joined, phonetic runs dropped.
pub fn parse_shared_strings(xml_text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut r = Reader::new(xml_text);
    let mut cur: Option<String> = None;
    let mut skip_depth = 0u32;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => match tag.name {
                "si" if tag.empty => out.push(String::new()),
                "si" => cur = Some(String::new()),
                "rPh" | "phoneticPr" if !tag.empty => skip_depth += 1,
                "t" if !tag.empty && skip_depth == 0 => {
                    let (s, _) = r.text_until_end("t");
                    if let Some(c) = cur.as_mut() {
                        c.push_str(&xml::unescape_st_xstring(&s));
                    }
                }
                _ => {}
            },
            Token::End { name, .. } => match name {
                "si" => out.push(cur.take().unwrap_or_default()),
                "rPh" | "phoneticPr" => skip_depth = skip_depth.saturating_sub(1),
                _ => {}
            },
            Token::Text { .. } => {}
        }
    }
    out
}

/// An ISO 8601 date-time (`t="d"` cells) as a serial in the 1900 system.
fn iso_to_serial(s: &str, date1904: bool) -> Option<f64> {
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    let mut it = date.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    let mut serial = crate::numfmt::date_to_serial(y, m, d, date1904) as f64;
    if !time.is_empty() {
        let time = time.trim_end_matches('Z');
        let mut parts = time.split(':');
        let h: f64 = parts.next()?.parse().ok()?;
        let mi: f64 = parts.next().unwrap_or("0").parse().ok()?;
        let sec: f64 = parts.next().unwrap_or("0").parse().ok()?;
        serial += (h * 3600.0 + mi * 60.0 + sec) / 86_400.0;
    }
    Some(serial)
}

/// Parses a worksheet part.
pub fn parse(text: &str, strings: &[String], date1904: bool) -> Sheet {
    let mut sheet = Sheet::default();
    let mut r = Reader::new(text);
    let mut row_idx: u32 = 0;
    let mut next_row: u32 = 0;
    let mut col_idx: u32 = 0;
    let mut in_cols = false;
    let mut first_view_done = false;
    let mut shared: HashMap<u32, (CellRef, String)> = HashMap::new();
    let mut arrays: Vec<(Range, CellRef)> = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => match tag.name {
                "dimension" => {
                    sheet.dimension = Some((
                        tag.span.clone(),
                        tag.attr("ref").unwrap_or_default().into_owned(),
                    ));
                }
                "sheetView" => {}
                "pane" if !first_view_done => {
                    if tag
                        .attr("state")
                        .as_deref()
                        .is_some_and(|s| s == "frozen" || s == "frozenSplit")
                    {
                        let x = tag.attr("xSplit").and_then(|v| number(&v)).unwrap_or(0.0) as u32;
                        let y = tag.attr("ySplit").and_then(|v| number(&v)).unwrap_or(0.0) as u32;
                        sheet.frozen = Some((y, x));
                    }
                }
                "sheetFormatPr" => {
                    sheet.default_col_width = tag
                        .attr("defaultColWidth")
                        .and_then(|v| number(&v))
                        .or_else(|| {
                            tag.attr("baseColWidth")
                                .and_then(|v| number(&v))
                                .map(|b| b + 0.7109375)
                        });
                    sheet.default_row_height =
                        tag.attr("defaultRowHeight").and_then(|v| number(&v));
                }
                "cols" => in_cols = !tag.empty,
                "col" if in_cols => {
                    let get = |k| tag.attr(k).and_then(|v| v.parse::<u32>().ok());
                    if let (Some(min), Some(max)) = (get("min"), get("max")) {
                        sheet.cols.push(Cols {
                            min: min.saturating_sub(1),
                            max: max.saturating_sub(1),
                            width: tag.attr("width").and_then(|v| number(&v)),
                            hidden: tag
                                .attr("hidden")
                                .as_deref()
                                .is_some_and(|v| v == "1" || v == "true"),
                        });
                    }
                }
                "sheetData" => {
                    sheet.prefix = xml::prefix(tag.qname).to_owned();
                    sheet.sheet_data = Some((tag.span.clone(), None));
                }
                "row" => {
                    row_idx = tag
                        .attr("r")
                        .and_then(|v| v.parse::<u32>().ok())
                        .map_or(next_row, |r| r.saturating_sub(1));
                    next_row = row_idx + 1;
                    col_idx = 0;
                    sheet.rows.insert(
                        row_idx,
                        Row {
                            height: tag.attr("ht").and_then(|v| number(&v)),
                            hidden: tag
                                .attr("hidden")
                                .as_deref()
                                .is_some_and(|v| v == "1" || v == "true"),
                            start: tag.span.clone(),
                            end: None,
                        },
                    );
                }
                "c" => {
                    let pos = tag
                        .attr("r")
                        .and_then(|v| CellRef::parse(&v))
                        .unwrap_or(CellRef::new(row_idx, col_idx));
                    col_idx = pos.col + 1;
                    let style = tag.attr("s").and_then(|v| v.parse().ok()).unwrap_or(0);
                    let ty = tag
                        .attr("t")
                        .map(|t| t.into_owned())
                        .unwrap_or_else(|| "n".into());
                    let start = tag.span.start;
                    let mut raw_v: Option<String> = None;
                    let mut inline: Option<String> = None;
                    // The text, `t`, `si` and `ref` of `<f>`.
                    type RawFormula = (String, Option<String>, Option<String>, Option<String>);
                    let mut f: Option<RawFormula> = None;
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        let mut end = text.len();
                        while let Some(t) = r.next_token() {
                            match t {
                                Token::Start(child) => match child.name {
                                    "v" if !child.empty => raw_v = Some(r.text_until_end("v").0),
                                    "f" => {
                                        let ft = child.attr("t").map(|v| v.into_owned());
                                        let si = child.attr("si").map(|v| v.into_owned());
                                        let rf = child.attr("ref").map(|v| v.into_owned());
                                        let body = if child.empty {
                                            String::new()
                                        } else {
                                            r.text_until_end("f").0
                                        };
                                        f = Some((body, ft, si, rf));
                                    }
                                    "is" if !child.empty => {
                                        let mut s = String::new();
                                        let mut skip = 0;
                                        while let Some(t) = r.next_token() {
                                            match t {
                                                Token::Start(g)
                                                    if matches!(g.name, "rPh" | "phoneticPr")
                                                        && !g.empty =>
                                                {
                                                    skip += 1
                                                }
                                                Token::Start(g)
                                                    if g.name == "t" && !g.empty && skip == 0 =>
                                                {
                                                    s.push_str(&xml::unescape_st_xstring(
                                                        &r.text_until_end("t").0,
                                                    ));
                                                }
                                                Token::End {
                                                    name: "rPh" | "phoneticPr",
                                                    ..
                                                } => skip -= 1,
                                                Token::End { name: "is", .. } => break,
                                                _ => {}
                                            }
                                        }
                                        inline = Some(s);
                                    }
                                    _ if !child.empty => {
                                        r.skip_element();
                                    }
                                    _ => {}
                                },
                                Token::End { name: "c", span } => {
                                    end = span.end;
                                    break;
                                }
                                _ => {}
                            }
                        }
                        end
                    };
                    let value = match ty.as_str() {
                        "s" => raw_v
                            .as_deref()
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .and_then(|i| strings.get(i))
                            .map_or(Value::Empty, |s| Value::Text(s.clone())),
                        "inlineStr" => inline.map_or(Value::Empty, Value::Text),
                        "str" => raw_v.map_or(Value::Empty, |v| {
                            Value::Text(xml::unescape_st_xstring(&v).into_owned())
                        }),
                        "b" => raw_v.map_or(Value::Empty, |v| {
                            Value::Bool(v.trim() == "1" || v.trim() == "true")
                        }),
                        "e" => raw_v.map_or(Value::Empty, |v| Value::Error(v.trim().to_owned())),
                        "d" => raw_v
                            .and_then(|v| iso_to_serial(v.trim(), date1904))
                            .map_or(Value::Empty, Value::Number),
                        _ => raw_v
                            .and_then(|v| number(&v))
                            .map_or(Value::Empty, Value::Number),
                    };
                    let formula = f.map(|(body, ft, si, rf)| {
                        let si_n = si.as_deref().and_then(|s| s.parse::<u32>().ok());
                        match (ft.as_deref(), si_n) {
                            (Some("shared"), Some(si)) => {
                                if rf.is_some() && !body.is_empty() {
                                    shared.insert(si, (pos, body.clone()));
                                    Formula {
                                        text: body,
                                        kind: FormulaKind::Shared { si, master: true },
                                    }
                                } else {
                                    let text = shared
                                        .get(&si)
                                        .map(|(m, t)| {
                                            formula::shift(
                                                t,
                                                i64::from(pos.row) - i64::from(m.row),
                                                i64::from(pos.col) - i64::from(m.col),
                                            )
                                        })
                                        .unwrap_or_default();
                                    Formula {
                                        text,
                                        kind: FormulaKind::Shared { si, master: false },
                                    }
                                }
                            }
                            (Some("array"), _) => {
                                let range = rf.as_deref().and_then(Range::parse).unwrap_or(Range {
                                    start: pos,
                                    end: pos,
                                });
                                arrays.push((range, pos));
                                Formula {
                                    text: body,
                                    kind: FormulaKind::Array { range },
                                }
                            }
                            (Some("dataTable"), _) => Formula {
                                text: body,
                                kind: FormulaKind::DataTable,
                            },
                            _ => Formula {
                                text: body,
                                kind: FormulaKind::Normal,
                            },
                        }
                    });
                    sheet.cells.insert(
                        pos,
                        Cell {
                            value,
                            formula,
                            style,
                            span: start..end,
                            in_array_of: None,
                        },
                    );
                }
                "mergeCell" => {
                    if let Some(rg) = tag.attr("ref").and_then(|v| Range::parse(&v)) {
                        sheet.merged.push(rg);
                    }
                }
                "drawing" | "legacyDrawing" | "picture" | "oleObjects" | "controls" => {
                    sheet.has_drawing = true
                }
                _ => {}
            },
            Token::End { name, span } => match name {
                "row" => {
                    if let Some(row) = sheet.rows.get_mut(&row_idx) {
                        row.end = Some(span);
                    }
                }
                "sheetData" => {
                    if let Some(sd) = sheet.sheet_data.as_mut() {
                        sd.1 = Some(span);
                    }
                }
                "cols" => in_cols = false,
                "sheetView" => first_view_done = true,
                _ => {}
            },
            Token::Text { .. } => {}
        }
    }
    for (range, anchor) in arrays {
        for (pos, c) in sheet.cells.range_mut(range.start..=range.end) {
            if range.contains(*pos) && *pos != anchor {
                c.in_array_of = Some(anchor);
            }
        }
    }
    sheet
}

impl Sheet {
    /// The used range: every cell that exists, `None` for an empty sheet.
    pub fn used_range(&self) -> Option<Range> {
        let mut it = self.cells.keys();
        let first = *it.next()?;
        let (mut r0, mut r1, mut c0, mut c1) = (first.row, first.row, first.col, first.col);
        for k in self.cells.keys() {
            r0 = r0.min(k.row);
            r1 = r1.max(k.row);
            c0 = c0.min(k.col);
            c1 = c1.max(k.col);
        }
        Some(Range {
            start: CellRef::new(r0, c0),
            end: CellRef::new(r1, c1),
        })
    }

    /// The merged range covering a cell, if any.
    pub fn merge_at(&self, c: CellRef) -> Option<Range> {
        self.merged.iter().copied().find(|m| m.contains(c))
    }

    /// The width of a column in characters.
    pub fn col_width(&self, col: u32) -> Option<f64> {
        self.cols
            .iter()
            .find(|c| (c.min..=c.max).contains(&col))
            .and_then(|c| c.width)
    }

    /// Whether a column is hidden.
    pub fn col_hidden(&self, col: u32) -> bool {
        self.cols
            .iter()
            .any(|c| (c.min..=c.max).contains(&col) && c.hidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: &str = r#"<worksheet><dimension ref="A1:C3"/><sheetViews><sheetView><pane ySplit="1" topLeftCell="A2" state="frozen"/></sheetView></sheetViews>
<cols><col min="2" max="3" width="20" customWidth="1"/></cols>
<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>inline</t></is></c><c r="C1" t="b"><v>1</v></c></row>
<row r="2"><c r="A2"><v>2.5</v></c><c r="B2"><f t="shared" ref="B2:B3" si="0">A2*2</f><v>5</v></c><c r="C2" t="e"><v>#DIV/0!</v></c></row>
<row r="3"><c r="A3" s="1"/><c r="B3"><f t="shared" si="0"/><v>0</v></c><c r="C3" t="str"><f>"x"&amp;"y"</f><v>xy</v></c></row></sheetData>
<mergeCells count="1"><mergeCell ref="A3:A4"/></mergeCells></worksheet>"#;

    #[test]
    fn cells_values_and_formulas() {
        let s = parse(SHEET, &["shared".into()], false);
        let get = |r: &str| s.cells.get(&CellRef::parse(r).unwrap()).unwrap();
        assert_eq!(get("A1").value, Value::Text("shared".into()));
        assert_eq!(get("B1").value, Value::Text("inline".into()));
        assert_eq!(get("C1").value, Value::Bool(true));
        assert_eq!(get("A2").value, Value::Number(2.5));
        assert_eq!(get("C2").value, Value::Error("#DIV/0!".into()));
        assert_eq!(get("A3").value, Value::Empty);
        assert_eq!(get("A3").style, 1);
        assert_eq!(get("B3").formula.as_ref().unwrap().text, "A3*2");
        assert_eq!(get("C3").formula.as_ref().unwrap().text, "\"x\"&\"y\"");
        assert_eq!(&SHEET[get("A2").span.clone()], "<c r=\"A2\"><v>2.5</v></c>");
        assert_eq!(s.frozen, Some((1, 0)));
        assert_eq!(s.col_width(1), Some(20.0));
        assert_eq!(s.merged.len(), 1);
        assert_eq!(s.used_range().unwrap().to_string(), "A1:C3");
        assert!(s.rows[&2].end.is_some());
    }

    #[test]
    fn shared_strings_rich_and_phonetic() {
        let sst = r#"<sst><si><t>plain</t></si><si><r><t>ri</t></r><r><rPr><b/></rPr><t>ch</t></r><rPh><t>x</t></rPh></si><si/></sst>"#;
        assert_eq!(parse_shared_strings(sst), ["plain", "rich", ""]);
    }
}
