//! New documents (WP12a): the package of a blank document as Word makes
//! one, of each of the four kinds, for Kalem's New commands through the
//! contract's `Viewer::new_file` (the `formats` interface's `new-file`).
//!
//! The parts are Word's for a new document: the main part with one empty
//! paragraph and its section, the styles (Word's document defaults and
//! the built-in styles a new document carries: Normal, the headings,
//! Title, Subtitle, Quote, Intense Quote, List Paragraph and their
//! character styles), the settings (Word 2013's compatibility mode, so
//! that Word does not open it in compatibility mode), the web settings,
//! the font table, the Office theme ([`kalem_ooxml::theme::OFFICE`]) and
//! the core and extended properties. Two choices are not Word's, which
//! takes them from the user's locale: the page is A4 with margins of
//! 2.5 cm, and the language is left out, so that Word proofs the text in
//! its own editing language.
//!
//! The contract hands a new file sheets of entries (rows of a text file,
//! made into a workbook): a document writes each sheet that holds any as
//! a table, the sheet's name as a heading above it when there are
//! several, each entry's text as typed (a leading `'` taken off).

use kalem_ooxml::package::Package;
use kalem_viewer::NewSheet;

const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";
const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const NS_W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const NS_W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
/// Word's own settings in `w:compatSetting`.
const URI_WORD: &str = "http://schemas.microsoft.com/office/word";
const CT_WML: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";

/// A4 in twentieths of a point, its margins 2.5 cm, the header and the
/// footer 1.25 cm from the edge: Word's page outside North America.
const PAGE: (u32, u32) = (11906, 16838);
const MARGIN: u32 = 1417;
/// The width text has on the page, which a table's columns share.
const TEXT_WIDTH: u32 = PAGE.0 - 2 * MARGIN;

/// The main part's content type of a document saved as `extension`.
fn main_content_type(extension: &str) -> Option<String> {
    Some(match extension {
        "docx" => format!("{CT_WML}.document.main+xml"),
        "docm" => "application/vnd.ms-word.document.macroEnabled.main+xml".into(),
        "dotx" => format!("{CT_WML}.template.main+xml"),
        "dotm" => "application/vnd.ms-word.template.macroEnabledTemplate.main+xml".into(),
        _ => return None,
    })
}

/// A new document of `extension` (`docx`, `docm`, `dotx`, `dotm`),
/// blank, or holding each of `sheets` that has entries as a table.
pub fn new_document(extension: &str, sheets: &[NewSheet]) -> Result<Vec<u8>, String> {
    let Some(main) = main_content_type(extension) else {
        return Err(format!("A Word document is not a .{extension} file"));
    };
    let body = Body::of(sheets);
    let now = crate::document::now_iso();
    let mut p = Package::new();
    for (name, text) in [
        ("[Content_Types].xml", content_types(&main)),
        ("_rels/.rels", root_rels()),
        ("word/document.xml", document(&body.xml)),
        ("word/_rels/document.xml.rels", document_rels()),
        (
            "word/theme/theme1.xml",
            kalem_ooxml::theme::OFFICE.to_owned(),
        ),
        ("word/settings.xml", settings()),
        ("word/styles.xml", styles(body.tables)),
        ("word/webSettings.xml", web_settings()),
        ("word/fontTable.xml", font_table()),
        ("docProps/core.xml", core(&now)),
        ("docProps/app.xml", app(&body)),
    ] {
        p.set_part(name, text.into_bytes());
    }
    p.write().map_err(|e| e.to_string())
}

/// The namespaces Word declares on a part's root, `mc:Ignorable` naming
/// those a reader may not know.
fn namespaces() -> String {
    format!(
        "xmlns:mc=\"{NS_MC}\" xmlns:r=\"{NS_R}\" xmlns:w=\"{NS_W}\" xmlns:w14=\"{NS_W14}\" xmlns:w15=\"{NS_W15}\" mc:Ignorable=\"w14 w15\""
    )
}

