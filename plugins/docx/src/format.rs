//! Formatting written as Word writes it (the docx list's WP9): a run's
//! properties (`w:rPr`, ECMA-376 part 1, 17.3.2) changed in place, each
//! property where the schema's sequence puts it (Word repairs a file
//! whose properties are out of order), the old ones kept in a
//! `w:rPrChange` when the document tracks changes; a paragraph's style
//! (`w:pStyle`, the first of its `w:pPr`), with a `w:pPrChange` likewise.

use kalem_ooxml::xml::{self, Reader, Token};
use kalem_viewer::{MarkChange, Script};

use crate::props::Span;

/// The children of `w:rPr` in the schema's order (`EG_RPrBase`).
const RPR_ORDER: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
];

/// What Clear takes away: the direct look of characters, their style
/// (`w:rStyle`), language and the like kept, as Word's Reset Character
/// Formatting does.
const CLEARED: &[&str] = &[
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "vanish",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "vertAlign",
    "em",
];

/// A change of one property of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// The property (by its local name) as this element.
    Set(&'static str, String),
    /// The property gone.
    Remove(&'static str),
    /// The typeface's attributes of `w:rFonts` set (`Some`) or taken
    /// away (`None`), the theme's fonts, which would win, taken away.
    Face(Option<String>),
}

/// A highlight color's name among the sixteen `w:highlight` has, when
/// the color is one of them.
pub fn highlight_name(rgb: [u8; 3]) -> Option<&'static str> {
    let v = (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2]);
    crate::flow::HIGHLIGHTS
        .iter()
        .find(|(_, c)| *c == v)
        .map(|(n, _)| *n)
}

fn hex(rgb: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// The operations a change makes on a run's properties, with prefix `p`:
/// a toggle turned off is taken away (and written as off by
/// [`explicit_off`] where a style still turns it on).
pub fn ops(p: &str, change: &MarkChange) -> Vec<Op> {
    let el = |name: &str| format!("<{p}{name}/>");
    let val = |name: &str, v: &str| format!("<{p}{name} {p}val=\"{v}\"/>");
    match change {
        MarkChange::Bold(true) => vec![Op::Set("b", el("b")), Op::Set("bCs", el("bCs"))],
        MarkChange::Bold(false) => vec![Op::Remove("b"), Op::Remove("bCs")],
        MarkChange::Italic(true) => vec![Op::Set("i", el("i")), Op::Set("iCs", el("iCs"))],
        MarkChange::Italic(false) => vec![Op::Remove("i"), Op::Remove("iCs")],
        MarkChange::Strike(true) => vec![Op::Set("strike", el("strike")), Op::Remove("dstrike")],
        MarkChange::Strike(false) => vec![Op::Remove("strike"), Op::Remove("dstrike")],
        MarkChange::Underline(Some(kind)) => {
            let kind = if kind.is_empty() {
                "single"
            } else {
                kind.as_str()
            };
            vec![Op::Set("u", val("u", kind))]
        }
        MarkChange::Underline(None) => vec![Op::Remove("u")],
        MarkChange::Script(Script::Superscript) => {
            vec![Op::Set("vertAlign", val("vertAlign", "superscript"))]
        }
        MarkChange::Script(Script::Subscript) => {
            vec![Op::Set("vertAlign", val("vertAlign", "subscript"))]
        }
        MarkChange::Script(Script::Baseline) => vec![Op::Remove("vertAlign")],
        MarkChange::Color(Some(rgb)) => vec![Op::Set("color", val("color", &hex(*rgb)))],
        MarkChange::Color(None) => vec![Op::Remove("color")],
        MarkChange::Highlight(Some(rgb)) => match highlight_name(*rgb) {
            Some(name) => vec![
                Op::Set("highlight", val("highlight", name)),
                Op::Remove("shd"),
            ],
            None => vec![
                Op::Remove("highlight"),
                Op::Set(
                    "shd",
                    format!(
                        "<{p}shd {p}val=\"clear\" {p}color=\"auto\" {p}fill=\"{}\"/>",
                        hex(*rgb)
                    ),
                ),
            ],
        },
        MarkChange::Highlight(None) => vec![Op::Remove("highlight"), Op::Remove("shd")],
        MarkChange::Size(Some(pt)) => {
            let half = (pt * 2.0).round().clamp(2.0, 3276.0) as u32;
            vec![
                Op::Set("sz", val("sz", &half.to_string())),
                Op::Set("szCs", val("szCs", &half.to_string())),
            ]
        }
        MarkChange::Size(None) => vec![Op::Remove("sz"), Op::Remove("szCs")],
        MarkChange::Face(face) => vec![Op::Face(face.clone())],
        MarkChange::Clear => CLEARED.iter().map(|n| Op::Remove(n)).collect(),
    }
}

/// The operations writing a toggle off that a style still turns on.
pub fn explicit_off(p: &str, change: &MarkChange) -> Vec<Op> {
    let off = |name: &'static str| Op::Set(name, format!("<{p}{name} {p}val=\"0\"/>"));
    match change {
        MarkChange::Bold(false) => vec![off("b"), off("bCs")],
        MarkChange::Italic(false) => vec![off("i"), off("iCs")],
        MarkChange::Strike(false) => vec![off("strike")],
        MarkChange::Underline(None) => {
            vec![Op::Set("u", format!("<{p}u {p}val=\"none\"/>"))]
        }
        MarkChange::Script(Script::Baseline) => vec![Op::Set(
            "vertAlign",
            format!("<{p}vertAlign {p}val=\"baseline\"/>"),
        )],
        _ => Vec::new(),
    }
}

/// The children of a properties element's text: each one's local name
/// and span, and where its content starts and ends.
struct Children {
    /// The content's start (after the start tag) and end (the end tag's
    /// start).
    content: Span,
    items: Vec<(String, Span)>,
}

fn children(text: &str) -> Option<Children> {
    let mut r = Reader::new(text);
    let Some(Token::Start(root)) = r.next_token() else {
        return None;
    };
    if root.empty {
        return Some(Children {
            content: root.span.end..root.span.end,
            items: Vec::new(),
        });
    }
    let start = root.span.end;
    let mut items = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(c) => {
                let from = c.span.start;
                let to = if c.empty {
                    c.span.end
                } else {
                    r.skip_element()
                };
                items.push((c.name.to_owned(), from..to));
            }
            Token::End { span, .. } => {
                return Some(Children {
                    content: start..span.start,
                    items,
                });
            }
            Token::Text { .. } => {}
        }
    }
    None
}

