//! Formatting properties as WordprocessingML writes them: a run's
//! (`w:rPr`, ECMA-376 part 1, 17.3.2), a paragraph's (`w:pPr`, 17.3.1), a
//! table's, a row's and a cell's (17.4). Each property is optional, so
//! that a style, a numbering level or direct formatting says only what it
//! sets, and [`RunProps::merge`] and [`ParaProps::merge`] lay one level
//! over another as 17.7.2 does.

use std::ops::Range;

use kalem_ooxml::xml::{self, Reader, Tag, Token};

/// A byte range of a part.
pub type Span = Range<usize>;

/// An RGB color, `0xRRGGBB`.
pub type Rgb = u32;

/// An on/off value (ST_OnOff): `w:val` absent is on.
pub(crate) fn on_off(tag: &Tag<'_>) -> bool {
    !matches!(tag.attr("val").as_deref(), Some("0" | "false" | "off"))
}

fn int(tag: &Tag<'_>, name: &str) -> Option<i64> {
    let v = tag.attr(name)?;
    let v = v.trim();
    // Some producers write decimals where integers are due (`720.0`).
    v.parse::<i64>()
        .ok()
        .or_else(|| v.parse::<f64>().ok().map(|f| f.round() as i64))
}

fn hex_byte(tag: &Tag<'_>, name: &str) -> Option<u8> {
    u8::from_str_radix(tag.attr(name)?.trim(), 16).ok()
}

/// Calls `f` for each child element of `tag` (whose start tag was just
/// read), and leaves `r` after its end tag. `f` returns whether it read
/// the child to its end; a child it did not is skipped.
pub(crate) fn children<'a>(
    r: &mut Reader<'a>,
    tag: &Tag<'a>,
    mut f: impl FnMut(&mut Reader<'a>, Tag<'a>) -> bool,
) {
    if tag.empty {
        return;
    }
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(child) => {
                let empty = child.empty;
                if !f(r, child) && !empty {
                    r.skip_element();
                }
            }
            Token::End { .. } => return,
            Token::Text { .. } => {}
        }
    }
}

/// The text of the element whose start tag `tag` was just read, nested
/// markup dropped; its content's span; and where its end tag ends.
pub(crate) fn text_of<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> (String, Span, usize) {
    if tag.empty {
        let e = tag.span.end;
        return (String::new(), e..e, e);
    }
    let start = tag.span.end;
    let mut out = String::new();
    let mut depth = 0usize;
    while let Some(t) = r.next_token() {
        match t {
            Token::Text { raw, .. } => out.push_str(&xml::text(raw)),
            Token::Start(t) if !t.empty => depth += 1,
            Token::Start(_) => {}
            Token::End { span, .. } => {
                if depth == 0 {
                    return (out, start..span.start, span.end);
                }
                depth -= 1;
            }
        }
    }
    let end = r.pos();
    (out, start..end, end)
}

/// A color as WordprocessingML gives one: an RGB value or `auto`, and the
/// theme color it was taken from, with its tint or shade.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Color {
    /// The RGB value written (`w:val`, `w:fill`); `None` for `auto` or
    /// none.
    pub rgb: Option<Rgb>,
    /// `auto`: the application's choice (black text, no fill).
    pub auto: bool,
    /// The theme color (`accent1`, `text1`).
    pub theme: Option<String>,
    /// Its tint, 0 to 255 (255 is the color itself).
    pub tint: Option<u8>,
    /// Its shade, 0 to 255.
    pub shade: Option<u8>,
}

impl Color {
    /// Reads a color from four attributes of a tag (`val`, `themeColor`,
    /// `themeTint`, `themeShade` for `w:color`).
    fn read(tag: &Tag<'_>, val: &str, theme: &str, tint: &str, shade: &str) -> Option<Color> {
        let v = tag.attr(val);
        let theme = tag.attr(theme).map(|t| t.into_owned());
        if v.is_none() && theme.is_none() {
            return None;
        }
        let auto = v.as_deref().is_some_and(|v| v.eq_ignore_ascii_case("auto"));
        let rgb = v
            .filter(|_| !auto)
            .and_then(|v| u32::from_str_radix(v.trim(), 16).ok());
        Some(Color {
            rgb,
            auto,
            theme: theme.filter(|t| t != "none"),
            tint: hex_byte(tag, tint),
            shade: hex_byte(tag, shade),
        })
    }
}

/// Shading (`w:shd`, 17.3.5): a pattern in a color over a fill.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shading {
    /// The pattern (`clear`, `solid`, `pct10`…).
    pub pattern: String,
    /// The fill behind the pattern.
    pub fill: Option<Color>,
    /// The pattern's color.
    pub color: Option<Color>,
}

impl Shading {
    fn read(tag: &Tag<'_>) -> Shading {
        Shading {
            pattern: tag.attr("val").unwrap_or_default().into_owned(),
            fill: Color::read(tag, "fill", "themeFill", "themeFillTint", "themeFillShade"),
            color: Color::read(tag, "color", "themeColor", "themeTint", "themeShade"),
        }
    }
}

