//! A cell's hyperlink read, added and removed as Excel writes it: a
//! `<hyperlink>` in the sheet's `<hyperlinks>`, with a `location` in the
//! workbook or an external relationship of the sheet for an address, and
//! the cell in the blue underlined look of a link.

use super::*;

/// What Excel's Hyperlink style draws: blue, underlined.
const LINK_BLUE: [u8; 3] = [0x05, 0x63, 0xC1];

/// A `<hyperlink>` of a sheet part: its range, where its element lies,
/// its relationship and its place in the workbook.
struct Link {
    range: Range,
    span: std::ops::Range<usize>,
    rid: Option<String>,
    location: Option<String>,
}

fn links_of(text: &str) -> Vec<Link> {
    let mut r = Reader::new(text);
    let mut out = Vec::new();
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != "hyperlink" {
            continue;
        }
        let start = tag.span.start;
        let end = if tag.empty {
            tag.span.end
        } else {
            r.skip_element()
        };
        let Some(range) = tag.attr("ref").and_then(|v| {
            Range::parse(&v).or_else(|| CellRef::parse(&v).map(|c| Range { start: c, end: c }))
        }) else {
            continue;
        };
        out.push(Link {
            range,
            span: start..end,
            rid: tag.attr("id").map(|v| v.into_owned()),
            location: tag.attr("location").map(|v| v.into_owned()),
        });
    }
    out
}

impl Workbook {
    /// A cell's hyperlink: its address or file as written, or `#` and its
    /// place in the workbook.
    pub fn link(&mut self, idx: usize, at: CellRef) -> Result<Option<String>> {
        self.load(idx)?;
        let text = &self.loaded[&idx].0;
        let Some(link) = links_of(text).into_iter().find(|l| l.range.contains(at)) else {
            return Ok(None);
        };
        if let Some(rid) = &link.rid {
            let rels_path = rels::rels_path(&self.sheets[idx].part);
            if self.pkg.contains(&rels_path) {
                let rels = rels::parse(&text_of(self.pkg.part(&rels_path)?, &rels_path)?);
                if let Some(r) = rels.iter().find(|r| &r.id == rid) {
                    let mut t = r.target.clone();
                    if let Some(loc) = &link.location {
                        t = format!("{t}#{loc}");
                    }
                    return Ok(Some(t));
                }
            }
        }
        Ok(link.location.map(|l| format!("#{l}")))
    }

