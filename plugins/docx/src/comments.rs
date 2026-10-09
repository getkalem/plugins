//! Comments written as Word writes them (ECMA-376 part 1, 17.13.4; the
//! extended comments part of [MS-DOCX] 2.5.42 for answers and done
//! marks): a `w:comment` in the comments part, its range in the story
//! between `w:commentRangeStart` and `w:commentRangeEnd`, a run with its
//! `w:commentReference` after the end; an answer a comment of its own,
//! anchored beside the comment it answers, whose `w15:commentEx` names
//! the answered one's last paragraph (`w15:paraIdParent`, by its
//! `w14:paraId`).

use std::collections::{HashMap, HashSet};

use kalem_ooxml::xml::{self, Reader, Token};

use crate::props::Span;

/// The relationship type of the comments part.
pub const REL_COMMENTS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
/// The content type of the comments part.
pub const CT_COMMENTS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
/// The relationship type of the extended comments part.
pub const REL_COMMENTS_EX: &str =
    "http://schemas.microsoft.com/office/2011/relationships/commentsExtended";
/// The content type of the extended comments part.
pub const CT_COMMENTS_EX: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml";
const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// The namespace of `w14:paraId`.
pub const NS_W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const NS_W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

/// A name's initials, as Word takes them by default: each word's first
/// letter, upper-cased.
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// A comments part with no comments, as Word starts one.
pub fn new_comments_part() -> String {
    format!(
        "{DECLARATION}<w:comments xmlns:mc=\"{NS_MC}\" xmlns:w=\"{NS_W}\" xmlns:w14=\"{NS_W14}\" mc:Ignorable=\"w14\"></w:comments>"
    )
}

/// An extended comments part with no entries.
pub fn new_comments_ex_part() -> String {
    format!(
        "{DECLARATION}<w15:commentsEx xmlns:mc=\"{NS_MC}\" xmlns:w15=\"{NS_W15}\" mc:Ignorable=\"w15\"></w15:commentsEx>"
    )
}

/// A part's root element: its start tag, its qualified name, and where
/// its content ends (its end tag's start; the start tag's end when it
/// closes itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    /// The start tag.
    pub tag: Span,
    /// Its qualified name (`w:comments`).
    pub qname: String,
    /// Written as `<w:comments/>`.
    pub empty: bool,
    /// Where its end tag starts.
    pub content_end: usize,
}

impl Root {
    /// The prefix of its name (`w:`).
    pub fn prefix(&self) -> &str {
        xml::prefix(&self.qname)
    }
}

/// The root element of a part.
pub fn root(src: &str) -> Option<Root> {
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t {
            let qname = tag.qname.to_owned();
            let content_end = if tag.empty {
                tag.span.end
            } else {
                let close = format!("</{qname}");
                src.rfind(&close).unwrap_or(src.len())
            };
            return Some(Root {
                tag: tag.span.clone(),
                qname,
                empty: tag.empty,
                content_end,
            });
        }
    }
    None
}

/// The namespaces a start tag declares: prefix and URI.
fn declarations(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag;
    while let Some(p) = rest.find("xmlns:") {
        let before_ok = p == 0 || rest.as_bytes()[p - 1].is_ascii_whitespace();
        rest = &rest[p + 6..];
        let Some(eq) = rest.find('=') else { break };
        let name = rest[..eq].trim().to_owned();
        let value = rest[eq + 1..].trim_start();
        let Some(q) = value.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let Some(end) = value[1..].find(q) else { break };
        if before_ok {
            out.push((name, value[1..1 + end].to_owned()));
        }
    }
    out
}

/// The prefix a start tag declares for `uri`, with its colon.
pub fn prefix_for(tag: &str, uri: &str) -> Option<String> {
    declarations(tag)
        .into_iter()
        .find(|(_, u)| u == uri)
        .map(|(p, _)| format!("{p}:"))
}

/// A root start tag declaring `uri` (as `prefix`, unless it is declared
/// already) and listing its prefix among the ignorable ones of markup
/// compatibility, as Word does for its extensions; and the prefix it
/// has, with its colon.
pub fn with_extension(tag: &str, prefix: &str, uri: &str) -> (String, String) {
    if let Some(p) = prefix_for(tag, uri) {
        return (tag.to_owned(), p);
    }
    let mut tag = xml::set_attr(tag, &format!("xmlns:{prefix}"), uri);
    let mc = match prefix_for(&tag, NS_MC) {
        Some(p) => p,
        None => {
            tag = xml::set_attr(&tag, "xmlns:mc", NS_MC);
            "mc:".to_owned()
        }
    };
    let name = format!("{mc}Ignorable");
    let current = start_tag_attr(&tag, &name).unwrap_or_default();
    if !current.split_whitespace().any(|p| p == prefix) {
        let list = if current.trim().is_empty() {
            prefix.to_owned()
        } else {
            format!("{} {prefix}", current.trim())
        };
        tag = xml::set_attr(&tag, &name, &list);
    }
    (tag, format!("{prefix}:"))
}