/// A border line (17.3.4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Border {
    /// The line (`single`, `double`, `dashed`, `nil`, `none`…).
    pub style: String,
    /// Its width in eighths of a point.
    pub size: u32,
    /// Its distance from the text, in points.
    pub space: u32,
    /// Its color.
    pub color: Option<Color>,
}

impl Border {
    fn read(tag: &Tag<'_>) -> Border {
        Border {
            style: tag.attr("val").unwrap_or_default().into_owned(),
            size: int(tag, "sz").unwrap_or(0).max(0) as u32,
            space: int(tag, "space").unwrap_or(0).max(0) as u32,
            color: Color::read(tag, "color", "themeColor", "themeTint", "themeShade"),
        }
    }

    /// Whether a line is drawn.
    pub fn drawn(&self) -> bool {
        !matches!(self.style.as_str(), "nil" | "none" | "")
    }
}

/// The borders of a paragraph, a table or a cell, by side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Borders {
    /// Top.
    pub top: Option<Border>,
    /// Left (`left` or `start`).
    pub left: Option<Border>,
    /// Bottom.
    pub bottom: Option<Border>,
    /// Right (`right` or `end`).
    pub right: Option<Border>,
    /// Between paragraphs of the same borders.
    pub between: Option<Border>,
    /// Between rows of a table.
    pub inside_h: Option<Border>,
    /// Between columns of a table.
    pub inside_v: Option<Border>,
}

impl Borders {
    fn read<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> Borders {
        let mut b = Borders::default();
        children(r, tag, |_, c| {
            let v = Some(Border::read(&c));
            match c.name {
                "top" => b.top = v,
                "left" | "start" => b.left = v,
                "bottom" => b.bottom = v,
                "right" | "end" => b.right = v,
                "between" => b.between = v,
                "insideH" => b.inside_h = v,
                "insideV" => b.inside_v = v,
                _ => {}
            }
            false
        });
        b
    }

    fn merge(&mut self, o: &Borders) {
        for (s, v) in [
            (&mut self.top, &o.top),
            (&mut self.left, &o.left),
            (&mut self.bottom, &o.bottom),
            (&mut self.right, &o.right),
            (&mut self.between, &o.between),
            (&mut self.inside_h, &o.inside_h),
            (&mut self.inside_v, &o.inside_v),
        ] {
            if v.is_some() {
                s.clone_from(v);
            }
        }
    }

    /// Whether no side is given.
    pub fn is_empty(&self) -> bool {
        *self == Borders::default()
    }
}

/// The toggle properties (17.7.3): on in a style, they turn the value
/// they inherit over rather than set it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    /// `w:b`.
    Bold,
    /// `w:bCs`.
    BoldCs,
    /// `w:i`.
    Italic,
    /// `w:iCs`.
    ItalicCs,
    /// `w:caps`.
    Caps,
    /// `w:smallCaps`.
    SmallCaps,
    /// `w:strike`.
    Strike,
    /// `w:outline`.
    Outline,
    /// `w:shadow`.
    Shadow,
    /// `w:emboss`.
    Emboss,
    /// `w:imprint`.
    Imprint,
    /// `w:vanish`: hidden text.
    Vanish,
}

impl Toggle {
    /// Every toggle property.
    pub const ALL: [Toggle; 12] = [
        Toggle::Bold,
        Toggle::BoldCs,
        Toggle::Italic,
        Toggle::ItalicCs,
        Toggle::Caps,
        Toggle::SmallCaps,
        Toggle::Strike,
        Toggle::Outline,
        Toggle::Shadow,
        Toggle::Emboss,
        Toggle::Imprint,
        Toggle::Vanish,
    ];

    fn by_name(name: &str) -> Option<Toggle> {
        Some(match name {
            "b" => Toggle::Bold,
            "bCs" => Toggle::BoldCs,
            "i" => Toggle::Italic,
            "iCs" => Toggle::ItalicCs,
            "caps" => Toggle::Caps,
            "smallCaps" => Toggle::SmallCaps,
            "strike" => Toggle::Strike,
            "outline" => Toggle::Outline,
            "shadow" => Toggle::Shadow,
            "emboss" => Toggle::Emboss,
            "imprint" => Toggle::Imprint,
            "vanish" => Toggle::Vanish,
            _ => return None,
        })
    }
}

/// A run's fonts (`w:rFonts`, 17.3.2.26), by script, each a typeface or
/// a theme font (`minorHAnsi`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fonts {
    /// Basic Latin.
    pub ascii: Option<String>,
    /// Other Latin and most other scripts.
    pub h_ansi: Option<String>,
    /// East Asian scripts.
    pub east_asia: Option<String>,
    /// Complex scripts (Arabic, Hebrew, Thai…).
    pub cs: Option<String>,
    /// The theme font for `ascii`, which wins over it.
    pub ascii_theme: Option<String>,
    /// The theme font for `h_ansi`.
    pub h_ansi_theme: Option<String>,
    /// The theme font for `east_asia`.
    pub east_asia_theme: Option<String>,
    /// The theme font for `cs`.
    pub cs_theme: Option<String>,
}