fn order(name: &str) -> Option<usize> {
    RPR_ORDER.iter().position(|n| *n == name)
}

/// A run's properties' content with `ops` made, every other byte kept.
fn apply_ops(content: &str, items: &[(String, Span)], p: &str, ops: &[Op]) -> String {
    // The children as text, in order, each tagged with its name.
    let mut kids: Vec<(String, String)> = items
        .iter()
        .map(|(n, s)| (n.clone(), content[s.clone()].to_owned()))
        .collect();
    for op in ops {
        match op {
            Op::Remove(name) => kids.retain(|(n, _)| n != name),
            Op::Set(name, xml) => set_child(&mut kids, name, xml.clone()),
            Op::Face(face) => {
                let at = kids.iter().position(|(n, _)| n == "rFonts");
                let tag = at.map(|i| kids[i].1.clone());
                let mut tag = tag.unwrap_or_else(|| format!("<{p}rFonts/>"));
                for a in ["asciiTheme", "hAnsiTheme"] {
                    tag = xml::remove_attr(&tag, &format!("{p}{a}"));
                }
                match face {
                    Some(f) => {
                        tag = xml::set_attr(&tag, &format!("{p}ascii"), f);
                        tag = xml::set_attr(&tag, &format!("{p}hAnsi"), f);
                    }
                    None => {
                        tag = xml::remove_attr(&tag, &format!("{p}ascii"));
                        tag = xml::remove_attr(&tag, &format!("{p}hAnsi"));
                    }
                }
                let bare = tag.trim_end_matches("/>").trim_end() == format!("<{p}rFonts");
                if bare {
                    kids.retain(|(n, _)| n != "rFonts");
                } else {
                    set_child(&mut kids, "rFonts", tag);
                }
            }
        }
    }
    kids.into_iter().map(|(_, x)| x).collect()
}

/// `name` set to `xml` among the children: in its place when it is
/// there, else after the last child the schema puts before it (an
/// extension's child, of no known place, left where it is).
fn set_child(kids: &mut Vec<(String, String)>, name: &str, xml: String) {
    if let Some(k) = kids.iter_mut().find(|(n, _)| n == name) {
        k.1 = xml;
        return;
    }
    let mine = order(name).unwrap_or(usize::MAX);
    let at = kids
        .iter()
        .rposition(|(n, _)| order(n).is_some_and(|o| o < mine))
        .map_or(0, |i| i + 1);
    // Before a change record, which is always last.
    let at = kids
        .iter()
        .position(|(n, _)| n == "rPrChange")
        .map_or(at, |c| at.min(c));
    kids.insert(at, (name.to_owned(), xml));
}