/// An attribute of a start tag by its qualified name.
fn start_tag_attr(tag: &str, qname: &str) -> Option<String> {
    let mut r = Reader::new(tag);
    match r.next_token() {
        Some(Token::Start(t)) => t.attr_qualified(qname).map(|v| v.into_owned()),
        _ => None,
    }
}

/// A comment to write.
#[derive(Debug, Clone)]
pub struct NewComment<'a> {
    /// Its `w:id`.
    pub id: &'a str,
    /// Its author.
    pub author: &'a str,
    /// Its date, ISO 8601.
    pub date: &'a str,
    /// Its text: a paragraph a line.
    pub text: &'a str,
}

/// The lines of a comment's text, each a paragraph.
pub fn lines(text: &str) -> Vec<&str> {
    text.split('\n').map(|l| l.trim_end_matches('\r')).collect()
}

/// A `w:comment` with prefix `p`: a paragraph a line, the first holding
/// the comment's mark (`w:annotationRef`); each paragraph with its
/// `w14:paraId` (prefix `w14`) when given; the styles Word gives them
/// when the document has them.
pub fn comment_xml(
    p: &str,
    w14: Option<&str>,
    c: &NewComment<'_>,
    para_ids: &[String],
    text_style: bool,
    reference_style: bool,
) -> Result<String, String> {
    let mut out = format!(
        "<{p}comment {p}id=\"{}\" {p}author=\"{}\" {p}date=\"{}\" {p}initials=\"{}\">",
        c.id,
        xml::escape(c.author),
        xml::escape(c.date),
        xml::escape(&initials(c.author))
    );
    for (i, line) in lines(c.text).into_iter().enumerate() {
        let ids = match (w14, para_ids.get(i)) {
            (Some(w), Some(id)) => format!(" {w}paraId=\"{id}\" {w}textId=\"77777777\""),
            _ => String::new(),
        };
        out.push_str(&format!("<{p}p{ids}>"));
        if text_style {
            out.push_str(&format!(
                "<{p}pPr><{p}pStyle {p}val=\"CommentText\"/></{p}pPr>"
            ));
        }
        if i == 0 {
            out.push_str(&format!("<{p}r>"));
            if reference_style {
                out.push_str(&format!(
                    "<{p}rPr><{p}rStyle {p}val=\"CommentReference\"/></{p}rPr>"
                ));
            }
            out.push_str(&format!("<{p}annotationRef/></{p}r>"));
        }
        if !line.is_empty() {
            let content = crate::edit::run_content(p, line)?;
            out.push_str(&format!("<{p}r>{content}</{p}r>"));
        }
        out.push_str(&format!("</{p}p>"));
    }
    out.push_str(&format!("</{p}comment>"));
    Ok(out)
}

/// The run holding a comment's reference mark, after its range.
pub fn reference_run(p: &str, id: &str, styled: bool) -> String {
    let rpr = if styled {
        format!("<{p}rPr><{p}rStyle {p}val=\"CommentReference\"/></{p}rPr>")
    } else {
        String::new()
    };
    format!("<{p}r>{rpr}<{p}commentReference {p}id=\"{id}\"/></{p}r>")
}

/// An entry of the extended comments part with prefix `p`.
pub fn comment_ex(p: &str, para: &str, parent: Option<&str>, done: bool) -> String {
    let parent = parent.map_or(String::new(), |id| format!(" {p}paraIdParent=\"{id}\""));
    let done = u8::from(done);
    format!("<{p}commentEx {p}paraId=\"{para}\"{parent} {p}done=\"{done}\"/>")
}

/// Every element named `name` (its local name) with attribute `attr`:
/// the whole element, its start tag, and the attribute's value.
pub fn elements(src: &str, name: &str, attr: &str) -> Vec<(Span, Span, String)> {
    let mut out = Vec::new();
    let mut r = Reader::new(src);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != name {
            continue;
        }
        let Some(v) = tag.attr(attr).map(|v| v.into_owned()) else {
            continue;
        };
        let start = tag.span.clone();
        let end = if tag.empty {
            start.end
        } else {
            r.skip_element()
        };
        out.push((start.start..end, start, v));
    }
    out
}

