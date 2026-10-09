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

/// The Office theme as Office writes it into a new file since 2023 (Word,
/// Excel and PowerPoint alike): its colors, Aptos Display for headings
/// and Aptos for the body, and the format scheme Office's shapes take.
/// The script fonts are left to the application, as the scheme allows.
pub const OFFICE: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
    "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Office Theme\">",
    "<a:themeElements><a:clrScheme name=\"Office\">",
    "<a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>",
    "<a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>",
    "<a:dk2><a:srgbClr val=\"0E2841\"/></a:dk2><a:lt2><a:srgbClr val=\"E8E8E8\"/></a:lt2>",
    "<a:accent1><a:srgbClr val=\"156082\"/></a:accent1><a:accent2><a:srgbClr val=\"E97132\"/></a:accent2>",
    "<a:accent3><a:srgbClr val=\"196B24\"/></a:accent3><a:accent4><a:srgbClr val=\"0F9ED5\"/></a:accent4>",
    "<a:accent5><a:srgbClr val=\"A02B93\"/></a:accent5><a:accent6><a:srgbClr val=\"4EA72E\"/></a:accent6>",
    "<a:hlink><a:srgbClr val=\"467886\"/></a:hlink><a:folHlink><a:srgbClr val=\"96607D\"/></a:folHlink>",
    "</a:clrScheme><a:fontScheme name=\"Office\">",
    "<a:majorFont><a:latin typeface=\"Aptos Display\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>",
    "<a:minorFont><a:latin typeface=\"Aptos\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont>",
    "</a:fontScheme><a:fmtScheme name=\"Office\"><a:fillStyleLst>",
    "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>",
    "<a:gradFill rotWithShape=\"1\"><a:gsLst>",
    "<a:gs pos=\"0\"><a:schemeClr val=\"phClr\"><a:lumMod val=\"110000\"/><a:satMod val=\"105000\"/><a:tint val=\"67000\"/></a:schemeClr></a:gs>",
    "<a:gs pos=\"50000\"><a:schemeClr val=\"phClr\"><a:lumMod val=\"105000\"/><a:satMod val=\"103000\"/><a:tint val=\"73000\"/></a:schemeClr></a:gs>",
    "<a:gs pos=\"100000\"><a:schemeClr val=\"phClr\"><a:lumMod val=\"105000\"/><a:satMod val=\"109000\"/><a:tint val=\"81000\"/></a:schemeClr></a:gs>",
    "</a:gsLst><a:lin ang=\"5400000\" scaled=\"0\"/></a:gradFill>",
    "<a:gradFill rotWithShape=\"1\"><a:gsLst>",
    "<a:gs pos=\"0\"><a:schemeClr val=\"phClr\"><a:satMod val=\"103000\"/><a:lumMod val=\"102000\"/><a:tint val=\"94000\"/></a:schemeClr></a:gs>",
    "<a:gs pos=\"50000\"><a:schemeClr val=\"phClr\"><a:satMod val=\"110000\"/><a:lumMod val=\"100000\"/><a:shade val=\"100000\"/></a:schemeClr></a:gs>",
    "<a:gs pos=\"100000\"><a:schemeClr val=\"phClr\"><a:lumMod val=\"99000\"/><a:satMod val=\"120000\"/><a:shade val=\"78000\"/></a:schemeClr></a:gs>",
    "</a:gsLst><a:lin ang=\"5400000\" scaled=\"0\"/></a:gradFill>",
    "</a:fillStyleLst><a:lnStyleLst>",
    "<a:ln w=\"12700\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>",
    "<a:ln w=\"19050\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>",
    "<a:ln w=\"25400\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>",
    "</a:lnStyleLst><a:effectStyleLst>",
    "<a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle>",
    "<a:effectStyle><a:effectLst><a:outerShdw blurRad=\"57150\" dist=\"19050\" dir=\"5400000\" algn=\"ctr\" rotWithShape=\"0\"><a:srgbClr val=\"000000\"><a:alpha val=\"63000\"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle>",
    "</a:effectStyleLst><a:bgFillStyleLst>",
    "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>",
    "<a:solidFill><a:schemeClr val=\"phClr\"><a:tint val=\"95000\"/><a:satMod val=\"170000\"/></a:schemeClr></a:solidFill>",
    "<a:gradFill rotWithShape=\"1\"><a:gsLst>",
    "<a:gs pos=\"0\"><a:schemeClr val=\"phClr\"><a:tint val=\"93000\"/><a:satMod val=\"150000\"/><a:shade val=\"98000\"/><a:lumMod val=\"102000\"/></a:schemeClr></a:gs>",
    "<a:gs pos=\"50000\"><a:schemeClr val=\"phClr\"><a:tint val=\"98000\"/><a:satMod val=\"130000\"/><a:shade val=\"90000\"/><a:lumMod val=\"103000\"/></a:schemeClr></a:gs>",
    "<a:gs pos=\"100000\"><a:schemeClr val=\"phClr\"><a:shade val=\"63000\"/><a:satMod val=\"120000\"/></a:schemeClr></a:gs>",
    "</a:gsLst><a:lin ang=\"5400000\" scaled=\"0\"/></a:gradFill>",
    "</a:bgFillStyleLst></a:fmtScheme></a:themeElements>",
    "<a:objectDefaults/><a:extraClrSchemeLst/></a:theme>",
);

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
    fn the_office_theme() {
        let t = parse(OFFICE);
        assert_eq!(t.colors.len(), 12);
        assert_eq!(t.color("lt1"), Some(0xFFFFFF));
        assert_eq!(t.color("accent1"), Some(0x156082));
        assert_eq!(t.color("folHlink"), Some(0x96607D));
        assert_eq!(t.major.latin, "Aptos Display");
        assert_eq!(t.minor.latin, "Aptos");
    }

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
