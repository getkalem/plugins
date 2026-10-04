//! How a sheet prints, as Excel's Page Layout writes it: `<printOptions>`,
//! `<pageMargins>`, `<pageSetup>`, `<sheetPr><pageSetUpPr fitToPage>`,
//! `<headerFooter>`, `<rowBreaks>`, `<colBreaks>`, the header's and
//! footer's pictures in a VML drawing (`<legacyDrawingHF>`), and the
//! sheet's `_xlnm.Print_Area` and `_xlnm.Print_Titles` names.

use super::*;
use kalem_viewer::{HeaderPicture, PageSetup};

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
        let mut fit = (1, 1);
        let mut in_cols = false;
        let mut hf_rid = None;
        while let Some(tok) = r.next_token() {
            if let Token::End { name, .. } = tok
                && name == "colBreaks"
            {
                in_cols = false;
            }
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
                    let n = |k: &str| tag.attr(k).and_then(|v| v.parse::<u32>().ok());
                    fit = (n("fitToWidth").unwrap_or(1), n("fitToHeight").unwrap_or(1));
                    s.scale = n("scale").unwrap_or(100).clamp(10, 400);
                }
                "printOptions" => {
                    let on = |k: &str| matches!(tag.attr(k).as_deref(), Some("1" | "true"));
                    s.gridlines = on("gridLines");
                    s.headings = on("headings");
                }
                "legacyDrawingHF" => {
                    hf_rid = tag.attr("id").map(|v| v.into_owned());
                }
                "oddHeader" if !tag.empty => s.header = r.text_until_end("oddHeader").0,
                "oddFooter" if !tag.empty => s.footer = r.text_until_end("oddFooter").0,
                "colBreaks" => in_cols = !tag.empty,
                "brk" => {
                    if let Some(id) = tag.attr("id").and_then(|v| v.parse().ok()) {
                        if in_cols {
                            s.col_breaks.push(id);
                        } else {
                            s.row_breaks.push(id);
                        }
                    }
                }
                _ => {}
            }
        }
        s.fit = fit_to_page.then_some(fit);
        if let Some(rid) = hf_rid {
            s.pictures = self.header_pictures(&self.sheets[idx].part.clone(), &rid);
        }
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
                for r in local_ref(&d.refers_to) {
                    let Some((a, b)) = r.split_once(':') else {
                        continue;
                    };
                    if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                        if a > 0 && b > 0 {
                            s.title_rows = Some((a - 1, b - 1));
                        }
                    } else if let (Some(a), Some(b)) = (column_number(a), column_number(b)) {
                        s.title_cols = Some((a, b));
                    }
                }
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
            // The page setup's other attributes (its printer settings) kept.
            let mut tag = top_level(&text, "pageSetup")
                .map_or_else(|| format!("<{p}pageSetup/>"), |(_, t)| t);
            tag = xml::set_attr(&tag, "paperSize", &s.paper.to_string());
            if s.scale == 100 {
                tag = xml::remove_attr(&tag, "scale");
            } else {
                tag = xml::set_attr(&tag, "scale", &s.scale.clamp(10, 400).to_string());
            }
            for (k, v) in [
                ("fitToWidth", s.fit.map(|f| f.0)),
                ("fitToHeight", s.fit.map(|f| f.1)),
            ] {
                tag = match v {
                    Some(n) if n != 1 => xml::set_attr(&tag, k, &n.to_string()),
                    _ => xml::remove_attr(&tag, k),
                };
            }
            tag = xml::set_attr(
                &tag,
                "orientation",
                if s.landscape { "landscape" } else { "portrait" },
            );
            text = with_element(&text, "pageSetup", &tag);
            let mut opts = top_level(&text, "printOptions")
                .map_or_else(|| format!("<{p}printOptions/>"), |(_, t)| t);
            for (k, on) in [("gridLines", s.gridlines), ("headings", s.headings)] {
                opts = if on {
                    xml::set_attr(&opts, k, "1")
                } else {
                    xml::remove_attr(&opts, k)
                };
            }
            let bare = opts.trim_start_matches('<').split([' ', '/', '>']).next() == Some(&format!("{p}printOptions"))
                && !opts.contains('=');
            text = with_element(&text, "printOptions", if bare { "" } else { &opts });
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
            let breaks = if s.col_breaks.is_empty() {
                String::new()
            } else {
                let mut cols = s.col_breaks.clone();
                cols.sort_unstable();
                cols.dedup();
                let items: String = cols
                    .iter()
                    .map(|c| format!("<{p}brk id=\"{c}\" max=\"1048575\" man=\"1\"/>"))
                    .collect();
                format!(
                    "<{p}colBreaks count=\"{n}\" manualBreakCount=\"{n}\">{items}</{p}colBreaks>",
                    n = cols.len()
                )
            };
            text = with_element(&text, "colBreaks", &breaks);
            text = with_fit_to_page(&text, &p, s.fit.is_some());
            wb.replace_sheet_text(idx, text);
            wb.set_header_pictures(idx, &s.pictures)?;
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
            let letters = |c: u32| {
                let s = CellRef::new(0, c).to_string();
                s.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_owned()
            };
            let titles: Vec<String> = s
                .title_cols
                .map(|(a, b)| format!("{quoted}!${}:${}", letters(a), letters(b)))
                .into_iter()
                .chain(
                    s.title_rows
                        .map(|(a, b)| format!("{quoted}!${}:${}", a + 1, b + 1)),
                )
                .collect();
            let titles = (!titles.is_empty()).then(|| titles.join(","));
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