fn content_types(main: &str) -> String {
    let part =
        |name: &str, ct: &str| format!("<Override PartName=\"{name}\" ContentType=\"{ct}\"/>");
    let mut s = format!(
        "{DECLARATION}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>"
    );
    s.push_str(&part("/word/document.xml", main));
    s.push_str(&part("/word/styles.xml", &format!("{CT_WML}.styles+xml")));
    s.push_str(&part(
        "/word/settings.xml",
        &format!("{CT_WML}.settings+xml"),
    ));
    s.push_str(&part(
        "/word/webSettings.xml",
        &format!("{CT_WML}.webSettings+xml"),
    ));
    s.push_str(&part(
        "/word/fontTable.xml",
        &format!("{CT_WML}.fontTable+xml"),
    ));
    s.push_str(&part(
        "/word/theme/theme1.xml",
        "application/vnd.openxmlformats-officedocument.theme+xml",
    ));
    s.push_str(&part(
        "/docProps/core.xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
    ));
    s.push_str(&part(
        "/docProps/app.xml",
        "application/vnd.openxmlformats-officedocument.extended-properties+xml",
    ));
    s.push_str("</Types>");
    s
}

fn relationships(rels: &[(&str, &str, &str)]) -> String {
    let mut s = format!(
        "{DECLARATION}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">"
    );
    for (id, kind, target) in rels {
        s.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{target}\"/>"
        ));
    }
    s.push_str("</Relationships>");
    s
}

fn root_rels() -> String {
    relationships(&[
        (
            "rId3",
            &format!("{NS_R}/extended-properties"),
            "docProps/app.xml",
        ),
        (
            "rId2",
            "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties",
            "docProps/core.xml",
        ),
        (
            "rId1",
            &format!("{NS_R}/officeDocument"),
            "word/document.xml",
        ),
    ])
}

fn document_rels() -> String {
    relationships(&[
        ("rId3", &format!("{NS_R}/webSettings"), "webSettings.xml"),
        ("rId2", &format!("{NS_R}/settings"), "settings.xml"),
        ("rId1", &format!("{NS_R}/styles"), "styles.xml"),
        ("rId5", &format!("{NS_R}/theme"), "theme/theme1.xml"),
        ("rId4", &format!("{NS_R}/fontTable"), "fontTable.xml"),
    ])
}

fn document(body: &str) -> String {
    format!(
        "{DECLARATION}<w:document {}><w:body>{body}<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:top=\"{MARGIN}\" w:right=\"{MARGIN}\" w:bottom=\"{MARGIN}\" w:left=\"{MARGIN}\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/><w:cols w:space=\"708\"/><w:docGrid w:linePitch=\"360\"/></w:sectPr></w:body></w:document>",
        namespaces().replace("xmlns:r=", &format!("xmlns:m=\"{NS_M}\" xmlns:r=")),
        PAGE.0,
        PAGE.1,
    )
}

/// The body before its section: the paragraphs and tables, and what
/// the extended properties count of them.
#[derive(Debug, Default)]
struct Body {
    xml: String,
    tables: bool,
    paragraphs: usize,
    words: usize,
    /// Characters without spaces, and with them.
    chars: (usize, usize),
}

impl Body {
    fn of(sheets: &[NewSheet]) -> Body {
        let mut b = Body::default();
        let filled: Vec<&NewSheet> = sheets
            .iter()
            .filter(|s| s.rows.iter().flatten().any(|e| !e.is_empty()))
            .collect();
        for s in &filled {
            if filled.len() > 1 {
                let name = b.runs(&s.name);
                b.xml.push_str(&format!(
                    "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>{name}</w:p>"
                ));
            }
            b.table(s);
            // A table is followed by a paragraph, as Word keeps one.
            b.xml.push_str("<w:p/>");
        }
        if filled.is_empty() {
            b.xml.push_str("<w:p/>");
        }
        b
    }