/// Whether a run holds nothing but a comment's reference (and its
/// properties), so that it goes with the comment.
pub fn reference_only(src: &str, run: &Span) -> bool {
    let mut r = Reader::new(&src[run.clone()]);
    let _ = r.next_token();
    let mut depth = 0usize;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 0 && !matches!(tag.name, "rPr" | "commentReference") {
                    return false;
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            Token::Text { .. } => {}
        }
    }
    true
}

/// Where a comment is anchored in a story part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Markers {
    /// Its `w:commentRangeStart`.
    pub start: Option<Span>,
    /// Its `w:commentRangeEnd`.
    pub end: Option<Span>,
    /// The run holding its `w:commentReference`.
    pub reference_run: Option<Span>,
    /// The `w:commentReference` itself.
    pub reference: Option<Span>,
}

impl Markers {
    /// Whether the comment is anchored here at all.
    pub fn found(&self) -> bool {
        self.start.is_some() || self.end.is_some() || self.reference_run.is_some()
    }
}

/// Where comment `id` is anchored in a story part.
pub fn markers(src: &str, id: &str) -> Markers {
    let mut out = Markers::default();
    let mut r = Reader::new(src);
    // The runs open: where each starts.
    let mut runs: Vec<usize> = Vec::new();
    let mut stack: Vec<&str> = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                let mine = || tag.attr("id").as_deref() == Some(id);
                match tag.name {
                    "commentRangeStart" if mine() => out.start = Some(tag.span.clone()),
                    "commentRangeEnd" if mine() => out.end = Some(tag.span.clone()),
                    "commentReference" if mine() => {
                        out.reference = Some(tag.span.clone());
                        if let Some(&start) = runs.last() {
                            let mut rr = Reader::new(&src[start..]);
                            let _ = rr.next_token();
                            out.reference_run = Some(start..start + rr.skip_element());
                        }
                    }
                    _ => {}
                }
                if !tag.empty {
                    if tag.name == "r" {
                        runs.push(tag.span.start);
                    }
                    stack.push(tag.name);
                }
            }
            Token::End { .. } => {
                if stack.pop() == Some("r") {
                    runs.pop();
                }
            }
            Token::Text { .. } => {}
        }
    }
    out
}

/// The `w14:paraId`s a part's text holds.
pub fn para_ids(src: &str) -> impl Iterator<Item = u32> + '_ {
    src.match_indices(":paraId=\"").filter_map(move |(at, m)| {
        let rest = &src[at + m.len()..];
        let end = rest.find('"')?;
        u32::from_str_radix(&rest[..end], 16).ok()
    })
}

/// `n` paragraph IDs none of `used` is, each below `0x80000000` as
/// [MS-DOCX] asks, written as Word writes them (eight hexadecimal
/// digits).
pub fn fresh_para_ids(used: &HashSet<u32>, n: usize) -> Vec<String> {
    let max = used
        .iter()
        .copied()
        .filter(|v| *v < 0x8000_0000)
        .max()
        .unwrap_or(0);
    let mut next = if (max as u64) + (n as u64) < 0x8000_0000 {
        max + 1
    } else {
        1
    };
    let mut out = Vec::with_capacity(n);
    while out.len() < n && next < 0x8000_0000 {
        if !used.contains(&next) {
            out.push(format!("{next:08X}"));
        }
        next += 1;
    }
    out
}

/// Each comment's paragraphs in a comments part: the comment's `w:id`,
/// and the start tag and `w14:paraId` of each of its `w:p`, in order.
pub fn comment_paragraphs(src: &str) -> HashMap<String, Vec<(Span, Option<String>)>> {
    let mut out: HashMap<String, Vec<(Span, Option<String>)>> = HashMap::new();
    let mut r = Reader::new(src);
    let mut current: Option<String> = None;
    let mut depth = 0usize;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if tag.name == "comment" && current.is_none() {
                    let id = tag.attr("id").unwrap_or_default().into_owned();
                    out.entry(id.clone()).or_default();
                    if !tag.empty {
                        current = Some(id);
                        depth = 0;
                    }
                    continue;
                }
                if let Some(id) = &current {
                    if tag.name == "p" {
                        let para = tag.attr("paraId").map(|v| v.into_owned());
                        out.entry(id.clone())
                            .or_default()
                            .push((tag.span.clone(), para));
                    }
                    if !tag.empty {
                        depth += 1;
                    }
                }
            }
            Token::End { .. } => {
                if current.is_some() {
                    if depth == 0 {
                        current = None;
                    } else {
                        depth -= 1;
                    }
                }
            }
            Token::Text { .. } => {}
        }
    }
    out
}

