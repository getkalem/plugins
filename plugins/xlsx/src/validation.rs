//! Data validation (ECMA-376 part 1, 18.3.1.32 and 18.3.1.33): read from a
//! sheet, both the plain `<dataValidation>` and Excel 2010's `x14` form in
//! the sheet's extensions, and written as Excel writes it.

use std::ops::Range as Span;

use crate::cellref::{CellRef, Range};
use crate::xml::{self, Reader, Token};

/// One `<dataValidation>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataValidation {
    /// `whole`, `decimal`, `list`, `date`, `time`, `textLength`, `custom`,
    /// or `none`.
    pub kind: String,
    /// `between`, `greaterThan`, …
    pub operator: String,
    /// Blank cells are accepted.
    pub allow_blank: bool,
    /// A list offers its values (the file's `showDropDown` says the opposite).
    pub dropdown: bool,
    /// The input message is shown.
    pub show_input: bool,
    /// Refused values are refused.
    pub show_error: bool,
    /// `stop`, `warning` or `information`.
    pub error_style: String,
    /// The alert's title.
    pub error_title: String,
    /// The alert's text.
    pub error: String,
    /// The input message's title.
    pub prompt_title: String,
    /// The input message.
    pub prompt: String,
    /// The first value or the list, as formula text.
    pub formula1: Option<String>,
    /// The second value.
    pub formula2: Option<String>,
    /// The ranges it covers.
    pub ranges: Vec<Range>,
    /// The element's bytes.
    pub span: Span<usize>,
    /// Where `sqref` is: in the start tag, or the `x14` form's element.
    pub sqref_span: Option<Span<usize>>,
    /// The `x14` form, in the sheet's extensions.
    pub ext: bool,
}

impl DataValidation {
    /// Whether it covers a cell.
    pub fn covers(&self, at: CellRef) -> bool {
        self.ranges.iter().any(|r| r.contains(at))
    }

    /// The first cell of its ranges, which its formulas are written for.
    pub fn first(&self) -> CellRef {
        self.ranges
            .iter()
            .map(|r| r.start)
            .min_by_key(|c| (c.row, c.col))
            .unwrap_or(CellRef::new(0, 0))
    }
}

fn sqref(s: &str) -> Vec<Range> {
    s.split_whitespace().filter_map(Range::parse).collect()
}

/// The data validations of a sheet part.
pub fn parse(text: &str) -> Vec<DataValidation> {
    let mut out = Vec::new();
    let mut r = Reader::new(text);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != "dataValidation" {
            continue;
        }
        let ext = !xml::prefix(tag.qname).is_empty() && tag.qname.starts_with("x14");
        let a = |k: &str| tag.attr(k).map(|v| v.into_owned()).unwrap_or_default();
        let flag = |k: &str| tag.attr(k).is_some_and(|v| v == "1" || v == "true");
        let mut dv = DataValidation {
            kind: tag.attr("type").map_or("none".into(), |v| v.into_owned()),
            operator: tag
                .attr("operator")
                .map_or("between".into(), |v| v.into_owned()),
            allow_blank: flag("allowBlank"),
            dropdown: !flag("showDropDown"),
            show_input: flag("showInputMessage"),
            show_error: flag("showErrorMessage"),
            error_style: tag
                .attr("errorStyle")
                .map_or("stop".into(), |v| v.into_owned()),
            error_title: a("errorTitle"),
            error: a("error"),
            prompt_title: a("promptTitle"),
            prompt: a("prompt"),
            ranges: sqref(&a("sqref")),
            ext,
            ..DataValidation::default()
        };
        let start = tag.span.start;
        if tag.attr("sqref").is_some() {
            dv.sqref_span = Some(tag.span.clone());
        }
        let end = if tag.empty {
            tag.span.end
        } else {
            let mut end = text.len();
            while let Some(t) = r.next_token() {
                match t {
                    Token::Start(t2) if !t2.empty && t2.name == "formula1" => {
                        dv.formula1 = Some(r.text_until_end("formula1").0);
                    }
                    Token::Start(t2) if !t2.empty && t2.name == "formula2" => {
                        dv.formula2 = Some(r.text_until_end("formula2").0);
                    }
                    Token::Start(t2) if !t2.empty && t2.name == "sqref" => {
                        let from = t2.span.start;
                        let (s, e) = r.text_until_end("sqref");
                        dv.ranges = sqref(&s);
                        dv.sqref_span = Some(from..e);
                    }
                    Token::End {
                        name: "dataValidation",
                        span,
                    } => {
                        end = span.end;
                        break;
                    }
                    _ => {}
                }
            }
            end
        };
        dv.span = start..end;
        out.push(dv);
    }
    out
}

