//! The plugin as Kalem's `flow-viewer` (D54, plugin API 0.2.7): a
//! document is one unit of flowing text whose paragraphs Kalem lays out
//! itself (the `flow` interface: [`crate::contract`]) and edits as text,
//! each edit made here in the document's runs; its comments and tracked
//! changes are the `annotations` interface's, accepted and rejected here.
//! A document with a password to open opens with it; the file saves as
//! itself, encrypted again when it was. The unit has no picture: its
//! render says so, and a Kalem older than 0.2.7 shows its text.

use kalem_viewer::{
    Detection, FileHandle, InfoField, OutlineEntry, RenderRequest, Rendered, Result, SaveOutput,
    Structure, Unit, UnitKind, Viewer, ViewerDocument, ViewerError,
};

use crate::document::{DocView, Document, Kind};
use crate::flow::{self, VBlock, VPara};

/// The extensions of WordprocessingML documents.
const EXTENSIONS: [&str; 4] = ["docx", "docm", "dotx", "dotm"];

/// Whether `bytes` is an Office file encrypted with a password: a
/// compound file (OLE) holding an `EncryptionInfo` stream (MS-OFFCRYPTO
/// 2.3.4), its directory naming the stream in UTF-16.
fn encrypted(bytes: &[u8]) -> bool {
    const NAME: &[u8] = b"E\0n\0c\0r\0y\0p\0t\0i\0o\0n\0I\0n\0f\0o\0";
    bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) && bytes.windows(NAME.len()).any(|w| w == NAME)
}

/// 64 random bits for an encrypted document's salts and key: from Kalem
/// in a component (the system's secure source), natively from the
/// process's random hash keys.
fn random_bits() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        kalem_plugin::viewer::kalem::plugin::clock::random()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::hash::{BuildHasher, Hasher};
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(N.fetch_add(1, Ordering::Relaxed));
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.finish()
    }
}

fn err(e: impl std::fmt::Display) -> ViewerError {
    ViewerError(e.to_string())
}

/// The viewer of Word documents.
#[derive(Debug, Default, Clone, Copy)]
pub struct DocxViewer;

impl Viewer for DocxViewer {
    fn id(&self) -> &str {
        "docx"
    }

    fn name(&self) -> &str {
        "Word documents"
    }

    fn extensions(&self) -> &[&str] {
        &EXTENSIONS
    }

    fn detect(&self, name: &str, head: &[u8]) -> Detection {
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !EXTENSIONS.contains(&ext.as_str()) {
            return Detection::No;
        }
        if head.starts_with(b"PK\x03\x04") {
            Detection::Magic
        } else {
            // An encrypted document is a compound file.
            Detection::Extension
        }
    }

    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        self.open_with_password(file, "")
    }

    fn open_with_password(
        &self,
        file: FileHandle,
        password: &str,
    ) -> Result<Box<dyn ViewerDocument>> {
        #[cfg(target_arch = "wasm32")]
        crate::component_clock();
        let bytes = file.read_all()?;
        let (bytes, password) = if encrypted(&bytes) {
            if password.is_empty() {
                return Err(ViewerError::needs_password());
            }
            match kalem_ooxml::crypto::decrypt(&bytes, password) {
                Ok(plain) => (plain, Some(password.to_owned())),
                Err(kalem_ooxml::crypto::CryptoError::WrongPassword) => {
                    return Err(ViewerError::needs_password());
                }
                Err(e) => return Err(err(e.describe("document"))),
            }
        } else {
            (bytes, None)
        };
        let doc = Document::open(bytes).map_err(err)?;
        let view = doc.view();
        Ok(Box::new(DocxDoc {
            doc,
            view,
            password,
            name: file.name().to_owned(),
            version: 0,
            flow: None,
        }))
    }

    /// A new document of one of the four kinds, as Word makes a blank
    /// one; sheets of entries become tables ([`crate::blank`]).
    fn new_file(&self, extension: &str, sheets: &[kalem_viewer::NewSheet]) -> Result<Vec<u8>> {
        #[cfg(target_arch = "wasm32")]
        crate::component_clock();
        crate::blank::new_document(&extension.to_ascii_lowercase(), sheets).map_err(err)
    }
}

/// An open document.
struct DocxDoc {
    doc: Document,
    view: DocView,
    password: Option<String>,
    name: String,
    /// The flow's version: one more at each change.
    version: u64,
    /// The flow as last given to Kalem, until the next change.
    flow: Option<crate::contract::FlowCache>,
}

/// A tracked change's `w:id` from its annotation's ID (`r` and the ID).
fn revision(id: &str) -> Result<&str> {
    id.strip_prefix('r')
        .ok_or_else(|| ViewerError(format!("{id} is not a tracked change")))
}

