//! How a sheet prints, as Excel's Page Layout writes it: `<pageMargins>`,
//! `<pageSetup>`, `<sheetPr><pageSetUpPr fitToPage>`, `<headerFooter>`,
//! `<rowBreaks>`, and the sheet's `_xlnm.Print_Area` and
//! `_xlnm.Print_Titles` names.

use super::*;
use kalem_viewer::PageSetup;

const AREA: &str = "_xlnm.Print_Area";
const TITLES: &str = "_xlnm.Print_Titles";

/// The worksheet's children after `name` in the schema (CT_Worksheet), for
/// where it goes when the sheet has none.
fn after(name: &str) -> &'static [&'static str] {
    const ORDER: [&str; 20] = [
        "printOptions",
        "pageMargins",
        "pageSetup",
        "headerFooter",
        "rowBreaks",
        "colBreaks",
        "customProperties",
        "cellWatches",
        "ignoredErrors",
        "smartTags",
        "drawing",
        "legacyDrawing",
        "legacyDrawingHF",
        "drawingHF",
        "picture",
        "oleObjects",
        "controls",
        "webPublishItems",
        "tableParts",
        "extLst",
    ];
    let i = ORDER
        .iter()
        .position(|n| *n == name)
        .map_or(ORDER.len(), |i| i + 1);
    &ORDER[i..]
}

/// A sheet part with its top-level element `name` replaced by `el` (taken
/// away when empty), or `el` put where the schema puts it.
fn with_element(text: &str, name: &str, el: &str) -> String {
    let mut r = Reader::new(text);
    let mut depth = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == name {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    return splice(text, vec![(tag.span.start..end, el.to_owned())]);
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => depth -= 1,
            Token::Text { .. } => {}
        }
    }
    if el.is_empty() {
        return text.to_owned();
    }
    insert_top_level(text, after(name), el)
}

/// The part of a print name's formula on the sheet, without the sheet.
fn local_ref(refers_to: &str) -> Vec<String> {
    refers_to
        .split(',')
        .map(|p| {
            let p = p.trim();
            p.rsplit_once('!').map_or(p, |(_, r)| r).replace('$', "")
        })
        .collect()
}