impl Fonts {
    fn read(tag: &Tag<'_>) -> Fonts {
        let a = |n: &str| tag.attr(n).map(|v| v.into_owned());
        Fonts {
            ascii: a("ascii"),
            h_ansi: a("hAnsi"),
            east_asia: a("eastAsia"),
            cs: a("cs"),
            ascii_theme: a("asciiTheme"),
            h_ansi_theme: a("hAnsiTheme"),
            east_asia_theme: a("eastAsiaTheme"),
            cs_theme: a("cstheme"),
        }
    }

    /// Lays `o` over these fonts, slot by slot: a typeface given clears
    /// the theme font it inherits, which would otherwise win over it.
    fn merge(&mut self, o: &Fonts) {
        let slots = [
            (
                &mut self.ascii,
                &mut self.ascii_theme,
                &o.ascii,
                &o.ascii_theme,
            ),
            (
                &mut self.h_ansi,
                &mut self.h_ansi_theme,
                &o.h_ansi,
                &o.h_ansi_theme,
            ),
            (
                &mut self.east_asia,
                &mut self.east_asia_theme,
                &o.east_asia,
                &o.east_asia_theme,
            ),
            (&mut self.cs, &mut self.cs_theme, &o.cs, &o.cs_theme),
        ];
        for (face, theme, oface, otheme) in slots {
            if oface.is_some() {
                face.clone_from(oface);
                if otheme.is_none() {
                    *theme = None;
                }
            }
            if otheme.is_some() {
                theme.clone_from(otheme);
            }
        }
    }
}

/// A run's properties (`w:rPr`), each as written or absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunProps {
    /// The character style (`w:rStyle`), by its ID.
    pub style: Option<String>,
    /// The fonts.
    pub fonts: Fonts,
    /// The toggle properties, in [`Toggle::ALL`]'s order.
    pub toggles: [Option<bool>; 12],
    /// Double strikethrough (`w:dstrike`), not a toggle.
    pub dstrike: Option<bool>,
    /// Hidden on the web (`w:webHidden`).
    pub web_hidden: Option<bool>,
    /// Right to left (`w:rtl`).
    pub rtl: Option<bool>,
    /// Complex script formatting (`w:cs`).
    pub cs: Option<bool>,
    /// The text color.
    pub color: Option<Color>,
    /// The size in half-points (`w:sz`).
    pub size: Option<u32>,
    /// The complex scripts' size (`w:szCs`).
    pub size_cs: Option<u32>,
    /// The highlight, by name (`yellow`, `none`).
    pub highlight: Option<String>,
    /// The underline's kind (`single`, `double`, `none`…) and color.
    pub underline: Option<(String, Option<Color>)>,
    /// `superscript`, `subscript` or `baseline`.
    pub vert_align: Option<String>,
    /// The run's shading.
    pub shading: Option<Shading>,
    /// Letter spacing in twentieths of a point.
    pub spacing: Option<i64>,
    /// Raised (positive) or lowered, in half-points.
    pub position: Option<i64>,
    /// Width scale, in percent.
    pub scale: Option<u32>,
    /// The language of Latin text (`w:lang w:val`).
    pub lang: Option<String>,
    /// The language of East Asian text.
    pub lang_east_asia: Option<String>,
    /// The language of complex script text.
    pub lang_bidi: Option<String>,
}

impl RunProps {
    /// A toggle property as written.
    pub fn toggle(&self, t: Toggle) -> Option<bool> {
        self.toggles[t as usize]
    }

    /// Sets a toggle property.
    pub fn set_toggle(&mut self, t: Toggle, v: Option<bool>) {
        self.toggles[t as usize] = v;
    }

    /// Reads one child of a `w:rPr`; whether it was a run property.
    fn read_child(&mut self, c: &Tag<'_>) -> bool {
        if let Some(t) = Toggle::by_name(c.name) {
            self.toggles[t as usize] = Some(on_off(c));
            return true;
        }
        let val = || c.attr("val").map(|v| v.into_owned());
        match c.name {
            "rStyle" => self.style = val(),
            "rFonts" => self.fonts.merge(&Fonts::read(c)),
            "dstrike" => self.dstrike = Some(on_off(c)),
            "webHidden" => self.web_hidden = Some(on_off(c)),
            "rtl" => self.rtl = Some(on_off(c)),
            "cs" => self.cs = Some(on_off(c)),
            "color" => self.color = Color::read(c, "val", "themeColor", "themeTint", "themeShade"),
            "sz" => self.size = int(c, "val").map(|v| v.max(0) as u32),
            "szCs" => self.size_cs = int(c, "val").map(|v| v.max(0) as u32),
            "highlight" => self.highlight = val(),
            "u" => {
                self.underline = Some((
                    val().unwrap_or_else(|| "single".into()),
                    Color::read(c, "color", "themeColor", "themeTint", "themeShade"),
                ));
            }
            "vertAlign" => self.vert_align = val(),
            "shd" => self.shading = Some(Shading::read(c)),
            "spacing" => self.spacing = int(c, "val"),
            "position" => self.position = int(c, "val"),
            "w" => self.scale = int(c, "val").map(|v| v.max(0) as u32),
            "lang" => {
                self.lang = c.attr("val").map(|v| v.into_owned()).or(self.lang.take());
                self.lang_east_asia = c
                    .attr("eastAsia")
                    .map(|v| v.into_owned())
                    .or(self.lang_east_asia.take());
                self.lang_bidi = c
                    .attr("bidi")
                    .map(|v| v.into_owned())
                    .or(self.lang_bidi.take());
            }
            _ => return false,
        }
        true
    }

