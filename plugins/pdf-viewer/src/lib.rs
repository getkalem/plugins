//! `pdf-viewer`, the `document-viewer` of PDF files (D54, T3.7.3): pages
//! rendered by hayro, a pure-Rust rasterizer (D56), with their labels, the
//! outline, links and a text layer for search, copy and the terminal.
//!
//! A page is rendered at the scale the host asks for, so it stays sharp
//! when zoomed, and the last few renders are kept. Annotations and form
//! fields are drawn from their appearance streams. Files whose fonts are
//! not embedded draw with the base-14 substitutes hayro embeds.
//!
//! Nothing is edited yet: highlights, notes and form filling written as
//! incremental updates are T3.7.3b.

mod catalog;
mod strings;
mod text;

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use hayro::hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf, PdfVersion};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings};
use kalem_viewer::{
    Bitmap, Detection, FileHandle, InfoField, Link, RenderRequest, Rendered, Result, Structure,
    Theme, Unit, UnitKind, Viewer, ViewerDocument, ViewerError,
};

use crate::catalog::Catalog;

/// The longest side a page is rendered at, in pixels.
pub const MAX_SIDE: f32 = 8192.0;

/// The most pixels a page is rendered at: 40 megapixels, 160 MB.
pub const MAX_PIXELS: f32 = 40_000_000.0;

/// How many renders a document keeps: the page shown, its neighbors, and
/// the page at the zoom before.
const CACHED_RENDERS: usize = 6;

/// The error of a file that needs a password; the host asks for one and
/// opens it with [`PdfViewer::open_with_password`].
pub const PASSWORD_REQUIRED: &str = "This PDF is protected by a password";

/// The PDF viewer.
#[derive(Debug, Default)]
pub struct PdfViewer;

impl Viewer for PdfViewer {
    fn id(&self) -> &str {
        "pdf-viewer"
    }

    fn name(&self) -> &str {
        "PDF"
    }

    fn extensions(&self) -> &[&str] {
        &["pdf"]
    }

    fn detect(&self, name: &str, head: &[u8]) -> Detection {
        // The header may follow up to 1,024 bytes of anything (Annex H's
        // implementation notes; Acrobat reads it there).
        let head = &head[..head.len().min(1024)];
        if head.windows(5).any(|w| w == b"%PDF-") {
            return Detection::Magic;
        }
        let pdf = name
            .rsplit_once('.')
            .is_some_and(|(_, e)| e.eq_ignore_ascii_case("pdf"));
        if pdf {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        self.open_with_password(file, "")
    }
}

impl PdfViewer {
    /// Opens an encrypted file with its password (the user's or the
    /// owner's); `open` tries the empty one.
    pub fn open_with_password(
        &self,
        file: FileHandle,
        password: &str,
    ) -> Result<Box<dyn ViewerDocument>> {
        Ok(Box::new(PdfDocument::new(file, password)?))
    }
}

/// A PDF file opened.
pub struct PdfDocument {
    pdf: Pdf,
    catalog: Catalog,
    structure: Structure,
    settings: InterpreterSettings,
    name: String,
    size: u64,
    encrypted: bool,
    /// Pages' texts and their glyphs' boxes, extracted when first asked
    /// for.
    texts: Mutex<HashMap<usize, text::PageText>>,
    /// The last renders: page, scale, theme.
    renders: VecDeque<(usize, u32, Theme, Bitmap)>,
}

impl std::fmt::Debug for PdfDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfDocument")
            .field("name", &self.name)
            .field("pages", &self.structure.units.len())
            .finish_non_exhaustive()
    }
}