/// `a` without `b`: at most four ranges.
pub fn subtract(a: Range, b: Range) -> Vec<Range> {
    let meets = a.start.row <= b.end.row
        && b.start.row <= a.end.row
        && a.start.col <= b.end.col
        && b.start.col <= a.end.col;
    if !meets {
        return vec![a];
    }
    let rg = |r0, c0, r1, c1| Range {
        start: CellRef::new(r0, c0),
        end: CellRef::new(r1, c1),
    };
    let mut out = Vec::new();
    // Above and below, the full width; left and right, between them.
    if a.start.row < b.start.row {
        out.push(rg(a.start.row, a.start.col, b.start.row - 1, a.end.col));
    }
    if b.end.row < a.end.row {
        out.push(rg(b.end.row + 1, a.start.col, a.end.row, a.end.col));
    }
    let (top, bottom) = (a.start.row.max(b.start.row), a.end.row.min(b.end.row));
    if a.start.col < b.start.col {
        out.push(rg(top, a.start.col, bottom, b.start.col - 1));
    }
    if b.end.col < a.end.col {
        out.push(rg(top, b.end.col + 1, bottom, a.end.col));
    }
    out
}

/// A range as `sqref` writes it: one cell without a colon.
pub fn range_text(r: &Range) -> String {
    if r.start == r.end {
        r.start.to_string()
    } else {
        r.to_string()
    }
}

/// A `<dataValidation>` for the contract's validation, its formulas made
/// already.
pub fn element(
    p: &str,
    v: &kalem_viewer::Validation,
    f1: Option<&str>,
    f2: Option<&str>,
    sqref: &str,
) -> String {
    use kalem_viewer::{CompareOp, ErrorStyle, ValidationKind};
    let kind = match v.kind {
        ValidationKind::Any => "none",
        ValidationKind::Whole => "whole",
        ValidationKind::Decimal => "decimal",
        ValidationKind::List => "list",
        ValidationKind::Date => "date",
        ValidationKind::Time => "time",
        ValidationKind::TextLength => "textLength",
        ValidationKind::Custom => "custom",
    };
    let mut attrs = String::new();
    if kind != "none" {
        attrs.push_str(&format!(" type=\"{kind}\""));
    }
    let compares = !matches!(
        v.kind,
        ValidationKind::Any | ValidationKind::List | ValidationKind::Custom
    );
    if compares && v.op != CompareOp::Between {
        let op = match v.op {
            CompareOp::Greater => "greaterThan",
            CompareOp::Less => "lessThan",
            CompareOp::GreaterOrEqual => "greaterThanOrEqual",
            CompareOp::LessOrEqual => "lessThanOrEqual",
            CompareOp::Equal => "equal",
            CompareOp::NotEqual => "notEqual",
            CompareOp::NotBetween => "notBetween",
            CompareOp::Between => "between",
        };
        attrs.push_str(&format!(" operator=\"{op}\""));
    }
    if let Some((style, _, _)) = &v.error {
        match style {
            ErrorStyle::Warning => attrs.push_str(" errorStyle=\"warning\""),
            ErrorStyle::Information => attrs.push_str(" errorStyle=\"information\""),
            ErrorStyle::Stop => {}
        }
    }
    if v.allow_blank {
        attrs.push_str(" allowBlank=\"1\"");
    }
    if v.kind == ValidationKind::List && !v.dropdown {
        attrs.push_str(" showDropDown=\"1\"");
    }
    if v.prompt.is_some() {
        attrs.push_str(" showInputMessage=\"1\"");
    }
    if v.error.is_some() {
        attrs.push_str(" showErrorMessage=\"1\"");
    }
    let mut text_attr = |name: &str, value: &str| {
        if !value.is_empty() {
            attrs.push_str(&format!(" {name}=\"{}\"", xml::escape(value)));
        }
    };
    if let Some((_, title, message)) = &v.error {
        text_attr("errorTitle", title);
        text_attr("error", message);
    }
    if let Some((title, message)) = &v.prompt {
        text_attr("promptTitle", title);
        text_attr("prompt", message);
    }
    let mut body = String::new();
    for (name, f) in [("formula1", f1), ("formula2", f2)] {
        if let Some(f) = f {
            body.push_str(&format!("<{p}{name}>{}</{p}{name}>", xml::escape(f)));
        }
    }
    if body.is_empty() {
        format!("<{p}dataValidation{attrs} sqref=\"{sqref}\"/>")
    } else {
        format!("<{p}dataValidation{attrs} sqref=\"{sqref}\">{body}</{p}dataValidation>")
    }
}

