//! A story as it is shown: Word's web layout, continuous, without pages
//! (Kalem's task T3.7.5). Each paragraph comes with its style resolved,
//! its list label counted, its runs' look put together, fields showing
//! their results, tracked changes and comments marked, and unknown
//! constructs as placeholders naming them: the blocks Kalem's flow view
//! will lay out (the docx list's WP5), and the text search, copy and the
//! terminal read.
//!
//! Each paragraph also comes with its [`Layout`]: its text in the
//! coordinates edits use (one character per tab, break, symbol or object)
//! and, for each piece of it, the element it was read from, so that
//! [`crate::edit`] rewrites a run's text and nothing around it.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use kalem_ooxml::rels::Rel;
use kalem_ooxml::theme::Theme;

use crate::numbering::{Counters, Numbering, Suffix};
use crate::props::{Borders, Color, Revision, Rgb, RunProps, Span, TableLook, Toggle, VMerge};
use crate::story::{
    AtomKind, Block, BreakKind, Control, FieldChar, Graphic, GroupKind, Inline, MarkerKind,
    Paragraph, Run, Table,
};
use crate::styles::{CONDITIONS, StyleKind, Styles, TableFormat};

/// The character a non-text atom stands for in a paragraph's edit text:
/// a picture, a note's mark, an equation.
pub const OBJECT: char = '\u{FFFC}';
/// A line break in the edit text, as Word's own text has it.
pub const LINE_BREAK: char = '\u{B}';
/// A page break in the edit text.
pub const PAGE_BREAK: char = '\u{C}';
/// A column break in the edit text.
pub const COLUMN_BREAK: char = '\u{E}';

/// Where a story is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StoryId {
    /// The main document's body.
    Body,
    /// A header part.
    Header(String),
    /// A footer part.
    Footer(String),
    /// A footnote, by its `w:id`.
    Footnote(String),
    /// An endnote.
    Endnote(String),
    /// A comment.
    Comment(String),
}

/// A paragraph's place: its story, and its index among the story's
/// paragraphs in document order (tables' included, text boxes' not).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParaAt {
    /// The story.
    pub story: StoryId,
    /// The paragraph's index.
    pub index: usize,
}

/// A paragraph's alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// At the start (left in a left-to-right paragraph).
    #[default]
    Start,
    /// Centered.
    Center,
    /// At the end.
    End,
    /// Justified.
    Justify,
}

/// Raised or lowered text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vert {
    /// On the baseline.
    #[default]
    Baseline,
    /// Superscript.
    Super,
    /// Subscript.
    Sub,
}

/// How a run looks, everything resolved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Look {
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// The underline's kind (`single`, `double`, `wave`…), if any.
    pub underline: Option<String>,
    /// Struck through.
    pub strike: bool,
    /// Struck through twice.
    pub double_strike: bool,
    /// Shown in capitals.
    pub caps: bool,
    /// Shown in small capitals.
    pub small_caps: bool,
    /// Hidden text.
    pub hidden: bool,
    /// The text's color; `None` for automatic (the theme's text).
    pub color: Option<Rgb>,
    /// The highlight.
    pub highlight: Option<Rgb>,
    /// The shading behind the text.
    pub shading: Option<Rgb>,
    /// The size in points.
    pub size: f32,
    /// The typeface.
    pub face: Arc<str>,
    /// Raised or lowered.
    pub vert: Vert,
}

/// What a run of the view is.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    /// Text.
    Text,
    /// A tab.
    Tab,
    /// A line break.
    LineBreak,
    /// A page break.
    PageBreak,
    /// A column break.
    ColumnBreak,
    /// A footnote's mark, numbered.
    FootnoteRef(String),
    /// An endnote's mark, numbered.
    EndnoteRef(String),
    /// A picture: the image part, its size in points, its alternative
    /// text.
    Picture {
        /// The image's part name (`word/media/image1.png`), or an
        /// external link.
        target: Option<String>,
        /// Its size in points.
        size: (f32, f32),
        /// Its alternative text.
        alt: String,
        /// It floats beside the text where Word places it; shown in line.
        floating: bool,
    },
    /// A construct shown by name (a chart, a shape, an equation, a text
    /// box's frame, an element the plugin does not know).
    Placeholder(String),
}

/// A run of the view: text with one look and one context.
#[derive(Debug, Clone, PartialEq)]
pub struct VRun {
    /// What is shown.
    pub text: String,
    /// What it is.
    pub piece: Piece,
    /// The bytes of the paragraph's edit text it stands for.
    pub source: Range<usize>,
    /// Its look.
    pub look: Look,
    /// The target of a link it is in: a URL, or `#bookmark`.
    pub link: Option<String>,
    /// Inserted (a tracked change), and by whom.
    pub inserted: Option<Box<Revision>>,
    /// Deleted (a tracked change), and by whom.
    pub deleted: Option<Box<Revision>>,
    /// Its formatting changed (a tracked change).
    pub format_changed: Option<Box<Revision>>,
    /// The comments whose range it is in, by `w:id`.
    pub comments: Vec<String>,
    /// The instruction of the field whose result it is.
    pub field: Option<String>,
}

/// A paragraph of the view.
#[derive(Debug, Clone, PartialEq)]
pub struct VPara {
    /// Where it is, for edits; `None` for a text box's paragraph, which
    /// is not edited yet.
    pub at: Option<ParaAt>,
    /// Its style's ID.
    pub style_id: String,
    /// Its style's name (`Heading 1`).
    pub style: String,
    /// Its list label (`1.`, `•`) and what follows it.
    pub label: Option<(String, Suffix, Look)>,
    /// Its list level, from 0, when it has a label.
    pub list_level: Option<u8>,
    /// Its outline level, from 0, when it is a heading.
    pub outline: Option<u8>,
    /// Its alignment.
    pub align: Align,
    /// Its start and end indents and its first line's (negative when
    /// hanging), in points.
    pub indent: (f32, f32, f32),
    /// Space before and after, in points.
    pub spacing: (f32, f32),
    /// Its shading.
    pub shading: Option<Rgb>,
    /// Its runs.
    pub runs: Vec<VRun>,
    /// The mark inserted or deleted (a tracked change).
    pub mark_change: Option<(bool, Revision)>,
    /// The kind of the section break it ends with (`nextPage`,
    /// `continuous`…).
    pub section_break: Option<String>,
    /// Its edit coordinates.
    pub layout: Layout,
}