    /// Lays `o` over these properties: what it gives wins (toggles
    /// included; their combining across style kinds is the resolver's).
    pub fn merge(&mut self, o: &RunProps) {
        if o.style.is_some() {
            self.style.clone_from(&o.style);
        }
        self.fonts.merge(&o.fonts);
        for (s, v) in self.toggles.iter_mut().zip(o.toggles) {
            if v.is_some() {
                *s = v;
            }
        }
        macro_rules! over {
            ($($f:ident),*) => {$(
                if o.$f.is_some() {
                    self.$f.clone_from(&o.$f);
                }
            )*};
        }
        over!(
            dstrike,
            web_hidden,
            rtl,
            cs,
            color,
            size,
            size_cs,
            highlight,
            underline,
            vert_align,
            shading,
            spacing,
            position,
            scale,
            lang,
            lang_east_asia,
            lang_bidi
        );
    }
}

/// What a `w:rPr` holds beyond the run's properties: the change to them
/// that is tracked, and for a paragraph mark's `w:rPr`, whether the mark
/// itself was inserted or deleted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunPropsRead {
    /// The properties.
    pub props: RunProps,
    /// A tracked change of formatting (`w:rPrChange`).
    pub change: Option<Revision>,
    /// The paragraph mark inserted (`w:ins` in a mark's properties).
    pub inserted: Option<Revision>,
    /// The paragraph mark deleted.
    pub deleted: Option<Revision>,
}

/// Reads a `w:rPr` whose start tag was just read.
pub(crate) fn read_rpr<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> RunPropsRead {
    let mut out = RunPropsRead::default();
    children(r, tag, |_, c| {
        match c.name {
            "rPrChange" => out.change = Some(Revision::read(&c)),
            "ins" | "moveTo" => out.inserted = Some(Revision::read(&c)),
            "del" | "moveFrom" => out.deleted = Some(Revision::read(&c)),
            _ => {
                out.props.read_child(&c);
            }
        }
        false
    });
    out
}

/// A tracked change's author, date and identifier (17.13).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Revision {
    /// `w:id`.
    pub id: String,
    /// `w:author`.
    pub author: String,
    /// `w:date`, as written (ISO 8601).
    pub date: Option<String>,
}

impl Revision {
    pub(crate) fn read(tag: &Tag<'_>) -> Revision {
        Revision {
            id: tag.attr("id").unwrap_or_default().into_owned(),
            author: tag.attr("author").unwrap_or_default().into_owned(),
            date: tag.attr("date").map(|d| d.into_owned()),
        }
    }
}

/// A tab stop (`w:tab` of `w:tabs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabStop {
    /// `left` (`start`), `center`, `right` (`end`), `decimal`, `bar`,
    /// `clear`.
    pub kind: String,
    /// Its position in twentieths of a point.
    pub pos: i64,
    /// The leader (`dot`, `hyphen`, `none`…).
    pub leader: Option<String>,
}

/// A paragraph's properties (`w:pPr`), each as written or absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParaProps {
    /// The paragraph style, by its ID.
    pub style: Option<String>,
    /// The numbering instance (`w:numPr/w:numId`); `0` takes it away.
    pub num_id: Option<String>,
    /// The list level (`w:numPr/w:ilvl`), from 0.
    pub ilvl: Option<u8>,
    /// The alignment (`left`, `start`, `center`, `right`, `end`, `both`,
    /// `distribute`).
    pub jc: Option<String>,
    /// The left (start) indent in twentieths of a point.
    pub ind_left: Option<i64>,
    /// The right (end) indent.
    pub ind_right: Option<i64>,
    /// The first line's indent: positive for `firstLine`, negative for
    /// `hanging`.
    pub ind_first: Option<i64>,
    /// Space before, in twentieths of a point.
    pub space_before: Option<i64>,
    /// Space after.
    pub space_after: Option<i64>,
    /// Line spacing: in 240ths of a line when `line_rule` is `auto`, else
    /// in twentieths of a point.
    pub line: Option<i64>,
    /// `auto`, `exact` or `atLeast`.
    pub line_rule: Option<String>,
    /// No space between paragraphs of the same style.
    pub contextual: Option<bool>,
    /// The outline level, from 0 (a heading's level less one); 9 is body
    /// text.
    pub outline: Option<u8>,
    /// Kept with the next paragraph.
    pub keep_next: Option<bool>,
    /// Kept on one page.
    pub keep_lines: Option<bool>,
    /// A page break before it.
    pub page_break_before: Option<bool>,
    /// Widow and orphan control.
    pub widow: Option<bool>,
    /// Right to left.
    pub bidi: Option<bool>,
    /// Its borders.
    pub borders: Borders,
    /// Its shading.
    pub shading: Option<Shading>,
    /// Its tab stops, a `clear` one taking away an inherited stop.
    pub tabs: Vec<TabStop>,
    /// A text frame (`w:framePr`).
    pub frame: Option<bool>,
}

