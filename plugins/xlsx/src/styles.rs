//! The style sheet (ECMA-376 part 1, 18.8) as far as the view shows it:
//! number formats, fonts, fills and alignment of each cell format, with
//! theme and indexed colors resolved to RGB.

use std::collections::HashMap;

use crate::numfmt;
use crate::xml::{Reader, Token};

/// An RGB color, `0xRRGGBB`.
pub type Rgb = u32;

/// What the view shows of a cell format (`<xf>` of `cellXfs`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellStyle {
    /// The number format code.
    pub num_fmt: String,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strike: bool,
    /// Font size in points.
    pub size: Option<f64>,
    /// Font family name.
    pub font: Option<String>,
    /// Text color.
    pub color: Option<Rgb>,
    /// Solid fill color.
    pub fill: Option<Rgb>,
    /// `left`, `center`, `right`, `fill`, `justify`, …; `None` is general.
    pub align: Option<String>,
    /// `top`, `center`, `bottom`, …
    pub valign: Option<String>,
    /// Text wraps in the cell.
    pub wrap: bool,
    /// Whether any border is drawn.
    pub border: bool,
    /// A leading apostrophe was typed: the value is text even if it reads as a number.
    pub quote_prefix: bool,
}

#[derive(Debug, Clone, Default)]
struct Font {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    size: Option<f64>,
    name: Option<String>,
    color: Option<Rgb>,
}

/// The parsed style sheet.
#[derive(Debug, Clone, Default)]
pub struct Styles {
    /// One entry per `cellXfs` `<xf>`; a cell's `s` indexes it.
    pub cell: Vec<CellStyle>,
    /// Whether each cell format shows dates.
    date: Vec<bool>,
}

impl Styles {
    /// The style of a cell's `s`, the default when out of range.
    pub fn get(&self, s: u32) -> CellStyle {
        self.cell
            .get(s as usize)
            .cloned()
            .unwrap_or_else(|| CellStyle {
                num_fmt: "General".into(),
                ..CellStyle::default()
            })
    }

    /// Whether the cell format shows dates or times.
    pub fn is_date(&self, s: u32) -> bool {
        self.date.get(s as usize).copied().unwrap_or(false)
    }
}

/// The 64 colors of the legacy indexed palette (18.8.27).
const INDEXED: [Rgb; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000,
    0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x800000, 0x008000,
    0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC,
    0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF,
    0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF,
    0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600,
    0x666699, 0x969696, 0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399,
    0x333333,
];

/// The theme's color scheme in `clrScheme` order (dk1, lt1, dk2, lt2,
/// accent1–6, hlink, folHlink); the style sheet's theme index swaps the
/// first two pairs (0 is lt1, 1 dk1, 2 lt2, 3 dk2), as Excel reads it.
pub fn parse_theme(xml: &str) -> Vec<Rgb> {
    let mut out = Vec::new();
    let mut r = Reader::new(xml);
    let mut in_scheme = false;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "clrScheme" => in_scheme = true,
            Token::End {
                name: "clrScheme", ..
            } => break,
            Token::Start(tag) if in_scheme && tag.name == "srgbClr" => {
                out.push(
                    tag.attr("val")
                        .and_then(|v| u32::from_str_radix(&v, 16).ok())
                        .unwrap_or(0),
                );
            }
            Token::Start(tag) if in_scheme && tag.name == "sysClr" => {
                out.push(
                    tag.attr("lastClr")
                        .and_then(|v| u32::from_str_radix(&v, 16).ok())
                        .unwrap_or(if tag.attr("val").as_deref() == Some("window") {
                            0xFFFFFF
                        } else {
                            0
                        }),
                );
            }
            _ => {}
        }
    }
    if out.len() >= 4 {
        out.swap(0, 1);
        out.swap(2, 3);
    }
    out
}

fn apply_tint(c: Rgb, tint: f64) -> Rgb {
    if tint == 0.0 {
        return c;
    }
    let ch = |shift: u32| -> u32 {
        let v = f64::from((c >> shift) & 0xFF);
        let v = if tint < 0.0 {
            v * (1.0 + tint)
        } else {
            v + (255.0 - v) * tint
        };
        (v.round().clamp(0.0, 255.0) as u32) << shift
    };
    ch(16) | ch(8) | ch(0)
}