    /// A sheet's rows as a table of the page's width, every row as wide
    /// as the widest, its columns equal.
    fn table(&mut self, s: &NewSheet) {
        self.tables = true;
        let last = s
            .rows
            .iter()
            .rposition(|r| r.iter().any(|e| !e.is_empty()))
            .unwrap_or(0);
        let rows = &s.rows[..=last];
        let cols = rows
            .iter()
            .map(|r| r.iter().rposition(|e| !e.is_empty()).map_or(0, |c| c + 1))
            .max()
            .unwrap_or(1)
            .max(1);
        let width = TEXT_WIDTH / cols as u32;
        self.xml.push_str("<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblLook w:val=\"04A0\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"1\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/></w:tblPr><w:tblGrid>");
        for _ in 0..cols {
            self.xml.push_str(&format!("<w:gridCol w:w=\"{width}\"/>"));
        }
        self.xml.push_str("</w:tblGrid>");
        for row in rows {
            self.xml.push_str("<w:tr>");
            for c in 0..cols {
                let entry = row.get(c).map_or("", String::as_str);
                let text = entry.strip_prefix('\'').unwrap_or(entry);
                self.xml.push_str(&format!(
                    "<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/></w:tcPr><w:p>"
                ));
                let runs = self.runs(text);
                self.xml.push_str(&runs);
                self.xml.push_str("</w:p></w:tc>");
            }
            self.xml.push_str("</w:tr>");
        }
        self.xml.push_str("</w:tbl>");
    }

    /// A paragraph's run holding `text`, its line breaks and tabs as
    /// Word's elements, counted for the properties; nothing for no text.
    fn runs(&mut self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        self.paragraphs += 1;
        self.words += text.split_whitespace().count();
        self.chars.0 += text.chars().filter(|c| !c.is_whitespace()).count();
        self.chars.1 += text.chars().filter(|c| *c != '\n' && *c != '\r').count();
        let mut out = String::from("<w:r>");
        let mut piece = String::new();
        let flush = |out: &mut String, piece: &mut String| {
            if piece.is_empty() {
                return;
            }
            let space = if piece.starts_with(' ') || piece.ends_with(' ') {
                " xml:space=\"preserve\""
            } else {
                ""
            };
            out.push_str(&format!("<w:t{space}>{}</w:t>", esc(piece)));
            piece.clear();
        };
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\t' => {
                    flush(&mut out, &mut piece);
                    out.push_str("<w:tab/>");
                }
                '\r' if chars.peek() == Some(&'\n') => {}
                '\n' | '\r' => {
                    flush(&mut out, &mut piece);
                    out.push_str("<w:br/>");
                }
                // Characters XML 1.0 does not allow.
                c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
                c => piece.push(c),
            }
        }
        flush(&mut out, &mut piece);
        out.push_str("</w:r>");
        out
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn settings() -> String {
    let compat = [
        ("compatibilityMode", "15"),
        ("overrideTableStyleFontSizeAndJustification", "1"),
        ("enableOpenTypeFeatures", "1"),
        ("doNotFlipMirrorIndents", "1"),
        ("differentiateMultirowTableHeaders", "1"),
        ("useWord2013TrackBottomHyphenation", "0"),
    ]
    .iter()
    .map(|(name, v)| {
        format!("<w:compatSetting w:name=\"{name}\" w:uri=\"{URI_WORD}\" w:val=\"{v}\"/>")
    })
    .collect::<String>();
    format!(
        "{DECLARATION}<w:settings {}><w:zoom w:percent=\"100\"/><w:defaultTabStop w:val=\"708\"/><w:characterSpacingControl w:val=\"doNotCompress\"/><w:compat>{compat}</w:compat><m:mathPr><m:mathFont m:val=\"Cambria Math\"/><m:brkBin m:val=\"before\"/><m:brkBinSub m:val=\"--\"/><m:smallFrac m:val=\"0\"/><m:dispDef/><m:lMargin m:val=\"0\"/><m:rMargin m:val=\"0\"/><m:defJc m:val=\"centerGroup\"/><m:wrapIndent m:val=\"1440\"/><m:intLim m:val=\"subSup\"/><m:naryLim m:val=\"undOvr\"/></m:mathPr><w:clrSchemeMapping w:bg1=\"light1\" w:t1=\"dark1\" w:bg2=\"light2\" w:t2=\"dark2\" w:accent1=\"accent1\" w:accent2=\"accent2\" w:accent3=\"accent3\" w:accent4=\"accent4\" w:accent5=\"accent5\" w:accent6=\"accent6\" w:hyperlink=\"hyperlink\" w:followedHyperlink=\"followedHyperlink\"/></w:settings>",
        namespaces().replace("xmlns:r=", &format!("xmlns:m=\"{NS_M}\" xmlns:r=")),
    )
}

fn web_settings() -> String {
    format!(
        "{DECLARATION}<w:webSettings {}><w:optimizeForBrowser/><w:allowPNG/></w:webSettings>",
        namespaces()
    )
}

fn font_table() -> String {
    format!(
        "{DECLARATION}<w:fonts {}><w:font w:name=\"Aptos\"><w:charset w:val=\"00\"/><w:family w:val=\"swiss\"/><w:pitch w:val=\"variable\"/></w:font><w:font w:name=\"Times New Roman\"><w:panose1 w:val=\"02020603050405020304\"/><w:charset w:val=\"00\"/><w:family w:val=\"roman\"/><w:pitch w:val=\"variable\"/></w:font><w:font w:name=\"Aptos Display\"><w:charset w:val=\"00\"/><w:family w:val=\"swiss\"/><w:pitch w:val=\"variable\"/></w:font></w:fonts>",
        namespaces()
    )
}

