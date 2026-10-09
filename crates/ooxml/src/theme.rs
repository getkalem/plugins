//! The theme part (`theme1.xml`, DrawingML, ECMA-376 part 1, 20.1.6):
//! its color scheme and its font scheme, which cells, runs and shapes
//! name instead of giving a color or a typeface.

use crate::xml::{Reader, Token};

/// An RGB color, `0xRRGGBB`.
pub type Rgb = u32;

/// The names of the color scheme's twelve colors, in `clrScheme` order.
pub const COLOR_NAMES: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// A typeface of the font scheme: the Latin, East Asian and complex
/// script ones (empty when the theme leaves them to the script's).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fonts {
    /// `a:latin`.
    pub latin: String,
    /// `a:ea`.
    pub east_asian: String,
    /// `a:cs`.
    pub complex: String,
    /// `a:font script="…"`: the typeface for a script (`Jpan`, `Arab`).
    pub scripts: Vec<(String, String)>,
}

/// A theme's colors and fonts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Theme {
    /// The color scheme in `clrScheme` order (dk1, lt1, dk2, lt2,
    /// accent1–6, hlink, folHlink).
    pub colors: Vec<Rgb>,
    /// The headings' fonts (`a:majorFont`).
    pub major: Fonts,
    /// The body's fonts (`a:minorFont`).
    pub minor: Fonts,
}

impl Theme {
    /// A color by its scheme name (`accent1`, `dk1`).
    pub fn color(&self, name: &str) -> Option<Rgb> {
        let i = COLOR_NAMES.iter().position(|n| *n == name)?;
        self.colors.get(i).copied()
    }
}

fn hex(v: &str) -> Option<Rgb> {
    u32::from_str_radix(v.trim(), 16).ok()
}

/// The color scheme in `clrScheme` order: `srgbClr` values, a `sysClr`'s
/// last color (window white, text black when the file gives none).
pub fn colors(xml: &str) -> Vec<Rgb> {
    parse(xml).colors
}

/// Reads a theme part.
pub fn parse(xml: &str) -> Theme {
    let mut theme = Theme::default();
    let mut r = Reader::new(xml);
    let mut in_scheme = false;
    // 1 inside majorFont, 2 inside minorFont.
    let mut font = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "clrScheme" && !tag.empty => in_scheme = true,
            Token::End {
                name: "clrScheme", ..
            } => in_scheme = false,
            Token::Start(tag) if in_scheme && tag.name == "srgbClr" => {
                theme
                    .colors
                    .push(tag.attr("val").and_then(|v| hex(&v)).unwrap_or(0));
            }
            Token::Start(tag) if in_scheme && tag.name == "sysClr" => {
                theme
                    .colors
                    .push(tag.attr("lastClr").and_then(|v| hex(&v)).unwrap_or(
                        if tag.attr("val").as_deref() == Some("window") {
                            0xFFFFFF
                        } else {
                            0
                        },
                    ));
            }
            Token::Start(tag) if tag.name == "majorFont" && !tag.empty => font = 1,
            Token::Start(tag) if tag.name == "minorFont" && !tag.empty => font = 2,
            Token::End {
                name: "majorFont" | "minorFont",
                ..
            } => font = 0,
            Token::Start(tag) if font != 0 => {
                let f = if font == 1 {
                    &mut theme.major
                } else {
                    &mut theme.minor
                };
                let face = || tag.attr("typeface").unwrap_or_default().into_owned();
                match tag.name {
                    "latin" => f.latin = face(),
                    "ea" => f.east_asian = face(),
                    "cs" => f.complex = face(),
                    "font" => f
                        .scripts
                        .push((tag.attr("script").unwrap_or_default().into_owned(), face())),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    theme
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEME: &str = r#"<a:theme xmlns:a="x" name="Office"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/><a:font script="Jpan" typeface="游ゴシック Light"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;

    #[test]
    fn colors_and_fonts() {
        let t = parse(THEME);
        assert_eq!(t.colors, [0x000000, 0xFFFFFF, 0x44546A, 0xE7E6E6, 0x4472C4]);
        assert_eq!(t.color("accent1"), Some(0x4472C4));
        assert_eq!(t.color("accent2"), None);
        assert_eq!(t.major.latin, "Calibri Light");
        assert_eq!(t.minor.latin, "Calibri");
        assert_eq!(
            t.major.scripts,
            [("Jpan".into(), "游ゴシック Light".into())]
        );
        assert_eq!(colors(THEME).len(), 5);
    }
}