impl PdfDocument {
    fn new(file: FileHandle, password: &str) -> Result<PdfDocument> {
        let bytes = file.read_all()?;
        let size = bytes.len() as u64;
        let encrypted = has_encrypt(&bytes);
        let pdf = Pdf::new_with_password(bytes, password).map_err(|e| match e {
            LoadPdfError::Decryption(DecryptionError::PasswordProtected) => {
                ViewerError(PASSWORD_REQUIRED.into())
            }
            LoadPdfError::Decryption(DecryptionError::UnsupportedAlgorithm) => {
                ViewerError("This PDF is encrypted in a way the viewer does not read".into())
            }
            LoadPdfError::Decryption(_) => ViewerError("This PDF's encryption is damaged".into()),
            LoadPdfError::Invalid => {
                ViewerError("This is not a PDF file the viewer can read".into())
            }
        })?;
        if pdf.pages().is_empty() {
            return Err(ViewerError("The PDF has no pages".into()));
        }
        let catalog = Catalog::new(&pdf);
        let units = catalog
            .labels(&pdf)
            .into_iter()
            .map(|label| Unit {
                kind: UnitKind::Page,
                label,
                duration_ms: None,
            })
            .collect();
        let structure = Structure {
            units,
            outline: catalog.outline(&pdf),
        };
        Ok(PdfDocument {
            pdf,
            catalog,
            structure,
            settings: InterpreterSettings::default(),
            name: file.name().to_string(),
            size,
            encrypted,
            texts: Mutex::new(HashMap::new()),
            renders: VecDeque::new(),
        })
    }

    /// The scale page `unit` is rendered at for `asked`: smaller where the
    /// page would pass [`MAX_SIDE`] or [`MAX_PIXELS`].
    fn bounded_scale(&self, unit: usize, asked: f32) -> Option<(f32, u16, u16)> {
        let page = self.pdf.pages().get(unit)?;
        let (w, h) = page.render_dimensions();
        let (w, h) = (w.max(1.0), h.max(1.0));
        let mut s = asked.max(0.01);
        if w.max(h) * s > MAX_SIDE {
            s = MAX_SIDE / w.max(h);
        }
        if w * h * s * s > MAX_PIXELS {
            // Below the budget by more than the rounding up of each side.
            s = (MAX_PIXELS / (w * h)).sqrt() * 0.999;
        }
        let px = |v: f32| (v * s - 0.01).ceil().clamp(1.0, u16::MAX as f32) as u16;
        Some((s, px(w), px(h)))
    }

    /// Extracts the texts of `units` not extracted yet, the fonts parsed
    /// once for all of them.
    fn extract(&self, units: &[usize]) {
        let mut texts = self.texts.lock().unwrap_or_else(|e| e.into_inner());
        let cache = InterpreterCache::new();
        let pages = self.pdf.pages();
        for &unit in units {
            if texts.contains_key(&unit) {
                continue;
            }
            if let Some(page) = pages.get(unit) {
                texts.insert(unit, text::page_text(page, &self.settings, &cache));
            }
        }
    }
}

impl ViewerDocument for PdfDocument {
    fn structure(&self) -> Structure {
        self.structure.clone()
    }

    fn size(&self, unit: usize) -> Option<(f32, f32)> {
        self.pdf.pages().get(unit).map(|p| p.render_dimensions())
    }

    fn render(&mut self, unit: usize, request: RenderRequest) -> Result<Rendered> {
        let key = (unit, request.scale.to_bits(), request.theme);
        if let Some(i) = self
            .renders
            .iter()
            .position(|(u, s, t, _)| (*u, *s, *t) == key)
        {
            let entry = self.renders.remove(i).expect("found");
            let bitmap = entry.3.clone();
            self.renders.push_back(entry);
            return Ok(Rendered::Bitmap(bitmap));
        }
        let (scale, width, height) = self
            .bounded_scale(unit, request.scale)
            .ok_or_else(|| ViewerError(format!("No page {}", unit + 1)))?;
        let page = &self.pdf.pages()[unit];
        let pixmap = hayro::render(
            page,
            &RenderCache::new(),
            &self.settings,
            &RenderSettings {
                x_scale: scale,
                y_scale: scale,
                width: Some(width),
                height: Some(height),
                bg_color: WHITE,
            },
        );
        // An opaque background: premultiplied and straight alpha agree.
        let mut rgba = pixmap.data_as_u8_slice().to_vec();
        if request.theme.dark {
            recolor(&mut rgba, request.theme);
        }
        let bitmap = Bitmap::new(width as u32, height as u32, rgba);
        if self.renders.len() >= CACHED_RENDERS {
            self.renders.pop_front();
        }
        self.renders
            .push_back((key.0, key.1, key.2, bitmap.clone()));
        Ok(Rendered::Bitmap(bitmap))
    }

