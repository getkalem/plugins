//! Relationships and content types (ECMA-376 part 2, 9.3 and 10.1).

use crate::xml::{Reader, Token};

/// One relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rel {
    /// `rId…`.
    pub id: String,
    /// The type's last path segment (`worksheet`), which is what the
    /// transitional and the strict namespaces have in common.
    pub kind: String,
    /// The target as written.
    pub target: String,
    /// Whether the target is outside the package.
    pub external: bool,
}

/// The relationships part of `part` (`xl/workbook.xml` → `xl/_rels/workbook.xml.rels`).
pub fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Parses a relationships part.
pub fn parse(xml: &str) -> Vec<Rel> {
    let mut out = Vec::new();
    let mut r = Reader::new(xml);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "Relationship"
        {
            let ty = tag.attr("Type").unwrap_or_default();
            out.push(Rel {
                id: tag.attr("Id").unwrap_or_default().into_owned(),
                kind: ty.rsplit('/').next().unwrap_or_default().to_owned(),
                target: tag.attr("Target").unwrap_or_default().into_owned(),
                external: tag.attr("TargetMode").as_deref() == Some("External"),
            });
        }
    }
    out
}

/// The part a relative target names, seen from `source` (part names have no
/// leading slash here, as in the ZIP directory).
pub fn resolve(source: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or(target);
    let mut segs: Vec<&str> = if let Some(abs) = target.strip_prefix('/') {
        return normalize(abs.split('/').collect());
    } else {
        source
            .rsplit_once('/')
            .map_or(Vec::new(), |(d, _)| d.split('/').collect())
    };
    segs.extend(target.split('/'));
    normalize(segs)
}

fn normalize(segs: Vec<&str>) -> String {
    let mut out: Vec<&str> = Vec::new();
    for s in segs {
        match s {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// Removes the relationship with `id` from a relationships part, keeping
/// every other byte.
pub fn remove(xml: &str, id: &str) -> String {
    let mut r = Reader::new(xml);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "Relationship"
            && tag.attr("Id").as_deref() == Some(id)
        {
            let end = if tag.empty {
                tag.span.end
            } else {
                r.skip_element()
            };
            return format!("{}{}", &xml[..tag.span.start], &xml[end..]);
        }
    }
    xml.to_owned()
}

/// Removes the `Override` for `/part` from `[Content_Types].xml`.
pub fn remove_override(xml: &str, part: &str) -> String {
    let want = format!("/{part}");
    let mut r = Reader::new(xml);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "Override"
            && tag
                .attr("PartName")
                .is_some_and(|p| p.eq_ignore_ascii_case(&want))
        {
            let end = if tag.empty {
                tag.span.end
            } else {
                r.skip_element()
            };
            return format!("{}{}", &xml[..tag.span.start], &xml[end..]);
        }
    }
    xml.to_owned()
}

/// The content type `[Content_Types].xml` gives `part` (no leading
/// slash): its `Override`, else the `Default` of its extension, both
/// compared without regard to case as part names are (part 2, 9.1.1).
pub fn content_type(types: &str, part: &str) -> Option<String> {
    let want = format!("/{part}");
    let ext = part.rsplit_once('.').map(|(_, e)| e);
    let mut default = None;
    let mut r = Reader::new(types);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        match tag.name {
            "Override"
                if tag
                    .attr("PartName")
                    .is_some_and(|p| p.eq_ignore_ascii_case(&want)) =>
            {
                return tag.attr("ContentType").map(|c| c.into_owned());
            }
            "Default"
                if default.is_none()
                    && ext.is_some_and(|e| {
                        tag.attr("Extension")
                            .is_some_and(|x| x.eq_ignore_ascii_case(e))
                    }) =>
            {
                default = tag.attr("ContentType").map(|c| c.into_owned());
            }
            _ => {}
        }
    }
    default
}

/// An `Id` not yet used in a relationships part: `rId` and the next
/// number after the highest.
pub fn next_id(xml: &str) -> String {
    let n = parse(xml)
        .iter()
        .filter_map(|r| r.id.strip_prefix("rId").and_then(|n| n.parse::<u64>().ok()))
        .max()
        .unwrap_or(0);
    format!("rId{}", n + 1)
}

/// A relationships part with one more relationship, written before its
/// closing tag with the prefix the part uses; its `Id`. `rels` empty
/// makes the part.
pub fn add(rels: &str, ty: &str, target: &str, external: bool) -> (String, String) {
    const EMPTY: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>";
    let rels = if rels.trim().is_empty() { EMPTY } else { rels };
    let id = next_id(rels);
    let mode = if external {
        " TargetMode=\"External\""
    } else {
        ""
    };
    let (at, prefix) = closing(rels, "Relationships");
    let rel = format!(
        "<{prefix}Relationship Id=\"{id}\" Type=\"{}\" Target=\"{}\"{mode}/>",
        crate::xml::escape(ty),
        crate::xml::escape(target)
    );
    (format!("{}{rel}{}", &rels[..at], &rels[at..]), id)
}

