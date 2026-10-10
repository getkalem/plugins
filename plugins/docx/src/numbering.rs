//! Lists (`numbering.xml`, ECMA-376 part 1, 17.9): the abstract
//! definitions with their nine levels, the instances paragraphs name by
//! `w:numId` with their overrides, and the counters that give each
//! numbered paragraph the label Word shows (`1.`, `1.2.`, `a)`, `•`).

use std::collections::{HashMap, HashSet};

use kalem_ooxml::xml::{Reader, Token};

use crate::chars;
use crate::props::{self, ParaProps, RunProps, children};
use crate::styles::{StyleKind, Styles};

/// What follows a list label (`w:suff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Suffix {
    /// A tab.
    #[default]
    Tab,
    /// A space.
    Space,
    /// Nothing.
    Nothing,
}

/// A level of a list (`w:lvl`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Level {
    /// The first number (`w:start`).
    pub start: i64,
    /// The number format (`decimal`, `bullet`, `lowerLetter`…).
    pub format: String,
    /// The label's text (`w:lvlText`), `%1` to `%9` standing for the
    /// levels' numbers; none for no label.
    pub text: Option<String>,
    /// After which level this one starts again (`w:lvlRestart`): 0 never,
    /// N after level N (from 1) is used; absent after any level above.
    pub restart: Option<i64>,
    /// The paragraph style linked to this level.
    pub style: Option<String>,
    /// Every level's number in decimal (`w:isLgl`).
    pub legal: bool,
    /// What follows the label.
    pub suffix: Suffix,
    /// The label's alignment.
    pub jc: Option<String>,
    /// The paragraph's indents for this level.
    pub ppr: ParaProps,
    /// The label's run properties (its font).
    pub rpr: RunProps,
}

/// An abstract list definition (`w:abstractNum`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbstractList {
    /// `w:abstractNumId`.
    pub id: String,
    /// Its levels, 0 to 8.
    pub levels: Vec<Option<Level>>,
    /// The numbering style this definition stands for (`w:styleLink`).
    pub style_link: Option<String>,
    /// The numbering style whose definition it uses (`w:numStyleLink`).
    pub num_style_link: Option<String>,
}

/// A list instance (`w:num`): an abstract definition and its overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListInstance {
    /// The abstract definition.
    pub abstract_id: String,
    /// Per level: its new start (`w:startOverride`) and its new
    /// definition (`w:lvl` inside `w:lvlOverride`).
    pub overrides: HashMap<u8, (Option<i64>, Option<Level>)>,
}

/// The lists of a document.
#[derive(Debug, Clone, Default)]
pub struct Numbering {
    abstracts: HashMap<String, AbstractList>,
    instances: HashMap<String, ListInstance>,
}

fn read_level<'a>(r: &mut Reader<'a>, tag: &kalem_ooxml::xml::Tag<'a>) -> (u8, Level) {
    let ilvl = tag
        .attr("ilvl")
        .and_then(|v| v.trim().parse::<u8>().ok())
        .unwrap_or(0)
        .min(8);
    let mut l = Level {
        start: 0,
        format: "decimal".into(),
        ..Level::default()
    };
    let mut start = None;
    children(r, tag, |r, c| {
        let val = || c.attr("val").map(|v| v.into_owned());
        match c.name {
            "start" => start = val().and_then(|v| v.trim().parse().ok()),
            "numFmt" => l.format = val().unwrap_or_else(|| "decimal".into()),
            "lvlText" => l.text = val(),
            "lvlRestart" => l.restart = val().and_then(|v| v.trim().parse().ok()),
            "pStyle" => l.style = val(),
            "isLgl" => l.legal = props::on_off(&c),
            "suff" => {
                l.suffix = match val().as_deref() {
                    Some("space") => Suffix::Space,
                    Some("nothing") => Suffix::Nothing,
                    _ => Suffix::Tab,
                }
            }
            "lvlJc" => l.jc = val(),
            "pPr" => {
                l.ppr = props::read_ppr(r, &c).props;
                return true;
            }
            "rPr" => {
                l.rpr = props::read_rpr(r, &c).props;
                return true;
            }
            _ => {}
        }
        false
    });
    // A level without `w:start` counts from 0 (17.9.25), but Word's own
    // levels always give it.
    l.start = start.unwrap_or(0);
    (ilvl, l)
}