/// What the extended comments part says of each comment, by `w:id`: the
/// comment it answers, and whether it is done.
pub fn threads(comments: &str, extended: &str) -> HashMap<String, (Option<String>, bool)> {
    let by_para: HashMap<String, String> = comment_paragraphs(comments)
        .into_iter()
        .filter_map(|(id, ps)| ps.last().and_then(|(_, p)| p.clone()).map(|p| (p, id)))
        .collect();
    let mut out = HashMap::new();
    let mut r = Reader::new(extended);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != "commentEx" {
            continue;
        }
        let Some(id) = tag
            .attr("paraId")
            .and_then(|p| by_para.get(p.as_ref()).cloned())
        else {
            continue;
        };
        let parent = tag
            .attr("paraIdParent")
            .and_then(|p| by_para.get(p.as_ref()).cloned());
        let done = tag
            .attr("done")
            .is_some_and(|v| matches!(v.as_ref(), "1" | "true" | "on"));
        out.insert(id, (parent, done));
    }
    out
}

/// A paragraph of a comment, as an edit of its text sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommentPara {
    /// The whole `w:p`.
    span: Span,
    /// Its start tag.
    start_tag: Span,
    /// Written as `<w:p/>`.
    empty: bool,
    /// Its `w:pPr`.
    ppr: Option<Span>,
    /// The run holding the comment's mark (`w:annotationRef`).
    mark_run: Option<Span>,
}

/// The paragraphs of a `w:comment` (its text from its start tag on).
fn comment_paras(item: &str) -> Result<Vec<CommentPara>, String> {
    let mut r = Reader::new(item);
    match r.next_token() {
        Some(Token::Start(t)) if t.name == "comment" && !t.empty => {}
        Some(Token::Start(t)) if t.name == "comment" => return Ok(Vec::new()),
        _ => return Err("This is not a comment".into()),
    }
    let mut out = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "p" => {
                let start_tag = tag.span.clone();
                if tag.empty {
                    out.push(CommentPara {
                        span: start_tag.clone(),
                        start_tag,
                        empty: true,
                        ppr: None,
                        mark_run: None,
                    });
                    continue;
                }
                let (mut ppr, mut mark_run) = (None, None);
                loop {
                    match r.next_token() {
                        Some(Token::Start(c)) => {
                            let from = c.span.start;
                            let to = if c.empty {
                                c.span.end
                            } else {
                                r.skip_element()
                            };
                            match c.name {
                                "pPr" => ppr = Some(from..to),
                                "r" if mark_run.is_none()
                                    && item[from..to].contains("annotationRef") =>
                                {
                                    mark_run = Some(from..to);
                                }
                                _ => {}
                            }
                        }
                        Some(Token::End { span, .. }) => {
                            out.push(CommentPara {
                                span: start_tag.start..span.end,
                                start_tag,
                                empty: false,
                                ppr,
                                mark_run,
                            });
                            break;
                        }
                        Some(Token::Text { .. }) => {}
                        None => return Err("The comment is cut short".into()),
                    }
                }
            }
            Token::Start(tag) if matches!(tag.name, "tbl" | "sdt" | "customXml" | "altChunk") => {
                return Err(
                    "A comment holding a table or a content control is not edited yet".into(),
                );
            }
            Token::Start(tag) => {
                if !tag.empty {
                    r.skip_element();
                }
            }
            Token::End { .. } => break,
            Token::Text { .. } => {}
        }
    }
    Ok(out)
}

/// Whether a comment's paragraphs carry `w14:paraId`s.
pub fn has_para_ids(item: &str) -> bool {
    comment_paras(item)
        .map(|ps| {
            ps.iter()
                .any(|p| item[p.start_tag.clone()].contains(":paraId="))
        })
        .unwrap_or(false)
}

