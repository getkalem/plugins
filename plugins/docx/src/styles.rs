//! The style sheet (`styles.xml`, ECMA-376 part 1, 17.7): the document
//! defaults, the paragraph, character, table and numbering styles with
//! their `basedOn` chains, and how a run's and a paragraph's formatting
//! is put together from them (17.7.2) with the toggle properties
//! combined as 17.7.3 says.

use std::cell::RefCell;
use std::collections::HashMap;

use kalem_ooxml::xml::{Reader, Token};

use crate::props::{self, CellProps, ParaProps, RowProps, RunProps, TableProps, Toggle, children};

/// What a style applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleKind {
    /// Paragraphs (and their runs).
    Paragraph,
    /// Runs.
    Character,
    /// Tables.
    Table,
    /// Lists.
    Numbering,
}

/// The parts of a table a conditional format applies to (17.18.89), in
/// the order they are laid over the whole table's.
pub const CONDITIONS: [&str; 12] = [
    "band1Vert",
    "band2Vert",
    "band1Horz",
    "band2Horz",
    "firstCol",
    "lastCol",
    "firstRow",
    "lastRow",
    "neCell",
    "nwCell",
    "seCell",
    "swCell",
];

/// A table style's format for a part of the table (`w:tblStylePr`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableFormat {
    /// Its paragraphs' properties.
    pub ppr: ParaProps,
    /// Its runs' properties.
    pub rpr: RunProps,
    /// The table's.
    pub tblpr: TableProps,
    /// Its rows'.
    pub trpr: RowProps,
    /// Its cells'.
    pub tcpr: CellProps,
}

impl TableFormat {
    fn merge(&mut self, o: &TableFormat) {
        self.ppr.merge(&o.ppr);
        self.rpr.merge(&o.rpr);
        self.tblpr.merge(&o.tblpr);
        self.trpr.merge(&o.trpr);
        self.tcpr.merge(&o.tcpr);
    }
}

/// A style (`w:style`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    /// Its ID, which paragraphs and runs name.
    pub id: String,
    /// What it applies to.
    pub kind: StyleKind,
    /// Its name for people (`heading 1`; Word shows the built-in ones
    /// capitalized).
    pub name: String,
    /// The style it is based on.
    pub based_on: Option<String>,
    /// The style of the paragraph Enter makes after one of this style.
    pub next: Option<String>,
    /// The paragraph or character style linked to it.
    pub link: Option<String>,
    /// The default style of its kind.
    pub default: bool,
    /// Hidden from the styles a user picks.
    pub hidden: bool,
    /// Its paragraph properties (with a numbering style's `w:numPr`).
    pub ppr: ParaProps,
    /// Its run properties.
    pub rpr: RunProps,
    /// A table style's formats: the whole table's first, then each
    /// conditional one by its type.
    pub table: TableFormat,
    /// A table style's conditional formats (`w:tblStylePr`) by type.
    pub conditions: Vec<(String, TableFormat)>,
}

/// The document defaults and the styles.
#[derive(Debug, Default)]
pub struct Styles {
    /// The runs' defaults (`w:rPrDefault`).
    pub run_defaults: RunProps,
    /// The paragraphs' defaults (`w:pPrDefault`).
    pub para_defaults: ParaProps,
    /// The styles, in the order written.
    pub styles: Vec<Style>,
    by_id: HashMap<String, usize>,
    /// Resolved chains, memoized: a paragraph style's properties.
    para_cache: RefCell<HashMap<String, (ParaProps, RunProps)>>,
    char_cache: RefCell<HashMap<String, RunProps>>,
}

/// How deep a `basedOn` chain is followed (a cycle ends it).
const MAX_CHAIN: usize = 32;

/// The Word style names that are written in lower case and shown
/// capitalized (`heading 1` is shown as "Heading 1").
fn shown_name(name: &str) -> String {
    let lower_builtin = name.starts_with("heading ")
        || name.starts_with("toc ")
        || name.starts_with("index ")
        || matches!(
            name,
            "footnote text"
                | "footnote reference"
                | "endnote text"
                | "endnote reference"
                | "annotation text"
                | "annotation reference"
                | "annotation subject"
                | "header"
                | "footer"
                | "caption"
                | "table of figures"
                | "toa heading"
                | "envelope address"
                | "envelope return"
                | "line number"
                | "page number"
                | "macro"
                | "table of authorities"
                | "index heading"
                | "balloon text"
                | "plain text"
        );
    if lower_builtin {
        let mut c = name.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
    } else {
        name.to_owned()
    }
}