/// A list's literal values (`"a,b,c"`), or `None` for a reference.
pub fn literal_list(formula: &str) -> Option<Vec<String>> {
    let inner = formula.trim().strip_prefix('"')?.strip_suffix('"')?;
    Some(
        inner
            .replace("\"\"", "\"")
            .split(',')
            .map(str::to_owned)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_forms_read() {
        let sheet = r#"<worksheet xmlns:x14="x" xmlns:xm="m"><sheetData/><dataValidations count="2"><dataValidation type="list" allowBlank="1" showInputMessage="1" showErrorMessage="1" promptTitle="Pick" prompt="One of them" sqref="B2:B5 D1"><formula1>"Food,Rent,Travel"</formula1></dataValidation><dataValidation type="whole" operator="greaterThan" errorStyle="warning" sqref="C2"><formula1>0</formula1></dataValidation></dataValidations><extLst><ext uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}"><x14:dataValidations count="1"><x14:dataValidation type="list" allowBlank="1"><x14:formula1><xm:f>Lists!$A$1:$A$3</xm:f></x14:formula1><xm:sqref>E2:E9</xm:sqref></x14:dataValidation></x14:dataValidations></ext></extLst></worksheet>"#;
        let v = parse(sheet);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].kind, "list");
        assert_eq!(v[0].ranges.len(), 2);
        assert_eq!(v[0].prompt, "One of them");
        assert_eq!(
            literal_list(v[0].formula1.as_deref().unwrap()).unwrap(),
            ["Food", "Rent", "Travel"]
        );
        assert_eq!(
            (v[1].operator.as_str(), v[1].error_style.as_str()),
            ("greaterThan", "warning")
        );
        assert!(v[2].ext);
        assert_eq!(v[2].formula1.as_deref(), Some("Lists!$A$1:$A$3"));
        assert_eq!(v[2].ranges, vec![Range::parse("E2:E9").unwrap()]);
        assert_eq!(
            &sheet[v[2].sqref_span.clone().unwrap()],
            "<xm:sqref>E2:E9</xm:sqref>"
        );
    }

    #[test]
    fn ranges_subtracted() {
        let r = |s| Range::parse(s).unwrap();
        assert_eq!(subtract(r("A1:C3"), r("E5")), vec![r("A1:C3")]);
        assert_eq!(
            subtract(r("A1:C3"), r("B2")),
            vec![r("A1:C1"), r("A3:C3"), r("A2:A2"), r("C2:C2")]
        );
        assert!(subtract(r("B2:B3"), r("A1:D9")).is_empty());
        assert_eq!(range_text(&r("B2:B2")), "B2");
    }
}