/// A tracked change's author, date and ID.
#[derive(Debug, Clone)]
pub struct Tracking {
    /// The author.
    pub author: String,
    /// The date.
    pub date: String,
    /// The next ID to give.
    pub next_id: u64,
}

/// A run's `w:rPr` (its text, or `None` when the run has none) with
/// `ops` made: the new element, empty when nothing is left; `None` when
/// nothing changes. With `track`, the old properties are kept in a
/// `w:rPrChange` (unless one keeps them already).
pub fn edit_rpr(
    rpr: Option<&str>,
    p: &str,
    ops: &[Op],
    track: Option<&mut Tracking>,
) -> Option<String> {
    let text = rpr.map_or_else(|| format!("<{p}rPr/>"), str::to_owned);
    let c = children(&text)?;
    let content = &text[c.content.clone()];
    let items: Vec<(String, Span)> = c
        .items
        .iter()
        .map(|(n, s)| {
            (
                n.clone(),
                s.start - c.content.start..s.end - c.content.start,
            )
        })
        .collect();
    let new = apply_ops(content, &items, p, ops);
    if new == content {
        return None;
    }
    let mut new = new;
    if let Some(t) = track
        && !items.iter().any(|(n, _)| n == "rPrChange")
    {
        let old: String = items
            .iter()
            .filter(|(n, _)| n != "rPrChange")
            .map(|(_, s)| &content[s.clone()])
            .collect();
        new.push_str(&format!(
            "<{p}rPrChange {p}id=\"{}\" {p}author=\"{}\" {p}date=\"{}\"><{p}rPr>{old}</{p}rPr></{p}rPrChange>",
            t.next_id,
            xml::escape(&t.author),
            xml::escape(&t.date)
        ));
        t.next_id += 1;
    }
    if new.is_empty() {
        return Some(String::new());
    }
    Some(format!("<{p}rPr>{new}</{p}rPr>"))
}

