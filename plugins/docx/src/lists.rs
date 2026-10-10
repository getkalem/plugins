//! Lists written as Word writes them (ECMA-376 part 1, 17.9): a
//! paragraph made a list item by its `w:numPr` (a level and an instance,
//! `w:numId`); an instance (`w:num`) of an abstract definition
//! (`w:abstractNum`) with its nine levels, which the numbering part
//! (`numbering.xml`) holds, every definition before every instance, as
//! the schema orders them. The definitions Kalem adds are Word's own
//! bullet and numbered lists.

use kalem_ooxml::xml::{self, Reader, Token};
use kalem_viewer::ListKind;

use crate::props::Span;

/// The relationship type of the numbering part.
pub const REL_NUMBERING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
/// The content type of the numbering part.
pub const CT_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// A numbering part with no lists.
pub fn new_numbering_part() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:numbering xmlns:w=\"{NS_W}\"></w:numbering>"
    )
}

/// A list kind's number format (`w:numFmt`) for its first level: CSS's
/// names of numbering styles as Word names them; `None` for one Word
/// lacks.
pub fn format_of(kind: &ListKind) -> Option<&'static str> {
    Some(match kind {
        ListKind::Bullet => "bullet",
        ListKind::Numbered(style) => match style.as_str() {
            "decimal" => "decimal",
            "lower-alpha" | "lower-latin" => "lowerLetter",
            "upper-alpha" | "upper-latin" => "upperLetter",
            "lower-roman" => "lowerRoman",
            "upper-roman" => "upperRoman",
            "decimal-leading-zero" => "decimalZero",
            _ => return None,
        },
    })
}

/// Word's own definition of a list whose first level is numbered as
/// `format` (`bullet` for bullets), with prefix `p` and ID `id`: nine
/// levels, each indented half an inch more with a quarter-inch hanging
/// indent; bullets the Symbol dot, Courier New's `o` and the Wingdings
/// square in turn, numbers `1.`, `a.` and `i.` in turn after the first.
pub fn abstract_xml(p: &str, id: u32, format: &str) -> String {
    let mut out = format!(
        "<{p}abstractNum {p}abstractNumId=\"{id}\"><{p}multiLevelType {p}val=\"hybridMultilevel\"/>"
    );
    for i in 0..9u32 {
        let left = 720 * (i + 1);
        let (fmt, text, font) = if format == "bullet" {
            match i % 3 {
                0 => ("bullet", "\u{F0B7}".to_string(), Some("Symbol")),
                1 => ("bullet", "o".to_string(), Some("Courier New")),
                _ => ("bullet", "\u{F0A7}".to_string(), Some("Wingdings")),
            }
        } else {
            let fmt = if i == 0 {
                format
            } else {
                ["decimal", "lowerLetter", "lowerRoman"][(i % 3) as usize]
            };
            (fmt, format!("%{}.", i + 1), None)
        };
        let rpr = font.map_or(String::new(), |f| {
            format!(
                "<{p}rPr><{p}rFonts {p}ascii=\"{f}\" {p}hAnsi=\"{f}\" {p}hint=\"default\"/></{p}rPr>"
            )
        });
        out.push_str(&format!(
            "<{p}lvl {p}ilvl=\"{i}\"><{p}start {p}val=\"1\"/><{p}numFmt {p}val=\"{fmt}\"/><{p}lvlText {p}val=\"{}\"/><{p}lvlJc {p}val=\"left\"/><{p}pPr><{p}ind {p}left=\"{left}\" {p}hanging=\"360\"/></{p}pPr>{rpr}</{p}lvl>",
            xml::escape(&text)
        ));
    }
    out.push_str(&format!("</{p}abstractNum>"));
    out
}

/// An instance of definition `abstract_id` with prefix `p` and ID `id`;
/// with `restart`, its first level starting at 1 again, as a new list
/// does.
pub fn num_xml(p: &str, id: u32, abstract_id: &str, restart: bool) -> String {
    let over = if restart {
        format!("<{p}lvlOverride {p}ilvl=\"0\"><{p}startOverride {p}val=\"1\"/></{p}lvlOverride>")
    } else {
        String::new()
    };
    format!("<{p}num {p}numId=\"{id}\"><{p}abstractNumId {p}val=\"{abstract_id}\"/>{over}</{p}num>")
}