fn color(tag: &crate::xml::Tag<'_>, theme: &[Rgb]) -> Option<Rgb> {
    let tint: f64 = tag.attr("tint").and_then(|t| t.parse().ok()).unwrap_or(0.0);
    let base = if let Some(rgb) = tag.attr("rgb") {
        let h = rgb.trim();
        let h = if h.len() == 8 { &h[2..] } else { h };
        u32::from_str_radix(h, 16).ok()
    } else if let Some(t) = tag.attr("theme") {
        t.parse::<usize>().ok().and_then(|i| theme.get(i).copied())
    } else if let Some(i) = tag.attr("indexed") {
        i.parse::<usize>()
            .ok()
            .and_then(|i| INDEXED.get(i).copied())
    } else {
        None
    }?;
    Some(apply_tint(base, tint))
}

fn flag(tag: &crate::xml::Tag<'_>) -> bool {
    !matches!(tag.attr("val").as_deref(), Some("0" | "false" | "none"))
}

/// Parses `styles.xml` with the theme's colors.
pub fn parse(xml: &str, theme: &[Rgb]) -> Styles {
    let mut fmts: HashMap<u32, String> = HashMap::new();
    let mut fonts: Vec<Font> = Vec::new();
    let mut fills: Vec<Option<Rgb>> = Vec::new();
    let mut borders: Vec<bool> = Vec::new();
    let mut styles = Styles::default();
    let mut r = Reader::new(xml);
    // Which list is open: the same `<xf>`, `<font>` and `<color>` tags mean
    // different things inside `cellStyleXfs`, `dxfs` and the rest.
    let mut section = "";
    let mut font: Option<Font> = None;
    let mut fill: Option<Option<Rgb>> = None;
    let mut fill_pattern_solid = false;
    let mut border: Option<bool> = None;
    let mut xf: Option<(CellStyle, u32)> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => match tag.name {
                "numFmts" | "fonts" | "fills" | "borders" | "cellXfs" | "cellStyleXfs" | "dxfs"
                | "extLst" => {
                    if !tag.empty {
                        section = tag.name;
                    }
                }
                "numFmt" if section == "numFmts" => {
                    if let (Some(id), Some(code)) = (tag.attr("numFmtId"), tag.attr("formatCode"))
                        && let Ok(id) = id.parse()
                    {
                        fmts.insert(id, code.into_owned());
                    }
                }
                "font" if section == "fonts" => {
                    font = Some(Font::default());
                    if tag.empty {
                        fonts.push(font.take().unwrap_or_default());
                    }
                }
                "b" if font.is_some() => font.as_mut().map(|f| f.bold = flag(&tag)).unwrap_or(()),
                "i" if font.is_some() => font.as_mut().map(|f| f.italic = flag(&tag)).unwrap_or(()),
                "u" if font.is_some() => font
                    .as_mut()
                    .map(|f| f.underline = flag(&tag))
                    .unwrap_or(()),
                "strike" if font.is_some() => {
                    font.as_mut().map(|f| f.strike = flag(&tag)).unwrap_or(())
                }
                "sz" if font.is_some() => {
                    let v = tag.attr("val").and_then(|v| v.parse().ok());
                    font.as_mut().map(|f| f.size = v).unwrap_or(());
                }
                "name" if font.is_some() => {
                    let v = tag.attr("val").map(|v| v.into_owned());
                    font.as_mut().map(|f| f.name = v).unwrap_or(());
                }
                "color" if font.is_some() => {
                    let c = color(&tag, theme);
                    font.as_mut().map(|f| f.color = c).unwrap_or(());
                }
                "fill" if section == "fills" => {
                    fill = Some(None);
                    fill_pattern_solid = false;
                    if tag.empty {
                        fills.push(None);
                        fill = None;
                    }
                }
                "patternFill" if fill.is_some() => {
                    fill_pattern_solid = tag
                        .attr("patternType")
                        .as_deref()
                        .is_some_and(|p| p != "none");
                }
                "fgColor" if fill.is_some() && fill_pattern_solid => {
                    fill = Some(color(&tag, theme))
                }
                "stop" if fill.is_some() => {}
                "border" if section == "borders" => {
                    border = Some(false);
                    if tag.empty {
                        borders.push(false);
                        border = None;
                    }
                }
                "left" | "right" | "top" | "bottom" | "start" | "end" if border.is_some() => {
                    if tag.attr("style").is_some_and(|s| s != "none") {
                        border = Some(true);
                    }
                }
                "xf" if section == "cellXfs" => {
                    let num = tag
                        .attr("numFmtId")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0u32);
                    let get = |k: &str| {
                        tag.attr(k)
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0)
                    };
                    let f = fonts.get(get("fontId")).cloned().unwrap_or_default();
                    let style = CellStyle {
                        num_fmt: fmts
                            .get(&num)
                            .cloned()
                            .or_else(|| numfmt::builtin(num).map(str::to_owned))
                            .unwrap_or_else(|| "General".into()),
                        bold: f.bold,
                        italic: f.italic,
                        underline: f.underline,
                        strike: f.strike,
                        size: f.size,
                        font: f.name,
                        color: f.color,
                        fill: fills.get(get("fillId")).copied().flatten(),
                        border: borders.get(get("borderId")).copied().unwrap_or(false),
                        quote_prefix: tag
                            .attr("quotePrefix")
                            .as_deref()
                            .is_some_and(|v| v == "1" || v == "true"),
                        ..CellStyle::default()
                    };
                    if tag.empty {
                        push_xf(&mut styles, style);
                    } else {
                        xf = Some((style, 0));
                    }
                }
                "alignment" if xf.is_some() => {
                    if let Some((s, _)) = xf.as_mut() {
                        s.align = tag.attr("horizontal").map(|v| v.into_owned());
                        s.valign = tag.attr("vertical").map(|v| v.into_owned());
                        s.wrap = tag
                            .attr("wrapText")
                            .as_deref()
                            .is_some_and(|v| v == "1" || v == "true");
                    }
                }
                _ => {}
            },
            Token::End { name, .. } => match name {
                "font" if section == "fonts" => {
                    if let Some(f) = font.take() {
                        fonts.push(f);
                    }
                }
                "fill" if section == "fills" => {
                    if let Some(f) = fill.take() {
                        fills.push(f);
                    }
                }
                "border" if section == "borders" => {
                    if let Some(b) = border.take() {
                        borders.push(b);
                    }
                }
                "xf" if section == "cellXfs" => {
                    if let Some((s, _)) = xf.take() {
                        push_xf(&mut styles, s);
                    }
                }
                n if n == section => section = "",
                _ => {}
            },
            Token::Text { .. } => {}
        }
    }
    styles
}

