//! A story of WordprocessingML (the body, a header, a footer, a note, a
//! comment: ECMA-376 part 1, 17.2 to 17.4) read into blocks, paragraphs,
//! runs and what runs hold, each with the byte span it was read from, so
//! that an edit rewrites the element it touches and nothing else.
//!
//! The reader keeps what it does not know: an element it does not read is
//! kept as [`Block::Other`], [`Inline::Other`] or [`AtomKind::Other`] with
//! its span, shown as a placeholder naming it, and written back as it was.
//! `mc:AlternateContent` is read through the choice the plugin
//! understands (a text box drawn with `wps`), else its fallback.

use kalem_ooxml::xml::{Reader, Tag, Token};

use crate::chars;
use crate::props::{
    self, CellProps, ParaProps, Revision, RowProps, RunProps, Span, TableProps, children,
};

/// The markup-compatibility namespaces (by their usual prefixes) whose
/// `mc:Choice` the reader takes: the ones whose content it reads as
/// well as or better than the fallback's.
const UNDERSTOOD: &[&str] = &[
    "wps", "wpg", "wpc", "wpi", "wp14", "w14", "w15", "w16", "w16se", "w16cid", "w16cex", "w16du",
    "w16sdtdh", "w16sdtfl", "a14", "m", "v", "o", "w10", "wne",
];

/// A block of a story.
// Paragraphs are most blocks: boxing them would cost an allocation each.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A paragraph.
    Paragraph(Paragraph),
    /// A table.
    Table(Table),
    /// A content control around blocks (`w:sdt`).
    Control {
        /// The whole element.
        span: Span,
        /// What it is.
        control: Control,
        /// Its blocks.
        blocks: Vec<Block>,
    },
    /// Custom XML around blocks (`w:customXml`), or the choice read of an
    /// `mc:AlternateContent` around blocks.
    Wrapper {
        /// The whole element.
        span: Span,
        /// Its name (`customXml`, `AlternateContent`).
        name: String,
        /// Its blocks.
        blocks: Vec<Block>,
    },
    /// A bookmark's or a comment's start or end between blocks.
    Marker(Marker),
    /// The body's last section's properties (`w:sectPr`).
    Section(Span),
    /// An element the reader does not know, kept.
    Other {
        /// Its name.
        name: String,
        /// The whole element.
        span: Span,
    },
}

/// A paragraph (`w:p`).
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    /// The whole element.
    pub span: Span,
    /// Its start tag (`<w:p …>`, or the whole `<w:p/>`).
    pub start_tag: Span,
    /// Written as `<w:p/>`.
    pub empty: bool,
    /// Its properties' element.
    pub ppr: Option<Span>,
    /// Its own properties.
    pub props: ParaProps,
    /// The paragraph mark's run properties.
    pub mark: RunProps,
    /// Where the mark's `w:rPr` is.
    pub mark_span: Option<Span>,
    /// The paragraph mark inserted, a tracked change.
    pub mark_inserted: Option<Revision>,
    /// The paragraph mark deleted: the paragraph joins the next one once
    /// the change is accepted.
    pub mark_deleted: Option<Revision>,
    /// A tracked change of its properties.
    pub props_change: Option<Revision>,
    /// The section it ends (`w:pPr/w:sectPr`).
    pub sect: Option<Span>,
    /// Its content.
    pub content: Vec<Inline>,
    /// Where its end tag starts (its span's end for `<w:p/>`).
    pub content_end: usize,
}

/// What a paragraph holds.
// Runs are most inlines: boxing them would cost an allocation each.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// A run.
    Run(Run),
    /// Inlines grouped by a link, a tracked change, a content control, a
    /// simple field…
    Group(Group),
    /// A start or end of a bookmark or a comment's range.
    Marker(Marker),
    /// Office Math (`m:oMath`, `m:oMathPara`), with its text.
    Math {
        /// The whole element.
        span: Span,
        /// Its text, as the reader's linear form.
        text: String,
    },
    /// An element the reader does not know, kept.
    Other {
        /// Its name.
        name: String,
        /// The whole element.
        span: Span,
    },
}

impl Inline {
    /// The whole element's span.
    pub fn span(&self) -> Span {
        match self {
            Inline::Run(r) => r.span.clone(),
            Inline::Group(g) => g.span.clone(),
            Inline::Marker(m) => m.span.clone(),
            Inline::Math { span, .. } | Inline::Other { span, .. } => span.clone(),
        }
    }
}

/// A group of inlines.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// What groups them.
    pub kind: GroupKind,
    /// The whole element.
    pub span: Span,
    /// Its start tag.
    pub start_tag: Span,
    /// The inlines.
    pub content: Vec<Inline>,
    /// Its end tag (empty at the span's end for a self-closing one).
    pub end_tag: Span,
}

/// What groups inlines.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupKind {
    /// A hyperlink (`w:hyperlink`): an external target by relationship,
    /// or a bookmark in the document.
    Hyperlink {
        /// `r:id`.
        rid: Option<String>,
        /// `w:anchor`.
        anchor: Option<String>,
    },
    /// Inserted text, a tracked change (`w:ins`).
    Inserted(Revision),
    /// Deleted text (`w:del`).
    Deleted(Revision),
    /// Text moved away (`w:moveFrom`).
    MovedFrom(Revision),
    /// Text moved here (`w:moveTo`).
    MovedTo(Revision),
    /// A content control (`w:sdt`).
    Control(Control),
    /// A simple field (`w:fldSimple`): its instruction, its runs the
    /// result.
    SimpleField {
        /// The field's instruction (` DATE \@ "yyyy" `).
        instr: String,
    },
    /// A smart tag, custom XML, or a direction override.
    Wrapper(String),
    /// The choice read of an `mc:AlternateContent`.
    Alternate,
}

/// A content control's properties (`w:sdtPr`) as far as the view needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Control {
    /// `w:alias`.
    pub alias: Option<String>,
    /// `w:tag`.
    pub tag: Option<String>,
    /// What it is: `text`, `richText`, `date`, `dropDownList`,
    /// `comboBox`, `picture`, `checkbox`, `docPartObj`, `group`…
    pub kind: String,
    /// A check box's state.
    pub checked: Option<bool>,
    /// Its content is the placeholder text (`w:showingPlcHdr`).
    pub placeholder: bool,
}