/// The heading color of the Office theme: accent 1, darkened a quarter.
const ACCENT: &str = "<w:color w:val=\"0F4761\" w:themeColor=\"accent1\" w:themeShade=\"BF\"/>";
const MAJOR: &str = "<w:rFonts w:asciiTheme=\"majorHAnsi\" w:eastAsiaTheme=\"majorEastAsia\" w:hAnsiTheme=\"majorHAnsi\" w:cstheme=\"majorBidi\"/>";
const MAJOR_EA: &str = "<w:rFonts w:eastAsiaTheme=\"majorEastAsia\" w:cstheme=\"majorBidi\"/>";

/// A paragraph style and the character style linked to it, which Word
/// gives a style whose look a run can take (`Heading1Char`).
fn linked(id: &str, name: &str, priority: u32, hidden: bool, ppr: &str, rpr: &str) -> String {
    let hide = if hidden {
        "<w:semiHidden/><w:unhideWhenUsed/>"
    } else {
        ""
    };
    format!(
        "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:link w:val=\"{id}Char\"/><w:uiPriority w:val=\"{priority}\"/>{hide}<w:qFormat/><w:pPr>{ppr}</w:pPr><w:rPr>{rpr}</w:rPr></w:style><w:style w:type=\"character\" w:customStyle=\"1\" w:styleId=\"{id}Char\"><w:name w:val=\"{} Char\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:link w:val=\"{id}\"/><w:uiPriority w:val=\"{priority}\"/>{hide}<w:rPr>{rpr}</w:rPr></w:style>",
        // Word names the built-in headings in lower case, shows them
        // capitalized.
        if id.starts_with("Heading") {
            name.to_lowercase()
        } else {
            name.to_owned()
        },
        name,
    )
}