impl Workbook {
    /// How sheet `idx` prints.
    pub fn page_setup(&mut self, idx: usize) -> Result<PageSetup> {
        self.load(idx)?;
        let text = self.loaded[&idx].0.clone();
        let mut s = PageSetup::default();
        let mut r = Reader::new(&text);
        let num = |t: &crate::xml::Tag<'_>, k: &str| t.attr(k).and_then(|v| v.parse::<f32>().ok());
        let mut fit_to_page = false;
        let mut fit_width = None;
        while let Some(tok) = r.next_token() {
            let Token::Start(tag) = tok else { continue };
            match tag.name {
                "pageSetUpPr" => {
                    fit_to_page = tag
                        .attr("fitToPage")
                        .as_deref()
                        .is_some_and(|v| v == "1" || v == "true");
                }
                "pageMargins" => {
                    let d = s.margins;
                    s.margins = [
                        num(&tag, "left").unwrap_or(d[0]),
                        num(&tag, "right").unwrap_or(d[1]),
                        num(&tag, "top").unwrap_or(d[2]),
                        num(&tag, "bottom").unwrap_or(d[3]),
                    ];
                }
                "pageSetup" => {
                    s.landscape = tag.attr("orientation").as_deref() == Some("landscape");
                    s.paper = tag
                        .attr("paperSize")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(9);
                    fit_width = Some(
                        tag.attr("fitToWidth")
                            .and_then(|v| v.parse::<u32>().ok())
                            .unwrap_or(1)
                            == 1
                            && tag.attr("fitToHeight").and_then(|v| v.parse::<u32>().ok())
                                == Some(0),
                    );
                }
                "oddHeader" if !tag.empty => s.header = r.text_until_end("oddHeader").0,
                "oddFooter" if !tag.empty => s.footer = r.text_until_end("oddFooter").0,
                "rowBreaks" => {}
                "brk" => {
                    if let Some(id) = tag.attr("id").and_then(|v| v.parse().ok()) {
                        s.row_breaks.push(id);
                    }
                }
                // The column breaks' `<brk>`s are not rows.
                "colBreaks" if !tag.empty => {
                    r.skip_element();
                }
                _ => {}
            }
        }
        s.fit_width = fit_to_page && fit_width.unwrap_or(true);
        for d in self
            .defined_names
            .iter()
            .filter(|d| d.local_sheet == Some(idx))
        {
            if d.name.eq_ignore_ascii_case(AREA) {
                s.print_area = local_ref(&d.refers_to).first().and_then(|r| {
                    Range::parse(r)
                        .or_else(|| CellRef::parse(r).map(|c| Range { start: c, end: c }))
                        .map(|r| [r.start.row, r.start.col, r.end.row, r.end.col])
                });
            } else if d.name.eq_ignore_ascii_case(TITLES) {
                s.title_rows = local_ref(&d.refers_to).iter().find_map(|r| {
                    let (a, b) = r.split_once(':')?;
                    let (a, b) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?);
                    Some((a.checked_sub(1)?, b.checked_sub(1)?))
                });
            }
        }
        Ok(s)
    }

    /// Sets how sheet `idx` prints, as Excel writes it. One undo step.
    pub fn set_page_setup(&mut self, idx: usize, s: &PageSetup) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let sheet = self.sheets[idx].name.clone();
        self.in_one_step(|wb| {
            let p = wb.loaded[&idx].1.prefix.clone();
            let mut text = wb.loaded[&idx].0.clone();
            let [l, r, t, b] = s.margins;
            text = with_element(
                &text,
                "pageMargins",
                &format!(
                    "<{p}pageMargins left=\"{l}\" right=\"{r}\" top=\"{t}\" bottom=\"{b}\" header=\"0.3\" footer=\"0.3\"/>"
                ),
            );
            let fit = if s.fit_width {
                " fitToWidth=\"1\" fitToHeight=\"0\""
            } else {
                ""
            };
            text = with_element(
                &text,
                "pageSetup",
                &format!(
                    "<{p}pageSetup paperSize=\"{}\" orientation=\"{}\"{fit}/>",
                    s.paper,
                    if s.landscape { "landscape" } else { "portrait" }
                ),
            );
            let hf = if s.header.is_empty() && s.footer.is_empty() {
                String::new()
            } else {
                let part = |n: &str, v: &str| {
                    if v.is_empty() {
                        String::new()
                    } else {
                        format!("<{p}{n}>{}</{p}{n}>", xml::escape(v))
                    }
                };
                format!(
                    "<{p}headerFooter>{}{}</{p}headerFooter>",
                    part("oddHeader", &s.header),
                    part("oddFooter", &s.footer)
                )
            };
            text = with_element(&text, "headerFooter", &hf);
            let breaks = if s.row_breaks.is_empty() {
                String::new()
            } else {
                let mut rows = s.row_breaks.clone();
                rows.sort_unstable();
                rows.dedup();
                let items: String = rows
                    .iter()
                    .map(|r| format!("<{p}brk id=\"{r}\" max=\"16383\" man=\"1\"/>"))
                    .collect();
                format!(
                    "<{p}rowBreaks count=\"{n}\" manualBreakCount=\"{n}\">{items}</{p}rowBreaks>",
                    n = rows.len()
                )
            };
            text = with_element(&text, "rowBreaks", &breaks);
            text = with_fit_to_page(&text, &p, s.fit_width);
            wb.replace_sheet_text(idx, text);
            // The print area and titles: the sheet's own names.
            let quoted = super::sheet_ops::qualified(&sheet);
            let area = s.print_area.map(|a| {
                let c = |r: u32, col: u32| {
                    let s = CellRef::new(r, col).to_string();
                    let k = s.find(|ch: char| ch.is_ascii_digit()).unwrap_or(s.len());
                    format!("${}${}", &s[..k], &s[k..])
                };
                format!("{quoted}!{}:{}", c(a[0], a[1]), c(a[2], a[3]))
            });
            wb.set_local_name(AREA, idx, area.as_deref());
            let titles = s.title_rows.map(|(a, b)| format!("{quoted}!${}:${}", a + 1, b + 1));
            wb.set_local_name(TITLES, idx, titles.as_deref());
            wb.batch_changed = true;
            Ok(())
        })
    }
}

/// A sheet part whose `<sheetPr>` says it fits to pages, or not.
fn with_fit_to_page(text: &str, p: &str, fit: bool) -> String {
    let mut r = Reader::new(text);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name == "sheetPr" {
            let start = tag.span.start;
            let end = if tag.empty {
                tag.span.end
            } else {
                r.skip_element()
            };
            let el = &text[start..end];
            let pr = if fit {
                format!("<{p}pageSetUpPr fitToPage=\"1\"/>")
            } else {
                String::new()
            };
            let new =
                crate::chart::set_child(el, &["pageSetUpPr"], &pr, &["tabColor", "outlinePr"]);
            return splice(text, vec![(start..end, new)]);
        }
        if matches!(
            tag.name,
            "dimension" | "sheetViews" | "sheetFormatPr" | "cols" | "sheetData"
        ) {
            break;
        }
    }
    if !fit {
        return text.to_owned();
    }
    insert_top_level(
        text,
        &[
            "dimension",
            "sheetViews",
            "sheetFormatPr",
            "cols",
            "sheetData",
        ],
        &format!("<{p}sheetPr><{p}pageSetUpPr fitToPage=\"1\"/></{p}sheetPr>"),
    )
}