/// `[Content_Types].xml` with an `Override` for `/part`, unless one is
/// there.
pub fn add_override(types: &str, part: &str, content_type: &str) -> String {
    let want = format!("/{part}");
    let mut r = Reader::new(types);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "Override"
            && tag
                .attr("PartName")
                .is_some_and(|p| p.eq_ignore_ascii_case(&want))
        {
            return types.to_owned();
        }
    }
    let (at, prefix) = closing(types, "Types");
    format!(
        "{}<{prefix}Override PartName=\"{}\" ContentType=\"{}\"/>{}",
        &types[..at],
        crate::xml::escape(&want),
        crate::xml::escape(content_type),
        &types[at..]
    )
}

/// `[Content_Types].xml` with a `Default` for the extension, unless one
/// is there.
pub fn add_default(types: &str, extension: &str, content_type: &str) -> String {
    let mut r = Reader::new(types);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "Default"
            && tag
                .attr("Extension")
                .is_some_and(|x| x.eq_ignore_ascii_case(extension))
        {
            return types.to_owned();
        }
    }
    let (at, prefix) = closing(types, "Types");
    format!(
        "{}<{prefix}Default Extension=\"{}\" ContentType=\"{}\"/>{}",
        &types[..at],
        crate::xml::escape(extension),
        crate::xml::escape(content_type),
        &types[at..]
    )
}

/// Where the root element `name` closes, and the prefix it is written
/// with (`x:`, or empty).
fn closing(xml: &str, name: &str) -> (usize, String) {
    let mut r = Reader::new(xml);
    let mut prefix = String::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == name => {
                prefix = crate::xml::prefix(tag.qname).to_owned();
                if tag.empty {
                    // `<Types/>`: opened up so that children fit.
                    return (tag.span.start, prefix);
                }
            }
            Token::End { name: n, span } if n == name => return (span.start, prefix),
            _ => {}
        }
    }
    (xml.len(), prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(rels_path("xl/workbook.xml"), "xl/_rels/workbook.xml.rels");
        assert_eq!(
            rels_path("[Content_Types].xml"),
            "_rels/[Content_Types].xml.rels"
        );
        assert_eq!(
            resolve("xl/workbook.xml", "worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            resolve("xl/worksheets/sheet1.xml", "../comments1.xml"),
            "xl/comments1.xml"
        );
        assert_eq!(
            resolve("xl/workbook.xml", "/xl/styles.xml"),
            "xl/styles.xml"
        );
        assert_eq!(resolve("", "xl/workbook.xml"), "xl/workbook.xml");
    }

    #[test]
    fn parse_and_remove() {
        let xml = r#"<Relationships><Relationship Id="rId1" Type="http://x/worksheet" Target="a.xml"/><Relationship Id="rId2" Type="http://x/calcChain" Target="calcChain.xml"/></Relationships>"#;
        let rels = parse(xml);
        assert_eq!(rels[1].kind, "calcChain");
        let out = remove(xml, "rId2");
        assert_eq!(parse(&out).len(), 1);
        assert!(out.ends_with("/></Relationships>"));
    }

    #[test]
    fn content_types_and_additions() {
        let types = r#"<Types xmlns="x"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="main"/></Types>"#;
        assert_eq!(
            content_type(types, "word/document.xml").as_deref(),
            Some("main")
        );
        assert_eq!(
            content_type(types, "word/Styles.XML").as_deref(),
            Some("application/xml")
        );
        assert_eq!(content_type(types, "media/a.png"), None);
        let more = add_override(types, "word/comments.xml", "c");
        assert_eq!(
            content_type(&more, "word/comments.xml").as_deref(),
            Some("c")
        );
        assert_eq!(add_override(&more, "word/comments.xml", "c"), more);
        let png = add_default(&more, "png", "image/png");
        assert_eq!(
            content_type(&png, "word/media/image1.png").as_deref(),
            Some("image/png")
        );
        let rels = r#"<Relationships xmlns="x"><Relationship Id="rId7" Type="t/a" Target="a.xml"/></Relationships>"#;
        let (out, id) = add(rels, "t/comments", "comments.xml", false);
        assert_eq!(id, "rId8");
        assert_eq!(parse(&out)[1].kind, "comments");
        let (fresh, id) = add("", "t/image", "https://example.com/a&b", true);
        assert_eq!(id, "rId1");
        assert!(parse(&fresh)[0].external);
        assert_eq!(parse(&fresh)[0].target, "https://example.com/a&b");
    }
}