fn styles(tables: bool) -> String {
    let mut s = format!(
        "{DECLARATION}<w:styles {}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme=\"minorHAnsi\" w:eastAsiaTheme=\"minorEastAsia\" w:hAnsiTheme=\"minorHAnsi\" w:cstheme=\"minorBidi\"/><w:kern w:val=\"2\"/><w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/><w14:ligatures w14:val=\"standardContextual\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"278\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
        namespaces()
    );
    s.push_str("<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>");
    // The headings: their space before and after, size, italics and
    // color as Word's, the first two in the headings' font.
    let headings: [(&str, &str, &str, &str); 9] = [
        (
            "360",
            "80",
            MAJOR,
            "<w:sz w:val=\"40\"/><w:szCs w:val=\"40\"/>",
        ),
        (
            "160",
            "80",
            MAJOR,
            "<w:sz w:val=\"32\"/><w:szCs w:val=\"32\"/>",
        ),
        (
            "160",
            "80",
            MAJOR_EA,
            "<w:sz w:val=\"28\"/><w:szCs w:val=\"28\"/>",
        ),
        ("80", "40", MAJOR_EA, "<w:i/><w:iCs/>"),
        ("80", "40", MAJOR_EA, ""),
        ("40", "0", MAJOR_EA, "<w:i/><w:iCs/>"),
        ("40", "0", MAJOR_EA, ""),
        ("0", "0", MAJOR_EA, "<w:i/><w:iCs/>"),
        ("0", "0", MAJOR_EA, ""),
    ];
    for (i, (before, after, fonts, look)) in headings.iter().enumerate() {
        let n = i + 1;
        let color = match n {
            1..=5 => ACCENT,
            6 | 7 => "<w:color w:val=\"595959\" w:themeColor=\"text1\" w:themeTint=\"A6\"/>",
            _ => "<w:color w:val=\"272727\" w:themeColor=\"text1\" w:themeTint=\"D8\"/>",
        };
        // `w:i` comes before `w:color`, `w:sz` after it (17.3.2).
        let (italic, size) = if look.starts_with("<w:i/>") {
            (*look, "")
        } else {
            ("", *look)
        };
        let before = if *before == "0" {
            String::new()
        } else {
            format!(" w:before=\"{before}\"")
        };
        s.push_str(&linked(
            &format!("Heading{n}"),
            &format!("Heading {n}"),
            9,
            n > 1,
            &format!(
                "<w:keepNext/><w:keepLines/><w:spacing{before} w:after=\"{after}\"/><w:outlineLvl w:val=\"{i}\"/>"
            ),
            &format!("{fonts}{italic}{color}{size}"),
        ));
    }
    s.push_str("<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/><w:uiPriority w:val=\"1\"/><w:semiHidden/><w:unhideWhenUsed/></w:style>");
    s.push_str("<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>");
    s.push_str("<w:style w:type=\"numbering\" w:default=\"1\" w:styleId=\"NoList\"><w:name w:val=\"No List\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/></w:style>");
    s.push_str(&linked(
        "Title",
        "Title",
        10,
        false,
        "<w:spacing w:after=\"80\" w:line=\"240\" w:lineRule=\"auto\"/><w:contextualSpacing/>",
        &format!("{MAJOR}<w:spacing w:val=\"-10\"/><w:kern w:val=\"28\"/><w:sz w:val=\"56\"/><w:szCs w:val=\"56\"/>"),
    ));
    s.push_str(&linked(
        "Subtitle",
        "Subtitle",
        11,
        false,
        "<w:spacing w:after=\"160\"/>",
        &format!("{MAJOR_EA}<w:color w:val=\"595959\" w:themeColor=\"text1\" w:themeTint=\"A6\"/><w:spacing w:val=\"15\"/><w:sz w:val=\"28\"/><w:szCs w:val=\"28\"/>"),
    ));
    s.push_str(&linked(
        "Quote",
        "Quote",
        29,
        false,
        "<w:spacing w:before=\"160\"/><w:jc w:val=\"center\"/>",
        "<w:i/><w:iCs/><w:color w:val=\"404040\" w:themeColor=\"text1\" w:themeTint=\"BF\"/>",
    ));
    s.push_str("<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:uiPriority w:val=\"34\"/><w:qFormat/><w:pPr><w:ind w:left=\"720\"/><w:contextualSpacing/></w:pPr></w:style>");
    s.push_str(&format!("<w:style w:type=\"character\" w:styleId=\"IntenseEmphasis\"><w:name w:val=\"Intense Emphasis\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:uiPriority w:val=\"21\"/><w:qFormat/><w:rPr><w:i/><w:iCs/>{ACCENT}</w:rPr></w:style>"));
    let border = |side: &str| {
        format!(
            "<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"10\" w:color=\"0F4761\" w:themeColor=\"accent1\" w:themeShade=\"BF\"/>"
        )
    };
    s.push_str(&linked(
        "IntenseQuote",
        "Intense Quote",
        30,
        false,
        &format!(
            "<w:pBdr>{}{}</w:pBdr><w:spacing w:before=\"360\" w:after=\"360\"/><w:ind w:left=\"864\" w:right=\"864\"/><w:jc w:val=\"center\"/>",
            border("top"),
            border("bottom")
        ),
        &format!("<w:i/><w:iCs/>{ACCENT}"),
    ));
    s.push_str(&format!("<w:style w:type=\"character\" w:styleId=\"IntenseReference\"><w:name w:val=\"Intense Reference\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:uiPriority w:val=\"32\"/><w:qFormat/><w:rPr><w:b/><w:bCs/><w:smallCaps/>{ACCENT}<w:spacing w:val=\"5\"/></w:rPr></w:style>"));
    if tables {
        // Word adds Table Grid with the first table.
        let line = |side: &str| {
            format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>")
        };
        s.push_str(&format!(
            "<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:basedOn w:val=\"TableNormal\"/><w:uiPriority w:val=\"39\"/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:tblPr><w:tblBorders>{}{}{}{}{}{}</w:tblBorders></w:tblPr></w:style>",
            line("top"),
            line("left"),
            line("bottom"),
            line("right"),
            line("insideH"),
            line("insideV"),
        ));
    }
    s.push_str("</w:styles>");
    s
}

fn core(now: &str) -> String {
    format!(
        "{DECLARATION}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><cp:revision>1</cp:revision><dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified></cp:coreProperties>"
    )
}