/// A comment's paragraphs written anew with `text`, a paragraph a line,
/// as Word rewrites them: each new paragraph takes an old one's start tag
/// and properties in order, the last line the last paragraph's (so its
/// `w14:paraId`, which answers and the done mark name, stays); a line
/// more than there were paragraphs gets a new one, with a fresh ID from
/// `fresh` (prefix `w14`) when the others have IDs; the comment's mark
/// stays at the start. The bytes of the `w:comment` (`item`) replaced,
/// and what replaces them.
pub fn retext(
    item: &str,
    p: &str,
    w14: Option<&str>,
    text: &str,
    fresh: &mut dyn FnMut() -> Option<String>,
) -> Result<(Span, String), String> {
    let paras = comment_paras(item)?;
    let (Some(first), Some(last)) = (paras.first(), paras.last()) else {
        return Err("The comment has no paragraph".into());
    };
    let mark = paras
        .iter()
        .find_map(|c| c.mark_run.as_ref())
        .map_or(String::new(), |s| item[s.clone()].to_owned());
    let open = |c: &CommentPara| {
        let tag = &item[c.start_tag.clone()];
        if c.empty {
            tag.trim_end_matches("/>").trim_end().to_owned() + ">"
        } else {
            tag.to_owned()
        }
    };
    let ppr_of = |c: &CommentPara| {
        c.ppr
            .as_ref()
            .map_or(String::new(), |s| item[s.clone()].to_owned())
    };
    let lines = lines(text);
    let n = lines.len();
    let mut out = String::new();
    for (i, line) in lines.into_iter().enumerate() {
        let (tag, ppr) = if i + 1 == n {
            (open(last), ppr_of(last))
        } else if i + 1 < paras.len() {
            (open(&paras[i]), ppr_of(&paras[i]))
        } else {
            let id = match w14 {
                Some(w) => match fresh() {
                    Some(id) => format!(" {w}paraId=\"{id}\" {w}textId=\"77777777\""),
                    None => return Err("No paragraph ID is left for the comment".into()),
                },
                None => String::new(),
            };
            (format!("<{p}p{id}>"), ppr_of(first))
        };
        out.push_str(&tag);
        out.push_str(&ppr);
        if i == 0 {
            out.push_str(&mark);
        }
        if !line.is_empty() {
            let content = crate::edit::run_content(p, line)?;
            out.push_str(&format!("<{p}r>{content}</{p}r>"));
        }
        out.push_str(&format!("</{p}p>"));
    }
    Ok((first.span.start..last.span.end, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_and_threads_found() {
        let story = r#"<w:p><w:commentRangeStart w:id="3"/><w:r><w:t>x</w:t></w:r><w:commentRangeEnd w:id="3"/><w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="3"/></w:r></w:p>"#;
        let m = markers(story, "3");
        assert_eq!(
            &story[m.start.unwrap()],
            r#"<w:commentRangeStart w:id="3"/>"#
        );
        assert_eq!(&story[m.end.unwrap()], r#"<w:commentRangeEnd w:id="3"/>"#);
        let run = &story[m.reference_run.unwrap()];
        assert!(
            run.starts_with("<w:r>") && run.ends_with("</w:r>") && run.contains("commentReference")
        );
        assert!(!markers(story, "4").found());
        let comments = r#"<w:comments><w:comment w:id="3"><w:p w14:paraId="0000000A"/><w:p w14:paraId="0000000B"/></w:comment><w:comment w:id="5"><w:p w14:paraId="0000000C"/></w:comment></w:comments>"#;
        let ex = r#"<w15:commentsEx><w15:commentEx w15:paraId="0000000B" w15:done="1"/><w15:commentEx w15:paraId="0000000C" w15:paraIdParent="0000000B" w15:done="0"/></w15:commentsEx>"#;
        let t = threads(comments, ex);
        assert_eq!(t["3"], (None, true));
        assert_eq!(t["5"], (Some("3".into()), false));
        let used: HashSet<u32> = para_ids(comments).collect();
        assert_eq!(fresh_para_ids(&used, 2), ["0000000D", "0000000E"]);
    }

    #[test]
    fn extensions_declared_once() {
        let tag = r#"<w:comments xmlns:w="x">"#;
        let (t, p) = with_extension(tag, "w14", NS_W14);
        assert_eq!(p, "w14:");
        assert!(t.contains(&format!("xmlns:w14=\"{NS_W14}\"")));
        assert!(t.contains("mc:Ignorable=\"w14\""));
        let (t2, _) = with_extension(&t, "w15", NS_W15);
        assert!(t2.contains("mc:Ignorable=\"w14 w15\""), "{t2}");
        assert_eq!(with_extension(&t2, "w14", NS_W14).0, t2);
        assert_eq!(initials("Ayşe Nur Yılmaz"), "ANY");
    }
}
