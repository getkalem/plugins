//! Relationships and content types (ECMA-376 part 2, 9.3 and 10.1).

use crate::xml::{Reader, Token};

/// The relationship types this plugin follows.
pub mod kind {
    /// The workbook part from the package root.
    pub const OFFICE_DOCUMENT: &str = "officeDocument";
    /// A worksheet from the workbook.
    pub const WORKSHEET: &str = "worksheet";
    /// A chart sheet.
    pub const CHARTSHEET: &str = "chartsheet";
    /// A dialog sheet (Excel 5).
    pub const DIALOGSHEET: &str = "dialogsheet";
    /// An Excel 4 macro sheet.
    pub const MACROSHEET: &str = "macrosheet";
    /// The shared string table.
    pub const SHARED_STRINGS: &str = "sharedStrings";
    /// The style sheet.
    pub const STYLES: &str = "styles";
    /// The calculation chain.
    pub const CALC_CHAIN: &str = "calcChain";
    /// Cell comments of a sheet.
    pub const COMMENTS: &str = "comments";
    /// The VBA project.
    pub const VBA_PROJECT: &str = "vbaProject";
}

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
}