impl ParaProps {
    /// Reads one child of a `w:pPr`; whether it was a paragraph property
    /// read to its end.
    fn read_child<'a>(&mut self, r: &mut Reader<'a>, c: &Tag<'a>) -> bool {
        let val = || c.attr("val").map(|v| v.into_owned());
        match c.name {
            "pStyle" => self.style = val(),
            "numPr" => {
                children(r, c, |_, n| {
                    match n.name {
                        "ilvl" => self.ilvl = int(&n, "val").map(|v| v.clamp(0, 8) as u8),
                        "numId" => self.num_id = n.attr("val").map(|v| v.into_owned()),
                        _ => {}
                    }
                    false
                });
                return true;
            }
            "jc" => self.jc = val(),
            "ind" => {
                if let Some(v) = int(c, "left").or_else(|| int(c, "start")) {
                    self.ind_left = Some(v);
                }
                if let Some(v) = int(c, "right").or_else(|| int(c, "end")) {
                    self.ind_right = Some(v);
                }
                if let Some(v) = int(c, "hanging") {
                    self.ind_first = Some(-v);
                } else if let Some(v) = int(c, "firstLine") {
                    self.ind_first = Some(v);
                }
            }
            "spacing" => {
                if let Some(v) = int(c, "before") {
                    self.space_before = Some(v);
                }
                if let Some(v) = int(c, "after") {
                    self.space_after = Some(v);
                }
                if let Some(v) = int(c, "line") {
                    self.line = Some(v);
                }
                if let Some(v) = c.attr("lineRule") {
                    self.line_rule = Some(v.into_owned());
                }
            }
            "contextualSpacing" => self.contextual = Some(on_off(c)),
            "outlineLvl" => self.outline = int(c, "val").map(|v| v.clamp(0, 9) as u8),
            "keepNext" => self.keep_next = Some(on_off(c)),
            "keepLines" => self.keep_lines = Some(on_off(c)),
            "pageBreakBefore" => self.page_break_before = Some(on_off(c)),
            "widowControl" => self.widow = Some(on_off(c)),
            "bidi" => self.bidi = Some(on_off(c)),
            "pBdr" => {
                let b = Borders::read(r, c);
                self.borders.merge(&b);
                return true;
            }
            "shd" => self.shading = Some(Shading::read(c)),
            "tabs" => {
                children(r, c, |_, t| {
                    if t.name == "tab" {
                        self.tabs.push(TabStop {
                            kind: t.attr("val").unwrap_or_default().into_owned(),
                            pos: int(&t, "pos").unwrap_or(0),
                            leader: t.attr("leader").map(|v| v.into_owned()),
                        });
                    }
                    false
                });
                return true;
            }
            "framePr" => self.frame = Some(true),
            _ => return false,
        }
        false
    }

    /// Lays `o` over these properties.
    pub fn merge(&mut self, o: &ParaProps) {
        macro_rules! over {
            ($($f:ident),*) => {$(
                if o.$f.is_some() {
                    self.$f.clone_from(&o.$f);
                }
            )*};
        }
        over!(
            style,
            num_id,
            ilvl,
            jc,
            ind_left,
            ind_right,
            ind_first,
            space_before,
            space_after,
            line,
            line_rule,
            contextual,
            outline,
            keep_next,
            keep_lines,
            page_break_before,
            widow,
            bidi,
            shading,
            frame
        );
        self.borders.merge(&o.borders);
        for t in &o.tabs {
            self.tabs.retain(|s| s.pos != t.pos);
            if t.kind != "clear" {
                self.tabs.push(t.clone());
            }
        }
        self.tabs.sort_by_key(|t| t.pos);
    }
}

/// What a `w:pPr` holds beyond the paragraph's properties.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParaPropsRead {
    /// The properties.
    pub props: ParaProps,
    /// The paragraph mark's run properties (`w:pPr/w:rPr`) and their span.
    pub mark: Option<(RunPropsRead, Span)>,
    /// The section this paragraph ends (`w:pPr/w:sectPr`), its span.
    pub sect: Option<Span>,
    /// A tracked change of the paragraph's properties (`w:pPrChange`).
    pub change: Option<Revision>,
}

