//! Named cell styles (`<cellStyles>`, their formats in `<cellStyleXfs>`):
//! Excel's built-in ones made when first used, the user's own made of a
//! cell's format; a cell given one takes its format and names it
//! (`xfId`). The workbook's theme (`xl/theme/theme1.xml`): its colors and
//! fonts, one of Office's themes chosen.

use super::*;

/// A built-in style: its name, Excel's `builtinId`, and its format: bold,
/// italic, size, font color, fill, the bottom and top lines (style and
/// color), the number format.
struct Builtin {
    name: &'static str,
    id: u32,
    bold: bool,
    italic: bool,
    size: Option<f64>,
    color: Option<&'static str>,
    fill: Option<&'static str>,
    bottom: Option<(&'static str, &'static str)>,
    top: Option<(&'static str, &'static str)>,
    box_line: Option<(&'static str, &'static str)>,
    num_fmt: u32,
}

const fn style(name: &'static str, id: u32) -> Builtin {
    Builtin {
        name,
        id,
        bold: false,
        italic: false,
        size: None,
        color: None,
        fill: None,
        bottom: None,
        top: None,
        box_line: None,
        num_fmt: 0,
    }
}

const BUILTIN: &[Builtin] = &[
    style("Normal", 0),
    Builtin {
        num_fmt: 43,
        ..style("Comma", 3)
    },
    Builtin {
        num_fmt: 44,
        ..style("Currency", 4)
    },
    Builtin {
        num_fmt: 9,
        ..style("Percent", 5)
    },
    Builtin {
        fill: Some("FFFFCC"),
        box_line: Some(("thin", "B2B2B2")),
        ..style("Note", 10)
    },
    Builtin {
        color: Some("FF0000"),
        ..style("Warning Text", 11)
    },
    Builtin {
        bold: true,
        size: Some(18.0),
        color: Some("44546A"),
        ..style("Title", 15)
    },
    Builtin {
        bold: true,
        size: Some(15.0),
        color: Some("44546A"),
        bottom: Some(("thick", "4472C4")),
        ..style("Heading 1", 16)
    },
    Builtin {
        bold: true,
        size: Some(13.0),
        color: Some("44546A"),
        bottom: Some(("thick", "A2B8E1")),
        ..style("Heading 2", 17)
    },
    Builtin {
        bold: true,
        color: Some("44546A"),
        bottom: Some(("medium", "8EA9DB")),
        ..style("Heading 3", 18)
    },
    Builtin {
        bold: true,
        color: Some("44546A"),
        ..style("Heading 4", 19)
    },
    Builtin {
        color: Some("3F3F76"),
        fill: Some("FFCC99"),
        box_line: Some(("thin", "7F7F7F")),
        ..style("Input", 20)
    },
    Builtin {
        bold: true,
        color: Some("3F3F3F"),
        fill: Some("F2F2F2"),
        box_line: Some(("thin", "3F3F3F")),
        ..style("Output", 21)
    },
    Builtin {
        bold: true,
        color: Some("FA7D00"),
        fill: Some("F2F2F2"),
        box_line: Some(("thin", "7F7F7F")),
        ..style("Calculation", 22)
    },
    Builtin {
        bold: true,
        color: Some("FFFFFF"),
        fill: Some("A5A5A5"),
        box_line: Some(("double", "3F3F3F")),
        ..style("Check Cell", 23)
    },
    Builtin {
        color: Some("FA7D00"),
        bottom: Some(("double", "FF8001")),
        ..style("Linked Cell", 24)
    },
    Builtin {
        bold: true,
        top: Some(("thin", "4472C4")),
        bottom: Some(("double", "4472C4")),
        ..style("Total", 25)
    },
    Builtin {
        color: Some("006100"),
        fill: Some("C6EFCE"),
        ..style("Good", 26)
    },
    Builtin {
        color: Some("9C0006"),
        fill: Some("FFC7CE"),
        ..style("Bad", 27)
    },
    Builtin {
        color: Some("9C5700"),
        fill: Some("FFEB9C"),
        ..style("Neutral", 28)
    },
    Builtin {
        italic: true,
        color: Some("7F7F7F"),
        ..style("Explanatory Text", 53)
    },
];