fn app(b: &Body) -> String {
    format!(
        "{DECLARATION}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><TotalTime>0</TotalTime><Pages>1</Pages><Words>{}</Words><Characters>{}</Characters><Application>Kalem</Application><DocSecurity>0</DocSecurity><Lines>{}</Lines><Paragraphs>{}</Paragraphs><ScaleCrop>false</ScaleCrop><LinksUpToDate>false</LinksUpToDate><CharactersWithSpaces>{}</CharactersWithSpaces><SharedDoc>false</SharedDoc><HyperlinksChanged>false</HyperlinksChanged></Properties>",
        b.words, b.chars.0, b.paragraphs, b.paragraphs, b.chars.1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(bytes: &[u8], name: &str) -> String {
        String::from_utf8(Package::read(bytes.to_vec()).unwrap().part(name).unwrap()).unwrap()
    }

    #[test]
    fn every_kind_declares_itself() {
        for (ext, ct) in [
            ("docx", "wordprocessingml.document.main+xml"),
            ("docm", "document.macroEnabled.main+xml"),
            ("dotx", "wordprocessingml.template.main+xml"),
            ("dotm", "template.macroEnabledTemplate.main+xml"),
        ] {
            let bytes = new_document(ext, &[]).unwrap();
            assert!(part(&bytes, "[Content_Types].xml").contains(ct), "{ext}");
        }
        let e = new_document("odt", &[]).unwrap_err();
        assert!(e.contains(".odt"), "{e}");
    }

    #[test]
    fn the_parts_word_writes() {
        let bytes = new_document("docx", &[]).unwrap();
        let p = Package::read(bytes.clone()).unwrap();
        assert_eq!(
            p.names(),
            [
                "[Content_Types].xml",
                "_rels/.rels",
                "word/document.xml",
                "word/_rels/document.xml.rels",
                "word/theme/theme1.xml",
                "word/settings.xml",
                "word/styles.xml",
                "word/webSettings.xml",
                "word/fontTable.xml",
                "docProps/core.xml",
                "docProps/app.xml",
            ]
        );
        let doc = part(&bytes, "word/document.xml");
        assert!(doc.contains("<w:body><w:p/><w:sectPr>"), "{doc}");
        assert!(doc.contains("<w:pgSz w:w=\"11906\" w:h=\"16838\"/>"));
        let settings = part(&bytes, "word/settings.xml");
        assert!(settings.contains("w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\""));
        let styles = part(&bytes, "word/styles.xml");
        assert!(styles.contains("<w:name w:val=\"heading 1\"/>"));
        assert!(styles.contains("w:styleId=\"Heading9Char\""));
        assert!(!styles.contains("TableGrid"));
        // Every part is well-formed: each start tag closed in order.
        for name in p.names() {
            let text = part(&bytes, &name);
            let mut open: Vec<String> = Vec::new();
            let mut r = kalem_ooxml::xml::Reader::new(&text);
            while let Some(t) = r.next_token() {
                match t {
                    kalem_ooxml::xml::Token::Start(tag) if !tag.empty => {
                        open.push(tag.qname.to_owned());
                    }
                    kalem_ooxml::xml::Token::End { .. } => {
                        assert!(open.pop().is_some(), "{name}");
                    }
                    _ => {}
                }
            }
            assert!(open.is_empty(), "{name}: {open:?} left open");
        }
    }

    #[test]
    fn sheets_as_tables() {
        let sheets = [
            NewSheet {
                name: "Prices".into(),
                rows: vec![
                    vec!["Item".into(), "Price".into()],
                    vec!["Çay & şeker".into(), "'007".into(), String::new()],
                    vec!["two\nlines".into()],
                    vec![],
                ],
            },
            NewSheet {
                name: "Empty".into(),
                rows: vec![vec![String::new()]],
            },
        ];
        let bytes = new_document("docx", &sheets).unwrap();
        let doc = part(&bytes, "word/document.xml");
        // One sheet holds entries: no heading, a table of two columns.
        assert!(!doc.contains("Heading1"), "{doc}");
        assert_eq!(doc.matches("<w:gridCol w:w=\"4536\"/>").count(), 2);
        assert_eq!(doc.matches("<w:tr>").count(), 3);
        assert!(doc.contains("<w:t>Çay &amp; şeker</w:t>"));
        assert!(doc.contains("<w:t>007</w:t>"));
        assert!(doc.contains("<w:t>two</w:t><w:br/><w:t>lines</w:t>"));
        assert!(doc.contains("</w:tbl><w:p/><w:sectPr>"));
        assert!(part(&bytes, "word/styles.xml").contains("w:styleId=\"TableGrid\""));
        assert!(part(&bytes, "docProps/app.xml").contains("<Words>8</Words>"));
        // Two that do: each named by a heading.
        let mut two = sheets.clone();
        two[1].rows = vec![vec!["x".into()]];
        let doc = part(&new_document("docx", &two).unwrap(), "word/document.xml");
        assert_eq!(doc.matches("<w:pStyle w:val=\"Heading1\"/>").count(), 2);
        assert!(doc.contains("<w:t>Empty</w:t>"));
    }
}
