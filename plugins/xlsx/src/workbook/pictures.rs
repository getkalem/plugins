//! Pictures and shapes on a sheet, as Excel writes them: anchors in the
//! sheet's drawing part (`<xdr:pic>` naming its image in `xl/media`,
//! `<xdr:sp>` with a preset geometry, fill, outline and text), the drawing
//! made with the sheet's `<drawing>` when the sheet has none.

use super::*;
use kalem_viewer::{Drawing, DrawingKind};

const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// An anchor of a drawing part: where it lies, the cells it covers, and
/// what it holds.
struct Anchor {
    span: std::ops::Range<usize>,
    from: (u32, u32),
    to: (u32, u32),
    name: String,
    /// The picture's image relationship.
    image: Option<String>,
    /// The shape, when it is one.
    shape: Option<DrawingKind>,
}

fn rgb(v: &str) -> Option<[u8; 3]> {
    let n = u32::from_str_radix(v.trim(), 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// The pictures' and shapes' anchors of a drawing (charts left out).
fn anchors(text: &str) -> Vec<Anchor> {
    let mut r = Reader::new(text);
    let mut out = Vec::new();
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if !matches!(tag.name, "twoCellAnchor" | "oneCellAnchor") || tag.empty {
            continue;
        }
        let start = tag.span.start;
        let end = r.skip_element();
        let el = &text[start..end];
        let mut a = Anchor {
            span: start..end,
            from: (0, 0),
            to: (0, 0),
            name: String::new(),
            image: None,
            shape: None,
        };
        let mut inner = Reader::new(el);
        let mut corner = 0;
        let (mut in_sp_pr, mut in_ln, mut in_body) = (false, false, false);
        let (mut preset, mut fill, mut line, mut body, mut text_box) = (
            String::from("rect"),
            None,
            None,
            Vec::<String>::new(),
            false,
        );
        let mut is_shape = false;
        let mut chart = false;
        while let Some(t) = inner.next_token() {
            match t {
                Token::Start(k) => match k.name {
                    "from" => corner = 1,
                    "to" => corner = 2,
                    "col" | "row" if corner > 0 => {
                        let v: u32 = inner.text_until_end(k.name).0.trim().parse().unwrap_or(0);
                        let p = if corner == 1 { &mut a.from } else { &mut a.to };
                        if k.name == "col" {
                            p.1 = v;
                        } else {
                            p.0 = v;
                        }
                    }
                    "graphicFrame" => chart = true,
                    "sp" => is_shape = true,
                    "cNvPr" => a.name = k.attr("name").map(|v| v.into_owned()).unwrap_or_default(),
                    "cNvSpPr" => {
                        text_box = k
                            .attr("txBox")
                            .as_deref()
                            .is_some_and(|v| v == "1" || v == "true")
                    }
                    "blip" => a.image = k.attr("embed").map(|v| v.into_owned()),
                    "spPr" if !k.empty => in_sp_pr = true,
                    "ln" if in_sp_pr && !k.empty => in_ln = true,
                    "prstGeom" => {
                        if let Some(p) = k.attr("prst") {
                            preset = p.into_owned();
                        }
                    }
                    "srgbClr" if in_sp_pr => {
                        let c = k.attr("val").and_then(|v| rgb(&v));
                        if in_ln {
                            line = line.or(c);
                        } else {
                            fill = fill.or(c);
                        }
                    }
                    "txBody" => in_body = true,
                    "p" if in_body => body.push(String::new()),
                    "t" if in_body && !k.empty => {
                        let s = xml::unescape(&inner.text_until_end("t").0).into_owned();
                        if body.is_empty() {
                            body.push(String::new());
                        }
                        if let Some(last) = body.last_mut() {
                            last.push_str(&s);
                        }
                    }
                    _ => {}
                },
                Token::End { name, .. } => match name {
                    "from" | "to" => corner = 0,
                    "spPr" => in_sp_pr = false,
                    "ln" => in_ln = false,
                    "txBody" => in_body = false,
                    _ => {}
                },
                Token::Text { .. } => {}
            }
        }
        if chart {
            continue;
        }
        if tag.name == "oneCellAnchor" {
            a.to = (a.from.0 + 1, a.from.1 + 1);
        }
        if is_shape {
            a.shape = Some(DrawingKind::Shape {
                preset,
                fill,
                line,
                text: body.join("\n"),
                text_box,
            });
        }
        if a.image.is_some() || a.shape.is_some() {
            out.push(a);
        }
    }
    out
}

/// The drawing part's element prefix (`xdr:`).
fn prefix_of(text: &str) -> String {
    text.find("wsDr")
        .and_then(|i| text[..i].rfind('<').map(|s| text[s + 1..i].to_owned()))
        .unwrap_or_default()
}

fn corners(p: &str, from: (u32, u32), to: (u32, u32)) -> String {
    format!(
        "<{p}from><{p}col>{}</{p}col><{p}colOff>0</{p}colOff><{p}row>{}</{p}row><{p}rowOff>0</{p}rowOff></{p}from><{p}to><{p}col>{}</{p}col><{p}colOff>0</{p}colOff><{p}row>{}</{p}row><{p}rowOff>0</{p}rowOff></{p}to>",
        from.1,
        from.0,
        to.1 + 1,
        to.0 + 1
    )
}

/// A shape's text as DrawingML paragraphs.
fn text_body(p: &str, text: &str) -> String {
    let paras: String = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                "<a:p/>".to_owned()
            } else {
                format!(
                    "<a:p><a:r><a:rPr lang=\"en-US\" sz=\"1100\"/><a:t>{}</a:t></a:r></a:p>",
                    xml::escape(line)
                )
            }
        })
        .collect();
    format!(
        "<{p}txBody><a:bodyPr xmlns:a=\"{A}\" vertOverflow=\"clip\" wrap=\"square\" rtlCol=\"0\" anchor=\"t\"/><a:lstStyle xmlns:a=\"{A}\"/>{}</{p}txBody>",
        paras.replace("<a:p", &format!("<a:p xmlns:a=\"{A}\""))
    )
}