    /// Gives cell `at` a hyperlink (`#` and a place in the workbook, or an
    /// address), or takes its link away, as one undo step: the cell drawn
    /// as a link, given the address as its text when it has none.
    pub fn set_link(&mut self, idx: usize, at: CellRef, target: Option<&str>) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if let Some(t) = target
            && t.trim().is_empty()
        {
            return Err(Error::Refused("A link needs an address".into()));
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = self.set_link_now(idx, at, target);
        self.batch_changed = true;
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    fn set_link_now(&mut self, idx: usize, at: CellRef, target: Option<&str>) -> Result<()> {
        let sheet_part = self.sheets[idx].part.clone();
        let mut text = self.loaded[&idx].0.clone();
        let p = self.loaded[&idx].1.prefix.clone();
        // The cell's old link out, and its relationship.
        let had = links_of(&text).into_iter().find(|l| l.range.contains(at));
        if let Some(old) = &had {
            text = splice(&text, vec![(old.span.clone(), String::new())]);
            if let Some(rid) = &old.rid {
                self.remove_sheet_rel(&sheet_part, rid)?;
            }
        }
        if let Some(target) = target {
            let element = match target.strip_prefix('#') {
                Some(place) => format!(
                    "<{p}hyperlink ref=\"{at}\" location=\"{}\" display=\"{}\"/>",
                    xml::escape(place),
                    xml::escape(place)
                ),
                None => {
                    let rid = self.add_external_rel(&sheet_part, "hyperlink", target)?;
                    let (rp, decl) = sheet_r_prefix(&text);
                    format!("<{p}hyperlink ref=\"{at}\"{decl} {rp}:id=\"{rid}\"/>")
                }
            };
            text = match text.find(&format!("<{p}hyperlinks")) {
                Some(_) => insert_into_list(&text, "hyperlinks", &element, &p),
                None => insert_top_level(
                    &text,
                    &AFTER_MERGE_CELLS[4..],
                    &format!("<{p}hyperlinks>{element}</{p}hyperlinks>"),
                ),
            };
        } else if had.is_none() {
            return Ok(());
        }
        // An empty `<hyperlinks/>` left would not be valid.
        for empty in [
            format!("<{p}hyperlinks></{p}hyperlinks>"),
            format!("<{p}hyperlinks/>"),
        ] {
            text = text.replace(&empty, "");
        }
        let model = sheet::parse(&text, &self.strings, self.date1904);
        self.loaded.insert(idx, (text, model));
        self.generation += 1;
        if !self.dirty_sheets.contains(&idx) {
            self.dirty_sheets.push(idx);
        }
        // The cell drawn as a link, and an empty one given the address.
        let range = Range { start: at, end: at };
        let change = kalem_viewer::StyleChange {
            underline: Some(target.is_some()),
            color: Some(target.map(|_| LINK_BLUE)),
            ..kalem_viewer::StyleChange::default()
        };
        self.change_style(idx, range, &change)?;
        if let Some(t) = target
            && self.display(idx, at)?.is_empty()
        {
            let shown = t.strip_prefix('#').unwrap_or(t).to_owned();
            self.set_input(idx, at, Input::Text(shown))?;
        }
        Ok(())
    }

    /// A relationship of a sheet to an address outside the package.
    fn add_external_rel(&mut self, source: &str, kind: &str, target: &str) -> Result<String> {
        let rels_path = rels::rels_path(source);
        let text = if self.pkg.contains(&rels_path) {
            text_of(self.pkg.part(&rels_path)?, &rels_path)?
        } else {
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_string()
        };
        let used: Vec<String> = rels::parse(&text).into_iter().map(|r| r.id).collect();
        let id = (1..)
            .map(|n| format!("rId{n}"))
            .find(|i| !used.contains(i))
            .expect("a free id");
        let item = format!(
            "<Relationship Id=\"{id}\" Type=\"{}\" Target=\"{}\" TargetMode=\"External\"/>",
            self.rel_type(kind),
            xml::escape(target)
        );
        let close = text.rfind("</").unwrap_or(text.len());
        let new = format!("{}{item}{}", &text[..close], &text[close..]);
        self.pkg.set_part(&rels_path, new.into_bytes());
        Ok(id)
    }

    /// A relationship of a sheet taken away.
    fn remove_sheet_rel(&mut self, source: &str, rid: &str) -> Result<()> {
        let path = rels::rels_path(source);
        if !self.pkg.contains(&path) {
            return Ok(());
        }
        let text = text_of(self.pkg.part(&path)?, &path)?;
        let mut r = Reader::new(&text);
        let mut splices = Vec::new();
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "Relationship"
                && tag.attr("Id").as_deref() == Some(rid)
            {
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                splices.push((tag.span.start..end, String::new()));
            }
        }
        self.pkg
            .set_part(&path, splice(&text, splices).into_bytes());
        Ok(())
    }
}

/// The prefix a sheet part gives the relationships namespace, or the
/// declaration an element needs when it has none.
pub(super) fn sheet_r_prefix(text: &str) -> (String, String) {
    const NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let head_end = text.find("<sheetData").unwrap_or(text.len());
    text[..head_end]
        .split("xmlns:")
        .skip(1)
        .find_map(|d| {
            let (name, rest) = d.split_once("=\"")?;
            rest.starts_with(NS)
                .then(|| (name.to_owned(), String::new()))
        })
        .unwrap_or_else(|| ("r".into(), format!(" xmlns:r=\"{NS}\"")))
}

/// A part with `el` put at the end of its element `list`.
fn insert_into_list(text: &str, list: &str, el: &str, p: &str) -> String {
    let mut r = Reader::new(text);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != list {
            continue;
        }
        if tag.empty {
            return splice(
                text,
                vec![(tag.span.clone(), format!("<{p}{list}>{el}</{p}{list}>"))],
            );
        }
        let end = r.skip_element();
        let close = text[..end].rfind("</").unwrap_or(end);
        return splice(text, vec![(close..close, el.to_owned())]);
    }
    text.to_owned()
}