/// Reads a `w:pPr` whose start tag was just read.
pub(crate) fn read_ppr<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> ParaPropsRead {
    let mut out = ParaPropsRead::default();
    children(r, tag, |r, c| match c.name {
        "rPr" => {
            let start = c.span.start;
            let read = read_rpr(r, &c);
            let end = if c.empty { c.span.end } else { r.pos() };
            out.mark = Some((read, start..end));
            true
        }
        "sectPr" => {
            let start = c.span.start;
            let end = if c.empty {
                c.span.end
            } else {
                r.skip_element()
            };
            out.sect = Some(start..end);
            true
        }
        "pPrChange" => {
            out.change = Some(Revision::read(&c));
            false
        }
        _ => out.props.read_child(r, &c),
    });
    out
}

/// Which of a table style's conditional formats apply (`w:tblLook`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TableLook {
    /// The first row is a header.
    pub first_row: bool,
    /// The last row is a total.
    pub last_row: bool,
    /// The first column stands out.
    pub first_col: bool,
    /// The last column stands out.
    pub last_col: bool,
    /// Rows are not banded.
    pub no_h_band: bool,
    /// Columns are not banded.
    pub no_v_band: bool,
}

impl TableLook {
    fn read(tag: &Tag<'_>) -> TableLook {
        // The bits of the older `w:val` (17.4.56), then the attributes,
        // which win where given.
        let bits = tag
            .attr("val")
            .and_then(|v| u16::from_str_radix(v.trim(), 16).ok())
            .unwrap_or(0);
        let flag =
            |name: &str, bit: u16| tag.attr(name).map_or(bits & bit != 0, |v| on_off_str(&v));
        TableLook {
            first_row: flag("firstRow", 0x0020),
            last_row: flag("lastRow", 0x0040),
            first_col: flag("firstColumn", 0x0080),
            last_col: flag("lastColumn", 0x0100),
            no_h_band: flag("noHBand", 0x0200),
            no_v_band: flag("noVBand", 0x0400),
        }
    }
}

fn on_off_str(v: &str) -> bool {
    !matches!(v, "0" | "false" | "off")
}

/// A width (`w:tblW`, `w:tcW`, 17.18.87): its value and its type
/// (`dxa` in twentieths of a point, `pct` in fiftieths of a percent,
/// `auto`, `nil`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Width {
    /// The value.
    pub w: i64,
    /// Its unit.
    pub kind: String,
}

impl Width {
    fn read(tag: &Tag<'_>) -> Width {
        let kind = tag
            .attr("type")
            .unwrap_or_else(|| "dxa".into())
            .into_owned();
        // `pct` may be written as `50%`.
        let w = tag
            .attr("w")
            .map(|v| {
                let v = v.trim();
                v.strip_suffix('%')
                    .and_then(|p| p.parse::<f64>().ok())
                    .map(|p| (p * 50.0).round() as i64)
                    .or_else(|| v.parse::<f64>().ok().map(|f| f.round() as i64))
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        Width { w, kind }
    }
}

/// A table's properties (`w:tblPr`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableProps {
    /// The table style, by its ID.
    pub style: Option<String>,
    /// The table's width.
    pub width: Option<Width>,
    /// Its alignment (`left`, `center`, `right`).
    pub jc: Option<String>,
    /// Its indent from the margin, in twentieths of a point.
    pub ind: Option<i64>,
    /// Its borders.
    pub borders: Borders,
    /// Its shading.
    pub shading: Option<Shading>,
    /// The cells' margins: top, left, bottom, right.
    pub margins: [Option<i64>; 4],
    /// `fixed` or `autofit`.
    pub layout: Option<String>,
    /// Which conditional formats of its style apply.
    pub look: Option<TableLook>,
    /// Rows per band of the style's row banding.
    pub row_band: Option<u32>,
    /// Columns per band.
    pub col_band: Option<u32>,
    /// A floating table (`w:tblpPr`).
    pub floating: Option<bool>,
}

impl TableProps {
    fn read_child<'a>(&mut self, r: &mut Reader<'a>, c: &Tag<'a>) -> bool {
        match c.name {
            "tblStyle" => self.style = c.attr("val").map(|v| v.into_owned()),
            "tblW" => self.width = Some(Width::read(c)),
            "jc" => self.jc = c.attr("val").map(|v| v.into_owned()),
            "tblInd" => self.ind = int(c, "w"),
            "tblBorders" => {
                let b = Borders::read(r, c);
                self.borders.merge(&b);
                return true;
            }
            "shd" => self.shading = Some(Shading::read(c)),
            "tblCellMar" => {
                children(r, c, |_, m| {
                    let v = int(&m, "w");
                    match m.name {
                        "top" => self.margins[0] = v,
                        "left" | "start" => self.margins[1] = v,
                        "bottom" => self.margins[2] = v,
                        "right" | "end" => self.margins[3] = v,
                        _ => {}
                    }
                    false
                });
                return true;
            }
            "tblLayout" => self.layout = c.attr("type").map(|v| v.into_owned()),
            "tblLook" => self.look = Some(TableLook::read(c)),
            "tblStyleRowBandSize" => self.row_band = int(c, "val").map(|v| v.max(1) as u32),
            "tblStyleColBandSize" => self.col_band = int(c, "val").map(|v| v.max(1) as u32),
            "tblpPr" => self.floating = Some(true),
            _ => return false,
        }
        false
    }