/// A theme: its name, its colors (dark 2, light 2, accents 1 to 6, the
/// link and the followed link) and its heading and body fonts.
struct Theme {
    name: &'static str,
    colors: [&'static str; 10],
    major: &'static str,
    minor: &'static str,
}

const THEMES: &[Theme] = &[
    Theme {
        name: "Office",
        colors: [
            "0E2841", "E8E8E8", "156082", "E97132", "196B24", "0F9ED5", "A02B93", "4EA72E",
            "467886", "96607D",
        ],
        major: "Aptos Display",
        minor: "Aptos Narrow",
    },
    Theme {
        name: "Office 2013 - 2022",
        colors: [
            "44546A", "E7E6E6", "4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47",
            "0563C1", "954F72",
        ],
        major: "Calibri Light",
        minor: "Calibri",
    },
    Theme {
        name: "Office 2007 - 2010",
        colors: [
            "1F497D", "EEECE1", "4F81BD", "C0504D", "9BBB59", "8064A2", "4BACC6", "F79646",
            "0000FF", "800080",
        ],
        major: "Cambria",
        minor: "Calibri",
    },
    Theme {
        name: "Blue",
        colors: [
            "17406D", "DBEFF9", "0F6FC6", "009DD9", "0BD0D9", "10CF9B", "7CCA62", "A5C249",
            "F49100", "85DFD0",
        ],
        major: "Calibri",
        minor: "Calibri",
    },
    Theme {
        name: "Green",
        colors: [
            "455F51", "E3DED1", "549E39", "8AB833", "C0CF3A", "029676", "4AB5C4", "0989B1",
            "6B9F25", "BA6906",
        ],
        major: "Calibri",
        minor: "Calibri",
    },
    Theme {
        name: "Red",
        colors: [
            "323232", "E5C243", "A5300F", "D55816", "E19825", "B19C7D", "7F5F52", "B27D49",
            "6B9F25", "B26B02",
        ],
        major: "Calibri",
        minor: "Calibri",
    },
    Theme {
        name: "Violet",
        colors: [
            "373545", "DCD8DC", "AD84C6", "8784C7", "5D739A", "6997AF", "84ACB6", "6FAA9F",
            "B292CA", "6B56A3",
        ],
        major: "Calibri",
        minor: "Calibri",
    },
    Theme {
        name: "Grayscale",
        colors: [
            "000000", "F8F8F8", "DDDDDD", "B2B2B2", "969696", "808080", "5F5F5F", "4D4D4D",
            "5F5F5F", "919191",
        ],
        major: "Calibri",
        minor: "Calibri",
    },
];

/// A style list's elements' attribute `k` (as text), one per element.
fn names_of(text: &str) -> Vec<(String, usize)> {
    let mut r = Reader::new(text);
    let mut out = Vec::new();
    let mut inside = false;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "cellStyles" => inside = !tag.empty,
            Token::End {
                name: "cellStyles", ..
            } => inside = false,
            Token::Start(tag) if inside && tag.name == "cellStyle" => {
                if let Some(n) = tag.attr("name") {
                    let x = tag.attr("xfId").and_then(|v| v.parse().ok()).unwrap_or(0);
                    out.push((n.into_owned(), x));
                }
            }
            _ => {}
        }
    }
    out
}

/// An `<xf>`'s attribute as a number.
fn attr_of(xf: &str, k: &str) -> usize {
    xf.split(&format!(" {k}=\""))
        .nth(1)
        .and_then(|v| v.split('"').next()?.parse().ok())
        .unwrap_or(0)
}

impl Workbook {
    /// The workbook's named styles.
    pub fn cell_styles(&self) -> Vec<String> {
        self.styles_now()
            .map(|(_, t)| names_of(&t).into_iter().map(|n| n.0).collect())
            .unwrap_or_default()
    }