/// A paragraph's `w:pPr` (its text, or `None`) with style `style` (none:
/// the default paragraph style, written as no `w:pStyle`): the new
/// element, empty when nothing is left; `None` when nothing changes.
/// With `track`, the old properties are kept in a `w:pPrChange` (unless
/// one keeps them already).
pub fn edit_ppr(
    ppr: Option<&str>,
    p: &str,
    style: Option<&str>,
    track: Option<&mut Tracking>,
) -> Option<String> {
    let text = ppr.map_or_else(|| format!("<{p}pPr/>"), str::to_owned);
    let c = children(&text)?;
    let content = &text[c.content.clone()];
    let items: Vec<(String, Span)> = c
        .items
        .iter()
        .map(|(n, s)| {
            (
                n.clone(),
                s.start - c.content.start..s.end - c.content.start,
            )
        })
        .collect();
    let mut kids: Vec<(String, String)> = items
        .iter()
        .map(|(n, s)| (n.clone(), content[s.clone()].to_owned()))
        .collect();
    kids.retain(|(n, _)| n != "pStyle");
    if let Some(s) = style {
        kids.insert(
            0,
            (
                "pStyle".into(),
                format!("<{p}pStyle {p}val=\"{}\"/>", xml::escape(s)),
            ),
        );
    }
    let mut new: String = kids.into_iter().map(|(_, x)| x).collect();
    if new == content {
        return None;
    }
    if let Some(t) = track
        && !items.iter().any(|(n, _)| n == "pPrChange")
    {
        let old: String = items
            .iter()
            .filter(|(n, _)| !matches!(n.as_str(), "rPr" | "sectPr" | "pPrChange"))
            .map(|(_, s)| &content[s.clone()])
            .collect();
        new.push_str(&format!(
            "<{p}pPrChange {p}id=\"{}\" {p}author=\"{}\" {p}date=\"{}\"><{p}pPr>{old}</{p}pPr></{p}pPrChange>",
            t.next_id,
            xml::escape(&t.author),
            xml::escape(&t.date)
        ));
        t.next_id += 1;
    }
    if new.is_empty() {
        return Some(String::new());
    }
    Some(format!("<{p}pPr>{new}</{p}pPr>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(rpr: Option<&str>, changes: &[MarkChange]) -> Option<String> {
        let ops: Vec<Op> = changes.iter().flat_map(|c| ops("w:", c)).collect();
        edit_rpr(rpr, "w:", &ops, None)
    }

    #[test]
    fn properties_in_the_schemas_order() {
        assert_eq!(
            run(None, &[MarkChange::Bold(true)]).unwrap(),
            "<w:rPr><w:b/><w:bCs/></w:rPr>"
        );
        // Between what comes before and after, an extension left in place.
        let rpr = r#"<w:rPr><w:rStyle w:val="X"/><w:sz w:val="20"/><w:lang w:val="tr-TR"/><w14:ligatures w14:val="none"/></w:rPr>"#;
        assert_eq!(
            run(
                Some(rpr),
                &[
                    MarkChange::Italic(true),
                    MarkChange::Color(Some([255, 0, 0]))
                ]
            )
            .unwrap(),
            r#"<w:rPr><w:rStyle w:val="X"/><w:i/><w:iCs/><w:color w:val="FF0000"/><w:sz w:val="20"/><w:lang w:val="tr-TR"/><w14:ligatures w14:val="none"/></w:rPr>"#
        );
        // Taken away to nothing: no element left.
        assert_eq!(
            run(Some("<w:rPr><w:b/></w:rPr>"), &[MarkChange::Bold(false)]).unwrap(),
            ""
        );
        // Already so: no change.
        assert_eq!(
            run(
                Some("<w:rPr><w:b/><w:bCs/></w:rPr>"),
                &[MarkChange::Bold(true)]
            ),
            None
        );
        // The typeface, the theme's fonts that would win taken away.
        assert_eq!(
            run(
                Some(
                    r#"<w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:eastAsia="MS Mincho"/></w:rPr>"#
                ),
                &[
                    MarkChange::Face(Some("Georgia".into())),
                    MarkChange::Size(Some(13.0))
                ]
            )
            .unwrap(),
            r#"<w:rPr><w:rFonts w:eastAsia="MS Mincho" w:ascii="Georgia" w:hAnsi="Georgia"/><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr>"#
        );
        // A highlight Word names, else a shading.
        assert_eq!(
            run(None, &[MarkChange::Highlight(Some([255, 255, 0]))]).unwrap(),
            r#"<w:rPr><w:highlight w:val="yellow"/></w:rPr>"#
        );
        assert_eq!(
            run(None, &[MarkChange::Highlight(Some([1, 2, 3]))]).unwrap(),
            r#"<w:rPr><w:shd w:val="clear" w:color="auto" w:fill="010203"/></w:rPr>"#
        );
        // Clear keeps the style and the language.
        assert_eq!(
            run(Some(r#"<w:rPr><w:rStyle w:val="X"/><w:b/><w:color w:val="FF0000"/><w:lang w:val="tr-TR"/></w:rPr>"#), &[MarkChange::Clear]).unwrap(),
            r#"<w:rPr><w:rStyle w:val="X"/><w:lang w:val="tr-TR"/></w:rPr>"#
        );
    }

    #[test]
    fn tracked_formatting_keeps_the_old_properties() {
        let mut t = Tracking {
            author: "A".into(),
            date: "D".into(),
            next_id: 7,
        };
        let ops = ops("w:", &MarkChange::Bold(true));
        let new = edit_rpr(Some("<w:rPr><w:i/></w:rPr>"), "w:", &ops, Some(&mut t)).unwrap();
        assert_eq!(
            new,
            r#"<w:rPr><w:b/><w:bCs/><w:i/><w:rPrChange w:id="7" w:author="A" w:date="D"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr>"#
        );
        // A second change keeps the first record, and puts new properties
        // before it.
        let ops = ops_of(&MarkChange::Size(Some(10.0)));
        let again = edit_rpr(Some(&new), "w:", &ops, Some(&mut t)).unwrap();
        assert!(again.ends_with(r#"<w:sz w:val="20"/><w:szCs w:val="20"/><w:rPrChange w:id="7" w:author="A" w:date="D"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr>"#), "{again}");
        assert_eq!(t.next_id, 8);
        let ppr = edit_ppr(
            Some(r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#),
            "w:",
            Some("Heading1"),
            Some(&mut t),
        )
        .unwrap();
        assert_eq!(
            ppr,
            r#"<w:pPr><w:pStyle w:val="Heading1"/><w:jc w:val="center"/><w:pPrChange w:id="8" w:author="A" w:date="D"><w:pPr><w:jc w:val="center"/></w:pPr></w:pPrChange></w:pPr>"#
        );
        assert_eq!(
            edit_ppr(
                Some(r#"<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>"#),
                "w:",
                None,
                None
            )
            .unwrap(),
            ""
        );
    }

    fn ops_of(c: &MarkChange) -> Vec<Op> {
        ops("w:", c)
    }
}