impl Numbering {
    /// Reads `numbering.xml`.
    pub fn parse(xml: &str) -> Numbering {
        let mut n = Numbering::default();
        let mut r = Reader::new(xml);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "abstractNum" => {
                    let mut a = AbstractList {
                        id: tag.attr("abstractNumId").unwrap_or_default().into_owned(),
                        levels: vec![None; 9],
                        ..AbstractList::default()
                    };
                    children(&mut r, &tag, |r, c| {
                        match c.name {
                            "lvl" => {
                                let (i, l) = read_level(r, &c);
                                a.levels[usize::from(i)] = Some(l);
                                return true;
                            }
                            "styleLink" => a.style_link = c.attr("val").map(|v| v.into_owned()),
                            "numStyleLink" => {
                                a.num_style_link = c.attr("val").map(|v| v.into_owned());
                            }
                            _ => {}
                        }
                        false
                    });
                    n.abstracts.insert(a.id.clone(), a);
                }
                "num" => {
                    let id = tag.attr("numId").unwrap_or_default().into_owned();
                    let mut inst = ListInstance::default();
                    children(&mut r, &tag, |r, c| {
                        match c.name {
                            "abstractNumId" => {
                                inst.abstract_id = c.attr("val").unwrap_or_default().into_owned();
                            }
                            "lvlOverride" => {
                                let ilvl = c
                                    .attr("ilvl")
                                    .and_then(|v| v.trim().parse::<u8>().ok())
                                    .unwrap_or(0)
                                    .min(8);
                                let mut start = None;
                                let mut level = None;
                                children(r, &c, |r, o| {
                                    match o.name {
                                        "startOverride" => {
                                            start =
                                                o.attr("val").and_then(|v| v.trim().parse().ok());
                                        }
                                        "lvl" => {
                                            level = Some(read_level(r, &o).1);
                                            return true;
                                        }
                                        _ => {}
                                    }
                                    false
                                });
                                inst.overrides.insert(ilvl, (start, level));
                                return true;
                            }
                            _ => {}
                        }
                        false
                    });
                    n.instances.insert(id, inst);
                }
                _ => {}
            }
        }
        n
    }

    /// The abstract definitions.
    pub fn abstracts(&self) -> impl Iterator<Item = &AbstractList> {
        self.abstracts.values()
    }

    /// The instances, by `w:numId`.
    pub fn instances(&self) -> impl Iterator<Item = (&String, &ListInstance)> {
        self.instances.iter()
    }

    /// Whether the document has no lists.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// An instance by its `w:numId`.
    pub fn instance(&self, num_id: &str) -> Option<&ListInstance> {
        self.instances.get(num_id)
    }

    /// The abstract definition an instance uses, a `w:numStyleLink`
    /// followed to the numbering style's own instance.
    pub fn abstract_of(&self, num_id: &str, styles: &Styles) -> Option<&AbstractList> {
        let mut id = &self.instances.get(num_id)?.abstract_id;
        for _ in 0..8 {
            let a = self.abstracts.get(id)?;
            let Some(link) = &a.num_style_link else {
                return Some(a);
            };
            let style = styles.get_kind(link, StyleKind::Numbering)?;
            let num = style.ppr.num_id.as_deref()?;
            id = &self.instances.get(num)?.abstract_id;
        }
        None
    }

    /// The level `ilvl` of instance `num_id` as it applies: the override's
    /// definition when it gives one.
    pub fn level(&self, num_id: &str, ilvl: u8, styles: &Styles) -> Option<Level> {
        let inst = self.instances.get(num_id)?;
        if let Some((_, Some(l))) = inst.overrides.get(&ilvl) {
            return Some(l.clone());
        }
        self.abstract_of(num_id, styles)?
            .levels
            .get(usize::from(ilvl))?
            .clone()
    }

    /// The level of an instance a paragraph style is linked to
    /// (`w:pStyle` of a `w:lvl`), for a style naming the list but no
    /// level.
    pub fn level_of_style(&self, num_id: &str, style: &str, styles: &Styles) -> Option<u8> {
        let a = self.abstract_of(num_id, styles)?;
        a.levels
            .iter()
            .position(|l| l.as_ref().and_then(|l| l.style.as_deref()) == Some(style))
            .map(|i| i as u8)
    }
}