impl Styles {
    /// Reads `styles.xml`; an empty text gives Word's defaults.
    pub fn parse(xml: &str) -> Styles {
        let mut s = Styles::default();
        let mut r = Reader::new(xml);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "rPrDefault" => {
                    children(&mut r, &tag, |r, c| {
                        if c.name == "rPr" {
                            s.run_defaults = props::read_rpr(r, &c).props;
                            return true;
                        }
                        false
                    });
                }
                "pPrDefault" => {
                    children(&mut r, &tag, |r, c| {
                        if c.name == "pPr" {
                            s.para_defaults = props::read_ppr(r, &c).props;
                            return true;
                        }
                        false
                    });
                }
                "latentStyles" if !tag.empty => {
                    r.skip_element();
                }
                "style" => {
                    let kind = match tag.attr("type").as_deref() {
                        Some("character") => StyleKind::Character,
                        Some("table") => StyleKind::Table,
                        Some("numbering") => StyleKind::Numbering,
                        _ => StyleKind::Paragraph,
                    };
                    let mut style = Style {
                        id: tag.attr("styleId").unwrap_or_default().into_owned(),
                        kind,
                        name: String::new(),
                        based_on: None,
                        next: None,
                        link: None,
                        default: matches!(
                            tag.attr("default").as_deref(),
                            Some("1" | "true" | "on")
                        ),
                        hidden: false,
                        ppr: ParaProps::default(),
                        rpr: RunProps::default(),
                        table: TableFormat::default(),
                        conditions: Vec::new(),
                    };
                    children(&mut r, &tag, |r, c| {
                        let val = || c.attr("val").map(|v| v.into_owned());
                        match c.name {
                            "name" => style.name = shown_name(&val().unwrap_or_default()),
                            "basedOn" => style.based_on = val(),
                            "next" => style.next = val(),
                            "link" => style.link = val(),
                            "semiHidden" | "hidden" => style.hidden |= props::on_off(&c),
                            "pPr" => {
                                style.ppr = props::read_ppr(r, &c).props;
                                return true;
                            }
                            "rPr" => {
                                style.rpr = props::read_rpr(r, &c).props;
                                return true;
                            }
                            "tblPr" => {
                                style.table.tblpr = props::read_tblpr(r, &c);
                                return true;
                            }
                            "trPr" => {
                                style.table.trpr = props::read_trpr(r, &c);
                                return true;
                            }
                            "tcPr" => {
                                style.table.tcpr = props::read_tcpr(r, &c);
                                return true;
                            }
                            "tblStylePr" => {
                                let ty = c.attr("type").unwrap_or_default().into_owned();
                                let mut f = TableFormat::default();
                                children(r, &c, |r, p| {
                                    match p.name {
                                        "pPr" => f.ppr = props::read_ppr(r, &p).props,
                                        "rPr" => f.rpr = props::read_rpr(r, &p).props,
                                        "tblPr" => f.tblpr = props::read_tblpr(r, &p),
                                        "trPr" => f.trpr = props::read_trpr(r, &p),
                                        "tcPr" => f.tcpr = props::read_tcpr(r, &p),
                                        _ => return false,
                                    }
                                    true
                                });
                                style.conditions.push((ty, f));
                                return true;
                            }
                            _ => {}
                        }
                        false
                    });
                    if style.name.is_empty() {
                        style.name = style.id.clone();
                    }
                    style.table.ppr = style.ppr.clone();
                    style.table.rpr = style.rpr.clone();
                    s.styles.push(style);
                }
                _ => {}
            }
        }
        for (i, st) in s.styles.iter().enumerate() {
            s.by_id.entry(st.id.clone()).or_insert(i);
        }
        s
    }

    /// A style by its ID.
    pub fn get(&self, id: &str) -> Option<&Style> {
        self.by_id.get(id).map(|&i| &self.styles[i])
    }

    /// A style by its ID, of a kind.
    pub fn get_kind(&self, id: &str, kind: StyleKind) -> Option<&Style> {
        self.get(id).filter(|s| s.kind == kind)
    }

    /// A style by its name as shown (`Heading 1`), case ignored.
    pub fn by_name(&self, name: &str) -> Option<&Style> {
        self.styles
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The default style of a kind (`Normal` for paragraphs).
    pub fn default_of(&self, kind: StyleKind) -> Option<&Style> {
        self.styles.iter().find(|s| s.kind == kind && s.default)
    }

    /// The style a paragraph naming `id` has: that style when it is a
    /// paragraph style, else the default one.
    pub fn para_style(&self, id: Option<&str>) -> Option<&Style> {
        id.and_then(|i| self.get_kind(i, StyleKind::Paragraph))
            .or_else(|| self.default_of(StyleKind::Paragraph))
    }

    /// A style and the ones it is based on, the root first; a cycle or a
    /// missing style ends the chain.
    pub fn chain(&self, id: &str) -> Vec<&Style> {
        let mut out: Vec<&Style> = Vec::new();
        let mut cur = self.get(id);
        while let Some(s) = cur {
            if out.len() >= MAX_CHAIN || out.iter().any(|o| o.id == s.id) {
                break;
            }
            out.push(s);
            cur = s
                .based_on
                .as_deref()
                .and_then(|b| self.get(b))
                .filter(|b| b.kind == s.kind);
        }
        out.reverse();
        out
    }

    /// A paragraph style's paragraph and run properties, its chain laid
    /// root first (no document defaults).
    pub fn para_style_props(&self, id: &str) -> (ParaProps, RunProps) {
        if let Some(hit) = self.para_cache.borrow().get(id) {
            return hit.clone();
        }
        let mut p = ParaProps::default();
        let mut r = RunProps::default();
        for s in self.chain(id) {
            p.merge(&s.ppr);
            r.merge(&s.rpr);
        }
        p.style = None;
        self.para_cache
            .borrow_mut()
            .insert(id.to_owned(), (p.clone(), r.clone()));
        (p, r)
    }

    /// A character style's run properties, its chain laid root first.
    pub fn char_style_props(&self, id: &str) -> RunProps {
        if let Some(hit) = self.char_cache.borrow().get(id) {
            return hit.clone();
        }
        let mut r = RunProps::default();
        for s in self.chain(id) {
            r.merge(&s.rpr);
        }
        r.style = None;
        self.char_cache
            .borrow_mut()
            .insert(id.to_owned(), r.clone());
        r
    }

    /// A table style's formats with its chain laid root first: the whole
    /// table's, and each conditional one's.
    pub fn table_style(&self, id: &str) -> (TableFormat, HashMap<String, TableFormat>) {
        let mut whole = TableFormat::default();
        let mut conds: HashMap<String, TableFormat> = HashMap::new();
        for s in self
            .chain(id)
            .into_iter()
            .filter(|s| s.kind == StyleKind::Table)
        {
            whole.merge(&s.table);
            for (ty, f) in &s.conditions {
                conds.entry(ty.clone()).or_default().merge(f);
            }
        }
        whole.tblpr.style = None;
        (whole, conds)
    }

    /// A run's properties put together (17.7.2): the document defaults,
    /// the table's, the paragraph style's, the character style's, then
    /// the run's own, toggle properties combined as 17.7.3 says. `para`
    /// is the paragraph style's ID (none for the default), `table` the
    /// table style's run properties for the run's cell, `run` the run's
    /// own properties.
    pub fn resolve_run(
        &self,
        para: Option<&str>,
        table: Option<&RunProps>,
        run: &RunProps,
    ) -> RunProps {
        let para_rpr = self
            .para_style(para)
            .map(|s| self.para_style_props(&s.id).1)
            .unwrap_or_default();
        let char_rpr = run
            .style
            .as_deref()
            .and_then(|id| self.get_kind(id, StyleKind::Character))
            .or_else(|| self.default_of(StyleKind::Character))
            .map(|s| self.char_style_props(&s.id))
            .unwrap_or_default();
        let empty = RunProps::default();
        let mut out = self.run_defaults.clone();
        let levels: [&RunProps; 3] = [table.unwrap_or(&empty), &para_rpr, &char_rpr];
        for l in levels {
            out.merge(l);
        }
        // Toggles: the styles' values combined by exclusive or, each kind
        // of style turning over what the ones before it gave; the run's
        // own value wins.
        for t in Toggle::ALL {
            let mut v: Option<bool> = None;
            for l in levels {
                if let Some(x) = l.toggle(t) {
                    v = Some(v.unwrap_or(false) ^ x);
                }
            }
            let v = v.or(self.run_defaults.toggle(t));
            out.set_toggle(t, v);
        }
        let mut own = run.clone();
        own.style = None;
        out.merge(&own);
        out
    }

    /// A paragraph's properties put together (17.7.2): the document
    /// defaults, the table style's, the paragraph style's chain, then the
    /// paragraph's own. The numbering level's indents are laid in by the
    /// caller, which knows the list.
    pub fn resolve_para(&self, table: Option<&ParaProps>, own: &ParaProps) -> ParaProps {
        let mut out = self.para_defaults.clone();
        if let Some(t) = table {
            out.merge(t);
        }
        if let Some(s) = self.para_style(own.style.as_deref()) {
            out.merge(&self.para_style_props(&s.id).0);
        }
        out.merge(own);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: &str = r#"<w:styles><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:latentStyles><w:lsdException w:name="Normal"/></w:latentStyles>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:outlineLvl w:val="0"/><w:spacing w:before="240"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Heading1"/><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:sz w:val="26"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:rPr><w:i/></w:rPr></w:style>
<w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font"/></w:style>
<w:style w:type="character" w:styleId="Emphasis"><w:name w:val="Emphasis"/><w:rPr><w:i/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="LoopA"><w:name w:val="Loop A"/><w:basedOn w:val="LoopB"/></w:style>
<w:style w:type="paragraph" w:styleId="LoopB"><w:name w:val="Loop B"/><w:basedOn w:val="LoopA"/></w:style>
<w:style w:type="table" w:styleId="Grid"><w:name w:val="Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/></w:tblBorders></w:tblPr></w:style>
<w:style w:type="table" w:styleId="Banded"><w:name w:val="Banded"/><w:basedOn w:val="Grid"/><w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr><w:tcPr><w:shd w:val="clear" w:fill="4472C4"/></w:tcPr></w:tblStylePr></w:style>
</w:styles>"#;

    #[test]
    fn styles_read_with_their_chains() {
        let s = Styles::parse(STYLES);
        assert_eq!(s.get("Heading1").unwrap().name, "Heading 1");
        assert_eq!(s.by_name("heading 2").unwrap().id, "Heading2");
        assert_eq!(s.default_of(StyleKind::Paragraph).unwrap().id, "Normal");
        let chain: Vec<&str> = s.chain("Heading2").iter().map(|s| s.id.as_str()).collect();
        assert_eq!(chain, ["Normal", "Heading1", "Heading2"]);
        let (p, r) = s.para_style_props("Heading2");
        assert_eq!((p.outline, p.space_before), (Some(1), Some(240)));
        assert_eq!((r.size, r.toggle(Toggle::Bold)), (Some(26), Some(true)));
        // A cycle ends the chain instead of looping.
        assert_eq!(s.chain("LoopA").len(), 2);
    }

    #[test]
    fn runs_put_together_with_toggles() {
        let s = Styles::parse(STYLES);
        let plain = s.resolve_run(None, None, &RunProps::default());
        assert_eq!(plain.size, Some(22));
        assert_eq!(plain.fonts.ascii_theme.as_deref(), Some("minorHAnsi"));
        assert_eq!(plain.toggle(Toggle::Italic), None);
        let emph = RunProps {
            style: Some("Emphasis".into()),
            ..RunProps::default()
        };
        // Emphasis inside an italic paragraph style turns italics off.
        let r = s.resolve_run(Some("Quote"), None, &emph);
        assert_eq!(r.toggle(Toggle::Italic), Some(false));
        let r = s.resolve_run(None, None, &emph);
        assert_eq!(r.toggle(Toggle::Italic), Some(true));
        // The run's own value wins.
        let mut own = emph.clone();
        own.set_toggle(Toggle::Italic, Some(true));
        assert_eq!(
            s.resolve_run(Some("Quote"), None, &own)
                .toggle(Toggle::Italic),
            Some(true)
        );
        let h = s.resolve_run(Some("Heading2"), None, &RunProps::default());
        assert_eq!((h.size, h.toggle(Toggle::Bold)), (Some(26), Some(true)));
    }

    #[test]
    fn paragraphs_put_together() {
        let s = Styles::parse(STYLES);
        let mut own = ParaProps {
            style: Some("Heading2".into()),
            space_before: Some(0),
            ..ParaProps::default()
        };
        let p = s.resolve_para(None, &own);
        assert_eq!(
            (p.space_before, p.space_after, p.outline),
            (Some(0), Some(160), Some(1))
        );
        // An unknown style is the default one.
        own.style = Some("Nope".into());
        assert_eq!(s.resolve_para(None, &own).outline, None);
    }

    #[test]
    fn table_styles_with_conditions() {
        let s = Styles::parse(STYLES);
        let (whole, conds) = s.table_style("Banded");
        assert!(whole.tblpr.borders.top.is_some());
        let first = &conds["firstRow"];
        assert_eq!(first.rpr.toggle(Toggle::Bold), Some(true));
        assert_eq!(
            first
                .tcpr
                .shading
                .as_ref()
                .unwrap()
                .fill
                .as_ref()
                .unwrap()
                .rgb,
            Some(0x4472C4)
        );
    }
}