    /// Lays `o` over these properties.
    pub fn merge(&mut self, o: &TableProps) {
        macro_rules! over {
            ($($f:ident),*) => {$(
                if o.$f.is_some() {
                    self.$f.clone_from(&o.$f);
                }
            )*};
        }
        over!(
            style, width, jc, ind, shading, layout, look, row_band, col_band, floating
        );
        self.borders.merge(&o.borders);
        for (s, v) in self.margins.iter_mut().zip(o.margins) {
            if v.is_some() {
                *s = v;
            }
        }
    }
}

/// Reads a `w:tblPr` (or a table style's) whose start tag was just read.
pub(crate) fn read_tblpr<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> TableProps {
    let mut p = TableProps::default();
    children(r, tag, |r, c| p.read_child(r, &c));
    p
}

/// How a cell takes part in a vertical merge (`w:vMerge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMerge {
    /// The first cell of the merged ones.
    Restart,
    /// Merged into the cell above.
    Continue,
}

/// A cell's properties (`w:tcPr`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellProps {
    /// Its width.
    pub width: Option<Width>,
    /// How many grid columns it spans.
    pub grid_span: Option<u32>,
    /// Its part in a vertical merge.
    pub v_merge: Option<VMerge>,
    /// Its shading.
    pub shading: Option<Shading>,
    /// Its borders.
    pub borders: Borders,
    /// `top`, `center` or `bottom`.
    pub v_align: Option<String>,
    /// Its text does not wrap.
    pub no_wrap: Option<bool>,
}

/// Reads a `w:tcPr` whose start tag was just read.
pub(crate) fn read_tcpr<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> CellProps {
    let mut p = CellProps::default();
    children(r, tag, |r, c| {
        match c.name {
            "tcW" => p.width = Some(Width::read(&c)),
            "gridSpan" => p.grid_span = int(&c, "val").map(|v| v.max(1) as u32),
            "vMerge" => {
                p.v_merge = Some(if c.attr("val").as_deref() == Some("restart") {
                    VMerge::Restart
                } else {
                    VMerge::Continue
                });
            }
            "shd" => p.shading = Some(Shading::read(&c)),
            "tcBorders" => {
                p.borders = Borders::read(r, &c);
                return true;
            }
            "vAlign" => p.v_align = c.attr("val").map(|v| v.into_owned()),
            "noWrap" => p.no_wrap = Some(on_off(&c)),
            _ => {}
        }
        false
    });
    p
}

impl CellProps {
    /// Lays `o` over these properties.
    pub fn merge(&mut self, o: &CellProps) {
        macro_rules! over {
            ($($f:ident),*) => {$(
                if o.$f.is_some() {
                    self.$f.clone_from(&o.$f);
                }
            )*};
        }
        over!(width, grid_span, v_merge, shading, v_align, no_wrap);
        self.borders.merge(&o.borders);
    }
}

/// A row's properties (`w:trPr`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowProps {
    /// Repeated at the top of each page.
    pub header: Option<bool>,
    /// Not split across pages.
    pub cant_split: Option<bool>,
    /// Its height in twentieths of a point and its rule (`atLeast`,
    /// `exact`, `auto`).
    pub height: Option<(i64, String)>,
    /// The row inserted, a tracked change.
    pub inserted: Option<Revision>,
    /// The row deleted, a tracked change.
    pub deleted: Option<Revision>,
}

/// Reads a `w:trPr` whose start tag was just read.
pub(crate) fn read_trpr<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> RowProps {
    let mut p = RowProps::default();
    children(r, tag, |_, c| {
        match c.name {
            "tblHeader" => p.header = Some(on_off(&c)),
            "cantSplit" => p.cant_split = Some(on_off(&c)),
            "trHeight" => {
                p.height = Some((
                    int(&c, "val").unwrap_or(0),
                    c.attr("hRule")
                        .unwrap_or_else(|| "atLeast".into())
                        .into_owned(),
                ));
            }
            "ins" => p.inserted = Some(Revision::read(&c)),
            "del" => p.deleted = Some(Revision::read(&c)),
            _ => {}
        }
        false
    });
    p
}

