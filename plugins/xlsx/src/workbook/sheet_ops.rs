//! A workbook's sheets inserted, deleted, renamed, moved, hidden and shown
//! again, as Excel's sheet tab menu does it: the workbook part's
//! `<sheets>` and defined names, the package's parts and relationships,
//! and every formula naming a sheet kept right.

use super::*;
use std::ops::Range as Span;

/// Characters a sheet name cannot have.
const NOT_IN_NAMES: [char; 7] = ['[', ']', ':', '*', '?', '/', '\\'];

/// A sheet name as a formula writes it before `!`: quoted when it is not
/// a plain word or reads as a cell.
pub(crate) fn qualified(name: &str) -> String {
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && crate::cellref::CellRef::parse(name).is_none()
        && !looks_like_r1c1(name);
    if plain {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

fn looks_like_r1c1(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    let rest = u.strip_prefix('R').unwrap_or(&u);
    let (a, b) = rest.split_once('C').unwrap_or((rest, ""));
    (u.starts_with('R') || u.starts_with('C'))
        && a.chars().all(|c| c.is_ascii_digit())
        && b.chars().all(|c| c.is_ascii_digit())
}

/// A formula with its references to sheet `old` naming `new` instead, or
/// `#REF!` (`None`) when the sheet is gone, as Excel makes them.
pub(crate) fn rename_in_formula(f: &str, old: &str, new: Option<&str>) -> String {
    let chars: Vec<char> = f.chars().collect();
    let to = match new {
        Some(n) => format!("{}!", qualified(n)),
        None => "#REF!".to_owned(),
    };
    let same = |name: &str| name.to_lowercase() == old.to_lowercase();
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '.';
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Text in quotes is left as it is.
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '"' {
                    if chars.get(i + 1) == Some(&'"') {
                        out.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        let after_boundary = i == 0 || !(word(chars[i - 1]) || chars[i - 1] == ']');
        if c == '\'' && after_boundary {
            // 'Quoted name'!
            let mut j = i + 1;
            let mut name = String::new();
            while j < chars.len() {
                if chars[j] == '\'' {
                    if chars.get(j + 1) == Some(&'\'') {
                        name.push('\'');
                        j += 2;
                        continue;
                    }
                    break;
                }
                name.push(chars[j]);
                j += 1;
            }
            if chars.get(j + 1) == Some(&'!') && same(&name) {
                out.push_str(&to);
                i = j + 2;
                continue;
            }
            let end = (j + 1).min(chars.len());
            out.extend(&chars[i..end]);
            i = end;
            continue;
        }
        if word(c) && after_boundary {
            let mut j = i;
            while j < chars.len() && word(chars[j]) {
                j += 1;
            }
            let name: String = chars[i..j].iter().collect();
            if chars.get(j) == Some(&'!') && same(&name) {
                out.push_str(&to);
                i = j + 1;
            } else {
                out.push_str(&name);
                i = j;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// An XML part with the text of every element named in `elements` (and
/// the `location` of hyperlinks, the `sheet` of a pivot cache's source)
/// changed by `rename`; `None` when nothing changed.
fn renamed_part(xml: &str, old: &str, new: Option<&str>) -> Option<String> {
    let elements = ["f", "formula", "formula1", "formula2", "definedName"];
    let mut r = Reader::new(xml);
    let mut splices = Vec::new();
    let mut inside = false;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                inside = !tag.empty && elements.contains(&tag.name);
                let attr = match tag.name {
                    "hyperlink" => Some("location"),
                    "worksheetSource" => Some("sheet"),
                    _ => None,
                };
                if let Some(a) = attr
                    && let Some(v) = tag.attr(a)
                {
                    let changed = if a == "sheet" {
                        (v.to_lowercase() == old.to_lowercase())
                            .then(|| new.unwrap_or("#REF!").to_owned())
                    } else {
                        let n = rename_in_formula(&v, old, new);
                        (n != v).then_some(n)
                    };
                    if let Some(n) = changed {
                        let span = tag.span.clone();
                        splices.push((span.clone(), xml::set_attr(&xml[span], a, &n)));
                    }
                }
            }
            Token::Text { raw, span } if inside => {
                let text = xml::unescape(raw);
                let n = rename_in_formula(&text, old, new);
                if n != text {
                    splices.push((span, xml::escape(&n)));
                }
            }
            Token::End { .. } => inside = false,
            Token::Text { .. } => {}
        }
    }
    (!splices.is_empty()).then(|| splice(xml, splices))
}

impl Workbook {
    /// Every sheet's edited text written into the package, and what was
    /// kept by sheet number forgotten: before the sheets change places.
    fn settle_sheets(&mut self) -> Result<()> {
        self.flush()?;
        for &i in &self.dirty_sheets {
            self.pkg
                .set_part(&self.sheets[i].part, self.loaded[&i].0.clone().into_bytes());
        }
        self.dirty_sheets.clear();
        self.loaded.clear();
        self.computed.clear();
        self.trusted.clear();
        self.validations.clear();
        self.complete.clear();
        self.scratch.clear();
        self.engine = None;
        self.generation += 1;
        Ok(())
    }

    /// The `<sheet>` elements of the workbook part, in order, and the
    /// `<definedName>` start tags with where their text lies.
    fn sheet_elements(&self) -> Vec<Span<usize>> {
        let mut r = Reader::new(&self.workbook_xml);
        let mut out = Vec::new();
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "sheet"
            {
                let start = tag.span.start;
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                out.push(start..end);
            }
        }
        out
    }

    /// The workbook part with each `<definedName>`'s `localSheetId`, and
    /// the views' `activeTab`, mapped to new places (`None`: the name goes).
    fn remap_sheet_ids(&mut self, map: impl Fn(usize) -> Option<usize>) {
        let text = self.workbook_xml.clone();
        let mut r = Reader::new(&text);
        let mut splices = Vec::new();
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "definedName" => {
                    let Some(id) = tag.attr("localSheetId").and_then(|v| v.parse().ok()) else {
                        continue;
                    };
                    let start = tag.span.start;
                    match map(id) {
                        Some(n) => splices.push((
                            tag.span.clone(),
                            xml::set_attr(&text[tag.span.clone()], "localSheetId", &n.to_string()),
                        )),
                        None => {
                            let end = if tag.empty {
                                tag.span.end
                            } else {
                                r.skip_element()
                            };
                            splices.push((start..end, String::new()));
                        }
                    }
                }
                "workbookView" => {
                    let active: usize = tag
                        .attr("activeTab")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let n = map(active).unwrap_or(0);
                    let mut t = xml::set_attr(&text[tag.span.clone()], "activeTab", &n.to_string());
                    if tag.attr("firstSheet").is_some() {
                        t = xml::set_attr(&t, "firstSheet", "0");
                    }
                    splices.push((tag.span.clone(), t));
                }
                _ => {}
            }
        }
        self.workbook_xml = splice(&text, splices);
    }

    /// The sheets and names read again from the workbook part.
    fn reread_workbook(&mut self) {
        self.sheets.clear();
        self.defined_names.clear();
        self.read_workbook_part();
    }

    /// Every formula in the package naming sheet `old` made to name `new`
    /// (or `#REF!`).
    fn rename_everywhere(&mut self, old: &str, new: Option<&str>) -> Result<()> {
        if let Some(t) = renamed_part(&self.workbook_xml, old, new) {
            self.workbook_xml = t;
        }
        for name in self.pkg.names() {
            let wanted = [
                "xl/worksheets/",
                "xl/charts/",
                "xl/pivotCache/",
                "xl/chartsheets/",
            ]
            .iter()
            .any(|d| name.starts_with(d))
                && name.ends_with(".xml");
            if !wanted {
                continue;
            }
            let text = text_of(self.pkg.part(&name)?, &name)?;
            if let Some(t) = renamed_part(&text, old, new) {
                self.pkg.set_part(&name, t.into_bytes());
            }
        }
        Ok(())
    }

    /// Whether `name` can name sheet `idx`: not empty, at most 31
    /// characters, none of `[]:*?/\`, not starting or ending with `'`, and
    /// no other sheet's (in either case).
    pub fn check_sheet_name(&self, idx: usize, name: &str) -> Result<()> {
        let refuse = |m: &str| Err(Error::Refused(m.into()));
        if name.trim().is_empty() {
            return refuse("A sheet's name cannot be empty");
        }
        if name.chars().count() > 31 {
            return refuse("A sheet's name is 31 characters at most");
        }
        if name.contains(NOT_IN_NAMES) {
            return refuse("A sheet's name cannot have [ ] : * ? / or \\");
        }
        if name.starts_with('\'') || name.ends_with('\'') {
            return refuse("A sheet's name cannot begin or end with '");
        }
        if name.eq_ignore_ascii_case("History") {
            return refuse("History is a name Excel keeps for itself");
        }
        if self
            .sheets
            .iter()
            .enumerate()
            .any(|(i, s)| i != idx && s.name.to_lowercase() == name.to_lowercase())
        {
            return refuse(&format!("There is a sheet named {name} already"));
        }
        Ok(())
    }

    /// Changes the sheets, as one undo step; the sheet to show after it.
    pub fn edit_sheets(&mut self, edit: &kalem_viewer::SheetEdit) -> Result<usize> {
        use kalem_viewer::SheetEdit;
        self.check_structure()?;
        let n = self.sheets.len();
        let check = |i: usize| {
            if i < n {
                Ok(())
            } else {
                Err(Error::Refused("No such sheet".into()))
            }
        };
        let visible_others = |this: &Self, i: usize| {
            this.sheets
                .iter()
                .enumerate()
                .any(|(j, s)| j != i && s.visibility == Visibility::Visible)
        };
        match edit {
            SheetEdit::Insert(at) => {
                if *at > n {
                    return Err(Error::Refused("No such place".into()));
                }
            }
            SheetEdit::Delete(i) => {
                check(*i)?;
                if !visible_others(self, *i) {
                    return Err(Error::Refused(
                        "A workbook must keep at least one visible sheet".into(),
                    ));
                }
            }
            SheetEdit::Rename(i, name) => {
                check(*i)?;
                self.check_sheet_name(*i, name)?;
            }
            SheetEdit::Move(from, to) => {
                check(*from)?;
                check(*to)?;
            }
            SheetEdit::Hide(i, hide) => {
                check(*i)?;
                if *hide && !visible_others(self, *i) {
                    return Err(Error::Refused(
                        "A workbook must keep at least one visible sheet".into(),
                    ));
                }
            }
        }
        let snapshot = self.snapshot();
        let result = self.edit_sheets_now(edit);
        match result {
            Ok(shown) => {
                // Saved with the rest, though only the workbook part changed.
                let part = self.workbook_part.clone();
                self.pkg
                    .set_part(&part, self.workbook_xml.clone().into_bytes());
                self.undo.push(snapshot);
                self.redo.clear();
                Ok(shown)
            }
            Err(e) => {
                self.restore(snapshot);
                Err(e)
            }
        }
    }

    fn edit_sheets_now(&mut self, edit: &kalem_viewer::SheetEdit) -> Result<usize> {
        use kalem_viewer::SheetEdit;
        self.settle_sheets()?;
        match edit {
            SheetEdit::Insert(at) => {
                let last = self.add_sheet("Sheet")?;
                if *at < last {
                    self.move_sheet(last, *at);
                }
                Ok(*at)
            }
            SheetEdit::Move(from, to) => {
                self.move_sheet(*from, *to);
                Ok(*to)
            }
            SheetEdit::Rename(i, name) => {
                let old = self.sheets[*i].name.clone();
                let els = self.sheet_elements();
                let el = &self.workbook_xml[els[*i].clone()];
                let tag_end = el.find('>').map_or(el.len(), |p| p + 1);
                let head = xml::set_attr(&el[..tag_end], "name", name);
                self.workbook_xml = splice(
                    &self.workbook_xml,
                    vec![(els[*i].start..els[*i].start + tag_end, head)],
                );
                if old != *name {
                    self.rename_everywhere(&old, Some(name))?;
                }
                self.reread_workbook();
                Ok(*i)
            }
            SheetEdit::Hide(i, hide) => {
                let els = self.sheet_elements();
                let el = &self.workbook_xml[els[*i].clone()];
                let tag_end = el.find('>').map_or(el.len(), |p| p + 1);
                let head = if *hide {
                    xml::set_attr(&el[..tag_end], "state", "hidden")
                } else {
                    xml::remove_attr(&el[..tag_end], "state")
                };
                self.workbook_xml = splice(
                    &self.workbook_xml,
                    vec![(els[*i].start..els[*i].start + tag_end, head)],
                );
                self.reread_workbook();
                // The tab shown when the file opens is a visible one.
                let i = *i;
                let first_visible = self
                    .sheets
                    .iter()
                    .position(|s| s.visibility == Visibility::Visible)
                    .unwrap_or(0);
                if *hide {
                    self.remap_sheet_ids(|j| Some(if j == i { first_visible } else { j }));
                }
                Ok(i)
            }
            SheetEdit::Delete(i) => {
                let i = *i;
                let info = self.sheets[i].clone();
                // Its `<sheet>`, its relationship, its part and its own
                // relationships go.
                let els = self.sheet_elements();
                let rid = {
                    let el = &self.workbook_xml[els[i].clone()];
                    let mut r = Reader::new(el);
                    let mut id = None;
                    if let Some(Token::Start(tag)) = r.next_token() {
                        id = tag.attr("id").map(|v| v.into_owned());
                    }
                    id
                };
                self.workbook_xml =
                    splice(&self.workbook_xml, vec![(els[i].clone(), String::new())]);
                self.remap_sheet_ids(|j| match j.cmp(&i) {
                    std::cmp::Ordering::Less => Some(j),
                    std::cmp::Ordering::Equal => None,
                    std::cmp::Ordering::Greater => Some(j - 1),
                });
                if let Some(rid) = rid {
                    self.remove_workbook_rel(&rid)?;
                }
                self.remove_part(&info.part)?;
                let own_rels = rels::rels_path(&info.part);
                if self.pkg.contains(&own_rels) {
                    self.pkg.remove_part(&own_rels);
                }
                self.rename_everywhere(&info.name, None)?;
                // The calculation chain names the sheet by its id: Excel
                // builds it again, and computes the formulas on opening.
                self.drop_calc_chain()?;
                self.set_full_calc_on_load();
                self.reread_workbook();
                Ok(i.min(self.sheets.len().saturating_sub(1)))
            }
        }
    }

    /// Sheet `from` put at place `to`, the others closing up.
    fn move_sheet(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        let els = self.sheet_elements();
        let texts: Vec<String> = els
            .iter()
            .map(|s| self.workbook_xml[s.clone()].to_owned())
            .collect();
        let mut order: Vec<usize> = (0..texts.len()).collect();
        let moved = order.remove(from);
        order.insert(to, moved);
        let joined: String = order.iter().map(|&k| texts[k].as_str()).collect();
        let (first, last) = (els[0].start, els[els.len() - 1].end);
        self.workbook_xml = splice(&self.workbook_xml, vec![(first..last, joined)]);
        let place = |j: usize| order.iter().position(|&k| k == j);
        self.remap_sheet_ids(place);
        self.reread_workbook();
    }

    /// A relationship of the workbook part taken away.
    fn remove_workbook_rel(&mut self, rid: &str) -> Result<()> {
        let path = rels::rels_path(&self.workbook_part);
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
        self.workbook_rels.retain(|r| r.id != rid);
        Ok(())
    }

    /// A part taken out of the package and its content types.
    fn remove_part(&mut self, name: &str) -> Result<()> {
        self.pkg.remove_part(name);
        let ct = "[Content_Types].xml";
        let types = text_of(self.pkg.part(ct)?, ct)?;
        let mut r = Reader::new(&types);
        let mut splices = Vec::new();
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "Override"
                && tag.attr("PartName").as_deref() == Some(&format!("/{name}"))
            {
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                splices.push((tag.span.start..end, String::new()));
            }
        }
        self.pkg.set_part(ct, splice(&types, splices).into_bytes());
        Ok(())
    }

    /// The sheets that are hidden.
    pub fn hidden_sheets(&self) -> Vec<usize> {
        self.sheets
            .iter()
            .enumerate()
            .filter(|(_, s)| s.visibility != Visibility::Visible)
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_follow_a_sheet() {
        let r = |f: &str| rename_in_formula(f, "Data", Some("My Data"));
        assert_eq!(r("=Data!A1+1"), "='My Data'!A1+1");
        assert_eq!(r("=SUM('Data'!B2:B9)"), "=SUM('My Data'!B2:B9)");
        assert_eq!(r("=data!A1"), "='My Data'!A1");
        // Not another sheet, a name ending so, or text in quotes.
        assert_eq!(r("=OldData!A1&\"Data!A1\""), "=OldData!A1&\"Data!A1\"");
        assert_eq!(r("=[1]Data!A1"), "=[1]Data!A1");
        let gone = |f: &str| rename_in_formula(f, "My Data", None);
        assert_eq!(gone("='My Data'!A1*2"), "=#REF!A1*2");
        assert_eq!(qualified("Sheet2"), "Sheet2");
        assert_eq!(qualified("A1"), "'A1'");
        assert_eq!(qualified("O'Brien"), "'O''Brien'");
        assert_eq!(qualified("Bütçe"), "Bütçe");
    }
}
