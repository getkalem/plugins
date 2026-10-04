//! Defined names added, changed and deleted as Excel's Name Manager does:
//! `<definedName>`s in the workbook part's `<definedNames>`, the formulas
//! computed again with them.

use super::*;

/// The workbook part's children after `<definedNames>` (CT_Workbook).
const AFTER_DEFINED_NAMES: [&str; 10] = [
    "calcPr",
    "oleSize",
    "customWorkbookViews",
    "pivotCaches",
    "smartTagPr",
    "smartTagTypes",
    "webPublishing",
    "fileRecoveryPr",
    "webPublishObjects",
    "extLst",
];

/// Whether Excel takes `name` as a name: a letter, `_` or `\` first, then
/// letters, digits, `_`, `.` and `\`; not a cell, not `R` or `C`.
pub(crate) fn check_name(name: &str) -> Result<()> {
    let refuse = |m: String| Err(Error::Refused(m));
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return refuse("A name cannot be empty".into());
    };
    if !(first.is_alphabetic() || first == '_' || first == '\\') {
        return refuse(format!("{name}: a name begins with a letter, _ or \\"));
    }
    if !chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\')) {
        return refuse(format!("{name}: a name has letters, digits, _ and . only"));
    }
    if name.chars().count() > 255 {
        return refuse("A name is 255 characters at most".into());
    }
    let upper = name.to_uppercase();
    let r1c1 = {
        let rest = upper.strip_prefix('R').unwrap_or(&upper);
        let (a, b) = rest.split_once('C').unwrap_or((rest, ""));
        (upper.starts_with('R') || upper.starts_with('C'))
            && a.chars().all(|c| c.is_ascii_digit())
            && b.chars().all(|c| c.is_ascii_digit())
    };
    if CellRef::parse(name).is_some() || r1c1 {
        return refuse(format!("{name} is a cell's name"));
    }
    Ok(())
}

impl Workbook {
    /// Defines a workbook-wide name as `refers_to` (formula text, without
    /// `=`), replacing one of that name, or deletes it (`None`). One undo
    /// step.
    pub fn set_defined_name(&mut self, name: &str, refers_to: Option<&str>) -> Result<()> {
        if refers_to.is_some() {
            check_name(name)?;
        }
        let exists = self
            .defined_names
            .iter()
            .any(|d| d.local_sheet.is_none() && d.name.to_lowercase() == name.to_lowercase());
        if refers_to.is_none() && !exists {
            return Err(Error::Refused(format!("No name {name}")));
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        // The results with the names as they were, and which stored ones
        // the engine reproduced.
        self.ensure_engine()?;
        let before = self.computed.clone();
        let trusted = self.trusted.clone();
        let mut text = self.workbook_xml.clone();
        // The name as it was, out.
        let mut r = Reader::new(&text);
        let mut splices = Vec::new();
        let mut prefix = String::new();
        let mut list: Option<(std::ops::Range<usize>, bool)> = None;
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "workbook" => prefix = xml::prefix(tag.qname).to_owned(),
                "definedNames" => list = Some((tag.span.clone(), tag.empty)),
                "definedName" => {
                    let start = tag.span.start;
                    let same = tag.attr("localSheetId").is_none()
                        && tag
                            .attr("name")
                            .is_some_and(|n| n.to_lowercase() == name.to_lowercase());
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    if same {
                        splices.push((start..end, String::new()));
                    }
                }
                _ => {}
            }
        }
        if let Some(to) = refers_to {
            let el = format!(
                "<{prefix}definedName name=\"{}\">{}</{prefix}definedName>",
                xml::escape(name),
                xml::escape(to.trim_start_matches('='))
            );
            match &list {
                Some((span, true)) => splices.push((
                    span.clone(),
                    format!("<{prefix}definedNames>{el}</{prefix}definedNames>"),
                )),
                Some((span, false)) => splices.push((span.end..span.end, el)),
                None => {
                    text = splice(&text, std::mem::take(&mut splices));
                    text = insert_top_level(
                        &text,
                        &AFTER_DEFINED_NAMES,
                        &format!("<{prefix}definedNames>{el}</{prefix}definedNames>"),
                    );
                }
            }
        }
        text = splice(&text, splices);
        for empty in [
            format!("<{prefix}definedNames></{prefix}definedNames>"),
            format!("<{prefix}definedNames/>"),
        ] {
            text = text.replace(&empty, "");
        }
        self.workbook_xml = text;
        let part = self.workbook_part.clone();
        self.pkg
            .set_part(&part, self.workbook_xml.clone().into_bytes());
        self.reread_defined_names();
        // Formulas naming it computed again.
        self.compute_again(&before, trusted)?;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }
}

impl Workbook {
    /// Every formula computed by a new engine, the results written as an
    /// edit writes them: stored ones the old engine reproduced (`trusted`,
    /// against `before`) replaced, the others dropped.
    pub(crate) fn compute_again(
        &mut self,
        before: &HashMap<(usize, CellRef), Value>,
        trusted: HashSet<(usize, CellRef)>,
    ) -> Result<()> {
        self.engine = None;
        self.ensure_engine()?;
        self.trusted = trusted;
        let after = std::mem::take(&mut self.computed);
        let cells = self.formula_cells();
        self.write_results(before, after, &cells, &[]);
        self.complete.clear();
        self.scratch.clear();
        self.generation += 1;
        Ok(())
    }

    /// Calculate Now: every formula computed again (the volatile ones,
    /// `NOW` and `RAND`, give new results).
    pub fn recalculate(&mut self) -> Result<()> {
        self.flush()?;
        self.ensure_engine()?;
        let before = self.computed.clone();
        let trusted = self.trusted.clone();
        self.compute_again(&before, trusted)
    }
}