/// A sheet part's top-level element `name`, as one empty element.
fn top_level(text: &str, name: &str) -> Option<(Span<usize>, String)> {
    let mut r = Reader::new(text);
    let mut depth = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == name {
                    let head = &text[tag.span.clone()];
                    let el = if tag.empty {
                        head.to_owned()
                    } else {
                        format!("{}/>", head.trim_end_matches('>'))
                    };
                    return Some((tag.span.clone(), el));
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => depth -= 1,
            Token::Text { .. } => {}
        }
    }
    None
}

/// A column's number from its letters (`A` 0).
fn column_number(letters: &str) -> Option<u32> {
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    CellRef::parse(&format!("{letters}1")).map(|c| c.col)
}

const VML_HF_TYPE: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";

/// A picture's extension and type by its first bytes.
fn picture_kind(data: &[u8]) -> Option<(&'static str, &'static str)> {
    if data.starts_with(b"\x89PNG") {
        Some(("png", "image/png"))
    } else if data.starts_with(&[0xFF, 0xD8]) {
        Some(("jpeg", "image/jpeg"))
    } else if data.starts_with(b"GIF8") {
        Some(("gif", "image/gif"))
    } else {
        None
    }
}

/// A VML length (`75pt`, `100px`, `1in`, `2cm`) in points.
fn points(v: &str) -> Option<f32> {
    let v = v.trim();
    let (n, unit) = v.split_at(v.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(v.len()));
    let n: f32 = n.trim().parse().ok()?;
    Some(match unit {
        "px" => n * 0.75,
        "in" => n * 72.0,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        _ => n,
    })
}