/// A bookmark's or a comment's start or end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// What it marks.
    pub kind: MarkerKind,
    /// The element.
    pub span: Span,
}

/// What a marker marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerKind {
    /// `w:bookmarkStart`.
    BookmarkStart {
        /// `w:id`.
        id: String,
        /// `w:name`.
        name: String,
    },
    /// `w:bookmarkEnd`.
    BookmarkEnd {
        /// `w:id`.
        id: String,
    },
    /// `w:commentRangeStart`.
    CommentStart {
        /// The comment's `w:id`.
        id: String,
    },
    /// `w:commentRangeEnd`.
    CommentEnd {
        /// The comment's `w:id`.
        id: String,
    },
    /// Another marker with no content (`w:proofErr`, `w:permStart`, a
    /// move's range…), by name.
    Other(String),
}

/// A run (`w:r`).
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// The whole element.
    pub span: Span,
    /// Its start tag (the whole of `<w:r/>`).
    pub start_tag: Span,
    /// Its properties' element.
    pub rpr: Option<Span>,
    /// Its own properties.
    pub props: RunProps,
    /// A tracked change of its formatting.
    pub format_change: Option<Revision>,
    /// What it holds, in order.
    pub atoms: Vec<Atom>,
    /// Its end tag (empty at the span's end for `<w:r/>`).
    pub end_tag: Span,
}

/// One thing a run holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    /// What it is.
    pub kind: AtomKind,
    /// The element (for one read through `mc:AlternateContent`, the
    /// whole of that).
    pub span: Span,
}

/// A break's kind (`w:br w:type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
    /// A line break.
    Line,
    /// A page break.
    Page,
    /// A column break.
    Column,
}

/// A complex field's character (`w:fldChar`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldChar {
    /// The field begins; its instruction follows.
    Begin,
    /// Its result follows.
    Separate,
    /// It ends.
    End,
}

/// What a run holds.
#[derive(Debug, Clone, PartialEq)]
pub enum AtomKind {
    /// Text (`w:t`), or deleted text (`w:delText`).
    Text {
        /// The text as shown: entities resolved, and without the leading
        /// and trailing white space Word drops when `xml:space` is not
        /// `preserve`.
        text: String,
        /// Its content's span, between the tags.
        inner: Span,
        /// `xml:space="preserve"`.
        preserve: bool,
        /// `w:delText`.
        deleted: bool,
    },
    /// A tab (`w:tab`, `w:ptab`).
    Tab,
    /// A break (`w:br`, `w:cr`).
    Break(BreakKind),
    /// A symbol (`w:sym`): its font and the character it shows.
    Symbol {
        /// The font (`Wingdings`).
        font: String,
        /// The character as Unicode.
        ch: char,
    },
    /// A non-breaking hyphen.
    NoBreakHyphen,
    /// An optional hyphen.
    SoftHyphen,
    /// A footnote's reference mark.
    FootnoteRef(String),
    /// An endnote's reference mark.
    EndnoteRef(String),
    /// A comment's reference mark.
    CommentRef(String),
    /// A note's own number inside it (`w:footnoteRef`, `w:endnoteRef`).
    NoteMark,
    /// A comment's mark inside it (`w:annotationRef`).
    AnnotationMark,
    /// The separator line of the notes (`w:separator`,
    /// `w:continuationSeparator`).
    Separator,
    /// A complex field's character.
    FieldChar(FieldChar),
    /// A complex field's instruction text (`w:instrText`, or deleted,
    /// `w:delInstrText`).
    Instr(String),
    /// A picture, a shape, a text box, a chart… (`w:drawing`, `w:pict`).
    Drawing(Drawing),
    /// An embedded object (`w:object`).
    Object {
        /// Its program (`Excel.Sheet.12`).
        prog_id: Option<String>,
        /// Its picture's relationship, when it has one.
        preview: Option<String>,
    },
    /// Where Word last broke a page (`w:lastRenderedPageBreak`): nothing.
    RenderedPageBreak,
    /// An element the reader does not know, kept.
    Other(String),
}

/// A drawing's anchor and what it shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drawing {
    /// Floating (`wp:anchor`) rather than in line with the text.
    pub floating: bool,
    /// `wp:docPr`'s name.
    pub name: String,
    /// Its alternative text (`descr`).
    pub descr: String,
    /// Its size in EMU (914,400 to the inch).
    pub size: (i64, i64),
    /// What it is.
    pub graphic: Graphic,
    /// Written in VML (`w:pict`).
    pub vml: bool,
}

/// What a drawing shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Graphic {
    /// A picture: its relationship (`r:embed`) or link (`r:link`).
    Picture {
        /// The embedded image's relationship.
        embed: Option<String>,
        /// The linked image's relationship.
        link: Option<String>,
    },
    /// A text box, with its blocks.
    TextBox(Vec<Block>),
    /// A shape.
    Shape,
    /// A group of shapes.
    Group,
    /// A drawing canvas.
    Canvas,
    /// A chart: its relationship.
    Chart(Option<String>),
    /// SmartArt.
    Diagram,
    /// Ink.
    Ink,
    /// Something else, by its `graphicData` URI.
    #[default]
    Unknown,
}

/// A table (`w:tbl`).
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// The whole element.
    pub span: Span,
    /// Its properties.
    pub props: TableProps,
    /// Its grid's columns, in twentieths of a point.
    pub grid: Vec<i64>,
    /// Its rows.
    pub rows: Vec<Row>,
}

/// A row (`w:tr`).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The whole element.
    pub span: Span,
    /// Its properties.
    pub props: RowProps,
    /// Its cells.
    pub cells: Vec<Cell>,
}

/// A cell (`w:tc`).
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The whole element.
    pub span: Span,
    /// Its properties.
    pub props: CellProps,
    /// Its blocks.
    pub blocks: Vec<Block>,
}

/// The `Requires` of an `mc:Choice` is one the reader takes.
fn understood(requires: &str) -> bool {
    requires.split_whitespace().all(|p| UNDERSTOOD.contains(&p))
}