    fn text(&self, unit: usize) -> String {
        self.extract(&[unit]);
        let texts = self.texts.lock().unwrap_or_else(|e| e.into_inner());
        texts.get(&unit).map(|t| t.text.clone()).unwrap_or_default()
    }

    fn text_rects(&self, unit: usize, range: std::ops::Range<usize>) -> Vec<[f32; 4]> {
        self.extract(&[unit]);
        let texts = self.texts.lock().unwrap_or_else(|e| e.into_inner());
        texts.get(&unit).map(|t| t.rects(range)).unwrap_or_default()
    }

    fn search(&self, query: &str) -> Vec<(usize, std::ops::Range<usize>)> {
        if query.is_empty() {
            return Vec::new();
        }
        let units: Vec<usize> = (0..self.structure.units.len()).collect();
        self.extract(&units);
        let query = query.to_lowercase();
        let texts = self.texts.lock().unwrap_or_else(|e| e.into_inner());
        let mut found = Vec::new();
        for unit in units {
            let Some(text) = texts.get(&unit).map(|t| &t.text) else {
                continue;
            };
            // The ranges are the text's: lower-casing keeps them only where
            // it keeps the length (`İ` does not), so such a page is
            // searched as it is.
            let lower = text.to_lowercase();
            let hay = if lower.len() == text.len() {
                lower.as_str()
            } else {
                text.as_str()
            };
            found.extend(
                hay.match_indices(query.as_str())
                    .map(|(i, m)| (unit, i..i + m.len())),
            );
        }
        found
    }

    fn links(&self, unit: usize) -> Vec<Link> {
        self.catalog.links(&self.pdf, unit)
    }

    fn info(&self) -> Vec<InfoField> {
        let meta = self.pdf.metadata();
        let mut fields = vec![InfoField::new(
            "Format",
            format!("PDF {}", version(self.pdf.version())),
        )];
        let text = |v: &Option<Vec<u8>>| {
            v.as_deref()
                .map(strings::decode)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        for (label, value) in [
            ("Title", &meta.title),
            ("Author", &meta.author),
            ("Subject", &meta.subject),
            ("Keywords", &meta.keywords),
        ] {
            if let Some(v) = text(value) {
                fields.push(InfoField::new(label, v));
            }
        }
        fields.push(InfoField::new(
            "Pages",
            self.structure.units.len().to_string(),
        ));
        if let Some(page) = self.pdf.pages().first() {
            let (w, h) = page.render_dimensions();
            fields.push(InfoField::new("Page size", page_size(w, h)));
        }
        fields.push(InfoField::new("File size", file_size(self.size)));
        for (label, value) in [("Creator", &meta.creator), ("Producer", &meta.producer)] {
            if let Some(v) = text(value) {
                fields.push(InfoField::new(label, v));
            }
        }
        for (label, date) in [
            ("Created", &meta.creation_date),
            ("Modified", &meta.modification_date),
        ] {
            if let Some(d) = date {
                let sign = if d.utc_offset_hour < 0 { '-' } else { '+' };
                fields.push(InfoField::new(
                    label,
                    format!(
                        "{:04}-{:02}-{:02} {:02}:{:02} {sign}{:02}:{:02}",
                        d.year,
                        d.month,
                        d.day,
                        d.hour,
                        d.minute,
                        d.utc_offset_hour.unsigned_abs(),
                        d.utc_offset_minute
                    ),
                ));
            }
        }
        if self.encrypted {
            fields.push(InfoField::new("Encrypted", "Yes"));
        }
        fields
    }
}

/// Whether the file's trailer names an encryption dictionary.
fn has_encrypt(bytes: &[u8]) -> bool {
    let tail = &bytes[bytes.len().saturating_sub(4096)..];
    tail.windows(8).any(|w| w == b"/Encrypt")
        || bytes.windows(8).take(4096).any(|w| w == b"/Encrypt")
}

fn version(v: PdfVersion) -> &'static str {
    match v {
        PdfVersion::Pdf10 => "1.0",
        PdfVersion::Pdf11 => "1.1",
        PdfVersion::Pdf12 => "1.2",
        PdfVersion::Pdf13 => "1.3",
        PdfVersion::Pdf14 => "1.4",
        PdfVersion::Pdf15 => "1.5",
        PdfVersion::Pdf16 => "1.6",
        PdfVersion::Pdf17 => "1.7",
        PdfVersion::Pdf20 => "2.0",
    }
}