impl Workbook {
    /// The pictures of the VML drawing a sheet's `<legacyDrawingHF>` names.
    fn header_pictures(&self, sheet_part: &str, rid: &str) -> Vec<HeaderPicture> {
        let Some(vml) = self.rel_target(sheet_part, rid) else {
            return Vec::new();
        };
        let Some(text) = self.pkg.part(&vml).ok().and_then(|b| text_of(b, &vml).ok()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut r = Reader::new(&text);
        let mut shape: Option<(String, (f32, f32))> = None;
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "shape" => {
                    let style = tag.attr("style").unwrap_or_default().into_owned();
                    let get = |k: &str| {
                        style.split(';').find_map(|d| {
                            let (a, b) = d.split_once(':')?;
                            (a.trim() == k).then(|| points(b)).flatten()
                        })
                    };
                    shape = tag.attr("id").map(|id| {
                        (
                            id.into_owned(),
                            (get("width").unwrap_or(72.0), get("height").unwrap_or(72.0)),
                        )
                    });
                }
                "imagedata" => {
                    let Some((place, size)) = shape.take() else {
                        continue;
                    };
                    let Some(rel) = tag.attr("relid").map(|v| v.into_owned()) else {
                        continue;
                    };
                    if let Some(data) = self
                        .rel_target(&vml, &rel)
                        .and_then(|p| self.pkg.part(&p).ok())
                    {
                        out.push(HeaderPicture { place, data, size });
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Sheet `idx`'s header and footer pictures made `pictures`: a VML
    /// drawing of them named by `<legacyDrawingHF>`, or none. Unchanged
    /// pictures are left as they are.
    fn set_header_pictures(&mut self, idx: usize, pictures: &[HeaderPicture]) -> Result<()> {
        let sheet_part = self.sheets[idx].part.clone();
        let text = self.loaded[&idx].0.clone();
        let old = top_level(&text, "legacyDrawingHF");
        let old_rid = old.as_ref().and_then(|(_, el)| {
            el.split_whitespace()
                .find(|a| a.contains(":id="))
                .and_then(|a| a.split('"').nth(1))
                .map(str::to_owned)
        });
        let now = old_rid
            .as_deref()
            .map_or_else(Vec::new, |rid| self.header_pictures(&sheet_part, rid));
        if now == pictures {
            return Ok(());
        }
        if let Some(rid) = &old_rid {
            self.remove_rel_of(&sheet_part, rid)?;
        }
        let mut text = with_element(&text, "legacyDrawingHF", "");
        if !pictures.is_empty() {
            let vml = self.free_part_ext("xl/drawings/vmlDrawing", "vml");
            let mut shapes = String::new();
            for (n, pic) in pictures.iter().enumerate() {
                let Some((ext, mime)) = picture_kind(&pic.data) else {
                    return Err(Error::Refused(
                        "A header's picture is a PNG, JPEG or GIF".into(),
                    ));
                };
                let media = self.free_part_ext("xl/media/image", ext);
                self.pkg.set_part(&media, pic.data.clone());
                self.ensure_default_type(ext, mime)?;
                let rid = self.add_rel(&vml, "image", &media)?;
                shapes.push_str(&format!(
                    "<v:shape id=\"{}\" o:spid=\"_x0000_s{}\" type=\"#_x0000_t75\" style=\"position:absolute;margin-left:0;margin-top:0;width:{:.2}pt;height:{:.2}pt;z-index:{}\"><v:imagedata o:relid=\"{rid}\" o:title=\"Picture {}\"/><o:lock v:ext=\"edit\" rotation=\"t\"/></v:shape>",
                    xml::escape(&pic.place),
                    (idx + 10) * 1024 + n + 1,
                    pic.size.0,
                    pic.size.1,
                    n + 1,
                    n + 1
                ));
            }
            let body = format!(
                "<xml xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\"><o:shapelayout v:ext=\"edit\"><o:idmap v:ext=\"edit\" data=\"{}\"/></o:shapelayout><v:shapetype id=\"_x0000_t75\" coordsize=\"21600,21600\" o:spt=\"75\" o:preferrelative=\"t\" path=\"m@4@5l@4@11@9@11@9@5xe\" filled=\"f\" stroked=\"f\"><v:stroke joinstyle=\"miter\"/><v:formulas><v:f eqn=\"if lineDrawn pixelLineWidth 0\"/><v:f eqn=\"sum @0 1 0\"/><v:f eqn=\"sum 0 0 @1\"/><v:f eqn=\"prod @2 1 2\"/><v:f eqn=\"prod @3 21600 pixelWidth\"/><v:f eqn=\"prod @3 21600 pixelHeight\"/><v:f eqn=\"sum @0 0 1\"/><v:f eqn=\"prod @6 1 2\"/><v:f eqn=\"prod @7 21600 pixelWidth\"/><v:f eqn=\"sum @8 21600 0\"/><v:f eqn=\"prod @7 21600 pixelHeight\"/><v:f eqn=\"sum @10 21600 0\"/></v:formulas><v:path o:extrusionok=\"f\" gradientshapeok=\"t\" o:connecttype=\"rect\"/><o:lock v:ext=\"edit\" aspectratio=\"t\"/></v:shapetype>{shapes}</xml>",
                idx + 10
            );
            self.pkg.set_part(&vml, body.into_bytes());
            self.ensure_default_type("vml", VML_HF_TYPE)?;
            let rid = self.add_rel(&sheet_part, "vmlDrawing", &vml)?;
            let p = self.loaded[&idx].1.prefix.clone();
            let (rp, decl) = super::links::sheet_r_prefix(&text);
            text = insert_top_level(
                &text,
                after("legacyDrawingHF"),
                &format!("<{p}legacyDrawingHF{decl} {rp}:id=\"{rid}\"/>"),
            );
        }
        self.replace_sheet_text(idx, text);
        Ok(())
    }
}