/// Calls `f` with the reader inside the branch of an `mc:AlternateContent`
/// (whose start tag was just read) that is read, until that branch's end;
/// skips the others. Returns where the whole element ends.
fn alternate<'a>(
    r: &mut Reader<'a>,
    tag: &Tag<'a>,
    mut f: impl FnMut(&mut Reader<'a>, &Tag<'a>),
) -> usize {
    if tag.empty {
        return tag.span.end;
    }
    let mut chosen = false;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(c) => {
                let take = !chosen
                    && match c.name {
                        "Choice" => understood(&c.attr("Requires").unwrap_or_default()),
                        "Fallback" => true,
                        _ => false,
                    };
                if take {
                    chosen = true;
                    f(r, &c);
                } else if !c.empty {
                    r.skip_element();
                }
            }
            Token::End { span, .. } => return span.end,
            Token::Text { .. } => {}
        }
    }
    r.pos()
}

/// The end of an element whose start tag was just read, reading nothing.
fn end_of(r: &mut Reader<'_>, tag: &Tag<'_>) -> usize {
    if tag.empty {
        tag.span.end
    } else {
        r.skip_element()
    }
}

fn marker(tag: &Tag<'_>) -> Option<MarkerKind> {
    let id = || tag.attr("id").unwrap_or_default().into_owned();
    Some(match tag.name {
        "bookmarkStart" => MarkerKind::BookmarkStart {
            id: id(),
            name: tag.attr("name").unwrap_or_default().into_owned(),
        },
        "bookmarkEnd" => MarkerKind::BookmarkEnd { id: id() },
        "commentRangeStart" => MarkerKind::CommentStart { id: id() },
        "commentRangeEnd" => MarkerKind::CommentEnd { id: id() },
        "proofErr"
        | "permStart"
        | "permEnd"
        | "moveFromRangeStart"
        | "moveFromRangeEnd"
        | "moveToRangeStart"
        | "moveToRangeEnd"
        | "customXmlInsRangeStart"
        | "customXmlInsRangeEnd"
        | "customXmlDelRangeStart"
        | "customXmlDelRangeEnd"
        | "customXmlMoveFromRangeStart"
        | "customXmlMoveFromRangeEnd"
        | "customXmlMoveToRangeStart"
        | "customXmlMoveToRangeEnd" => MarkerKind::Other(tag.name.to_owned()),
        _ => return None,
    })
}

/// Reads blocks until the end tag of the element whose start tag was just
/// read (`w:body`, `w:tc`, `w:txbxContent`, a note…).
pub fn parse_blocks<'a>(r: &mut Reader<'a>) -> Vec<Block> {
    let mut out = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => out.push(block(r, tag)),
            Token::End { .. } => break,
            Token::Text { .. } => {}
        }
    }
    out
}

/// Reads a block whose start tag was just read.
pub(crate) fn block<'a>(r: &mut Reader<'a>, tag: Tag<'a>) -> Block {
    let start = tag.span.start;
    match tag.name {
        "p" => Block::Paragraph(paragraph(r, &tag)),
        "tbl" => Block::Table(table(r, &tag)),
        "sdt" => {
            let mut control = Control::default();
            let mut blocks = Vec::new();
            children(r, &tag, |r, c| match c.name {
                "sdtPr" => {
                    control = control_props(r, &c);
                    true
                }
                "sdtContent" => {
                    if !c.empty {
                        blocks = parse_blocks(r);
                    }
                    true
                }
                _ => false,
            });
            Block::Control {
                span: start..r.pos(),
                control,
                blocks,
            }
        }
        "customXml" | "AlternateContent" => {
            let name = tag.name.to_owned();
            let mut blocks = Vec::new();
            let end = if tag.name == "AlternateContent" {
                alternate(r, &tag, |r, c| {
                    if !c.empty {
                        blocks = parse_blocks(r);
                    }
                })
            } else {
                children(r, &tag, |r, c| {
                    if c.name == "customXmlPr" {
                        return false;
                    }
                    blocks.push(block(r, c));
                    true
                });
                r.pos()
            };
            Block::Wrapper {
                span: start..end,
                name,
                blocks,
            }
        }
        "sectPr" => Block::Section(start..end_of(r, &tag)),
        _ => {
            if let Some(kind) = marker(&tag) {
                let end = end_of(r, &tag);
                return Block::Marker(Marker {
                    kind,
                    span: start..end,
                });
            }
            let name = tag.name.to_owned();
            Block::Other {
                name,
                span: start..end_of(r, &tag),
            }
        }
    }
}

fn control_props<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> Control {
    let mut c = Control {
        kind: "richText".into(),
        ..Control::default()
    };
    children(r, tag, |r, p| {
        match p.name {
            "alias" => c.alias = p.attr("val").map(|v| v.into_owned()),
            "tag" => c.tag = p.attr("val").map(|v| v.into_owned()),
            "showingPlcHdr" => c.placeholder = props::on_off(&p),
            "checkbox" => {
                c.kind = "checkbox".into();
                children(r, &p, |_, k| {
                    if k.name == "checked" {
                        c.checked = Some(props::on_off(&k));
                    }
                    false
                });
                return true;
            }
            "text"
            | "date"
            | "dropDownList"
            | "comboBox"
            | "picture"
            | "docPartObj"
            | "docPartList"
            | "group"
            | "citation"
            | "bibliography"
            | "equation"
            | "richText"
            | "repeatingSection"
            | "repeatingSectionItem" => c.kind = p.name.to_owned(),
            _ => {}
        }
        false
    });
    c
}

/// Reads a paragraph whose start tag was just read.
pub fn paragraph<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> Paragraph {
    let mut p = Paragraph {
        span: tag.span.clone(),
        start_tag: tag.span.clone(),
        empty: tag.empty,
        ppr: None,
        props: ParaProps::default(),
        mark: RunProps::default(),
        mark_span: None,
        mark_inserted: None,
        mark_deleted: None,
        props_change: None,
        sect: None,
        content: Vec::new(),
        content_end: tag.span.end,
    };
    if tag.empty {
        return p;
    }
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(c) if c.name == "pPr" => {
                let start = c.span.start;
                let read = props::read_ppr(r, &c);
                let end = if c.empty { c.span.end } else { r.pos() };
                p.ppr = Some(start..end);
                p.props = read.props;
                p.props_change = read.change;
                p.sect = read.sect;
                if let Some((m, span)) = read.mark {
                    p.mark = m.props;
                    p.mark_inserted = m.inserted;
                    p.mark_deleted = m.deleted;
                    p.mark_span = Some(span);
                }
            }
            Token::Start(c) => {
                let i = inline(r, c);
                p.content.push(i);
            }
            Token::End { span, .. } => {
                p.content_end = span.start;
                p.span = tag.span.start..span.end;
                return p;
            }
            Token::Text { .. } => {}
        }
    }
    p.span = tag.span.start..r.pos();
    p.content_end = r.pos();
    p
}