impl DocxDoc {
    /// The flow Kalem is given, made once a version.
    fn cache(&mut self) -> &crate::contract::FlowCache {
        if self.flow.is_none() {
            self.flow = Some(crate::contract::build(&self.doc.view_with(true)));
        }
        self.flow.get_or_insert_with(Default::default)
    }

    /// The paragraph of a flow index.
    fn para(&mut self, index: u32) -> Result<crate::flow::ParaAt> {
        self.cache()
            .paras
            .get(index as usize)
            .cloned()
            .ok_or_else(|| ViewerError(format!("There is no paragraph {index}")))
    }

    /// After a change: a new version, the view read again.
    fn refreshed(&mut self) {
        self.version += 1;
        self.flow = None;
        self.view = self.doc.view();
    }

    /// An edit's result; the view read again when it was made.
    fn changed(&mut self, r: std::result::Result<(), crate::document::Error>) -> Result<()> {
        match r {
            Ok(()) => {
                self.refreshed();
                Ok(())
            }
            Err(e) => Err(err(e)),
        }
    }
}

/// Counts of what a document holds, for the information panel.
#[derive(Debug, Default)]
struct Counts {
    paragraphs: usize,
    words: usize,
    characters: usize,
    tables: usize,
    pictures: usize,
    inserted: usize,
    deleted: usize,
    placeholders: usize,
}

fn count(blocks: &[VBlock], c: &mut Counts) {
    for b in blocks {
        match b {
            VBlock::Para(p) => count_para(p, c),
            VBlock::Table(t) => {
                c.tables += 1;
                for row in &t.rows {
                    for cell in &row.cells {
                        count(&cell.blocks, c);
                    }
                }
            }
            VBlock::Frame(f) => count(f, c),
            VBlock::Placeholder(_) => c.placeholders += 1,
        }
    }
}

fn count_para(p: &VPara, c: &mut Counts) {
    let text = flow::para_text(p);
    if !text.trim().is_empty() {
        c.paragraphs += 1;
    }
    c.words += text.split_whitespace().count();
    c.characters += text.chars().filter(|ch| !ch.is_control()).count();
    for r in &p.runs {
        match &r.piece {
            flow::Piece::Picture { .. } => c.pictures += 1,
            flow::Piece::Placeholder(_) => c.placeholders += 1,
            _ => {}
        }
    }
    let ins = p.runs.iter().filter(|r| r.inserted.is_some()).count();
    let del = p.runs.iter().filter(|r| r.deleted.is_some()).count();
    c.inserted += ins;
    c.deleted += del;
}

impl ViewerDocument for DocxDoc {
    fn structure(&self) -> Structure {
        Structure {
            units: vec![Unit {
                kind: UnitKind::Page,
                label: "Document".into(),
                duration_ms: None,
            }],
            outline: self
                .view
                .outline()
                .into_iter()
                .map(|(title, level)| OutlineEntry {
                    title,
                    unit: 0,
                    level,
                })
                .collect(),
        }
    }

    fn render(&mut self, _unit: usize, _request: RenderRequest) -> Result<Rendered> {
        Err(ViewerError(
            "A Word document is laid out by Kalem from its paragraphs (the flow interface, \
             plugin API 0.2.7), not rendered by the plugin"
                .into(),
        ))
    }

    fn text(&self, _unit: usize) -> String {
        self.view.text()
    }

    fn info(&self) -> Vec<kalem_viewer::InfoField> {
        let p = self.doc.properties();
        let mut c = Counts::default();
        count(&self.view.body, &mut c);
        let mut out = vec![InfoField::new("Name", self.name.clone())];
        let kind = match self.doc.kind() {
            Kind::Document => "Word document",
            Kind::MacroDocument => "Word document with macros",
            Kind::Template => "Word template",
            Kind::MacroTemplate => "Word template with macros",
        };
        out.push(InfoField::new("Kind", kind));
        for (label, v) in [
            ("Title", &p.title),
            ("Subject", &p.subject),
            ("Author", &p.creator),
            ("Keywords", &p.keywords),
            ("Created", &p.created),
            ("Modified", &p.modified),
            ("Last modified by", &p.last_modified_by),
            ("Application", &p.application),
            ("Pages (as last counted)", &p.pages),
        ] {
            if let Some(v) = v {
                out.push(InfoField::new(label, v.clone()));
            }
        }
        out.push(InfoField::new("Paragraphs", c.paragraphs.to_string()));
        out.push(InfoField::new("Words", c.words.to_string()));
        out.push(InfoField::new("Characters", c.characters.to_string()));
        out.push(InfoField::new(
            "Sections",
            self.doc.sections().len().to_string(),
        ));
        for (label, n) in [
            ("Tables", c.tables),
            ("Pictures", c.pictures),
            ("Footnotes", self.view.footnotes.len()),
            ("Endnotes", self.view.endnotes.len()),
            ("Comments", self.view.comments.len()),
            ("Insertions (tracked)", c.inserted),
            ("Deletions (tracked)", c.deleted),
            ("Shown by name", c.placeholders),
        ] {
            if n > 0 {
                out.push(InfoField::new(label, n.to_string()));
            }
        }
        if self.doc.tracks_changes() {
            out.push(InfoField::new("Track changes", "on"));
        }
        if let Some(p) = self.doc.protection() {
            out.push(InfoField::new("Protection", p.to_owned()));
        }
        if self.doc.has_vba() {
            out.push(InfoField::new("Macros", "a VBA project, kept as it is"));
        }
        if self.password.is_some() {
            out.push(InfoField::new("Encrypted", "with a password to open"));
        }
        out
    }

