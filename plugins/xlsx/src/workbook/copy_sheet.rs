//! Move or Copy's copy: a sheet's part copied with every part it names
//! (drawings, charts, notes, comments, tables, printer settings; pictures
//! shared, pivot tables left behind as their values), its relationships
//! kept by their ids so the sheet's own references hold; tables named anew,
//! charts reading the copy, its local names (the print area) its own.

use super::*;

/// The path from part `from`'s folder to part `to`.
fn relative(from: &str, to: &str) -> String {
    let dir = from.rsplit_once('/').map_or("", |(d, _)| d);
    let a: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = to.split('/').collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out: Vec<&str> = vec![".."; a.len() - common];
    out.extend(&b[common..]);
    out.join("/")
}

impl Workbook {
    /// A part's content type, and whether it is its own (an `Override`).
    fn content_type(&self, part: &str) -> Result<Option<(String, bool)>> {
        let ct = "[Content_Types].xml";
        let types = text_of(self.pkg.part(ct)?, ct)?;
        let mut r = Reader::new(&types);
        let ext = part
            .rsplit_once('.')
            .map_or("", |(_, e)| e)
            .to_ascii_lowercase();
        let mut default = None;
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "Override" if tag.attr("PartName").as_deref() == Some(&format!("/{part}")) => {
                    return Ok(tag.attr("ContentType").map(|c| (c.into_owned(), true)));
                }
                "Default"
                    if tag
                        .attr("Extension")
                        .is_some_and(|e| e.eq_ignore_ascii_case(&ext)) =>
                {
                    default = tag.attr("ContentType").map(|c| (c.into_owned(), false));
                }
                _ => {}
            }
        }
        Ok(default)
    }

    /// A copy of `part` under a free name like it, with copies of what it
    /// names; tables named anew, charts reading sheet `new_name` where they
    /// read `old_name`. The copy's name.
    fn copy_part(&mut self, part: &str, old_name: &str, new_name: &str) -> Result<String> {
        let (stem, ext) = part.rsplit_once('.').unwrap_or((part, "xml"));
        let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit());
        let new = self.free_part_ext(stem, ext);
        let mut bytes = self.pkg.part(part)?;
        let ct = self.content_type(part)?;
        let ct_name = ct.as_ref().map_or("", |c| c.0.as_str());
        if ct_name.ends_with("table+xml") {
            // A table's name and id are the workbook's to keep unique.
            let tables = self.tables().unwrap_or_default();
            let text = text_of(bytes.clone(), part)?;
            let mut old = String::from("Table");
            let mut rd = Reader::new(&text);
            while let Some(t) = rd.next_token() {
                if let Token::Start(tag) = t
                    && tag.name == "table"
                {
                    if let Some(v) = tag.attr("displayName") {
                        old = v.into_owned();
                    }
                    break;
                }
            }
            let base = old.trim_end_matches(|c: char| c.is_ascii_digit());
            let name = (1..)
                .map(|n| format!("{base}{n}"))
                .find(|n| !tables.iter().any(|t| t.name.eq_ignore_ascii_case(n)))
                .expect("a free name");
            let id = tables.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            let open = text
                .find("<table")
                .and_then(|s| text[s..].find('>').map(|e| (s, s + e + 1)));
            if let Some((s, e)) = open {
                let mut tag = text[s..e].to_owned();
                tag = xml::set_attr(&tag, "id", &id.to_string());
                tag = xml::set_attr(&tag, "name", &name);
                tag = xml::set_attr(&tag, "displayName", &name);
                bytes = splice(&text, vec![(s..e, tag)]).into_bytes();
            }
        } else if ct_name.contains("drawingml.chart") {
            // The copy's charts read the copy.
            let text = text_of(bytes.clone(), part)?;
            let q = |n: &str| format!("{}!", super::sheet_ops::qualified(n));
            let quoted = |n: &str| format!("'{}'!", n.replace('\'', "''"));
            let text = text
                .replace(&xml::escape(&q(old_name)), &xml::escape(&q(new_name)))
                .replace(&xml::escape(&quoted(old_name)), &xml::escape(&q(new_name)));
            bytes = text.into_bytes();
        }
        self.pkg.set_part(&new, bytes);
        if let Some((c, true)) = &ct {
            let types_path = "[Content_Types].xml";
            let types = text_of(self.pkg.part(types_path)?, types_path)?;
            let item = format!("<Override PartName=\"/{new}\" ContentType=\"{c}\"/>");
            let close = types.rfind("</").unwrap_or(types.len());
            self.pkg.set_part(
                types_path,
                format!("{}{item}{}", &types[..close], &types[close..]).into_bytes(),
            );
        }
        let rels_path = rels::rels_path(part);
        if self.pkg.contains(&rels_path) {
            let text = text_of(self.pkg.part(&rels_path)?, &rels_path)?;
            let copied = self.copy_rels(part, &new, &text, old_name, new_name)?;
            self.pkg
                .set_part(&rels::rels_path(&new), copied.into_bytes());
        }
        Ok(new)
    }

    /// Part `from`'s relationships made `to`'s: their ids kept, the parts
    /// they name copied (pictures and other media shared), pivot tables
    /// left out.
    fn copy_rels(
        &mut self,
        from: &str,
        to: &str,
        text: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<String> {
        let mut r = Reader::new(text);
        let mut splices = Vec::new();
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            if tag.name != "Relationship" {
                continue;
            }
            let span = tag.span.clone();
            let el = text[span.clone()].to_owned();
            if tag.attr("TargetMode").as_deref() == Some("External") {
                continue;
            }
            let (Some(ty), Some(target)) = (tag.attr("Type"), tag.attr("Target")) else {
                continue;
            };
            let kind = ty.rsplit('/').next().unwrap_or_default().to_owned();
            let part = rels::resolve(from, &target);
            if kind.starts_with("pivotTable") || !self.pkg.contains(&part) {
                splices.push((span, String::new()));
                continue;
            }
            let shared = matches!(
                kind.as_str(),
                "image" | "hdphoto" | "video" | "audio" | "media"
            );
            let new = if shared {
                part
            } else {
                self.copy_part(&part, old_name, new_name)?
            };
            splices.push((span, xml::set_attr(&el, "Target", &relative(to, &new))));
        }
        Ok(splice(text, splices))
    }

    /// A copy of sheet `i` put at place `at`, named `Name (2)` (or the next
    /// free number): its text, the parts it names and its local names.
    pub(crate) fn copy_sheet(&mut self, i: usize, at: usize) -> Result<usize> {
        self.load(i)?;
        if self.sheets[i].kind != SheetKind::Worksheet {
            return Err(Error::Refused("Only worksheets are copied".into()));
        }
        let src = self.sheets[i].clone();
        let name = (2..)
            .map(|n| {
                let suffix = format!(" ({n})");
                let keep: String = src.name.chars().take(31 - suffix.chars().count()).collect();
                format!("{keep}{suffix}")
            })
            .find(|n| self.sheet_index(n).is_none())
            .expect("a free name");
        let new_idx = self.add_sheet("Sheet")?;
        let part = self.sheets[new_idx].part.clone();
        // The copy is not the tab shown.
        let mut text = self.loaded[&i].0.clone();
        if let Some(s) = text.find("<sheetView")
            && let Some(e) = text[s..].find('>')
        {
            let tag = xml::remove_attr(&text[s..s + e + 1], "tabSelected");
            text = splice(&text, vec![(s..s + e + 1, tag)]);
        }
        self.replace_sheet_text(new_idx, text);
        let rels_path = rels::rels_path(&src.part);
        if self.pkg.contains(&rels_path) {
            let rels_text = text_of(self.pkg.part(&rels_path)?, &rels_path)?;
            let copied = self.copy_rels(&src.part, &part, &rels_text, &src.name, &name)?;
            self.pkg
                .set_part(&rels::rels_path(&part), copied.into_bytes());
        }
        // Its own names: the print area, its titles, names local to it.
        let q_old = format!("{}!", super::sheet_ops::qualified(&src.name));
        let q_new = format!("{}!", super::sheet_ops::qualified(&name));
        let locals: Vec<DefinedName> = self
            .defined_names
            .iter()
            .filter(|n| n.local_sheet == Some(i))
            .cloned()
            .collect();
        for n in locals {
            let to = n.refers_to.replace(&q_old, &q_new);
            self.set_local_name(&n.name, new_idx, Some(&to));
        }
        self.edit_sheets_now(&kalem_viewer::SheetEdit::Rename(new_idx, name))?;
        if at < new_idx {
            self.edit_sheets_now(&kalem_viewer::SheetEdit::Move(new_idx, at))?;
        }
        self.engine = None;
        self.generation += 1;
        Ok(at.min(new_idx))
    }
}