    /// The `cellStyleXfs` index of style `name`, made when it is built in
    /// and not there yet.
    fn style_xf(&mut self, text: &mut String, name: &str) -> Result<usize> {
        if let Some((_, x)) = names_of(text)
            .into_iter()
            .find(|n| n.0.eq_ignore_ascii_case(name))
        {
            return Ok(x);
        }
        let Some(b) = BUILTIN.iter().find(|b| b.name.eq_ignore_ascii_case(name)) else {
            return Err(Error::Refused(format!("No cell style {name}")));
        };
        // Its font: the workbook's first, with its own weight, size, color.
        let fonts = style_children(text, "fonts").unwrap_or_default();
        let base = fonts
            .first()
            .map_or("<font/>".to_owned(), |s| text[s.clone()].to_owned());
        let p = xml::prefix(
            base.trim_start_matches('<')
                .split([' ', '/', '>'])
                .next()
                .unwrap_or(""),
        )
        .to_owned();
        let mut f = base;
        if b.bold {
            f = crate::chart::set_child(&f, &["b"], &format!("<{p}b/>"), &[]);
        }
        if b.italic {
            f = crate::chart::set_child(&f, &["i"], &format!("<{p}i/>"), &["b"]);
        }
        if let Some(sz) = b.size {
            f = crate::chart::set_child(
                &f,
                &["sz"],
                &format!("<{p}sz val=\"{sz}\"/>"),
                &["b", "i", "strike", "u"],
            );
        }
        if let Some(c) = b.color {
            f = crate::chart::set_child(
                &f,
                &["color"],
                &format!("<{p}color rgb=\"FF{c}\"/>"),
                &["b", "i", "strike", "u", "sz"],
            );
        }
        let (t, font) = add_style_child(text, "fonts", &f);
        *text = t;
        let fill = match b.fill {
            Some(c) => {
                let el = format!(
                    "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{c}\"/><bgColor indexed=\"64\"/></patternFill></fill>"
                );
                let (t, id) = add_style_child(text, "fills", &el);
                *text = t;
                id
            }
            None => 0,
        };
        let side = |n: &str, l: Option<(&str, &str)>| match l {
            Some((st, c)) => format!("<{n} style=\"{st}\"><color rgb=\"FF{c}\"/></{n}>"),
            None => format!("<{n}/>"),
        };
        let border = if b.bottom.is_some() || b.top.is_some() || b.box_line.is_some() {
            let el = format!(
                "<border>{}{}{}{}<diagonal/></border>",
                side("left", b.box_line),
                side("right", b.box_line),
                side("top", b.top.or(b.box_line)),
                side("bottom", b.bottom.or(b.box_line))
            );
            let (t, id) = add_style_child(text, "borders", &el);
            *text = t;
            id
        } else {
            0
        };
        let xf = format!(
            "<xf numFmtId=\"{}\" fontId=\"{font}\" fillId=\"{fill}\" borderId=\"{border}\"/>",
            b.num_fmt
        );
        let (t, x) = add_style_child(text, "cellStyleXfs", &xf);
        *text = t;
        let (t, _) = add_style_child(
            text,
            "cellStyles",
            &format!(
                "<cellStyle name=\"{}\" xfId=\"{x}\" builtinId=\"{}\"/>",
                b.name, b.id
            ),
        );
        *text = t;
        Ok(x)
    }

    /// The `cellXfs` index of a cell format that is style `x`'s, its
    /// format the style's.
    fn xf_of_style(text: &mut String, x: usize) -> usize {
        let xfs = style_children(text, "cellStyleXfs").unwrap_or_default();
        let sx = xfs
            .get(x)
            .map_or("<xf/>".to_owned(), |s| text[s.clone()].to_owned());
        let mut xf = format!(
            "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"{}\" xfId=\"{x}\"/>",
            attr_of(&sx, "numFmtId"),
            attr_of(&sx, "fontId"),
            attr_of(&sx, "fillId"),
            attr_of(&sx, "borderId")
        );
        // Its alignment too.
        if let Some(a) = sx
            .find("<alignment")
            .and_then(|s| sx[s..].find("/>").map(|e| &sx[s..s + e + 2]))
        {
            xf = xf.replacen("/>", &format!(">{a}</xf>"), 1);
        }
        let (t, id) = add_style_child(text, "cellXfs", &xf);
        *text = t;
        id
    }

