//! Tracked changes found to be accepted or rejected (ECMA-376 part 1,
//! 17.13.5): an insertion or deletion of runs (`w:ins`, `w:del`,
//! `w:moveTo`, `w:moveFrom`), of a paragraph mark (the same, empty, in
//! the mark's `w:rPr`), or a change of formatting (`w:rPrChange`,
//! `w:pPrChange`), by its `w:id`, with the byte spans
//! [`crate::Document::decide`] rewrites.

use kalem_ooxml::xml::{Reader, Token};

use crate::props::Span;

/// What a tracked change is, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Runs inserted (`w:ins`, `w:moveTo`) or deleted (`w:del`,
    /// `w:moveFrom`): the element and its content.
    Runs {
        /// Deleted rather than inserted.
        deleted: bool,
        /// The whole element.
        span: Span,
        /// Its content.
        inner: Span,
    },
    /// A paragraph mark inserted or deleted: the empty element in the
    /// mark's properties, and where the paragraph starts.
    Mark {
        /// Deleted rather than inserted.
        deleted: bool,
        /// The element.
        span: Span,
        /// The `w:p`'s start.
        paragraph: usize,
    },
    /// A run's formatting changed: the `w:rPrChange`, and the `w:rPr` it is
    /// in with the old properties' content inside the change.
    Format {
        /// The `w:rPrChange`.
        change: Span,
        /// The `w:rPr` holding it.
        rpr: Span,
        /// The old `w:rPr`'s content, inside the change.
        old: Span,
    },
    /// A paragraph's formatting changed (`w:pPrChange`).
    ParaFormat {
        /// The `w:pPrChange`.
        change: Span,
    },
}

const NAMES: [&str; 6] = ["ins", "del", "moveFrom", "moveTo", "rPrChange", "pPrChange"];

