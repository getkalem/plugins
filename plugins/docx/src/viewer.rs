//! The plugin as Kalem's `document-viewer` (D54): a document is one unit
//! whose text, outline and information Kalem shows, searched and copied;
//! a document with a password to open opens with it; the file saves as
//! itself, encrypted again when it was.
//!
//! Kalem lays a document's paragraphs out itself through the interface of
//! the docx list's WP5, which does not exist yet: until then the unit has
//! no picture, and its render says so; the terminal and search read its
//! text.

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
        }))
    }
}

/// An open document.
struct DocxDoc {
    doc: Document,
    view: DocView,
    password: Option<String>,
    name: String,
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
            "Kalem does not lay Word documents out yet: their text is shown in the terminal, \
             searched and copied (the docx plugin's flow interface, WP5)"
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