impl RowProps {
    /// Lays `o` over these properties.
    pub fn merge(&mut self, o: &RowProps) {
        macro_rules! over {
            ($($f:ident),*) => {$(
                if o.$f.is_some() {
                    self.$f.clone_from(&o.$f);
                }
            )*};
        }
        over!(header, cant_split, height, inserted, deleted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpr(xml: &str) -> RunPropsRead {
        let mut r = Reader::new(xml);
        let Some(Token::Start(tag)) = r.next_token() else {
            panic!("a start tag")
        };
        read_rpr(&mut r, &tag)
    }

    fn ppr(xml: &str) -> ParaPropsRead {
        let mut r = Reader::new(xml);
        let Some(Token::Start(tag)) = r.next_token() else {
            panic!("a start tag")
        };
        read_ppr(&mut r, &tag)
    }

    #[test]
    fn run_properties() {
        let p = rpr(
            r#"<w:rPr><w:rStyle w:val="Strong"/><w:rFonts w:ascii="Cambria" w:hAnsiTheme="minorHAnsi"/><w:b/><w:i w:val="0"/><w:color w:val="2F5496" w:themeColor="accent1" w:themeShade="BF"/><w:sz w:val="28"/><w:u w:val="double" w:color="FF0000"/><w:highlight w:val="yellow"/><w:vertAlign w:val="superscript"/><w:lang w:val="tr-TR"/><w:rPrChange w:id="3" w:author="A" w:date="2026-10-01T00:00:00Z"><w:rPr/></w:rPrChange></w:rPr>"#,
        );
        let r = &p.props;
        assert_eq!(r.style.as_deref(), Some("Strong"));
        assert_eq!(r.fonts.ascii.as_deref(), Some("Cambria"));
        assert_eq!(r.fonts.h_ansi_theme.as_deref(), Some("minorHAnsi"));
        assert_eq!(r.toggle(Toggle::Bold), Some(true));
        assert_eq!(r.toggle(Toggle::Italic), Some(false));
        let c = r.color.as_ref().unwrap();
        assert_eq!(
            (c.rgb, c.theme.as_deref(), c.shade),
            (Some(0x2F5496), Some("accent1"), Some(0xBF))
        );
        assert_eq!(r.size, Some(28));
        assert_eq!(r.underline.as_ref().unwrap().0, "double");
        assert_eq!(r.highlight.as_deref(), Some("yellow"));
        assert_eq!(r.vert_align.as_deref(), Some("superscript"));
        assert_eq!(r.lang.as_deref(), Some("tr-TR"));
        assert_eq!(p.change.as_ref().unwrap().author, "A");
    }

    #[test]
    fn fonts_given_clear_the_theme_inherited() {
        let mut base = rpr(
            r#"<w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi"/></w:rPr>"#,
        )
        .props;
        base.merge(&rpr(r#"<w:rPr><w:rFonts w:ascii="Cambria"/></w:rPr>"#).props);
        assert_eq!(base.fonts.ascii.as_deref(), Some("Cambria"));
        assert_eq!(base.fonts.ascii_theme, None);
        assert_eq!(base.fonts.h_ansi_theme.as_deref(), Some("minorHAnsi"));
    }

    #[test]
    fn paragraph_properties() {
        let p = ppr(
            r#"<w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="1"/><w:numId w:val="4"/></w:numPr><w:spacing w:before="240" w:after="0" w:line="360" w:lineRule="auto"/><w:ind w:left="1440" w:hanging="360"/><w:jc w:val="center"/><w:tabs><w:tab w:val="right" w:leader="dot" w:pos="9026"/></w:tabs><w:rPr><w:b/><w:ins w:id="1" w:author="B"/></w:rPr><w:sectPr><w:type w:val="continuous"/></w:sectPr></w:pPr>"#,
        );
        let q = &p.props;
        assert_eq!(q.style.as_deref(), Some("ListParagraph"));
        assert_eq!((q.num_id.as_deref(), q.ilvl), (Some("4"), Some(1)));
        assert_eq!(
            (q.space_before, q.space_after, q.line),
            (Some(240), Some(0), Some(360))
        );
        assert_eq!((q.ind_left, q.ind_first), (Some(1440), Some(-360)));
        assert_eq!(q.jc.as_deref(), Some("center"));
        assert_eq!(q.tabs[0].pos, 9026);
        let (mark, _) = p.mark.as_ref().unwrap();
        assert_eq!(mark.props.toggle(Toggle::Bold), Some(true));
        assert_eq!(mark.inserted.as_ref().unwrap().author, "B");
        assert!(p.sect.is_some());
    }

    #[test]
    fn indents_and_tabs_merge_by_part() {
        let mut base = ppr(r#"<w:pPr><w:ind w:left="720" w:hanging="360"/><w:tabs><w:tab w:val="left" w:pos="720"/><w:tab w:val="left" w:pos="1440"/></w:tabs></w:pPr>"#).props;
        base.merge(&ppr(r#"<w:pPr><w:ind w:firstLine="200"/><w:tabs><w:tab w:val="clear" w:pos="720"/></w:tabs></w:pPr>"#).props);
        assert_eq!((base.ind_left, base.ind_first), (Some(720), Some(200)));
        assert_eq!(base.tabs.len(), 1);
        assert_eq!(base.tabs[0].pos, 1440);
    }

    #[test]
    fn table_look_from_bits_and_attributes() {
        let mut r = Reader::new(
            r#"<w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="5000" w:type="pct"/><w:tblLook w:val="04A0"/></w:tblPr>"#,
        );
        let Some(Token::Start(tag)) = r.next_token() else {
            panic!()
        };
        let p = read_tblpr(&mut r, &tag);
        let look = p.look.unwrap();
        assert!(look.first_row && look.first_col && look.no_v_band && !look.last_row);
        assert_eq!(p.width.unwrap().kind, "pct");
    }
}
