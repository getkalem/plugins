//! A Word document opened as itself: the package's parts found through
//! their relationships, the stories read and shown, and edits written
//! into the paragraphs they touch.

use std::collections::HashMap;
use std::fmt;

use kalem_ooxml::package::{Package, PackageError};
use kalem_ooxml::rels::{self, Rel};
use kalem_ooxml::theme::{self, Theme};
use kalem_ooxml::xml::{self, Reader, Token};

use crate::comments;
use crate::edit::{self, Splice};
use crate::flow::{self, Env, Layout, ParaAt, StoryId, VBlock, VComment, VNote, VPara, Walker};
use crate::numbering::Numbering;
use crate::props::{Span, children};
use crate::story::PartTree;
use crate::styles::Styles;

/// An error opening, editing or saving a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The ZIP container is broken.
    Package(PackageError),
    /// The package is not a WordprocessingML document.
    NotADocument(String),
    /// A part is not text Kalem reads.
    BadPart(String),
    /// An edit the document refuses, with its reason.
    Refused(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(e) => e.fmt(f),
            Self::NotADocument(m) => write!(f, "not a Word document: {m}"),
            Self::BadPart(m) => f.write_str(m),
            Self::Refused(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<PackageError> for Error {
    fn from(e: PackageError) -> Self {
        Self::Package(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// What the main part says the file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A document (`.docx`).
    Document,
    /// A document with macros (`.docm`).
    MacroDocument,
    /// A template (`.dotx`).
    Template,
    /// A template with macros (`.dotm`).
    MacroTemplate,
}

impl Kind {
    fn from_content_type(ct: &str) -> Option<Kind> {
        let ct = ct.to_ascii_lowercase();
        Some(match ct.as_str() {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml" => {
                Kind::Document
            }
            "application/vnd.ms-word.document.macroenabled.main+xml" => Kind::MacroDocument,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml" => {
                Kind::Template
            }
            "application/vnd.ms-word.template.macroenabledtemplate.main+xml" => Kind::MacroTemplate,
            _ => return None,
        })
    }

    /// The file extension of this kind.
    pub fn extension(self) -> &'static str {
        match self {
            Kind::Document => "docx",
            Kind::MacroDocument => "docm",
            Kind::Template => "dotx",
            Kind::MacroTemplate => "dotm",
        }
    }
}

/// The document's properties (`docProps/core.xml`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Properties {
    /// `dc:title`.
    pub title: Option<String>,
    /// `dc:subject`.
    pub subject: Option<String>,
    /// `dc:creator`.
    pub creator: Option<String>,
    /// `cp:keywords`.
    pub keywords: Option<String>,
    /// `dc:description`.
    pub description: Option<String>,
    /// `cp:lastModifiedBy`.
    pub last_modified_by: Option<String>,
    /// `dcterms:created`.
    pub created: Option<String>,
    /// `dcterms:modified`.
    pub modified: Option<String>,
    /// `Application` of `docProps/app.xml`.
    pub application: Option<String>,
    /// `Pages` of `docProps/app.xml`, as Word last counted them.
    pub pages: Option<String>,
}

/// A section's header and footer references and its break.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Section {
    /// `w:type` (`nextPage` when absent).
    pub kind: String,
    /// Header parts by type (`default`, `first`, `even`).
    pub headers: Vec<(String, String)>,
    /// Footer parts by type.
    pub footers: Vec<(String, String)>,
    /// A different first page (`w:titlePg`).
    pub title_page: bool,
}

/// The whole document as shown.
#[derive(Debug, Clone, PartialEq)]
pub struct DocView {
    /// The first section's header, shown once at the top.
    pub header: Vec<VBlock>,
    /// The body.
    pub body: Vec<VBlock>,
    /// The first section's footer, shown once at the bottom.
    pub footer: Vec<VBlock>,
    /// The footnotes, in the order they are referred to.
    pub footnotes: Vec<VNote>,
    /// The endnotes, in the order they are referred to.
    pub endnotes: Vec<VNote>,
    /// The comments.
    pub comments: Vec<VComment>,
}

