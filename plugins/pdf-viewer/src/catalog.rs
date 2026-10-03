//! What the document catalog says about the pages (ISO 32000-2, 7.7.2):
//! destinations, the outline (12.3.3), page labels (12.4.2) and link
//! annotations (12.5.6.5). Every walk is bounded and remembers the objects
//! it visited, so a malformed file with a cycle ends.

use std::collections::{HashMap, HashSet};

use hayro::hayro_interpret::util::{RectExt, TransformExt};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Object, ObjectIdentifier, Rect};
use kalem_viewer::{Link, OutlineEntry};
use kurbo::Shape;

use crate::strings;

/// The most entries an outline, a name tree or a number tree is read to.
const MAX_ENTRIES: usize = 100_000;
/// The deepest a tree is followed.
const MAX_DEPTH: usize = 64;

/// The catalog's view of a document's pages.
pub(crate) struct Catalog {
    /// The page objects' identifiers, by page index.
    pages: HashMap<ObjectIdentifier, usize>,
    /// The named destinations (the catalog's `Dests` and the `Dests`
    /// name tree), resolved to pages.
    named: HashMap<Vec<u8>, usize>,
    /// The page count.
    count: usize,
}

impl Catalog {
    pub(crate) fn new(pdf: &Pdf) -> Catalog {
        let pages: HashMap<ObjectIdentifier, usize> = pdf
            .pages()
            .iter()
            .enumerate()
            .filter_map(|(i, p)| Some((p.raw().obj_id()?, i)))
            .collect();
        let mut catalog = Catalog {
            pages,
            named: HashMap::new(),
            count: pdf.pages().len(),
        };
        let Some(root) = root(pdf) else {
            return catalog;
        };
        let mut named = HashMap::new();
        if let Some(dests) = root.get::<Dict<'_>>(b"Dests") {
            for (name, value) in dests.entries().take(MAX_ENTRIES) {
                if let Some(value) = resolve(pdf, value)
                    && let Some(page) = catalog.dest_page(&value, 0)
                {
                    named.insert(name.as_ref().to_vec(), page);
                }
            }
        }
        if let Some(tree) = root
            .get::<Dict<'_>>(b"Names")
            .and_then(|n| n.get::<Dict<'_>>(b"Dests"))
        {
            let mut entries = Vec::new();
            name_tree(&tree, &mut entries, &mut HashSet::new(), 0);
            for (name, value) in entries {
                if let Some(page) = catalog.dest_page(&value, 0) {
                    named.insert(name, page);
                }
            }
        }
        catalog.named = named;
        catalog
    }