impl Workbook {
    /// The sheet's drawing part, made (with its relationship and the
    /// sheet's `<drawing>`) when it has none.
    pub(crate) fn ensure_drawing(&mut self, idx: usize) -> Result<String> {
        if let Some(d) = self.sheet_drawing(idx) {
            return Ok(d);
        }
        let drawing = self.free_part("xl/drawings/drawing");
        self.add_part(
            &drawing,
            crate::chart::drawing_xml(""),
            "application/vnd.openxmlformats-officedocument.drawing+xml",
        )?;
        let sheet_part = self.sheets[idx].part.clone();
        let sheet_rid = self.add_rel(&sheet_part, "drawing", &drawing)?;
        let (text, model) = &self.loaded[&idx];
        let p = model.prefix.clone();
        let head = &text[..text.find("<sheetData").unwrap_or(text.len())];
        let rp = head.split("xmlns:").skip(1).find_map(|d| {
            let (pfx, rest) = d.split_once("=\"")?;
            rest.starts_with(R).then(|| pfx.to_owned())
        });
        let el = match rp {
            Some(rp) => format!("<{p}drawing {rp}:id=\"{sheet_rid}\"/>"),
            None => format!("<{p}drawing xmlns:r=\"{R}\" r:id=\"{sheet_rid}\"/>"),
        };
        let new = insert_top_level(
            text,
            &[
                "legacyDrawing",
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
        self.replace_sheet_text(idx, new);
        Ok(drawing)
    }

    /// The pictures and shapes of sheet `idx`.
    pub fn drawings(&mut self, idx: usize) -> Vec<Drawing> {
        let Some(part) = self.sheet_drawing(idx) else {
            return Vec::new();
        };
        let Ok(text) = self
            .pkg
            .part(&part)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
        else {
            return Vec::new();
        };
        anchors(&text)
            .into_iter()
            .map(|a| Drawing {
                name: a.name,
                anchor: [
                    a.from.0,
                    a.from.1,
                    a.to.0.saturating_sub(1).max(a.from.0),
                    a.to.1.saturating_sub(1).max(a.from.1),
                ],
                kind: a.shape.unwrap_or(DrawingKind::Picture),
            })
            .collect()
    }

    /// A picture's image bytes.
    pub fn drawing_image(&mut self, idx: usize, index: usize) -> Option<Vec<u8>> {
        let part = self.sheet_drawing(idx)?;
        let text = String::from_utf8_lossy(&self.pkg.part(&part).ok()?).into_owned();
        let rid = anchors(&text).into_iter().nth(index)?.image?;
        let media = self.rel_target(&part, &rid)?;
        self.pkg.part(&media).ok()
    }

    /// An anchor put into the sheet's drawing (made when it has none), as
    /// one undo step; `make` writes it from the prefix and a free id.
    fn add_anchor(
        &mut self,
        idx: usize,
        make: impl FnOnce(&mut Self, &str, &str, u32) -> Result<String>,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        self.in_one_step(|wb| {
            let drawing = wb.ensure_drawing(idx)?;
            let text = text_of(wb.pkg.part(&drawing)?, &drawing)?;
            let p = prefix_of(&text);
            let id = crate::chart::max_shape_id(&text) + 1;
            let anchor = make(wb, &drawing, &p, id)?;
            let text = text_of(wb.pkg.part(&drawing)?, &drawing)?;
            let close = text.rfind("</").unwrap_or(text.len());
            wb.pkg.set_part(
                &drawing,
                format!("{}{anchor}{}", &text[..close], &text[close..]).into_bytes(),
            );
            wb.generation += 1;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Insert Picture: the image put into `xl/media` and over `anchor`.
    pub fn insert_picture(
        &mut self,
        idx: usize,
        anchor: Range,
        bytes: &[u8],
        ext: &str,
    ) -> Result<()> {
        let (ext, mime) = match ext.to_ascii_lowercase().as_str() {
            "png" => ("png", "image/png"),
            "jpg" | "jpeg" => ("jpeg", "image/jpeg"),
            "gif" => ("gif", "image/gif"),
            other => {
                return Err(Error::Refused(format!(
                    "Not a picture Excel shows: .{other}"
                )));
            }
        };
        let bytes = bytes.to_vec();
        let (from, to) = (
            (anchor.start.row, anchor.start.col),
            (anchor.end.row, anchor.end.col),
        );
        self.add_anchor(idx, |wb, drawing, p, id| {
            let media = wb.free_part_ext("xl/media/image", ext);
            wb.pkg.set_part(&media, bytes);
            wb.ensure_default_type(ext, mime)?;
            let rid = wb.add_rel(drawing, "image", &media)?;
            Ok(format!(
                "<{p}twoCellAnchor editAs=\"oneCell\">{}<{p}pic><{p}nvPicPr><{p}cNvPr id=\"{id}\" name=\"Picture {}\"/><{p}cNvPicPr><a:picLocks xmlns:a=\"{A}\" noChangeAspect=\"1\"/></{p}cNvPicPr></{p}nvPicPr><{p}blipFill><a:blip xmlns:a=\"{A}\" xmlns:r=\"{R}\" r:embed=\"{rid}\"/><a:stretch xmlns:a=\"{A}\"><a:fillRect/></a:stretch></{p}blipFill><{p}spPr><a:prstGeom xmlns:a=\"{A}\" prst=\"rect\"><a:avLst/></a:prstGeom></{p}spPr></{p}pic><{p}clientData/></{p}twoCellAnchor>",
                corners(p, from, to),
                id - 1
            ))
        })
    }

    /// Insert Shape or Text Box over `anchor`, with `text`.
    pub fn insert_shape(
        &mut self,
        idx: usize,
        anchor: Range,
        preset: &str,
        text: &str,
        text_box: bool,
    ) -> Result<()> {
        let (from, to) = (
            (anchor.start.row, anchor.start.col),
            (anchor.end.row, anchor.end.col),
        );
        let preset = if preset.is_empty() { "rect" } else { preset }.to_owned();
        let text = text.to_owned();
        self.add_anchor(idx, |_, _, p, id| {
            let (name, nv, fill, line) = if text_box {
                ("TextBox", " txBox=\"1\"", "FFFFFF", "000000")
            } else {
                ("Shape", "", "4472C4", "2F528F")
            };
            Ok(format!(
                "<{p}twoCellAnchor>{}<{p}sp macro=\"\" textlink=\"\"><{p}nvSpPr><{p}cNvPr id=\"{id}\" name=\"{name} {}\"/><{p}cNvSpPr{nv}/></{p}nvSpPr><{p}spPr><a:prstGeom xmlns:a=\"{A}\" prst=\"{}\"><a:avLst/></a:prstGeom><a:solidFill xmlns:a=\"{A}\"><a:srgbClr val=\"{fill}\"/></a:solidFill><a:ln xmlns:a=\"{A}\" w=\"12700\"><a:solidFill><a:srgbClr val=\"{line}\"/></a:solidFill></a:ln></{p}spPr>{}</{p}sp><{p}clientData/></{p}twoCellAnchor>",
                corners(p, from, to),
                id - 1,
                xml::escape(&preset),
                text_body(p, &text)
            ))
        })
    }

    /// A drawing's anchor element changed by `edit`, as one undo step.
    fn edit_anchor(
        &mut self,
        idx: usize,
        index: usize,
        edit: impl FnOnce(&mut Self, &str, &str, &Anchor) -> Result<String>,
    ) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("The sheet has no pictures or shapes".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let a = anchors(&text)
            .into_iter()
            .nth(index)
            .ok_or_else(|| Error::Refused("No such picture or shape".into()))?;
        self.in_one_step(|wb| {
            let el = text[a.span.clone()].to_owned();
            let new = edit(wb, &drawing, &el, &a)?;
            wb.pkg.set_part(
                &drawing,
                splice(&text, vec![(a.span.clone(), new)]).into_bytes(),
            );
            wb.generation += 1;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Moves or sizes a picture or shape to cover `to`'s cells.
    pub fn move_drawing(&mut self, idx: usize, index: usize, to: Range) -> Result<()> {
        if to.end.row >= MAX_ROW || to.end.col >= MAX_COL {
            return Err(Error::Refused("A picture stays inside the sheet".into()));
        }
        self.edit_anchor(idx, index, |_, _, el, _| {
            Ok(crate::chart::moved_anchor(
                el,
                (to.start.row, to.start.col),
                (to.end.row, to.end.col),
            ))
        })
    }

    /// A shape's text replaced.
    pub fn set_shape_text(&mut self, idx: usize, index: usize, text: &str) -> Result<()> {
        let text = text.to_owned();
        self.edit_anchor(idx, index, |_, _, el, a| {
            if a.shape.is_none() {
                return Err(Error::Refused("A picture has no text".into()));
            }
            let p = el
                .trim_start_matches('<')
                .split(['>', ' '])
                .next()
                .map_or("", xml::prefix)
                .to_owned();
            let body = text_body(&p, &text);
            Ok(
                match (
                    el.find(&format!("<{p}txBody")),
                    el.find(&format!("</{p}txBody>")),
                ) {
                    (Some(s), Some(e)) => {
                        format!(
                            "{}{body}{}",
                            &el[..s],
                            &el[e + format!("</{p}txBody>").len()..]
                        )
                    }
                    _ => el.replacen(&format!("</{p}sp>"), &format!("{body}</{p}sp>"), 1),
                },
            )
        })
    }

    /// Deletes a picture (its image too) or a shape.
    pub fn delete_drawing(&mut self, idx: usize, index: usize) -> Result<()> {
        self.edit_anchor(idx, index, |wb, drawing, _, a| {
            if let Some(rid) = &a.image
                && let Some(media) = wb.rel_target(drawing, rid)
            {
                wb.remove_rel_of(drawing, rid)?;
                wb.remove_part_and_type(&media)?;
            }
            Ok(String::new())
        })
    }
}