fn find_para<'a>(blocks: &'a [VBlock], at: &ParaAt) -> Option<&'a VPara> {
    for b in blocks {
        match b {
            VBlock::Para(p) if p.at.as_ref() == Some(at) => return Some(p),
            VBlock::Table(t) => {
                for row in &t.rows {
                    for c in &row.cells {
                        if let Some(p) = find_para(&c.blocks, at) {
                            return Some(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn collect_paras<'a>(blocks: &'a [VBlock], out: &mut Vec<&'a VPara>) {
    for b in blocks {
        match b {
            VBlock::Para(p) => out.push(p),
            VBlock::Table(t) => {
                for row in &t.rows {
                    for c in &row.cells {
                        collect_paras(&c.blocks, out);
                    }
                }
            }
            VBlock::Frame(f) => collect_paras(f, out),
            VBlock::Placeholder(_) => {}
        }
    }
}

impl DocView {
    /// The text as shown: the header, the body, the footer, then the
    /// notes, each with its mark.
    pub fn text(&self) -> String {
        let mut out = String::new();
        flow::text(&self.header, &mut out);
        flow::text(&self.body, &mut out);
        flow::text(&self.footer, &mut out);
        for (notes, open) in [(&self.footnotes, "["), (&self.endnotes, "[")] {
            for n in notes {
                let mut s = String::new();
                flow::text(&n.blocks, &mut s);
                // The note's own mark leads its text: written once.
                let s = s.trim();
                let s = s.strip_prefix(n.mark.as_str()).unwrap_or(s).trim();
                out.push_str(&format!("{open}{}] {s}\n", n.mark));
            }
        }
        out
    }

    /// The headings: each one's text and level (from 1).
    pub fn outline(&self) -> Vec<(String, u8)> {
        let mut paras = Vec::new();
        collect_paras(&self.body, &mut paras);
        paras
            .into_iter()
            .filter_map(|p| {
                let level = p.outline?;
                let t = flow::para_text(p).trim().to_owned();
                (!t.is_empty()).then_some((t, level + 1))
            })
            .collect()
    }

    /// A paragraph of the view by its place.
    pub fn paragraph(&self, at: &ParaAt) -> Option<&VPara> {
        let blocks = match &at.story {
            StoryId::Body => &self.body,
            StoryId::Header(_) => &self.header,
            StoryId::Footer(_) => &self.footer,
            StoryId::Footnote(id) => &self.footnotes.iter().find(|n| &n.id == id)?.blocks,
            StoryId::Endnote(id) => &self.endnotes.iter().find(|n| &n.id == id)?.blocks,
            StoryId::Comment(id) => &self.comments.iter().find(|c| &c.id == id)?.blocks,
        };
        find_para(blocks, at)
    }

    /// Every paragraph of the body, in order (tables' and text boxes'
    /// included).
    pub fn body_paragraphs(&self) -> Vec<&VPara> {
        let mut out = Vec::new();
        collect_paras(&self.body, &mut out);
        out
    }
}

/// One step of the history: the splices it made, each with the text it
/// replaced.
#[derive(Debug, Clone)]
struct Step {
    splices: Vec<(Splice, String)>,
}

/// A Word document.
#[derive(Debug)]
pub struct Document {
    package: Package,
    kind: Kind,
    main: String,
    /// The text of the parts read, as edited.
    texts: HashMap<String, String>,
    /// Their text as read from the file.
    originals: HashMap<String, String>,
    rels: HashMap<String, Vec<Rel>>,
    styles: Styles,
    numbering: Numbering,
    theme: Theme,
    mapping: HashMap<String, String>,
    footnote_format: (String, i64),
    endnote_format: (String, i64),
    track_revisions: bool,
    protection: Option<String>,
    history: Vec<Step>,
    at: usize,
    saved_at: usize,
    batch: Option<Step>,
    author: String,
    date: Option<String>,
}

/// Now as ISO 8601 in UTC, to the second (`2026-10-09T12:00:00Z`), as
/// Word writes a change's date.
pub fn now_iso() -> String {
    let secs = crate::time::SystemTime::now()
        .duration_since(crate::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    iso(secs)
}

/// Seconds since 1970 as ISO 8601 in UTC.
pub fn iso(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn part_text(package: &Package, name: &str) -> Result<String> {
    let bytes = package.part(name)?;
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(Error::BadPart(format!(
            "{name} is written in UTF-16, which Kalem does not read yet"
        )));
    }
    String::from_utf8(bytes).map_err(|_| Error::BadPart(format!("{name} is not UTF-8 text")))
}

impl Document {
    /// Opens a document from the bytes of its package.
    pub fn open(bytes: Vec<u8>) -> Result<Document> {
        let package = Package::read(bytes)?;
        let root = part_text(&package, "_rels/.rels")
            .map_err(|_| Error::NotADocument("no package relationships".into()))?;
        let main = rels::parse(&root)
            .into_iter()
            .find(|r| r.kind == "officeDocument" && !r.external)
            .map(|r| rels::resolve("", &r.target))
            .ok_or_else(|| Error::NotADocument("no main document part".into()))?;
        let types = part_text(&package, "[Content_Types].xml")
            .map_err(|_| Error::NotADocument("no content types".into()))?;
        let ct = rels::content_type(&types, &main).unwrap_or_default();
        let kind = Kind::from_content_type(&ct)
            .ok_or_else(|| Error::NotADocument(format!("its main part is {ct}")))?;
        if !package.contains(&main) {
            return Err(Error::NotADocument(format!("{main} is missing")));
        }
        let mut doc = Document {
            package,
            kind,
            main: main.clone(),
            texts: HashMap::new(),
            originals: HashMap::new(),
            rels: HashMap::new(),
            styles: Styles::default(),
            numbering: Numbering::default(),
            theme: Theme::default(),
            mapping: HashMap::new(),
            footnote_format: ("decimal".into(), 1),
            endnote_format: ("lowerRoman".into(), 1),
            track_revisions: false,
            protection: None,
            history: Vec::new(),
            at: 0,
            saved_at: 0,
            batch: None,
            author: "Kalem".into(),
            date: None,
        };
        doc.load(&main)?;
        for rel in doc.part_rels(&main) {
            if rel.external {
                continue;
            }
            let target = rels::resolve(&main, &rel.target);
            if matches!(
                rel.kind.as_str(),
                "styles"
                    | "numbering"
                    | "settings"
                    | "theme"
                    | "footnotes"
                    | "endnotes"
                    | "comments"
                    | "commentsExtended"
                    | "commentsIds"
                    | "commentsExtensible"
                    | "header"
                    | "footer"
            ) && doc.package.contains(&target)
            {
                doc.load(&target)?;
                let _ = doc.part_rels(&target);
            }
        }
        doc.read_settings();
        Ok(doc)
    }

    fn load(&mut self, name: &str) -> Result<()> {
        if !self.texts.contains_key(name) {
            let t = part_text(&self.package, name)?;
            self.originals.insert(name.to_owned(), t.clone());
            self.texts.insert(name.to_owned(), t);
        }
        Ok(())
    }

    /// The relationships of a part, read once.
    fn part_rels(&mut self, part: &str) -> Vec<Rel> {
        if let Some(r) = self.rels.get(part) {
            return r.clone();
        }
        let path = rels::rels_path(part);
        let list = part_text(&self.package, &path)
            .map(|t| rels::parse(&t))
            .unwrap_or_default();
        self.rels.insert(part.to_owned(), list.clone());
        list
    }

    /// The part a relationship of the main part names, by its type.
    fn related(&self, kind: &str) -> Option<String> {
        self.rels
            .get(&self.main)?
            .iter()
            .find(|r| r.kind == kind && !r.external)
            .map(|r| rels::resolve(&self.main, &r.target))
    }

    fn read_settings(&mut self) {
        let text =
            |doc: &Document, kind: &str| doc.related(kind).and_then(|p| doc.texts.get(&p).cloned());
        self.styles = Styles::parse(&text(self, "styles").unwrap_or_default());
        self.numbering = Numbering::parse(&text(self, "numbering").unwrap_or_default());
        self.theme = theme::parse(&text(self, "theme").unwrap_or_default());
        let settings = text(self, "settings").unwrap_or_default();
        let mut r = Reader::new(&settings);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "clrSchemeMapping" => {
                    for (k, v) in tag.attrs() {
                        self.mapping.insert(k.to_owned(), v.into_owned());
                    }
                }
                "trackRevisions" => self.track_revisions = crate::props::on_off(&tag),
                "documentProtection" => {
                    let enforced = tag
                        .attr("enforcement")
                        .is_some_and(|v| matches!(v.as_ref(), "1" | "true" | "on"));
                    if enforced {
                        self.protection = Some(tag.attr("edit").unwrap_or_default().into_owned());
                    }
                }
                "footnotePr" | "endnotePr" => {
                    let foot = tag.name == "footnotePr";
                    let mut fmt = None;
                    let mut start = None;
                    children(&mut r, &tag, |_, c| {
                        match c.name {
                            "numFmt" => fmt = c.attr("val").map(|v| v.into_owned()),
                            "numStart" => start = c.attr("val").and_then(|v| v.parse().ok()),
                            _ => {}
                        }
                        false
                    });
                    let target = if foot {
                        &mut self.footnote_format
                    } else {
                        &mut self.endnote_format
                    };
                    if let Some(f) = fmt {
                        target.0 = f;
                    }
                    if let Some(s) = start {
                        target.1 = s;
                    }
                }
                _ => {}
            }
        }
    }

    /// What the file is.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The main document part's name (`word/document.xml`).
    pub fn main_part(&self) -> &str {
        &self.main
    }

    /// The names of the package's parts.
    pub fn parts(&self) -> Vec<String> {
        self.package.names()
    }

    /// A part's bytes (a picture's), as the package holds it.
    pub fn part_bytes(&self, name: &str) -> Result<Vec<u8>> {
        Ok(self.package.part(name)?)
    }

    /// The styles.
    pub fn styles(&self) -> &Styles {
        &self.styles
    }

    /// The lists.
    pub fn numbering(&self) -> &Numbering {
        &self.numbering
    }

    /// The theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Whether the document asks Word to track changes.
    pub fn tracks_changes(&self) -> bool {
        self.track_revisions
    }

    /// The protection enforced (`readOnly`, `comments`,
    /// `trackedChanges`, `forms`), if any.
    pub fn protection(&self) -> Option<&str> {
        self.protection.as_deref()
    }

    /// Whether it holds a VBA project.
    pub fn has_vba(&self) -> bool {
        self.related("vbaProject").is_some()
    }

    /// The document's properties.
    pub fn properties(&self) -> Properties {
        let mut p = Properties::default();
        let core = part_text(&self.package, "docProps/core.xml").unwrap_or_default();
        let mut r = Reader::new(&core);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            if tag.empty {
                continue;
            }
            let slot = match tag.name {
                "title" => &mut p.title,
                "subject" => &mut p.subject,
                "creator" => &mut p.creator,
                "keywords" => &mut p.keywords,
                "description" => &mut p.description,
                "lastModifiedBy" => &mut p.last_modified_by,
                "created" => &mut p.created,
                "modified" => &mut p.modified,
                _ => continue,
            };
            let (text, _, _) = crate::props::text_of(&mut r, &tag);
            if !text.trim().is_empty() {
                *slot = Some(text.trim().to_owned());
            }
        }
        let app = part_text(&self.package, "docProps/app.xml").unwrap_or_default();
        let mut r = Reader::new(&app);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            let slot = match tag.name {
                "Application" => &mut p.application,
                "Pages" => &mut p.pages,
                _ => continue,
            };
            let (text, _, _) = crate::props::text_of(&mut r, &tag);
            *slot = Some(text.trim().to_owned());
        }
        p
    }

    fn tree(&self, part: &str) -> PartTree {
        PartTree::parse(self.texts.get(part).map_or("", String::as_str))
    }

    /// The sections, in order, from the body's section properties.
    pub fn sections(&self) -> Vec<Section> {
        let text = self.texts.get(&self.main).map_or("", String::as_str);
        // Every `w:sectPr` of the body, in order, read without building
        // the tree: one ends each paragraph that ends a section, the last
        // one the body. A `w:sectPrChange`'s is inside one and skipped.
        let mut spans = Vec::new();
        let mut r = Reader::new(text);
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "sectPr"
            {
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                spans.push(tag.span.start..end);
            }
        }
        let rels = self.rels.get(&self.main).cloned().unwrap_or_default();
        let target = |id: &str| {
            rels.iter()
                .find(|r| r.id == id)
                .map(|r| rels::resolve(&self.main, &r.target))
        };
        spans
            .into_iter()
            .map(|span| {
                let mut s = Section {
                    kind: "nextPage".into(),
                    ..Section::default()
                };
                let mut r = Reader::new(&text[span]);
                while let Some(t) = r.next_token() {
                    let Token::Start(tag) = t else { continue };
                    let ty = || {
                        tag.attr("type")
                            .unwrap_or_else(|| "default".into())
                            .into_owned()
                    };
                    match tag.name {
                        "headerReference" => {
                            if let Some(p) = tag.attr("id").and_then(|id| target(&id)) {
                                s.headers.push((ty(), p));
                            }
                        }
                        "footerReference" => {
                            if let Some(p) = tag.attr("id").and_then(|id| target(&id)) {
                                s.footers.push((ty(), p));
                            }
                        }
                        "type" => s.kind = tag.attr("val").unwrap_or_default().into_owned(),
                        "titlePg" => s.title_page = crate::props::on_off(&tag),
                        _ => {}
                    }
                }
                s
            })
            .collect()
    }

    fn env<'a>(&'a self, part: &'a str) -> Env<'a> {
        Env {
            styles: &self.styles,
            numbering: &self.numbering,
            theme: &self.theme,
            mapping: &self.mapping,
            rels: self.rels.get(part).map_or(&[], Vec::as_slice),
            part,
        }
    }

    /// A story's blocks as shown, walked on its own.
    pub fn story_view(&self, story: &StoryId) -> Vec<VBlock> {
        let part = match story {
            StoryId::Body => self.main.clone(),
            StoryId::Header(p) | StoryId::Footer(p) => p.clone(),
            StoryId::Footnote(_) => self.related("footnotes").unwrap_or_default(),
            StoryId::Endnote(_) => self.related("endnotes").unwrap_or_default(),
            StoryId::Comment(_) => self.related("comments").unwrap_or_default(),
        };
        let mut w = Walker::new(self.env(&part), story.clone());
        w.note_formats(self.footnote_format.clone(), self.endnote_format.clone());
        if *story == StoryId::Body {
            return w.body(self.texts.get(&part).map_or("", String::as_str));
        }
        let tree = self.tree(&part);
        match story {
            StoryId::Body | StoryId::Header(_) | StoryId::Footer(_) => w.blocks(&tree.blocks),
            StoryId::Footnote(id) | StoryId::Endnote(id) | StoryId::Comment(id) => tree
                .items
                .iter()
                .find(|i| &i.id == id)
                .map(|i| w.blocks(&i.blocks))
                .unwrap_or_default(),
        }
    }

    /// The whole document as shown.
    pub fn view(&self) -> DocView {
        self.view_with(false)
    }

    /// The whole document as shown; with `layouts`, each paragraph of the
    /// body with its edit coordinates too (as the other stories always
    /// have them), for Kalem's flow.
    pub fn view_with(&self, layouts: bool) -> DocView {
        let main = self.main.clone();
        let mut w = Walker::new(self.env(&main), StoryId::Body);
        w.layouts = layouts;
        w.note_formats(self.footnote_format.clone(), self.endnote_format.clone());
        let body = w.body(self.texts.get(&main).map_or("", String::as_str));
        let sections = self.sections();
        let first = |pick: fn(&Section) -> &Vec<(String, String)>| {
            sections.iter().find_map(|s| {
                pick(s)
                    .iter()
                    .find(|(t, _)| t == "default")
                    .map(|(_, p)| p.clone())
            })
        };
        let header = first(|s| &s.headers)
            .map(|p| self.story_view(&StoryId::Header(p)))
            .unwrap_or_default();
        let footer = first(|s| &s.footers)
            .map(|p| self.story_view(&StoryId::Footer(p)))
            .unwrap_or_default();
        let notes = |kind: &str, refs: &[(String, String)], foot: bool| -> Vec<VNote> {
            let Some(part) = self.related(kind) else {
                return Vec::new();
            };
            let tree = self.tree(&part);
            refs.iter()
                .filter_map(|(id, mark)| {
                    let item = tree.items.iter().find(|i| &i.id == id)?;
                    let story = if foot {
                        StoryId::Footnote(id.clone())
                    } else {
                        StoryId::Endnote(id.clone())
                    };
                    let mut nw = Walker::new(self.env(&part), story);
                    nw.set_note_mark(Some(mark.clone()));
                    Some(VNote {
                        id: id.clone(),
                        mark: mark.clone(),
                        blocks: nw.blocks(&item.blocks),
                    })
                })
                .collect()
        };
        let footnotes = notes("footnotes", &w.footnotes, true);
        let endnotes = notes("endnotes", &w.endnotes, false);
        let threads = self.comment_threads();
        let comments = self
            .related("comments")
            .map(|part| {
                let tree = self.tree(&part);
                tree.items
                    .iter()
                    .map(|i| {
                        let mut cw = Walker::new(self.env(&part), StoryId::Comment(i.id.clone()));
                        let (parent, done) = threads.get(&i.id).cloned().unwrap_or_default();
                        VComment {
                            id: i.id.clone(),
                            author: i.author.clone().unwrap_or_default(),
                            date: i.date.clone(),
                            parent,
                            done,
                            blocks: cw.blocks(&i.blocks),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        DocView {
            header,
            body,
            footer,
            footnotes,
            endnotes,
            comments,
        }
    }

    /// A paragraph's layout, for edits.
    pub fn layout(&self, at: &ParaAt) -> Result<Layout> {
        let blocks = self.story_view(&at.story);
        find_para(&blocks, at)
            .map(|p| p.layout.clone())
            .ok_or_else(|| {
                Error::Refused(format!("there is no paragraph {} in that story", at.index))
            })
    }

    fn check_editable(&self) -> Result<()> {
        if let Some(p) = &self.protection
            && p != "none"
        {
            return Err(Error::Refused(format!(
                "The document is protected ({p}); Kalem does not take protection away yet"
            )));
        }
        Ok(())
    }

    /// Who the tracked changes made from now on are by, and their date
    /// (`None`: the time each is made).
    pub fn set_revision_author(&mut self, author: &str, date: Option<&str>) {
        self.author = author.to_owned();
        self.date = date.map(str::to_owned);
    }

    /// The author and date of a tracked change made now, and an ID no
    /// change, comment or bookmark of the document has.
    fn track(&self) -> edit::Track {
        let next_id = self
            .texts
            .values()
            .map(|t| edit::max_id(t))
            .max()
            .unwrap_or(0)
            + 1;
        edit::Track {
            author: self.author.clone(),
            date: self.date.clone().unwrap_or_else(now_iso),
            next_id,
        }
    }

    /// Turns Track Changes on or off (`w:trackRevisions` of the
    /// settings): while it is on, edits are written as tracked changes.
    pub fn set_track_changes(&mut self, on: bool) -> Result<()> {
        let part = self
            .related("settings")
            .filter(|p| self.texts.contains_key(p))
            .ok_or_else(|| Error::Refused("The document has no settings part".into()))?;
        let text = &self.texts[&part];
        let new = edit::set_track_revisions(text, on);
        if new != *text {
            let len = text.len();
            self.apply(Splice {
                part,
                range: 0..len,
                text: new,
            })?;
        }
        self.track_revisions = on;
        Ok(())
    }

    fn apply(&mut self, s: Splice) -> Result<()> {
        let text = self
            .texts
            .get_mut(&s.part)
            .ok_or_else(|| Error::Refused(format!("{} is not read", s.part)))?;
        let old = text[s.range.clone()].to_owned();
        text.replace_range(s.range.clone(), &s.text);
        let step = (s, old);
        match &mut self.batch {
            Some(b) => b.splices.push(step),
            None => {
                self.history.truncate(self.at);
                if self.saved_at > self.at {
                    // The saved state was undone past and is gone now.
                    self.saved_at = usize::MAX;
                }
                self.history.push(Step {
                    splices: vec![step],
                });
                self.at += 1;
            }
        }
        Ok(())
    }

    /// Begins a batch: the edits until [`Document::end_batch`] undo as one
    /// step.
    pub fn begin_batch(&mut self) {
        if self.batch.is_none() {
            self.batch = Some(Step {
                splices: Vec::new(),
            });
        }
    }

    /// Ends a batch; whether it changed anything.
    pub fn end_batch(&mut self) -> bool {
        let Some(b) = self.batch.take() else {
            return false;
        };
        if b.splices.is_empty() {
            return false;
        }
        self.history.truncate(self.at);
        if self.saved_at > self.at {
            self.saved_at = usize::MAX;
        }
        self.history.push(b);
        self.at += 1;
        true
    }

    /// Replaces `range` of a paragraph's edit text with `text`, as typing
    /// does: the text takes the formatting of the run before it.
    pub fn replace(
        &mut self,
        at: &ParaAt,
        range: std::ops::Range<usize>,
        text: &str,
    ) -> Result<()> {
        self.check_editable()?;
        let l = self.layout(at)?;
        let s = if self.track_revisions {
            let mut track = self.track();
            edit::replace_tracked(&l, &self.texts[&l.part], range, text, &mut track)
        } else {
            edit::replace(&l, &self.texts[&l.part], range, text)
        }
        .map_err(Error::Refused)?;
        self.apply(s)
    }

    /// Splits a paragraph at `offset` of its edit text (Enter). At its end
    /// the new paragraph takes the style's next style (a heading's
    /// `Normal`).
    pub fn split(&mut self, at: &ParaAt, offset: usize) -> Result<()> {
        self.check_editable()?;
        let l = self.layout(at)?;
        let next = if offset >= l.text.len() {
            let style = self.styles.para_style(l.style.as_deref());
            style
                .and_then(|s| s.next.clone().filter(|n| *n != s.id))
                .filter(|n| self.styles.get(n).is_some())
        } else {
            None
        };
        let default = self
            .styles
            .default_of(crate::styles::StyleKind::Paragraph)
            .map(|s| s.id.clone());
        let next = next.map(|n| (Some(&n) != default.as_ref()).then_some(n));
        let src = &self.texts[&l.part];
        let s = edit::split(&l, src, offset, next.as_ref().map(|n| n.as_deref()))
            .map_err(Error::Refused)?;
        if !self.track_revisions {
            return self.apply(s);
        }
        // Tracked, the first paragraph's mark is the one inserted.
        self.begin_batch();
        let r = self.apply(s).and_then(|()| {
            let first = self.layout(at)?;
            let mut track = self.track();
            let m = edit::mark_change(&first, &self.texts[&first.part], false, &mut track)
                .map_err(Error::Refused)?;
            self.apply(m)
        });
        self.finish_batch(r)
    }

    /// Ends a batch begun for one command: kept when it went through,
    /// undone when it failed.
    fn finish_batch(&mut self, r: Result<()>) -> Result<()> {
        match r {
            Ok(()) => {
                self.end_batch();
                Ok(())
            }
            Err(e) => {
                if let Some(b) = self.batch.take() {
                    for (s, old) in b.splices.iter().rev() {
                        if let Some(t) = self.texts.get_mut(&s.part) {
                            t.replace_range(s.range.start..s.range.start + s.text.len(), old);
                        }
                    }
                    self.after_relationships(&b);
                }
                Err(e)
            }
        }
    }

    /// Joins a paragraph with the next one (Delete at its end, Backspace
    /// at the next one's start).
    pub fn join(&mut self, at: &ParaAt) -> Result<()> {
        self.check_editable()?;
        let first = self.layout(at)?;
        let next = ParaAt {
            story: at.story.clone(),
            index: at.index + 1,
        };
        let second = self.layout(&next)?;
        let src = &self.texts[&first.part];
        if self.track_revisions && !first.mark_inserted {
            // Tracked, the mark between them is marked deleted; Word
            // joins them when the change is accepted.
            if first.mark_deleted {
                return Ok(());
            }
            edit::join(&first, &second, src, false).map_err(Error::Refused)?;
            let mut track = self.track();
            let s = edit::mark_change(&first, src, true, &mut track).map_err(Error::Refused)?;
            return self.apply(s);
        }
        let s = edit::join(
            &first,
            &second,
            src,
            self.track_revisions && first.mark_inserted,
        )
        .map_err(Error::Refused)?;
        self.apply(s)
    }

    /// Deletes from `from` (a paragraph and an offset of its edit text) to
    /// `to` in a later paragraph of the same story, as one step: the
    /// paragraphs between are removed and the two ends joined.
    pub fn delete_between(&mut self, from: (&ParaAt, usize), to: (&ParaAt, usize)) -> Result<()> {
        if from.0.story != to.0.story || to.0.index < from.0.index {
            return Err(Error::Refused(
                "The end of a deletion comes before its start".into(),
            ));
        }
        if from.0 == to.0 {
            return self.replace(from.0, from.1..to.1, "");
        }
        self.check_editable()?;
        if self.track_revisions {
            self.begin_batch();
            let r = (|| {
                // From the end: every paragraph's text and mark marked
                // deleted, nothing taken away.
                let last = self.layout(to.0)?;
                let upto = to.1.min(last.text.len());
                self.replace(to.0, 0..upto, "")?;
                for i in (from.0.index..to.0.index).rev() {
                    let at = ParaAt {
                        story: from.0.story.clone(),
                        index: i,
                    };
                    let l = self.layout(&at)?;
                    let start = if i == from.0.index {
                        from.1.min(l.text.len())
                    } else {
                        0
                    };
                    if start < l.text.len() {
                        self.replace(&at, start..l.text.len(), "")?;
                    }
                    self.join(&at)?;
                }
                Ok(())
            })();
            return self.finish_batch(r);
        }
        self.begin_batch();
        let result = (|| {
            // From the end, so that earlier places stay where they are.
            let last = self.layout(to.0)?;
            let src = &self.texts[&last.part];
            let s = edit::replace(&last, src, 0..to.1.min(last.text.len()), "")
                .map_err(Error::Refused)?;
            self.apply(s)?;
            for _ in from.0.index + 1..to.0.index {
                let mid = ParaAt {
                    story: from.0.story.clone(),
                    index: from.0.index + 1,
                };
                let l = self.layout(&mid)?;
                let first = self.layout(from.0)?;
                if l.sibling.0 != first.sibling.0 {
                    return Err(Error::Refused(
                        "A deletion across a table is not written yet".into(),
                    ));
                }
                if l.sect.is_some() {
                    return Err(Error::Refused(
                        "A section ends inside the deletion; its section break is not deleted yet"
                            .into(),
                    ));
                }
                self.apply(Splice {
                    part: l.part.clone(),
                    range: l.span.clone(),
                    text: String::new(),
                })?;
            }
            let first = self.layout(from.0)?;
            let src = &self.texts[&first.part];
            let s = edit::replace(
                &first,
                src,
                from.1.min(first.text.len())..first.text.len(),
                "",
            )
            .map_err(Error::Refused)?;
            self.apply(s)?;
            self.join(from.0)
        })();
        match result {
            Ok(()) => {
                self.end_batch();
                Ok(())
            }
            Err(e) => {
                // Undone: nothing of a refused deletion stays.
                if let Some(b) = self.batch.take() {
                    for (s, old) in b.splices.into_iter().rev() {
                        if let Some(t) = self.texts.get_mut(&s.part) {
                            t.replace_range(s.range.start..s.range.start + s.text.len(), &old);
                        }
                    }
                }
                Err(e)
            }
        }
    }

    /// The parts holding stories: the main part, its headers and footers,
    /// its notes and comments.
    pub fn story_parts(&self) -> Vec<String> {
        let mut out = vec![self.main.clone()];
        if let Some(rels) = self.rels.get(&self.main) {
            for r in rels {
                if !r.external
                    && matches!(
                        r.kind.as_str(),
                        "header" | "footer" | "footnotes" | "endnotes" | "comments"
                    )
                {
                    let p = rels::resolve(&self.main, &r.target);
                    if self.texts.contains_key(&p) && !out.contains(&p) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    /// Accepts (`accept`) or rejects the tracked change of `w:id` `id`, as
    /// Word does: an insertion accepted is the document's, rejected it is
    /// gone; a deletion accepted is gone, rejected its text is back; a
    /// formatting change rejected brings the old formatting back; a
    /// paragraph mark inserted and rejected, or deleted and accepted,
    /// joins its paragraph with the next. One step.
    pub fn decide(&mut self, id: &str, accept: bool) -> Result<()> {
        self.begin_batch();
        let r = self.decide_one(id, accept);
        self.finish_batch(r)
    }

    /// [`Document::decide`] for every tracked change; formatting changes
    /// of paragraphs, which are not rejected yet, are left when rejecting.
    pub fn decide_all(&mut self, accept: bool) -> Result<()> {
        self.begin_batch();
        let r = (|| {
            for part in self.story_parts() {
                // Read again after each: the spans move.
                for id in crate::review::ids(&self.texts[&part], accept) {
                    self.decide_one(&id, accept)?;
                }
            }
            Ok(())
        })();
        self.finish_batch(r)
    }

    fn decide_one(&mut self, id: &str, accept: bool) -> Result<()> {
        use crate::review::Found;
        for part in self.story_parts() {
            let Some(found) = crate::review::find(&self.texts[&part], id) else {
                continue;
            };
            let src = &self.texts[&part];
            let splice = |range: std::ops::Range<usize>, text: String| Splice {
                part: part.clone(),
                range,
                text,
            };
            match found {
                Found::Runs {
                    deleted,
                    span,
                    inner,
                } => {
                    let text = match (deleted, accept) {
                        (false, true) => src[inner].to_string(),
                        (false, false) | (true, true) => String::new(),
                        (true, false) => crate::review::undeleted(&src[inner]),
                    };
                    let s = splice(span, text);
                    return self.apply(s);
                }
                Found::Format { change, rpr, old } => {
                    let text = if accept {
                        format!(
                            "{}{}",
                            &src[rpr.start..change.start],
                            &src[change.end..rpr.end]
                        )
                    } else {
                        let open_end = src[rpr.start..]
                            .find('>')
                            .map_or(rpr.start, |e| rpr.start + e + 1);
                        let open = &src[rpr.start..open_end];
                        let close_start = src[..rpr.end].rfind("</").unwrap_or(rpr.end);
                        format!("{open}{}{}", &src[old], &src[close_start..rpr.end])
                    };
                    let s = splice(rpr, text);
                    return self.apply(s);
                }
                Found::ParaFormat { change } => {
                    if !accept {
                        return Err(Error::Refused(
                            "A paragraph's formatting change is not rejected yet".into(),
                        ));
                    }
                    let s = splice(change, String::new());
                    return self.apply(s);
                }
                Found::Mark {
                    deleted,
                    span,
                    paragraph,
                } => {
                    let joins = deleted == accept;
                    let s = splice(span, String::new());
                    self.apply(s)?;
                    if joins {
                        if part != self.main {
                            return Err(Error::Refused(
                                "A paragraph mark outside the body is not joined yet".into(),
                            ));
                        }
                        let story = StoryId::Body;
                        let blocks = self.story_view(&story);
                        let mut paras = Vec::new();
                        fn walk<'a>(b: &'a [VBlock], out: &mut Vec<&'a VPara>) {
                            for x in b {
                                match x {
                                    VBlock::Para(p) => out.push(p),
                                    VBlock::Table(t) => {
                                        for row in &t.rows {
                                            for c in &row.cells {
                                                walk(&c.blocks, out);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        walk(&blocks, &mut paras);
                        let at = paras
                            .iter()
                            .find(|p| p.layout.span.start == paragraph)
                            .and_then(|p| p.at.clone())
                            .ok_or_else(|| Error::Refused("The paragraph is gone".into()))?;
                        let first = self.layout(&at)?;
                        let second = self.layout(&ParaAt {
                            story: at.story.clone(),
                            index: at.index + 1,
                        })?;
                        // The mark that stays is the second's.
                        let s = edit::join(&first, &second, &self.texts[&first.part], true)
                            .map_err(Error::Refused)?;
                        self.apply(s)?;
                    }
                    return Ok(());
                }
            }
        }
        Err(Error::Refused(format!("There is no tracked change {id}")))
    }

    /// The relationships of the main part read again from their text,
    /// after a step that changed them was made, undone or redone.
    fn after_relationships(&mut self, step: &Step) {
        let path = rels::rels_path(&self.main);
        if step.splices.iter().any(|(s, _)| s.part == path)
            && let Some(t) = self.texts.get(&path)
        {
            self.rels.insert(self.main.clone(), rels::parse(t));
        }
    }

    /// A part's text made editable: read from the package when it is
    /// there, else empty (a part a save adds once an edit writes it).
    fn ensure_text(&mut self, name: &str) -> Result<()> {
        if self.texts.contains_key(name) {
            return Ok(());
        }
        if self.package.contains(name) {
            return self.load(name);
        }
        self.texts.insert(name.to_owned(), String::new());
        Ok(())
    }

    /// A part's whole text replaced, as one splice of the step.
    fn rewrite(&mut self, part: &str, new: String) -> Result<()> {
        let old = self.texts.get(part).map_or("", String::as_str);
        if old == new {
            return Ok(());
        }
        let len = old.len();
        self.apply(Splice {
            part: part.to_owned(),
            range: 0..len,
            text: new,
        })
    }

    /// Whether the document is written in the strict namespace.
    fn strict(&self) -> bool {
        let main = self.texts.get(&self.main).map_or("", String::as_str);
        main[..main.len().min(4096)].contains("http://purl.oclc.org/ooxml/wordprocessingml/main")
    }

    /// The part a relationship of the main part names, by its type, made
    /// when there is none: the part with `initial` as its text, beside
    /// the main part, its relationship and its content type, in the
    /// step being made.
    fn related_or_new(
        &mut self,
        kind: &str,
        rel_type: &str,
        content_type: &str,
        file: &str,
        initial: String,
    ) -> Result<String> {
        if let Some(p) = self.related(kind) {
            self.ensure_text(&p)?;
            return Ok(p);
        }
        if self.strict() {
            return Err(Error::Refused(
                "Kalem does not add parts to a document in the strict namespace yet".into(),
            ));
        }
        let dir = self.main.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
        let at = |f: &str| {
            if dir.is_empty() {
                f.to_owned()
            } else {
                format!("{dir}/{f}")
            }
        };
        let (stem, ext) = file.rsplit_once('.').unwrap_or((file, "xml"));
        let mut name = file.to_owned();
        let mut n = 1;
        while self.package.contains(&at(&name))
            || self.texts.get(&at(&name)).is_some_and(|t| !t.is_empty())
        {
            name = format!("{stem}{n}.{ext}");
            n += 1;
        }
        let part = at(&name);
        self.ensure_text(&part)?;
        self.rewrite(&part, initial)?;
        let path = rels::rels_path(&self.main);
        self.ensure_text(&path)?;
        let (with_rel, _) = rels::add(&self.texts[&path], rel_type, &name, false);
        self.rewrite(&path, with_rel)?;
        let types = "[Content_Types].xml";
        self.ensure_text(types)?;
        let with_type = rels::add_override(&self.texts[types], &part, content_type);
        self.rewrite(types, with_type)?;
        self.rels
            .insert(self.main.clone(), rels::parse(&self.texts[&path]));
        Ok(part)
    }

    /// Whether comments may be written: a document protected for
    /// comments only takes them, as Word lets it.
    fn check_commentable(&self) -> Result<()> {
        match self.protection.as_deref() {
            None | Some("none") | Some("comments") => Ok(()),
            Some(p) => Err(Error::Refused(format!(
                "The document is protected ({p}); Kalem does not take protection away yet"
            ))),
        }
    }

    /// A part's root declaring `w14`, the namespace of paragraph IDs; its
    /// prefix for it.
    fn declare_w14(&mut self, part: &str) -> Result<String> {
        let text = self.texts.get(part).map_or("", String::as_str);
        let root = comments::root(text)
            .ok_or_else(|| Error::BadPart(format!("{part} has no root element")))?;
        let tag = &text[root.tag.clone()];
        let (new, prefix) = comments::with_extension(tag, "w14", comments::NS_W14);
        if new != tag {
            self.apply(Splice {
                part: part.to_owned(),
                range: root.tag,
                text: new,
            })?;
        }
        Ok(prefix)
    }

    /// Where the content of a part's root ends, the root opened first
    /// when it closes itself.
    fn root_content_end(&mut self, part: &str) -> Result<(usize, String)> {
        let text = self.texts.get(part).map_or("", String::as_str);
        let root = comments::root(text)
            .ok_or_else(|| Error::BadPart(format!("{part} has no root element")))?;
        let prefix = root.prefix().to_owned();
        if root.empty {
            let tag = &text[root.tag.clone()];
            let open = tag.trim_end_matches("/>").trim_end().to_owned() + ">";
            let at = root.tag.start + open.len();
            let close = format!("</{}>", root.qname);
            self.apply(Splice {
                part: part.to_owned(),
                range: root.tag,
                text: format!("{open}{close}"),
            })?;
            return Ok((at, prefix));
        }
        Ok((root.content_end, prefix))
    }

    /// Every paragraph ID of the document.
    fn used_para_ids(&self) -> std::collections::HashSet<u32> {
        self.texts
            .values()
            .flat_map(|t| comments::para_ids(t))
            .collect()
    }

    /// What the extended comments part says of each comment, by `w:id`:
    /// the comment it answers and whether it is done.
    pub fn comment_threads(&self) -> HashMap<String, (Option<String>, bool)> {
        let text = |kind: &str| {
            self.related(kind)
                .and_then(|p| self.texts.get(&p))
                .map_or("", String::as_str)
        };
        comments::threads(text("comments"), text("commentsExtended"))
    }

    /// Writes comment `id` by the author into the comments part (made
    /// when the document has none), each paragraph with an ID; and its
    /// entry in the extended comments part when it answers the comment
    /// whose last paragraph is `parent` or the document has that part.
    /// Its last paragraph's ID.
    fn write_comment(&mut self, id: &str, text: &str, parent: Option<&str>) -> Result<String> {
        let part = self.related_or_new(
            "comments",
            comments::REL_COMMENTS,
            comments::CT_COMMENTS,
            "comments.xml",
            comments::new_comments_part(),
        )?;
        let w14 = self.declare_w14(&part)?;
        let (at, p) = self.root_content_end(&part)?;
        let ids = comments::fresh_para_ids(&self.used_para_ids(), comments::lines(text).len());
        let last = ids
            .last()
            .cloned()
            .ok_or_else(|| Error::Refused("No paragraph ID is left for the comment".into()))?;
        let date = self.date.clone().unwrap_or_else(now_iso);
        let author = self.author.clone();
        let xml = comments::comment_xml(
            &p,
            Some(&w14),
            &comments::NewComment {
                id,
                author: &author,
                date: &date,
                text,
            },
            &ids,
            self.styles.get("CommentText").is_some(),
            self.styles.get("CommentReference").is_some(),
        )
        .map_err(Error::Refused)?;
        self.apply(Splice {
            part,
            range: at..at,
            text: xml,
        })?;
        if parent.is_some() || self.related("commentsExtended").is_some() {
            let ex = self.related_or_new(
                "commentsExtended",
                comments::REL_COMMENTS_EX,
                comments::CT_COMMENTS_EX,
                "commentsExtended.xml",
                comments::new_comments_ex_part(),
            )?;
            let (at, p15) = self.root_content_end(&ex)?;
            self.apply(Splice {
                part: ex,
                range: at..at,
                text: comments::comment_ex(&p15, &last, parent, false),
            })?;
        }
        Ok(last)
    }

    /// Adds a comment by the author ([`Document::set_revision_author`]) on
    /// the text from `from` to `to`, each a paragraph and a byte of its
    /// edit text, in one story (the body or a note), as Word writes one:
    /// the comment in the comments part (made when there is none), its
    /// range's start and end around the text, a run with its reference
    /// after the end. An empty range is a comment at a point: its
    /// reference alone. Its `w:id`. One step.
    pub fn add_comment(
        &mut self,
        from: (&ParaAt, usize),
        to: (&ParaAt, usize),
        text: &str,
    ) -> Result<String> {
        self.check_commentable()?;
        if from.0.story != to.0.story {
            return Err(Error::Refused(
                "A comment's text is in one story: the body, or one note".into(),
            ));
        }
        match &from.0.story {
            StoryId::Body | StoryId::Footnote(_) | StoryId::Endnote(_) => {}
            StoryId::Header(_) | StoryId::Footer(_) => {
                return Err(Error::Refused(
                    "Word keeps no comments in headers and footers".into(),
                ));
            }
            StoryId::Comment(_) => {
                return Err(Error::Refused(
                    "A comment is not made inside a comment".into(),
                ));
            }
        }
        if text.trim().is_empty() {
            return Err(Error::Refused("A comment needs text".into()));
        }
        let (from, to) = if (from.0.index, from.1) <= (to.0.index, to.1) {
            (from, to)
        } else {
            (to, from)
        };
        let id = self.track().next_id.to_string();
        self.begin_batch();
        let r = self.add_comment_steps(&id, from, to, text);
        self.finish_batch(r).map(|()| id)
    }

    fn add_comment_steps(
        &mut self,
        id: &str,
        from: (&ParaAt, usize),
        to: (&ParaAt, usize),
        text: &str,
    ) -> Result<()> {
        self.write_comment(id, text, None)?;
        let styled = self.styles.get("CommentReference").is_some();
        let point = from == to;
        let l = self.layout(to.0)?;
        let src = &self.texts[&l.part];
        let p = edit::prefix_of(src, l.start_tag.start);
        let cut = edit::cut(&l, src, to.1, false).map_err(Error::Refused)?;
        let reference = comments::reference_run(&p, id, styled);
        let end = if point {
            String::new()
        } else {
            format!("<{p}commentRangeEnd {p}id=\"{id}\"/>")
        };
        match cut.container_end {
            // Inside a link or an insertion: the reference after it, at
            // the paragraph's level.
            Some(e) => {
                self.apply(Splice {
                    part: l.part.clone(),
                    range: e..e,
                    text: reference,
                })?;
                if !point {
                    self.apply(cut.splice(&l.part, &end))?;
                }
            }
            None => self.apply(cut.splice(&l.part, &format!("{end}{reference}")))?,
        }
        if !point {
            let l = self.layout(from.0)?;
            let src = &self.texts[&l.part];
            let cut = edit::cut(&l, src, from.1, true).map_err(Error::Refused)?;
            self.apply(cut.splice(&l.part, &format!("<{p}commentRangeStart {p}id=\"{id}\"/>")))?;
        }
        Ok(())
    }

    /// Answers comment `parent` (its `w:id`) by the author, as Word does:
    /// a comment of its own, anchored on the answered one's text, its
    /// range's start after that one's and its reference after that one's
    /// reference, and an entry in the extended comments part (made when
    /// there is none) naming the answered comment's last paragraph, which
    /// gets an ID when it has none. A thread is one level deep, as in
    /// Word: an answer to an answer answers the first comment. The
    /// answer's `w:id`. One step.
    pub fn reply_comment(&mut self, parent: &str, text: &str) -> Result<String> {
        self.check_commentable()?;
        if text.trim().is_empty() {
            return Err(Error::Refused("An answer needs text".into()));
        }
        let missing = || Error::Refused(format!("There is no comment {parent}"));
        let part = self
            .related("comments")
            .filter(|p| self.texts.contains_key(p))
            .ok_or_else(missing)?;
        if !comments::comment_paragraphs(&self.texts[&part]).contains_key(parent) {
            return Err(missing());
        }
        let first = self
            .comment_threads()
            .get(parent)
            .and_then(|(p, _)| p.clone())
            .unwrap_or_else(|| parent.to_owned());
        let (story, mut m) = self
            .story_parts()
            .into_iter()
            .filter(|p| *p != part)
            .find_map(|p| {
                let m = comments::markers(&self.texts[&p], &first);
                m.found().then_some((p, m))
            })
            .ok_or_else(|| {
                Error::Refused(
                    "The comment is not anchored in the text, so its answer has no place".into(),
                )
            })?;
        if m.reference_run.is_none() && m.end.is_none() {
            return Err(Error::Refused("The comment's range has no end".into()));
        }
        // After the answers it has already, in the order they were made.
        for (answer, (p, _)) in self.comment_threads() {
            if p.as_deref() != Some(first.as_str()) {
                continue;
            }
            let a = comments::markers(&self.texts[&story], &answer);
            if let (Some(s), Some(ms)) = (a.start, m.start.as_mut())
                && s.end > ms.end
            {
                *ms = s;
            }
            if let (Some(r), Some(mr)) = (a.reference_run, m.reference_run.as_mut())
                && r.end > mr.end
            {
                *mr = r;
            }
        }
        let id = self.track().next_id.to_string();
        self.begin_batch();
        let r = self.reply_steps(&id, &first, text, &part, &story, &m);
        self.finish_batch(r).map(|()| id)
    }

    fn reply_steps(
        &mut self,
        id: &str,
        first: &str,
        text: &str,
        part: &str,
        story: &str,
        m: &comments::Markers,
    ) -> Result<()> {
        let parent_para = self.comment_para_id(part, first)?;
        self.write_comment(id, text, Some(&parent_para))?;
        let styled = self.styles.get("CommentReference").is_some();
        let src = &self.texts[story];
        let mut inserts: Vec<(usize, String)> = Vec::new();
        if let Some(s) = &m.start {
            let p = edit::prefix_of(src, s.start);
            inserts.push((s.end, format!("<{p}commentRangeStart {p}id=\"{id}\"/>")));
        }
        let (after, p) = match (&m.reference_run, &m.end) {
            (Some(run), _) => (run.end, edit::prefix_of(src, run.start)),
            (None, Some(e)) => (e.end, edit::prefix_of(src, e.start)),
            (None, None) => unreachable!("checked before the step"),
        };
        let end = if m.start.is_some() {
            format!("<{p}commentRangeEnd {p}id=\"{id}\"/>")
        } else {
            String::new()
        };
        inserts.push((
            after,
            format!("{end}{}", comments::reference_run(&p, id, styled)),
        ));
        inserts.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        for (at, text) in inserts {
            self.apply(Splice {
                part: story.to_owned(),
                range: at..at,
                text,
            })?;
        }
        Ok(())
    }

    /// The comments part, holding comment `id`.
    fn comment_part(&self, id: &str) -> Result<String> {
        let missing = || Error::Refused(format!("There is no comment {id}"));
        let part = self
            .related("comments")
            .filter(|p| self.texts.contains_key(p))
            .ok_or_else(missing)?;
        if !comments::comment_paragraphs(&self.texts[&part]).contains_key(id) {
            return Err(missing());
        }
        Ok(part)
    }

    /// Changes comment `id`'s text, a paragraph a line, as Word does:
    /// its author, date, mark and paragraph properties kept, its last
    /// paragraph's ID kept so that its answers and its done mark stay its
    /// own ([`comments::retext`]). One step; none when the text is the
    /// same.
    pub fn set_comment_text(&mut self, id: &str, text: &str) -> Result<()> {
        self.check_commentable()?;
        if text.trim().is_empty() {
            return Err(Error::Refused("A comment needs text".into()));
        }
        let part = self.comment_part(id)?;
        let view = self.view();
        if let Some(c) = view.comments.iter().find(|c| c.id == id) {
            let mut old = String::new();
            flow::text(&c.blocks, &mut old);
            if old.trim_end_matches('\n') == text {
                return Ok(());
            }
        }
        self.begin_batch();
        let r = self.set_text_steps(&part, id, text);
        self.finish_batch(r)
    }

    fn set_text_steps(&mut self, part: &str, id: &str, text: &str) -> Result<()> {
        let part = part.to_owned();
        let item = self
            .tree(&part)
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.span.clone())
            .ok_or_else(|| Error::Refused(format!("There is no comment {id}")))?;
        let src = &self.texts[&part];
        let p = edit::prefix_of(src, item.start);
        let w14 = if comments::has_para_ids(&src[item.clone()]) {
            Some(self.declare_w14(&part)?)
        } else {
            None
        };
        // The root may have changed: the comment read again.
        let item = self
            .tree(&part)
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.span.clone())
            .ok_or_else(|| Error::Refused(format!("There is no comment {id}")))?;
        let mut used = self.used_para_ids();
        let mut fresh = || {
            let id = comments::fresh_para_ids(&used, 1).pop()?;
            used.insert(u32::from_str_radix(&id, 16).ok()?);
            Some(id)
        };
        let (range, new) = comments::retext(
            &self.texts[&part][item.clone()],
            &p,
            w14.as_deref(),
            text,
            &mut fresh,
        )
        .map_err(Error::Refused)?;
        self.apply(Splice {
            part,
            range: item.start + range.start..item.start + range.end,
            text: new,
        })
    }

    /// Marks comment `id` done (`done`) or open again, as Word does: on
    /// its thread's first comment, by `w15:done` in the extended comments
    /// part (made when there is none, and its last paragraph given an ID
    /// when it has none). One step.
    pub fn resolve_comment(&mut self, id: &str, done: bool) -> Result<()> {
        self.check_commentable()?;
        let part = self.comment_part(id)?;
        let threads = self.comment_threads();
        let first = threads
            .get(id)
            .and_then(|(p, _)| p.clone())
            .unwrap_or_else(|| id.to_owned());
        if threads.get(&first).is_some_and(|(_, d)| *d) == done {
            return Ok(());
        }
        self.begin_batch();
        let r = self.resolve_steps(&part, &first, done);
        self.finish_batch(r)
    }

    fn resolve_steps(&mut self, part: &str, first: &str, done: bool) -> Result<()> {
        let para = self.comment_para_id(part, first)?;
        let ex = self.related_or_new(
            "commentsExtended",
            comments::REL_COMMENTS_EX,
            comments::CT_COMMENTS_EX,
            "commentsExtended.xml",
            comments::new_comments_ex_part(),
        )?;
        let entry = comments::elements(&self.texts[&ex], "commentEx", "paraId")
            .into_iter()
            .find(|(_, _, v)| v.eq_ignore_ascii_case(&para));
        match entry {
            Some((_, tag, _)) => {
                let text = &self.texts[&ex][tag.clone()];
                let p = edit::prefix_of(&self.texts[&ex], tag.start);
                let new = xml::set_attr(text, &format!("{p}done"), if done { "1" } else { "0" });
                self.apply(Splice {
                    part: ex,
                    range: tag,
                    text: new,
                })
            }
            None => {
                let (at, p15) = self.root_content_end(&ex)?;
                self.apply(Splice {
                    part: ex,
                    range: at..at,
                    text: comments::comment_ex(&p15, &para, None, done),
                })
            }
        }
    }

    /// Takes comment `id` away, as Word does: with its answers when it is
    /// a thread's first comment; its range's start and end and its
    /// reference (the run, when it holds nothing else) out of the
    /// stories, the comment out of the comments part, its entries out of
    /// the parts Word keeps beside it (`commentsExtended`, `commentsIds`,
    /// `commentsExtensible`). The runs a range split stay as they are,
    /// as in Word. One step.
    pub fn remove_comment(&mut self, id: &str) -> Result<()> {
        self.check_commentable()?;
        let part = self.comment_part(id)?;
        let threads = self.comment_threads();
        let mut ids = vec![id.to_owned()];
        if threads.get(id).and_then(|(p, _)| p.as_ref()).is_none() {
            let mut answers: Vec<String> = threads
                .iter()
                .filter(|(_, (p, _))| p.as_deref() == Some(id))
                .map(|(a, _)| a.clone())
                .collect();
            answers.sort();
            ids.extend(answers);
        }
        self.begin_batch();
        let r = self.remove_steps(&part, &ids);
        self.finish_batch(r)
    }

    fn remove_steps(&mut self, part: &str, ids: &[String]) -> Result<()> {
        // The stories: the markers and references.
        for story in self.story_parts() {
            if story == part {
                continue;
            }
            let src = &self.texts[&story];
            let mut spans: Vec<Span> = Vec::new();
            for id in ids {
                let m = comments::markers(src, id);
                spans.extend(m.start);
                spans.extend(m.end);
                match (&m.reference_run, &m.reference) {
                    (Some(run), _) if comments::reference_only(src, run) => spans.push(run.clone()),
                    (_, Some(r)) => spans.push(r.clone()),
                    _ => {}
                }
            }
            self.remove_spans(&story, spans)?;
        }
        // The comments, and the paragraph IDs they leave.
        let paras = comments::comment_paragraphs(&self.texts[part]);
        let gone: Vec<String> = ids
            .iter()
            .filter_map(|id| paras.get(id))
            .flatten()
            .filter_map(|(_, p)| p.clone())
            .collect();
        let tree = self.tree(part);
        let spans = tree
            .items
            .iter()
            .filter(|i| ids.contains(&i.id))
            .map(|i| i.span.clone())
            .collect();
        self.remove_spans(part, spans)?;
        // Their entries beside: by paragraph ID, and by the durable ID
        // `commentsIds` gives each.
        let has = |v: &str| gone.iter().any(|g| g.eq_ignore_ascii_case(v));
        let mut durable: Vec<String> = Vec::new();
        for (kind, name) in [
            ("commentsExtended", "commentEx"),
            ("commentsIds", "commentId"),
        ] {
            let Some(p) = self.related(kind).filter(|p| self.texts.contains_key(p)) else {
                continue;
            };
            let src = &self.texts[&p];
            let mut spans = Vec::new();
            for (span, tag, v) in comments::elements(src, name, "paraId") {
                if has(&v) {
                    spans.push(span);
                    if let Some(d) = comments::elements(&src[tag.clone()], name, "durableId")
                        .first()
                        .map(|e| e.2.clone())
                    {
                        durable.push(d);
                    }
                }
            }
            self.remove_spans(&p, spans)?;
        }
        if let Some(p) = self
            .related("commentsExtensible")
            .filter(|p| self.texts.contains_key(p))
        {
            let spans = comments::elements(&self.texts[&p], "commentExtensible", "durableId")
                .into_iter()
                .filter(|(_, _, v)| durable.iter().any(|d| d.eq_ignore_ascii_case(v)))
                .map(|(s, _, _)| s)
                .collect();
            self.remove_spans(&p, spans)?;
        }
        Ok(())
    }

    /// Takes spans of a part away, those that overlap as one, the last
    /// first.
    fn remove_spans(&mut self, part: &str, mut spans: Vec<Span>) -> Result<()> {
        spans.sort_by_key(|s| (s.start, s.end));
        let mut merged: Vec<Span> = Vec::new();
        for s in spans {
            match merged.last_mut() {
                Some(m) if s.start < m.end => m.end = m.end.max(s.end),
                _ => merged.push(s),
            }
        }
        for s in merged.into_iter().rev() {
            self.apply(Splice {
                part: part.to_owned(),
                range: s,
                text: String::new(),
            })?;
        }
        Ok(())
    }

    /// The ID of comment `id`'s last paragraph, given one when it has
    /// none.
    fn comment_para_id(&mut self, part: &str, id: &str) -> Result<String> {
        let w14 = self.declare_w14(part)?;
        let paras = comments::comment_paragraphs(&self.texts[part]);
        let (span, para) = paras
            .get(id)
            .and_then(|ps| ps.last())
            .cloned()
            .ok_or_else(|| Error::Refused(format!("Comment {id} has no paragraph")))?;
        if let Some(p) = para {
            return Ok(p);
        }
        let fresh = comments::fresh_para_ids(&self.used_para_ids(), 1)
            .pop()
            .ok_or_else(|| Error::Refused("No paragraph ID is left for the comment".into()))?;
        let tag = &self.texts[part][span.clone()];
        let new = xml::set_attr(tag, &format!("{w14}paraId"), &fresh);
        self.apply(Splice {
            part: part.to_owned(),
            range: span,
            text: new,
        })?;
        Ok(fresh)
    }

    /// Undoes the last step; whether there was one.
    pub fn undo(&mut self) -> bool {
        if self.at == 0 {
            return false;
        }
        self.at -= 1;
        let step = self.history[self.at].clone();
        for (s, old) in step.splices.iter().rev() {
            if let Some(t) = self.texts.get_mut(&s.part) {
                t.replace_range(s.range.start..s.range.start + s.text.len(), old);
            }
        }
        self.after_relationships(&step);
        true
    }

    /// Redoes the step undone last; whether there was one.
    pub fn redo(&mut self) -> bool {
        if self.at >= self.history.len() {
            return false;
        }
        let step = self.history[self.at].clone();
        for (s, _) in &step.splices {
            if let Some(t) = self.texts.get_mut(&s.part) {
                t.replace_range(s.range.clone(), &s.text);
            }
        }
        self.after_relationships(&step);
        self.at += 1;
        true
    }

    /// Whether there are edits not saved.
    pub fn is_dirty(&self) -> bool {
        self.at != self.saved_at
    }

    /// The parts a save would write anew.
    pub fn changed_parts(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .texts
            .iter()
            .filter(|(n, t)| match self.originals.get(*n) {
                Some(o) => o != *t,
                None => !t.is_empty(),
            })
            .map(|(n, _)| n.clone())
            .collect();
        out.sort();
        out
    }

    /// The file with the edits: the input itself when nothing differs
    /// from it; else every part but the edited ones copied byte for byte.
    pub fn save(&mut self) -> Result<Vec<u8>> {
        let names: Vec<String> = self.texts.keys().cloned().collect();
        for n in names {
            match self.originals.get(&n) {
                Some(o) if Some(o) == self.texts.get(&n) => self.package.restore_part(&n),
                // A part an edit made, undone: not written.
                None if self.texts[&n].is_empty() => self.package.remove_part(&n),
                _ => self
                    .package
                    .set_part(&n, self.texts[&n].clone().into_bytes()),
            }
        }
        let out = self.package.write()?;
        self.saved_at = self.at;
        Ok(out)
    }

    /// The text of a part as edited (for tests and the example).
    pub fn part_text(&self, name: &str) -> Option<&str> {
        self.texts.get(name).map(String::as_str)
    }
}