    /// The page a destination goes to: an explicit destination (an array
    /// whose first element is a page or, for a remote file, its number), a
    /// name or a string naming one, or a dictionary holding one as `D`.
    pub(crate) fn dest_page(&self, dest: &Object<'_>, depth: usize) -> Option<usize> {
        if depth > 4 {
            return None;
        }
        match dest {
            Object::Array(a) => match a.raw_iter().next()? {
                MaybeRef::Ref(r) => self.pages.get(&r.into()).copied(),
                MaybeRef::NotRef(Object::Number(n)) => {
                    usize::try_from(n.as_i64()).ok().filter(|&i| i < self.count)
                }
                MaybeRef::NotRef(Object::Dict(d)) => {
                    d.obj_id().and_then(|id| self.pages.get(&id).copied())
                }
                MaybeRef::NotRef(_) => None,
            },
            Object::Name(n) => self.named.get(n.as_ref()).copied(),
            Object::String(s) => self.named.get(s.as_bytes()).copied(),
            Object::Dict(d) => self.dest_page(&d.get::<Object<'_>>(b"D")?, depth + 1),
            _ => None,
        }
    }

    /// Where an action goes: a page for `GoTo`, a URL for `URI`, a file
    /// for `GoToR` and `Launch`.
    fn action_target(&self, action: &Dict<'_>) -> Option<String> {
        let kind = action.get::<hayro::hayro_syntax::object::Name<'_>>(b"S")?;
        match kind.as_str() {
            "GoTo" => Some(unit_target(
                self.dest_page(&action.get::<Object<'_>>(b"D")?, 0)?,
            )),
            "URI" => Some(strings::decode(
                action
                    .get::<hayro::hayro_syntax::object::String<'_>>(b"URI")?
                    .as_bytes(),
            )),
            "GoToR" | "Launch" => file_spec(&action.get::<Object<'_>>(b"F")?),
            _ => None,
        }
    }

    /// The outline, its entries in order with their levels. An entry
    /// without a destination points where the one before it does.
    pub(crate) fn outline(&self, pdf: &Pdf) -> Vec<OutlineEntry> {
        let mut out = Vec::new();
        let Some(first) = root(pdf)
            .and_then(|r| r.get::<Dict<'_>>(b"Outlines"))
            .and_then(|o| o.get::<Dict<'_>>(b"First"))
        else {
            return out;
        };
        let mut seen = HashSet::new();
        self.outline_level(first, 1, &mut out, &mut seen);
        out
    }

    fn outline_level(
        &self,
        first: Dict<'_>,
        level: usize,
        out: &mut Vec<OutlineEntry>,
        seen: &mut HashSet<ObjectIdentifier>,
    ) {
        let mut node = Some(first);
        while let Some(item) = node {
            if out.len() >= MAX_ENTRIES || level > MAX_DEPTH {
                return;
            }
            if let Some(id) = item.obj_id()
                && !seen.insert(id)
            {
                return;
            }
            let unit = item
                .get::<Object<'_>>(b"Dest")
                .and_then(|d| self.dest_page(&d, 0))
                .or_else(|| {
                    let a = item.get::<Dict<'_>>(b"A")?;
                    self.action_target(&a)?.strip_prefix('#')?.parse().ok()
                })
                .or_else(|| out.last().map(|e| e.unit))
                .unwrap_or(0);
            let title = item
                .get::<hayro::hayro_syntax::object::String<'_>>(b"Title")
                .map(|t| strings::decode(t.as_bytes()))
                .unwrap_or_default();
            out.push(OutlineEntry {
                title: title.split_whitespace().collect::<Vec<_>>().join(" "),
                unit,
                level: level.min(u8::MAX as usize) as u8,
            });
            if let Some(child) = item.get::<Dict<'_>>(b"First") {
                self.outline_level(child, level + 1, out, seen);
            }
            node = item.get::<Dict<'_>>(b"Next");
        }
    }

    /// The links of page `index`, their rectangles in the page's pixels at
    /// scale 1 as it is rendered (cropped, turned by `Rotate`, y down).
    pub(crate) fn links(&self, pdf: &Pdf, index: usize) -> Vec<Link> {
        let Some(page) = pdf.pages().get(index) else {
            return Vec::new();
        };
        let Some(annots) = page.raw().get::<Array<'_>>(b"Annots") else {
            return Vec::new();
        };
        let to_page = page.initial_transform(true).to_kurbo();
        let mut links = Vec::new();
        for annot in annots.iter::<Dict<'_>>().take(MAX_ENTRIES) {
            let is_link = annot
                .get::<hayro::hayro_syntax::object::Name<'_>>(b"Subtype")
                .is_some_and(|s| s.as_str() == "Link");
            if !is_link {
                continue;
            }
            let Some(rect) = annot.get::<Rect>(b"Rect") else {
                continue;
            };
            let target = annot
                .get::<Dict<'_>>(b"A")
                .and_then(|a| self.action_target(&a))
                .or_else(|| {
                    let dest = annot.get::<Object<'_>>(b"Dest")?;
                    Some(unit_target(self.dest_page(&dest, 0)?))
                });
            let Some(target) = target else {
                continue;
            };
            let b = (to_page * rect.to_kurbo().to_path(0.1)).bounding_box();
            links.push(Link {
                rect: [
                    b.x0 as f32,
                    b.y0 as f32,
                    b.width() as f32,
                    b.height() as f32,
                ],
                target,
            });
        }
        links
    }

    /// Every page's label (12.4.2): from the `PageLabels` number tree, the
    /// page's number counted from 1 where there is none.
    pub(crate) fn labels(&self, pdf: &Pdf) -> Vec<String> {
        let mut ranges: Vec<(usize, Option<char>, String, i64)> = Vec::new();
        if let Some(tree) = root(pdf).and_then(|r| r.get::<Dict<'_>>(b"PageLabels")) {
            let mut entries = Vec::new();
            number_tree(&tree, &mut entries, &mut HashSet::new(), 0);
            for (start, label) in entries {
                let Some(label) = label.into_dict() else {
                    continue;
                };
                let style = label
                    .get::<hayro::hayro_syntax::object::Name<'_>>(b"S")
                    .and_then(|s| s.as_str().chars().next());
                let prefix = label
                    .get::<hayro::hayro_syntax::object::String<'_>>(b"P")
                    .map(|p| strings::decode(p.as_bytes()))
                    .unwrap_or_default();
                let first = label.get::<i64>(b"St").unwrap_or(1).max(1);
                if let Ok(start) = usize::try_from(start) {
                    ranges.push((start, style, prefix, first));
                }
            }
            ranges.sort_by_key(|r| r.0);
        }
        (0..self.count)
            .map(|i| {
                let Some((start, style, prefix, first)) = ranges.iter().rev().find(|r| r.0 <= i)
                else {
                    return (i + 1).to_string();
                };
                let n = first + (i - start) as i64;
                let number = match style {
                    Some('D') => n.to_string(),
                    Some('R') => roman(n).to_uppercase(),
                    Some('r') => roman(n),
                    Some('A') => letters(n).to_uppercase(),
                    Some('a') => letters(n),
                    _ => String::new(),
                };
                format!("{prefix}{number}")
            })
            .collect()
    }
}

/// The target of a link to a unit.
fn unit_target(unit: usize) -> String {
    format!("#{unit}")
}

/// A file specification's file name (7.11.2): a string, or a dictionary's
/// `UF` or `F`.
fn file_spec(spec: &Object<'_>) -> Option<String> {
    match spec {
        Object::String(s) => Some(strings::decode(s.as_bytes())),
        Object::Dict(d) => d
            .get::<hayro::hayro_syntax::object::String<'_>>(b"UF")
            .or_else(|| d.get::<hayro::hayro_syntax::object::String<'_>>(b"F"))
            .map(|s| strings::decode(s.as_bytes())),
        _ => None,
    }
}

/// The document catalog.
fn root(pdf: &Pdf) -> Option<Dict<'_>> {
    pdf.xref().get::<Dict<'_>>(pdf.xref().root_id())
}

fn resolve<'a>(pdf: &'a Pdf, value: MaybeRef<Object<'a>>) -> Option<Object<'a>> {
    match value {
        MaybeRef::Ref(r) => pdf.xref().get::<Object<'a>>(r.into()),
        MaybeRef::NotRef(o) => Some(o),
    }
}

/// The entries of a name tree (7.9.6), keys and values.
fn name_tree<'a>(
    node: &Dict<'a>,
    out: &mut Vec<(Vec<u8>, Object<'a>)>,
    seen: &mut HashSet<ObjectIdentifier>,
    depth: usize,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_ENTRIES {
        return;
    }
    if let Some(id) = node.obj_id()
        && !seen.insert(id)
    {
        return;
    }
    if let Some(names) = node.get::<Array<'_>>(b"Names") {
        let mut items = names.iter::<Object<'_>>();
        while let (Some(key), Some(value)) = (items.next(), items.next()) {
            if let Object::String(key) = key {
                out.push((key.as_bytes().to_vec(), value));
            }
        }
    }
    if let Some(kids) = node.get::<Array<'_>>(b"Kids") {
        for kid in kids.iter::<Dict<'_>>() {
            name_tree(&kid, out, seen, depth + 1);
        }
    }
}