/// Where the element whose start tag `tag` was read ends, and its
/// content's span.
fn element_end(r: &mut Reader<'_>, start_end: usize, empty: bool) -> (usize, Span) {
    if empty {
        return (start_end, start_end..start_end);
    }
    let mut depth = 0usize;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(c) if !c.empty => depth += 1,
            Token::End { span, .. } => {
                if depth == 0 {
                    return (span.end, start_end..span.start);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    (r.pos(), start_end..r.pos())
}

/// The tracked change of `w:id` `id` in a part's text.
pub fn find(src: &str, id: &str) -> Option<Found> {
    let mut r = Reader::new(src);
    let mut stack: Vec<(&str, usize)> = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if NAMES.contains(&tag.name) && tag.attr("id").as_deref() == Some(id) {
                    let parent = stack.last().map(|s| s.0);
                    let grand = stack.len().checked_sub(2).map(|i| stack[i].0);
                    let start = tag.span.start;
                    let name = tag.name;
                    let (end, inner) = element_end(&mut r, tag.span.end, tag.empty);
                    return Some(match name {
                        "rPrChange" => {
                            let rpr_start = stack.last()?.1;
                            // The old properties: the change's `w:rPr`.
                            let change_text = &src[start..end];
                            let mut cr = Reader::new(change_text);
                            let mut old = inner.end..inner.end;
                            while let Some(t) = cr.next_token() {
                                if let Token::Start(c) = t
                                    && c.name == "rPr"
                                {
                                    let (_, i) = element_end(&mut cr, c.span.end, c.empty);
                                    old = start + i.start..start + i.end;
                                    break;
                                }
                            }
                            let mut pr = Reader::new(&src[rpr_start..]);
                            let rpr_end = match pr.next_token() {
                                Some(Token::Start(p)) => {
                                    rpr_start + element_end(&mut pr, p.span.end, p.empty).0
                                }
                                _ => end,
                            };
                            Found::Format {
                                change: start..end,
                                rpr: rpr_start..rpr_end,
                                old,
                            }
                        }
                        "pPrChange" => Found::ParaFormat { change: start..end },
                        _ if parent == Some("rPr") && grand == Some("pPr") => Found::Mark {
                            deleted: matches!(name, "del" | "moveFrom"),
                            span: start..end,
                            paragraph: stack.iter().rev().find(|s| s.0 == "p").map_or(0, |s| s.1),
                        },
                        _ => Found::Runs {
                            deleted: matches!(name, "del" | "moveFrom"),
                            span: start..end,
                            inner,
                        },
                    });
                }
                if !tag.empty {
                    stack.push((tag.name, tag.span.start));
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    None
}

/// The IDs of a part's tracked changes, in order, each once; the
/// formatting changes of paragraphs left out when `paragraphs` is false.
pub fn ids(src: &str, paragraphs: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && NAMES.contains(&tag.name)
            && (paragraphs || tag.name != "pPrChange")
            && let Some(id) = tag.attr("id")
            && !out.iter().any(|o| *o == id)
        {
            out.push(id.into_owned());
        }
    }
    out
}

/// Deleted runs' text made ordinary again: `w:delText` as `w:t`,
/// `w:delInstrText` as `w:instrText`.
pub fn undeleted(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut at = 0;
    let mut r = Reader::new(xml);
    while let Some(t) = r.next_token() {
        let (span, qname) = match &t {
            Token::Start(tag) => (tag.span.clone(), tag.qname),
            Token::End { span, .. } => {
                let q = &xml[span.start + 2..span.end - 1];
                (span.clone(), q.trim())
            }
            Token::Text { .. } => continue,
        };
        let local = qname.rsplit(':').next().unwrap_or(qname);
        let new = match local {
            "delText" => "t",
            "delInstrText" => "instrText",
            _ => continue,
        };
        let prefix = &qname[..qname.len() - local.len()];
        let head = span.start + 1 + usize::from(matches!(t, Token::End { .. }));
        out.push_str(&xml[at..head]);
        out.push_str(prefix);
        out.push_str(new);
        at = head + qname.len();
    }
    out.push_str(&xml[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<w:body><w:p><w:pPr><w:rPr><w:ins w:id="7" w:author="A"/></w:rPr></w:pPr><w:ins w:id="1" w:author="A"><w:r><w:t>new</w:t></w:r></w:ins><w:del w:id="2" w:author="A"><w:r><w:delText xml:space="preserve"> old</w:delText></w:r></w:del><w:r><w:rPr><w:b/><w:rPrChange w:id="3" w:author="A"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>x</w:t></w:r></w:p></w:body>"#;

    #[test]
    fn changes_found_by_id() {
        let Some(Found::Runs {
            deleted,
            span,
            inner,
        }) = find(XML, "1")
        else {
            panic!()
        };
        assert!(!deleted);
        assert!(XML[span].starts_with("<w:ins w:id=\"1\""));
        assert_eq!(&XML[inner], "<w:r><w:t>new</w:t></w:r>");
        let Some(Found::Runs {
            deleted: true,
            inner,
            ..
        }) = find(XML, "2")
        else {
            panic!()
        };
        assert_eq!(
            undeleted(&XML[inner]),
            r#"<w:r><w:t xml:space="preserve"> old</w:t></w:r>"#
        );
        let Some(Found::Format { change, rpr, old }) = find(XML, "3") else {
            panic!()
        };
        assert!(XML[change].starts_with("<w:rPrChange"));
        assert_eq!(
            &XML[rpr.clone()],
            r#"<w:rPr><w:b/><w:rPrChange w:id="3" w:author="A"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr>"#
        );
        assert_eq!(&XML[old], "<w:i/>");
        let Some(Found::Mark {
            deleted: false,
            span,
            paragraph,
        }) = find(XML, "7")
        else {
            panic!()
        };
        assert_eq!(&XML[span], r#"<w:ins w:id="7" w:author="A"/>"#);
        assert_eq!(paragraph, XML.find("<w:p>").unwrap());
        assert_eq!(find(XML, "9"), None);
        assert_eq!(ids(XML, true), ["7", "1", "2", "3"]);
    }
}