    fn modified(&self) -> bool {
        self.doc.is_dirty()
    }

    // Kalem's flow (plugin API 0.2.7): the paragraphs Kalem lays out and
    // edits, every story's in one sequence.

    fn flow(&mut self, unit: usize) -> Option<kalem_viewer::FlowLayout> {
        if unit != 0 {
            return None;
        }
        let items = self.cache().items.len() as u32;
        Some(kalem_viewer::FlowLayout {
            items,
            version: self.version,
            editable: self.doc.protection().is_none(),
        })
    }

    fn flow_items(&mut self, _unit: usize, from: u32, count: u32) -> Vec<kalem_viewer::FlowItem> {
        let items = &self.cache().items;
        let from = (from as usize).min(items.len());
        let to = from.saturating_add(count as usize).min(items.len());
        items[from..to].to_vec()
    }

    fn flow_replace(
        &mut self,
        _unit: usize,
        paragraph: u32,
        range: std::ops::Range<u32>,
        text: &str,
    ) -> Result<()> {
        let at = self.para(paragraph)?;
        let r = self
            .doc
            .replace(&at, range.start as usize..range.end as usize, text);
        self.changed(r)
    }

    fn flow_split(&mut self, _unit: usize, at: kalem_viewer::FlowPlace) -> Result<()> {
        let p = self.para(at.paragraph)?;
        let r = self.doc.split(&p, at.offset as usize);
        self.changed(r)
    }

    fn flow_join(&mut self, _unit: usize, paragraph: u32) -> Result<()> {
        let at = self.para(paragraph)?;
        let next = self.para(paragraph + 1)?;
        if next.story != at.story || next.index != at.index + 1 {
            return Err(ViewerError(
                "Only a paragraph and the next one of the same part are joined".into(),
            ));
        }
        let r = self.doc.join(&at);
        self.changed(r)
    }

    fn flow_delete(
        &mut self,
        _unit: usize,
        from: kalem_viewer::FlowPlace,
        to: kalem_viewer::FlowPlace,
    ) -> Result<()> {
        let a = self.para(from.paragraph)?;
        let b = self.para(to.paragraph)?;
        if a.story != b.story {
            return Err(ViewerError(
                "A deletion across a header, a note and the body is not made".into(),
            ));
        }
        let r = self
            .doc
            .delete_between((&a, from.offset as usize), (&b, to.offset as usize));
        self.changed(r)
    }

    fn flow_set_marks(
        &mut self,
        _unit: usize,
        from: kalem_viewer::FlowPlace,
        to: kalem_viewer::FlowPlace,
        changes: &[kalem_viewer::MarkChange],
    ) -> Result<()> {
        let (from, to) = if (from.paragraph, from.offset) <= (to.paragraph, to.offset) {
            (from, to)
        } else {
            (to, from)
        };
        let mut spans = Vec::new();
        for i in from.paragraph..=to.paragraph {
            let at = self.para(i)?;
            let len = self.cache().lens.get(i as usize).copied().unwrap_or(0);
            let start = if i == from.paragraph {
                from.offset as usize
            } else {
                0
            };
            let end = if i == to.paragraph {
                to.offset as usize
            } else {
                len
            };
            spans.push((at, start.min(len)..end.min(len)));
        }
        let r = self.doc.set_marks(&spans, changes);
        self.changed(r)
    }

    fn flow_set_paragraphs(
        &mut self,
        _unit: usize,
        from: u32,
        to: u32,
        changes: &[kalem_viewer::ParagraphChange],
    ) -> Result<()> {
        let mut paras = Vec::new();
        for i in from.min(to)..=from.max(to) {
            paras.push(self.para(i)?);
        }
        let r = self.doc.set_paragraph_format(&paras, changes);
        self.changed(r)
    }

    fn flow_set_style(&mut self, _unit: usize, from: u32, to: u32, style: &str) -> Result<()> {
        let mut paras = Vec::new();
        for i in from.min(to)..=from.max(to) {
            paras.push(self.para(i)?);
        }
        let r = self.doc.set_paragraph_style(&paras, style);
        self.changed(r)
    }