/// The entries of a number tree (7.9.7), keys and values.
fn number_tree<'a>(
    node: &Dict<'a>,
    out: &mut Vec<(i64, Object<'a>)>,
    seen: &mut HashSet<ObjectIdentifier>,
    depth: usize,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_ENTRIES {
        return;
    }
    if let Some(id) = node.obj_id()
        && !seen.insert(id)
    {
        return;
    }
    if let Some(nums) = node.get::<Array<'_>>(b"Nums") {
        let mut items = nums.iter::<Object<'_>>();
        while let (Some(key), Some(value)) = (items.next(), items.next()) {
            if let Object::Number(key) = key {
                out.push((key.as_i64(), value));
            }
        }
    }
    if let Some(kids) = node.get::<Array<'_>>(b"Kids") {
        for kid in kids.iter::<Dict<'_>>() {
            number_tree(&kid, out, seen, depth + 1);
        }
    }
}

/// `n` in lower-case Roman numerals; beyond 4999, the number itself.
fn roman(n: i64) -> String {
    if !(1..5000).contains(&n) {
        return n.to_string();
    }
    const DIGITS: [(i64, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut n = n;
    let mut out = String::new();
    for (value, digits) in DIGITS {
        while n >= value {
            out.push_str(digits);
            n -= value;
        }
    }
    out
}

/// `n` in lower-case letters as page labels count: a to z, then aa to
/// zz, then aaa (12.4.2, Table 161).
fn letters(n: i64) -> String {
    if n < 1 {
        return n.to_string();
    }
    let n = (n - 1) as usize;
    let letter = (b'a' + (n % 26) as u8) as char;
    std::iter::repeat_n(letter, n / 26 + 1).collect()
}

#[cfg(test)]
mod tests {
    use super::{letters, roman};

    #[test]
    fn numbering_styles() {
        assert_eq!(roman(1), "i");
        assert_eq!(roman(4), "iv");
        assert_eq!(roman(1994), "mcmxciv");
        assert_eq!(letters(1), "a");
        assert_eq!(letters(26), "z");
        assert_eq!(letters(27), "aa");
        assert_eq!(letters(53), "aaa");
    }
}
