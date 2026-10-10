//! A cell's note added, edited and deleted as Excel writes it: a
//! `<comment>` in the sheet's comments part, and the note's hidden shape
//! in the sheet's legacy VML drawing, which Excel needs to show it.

use super::*;

const COMMENTS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml";
const VML_TYPE: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";
const VML_KIND: &str = "vmlDrawing";

/// The VML drawing a new one starts as: Excel's shape type for notes.
const VML_HEAD: &str = r##"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel"><o:shapelayout v:ext="edit"><o:idmap v:ext="edit" data="1"/></o:shapelayout><v:shapetype id="_x0000_t202" coordsize="21600,21600" o:spt="202" path="m,l,21600r21600,l21600,xe"><v:stroke joinstyle="miter"/><v:path gradientshapeok="t" o:connecttype="rect"/></v:shapetype></xml>"##;

/// The note's shape: hidden, beside the cell, as Excel makes it.
fn note_shape(id: u32, at: CellRef) -> String {
    let (r, c) = (at.row, at.col);
    format!(
        r##"<v:shape id="_x0000_s{id}" type="#_x0000_t202" style="position:absolute;margin-left:59.25pt;margin-top:1.5pt;width:108pt;height:59.25pt;z-index:1;visibility:hidden" fillcolor="#ffffe1" o:insetmode="auto"><v:fill color2="#ffffe1"/><v:shadow on="t" color="black" obscured="t"/><v:path o:connecttype="none"/><v:textbox style="mso-direction-alt:auto"><div style="text-align:left"></div></v:textbox><x:ClientData ObjectType="Note"><x:MoveWithCells/><x:SizeWithCells/><x:Anchor>{}, 15, {}, 2, {}, 15, {}, 16</x:Anchor><x:AutoFill>False</x:AutoFill><x:Row>{r}</x:Row><x:Column>{c}</x:Column></x:ClientData></v:shape>"##,
        c + 1,
        r.saturating_sub(1),
        c + 3,
        r + 3
    )
}

/// Where a `<comment>` or note shape for `at` lies in a part.
fn find_element(text: &str, name: &str, at: CellRef) -> Option<std::ops::Range<usize>> {
    let mut r = Reader::new(text);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != name {
            continue;
        }
        let start = tag.span.start;
        let is = match name {
            "comment" => tag.attr("ref").and_then(|v| CellRef::parse(&v)) == Some(at),
            _ => false,
        };
        let end = if tag.empty {
            tag.span.end
        } else {
            r.skip_element()
        };
        if is {
            return Some(start..end);
        }
        if name == "shape" {
            let el = &text[start..end];
            let num = |k: &str| {
                el.split(&format!("<x:{k}>"))
                    .nth(1)
                    .and_then(|v| v.split('<').next())
                    .and_then(|v| v.trim().parse::<u32>().ok())
            };
            if el.contains("ObjectType=\"Note\"")
                && num("Row") == Some(at.row)
                && num("Column") == Some(at.col)
            {
                return Some(start..end);
            }
        }
    }
    None
}

impl Workbook {
    /// The part of the sheet's relationship of `kind`, if it has one.
    fn sheet_part_of(&self, idx: usize, kind: &str) -> Result<Option<String>> {
        Ok(self
            .sheet_parts(idx)?
            .into_iter()
            .find(|(k, _)| k == kind)
            .map(|(_, p)| p))
    }

    /// Gives cell `at` a note (`Some`), or takes its note away, as one
    /// undo step.
    pub fn set_comment(&mut self, idx: usize, at: CellRef, text: Option<&str>) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let has = self.comments(idx)?.iter().any(|c| c.cell == at);
        if text.is_none() && !has {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let result = self.set_comment_now(idx, at, text);
        match (result, snapshot) {
            (Ok(()), Some(s)) => {
                self.push_undo(s);
                self.redo.clear();
                Ok(())
            }
            (Ok(()), None) => {
                self.batch_changed = true;
                Ok(())
            }
            (Err(e), Some(s)) => {
                self.restore(s);
                Err(e)
            }
            (Err(e), None) => Err(e),
        }
    }