/// Reads inlines until the end of the element whose start tag was just
/// read; returns them and that end tag's span.
fn inlines<'a>(r: &mut Reader<'a>) -> (Vec<Inline>, Span) {
    let mut out = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(c) => out.push(inline(r, c)),
            Token::End { span, .. } => return (out, span),
            Token::Text { .. } => {}
        }
    }
    let end = r.pos();
    (out, end..end)
}

fn inline<'a>(r: &mut Reader<'a>, tag: Tag<'a>) -> Inline {
    let start = tag.span.start;
    let group = |kind: GroupKind, r: &mut Reader<'a>, tag: &Tag<'a>| -> Inline {
        if tag.empty {
            return Inline::Group(Group {
                kind,
                span: tag.span.clone(),
                start_tag: tag.span.clone(),
                content: Vec::new(),
                end_tag: tag.span.end..tag.span.end,
            });
        }
        let (content, end_tag) = inlines(r);
        Inline::Group(Group {
            kind,
            span: tag.span.start..end_tag.end,
            start_tag: tag.span.clone(),
            content,
            end_tag,
        })
    };
    match tag.name {
        "r" => Inline::Run(run(r, &tag)),
        "hyperlink" => {
            let kind = GroupKind::Hyperlink {
                rid: tag.attr("id").map(|v| v.into_owned()),
                anchor: tag.attr("anchor").map(|v| v.into_owned()),
            };
            group(kind, r, &tag)
        }
        "ins" => group(GroupKind::Inserted(Revision::read(&tag)), r, &tag),
        "del" => group(GroupKind::Deleted(Revision::read(&tag)), r, &tag),
        "moveFrom" => group(GroupKind::MovedFrom(Revision::read(&tag)), r, &tag),
        "moveTo" => group(GroupKind::MovedTo(Revision::read(&tag)), r, &tag),
        "fldSimple" => {
            let instr = tag.attr("instr").unwrap_or_default().into_owned();
            group(GroupKind::SimpleField { instr }, r, &tag)
        }
        "smartTag" | "customXml" | "dir" | "bdo" => {
            // Their properties (`w:smartTagPr`, `w:customXmlPr`) are read
            // as unknown inlines and kept.
            group(GroupKind::Wrapper(tag.name.to_owned()), r, &tag)
        }
        "sdt" => {
            let mut control = Control::default();
            let mut content = Vec::new();
            let mut content_tag = tag.span.clone();
            let mut end_tag = tag.span.end..tag.span.end;
            children(r, &tag, |r, c| match c.name {
                "sdtPr" => {
                    control = control_props(r, &c);
                    true
                }
                "sdtContent" => {
                    content_tag = c.span.clone();
                    if !c.empty {
                        let (inl, end) = inlines(r);
                        content = inl;
                        end_tag = end;
                    } else {
                        end_tag = c.span.end..c.span.end;
                    }
                    true
                }
                _ => false,
            });
            Inline::Group(Group {
                kind: GroupKind::Control(control),
                span: start..r.pos(),
                // The group's own tags are its content's, so that an
                // insertion lands inside `w:sdtContent`.
                start_tag: content_tag,
                content,
                end_tag,
            })
        }
        "AlternateContent" => {
            let mut content = Vec::new();
            let mut start_tag = tag.span.clone();
            let mut end_tag = tag.span.end..tag.span.end;
            let end = alternate(r, &tag, |r, c| {
                start_tag = c.span.clone();
                if !c.empty {
                    let (inl, e) = inlines(r);
                    content = inl;
                    end_tag = e;
                }
            });
            Inline::Group(Group {
                kind: GroupKind::Alternate,
                span: start..end,
                start_tag,
                content,
                end_tag,
            })
        }
        "oMath" | "oMathPara" => {
            let (text, _, end) = props::text_of(r, &tag);
            Inline::Math {
                span: start..end,
                text,
            }
        }
        _ => {
            if let Some(kind) = marker(&tag) {
                let end = end_of(r, &tag);
                return Inline::Marker(Marker {
                    kind,
                    span: start..end,
                });
            }
            let name = tag.name.to_owned();
            Inline::Other {
                name,
                span: start..end_of(r, &tag),
            }
        }
    }
}

/// Reads a run whose start tag was just read.
pub fn run<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> Run {
    let mut run = Run {
        span: tag.span.clone(),
        start_tag: tag.span.clone(),
        rpr: None,
        props: RunProps::default(),
        format_change: None,
        atoms: Vec::new(),
        end_tag: tag.span.end..tag.span.end,
    };
    if tag.empty {
        return run;
    }
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(c) if c.name == "rPr" => {
                let start = c.span.start;
                let read = props::read_rpr(r, &c);
                run.rpr = Some(start..if c.empty { c.span.end } else { r.pos() });
                run.props = read.props;
                run.format_change = read.change;
            }
            Token::Start(c) => atoms(r, c, &mut run.atoms, None),
            Token::End { span, .. } => {
                run.span = tag.span.start..span.end;
                run.end_tag = span;
                return run;
            }
            Token::Text { .. } => {}
        }
    }
    run.span = tag.span.start..r.pos();
    run
}