/// A table of the view.
#[derive(Debug, Clone, PartialEq)]
pub struct VTable {
    /// Its style's name.
    pub style: Option<String>,
    /// The grid's columns, in points.
    pub grid: Vec<f32>,
    /// Its rows.
    pub rows: Vec<VRow>,
}

/// A row of the view.
#[derive(Debug, Clone, PartialEq)]
pub struct VRow {
    /// Its cells.
    pub cells: Vec<VCell>,
    /// A header row, repeated on each page.
    pub header: bool,
}

/// A border of the view: its color (automatic as `None`), width in
/// points and line.
#[derive(Debug, Clone, PartialEq)]
pub struct VBorder {
    /// Its color.
    pub color: Option<Rgb>,
    /// Its width in points.
    pub width: f32,
    /// Its line (`single`, `double`, `dashed`…).
    pub style: String,
}

/// A cell of the view.
#[derive(Debug, Clone, PartialEq)]
pub struct VCell {
    /// The grid columns it spans.
    pub columns: u32,
    /// Its part in a vertical merge.
    pub merge: Option<VMerge>,
    /// Its fill.
    pub fill: Option<Rgb>,
    /// Its borders: top, start, bottom, end.
    pub borders: [Option<VBorder>; 4],
    /// Its blocks.
    pub blocks: Vec<VBlock>,
}

/// A block of the view.
// Paragraphs are most blocks: boxing them would cost an allocation each.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum VBlock {
    /// A paragraph.
    Para(VPara),
    /// A table.
    Table(VTable),
    /// A text box's content, shown after the paragraph it is anchored
    /// in.
    Frame(Vec<VBlock>),
    /// A construct between blocks shown by name.
    Placeholder(String),
    /// A horizontal rule: a paragraph holding nothing but lines drawn as
    /// shapes (a separator).
    Rule,
}

/// How a shape that is a line shows among a paragraph's text.
pub const LINE_SHAPE: &str = "──";

/// A note of the view, in the order it is referred to.
#[derive(Debug, Clone, PartialEq)]
pub struct VNote {
    /// Its `w:id`.
    pub id: String,
    /// Its mark as shown (`1`, `iv`).
    pub mark: String,
    /// Its blocks.
    pub blocks: Vec<VBlock>,
}

/// A comment of the view.
#[derive(Debug, Clone, PartialEq)]
pub struct VComment {
    /// Its `w:id`.
    pub id: String,
    /// Its author.
    pub author: String,
    /// Its date.
    pub date: Option<String>,
    /// The `w:id` of the comment it answers.
    pub parent: Option<String>,
    /// Whether it is marked done.
    pub done: bool,
    /// Its blocks.
    pub blocks: Vec<VBlock>,
}

/// Why a piece of a paragraph is not edited as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lock {
    /// Deleted text of a tracked change: accept or reject it.
    Deleted,
    /// A field's result or code: it is computed.
    Field,
    /// Read through `mc:AlternateContent`, whose other branch would have
    /// to change with it.
    Alternate,
    /// A note's mark: the note goes with it.
    NoteRef,
    /// A content control that is not text (a check box), or one showing
    /// its placeholder.
    Control,
}

impl Lock {
    /// Why, for the user.
    pub fn reason(self) -> &'static str {
        match self {
            Lock::Deleted => {
                "This is deleted text of a tracked change: accept or reject the change"
            }
            Lock::Field => "This is a field's result, which Word computes",
            Lock::Alternate => {
                "This is drawn two ways in the file (mc:AlternateContent); it is not edited yet"
            }
            Lock::NoteRef => {
                "This is a note's mark; a note is not deleted by deleting its mark yet"
            }
            Lock::Control => "This content control is not edited as text",
        }
    }
}

/// A piece of a paragraph's edit text and where it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    /// Its bytes of the edit text.
    pub range: Range<usize>,
    /// The run it is in, an index of [`Layout::runs`].
    pub run: usize,
    /// Its element (`w:t`, `w:tab`, a drawing…).
    pub atom: Span,
    /// For text, the `w:t` content's span and whether it preserves
    /// spaces.
    pub text: Option<(Span, bool)>,
    /// Why it is not edited, if it is not.
    pub lock: Option<Lock>,
}

/// A run, or an object outside runs (an equation), as edits see it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunInfo {
    /// The whole element.
    pub span: Span,
    /// Its start tag; empty for an object outside runs.
    pub start_tag: Span,
    /// Its properties' element.
    pub rpr: Option<Span>,
    /// Its end tag.
    pub end_tag: Span,
    /// Its children but the properties, in order, each once.
    pub atoms: Vec<Span>,
    /// The paragraph's top-level inline it is in, an index of
    /// [`Layout::tops`].
    pub top: usize,
    /// Why typing into it is refused, if it is.
    pub lock: Option<Lock>,
    /// Inside an insertion of a tracked change: deleting from it takes
    /// the text away rather than marking it deleted.
    pub inserted: bool,
}

/// A paragraph in edit coordinates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    /// The part holding it.
    pub part: String,
    /// The whole `w:p`.
    pub span: Span,
    /// Its start tag.
    pub start_tag: Span,
    /// Written as `<w:p/>`.
    pub empty: bool,
    /// Its `w:pPr`.
    pub ppr: Option<Span>,
    /// Its mark's `w:rPr`.
    pub mark_rpr: Option<Span>,
    /// The section it ends.
    pub sect: Option<Span>,
    /// Where its end tag starts.
    pub content_end: usize,
    /// Its own style.
    pub style: Option<String>,
    /// Its text: one character per tab ('\t'), line break ('\u{B}'), page
    /// break ('\u{C}'), column break ('\u{E}'), symbol, hyphen, and
    /// object ('\u{FFFC}').
    pub text: String,
    /// The pieces of the text.
    pub segs: Vec<Seg>,
    /// Its runs.
    pub runs: Vec<RunInfo>,
    /// Its top-level inlines' spans.
    pub tops: Vec<Span>,
    /// The paragraph mark is deleted (a tracked change).
    pub mark_deleted: bool,
    /// The paragraph mark is inserted (a tracked change).
    pub mark_inserted: bool,
    /// Its container (the body, a cell, a content control) and its
    /// index there, so that a paragraph joins only its next sibling.
    pub sibling: (usize, usize),
}

/// What resolving a story needs of the document.
#[derive(Debug)]
pub struct Env<'a> {
    /// The styles.
    pub styles: &'a Styles,
    /// The lists.
    pub numbering: &'a Numbering,
    /// The theme.
    pub theme: &'a Theme,
    /// `w:clrSchemeMapping` of the settings: `t1` → `dark1`…
    pub mapping: &'a HashMap<String, String>,
    /// The relationships of the part being shown.
    pub rels: &'a [Rel],
    /// The part being shown.
    pub part: &'a str,
}