/// A numbered paragraph's label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The text (`1.2.`, `•`).
    pub text: String,
    /// What follows it.
    pub suffix: Suffix,
    /// The level, from 0.
    pub level: u8,
    /// A bullet rather than a number.
    pub bullet: bool,
    /// The level's paragraph properties (its indents).
    pub ppr: ParaProps,
    /// The label's run properties.
    pub rpr: RunProps,
}

/// The counters of every list, run through a document's paragraphs in
/// order.
#[derive(Debug)]
pub struct Counters<'a> {
    numbering: &'a Numbering,
    styles: &'a Styles,
    counts: HashMap<String, [Option<i64>; 9]>,
    started: HashSet<(String, u8)>,
}

impl<'a> Counters<'a> {
    /// Counters before the first paragraph.
    pub fn new(numbering: &'a Numbering, styles: &'a Styles) -> Counters<'a> {
        Counters {
            numbering,
            styles,
            counts: HashMap::new(),
            started: HashSet::new(),
        }
    }

    /// The label of the next paragraph of list `num_id` at level `ilvl`,
    /// counted. Instances of one abstract definition count together, as
    /// Word counts them, unless an instance starts a level again
    /// (`w:startOverride`), which then counts on its own.
    pub fn next(&mut self, num_id: &str, ilvl: u8) -> Option<Label> {
        if num_id.trim() == "0" {
            return None;
        }
        let inst = self.numbering.instance(num_id)?;
        let abs = self.numbering.abstract_of(num_id, self.styles)?;
        let restarts = inst
            .overrides
            .values()
            .any(|(s, l)| s.is_some() || l.is_some());
        let key = if restarts {
            format!("num:{num_id}")
        } else {
            format!("abs:{}", abs.id)
        };
        let levels: Vec<Option<Level>> = (0..9u8)
            .map(|i| self.numbering.level(num_id, i, self.styles))
            .collect();
        let level = levels[usize::from(ilvl)].clone()?;
        let start_of = |i: usize| -> i64 {
            inst.overrides
                .get(&(i as u8))
                .and_then(|(s, l)| s.or(l.as_ref().map(|l| l.start)))
                .or_else(|| levels[i].as_ref().map(|l| l.start))
                .unwrap_or(0)
        };
        let counts = self.counts.entry(key.clone()).or_insert([None; 9]);
        let i = usize::from(ilvl);
        // The first use of an overridden level starts it from its
        // override, whatever came before.
        if self.started.insert((key, ilvl)) && inst.overrides.contains_key(&ilvl) {
            counts[i] = None;
        }
        counts[i] = Some(match counts[i] {
            Some(c) => c + 1,
            None => start_of(i),
        });
        for (m, lv) in levels.iter().enumerate().skip(i + 1) {
            let restart = lv.as_ref().and_then(|l| l.restart);
            let resets = match restart {
                Some(0) => false,
                Some(r) => (i as i64) < r,
                None => true,
            };
            if resets {
                counts[m] = None;
            }
        }
        let bullet = level.format == "bullet";
        let font = level
            .rpr
            .fonts
            .ascii
            .clone()
            .or(level.rpr.fonts.h_ansi.clone())
            .unwrap_or_default();
        let template = level.text.clone().unwrap_or_default();
        let mut text = String::new();
        let mut chars_iter = template.chars().peekable();
        while let Some(c) = chars_iter.next() {
            if c == '%'
                && let Some(d) = chars_iter.peek().and_then(|d| d.to_digit(10))
                && (1..=9).contains(&d)
            {
                chars_iter.next();
                let m = (d - 1) as usize;
                let n = counts[m].unwrap_or_else(|| start_of(m));
                let fmt = levels[m].as_ref().map_or("decimal", |l| l.format.as_str());
                let fmt = if level.legal && m < i { "decimal" } else { fmt };
                if fmt == "bullet" {
                    continue;
                }
                text.push_str(&chars::format_number(n, fmt));
            } else if bullet {
                text.push(chars::symbol(&font, c));
            } else {
                text.push(c);
            }
        }
        Some(Label {
            text,
            suffix: level.suffix,
            level: ilvl,
            bullet,
            ppr: level.ppr,
            rpr: level.rpr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lvl(i: u8, fmt: &str, text: &str, extra: &str) -> String {
        format!(
            r#"<w:lvl w:ilvl="{i}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/>{extra}<w:lvlText w:val="{text}"/><w:pPr><w:ind w:left="{}" w:hanging="360"/></w:pPr></w:lvl>"#,
            720 * (u32::from(i) + 1)
        )
    }

    fn numbering() -> Numbering {
        let xml = format!(
            r#"<w:numbering><w:abstractNum w:abstractNumId="0">{}{}{}</w:abstractNum><w:abstractNum w:abstractNumId="1">{}{}</w:abstractNum><w:abstractNum w:abstractNumId="2">{}{}</w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="3"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride></w:num>
<w:num w:numId="4"><w:abstractNumId w:val="1"/></w:num>
<w:num w:numId="5"><w:abstractNumId w:val="2"/></w:num></w:numbering>"#,
            lvl(0, "decimal", "%1.", ""),
            lvl(1, "lowerLetter", "%1.%2)", ""),
            lvl(2, "lowerRoman", "%3.", r#"<w:lvlRestart w:val="0"/>"#),
            lvl(0, "bullet", "\u{F0B7}", "").replace(
                "</w:pPr>",
                r#"</w:pPr><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol"/></w:rPr>"#
            ),
            lvl(1, "bullet", "o", ""),
            lvl(0, "upperRoman", "%1.", ""),
            lvl(1, "decimal", "%1.%2", "<w:isLgl/>"),
        );
        Numbering::parse(&xml)
    }

    #[test]
    fn labels_counted_as_word_does() {
        let n = numbering();
        let s = Styles::default();
        let mut c = Counters::new(&n, &s);
        let mut next = |num: &str, l: u8| c.next(num, l).map(|l| l.text).unwrap_or_default();
        assert_eq!(next("1", 0), "1.");
        assert_eq!(next("1", 1), "1.a)");
        assert_eq!(next("1", 1), "1.b)");
        assert_eq!(next("1", 2), "i.");
        assert_eq!(next("1", 0), "2.");
        // Level 2 never restarts; level 1 does.
        assert_eq!(next("1", 1), "2.a)");
        assert_eq!(next("1", 2), "ii.");
        // Another instance of the same definition goes on counting.
        assert_eq!(next("2", 0), "3.");
        // An instance that starts again counts on its own from 5.
        assert_eq!(next("3", 0), "5.");
        assert_eq!(next("3", 0), "6.");
        assert_eq!(next("1", 0), "4.");
        assert_eq!(next("4", 0), "•");
        assert_eq!(next("4", 1), "o");
        assert_eq!(next("5", 0), "I.");
        assert_eq!(next("5", 0), "II.");
        // Legal numbering: the levels above in decimal.
        assert_eq!(next("5", 1), "2.1");
        assert_eq!(next("0", 0), "");
        assert_eq!(next("99", 0), "");
    }

    #[test]
    fn levels_carry_their_indents() {
        let n = numbering();
        let s = Styles::default();
        let l = n.level("1", 1, &s).unwrap();
        assert_eq!((l.ppr.ind_left, l.ppr.ind_first), (Some(1440), Some(-360)));
        let mut c = Counters::new(&n, &s);
        let label = c.next("4", 0).unwrap();
        assert!(label.bullet);
        assert_eq!(label.rpr.fonts.ascii.as_deref(), Some("Symbol"));
    }
}