/// Reads one child of a run into `out`; `outer` is the span of an
/// `mc:AlternateContent` it was read through.
fn atoms<'a>(r: &mut Reader<'a>, tag: Tag<'a>, out: &mut Vec<Atom>, outer: Option<&Span>) {
    let start = tag.span.start;
    let mut push = |kind: AtomKind, end: usize| {
        out.push(Atom {
            kind,
            span: outer.cloned().unwrap_or(start..end),
        });
    };
    match tag.name {
        "t" | "delText" => {
            let preserve = tag.attr_qualified("xml:space").as_deref() == Some("preserve");
            let (text, inner, end) = props::text_of(r, &tag);
            let text = if preserve {
                text
            } else {
                text.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r'))
                    .to_owned()
            };
            push(
                AtomKind::Text {
                    text,
                    inner,
                    preserve,
                    deleted: tag.name == "delText",
                },
                end,
            );
        }
        "tab" | "ptab" => push(AtomKind::Tab, end_of(r, &tag)),
        "br" => {
            let kind = match tag.attr("type").as_deref() {
                Some("page") => BreakKind::Page,
                Some("column") => BreakKind::Column,
                _ => BreakKind::Line,
            };
            push(AtomKind::Break(kind), end_of(r, &tag));
        }
        "cr" => push(AtomKind::Break(BreakKind::Line), end_of(r, &tag)),
        "sym" => {
            let font = tag.attr("font").unwrap_or_default().into_owned();
            let ch = chars::sym(&font, &tag.attr("char").unwrap_or_default());
            push(AtomKind::Symbol { font, ch }, end_of(r, &tag));
        }
        "noBreakHyphen" => push(AtomKind::NoBreakHyphen, end_of(r, &tag)),
        "softHyphen" => push(AtomKind::SoftHyphen, end_of(r, &tag)),
        "footnoteReference" | "endnoteReference" | "commentReference" => {
            let id = tag.attr("id").unwrap_or_default().into_owned();
            let kind = match tag.name {
                "footnoteReference" => AtomKind::FootnoteRef(id),
                "endnoteReference" => AtomKind::EndnoteRef(id),
                _ => AtomKind::CommentRef(id),
            };
            push(kind, end_of(r, &tag));
        }
        "footnoteRef" | "endnoteRef" => push(AtomKind::NoteMark, end_of(r, &tag)),
        "annotationRef" => push(AtomKind::AnnotationMark, end_of(r, &tag)),
        "separator" | "continuationSeparator" => push(AtomKind::Separator, end_of(r, &tag)),
        "fldChar" => {
            let kind = match tag.attr("fldCharType").as_deref() {
                Some("begin") => FieldChar::Begin,
                Some("separate") => FieldChar::Separate,
                _ => FieldChar::End,
            };
            push(AtomKind::FieldChar(kind), end_of(r, &tag));
        }
        "instrText" | "delInstrText" => {
            let (text, _, end) = props::text_of(r, &tag);
            push(AtomKind::Instr(text), end);
        }
        "drawing" => {
            let (d, end) = drawing(r, &tag);
            push(AtomKind::Drawing(d), end);
        }
        "pict" => {
            let (d, end) = vml(r, &tag);
            push(AtomKind::Drawing(d), end);
        }
        "object" => {
            let (d, end) = vml(r, &tag);
            let preview = match d.graphic {
                Graphic::Picture { embed, .. } => embed,
                _ => None,
            };
            push(
                AtomKind::Object {
                    prog_id: (!d.name.is_empty()).then_some(d.name),
                    preview,
                },
                end,
            );
        }
        "lastRenderedPageBreak" => push(AtomKind::RenderedPageBreak, end_of(r, &tag)),
        "AlternateContent" => {
            // The whole element is what an edit removes.
            let mut inner = Vec::new();
            let mut r2_end = 0;
            let outer_start = start;
            let end = alternate(r, &tag, |r, c| {
                if c.empty {
                    return;
                }
                while let Some(t) = r.next_token() {
                    match t {
                        Token::Start(x) => atoms(r, x, &mut inner, None),
                        Token::End { span, .. } => {
                            r2_end = span.end;
                            break;
                        }
                        Token::Text { .. } => {}
                    }
                }
            });
            let _ = r2_end;
            let whole = outer.cloned().unwrap_or(outer_start..end);
            for mut a in inner {
                a.span = whole.clone();
                out.push(a);
            }
        }
        _ => {
            let name = tag.name.to_owned();
            push(AtomKind::Other(name), end_of(r, &tag));
        }
    }
}

fn int_attr(tag: &Tag<'_>, name: &str) -> i64 {
    tag.attr(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(0, |f| f.round() as i64)
}

/// Reads a `w:drawing` whose start tag was just read; its end.
fn drawing<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> (Drawing, usize) {
    let mut d = Drawing::default();
    if tag.empty {
        return (d, tag.span.end);
    }
    let mut depth = 0usize;
    let mut uri = String::new();
    let mut text_box: Option<Vec<Block>> = None;
    let mut embed = None;
    let mut link = None;
    let mut chart = None;
    let mut sized = false;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(c) => {
                match c.name {
                    "inline" => d.floating = false,
                    "anchor" => d.floating = true,
                    "extent" if !sized => {
                        d.size = (int_attr(&c, "cx"), int_attr(&c, "cy"));
                        sized = true;
                    }
                    "docPr" => {
                        d.name = c.attr("name").unwrap_or_default().into_owned();
                        d.descr = c
                            .attr("descr")
                            .or_else(|| c.attr("title"))
                            .unwrap_or_default()
                            .into_owned();
                    }
                    "graphicData" if uri.is_empty() => {
                        uri = c.attr("uri").unwrap_or_default().into_owned();
                    }
                    "blip" if embed.is_none() && link.is_none() => {
                        embed = c.attr("embed").map(|v| v.into_owned());
                        link = c.attr("link").map(|v| v.into_owned());
                    }
                    "chart" if chart.is_none() => chart = c.attr("id").map(|v| v.into_owned()),
                    "txbxContent" if text_box.is_none() => {
                        text_box = Some(if c.empty { Vec::new() } else { parse_blocks(r) });
                        continue;
                    }
                    _ => {}
                }
                if !c.empty {
                    depth += 1;
                }
            }
            Token::End { span, .. } => {
                if depth == 0 {
                    d.graphic = graphic(&uri, text_box, embed, link, chart);
                    return (d, span.end);
                }
                depth -= 1;
            }
            Token::Text { .. } => {}
        }
    }
    d.graphic = graphic(&uri, text_box, embed, link, chart);
    (d, r.pos())
}