fn push_xf(styles: &mut Styles, s: CellStyle) {
    styles.date.push(numfmt::is_date_format(&s.num_fmt));
    styles.cell.push(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_resolve() {
        let xml = r#"<styleSheet><numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts>
<fonts><font><sz val="11"/><name val="Calibri"/></font><font><b/><color theme="1"/></font></fonts>
<fills><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>
<fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills>
<borders><border/></borders>
<cellStyleXfs><xf numFmtId="0"/></cellStyleXfs>
<cellXfs><xf numFmtId="0"/><xf numFmtId="164" fontId="1" fillId="2"><alignment horizontal="center"/></xf><xf numFmtId="4"/></cellXfs></styleSheet>"#;
        let theme = [0xFFFFFF, 0x000000];
        let s = parse(xml, &theme);
        assert_eq!(s.cell.len(), 3);
        assert!(s.is_date(1));
        assert!(!s.is_date(2));
        let c = s.get(1);
        assert!(c.bold);
        assert_eq!(c.color, Some(0x000000));
        assert_eq!(c.fill, Some(0xFFFF00));
        assert_eq!(c.align.as_deref(), Some("center"));
        assert_eq!(s.get(2).num_fmt, "#,##0.00");
        assert_eq!(s.get(0).size, Some(11.0));
    }

    #[test]
    fn tint() {
        assert_eq!(apply_tint(0x000000, 0.5), 0x808080);
        assert_eq!(apply_tint(0xFFFFFF, -0.5), 0x808080);
    }
}