pub(crate) const HIGHLIGHTS: &[(&str, Rgb)] = &[
    ("yellow", 0xFFFF00),
    ("green", 0x00FF00),
    ("cyan", 0x00FFFF),
    ("magenta", 0xFF00FF),
    ("blue", 0x0000FF),
    ("red", 0xFF0000),
    ("darkBlue", 0x000080),
    ("darkCyan", 0x008080),
    ("darkGreen", 0x008000),
    ("darkMagenta", 0x800080),
    ("darkRed", 0x800000),
    ("darkYellow", 0x808000),
    ("darkGray", 0x808080),
    ("lightGray", 0xC0C0C0),
    ("black", 0x000000),
    ("white", 0xFFFFFF),
];

fn hsl(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from((c >> 16) & 0xFF) / 255.0,
        f64::from((c >> 8) & 0xFF) / 255.0,
        f64::from(c & 0xFF) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> Rgb {
    let ch = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    if s == 0.0 {
        let v = ch(l);
        return (v << 16) | (v << 8) | v;
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (ch(hue(h + 1.0 / 3.0)) << 16) | (ch(hue(h)) << 8) | ch(hue(h - 1.0 / 3.0))
}

/// A theme color with Word's tint or shade (`w:themeTint`, 0 to 255)
/// applied to its luminance, as Word computes the value it writes beside.
pub fn tinted(c: Rgb, tint: Option<u8>, shade: Option<u8>) -> Rgb {
    let (h, s, mut l) = hsl(c);
    if let Some(t) = tint {
        let t = f64::from(t) / 255.0;
        l = l * t + (1.0 - t);
    }
    if let Some(sh) = shade {
        l *= f64::from(sh) / 255.0;
    }
    if tint.is_none() && shade.is_none() {
        return c;
    }
    from_hsl(h, s, l)
}

impl Env<'_> {
    /// A color resolved: a theme color through the settings' mapping and
    /// the theme, tinted or shaded; else the value written; `None` for
    /// automatic.
    pub fn color(&self, c: &Color) -> Option<Rgb> {
        if let Some(name) = &c.theme {
            let mapped = |key: &str, default: &str| {
                self.mapping
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| default.to_owned())
            };
            let scheme = match name.as_str() {
                "text1" => mapped("t1", "dark1"),
                "background1" => mapped("bg1", "light1"),
                "text2" => mapped("t2", "dark2"),
                "background2" => mapped("bg2", "light2"),
                n => n.to_owned(),
            };
            let key = match scheme.as_str() {
                "dark1" => "dk1",
                "light1" => "lt1",
                "dark2" => "dk2",
                "light2" => "lt2",
                "hyperlink" => "hlink",
                "followedHyperlink" => "folHlink",
                n => n,
            };
            if let Some(base) = self.theme.color(key) {
                return Some(tinted(base, c.tint, c.shade));
            }
        }
        if c.auto { None } else { c.rgb }
    }

    /// A typeface: the theme's when a theme font is named, else the one
    /// written.
    pub fn face(&self, p: &RunProps) -> String {
        let theme_font = |name: &str| -> Option<String> {
            let f = if name.starts_with("major") {
                &self.theme.major
            } else {
                &self.theme.minor
            };
            let face = if name.ends_with("EastAsia") {
                &f.east_asian
            } else if name.ends_with("Bidi") {
                &f.complex
            } else {
                &f.latin
            };
            (!face.is_empty()).then(|| face.clone())
        };
        let f = &p.fonts;
        f.ascii_theme
            .as_deref()
            .and_then(theme_font)
            .or_else(|| f.ascii.clone())
            .or_else(|| f.h_ansi_theme.as_deref().and_then(theme_font))
            .or_else(|| f.h_ansi.clone())
            .unwrap_or_else(|| "Times New Roman".into())
    }

    /// How resolved run properties look.
    pub fn look(&self, p: &RunProps) -> Look {
        self.look_with(p, self.face(p).into())
    }

    /// How resolved run properties look, their typeface given.
    pub fn look_with(&self, p: &RunProps, face: Arc<str>) -> Look {
        let on = |t: Toggle| p.toggle(t).unwrap_or(false);
        Look {
            bold: on(Toggle::Bold),
            italic: on(Toggle::Italic),
            underline: p
                .underline
                .as_ref()
                .map(|(k, _)| k.clone())
                .filter(|k| k != "none"),
            strike: on(Toggle::Strike),
            double_strike: p.dstrike.unwrap_or(false),
            caps: on(Toggle::Caps),
            small_caps: on(Toggle::SmallCaps),
            hidden: on(Toggle::Vanish),
            color: p.color.as_ref().and_then(|c| self.color(c)),
            highlight: p
                .highlight
                .as_deref()
                .and_then(|h| HIGHLIGHTS.iter().find(|(n, _)| *n == h).map(|(_, c)| *c)),
            shading: p
                .shading
                .as_ref()
                .and_then(|s| s.fill.as_ref())
                .and_then(|c| self.color(c)),
            size: p.size.unwrap_or(20) as f32 / 2.0,
            face,
            vert: match p.vert_align.as_deref() {
                Some("superscript") => Vert::Super,
                Some("subscript") => Vert::Sub,
                _ => Vert::Baseline,
            },
        }
    }

    fn rel_target(&self, rid: &str) -> Option<(String, bool)> {
        let rel = self.rels.iter().find(|r| r.id == rid)?;
        Some(if rel.external {
            (rel.target.clone(), true)
        } else {
            (kalem_ooxml::rels::resolve(self.part, &rel.target), false)
        })
    }

    fn border(&self, b: &crate::props::Border) -> Option<VBorder> {
        b.drawn().then(|| VBorder {
            color: b.color.as_ref().and_then(|c| self.color(c)),
            width: b.size as f32 / 8.0,
            style: b.style.clone(),
        })
    }
}

/// The URL or `#bookmark` a `HYPERLINK` field goes to.
fn hyperlink_field(instr: &str) -> Option<String> {
    let rest = instr.trim().strip_prefix("HYPERLINK")?.trim();
    let mut target = None;
    let mut anchor = None;
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in rest.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    let mut it = words.into_iter();
    while let Some(w) = it.next() {
        match w.as_str() {
            "\\l" => anchor = it.next(),
            s if s.starts_with('\\') => {
                if matches!(s, "\\o" | "\\t") {
                    it.next();
                }
            }
            _ if target.is_none() => target = Some(w),
            _ => {}
        }
    }
    match (target, anchor) {
        (Some(t), Some(a)) => Some(format!("{t}#{a}")),
        (Some(t), None) => Some(t),
        (None, Some(a)) => Some(format!("#{a}")),
        (None, None) => None,
    }
}