/// A page's size in points and millimetres, with the paper's name when it
/// is a common one.
fn page_size(w: f32, h: f32) -> String {
    const PAPERS: [(&str, f32, f32); 6] = [
        ("A3", 842.0, 1191.0),
        ("A4", 595.0, 842.0),
        ("A5", 420.0, 595.0),
        ("Letter", 612.0, 792.0),
        ("Legal", 612.0, 1008.0),
        ("B5", 499.0, 709.0),
    ];
    let mm = |pt: f32| (pt * 25.4 / 72.0).round();
    let (short, long) = (w.min(h), w.max(h));
    let paper = PAPERS
        .iter()
        .find(|(_, a, b)| (short - a).abs() < 2.0 && (long - b).abs() < 2.0)
        .map(|(name, _, _)| {
            let turn = if w > h { ", landscape" } else { "" };
            format!(" ({name}{turn})")
        })
        .unwrap_or_default();
    format!(
        "{} × {} pt, {} × {} mm{paper}",
        w.round(),
        h.round(),
        mm(w),
        mm(h)
    )
}

fn file_size(n: u64) -> String {
    match n {
        0..1_000 => format!("{n} bytes"),
        1_000..1_000_000 => format!("{:.1} kB", n as f64 / 1e3),
        _ => format!("{:.1} MB", n as f64 / 1e6),
    }
}

/// Maps a page's lightness onto a dark theme, keeping its hues: white
/// paper becomes the theme's background and black ink its foreground,
/// while a colored figure keeps its colors' differences from gray.
fn recolor(rgba: &mut [u8], theme: Theme) {
    let bg = theme.background.map(f32::from);
    let fg = theme.foreground.map(f32::from);
    for px in rgba.as_chunks_mut::<4>().0 {
        let c = [px[0], px[1], px[2]].map(f32::from);
        let l = (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / 255.0;
        for i in 0..3 {
            let mapped = fg[i] + (bg[i] - fg[i]) * l;
            px[i] = (mapped + c[i] - l * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_paper() {
        let theme = Theme {
            dark: true,
            background: [30, 30, 30],
            foreground: [220, 220, 220],
        };
        let mut px = vec![255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 255];
        recolor(&mut px, theme);
        assert_eq!(&px[0..3], &[30, 30, 30]);
        assert_eq!(&px[4..7], &[220, 220, 220]);
        // Red stays redder than it is green.
        assert!(px[8] > px[9]);
    }

    #[test]
    fn sizes() {
        assert_eq!(page_size(595.0, 842.0), "595 × 842 pt, 210 × 297 mm (A4)");
        assert_eq!(
            page_size(792.0, 612.0),
            "792 × 612 pt, 279 × 216 mm (Letter, landscape)"
        );
        assert_eq!(file_size(2_500), "2.5 kB");
    }
}