fn graphic(
    uri: &str,
    text_box: Option<Vec<Block>>,
    embed: Option<String>,
    link: Option<String>,
    chart: Option<String>,
) -> Graphic {
    let kind = uri.rsplit('/').next().unwrap_or_default();
    match kind {
        "picture" => Graphic::Picture { embed, link },
        "wordprocessingShape" => match text_box {
            Some(b) => Graphic::TextBox(b),
            None => Graphic::Shape,
        },
        "wordprocessingGroup" => Graphic::Group,
        "wordprocessingCanvas" => Graphic::Canvas,
        "chart" => Graphic::Chart(chart),
        "diagram" => Graphic::Diagram,
        "ink" | "wordprocessingInk" => Graphic::Ink,
        _ if chart.is_some() => Graphic::Chart(chart),
        _ if uri.contains("chartex") || uri.contains("chart") => Graphic::Chart(None),
        _ => Graphic::Unknown,
    }
}

/// VML's length (`2in`, `36pt`, `1.5cm`, `100px`) in EMU.
fn vml_length(v: &str) -> Option<i64> {
    let v = v.trim();
    let split = v
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(v.len());
    let n: f64 = v[..split].parse().ok()?;
    let emu = match &v[split..] {
        "in" => n * 914_400.0,
        "pt" | "" => n * 12_700.0,
        "cm" => n * 360_000.0,
        "mm" => n * 36_000.0,
        "pc" => n * 152_400.0,
        "px" => n * 9_525.0,
        "emu" => n,
        _ => return None,
    };
    Some(emu.round() as i64)
}

/// Reads a `w:pict` or a `w:object` (VML) whose start tag was just read;
/// its end. An object's program is the drawing's name.
fn vml<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> (Drawing, usize) {
    let mut d = Drawing {
        vml: true,
        graphic: Graphic::Shape,
        ..Drawing::default()
    };
    if tag.empty {
        return (d, tag.span.end);
    }
    let mut depth = 0usize;
    let mut picture = None;
    let mut text_box = None;
    let mut prog = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(c) => {
                match c.name {
                    "shape" | "rect" | "roundrect" | "oval" if d.name.is_empty() => {
                        d.name = c.attr("id").unwrap_or_default().into_owned();
                        d.descr = c.attr("alt").unwrap_or_default().into_owned();
                        let style = c.attr("style").unwrap_or_default();
                        let mut size = (0, 0);
                        for decl in style.split(';') {
                            if let Some((k, v)) = decl.split_once(':') {
                                match k.trim() {
                                    "width" => size.0 = vml_length(v).unwrap_or(0),
                                    "height" => size.1 = vml_length(v).unwrap_or(0),
                                    "position" => d.floating = v.trim() == "absolute",
                                    _ => {}
                                }
                            }
                        }
                        d.size = size;
                    }
                    "imagedata" if picture.is_none() => {
                        picture = c.attr("id").map(|v| v.into_owned());
                    }
                    "OLEObject" => prog = c.attr("ProgID").map(|v| v.into_owned()),
                    "txbxContent" if text_box.is_none() => {
                        text_box = Some(if c.empty { Vec::new() } else { parse_blocks(r) });
                        continue;
                    }
                    _ => {}
                }
                if !c.empty {
                    depth += 1;
                }
            }
            Token::End { span, .. } => {
                if depth == 0 {
                    finish_vml(&mut d, picture, text_box, prog);
                    return (d, span.end);
                }
                depth -= 1;
            }
            Token::Text { .. } => {}
        }
    }
    finish_vml(&mut d, picture, text_box, prog);
    (d, r.pos())
}

fn finish_vml(
    d: &mut Drawing,
    picture: Option<String>,
    text_box: Option<Vec<Block>>,
    prog: Option<String>,
) {
    if let Some(b) = text_box {
        d.graphic = Graphic::TextBox(b);
    } else if picture.is_some() {
        d.graphic = Graphic::Picture {
            embed: picture,
            link: None,
        };
    }
    if let Some(p) = prog {
        d.name = p;
    }
}

fn table<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> Table {
    let mut t = Table {
        span: tag.span.clone(),
        props: TableProps::default(),
        grid: Vec::new(),
        rows: Vec::new(),
    };
    children(r, tag, |r, c| {
        match c.name {
            "tblPr" => t.props = props::read_tblpr(r, &c),
            "tblGrid" => {
                children(r, &c, |_, g| {
                    if g.name == "gridCol" {
                        t.grid.push(int_attr(&g, "w"));
                    }
                    false
                });
            }
            "tr" => t.rows.push(row(r, &c)),
            // Rows inside a content control or custom XML.
            "sdt" | "customXml" => rows_within(r, &c, &mut t.rows),
            _ => return false,
        }
        true
    });
    t.span = tag.span.start..if tag.empty { tag.span.end } else { r.pos() };
    t
}

fn rows_within<'a>(r: &mut Reader<'a>, tag: &Tag<'a>, out: &mut Vec<Row>) {
    children(r, tag, |r, c| {
        match c.name {
            "tr" => out.push(row(r, &c)),
            "sdtContent" | "sdt" | "customXml" => rows_within(r, &c, out),
            _ => return false,
        }
        true
    });
}

fn row<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> Row {
    let mut w = Row {
        span: tag.span.clone(),
        props: RowProps::default(),
        cells: Vec::new(),
    };
    children(r, tag, |r, c| {
        match c.name {
            "trPr" => w.props = props::read_trpr(r, &c),
            "tc" => w.cells.push(cell(r, &c)),
            "sdt" | "customXml" => cells_within(r, &c, &mut w.cells),
            _ => return false,
        }
        true
    });
    w.span = tag.span.start..if tag.empty { tag.span.end } else { r.pos() };
    w
}

fn cells_within<'a>(r: &mut Reader<'a>, tag: &Tag<'a>, out: &mut Vec<Cell>) {
    children(r, tag, |r, c| {
        match c.name {
            "tc" => out.push(cell(r, &c)),
            "sdtContent" | "sdt" | "customXml" => cells_within(r, &c, out),
            _ => return false,
        }
        true
    });
}

fn cell<'a>(r: &mut Reader<'a>, tag: &Tag<'a>) -> Cell {
    let mut c = Cell {
        span: tag.span.clone(),
        props: CellProps::default(),
        blocks: Vec::new(),
    };
    children(r, tag, |r, k| {
        if k.name == "tcPr" {
            c.props = props::read_tcpr(r, &k);
        } else {
            c.blocks.push(block(r, k));
        }
        true
    });
    c.span = tag.span.start..if tag.empty { tag.span.end } else { r.pos() };
    c
}