    /// A range's cells given style `name` (their format the style's). One
    /// undo step.
    pub fn apply_cell_style(&mut self, idx: usize, range: Range, name: &str) -> Result<()> {
        self.load(idx)?;
        self.check_allowed(idx, "formatCells")?;
        let area = u64::from(range.end.row - range.start.row + 1)
            * u64::from(range.end.col - range.start.col + 1);
        if area > 200_000 {
            return Err(Error::Refused("Select fewer cells: 200,000 at most".into()));
        }
        self.in_one_step(|wb| {
            let (_, mut text) = wb.styles_now()?;
            let x = wb.style_xf(&mut text, name)?;
            let id = Self::xf_of_style(&mut text, x) as u32;
            wb.styles = styles::parse(&text, &wb.theme);
            wb.styles_xml = Some(text);
            let cells: Vec<(CellRef, u32)> = (range.start.row..=range.end.row)
                .flat_map(|r| {
                    (range.start.col..=range.end.col).map(move |c| (CellRef::new(r, c), id))
                })
                .collect();
            wb.style_many(idx, &cells)?;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// A new style `name` made of cell `at`'s format, the cell given it.
    /// One undo step.
    pub fn new_cell_style(&mut self, name: &str, idx: usize, at: CellRef) -> Result<()> {
        self.load(idx)?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 255 {
            return Err(Error::Refused(
                "A style's name is 1 to 255 characters".into(),
            ));
        }
        if self
            .cell_styles()
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
            || BUILTIN.iter().any(|b| b.name.eq_ignore_ascii_case(name))
        {
            return Err(Error::Refused(format!("There is a style named {name}")));
        }
        let style = self.loaded[&idx].1.cells.get(&at).map_or(0, |c| c.style) as usize;
        self.in_one_step(|wb| {
            let (_, mut text) = wb.styles_now()?;
            let xfs = style_children(&text, "cellXfs").unwrap_or_default();
            let cx = xfs
                .get(style)
                .map_or("<xf/>".to_owned(), |s| text[s.clone()].to_owned());
            let mut sx = format!(
                "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"{}\"/>",
                attr_of(&cx, "numFmtId"),
                attr_of(&cx, "fontId"),
                attr_of(&cx, "fillId"),
                attr_of(&cx, "borderId")
            );
            if let Some(a) = cx
                .find("<alignment")
                .and_then(|s| cx[s..].find("/>").map(|e| &cx[s..s + e + 2]))
            {
                sx = sx.replacen("/>", &format!(">{a}</xf>"), 1);
            }
            let (t, x) = add_style_child(&text, "cellStyleXfs", &sx);
            text = t;
            let (t, _) = add_style_child(
                &text,
                "cellStyles",
                &format!("<cellStyle name=\"{}\" xfId=\"{x}\"/>", xml::escape(name)),
            );
            text = t;
            let id = Self::xf_of_style(&mut text, x) as u32;
            wb.styles = styles::parse(&text, &wb.theme);
            wb.styles_xml = Some(text);
            wb.style_many(idx, &[(at, id)])?;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// The theme part, if the workbook has one.
    fn theme_part(&self) -> Option<String> {
        self.workbook_rels
            .iter()
            .find(|r| r.kind == "theme" && !r.external)
            .map(|r| rels::resolve(&self.workbook_part, &r.target))
            .filter(|p| self.pkg.contains(p))
    }

    /// The theme's name.
    pub fn theme_name(&self) -> Option<String> {
        let p = self.theme_part()?;
        let text = text_of(self.pkg.part(&p).ok()?, &p).ok()?;
        let mut r = Reader::new(&text);
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "theme"
            {
                return tag.attr("name").map(|v| v.into_owned());
            }
        }
        None
    }

    /// The themes a workbook can be given.
    pub fn theme_names() -> Vec<String> {
        THEMES.iter().map(|t| t.name.to_owned()).collect()
    }

    /// The theme's colors read again (after a change or an undo).
    pub(crate) fn reload_theme(&mut self) {
        if let Some(p) = self.theme_part()
            && let Ok(b) = self.pkg.part(&p)
            && let Ok(t) = text_of(b, &p)
        {
            self.theme = styles::parse_theme(&t);
            if let Ok((_, s)) = self.styles_now() {
                self.styles = styles::parse(&s, &self.theme);
            }
        }
    }

    /// The workbook given theme `name`: its color scheme and its fonts.
    /// One undo step.
    pub fn set_theme(&mut self, name: &str) -> Result<()> {
        self.in_one_step(|wb| wb.set_theme_now(name))
    }

    fn set_theme_now(&mut self, name: &str) -> Result<()> {
        let Some(theme) = THEMES.iter().find(|t| t.name.eq_ignore_ascii_case(name)) else {
            return Err(Error::Refused(format!("No theme {name}")));
        };
        let part = match self.theme_part() {
            Some(p) => p,
            None => {
                // A workbook without a theme gets one, as Excel gives it.
                let p = self.free_part("xl/theme/theme");
                self.add_part(
                    &p,
                    blank_theme(),
                    "application/vnd.openxmlformats-officedocument.theme+xml",
                )?;
                let wb = self.workbook_part.clone();
                self.add_rel(&wb, "theme", &p)?;
                p
            }
        };
        let text = text_of(self.pkg.part(&part)?, &part)?;
        let mut r = Reader::new(&text);
        let mut splices = Vec::new();
        // Which scheme color is open, and where the fonts are.
        let order = [
            "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
            "hlink", "folHlink",
        ];
        let mut slot: Option<usize> = None;
        let mut font: Option<&str> = None;
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else {
                if let Token::End { name, .. } = t
                    && order.contains(&name)
                {
                    slot = None;
                }
                continue;
            };
            match tag.name {
                "theme" => splices.push((
                    tag.span.clone(),
                    xml::set_attr(&text[tag.span.clone()], "name", theme.name),
                )),
                "clrScheme" => splices.push((
                    tag.span.clone(),
                    xml::set_attr(&text[tag.span.clone()], "name", theme.name),
                )),
                "fontScheme" => splices.push((
                    tag.span.clone(),
                    xml::set_attr(&text[tag.span.clone()], "name", theme.name),
                )),
                n if order.contains(&n) => slot = order.iter().position(|o| *o == n),
                "srgbClr" | "sysClr" if slot.is_some() => {
                    let p = xml::prefix(tag.qname);
                    let c = theme.colors[slot.unwrap_or(0)];
                    let el = format!("<{p}srgbClr val=\"{c}\"/>");
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    splices.push((tag.span.start..end, el));
                    slot = None;
                }
                "majorFont" => font = Some(theme.major),
                "minorFont" => font = Some(theme.minor),
                "latin" if font.is_some() => {
                    splices.push((
                        tag.span.clone(),
                        xml::set_attr(
                            &text[tag.span.clone()],
                            "typeface",
                            font.unwrap_or_default(),
                        ),
                    ));
                    font = None;
                }
                _ => {}
            }
        }
        let new = splice(&text, splices);
        self.pkg.set_part(&part, new.into_bytes());
        self.reload_theme();
        self.generation += 1;
        self.batch_changed = true;
        Ok(())
    }
}

/// A whole theme part (Office 2013 - 2022's), for a workbook without one.
fn blank_theme() -> String {
    let t = &THEMES[1];
    let names = [
        "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink",
        "folHlink",
    ];
    let colors: String = names
        .iter()
        .zip(t.colors)
        .map(|(n, c)| format!("<a:{n}><a:srgbClr val=\"{c}\"/></a:{n}>"))
        .collect();
    let fill = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
    let line = |w: u32| {
        format!(
            "<a:ln w=\"{w}\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/></a:ln>"
        )
    };
    let effect = "<a:effectStyle><a:effectLst/></a:effectStyle>";
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"{name}\"><a:themeElements>\
<a:clrScheme name=\"{name}\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
<a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>{colors}</a:clrScheme>\
<a:fontScheme name=\"{name}\"><a:majorFont><a:latin typeface=\"{major}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>\
<a:minorFont><a:latin typeface=\"{minor}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme>\
<a:fmtScheme name=\"{name}\"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst>\
<a:lnStyleLst>{l1}{l2}{l3}</a:lnStyleLst><a:effectStyleLst>{effect}{effect}{effect}</a:effectStyleLst>\
<a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>",
        name = t.name,
        major = t.major,
        minor = t.minor,
        l1 = line(6350),
        l2 = line(12700),
        l3 = line(19050),
    )
}