/// What surrounds a run while a story is walked.
#[derive(Debug, Clone, Default)]
struct Context {
    link: Option<String>,
    inserted: Option<Revision>,
    deleted: Option<Revision>,
    field: Option<String>,
    lock: Option<Lock>,
}

/// The table style's formats for one cell.
#[derive(Debug, Clone, Default)]
struct CellStyle {
    format: TableFormat,
}

/// A story walked in order: lists counted, fields and comments followed
/// across paragraphs, notes numbered as they are referred to.
#[derive(Debug)]
pub struct Walker<'a> {
    env: Env<'a>,
    counters: Counters<'a>,
    /// Open complex fields: their instruction, and whether their result
    /// has begun.
    fields: Vec<(String, bool)>,
    comments: Vec<String>,
    story: StoryId,
    index: usize,
    /// Footnotes and endnotes referred to, in order, with their marks.
    pub footnotes: Vec<(String, String)>,
    /// Endnotes referred to.
    pub endnotes: Vec<(String, String)>,
    note_mark: Option<String>,
    /// Whether paragraphs keep their edit coordinates ([`VPara::layout`]):
    /// a view only to show leaves them out.
    pub layouts: bool,
    faces: HashMap<String, Arc<str>>,
    footnote_format: (String, i64),
    endnote_format: (String, i64),
    containers: usize,
}