/// A note or a comment of a notes or comments part.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// `w:id`.
    pub id: String,
    /// `w:type` (`separator`, `continuationSeparator`, `normal`).
    pub kind: Option<String>,
    /// The whole element.
    pub span: Span,
    /// A comment's author.
    pub author: Option<String>,
    /// A comment's date.
    pub date: Option<String>,
    /// A comment's initials.
    pub initials: Option<String>,
    /// Its blocks.
    pub blocks: Vec<Block>,
}

/// A story part read: the blocks of the body, a header or a footer, or
/// the notes or comments of a notes or comments part.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PartTree {
    /// The root element's name (`document`, `hdr`, `ftr`, `footnotes`,
    /// `endnotes`, `comments`).
    pub root: String,
    /// The blocks of a body, header or footer.
    pub blocks: Vec<Block>,
    /// The items of a notes or comments part.
    pub items: Vec<Item>,
    /// The span of `w:body` in a main document part.
    pub body: Option<Span>,
}

impl PartTree {
    /// Reads a story part.
    pub fn parse(text: &str) -> PartTree {
        let mut out = PartTree::default();
        let mut r = Reader::new(text);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            out.root = tag.name.to_owned();
            match tag.name {
                "document" | "glossaryDocument" => {
                    children(&mut r, &tag, |r, c| {
                        if c.name == "body" {
                            if !c.empty {
                                out.blocks = parse_blocks(r);
                            }
                            out.body =
                                Some(c.span.start..if c.empty { c.span.end } else { r.pos() });
                            return true;
                        }
                        false
                    });
                }
                "hdr" | "ftr" => {
                    if !tag.empty {
                        out.blocks = parse_blocks(&mut r);
                    }
                }
                "footnotes" | "endnotes" | "comments" => {
                    children(&mut r, &tag, |r, c| {
                        if matches!(c.name, "footnote" | "endnote" | "comment") {
                            let a = |n: &str| c.attr(n).map(|v| v.into_owned());
                            let blocks = if c.empty { Vec::new() } else { parse_blocks(r) };
                            out.items.push(Item {
                                id: a("id").unwrap_or_default(),
                                kind: a("type"),
                                span: c.span.start..if c.empty { c.span.end } else { r.pos() },
                                author: a("author"),
                                date: a("date"),
                                initials: a("initials"),
                                blocks,
                            });
                            return true;
                        }
                        false
                    });
                }
                _ => {}
            }
            break;
        }
        out
    }
}