    pub(crate) fn set_comment_now(
        &mut self,
        idx: usize,
        at: CellRef,
        text: Option<&str>,
    ) -> Result<()> {
        let sheet_part = self.sheets[idx].part.clone();
        // The comments part.
        let comments = match self.sheet_part_of(idx, kind::COMMENTS)? {
            Some(p) => p,
            None => {
                let part = self.free_part("xl/comments");
                let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
                self.add_part(
                    &part,
                    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<comments xmlns=\"{main}\"><authors><author>Kalem</author></authors><commentList/></comments>"),
                    COMMENTS_TYPE,
                )?;
                self.add_rel(&sheet_part, kind::COMMENTS, &part)?;
                part
            }
        };
        let ctext = text_of(self.pkg.part(&comments)?, &comments)?;
        let p = {
            let mut r = Reader::new(&ctext);
            match r.next_token() {
                Some(Token::Start(tag)) => xml::prefix(tag.qname).to_owned(),
                _ => String::new(),
            }
        };
        let new_comments = match text {
            Some(body) => {
                let el = format!(
                    "<{p}comment ref=\"{at}\" authorId=\"0\"><{p}text><{p}t xml:space=\"preserve\">{}</{p}t></{p}text></{p}comment>",
                    xml::escape(body)
                );
                match find_element(&ctext, "comment", at) {
                    Some(span) => {
                        // The author kept.
                        let old = &ctext[span.clone()];
                        let author = old
                            .split("authorId=\"")
                            .nth(1)
                            .and_then(|v| v.split('"').next())
                            .unwrap_or("0");
                        splice(
                            &ctext,
                            vec![(
                                span,
                                el.replacen("authorId=\"0\"", &format!("authorId=\"{author}\""), 1),
                            )],
                        )
                    }
                    None => {
                        // An author for it when the part has none.
                        let t = if ctext.contains("<author>")
                            || ctext.contains(&format!("<{p}author>"))
                        {
                            ctext.clone()
                        } else {
                            insert_into(
                                &ctext,
                                "authors",
                                &format!("<{p}author>Kalem</{p}author>"),
                                &p,
                            )
                        };
                        insert_into(&t, "commentList", &el, &p)
                    }
                }
            }
            None => match find_element(&ctext, "comment", at) {
                Some(span) => splice(&ctext, vec![(span, String::new())]),
                None => ctext.clone(),
            },
        };
        self.pkg.set_part(&comments, new_comments.into_bytes());
        // The note's shape in the VML drawing.
        let vml = match self.sheet_part_of(idx, VML_KIND)? {
            Some(p) => p,
            None if text.is_some() => {
                let part = self.free_part_ext("xl/drawings/vmlDrawing", "vml");
                self.pkg.set_part(&part, VML_HEAD.as_bytes().to_vec());
                self.ensure_default_type("vml", VML_TYPE)?;
                let rid = self.add_rel(&sheet_part, VML_KIND, &part)?;
                self.add_legacy_drawing(idx, &rid)?;
                part
            }
            None => return Ok(()),
        };
        let vtext = text_of(self.pkg.part(&vml)?, &vml)?;
        let new_vml = match (text, find_element(&vtext, "shape", at)) {
            (Some(_), Some(_)) => vtext.clone(),
            (Some(_), None) => {
                let next = vtext
                    .split("id=\"_x0000_s")
                    .skip(1)
                    .filter_map(|v| v.split('"').next()?.parse::<u32>().ok())
                    .max()
                    .unwrap_or(1024)
                    + 1;
                let close = vtext.rfind("</xml>").unwrap_or(vtext.len());
                splice(&vtext, vec![(close..close, note_shape(next, at))])
            }
            (None, Some(span)) => splice(&vtext, vec![(span, String::new())]),
            (None, None) => vtext.clone(),
        };
        self.pkg.set_part(&vml, new_vml.into_bytes());
        Ok(())
    }

    /// Cell `at`'s note given author `name` (added to the authors).
    pub(crate) fn set_note_author(&mut self, idx: usize, at: CellRef, name: &str) -> Result<()> {
        let Some(part) = self.sheet_part_of(idx, kind::COMMENTS)? else {
            return Ok(());
        };
        let text = text_of(self.pkg.part(&part)?, &part)?;
        let p = {
            let mut r = Reader::new(&text);
            match r.next_token() {
                Some(Token::Start(tag)) => xml::prefix(tag.qname).to_owned(),
                _ => String::new(),
            }
        };
        let mut authors = Vec::new();
        let mut r = Reader::new(&text);
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "author"
                && !tag.empty
            {
                authors.push(r.text_until_end("author").0);
            }
        }
        let (text, n) = match authors.iter().position(|a| a == name) {
            Some(n) => (text, n),
            None => (
                insert_into(
                    &text,
                    "authors",
                    &format!("<{p}author>{}</{p}author>", xml::escape(name)),
                    &p,
                ),
                authors.len(),
            ),
        };
        let Some(span) = find_element(&text, "comment", at) else {
            return Ok(());
        };
        let open_end = text[span.start..]
            .find('>')
            .map_or(span.end, |e| span.start + e + 1);
        let tag = xml::set_attr(&text[span.start..open_end], "authorId", &n.to_string());
        let new = splice(&text, vec![(span.start..open_end, tag)]);
        self.pkg.set_part(&part, new.into_bytes());
        Ok(())
    }

    /// A part name not in the package: `{stem}{n}.{ext}`.
    pub(crate) fn free_part_ext(&self, stem: &str, ext: &str) -> String {
        (1..)
            .map(|n| format!("{stem}{n}.{ext}"))
            .find(|p| !self.pkg.contains(p))
            .expect("a free name")
    }

    /// A content type for files ending in `.ext`, when the package has none.
    pub(crate) fn ensure_default_type(&mut self, ext: &str, content_type: &str) -> Result<()> {
        let ct = "[Content_Types].xml";
        let types = text_of(self.pkg.part(ct)?, ct)?;
        if types.contains(&format!("Extension=\"{ext}\"")) {
            return Ok(());
        }
        let Some(open_end) = types
            .find("<Types")
            .and_then(|s| types[s..].find('>').map(|e| s + e + 1))
        else {
            return Err(Error::Refused("the content types have no <Types>".into()));
        };
        let item = format!("<Default Extension=\"{ext}\" ContentType=\"{content_type}\"/>");
        self.pkg.set_part(
            ct,
            splice(&types, vec![(open_end..open_end, item)]).into_bytes(),
        );
        Ok(())
    }

    /// The sheet's `<legacyDrawing>` naming relationship `rid`, where the
    /// schema puts it.
    fn add_legacy_drawing(&mut self, idx: usize, rid: &str) -> Result<()> {
        let text = self.loaded[&idx].0.clone();
        let p = self.loaded[&idx].1.prefix.clone();
        let ns = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let head_end = text.find("<sheetData").unwrap_or(text.len());
        let rp = text[..head_end].split("xmlns:").skip(1).find_map(|d| {
            let (name, rest) = d.split_once("=\"")?;
            rest.starts_with(ns).then(|| name.to_owned())
        });
        let el = match rp {
            Some(r) => format!("<{p}legacyDrawing {r}:id=\"{rid}\"/>"),
            None => format!("<{p}legacyDrawing xmlns:r=\"{ns}\" r:id=\"{rid}\"/>"),
        };
        let new = insert_top_level(
            &text,
            &[
                "legacyDrawingHF",
                "drawingHF",
                "picture",
                "oleObjects",
                "controls",
                "webPublishItems",
                "tableParts",
                "extLst",
            ],
            &el,
        );
        let model = sheet::parse(&new, &self.strings, self.date1904);
        self.loaded.insert(idx, Arc::new((new, model)));
        self.generation += 1;
        if !self.dirty_sheets.contains(&idx) {
            self.dirty_sheets.push(idx);
        }
        Ok(())
    }
}

/// A part with `el` put at the end of its element `list` (made out of an
/// empty `<list/>`).
fn insert_into(text: &str, list: &str, el: &str, p: &str) -> String {
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