impl<'a> Walker<'a> {
    /// A walker over a story of `env`'s part.
    pub fn new(env: Env<'a>, story: StoryId) -> Walker<'a> {
        let counters = Counters::new(env.numbering, env.styles);
        Walker {
            env,
            counters,
            fields: Vec::new(),
            comments: Vec::new(),
            story,
            index: 0,
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            note_mark: None,
            layouts: true,
            faces: HashMap::new(),
            footnote_format: ("decimal".into(), 1),
            endnote_format: ("lowerRoman".into(), 1),
            containers: 0,
        }
    }

    /// The number formats and starts of footnotes and endnotes
    /// (`w:footnotePr`, `w:endnotePr`).
    pub fn note_formats(&mut self, foot: (String, i64), end: (String, i64)) {
        self.footnote_format = foot;
        self.endnote_format = end;
    }

    /// The mark a note's own `w:footnoteRef` shows, while its blocks are
    /// walked.
    pub fn set_note_mark(&mut self, mark: Option<String>) {
        self.note_mark = mark;
    }

    /// Goes on with another story (a note after the body), its lists and
    /// notes counted on.
    pub fn restart(&mut self, env: Env<'a>, story: StoryId) {
        self.env = env;
        self.story = story;
        self.index = 0;
        self.fields.clear();
        self.comments.clear();
    }

    /// How run properties look, the typeface's name shared by every run
    /// of it.
    fn look(&mut self, p: &RunProps) -> Look {
        let name = self.env.face(p);
        let face = match self.faces.get(&name) {
            Some(f) => f.clone(),
            None => {
                let f: Arc<str> = Arc::from(name.as_str());
                self.faces.insert(name, f.clone());
                f
            }
        };
        self.env.look_with(p, face)
    }

    /// The view of blocks.
    pub fn blocks(&mut self, blocks: &[Block]) -> Vec<VBlock> {
        self.blocks_in(blocks, None, true)
    }

    fn blocks_in(
        &mut self,
        blocks: &[Block],
        cell: Option<&CellStyle>,
        editable: bool,
    ) -> Vec<VBlock> {
        let container = self.containers;
        self.containers += 1;
        let mut out = Vec::new();
        for (i, b) in blocks.iter().enumerate() {
            self.block(b, cell, editable, (container, i), &mut out);
        }
        out
    }

    fn block(
        &mut self,
        b: &Block,
        cell: Option<&CellStyle>,
        editable: bool,
        sibling: (usize, usize),
        out: &mut Vec<VBlock>,
    ) {
        match b {
            Block::Paragraph(p) => {
                let (mut vp, frames) = self.paragraph(p, cell, editable, sibling);
                if !self.layouts {
                    vp.layout = Layout::default();
                }
                // Nothing but lines drawn as shapes: a rule.
                let line = |r: &VRun| matches!(&r.piece, Piece::Placeholder(t) if t == LINE_SHAPE);
                if vp.runs.iter().any(line)
                    && vp.runs.iter().all(|r| line(r) || r.text.trim().is_empty())
                    && frames.is_empty()
                {
                    out.push(VBlock::Rule);
                    return;
                }
                out.push(VBlock::Para(vp));
                for f in frames {
                    out.push(VBlock::Frame(f));
                }
            }
            Block::Table(t) => out.push(VBlock::Table(self.table(t, editable))),
            Block::Control {
                blocks, control, ..
            } => {
                let ok = editable && control.kind != "checkbox" && !control.placeholder;
                out.extend(self.blocks_in(blocks, cell, ok));
            }
            Block::Wrapper { blocks, name, .. } => {
                let ok = editable && name != "AlternateContent";
                out.extend(self.blocks_in(blocks, cell, ok));
            }
            Block::Marker(m) => self.marker(&m.kind),
            Block::Section(_) => {}
            Block::Other { name, .. } => out.push(VBlock::Placeholder(format!("[{name}]"))),
        }
    }

    /// The view of the body of a main document part's text, read a block
    /// at a time so that the whole tree is never held.
    pub fn body(&mut self, text: &str) -> Vec<VBlock> {
        let mut r = kalem_ooxml::xml::Reader::new(text);
        loop {
            match r.next_token() {
                Some(kalem_ooxml::xml::Token::Start(t)) if t.name == "body" => {
                    if t.empty {
                        return Vec::new();
                    }
                    break;
                }
                None => return Vec::new(),
                _ => {}
            }
        }
        let container = self.containers;
        self.containers += 1;
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(t) = r.next_token() {
            match t {
                kalem_ooxml::xml::Token::Start(tag) => {
                    let b = crate::story::block(&mut r, tag);
                    self.block(&b, None, true, (container, i), &mut out);
                    i += 1;
                }
                kalem_ooxml::xml::Token::End { .. } => break,
                kalem_ooxml::xml::Token::Text { .. } => {}
            }
        }
        out
    }

    fn marker(&mut self, m: &MarkerKind) {
        match m {
            MarkerKind::CommentStart { id } => self.comments.push(id.clone()),
            MarkerKind::CommentEnd { id } => self.comments.retain(|c| c != id),
            _ => {}
        }
    }

    fn table(&mut self, t: &Table, editable: bool) -> VTable {
        let styles = self.env.styles;
        let style_id = t
            .props
            .style
            .clone()
            .or_else(|| styles.default_of(StyleKind::Table).map(|s| s.id.clone()));
        let (whole, conds) = style_id
            .as_deref()
            .map(|id| styles.table_style(id))
            .unwrap_or_default();
        let mut tblpr = whole.tblpr.clone();
        tblpr.merge(&t.props);
        let look = tblpr.look.unwrap_or(TableLook {
            first_row: true,
            first_col: true,
            no_v_band: true,
            ..TableLook::default()
        });
        let row_band = tblpr.row_band.unwrap_or(1).max(1) as usize;
        let col_band = tblpr.col_band.unwrap_or(1).max(1) as usize;
        let rows_n = t.rows.len();
        let cols_n = t.grid.len().max(
            t.rows
                .iter()
                .map(|r| {
                    r.cells
                        .iter()
                        .map(|c| c.props.grid_span.unwrap_or(1) as usize)
                        .sum::<usize>()
                })
                .max()
                .unwrap_or(0),
        );
        let mut rows = Vec::new();
        for (i, row) in t.rows.iter().enumerate() {
            let mut cells = Vec::new();
            let mut col = 0usize;
            for c in &row.cells {
                let span = c.props.grid_span.unwrap_or(1).max(1) as usize;
                let last_col = col + span >= cols_n;
                let first_row = look.first_row && i == 0;
                let last_row = look.last_row && i + 1 == rows_n && rows_n > 1;
                let first_col = look.first_col && col == 0;
                let last_col_on = look.last_col && last_col;
                let mut applies: Vec<&str> = Vec::new();
                if !look.no_v_band && !(first_col || last_col_on) {
                    let k = col.saturating_sub(usize::from(look.first_col)) / col_band;
                    applies.push(if k.is_multiple_of(2) {
                        "band1Vert"
                    } else {
                        "band2Vert"
                    });
                }
                if !look.no_h_band && !(first_row || last_row) {
                    let k = i.saturating_sub(usize::from(look.first_row)) / row_band;
                    applies.push(if k.is_multiple_of(2) {
                        "band1Horz"
                    } else {
                        "band2Horz"
                    });
                }
                if first_col {
                    applies.push("firstCol");
                }
                if last_col_on {
                    applies.push("lastCol");
                }
                if first_row {
                    applies.push("firstRow");
                }
                if last_row {
                    applies.push("lastRow");
                }
                if first_row && last_col_on {
                    applies.push("neCell");
                }
                if first_row && first_col {
                    applies.push("nwCell");
                }
                if last_row && last_col_on {
                    applies.push("seCell");
                }
                if last_row && first_col {
                    applies.push("swCell");
                }
                let mut format = whole.clone();
                for cond in CONDITIONS {
                    if applies.contains(&cond)
                        && let Some(f) = conds.get(cond)
                    {
                        format.ppr.merge(&f.ppr);
                        format.rpr.merge(&f.rpr);
                        format.tcpr.merge(&f.tcpr);
                    }
                }
                let mut tcpr = format.tcpr.clone();
                tcpr.merge(&c.props);
                let cell_style = CellStyle { format };
                let blocks = self.blocks_in(&c.blocks, Some(&cell_style), editable);
                let side = |own: &Option<crate::props::Border>,
                            edge: bool,
                            outer: &Option<crate::props::Border>,
                            inner: &Option<crate::props::Border>| {
                    own.as_ref()
                        .or(if edge { outer.as_ref() } else { inner.as_ref() })
                        .and_then(|b| self.env.border(b))
                };
                let tb: &Borders = &tblpr.borders;
                let cb = &tcpr.borders;
                let borders = [
                    side(&cb.top, i == 0, &tb.top, &tb.inside_h),
                    side(&cb.left, col == 0, &tb.left, &tb.inside_v),
                    side(&cb.bottom, i + 1 == rows_n, &tb.bottom, &tb.inside_h),
                    side(&cb.right, last_col, &tb.right, &tb.inside_v),
                ];
                cells.push(VCell {
                    columns: span as u32,
                    merge: tcpr.v_merge,
                    fill: tcpr
                        .shading
                        .as_ref()
                        .and_then(|s| s.fill.as_ref())
                        .and_then(|c| self.env.color(c)),
                    borders,
                    blocks,
                });
                col += span;
            }
            rows.push(VRow {
                cells,
                header: row.props.header.unwrap_or(false),
            });
        }
        VTable {
            style: style_id
                .as_deref()
                .and_then(|id| styles.get(id))
                .map(|s| s.name.clone()),
            grid: t.grid.iter().map(|w| *w as f32 / 20.0).collect(),
            rows,
        }
    }

    /// A paragraph's view, and the text boxes anchored in it.
    fn paragraph(
        &mut self,
        p: &Paragraph,
        cell: Option<&CellStyle>,
        editable: bool,
        sibling: (usize, usize),
    ) -> (VPara, Vec<Vec<VBlock>>) {
        let styles = self.env.styles;
        let style = styles.para_style(p.props.style.as_deref());
        let style_id = style.map(|s| s.id.clone()).unwrap_or_default();
        let (style_ppr, _) = style
            .map(|s| styles.para_style_props(&s.id))
            .unwrap_or_default();
        // The list: the paragraph's own `w:numPr` over its style's.
        let mut num_id = style_ppr.num_id.clone();
        let mut ilvl = style_ppr.ilvl;
        if num_id.is_some() && ilvl.is_none() {
            ilvl = self.env.numbering.level_of_style(
                num_id.as_deref().unwrap_or_default(),
                &style_id,
                styles,
            );
        }
        let direct_num = p.props.num_id.is_some() || p.props.ilvl.is_some();
        if p.props.num_id.is_some() {
            num_id.clone_from(&p.props.num_id);
        }
        if p.props.ilvl.is_some() {
            ilvl = p.props.ilvl;
        }
        let label = num_id
            .as_deref()
            .filter(|n| n.trim() != "0")
            .and_then(|n| self.counters.next(n, ilvl.unwrap_or(0)));
        let mut ppr = styles.para_defaults.clone();
        if let Some(c) = cell {
            ppr.merge(&c.format.ppr);
        }
        if let Some(l) = &label
            && !direct_num
        {
            ppr.merge(&l.ppr);
        }
        ppr.merge(&style_ppr);
        if let Some(l) = &label
            && direct_num
        {
            ppr.merge(&l.ppr);
        }
        let mut own = p.props.clone();
        own.style = None;
        ppr.merge(&own);
        let table_rpr = cell.map(|c| &c.format.rpr);
        let list_level = label.as_ref().map(|l| l.level);
        let label = label.map(|l| {
            let mut rp = styles.resolve_run(Some(&style_id), table_rpr, &p.mark);
            rp.merge(&l.rpr);
            (l.text, l.suffix, self.look(&rp))
        });
        let at = editable.then(|| ParaAt {
            story: self.story.clone(),
            index: self.index,
        });
        if editable {
            self.index += 1;
        }
        let tw = |v: Option<i64>| v.unwrap_or(0) as f32 / 20.0;
        let mut vp = VPara {
            at,
            style: style.map(|s| s.name.clone()).unwrap_or_default(),
            style_id: style_id.clone(),
            label,
            list_level,
            outline: ppr.outline.filter(|o| *o < 9),
            align: match ppr.jc.as_deref() {
                Some("center") => Align::Center,
                Some("right" | "end") => Align::End,
                Some(
                    "both" | "distribute" | "lowKashida" | "mediumKashida" | "highKashida"
                    | "thaiDistribute",
                ) => Align::Justify,
                _ => Align::Start,
            },
            indent: (tw(ppr.ind_left), tw(ppr.ind_right), tw(ppr.ind_first)),
            spacing: (tw(ppr.space_before), tw(ppr.space_after)),
            shading: ppr
                .shading
                .as_ref()
                .and_then(|s| s.fill.as_ref())
                .and_then(|c| self.env.color(c)),
            runs: Vec::new(),
            mark_change: p
                .mark_inserted
                .clone()
                .map(|r| (true, r))
                .or_else(|| p.mark_deleted.clone().map(|r| (false, r))),
            section_break: None,
            layout: Layout {
                part: self.env.part.to_owned(),
                span: p.span.clone(),
                start_tag: p.start_tag.clone(),
                empty: p.empty,
                ppr: p.ppr.clone(),
                mark_rpr: p.mark_span.clone(),
                sect: p.sect.clone(),
                content_end: p.content_end,
                style: p.props.style.clone(),
                mark_deleted: p.mark_deleted.is_some(),
                mark_inserted: p.mark_inserted.is_some(),
                sibling,
                ..Layout::default()
            },
        };
        let mut frames = Vec::new();
        let base = Context {
            lock: (!editable).then_some(Lock::Control),
            ..Context::default()
        };
        for (top, inline) in p.content.iter().enumerate() {
            vp.layout.tops.push(inline.span());
            self.inline(
                inline,
                &base,
                top,
                &style_id,
                table_rpr,
                &mut vp,
                &mut frames,
            );
        }
        (vp, frames)
    }

    #[allow(clippy::too_many_arguments)]
    fn inline(
        &mut self,
        inline: &Inline,
        ctx: &Context,
        top: usize,
        style: &str,
        table_rpr: Option<&RunProps>,
        vp: &mut VPara,
        frames: &mut Vec<Vec<VBlock>>,
    ) {
        match inline {
            Inline::Run(r) => self.run(r, ctx, top, style, table_rpr, vp, frames),
            Inline::Group(g) => {
                let mut c = ctx.clone();
                match &g.kind {
                    GroupKind::Hyperlink { rid, anchor } => {
                        let target = rid
                            .as_deref()
                            .and_then(|id| self.env.rel_target(id))
                            .map(|(t, _)| t);
                        c.link = match (target, anchor) {
                            (Some(t), Some(a)) => Some(format!("{t}#{a}")),
                            (Some(t), None) => Some(t),
                            (None, Some(a)) => Some(format!("#{a}")),
                            (None, None) => None,
                        };
                    }
                    GroupKind::Inserted(rev) | GroupKind::MovedTo(rev) => {
                        c.inserted = Some(rev.clone());
                    }
                    GroupKind::Deleted(rev) | GroupKind::MovedFrom(rev) => {
                        c.deleted = Some(rev.clone());
                        c.lock = Some(Lock::Deleted);
                    }
                    GroupKind::SimpleField { instr } => {
                        c.field = Some(instr.clone());
                        c.lock.get_or_insert(Lock::Field);
                        if let Some(l) = hyperlink_field(instr) {
                            c.link = Some(l);
                        }
                    }
                    GroupKind::Control(Control {
                        kind, placeholder, ..
                    }) => {
                        if kind == "checkbox" || *placeholder {
                            c.lock.get_or_insert(Lock::Control);
                        }
                    }
                    GroupKind::Alternate => {
                        c.lock.get_or_insert(Lock::Alternate);
                    }
                    GroupKind::Wrapper(_) => {}
                }
                for i in &g.content {
                    self.inline(i, &c, top, style, table_rpr, vp, frames);
                }
            }
            Inline::Marker(m) => self.marker(&m.kind),
            Inline::Math { span, text } => {
                let r = RunInfo {
                    span: span.clone(),
                    start_tag: span.start..span.start,
                    rpr: None,
                    end_tag: span.end..span.end,
                    atoms: vec![span.clone()],
                    top,
                    lock: ctx.lock,
                    inserted: ctx.inserted.is_some(),
                };
                let look = self.look(&self.env.styles.resolve_run(
                    Some(style),
                    table_rpr,
                    &RunProps::default(),
                ));
                self.object(
                    vp,
                    r,
                    span.clone(),
                    ctx,
                    look,
                    Piece::Placeholder(format!("[Equation: {text}]")),
                );
            }
            Inline::Other { name, span } => {
                let r = RunInfo {
                    span: span.clone(),
                    start_tag: span.start..span.start,
                    rpr: None,
                    end_tag: span.end..span.end,
                    atoms: vec![span.clone()],
                    top,
                    lock: ctx.lock,
                    inserted: ctx.inserted.is_some(),
                };
                let look = self.look(&self.env.styles.resolve_run(
                    Some(style),
                    table_rpr,
                    &RunProps::default(),
                ));
                self.object(
                    vp,
                    r,
                    span.clone(),
                    ctx,
                    look,
                    Piece::Placeholder(format!("[{name}]")),
                );
            }
        }
    }

    /// An object outside runs: one character of the edit text, deleted as
    /// a whole.
    fn object(
        &mut self,
        vp: &mut VPara,
        r: RunInfo,
        atom: Span,
        ctx: &Context,
        look: Look,
        piece: Piece,
    ) {
        let run = vp.layout.runs.len();
        vp.layout.runs.push(r);
        let start = vp.layout.text.len();
        vp.layout.text.push(OBJECT);
        let range = start..vp.layout.text.len();
        vp.layout.segs.push(Seg {
            range: range.clone(),
            run,
            atom,
            text: None,
            lock: ctx.lock,
        });
        let text = match &piece {
            Piece::Placeholder(t) => t.clone(),
            _ => String::new(),
        };
        vp.runs.push(VRun {
            text,
            piece,
            source: range,
            look,
            link: ctx.link.clone(),
            inserted: ctx.inserted.clone().map(Box::new),
            deleted: ctx.deleted.clone().map(Box::new),
            format_changed: None,
            comments: self.comments.clone(),
            field: ctx.field.clone(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        r: &Run,
        ctx: &Context,
        top: usize,
        style: &str,
        table_rpr: Option<&RunProps>,
        vp: &mut VPara,
        frames: &mut Vec<Vec<VBlock>>,
    ) {
        let resolved = self
            .env
            .styles
            .resolve_run(Some(style), table_rpr, &r.props);
        let look = self.look(&resolved);
        let mut atoms: Vec<Span> = Vec::new();
        for a in &r.atoms {
            if atoms.last() != Some(&a.span) {
                atoms.push(a.span.clone());
            }
        }
        let run = vp.layout.runs.len();
        vp.layout.runs.push(RunInfo {
            span: r.span.clone(),
            start_tag: r.start_tag.clone(),
            rpr: r.rpr.clone(),
            end_tag: r.end_tag.clone(),
            atoms,
            top,
            lock: ctx.lock,
            inserted: ctx.inserted.is_some(),
        });
        for a in &r.atoms {
            // Complex fields: the code is hidden, the result shown.
            match &a.kind {
                AtomKind::FieldChar(FieldChar::Begin) => {
                    self.fields.push((String::new(), false));
                    continue;
                }
                AtomKind::FieldChar(FieldChar::Separate) => {
                    if let Some(f) = self.fields.last_mut() {
                        f.1 = true;
                    }
                    continue;
                }
                AtomKind::FieldChar(FieldChar::End) => {
                    self.fields.pop();
                    continue;
                }
                AtomKind::Instr(t) => {
                    if let Some(f) = self.fields.last_mut()
                        && !f.1
                    {
                        f.0.push_str(t);
                    }
                    continue;
                }
                _ => {}
            }
            let in_code = self.fields.iter().any(|f| !f.1);
            if in_code {
                continue;
            }
            let mut c = ctx.clone();
            if let Some((instr, _)) = self.fields.last() {
                c.field = Some(instr.clone());
                c.lock.get_or_insert(Lock::Field);
                if c.link.is_none() {
                    c.link = self
                        .fields
                        .iter()
                        .rev()
                        .find_map(|(i, _)| hyperlink_field(i));
                }
            }
            let (src, shown, piece, lock): (String, String, Piece, Option<Lock>) = match &a.kind {
                AtomKind::Text { text, deleted, .. } => {
                    let lock = if *deleted {
                        Some(Lock::Deleted)
                    } else {
                        c.lock
                    };
                    // Text in a symbol font is shown as what it draws.
                    let shown = if text
                        .chars()
                        .any(|ch| ('\u{F020}'..='\u{F0FF}').contains(&ch))
                    {
                        text.chars()
                            .map(|ch| crate::chars::symbol(&look.face, ch))
                            .collect()
                    } else {
                        text.clone()
                    };
                    (text.clone(), shown, Piece::Text, lock)
                }
                AtomKind::Tab => ("\t".into(), "\t".into(), Piece::Tab, c.lock),
                AtomKind::Break(BreakKind::Line) => (
                    LINE_BREAK.to_string(),
                    "\n".into(),
                    Piece::LineBreak,
                    c.lock,
                ),
                AtomKind::Break(BreakKind::Page) => (
                    PAGE_BREAK.to_string(),
                    "\n".into(),
                    Piece::PageBreak,
                    c.lock,
                ),
                AtomKind::Break(BreakKind::Column) => (
                    COLUMN_BREAK.to_string(),
                    "\n".into(),
                    Piece::ColumnBreak,
                    c.lock,
                ),
                AtomKind::Symbol { ch, .. } => {
                    (ch.to_string(), ch.to_string(), Piece::Text, c.lock)
                }
                AtomKind::NoBreakHyphen => {
                    ("\u{2011}".into(), "\u{2011}".into(), Piece::Text, c.lock)
                }
                AtomKind::SoftHyphen => ("\u{AD}".into(), String::new(), Piece::Text, c.lock),
                AtomKind::FootnoteRef(id) => {
                    let mark = self.note(true, id);
                    (
                        OBJECT.to_string(),
                        mark,
                        Piece::FootnoteRef(id.clone()),
                        Some(c.lock.unwrap_or(Lock::NoteRef)),
                    )
                }
                AtomKind::EndnoteRef(id) => {
                    let mark = self.note(false, id);
                    (
                        OBJECT.to_string(),
                        mark,
                        Piece::EndnoteRef(id.clone()),
                        Some(c.lock.unwrap_or(Lock::NoteRef)),
                    )
                }
                AtomKind::NoteMark => {
                    let mark = self.note_mark.clone().unwrap_or_default();
                    (OBJECT.to_string(), mark, Piece::Text, Some(Lock::NoteRef))
                }
                AtomKind::CommentRef(_)
                | AtomKind::AnnotationMark
                | AtomKind::Separator
                | AtomKind::RenderedPageBreak
                | AtomKind::FieldChar(_)
                | AtomKind::Instr(_) => continue,
                AtomKind::Drawing(d) => {
                    let lock = c.lock;
                    let size = (d.size.0 as f32 / 12_700.0, d.size.1 as f32 / 12_700.0);
                    let piece = match &d.graphic {
                        Graphic::Picture { embed, link } => {
                            let target = embed
                                .as_deref()
                                .or(link.as_deref())
                                .and_then(|id| self.env.rel_target(id))
                                .map(|(t, _)| t);
                            Piece::Picture {
                                target,
                                size,
                                alt: d.descr.clone(),
                                floating: d.floating,
                            }
                        }
                        Graphic::TextBox(blocks) => {
                            // The box's paragraphs are shown after this
                            // one; they are not edited yet.
                            let saved = std::mem::take(&mut self.fields);
                            let inner = self.blocks_in(blocks, None, false);
                            self.fields = saved;
                            frames.push(inner);
                            Piece::Placeholder("[Text box]".into())
                        }
                        Graphic::Shape => Piece::Placeholder("[Shape]".into()),
                        Graphic::Line => Piece::Placeholder(LINE_SHAPE.into()),
                        Graphic::Group => Piece::Placeholder("[Group of shapes]".into()),
                        Graphic::Canvas => Piece::Placeholder("[Drawing canvas]".into()),
                        Graphic::Chart(_) => Piece::Placeholder("[Chart]".into()),
                        Graphic::Diagram => Piece::Placeholder("[SmartArt]".into()),
                        Graphic::Ink => Piece::Placeholder("[Ink]".into()),
                        Graphic::Unknown => Piece::Placeholder("[Drawing]".into()),
                    };
                    let shown = match &piece {
                        Piece::Placeholder(t) => t.clone(),
                        Piece::Picture { alt, .. } if !alt.is_empty() => {
                            format!("[Picture: {alt}]")
                        }
                        _ => "[Picture]".into(),
                    };
                    (OBJECT.to_string(), shown, piece, lock)
                }
                AtomKind::Object { prog_id, .. } => {
                    let name = prog_id.clone().unwrap_or_else(|| "object".into());
                    let shown = format!("[Object: {name}]");
                    (
                        OBJECT.to_string(),
                        shown.clone(),
                        Piece::Placeholder(shown),
                        c.lock,
                    )
                }
                AtomKind::Other(name) => {
                    let shown = format!("[{name}]");
                    (
                        OBJECT.to_string(),
                        shown.clone(),
                        Piece::Placeholder(shown),
                        c.lock,
                    )
                }
            };
            let start = vp.layout.text.len();
            vp.layout.text.push_str(&src);
            let range = start..vp.layout.text.len();
            let text_span = match &a.kind {
                AtomKind::Text {
                    inner, preserve, ..
                } => Some((inner.clone(), *preserve)),
                _ => None,
            };
            if !src.is_empty() {
                vp.layout.segs.push(Seg {
                    range: range.clone(),
                    run,
                    atom: a.span.clone(),
                    text: text_span,
                    lock,
                });
            }
            let mut look = look.clone();
            if matches!(piece, Piece::FootnoteRef(_) | Piece::EndnoteRef(_))
                && look.vert == Vert::Baseline
            {
                look.vert = Vert::Super;
            }
            vp.runs.push(VRun {
                text: shown,
                piece,
                source: range,
                look,
                link: c.link.clone(),
                inserted: c.inserted.clone().map(Box::new),
                deleted: c
                    .deleted
                    .clone()
                    .or_else(|| {
                        matches!(&a.kind, AtomKind::Text { deleted: true, .. })
                            .then(Revision::default)
                    })
                    .map(Box::new),
                format_changed: r.format_change.clone().map(Box::new),
                comments: self.comments.clone(),
                field: c.field.clone(),
            });
        }
    }

    /// A note's mark, numbered the first time the note is referred to.
    fn note(&mut self, foot: bool, id: &str) -> String {
        let (list, (fmt, start)) = if foot {
            (&mut self.footnotes, &self.footnote_format)
        } else {
            (&mut self.endnotes, &self.endnote_format)
        };
        if let Some((_, m)) = list.iter().find(|(i, _)| i == id) {
            return m.clone();
        }
        let n = start + list.len() as i64;
        let mark = crate::chars::format_number(n, fmt);
        list.push((id.to_owned(), mark.clone()));
        mark
    }
}

/// The text of blocks as shown: a paragraph a line, its label first; a
/// table row a line, its cells apart by tabs; hidden and deleted text and
/// fields' codes left out.
pub fn text(blocks: &[VBlock], out: &mut String) {
    for b in blocks {
        match b {
            VBlock::Para(p) => {
                out.push_str(&para_text(p));
                out.push('\n');
            }
            VBlock::Table(t) => {
                for row in &t.rows {
                    let cells: Vec<String> = row
                        .cells
                        .iter()
                        .map(|c| {
                            let mut s = String::new();
                            text(&c.blocks, &mut s);
                            s.trim_end_matches('\n').replace('\n', " ")
                        })
                        .collect();
                    out.push_str(&cells.join("\t"));
                    out.push('\n');
                }
            }
            VBlock::Frame(blocks) => text(blocks, out),
            VBlock::Placeholder(p) => {
                out.push_str(p);
                out.push('\n');
            }
            VBlock::Rule => out.push('\n'),
        }
    }
}

/// A paragraph's text as shown.
pub fn para_text(p: &VPara) -> String {
    let mut s = String::new();
    if let Some((label, suffix, _)) = &p.label {
        s.push_str(label);
        match suffix {
            Suffix::Tab => s.push('\t'),
            Suffix::Space => s.push(' '),
            Suffix::Nothing => {}
        }
    }
    for r in &p.runs {
        if r.look.hidden || r.deleted.is_some() {
            continue;
        }
        if r.look.caps {
            s.push_str(&r.text.to_uppercase());
        } else {
            s.push_str(&r.text);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tints_and_shades_as_word_writes_them() {
        // accent1 4472C4 shaded BF is the 2F5496 Word writes beside it;
        // text1 black tinted BF is 404040.
        assert_eq!(tinted(0x4472C4, None, Some(0xBF)), 0x2F5496);
        assert_eq!(tinted(0x000000, Some(0xBF), None), 0x404040);
        let t = tinted(0x4472C4, Some(0x33), None);
        let close = |a: u32, b: u32| (a as i32 - b as i32).abs() <= 1;
        assert!(
            close(t >> 16, 0xD9) && close((t >> 8) & 0xFF, 0xE2) && close(t & 0xFF, 0xF3),
            "{t:06X}"
        );
        assert_eq!(tinted(0x123456, None, None), 0x123456);
    }

    #[test]
    fn hyperlink_fields() {
        assert_eq!(
            hyperlink_field(r#" HYPERLINK "https://a.b/c" "#).as_deref(),
            Some("https://a.b/c")
        );
        assert_eq!(
            hyperlink_field(r#" HYPERLINK \l "intro" "#).as_deref(),
            Some("#intro")
        );
        assert_eq!(
            hyperlink_field(r#"HYPERLINK "x.docx" \l "b" \o "tip""#).as_deref(),
            Some("x.docx#b")
        );
        assert_eq!(hyperlink_field(" PAGE "), None);
    }
}