/// Where a numbering part's definitions and instances are: the end of
/// the last definition (where a new one goes), the end of the last
/// instance, and the content's end; and the highest IDs.
#[derive(Debug, Clone, Default)]
pub struct Places {
    /// Where a new `w:abstractNum` goes.
    pub abstract_at: usize,
    /// Where a new `w:num` goes.
    pub num_at: usize,
    /// The highest `w:abstractNumId`.
    pub max_abstract: u32,
    /// The highest `w:numId`.
    pub max_num: u32,
    /// Each instance's ID and its definition's.
    pub nums: Vec<(String, String)>,
}

/// The places of a numbering part's text (its root's content ending at
/// `content_end`).
pub fn places(src: &str, content_end: usize) -> Places {
    let mut out = Places::default();
    let mut last_abstract: Option<usize> = None;
    let mut first_num: Option<usize> = None;
    let mut last_num: Option<usize> = None;
    let mut cleanup: Option<usize> = None;
    let mut r = Reader::new(src);
    let mut depth = 0usize;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 {
                    let start = tag.span.start;
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    let span: Span = start..end;
                    match tag.name {
                        "abstractNum" => {
                            last_abstract = Some(span.end);
                            if let Some(n) = tag
                                .attr("abstractNumId")
                                .and_then(|v| v.trim().parse().ok())
                            {
                                out.max_abstract = out.max_abstract.max(n);
                            }
                        }
                        "num" => {
                            first_num.get_or_insert(span.start);
                            last_num = Some(span.end);
                            let id = tag
                                .attr("numId")
                                .map(|v| v.into_owned())
                                .unwrap_or_default();
                            if let Ok(n) = id.trim().parse::<u32>() {
                                out.max_num = out.max_num.max(n);
                            }
                            let body = &src[span.clone()];
                            let abs = body
                                .find("abstractNumId ")
                                .and_then(|i| {
                                    let rest = &body[i..];
                                    let v = rest.find("val=\"")? + 5;
                                    let e = rest[v..].find('"')?;
                                    Some(rest[v..v + e].to_owned())
                                })
                                .unwrap_or_default();
                            out.nums.push((id, abs));
                        }
                        "numIdMacAtCleanup" => {
                            cleanup.get_or_insert(span.start);
                        }
                        _ => {}
                    }
                    continue;
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => depth = depth.saturating_sub(1),
            Token::Text { .. } => {}
        }
    }
    let end = cleanup.unwrap_or(content_end);
    out.abstract_at = last_abstract.or(first_num).unwrap_or(end);
    out.num_at = last_num.unwrap_or(end);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_before_instances() {
        let src = r#"<w:numbering><w:abstractNum w:abstractNumId="3"><w:lvl w:ilvl="0"/></w:abstractNum><w:num w:numId="7"><w:abstractNumId w:val="3"/></w:num><w:numIdMacAtCleanup w:val="2"/></w:numbering>"#;
        let end = src.rfind("</w:numbering>").unwrap();
        let p = places(src, end);
        assert_eq!(
            &src[..p.abstract_at],
            r#"<w:numbering><w:abstractNum w:abstractNumId="3"><w:lvl w:ilvl="0"/></w:abstractNum>"#
        );
        assert!(src[p.num_at..].starts_with("<w:numIdMacAtCleanup"));
        assert_eq!((p.max_abstract, p.max_num), (3, 7));
        assert_eq!(p.nums, [("7".to_string(), "3".to_string())]);
        let empty = r#"<w:numbering></w:numbering>"#;
        let p = places(empty, empty.rfind("</").unwrap());
        assert_eq!((p.abstract_at, p.num_at), (13, 13));
        assert_eq!(
            format_of(&ListKind::Numbered("upper-roman".into())),
            Some("upperRoman")
        );
        assert_eq!(format_of(&ListKind::Numbered("hebrew".into())), None);
        let a = abstract_xml("w:", 4, "decimal");
        assert_eq!(a.matches("<w:lvl ").count(), 9);
        assert!(a.contains(r#"<w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2."/>"#));
    }
}