/// The paragraphs of `blocks` in document order, those inside tables,
/// content controls and wrappers included; text boxes left out.
pub fn paragraphs(blocks: &[Block]) -> Vec<&Paragraph> {
    fn walk<'a>(blocks: &'a [Block], out: &mut Vec<&'a Paragraph>) {
        for b in blocks {
            match b {
                Block::Paragraph(p) => out.push(p),
                Block::Table(t) => {
                    for row in &t.rows {
                        for c in &row.cells {
                            walk(&c.blocks, out);
                        }
                    }
                }
                Block::Control { blocks, .. } | Block::Wrapper { blocks, .. } => walk(blocks, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

/// The text of a `w:t` as written: escaped, with `xml:space` as its
/// content needs.
pub(crate) fn needs_preserve(text: &str) -> bool {
    text.starts_with(|c: char| c.is_whitespace()) || text.ends_with(|c: char| c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(xml: &str) -> Vec<Block> {
        let doc = format!(r#"<w:document xmlns:w="w"><w:body>{xml}</w:body></w:document>"#);
        PartTree::parse(&doc).blocks
    }

    fn para(b: &Block) -> &Paragraph {
        match b {
            Block::Paragraph(p) => p,
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    #[test]
    fn paragraphs_runs_and_spans() {
        let xml = r#"<w:p w14:paraId="1"><w:pPr><w:pStyle w:val="Heading1"/><w:rPr><w:b/></w:rPr></w:pPr><w:r><w:rPr><w:i/></w:rPr><w:t xml:space="preserve"> a &amp; b </w:t><w:tab/><w:t>  c  </w:t></w:r><w:r/></w:p><w:p/>"#;
        let doc = format!(r#"<w:document xmlns:w="w"><w:body>{xml}</w:body></w:document>"#);
        let blocks = PartTree::parse(&doc).blocks;
        assert_eq!(blocks.len(), 2);
        let p = para(&blocks[0]);
        assert_eq!(p.props.style.as_deref(), Some("Heading1"));
        assert!(p.mark.toggle(crate::props::Toggle::Bold).unwrap());
        assert_eq!(&doc[p.span.clone()], xml.strip_suffix("<w:p/>").unwrap());
        assert_eq!(&doc[p.start_tag.clone()], r#"<w:p w14:paraId="1">"#);
        assert_eq!(&doc[p.content_end..p.span.end], "</w:p>");
        let Inline::Run(run) = &p.content[0] else {
            panic!()
        };
        assert_eq!(run.atoms.len(), 3);
        let AtomKind::Text {
            text,
            inner,
            preserve,
            ..
        } = &run.atoms[0].kind
        else {
            panic!()
        };
        assert_eq!((text.as_str(), *preserve), (" a & b ", true));
        assert_eq!(&doc[inner.clone()], " a &amp; b ");
        // Without `xml:space="preserve"` the spaces around are dropped.
        let AtomKind::Text { text, .. } = &run.atoms[2].kind else {
            panic!()
        };
        assert_eq!(text, "c");
        assert!(matches!(run.atoms[1].kind, AtomKind::Tab));
        let Inline::Run(empty) = &p.content[1] else {
            panic!()
        };
        assert!(empty.atoms.is_empty());
        let q = para(&blocks[1]);
        assert!(q.empty && q.content.is_empty());
    }

    #[test]
    fn groups_markers_and_fields() {
        let b = body(
            r#"<w:p><w:bookmarkStart w:id="0" w:name="x"/><w:hyperlink r:id="rId5"><w:r><w:t>link</w:t></w:r></w:hyperlink><w:ins w:id="1" w:author="A"><w:r><w:t>new</w:t></w:r></w:ins><w:del w:id="2" w:author="A"><w:r><w:delText>old</w:delText></w:r></w:del><w:fldSimple w:instr=" PAGE "><w:r><w:t>3</w:t></w:r></w:fldSimple><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> REF x </w:instrText></w:r><w:bookmarkEnd w:id="0"/><m:oMath><m:r><m:t>x+1</m:t></m:r></m:oMath><w:proofErr w:type="spellStart"/><w:unknownThing a="1"><w:r/></w:unknownThing></w:p>"#,
        );
        let p = para(&b[0]);
        let kinds: Vec<String> = p
            .content
            .iter()
            .map(|i| match i {
                Inline::Run(_) => "run".into(),
                Inline::Group(g) => format!("{:?}", std::mem::discriminant(&g.kind))
                    .len()
                    .to_string(),
                Inline::Marker(m) => match &m.kind {
                    MarkerKind::BookmarkStart { name, .. } => format!("start {name}"),
                    MarkerKind::BookmarkEnd { .. } => "end".into(),
                    MarkerKind::Other(n) => n.clone(),
                    _ => "marker".into(),
                },
                Inline::Math { text, .. } => format!("math {text}"),
                Inline::Other { name, .. } => format!("other {name}"),
            })
            .collect();
        assert_eq!(kinds[0], "start x");
        assert_eq!(kinds[7], "end");
        assert_eq!(kinds[8], "math x+1");
        assert_eq!(kinds[9], "proofErr");
        assert_eq!(kinds[10], "other unknownThing");
        let Inline::Group(link) = &p.content[1] else {
            panic!()
        };
        assert_eq!(
            link.kind,
            GroupKind::Hyperlink {
                rid: Some("rId5".into()),
                anchor: None
            }
        );
        let Inline::Group(del) = &p.content[3] else {
            panic!()
        };
        let Inline::Run(r) = &del.content[0] else {
            panic!()
        };
        assert!(
            matches!(&r.atoms[0].kind, AtomKind::Text { deleted: true, text, .. } if text == "old")
        );
        let Inline::Group(f) = &p.content[4] else {
            panic!()
        };
        assert_eq!(
            f.kind,
            GroupKind::SimpleField {
                instr: " PAGE ".into()
            }
        );
        let Inline::Run(i) = &p.content[6] else {
            panic!()
        };
        assert_eq!(i.atoms[0].kind, AtomKind::Instr(" REF x ".into()));
    }

    #[test]
    fn tables_with_merges_and_nesting() {
        let b = body(
            r#"<w:tbl><w:tblPr><w:tblStyle w:val="Grid"/></w:tblPr><w:tblGrid><w:gridCol w:w="100"/><w:gridCol w:w="200"/></w:tblGrid><w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:tbl><w:tr><w:tc><w:p><w:r><w:t>inner</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>"#,
        );
        let Block::Table(t) = &b[0] else { panic!() };
        assert_eq!(t.grid, [100, 200]);
        assert_eq!(t.props.style.as_deref(), Some("Grid"));
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].props.header, Some(true));
        assert_eq!(t.rows[0].cells[0].props.grid_span, Some(2));
        assert_eq!(
            t.rows[1].cells[0].props.v_merge,
            Some(crate::props::VMerge::Restart)
        );
        let texts: Vec<usize> = paragraphs(&b).iter().map(|p| p.content.len()).collect();
        assert_eq!(texts, [1, 1, 0, 1, 0]);
    }

    #[test]
    fn drawings_and_alternate_content() {
        let b = body(
            r#"<w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor><wp:extent cx="100" cy="50"/><wp:docPr id="2" name="Text Box 2"/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r><w:r><w:drawing><wp:inline><wp:extent cx="304800" cy="304800"/><wp:docPr id="1" name="Picture 1" descr="A red square"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:blipFill><a:blip r:embed="rId20"/></pic:blipFill><pic:spPr><a:xfrm><a:ext cx="1" cy="1"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r><w:r><mc:AlternateContent><mc:Choice Requires="cx1"><w:drawing/></mc:Choice><mc:Fallback><w:t>fallback</w:t></mc:Fallback></mc:AlternateContent></w:r></w:p>"#,
        );
        let p = para(&b[0]);
        let Inline::Run(r0) = &p.content[0] else {
            panic!()
        };
        assert_eq!(r0.atoms.len(), 1);
        let AtomKind::Drawing(d) = &r0.atoms[0].kind else {
            panic!()
        };
        assert!(d.floating && !d.vml);
        assert_eq!(d.size, (100, 50));
        let Graphic::TextBox(blocks) = &d.graphic else {
            panic!()
        };
        assert_eq!(paragraphs(blocks).len(), 1);
        let Inline::Run(r1) = &p.content[1] else {
            panic!()
        };
        let AtomKind::Drawing(pic) = &r1.atoms[0].kind else {
            panic!()
        };
        assert_eq!(pic.descr, "A red square");
        assert_eq!(pic.size, (304800, 304800));
        assert_eq!(
            pic.graphic,
            Graphic::Picture {
                embed: Some("rId20".into()),
                link: None
            }
        );
        // A choice not understood: the fallback is read.
        let Inline::Run(r2) = &p.content[2] else {
            panic!()
        };
        assert!(matches!(&r2.atoms[0].kind, AtomKind::Text { text, .. } if text == "fallback"));
    }

    #[test]
    fn notes_and_comments_parts() {
        let t = PartTree::parse(
            r#"<w:footnotes><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>note</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        );
        assert_eq!(t.root, "footnotes");
        assert_eq!(t.items.len(), 2);
        assert_eq!(t.items[0].kind.as_deref(), Some("separator"));
        assert_eq!(t.items[1].id, "1");
        let c = PartTree::parse(
            r#"<w:comments><w:comment w:id="0" w:author="Ayşe" w:initials="AY"><w:p><w:r><w:t>hi</w:t></w:r></w:p></w:comment></w:comments>"#,
        );
        assert_eq!(c.items[0].author.as_deref(), Some("Ayşe"));
    }

    #[test]
    fn vml_lengths() {
        assert_eq!(vml_length("2in"), Some(1_828_800));
        assert_eq!(vml_length("36pt"), Some(457_200));
        assert_eq!(vml_length("1cm"), Some(360_000));
        assert_eq!(vml_length("x"), None);
    }
}