    fn flow_styles(&mut self) -> Vec<kalem_viewer::FlowStyle> {
        use crate::styles::StyleKind;
        self.doc
            .styles()
            .styles
            .iter()
            .filter_map(|s| {
                let kind = match s.kind {
                    StyleKind::Paragraph => kalem_viewer::FlowStyleKind::Paragraph,
                    StyleKind::Character => kalem_viewer::FlowStyleKind::Character,
                    _ => return None,
                };
                Some(kalem_viewer::FlowStyle {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    kind,
                    shown: !s.hidden,
                })
            })
            .collect()
    }

    fn has_history(&self) -> bool {
        true
    }

    fn undo(&mut self) -> Result<bool> {
        let done = self.doc.undo();
        if done {
            self.refreshed();
        }
        Ok(done)
    }

    fn redo(&mut self) -> Result<bool> {
        let done = self.doc.redo();
        if done {
            self.refreshed();
        }
        Ok(done)
    }

    fn begin_batch(&mut self) {
        self.doc.begin_batch();
    }

    fn end_batch(&mut self) {
        if self.doc.end_batch() {
            self.refreshed();
        }
    }

    // Comments and tracked changes (the `annotations` interface).

    fn annotations(&mut self, _unit: Option<usize>) -> Vec<kalem_viewer::Annotation> {
        self.cache().annotations.clone()
    }

    fn set_author(&mut self, name: &str) {
        self.doc.set_revision_author(name, None);
    }

    fn comment(&mut self, on: kalem_viewer::Anchor, text: &str) -> Result<String> {
        let kalem_viewer::Anchor::Flow { from, to, .. } = on else {
            return Err(ViewerError(
                "A Word document's comments are on its text".into(),
            ));
        };
        let a = self.para(from.paragraph)?;
        let b = self.para(to.paragraph)?;
        let r = self
            .doc
            .add_comment((&a, from.offset as usize), (&b, to.offset as usize), text);
        let id = r.map_err(err)?;
        self.refreshed();
        Ok(format!("c{id}"))
    }

    fn set_comment_text(&mut self, id: &str, text: &str) -> Result<()> {
        let id = id
            .strip_prefix('c')
            .ok_or_else(|| ViewerError(format!("{id} is not a comment")))?;
        let r = self.doc.set_comment_text(id, text);
        self.changed(r)
    }

    fn resolve(&mut self, id: &str, done: bool) -> Result<()> {
        let id = id
            .strip_prefix('c')
            .ok_or_else(|| ViewerError(format!("{id} is not a comment")))?;
        let r = self.doc.resolve_comment(id, done);
        self.changed(r)
    }

    fn remove_comment(&mut self, id: &str) -> Result<()> {
        let id = id
            .strip_prefix('c')
            .ok_or_else(|| ViewerError(format!("{id} is not a comment")))?;
        let r = self.doc.remove_comment(id);
        self.changed(r)
    }

    fn reply(&mut self, parent: &str, text: &str) -> Result<String> {
        let parent = parent
            .strip_prefix('c')
            .ok_or_else(|| ViewerError(format!("{parent} is not a comment")))?;
        let id = self.doc.reply_comment(parent, text).map_err(err)?;
        self.refreshed();
        Ok(format!("c{id}"))
    }

    fn accept(&mut self, id: &str) -> Result<()> {
        let r = self.doc.decide(revision(id)?, true);
        self.changed(r)
    }

    fn reject(&mut self, id: &str) -> Result<()> {
        let r = self.doc.decide(revision(id)?, false);
        self.changed(r)
    }

    fn accept_all(&mut self, _unit: Option<usize>) -> Result<()> {
        let r = self.doc.decide_all(true);
        self.changed(r)
    }

    fn reject_all(&mut self, _unit: Option<usize>) -> Result<()> {
        let r = self.doc.decide_all(false);
        self.changed(r)
    }

    fn tracking(&mut self) -> Option<bool> {
        Some(self.doc.tracks_changes())
    }

    fn set_tracking(&mut self, on: bool) -> Result<()> {
        let r = self.doc.set_track_changes(on);
        self.changed(r)
    }

    fn save(&mut self) -> Result<SaveOutput> {
        let package = self.doc.save().map_err(err)?;
        let bytes = match &self.password {
            Some(p) => kalem_ooxml::crypto::encrypt(&package, p, &mut random_bits).map_err(err)?,
            None => package,
        };
        Ok(SaveOutput {
            bytes,
            losses: Vec::new(),
        })
    }

    fn save_as(&mut self, extension: &str) -> Result<SaveOutput> {
        if extension.eq_ignore_ascii_case(self.doc.kind().extension()) {
            return self.save();
        }
        Err(ViewerError(format!(
            "Kalem does not write a .{} as a .{extension} yet",
            self.doc.kind().extension()
        )))
    }
}
