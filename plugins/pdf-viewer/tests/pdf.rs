//! The viewer on PDF files the tests write themselves, so no file's
//! license needs recording: pages with the base-14 Helvetica (not
//! embedded), page labels, an outline reaching its pages through each kind
//! of destination, links, a turned page, and malformed files.

use std::path::PathBuf;

use kalem_plugin_pdf_viewer::{MAX_SIDE, PdfViewer};
use kalem_viewer::{
    Detection, FileHandle, RenderRequest, Rendered, Theme, UnitKind, Viewer, ViewerDocument,
};

/// A PDF written object by object, with its cross-reference table.
struct Writer {
    objects: Vec<Vec<u8>>,
}

impl Writer {
    fn new() -> Writer {
        Writer {
            objects: Vec::new(),
        }
    }

    /// Adds an object and returns its number.
    fn add(&mut self, body: impl Into<Vec<u8>>) -> usize {
        self.objects.push(body.into());
        self.objects.len()
    }

    /// Sets object `n`, added before as a placeholder.
    fn set(&mut self, n: usize, body: impl Into<Vec<u8>>) {
        self.objects[n - 1] = body.into();
    }

    fn stream(&mut self, content: &str) -> usize {
        self.add(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ))
    }

    fn finish(&self, root: usize, info: Option<usize>) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", self.objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        let info = info.map(|i| format!(" /Info {i} 0 R")).unwrap_or_default();
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root {root} 0 R{info} >>\nstartxref\n{xref}\n%%EOF\n",
                self.objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}

/// A text string in UTF-16BE with its byte order mark, as hex.
fn utf16(s: &str) -> String {
    let mut hex = String::from("<FEFF");
    for u in s.encode_utf16() {
        hex.push_str(&format!("{u:04X}"));
    }
    hex.push('>');
    hex
}

fn text_page(text: &str, x: u32, y: u32) -> String {
    format!("BT /F1 24 Tf {x} {y} Td ({text}) Tj ET")
}

/// Three pages: "Hello World" and "Second line" on page 1 with two links,
/// "Second page" on page 2, a landscape page turned by `Rotate` on page 3;
/// labels i, ii, A-5; an outline; a title in UTF-16.
fn sample() -> Vec<u8> {
    let mut w = Writer::new();
    let catalog = w.add("");
    let pages = w.add("");
    let font = w.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let c1 = w.stream(&format!(
        "{} {}",
        text_page("Hello World", 72, 700),
        text_page("Second line", 72, 660)
    ));
    let c2 = w.stream(&text_page("Second page", 72, 700));
    let c3 = w.stream(&text_page("Turned", 72, 300));
    let p1 = w.add("");
    let p2 = w.add("");
    let p3 = w.add("");
    let page = |content: usize, extra: &str| {
        format!(
            "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 612 792] /Contents {content} 0 R /Resources << /Font << /F1 {font} 0 R >> >>{extra} >>"
        )
    };
    let uri = w.add("<< /Type /Annot /Subtype /Link /Rect [72 690 200 720] /A << /S /URI /URI (https://example.org/) >> >>");
    let goto = w.add(format!(
        "<< /Type /Annot /Subtype /Link /Rect [72 650 200 680] /Dest [{p3} 0 R /Fit] >>"
    ));
    w.set(p1, page(c1, &format!(" /Annots [{uri} 0 R {goto} 0 R]")));
    w.set(p2, page(c2, ""));
    w.set(
        p3,
        format!(
            "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 792 612] /Rotate 90 /Contents {c3} 0 R /Resources << /Font << /F1 {font} 0 R >> >> >>"
        ),
    );
    w.set(
        pages,
        format!("<< /Type /Pages /Kids [{p1} 0 R {p2} 0 R {p3} 0 R] /Count 3 >>"),
    );
    // The outline: an explicit destination, a named one in the name tree
    // (a string), and a GoTo action to one in the catalog's Dests (a name).
    let outlines = w.add("");
    let ch1 = w.add("");
    let sec = w.add("");
    let ch2 = w.add("");
    w.set(
        ch1,
        format!(
            "<< /Title (Chapter 1) /Parent {outlines} 0 R /Dest [{p1} 0 R /XYZ 0 792 0] /First {sec} 0 R /Last {sec} 0 R /Count 1 /Next {ch2} 0 R >>"
        ),
    );
    w.set(
        sec,
        format!("<< /Title (\\204 Section) /Parent {ch1} 0 R /Dest (sec) >>"),
    );
    w.set(
        ch2,
        format!(
            "<< /Title {} /Parent {outlines} 0 R /Prev {ch1} 0 R /A << /S /GoTo /D /ch2 >> >>",
            utf16("Bölüm 2")
        ),
    );
    w.set(
        outlines,
        format!("<< /Type /Outlines /First {ch1} 0 R /Last {ch2} 0 R /Count 3 >>"),
    );
    let names = w.add(format!("<< /Dests << /Names [(sec) [{p2} 0 R /Fit]] >> >>"));
    let labels = w.add("<< /Nums [0 << /S /r >> 2 << /S /D /P (A-) /St 5 >>] >>");
    w.set(
        catalog,
        format!(
            "<< /Type /Catalog /Pages {pages} 0 R /Outlines {outlines} 0 R /Names {names} 0 R /Dests << /ch2 [{p3} 0 R /Fit] >> /PageLabels {labels} 0 R >>"
        ),
    );
    let info = w.add(format!(
        "<< /Title {} /Author (Kalem) /CreationDate (D:20261003120000+03'00') >>",
        utf16("Şiir")
    ));
    w.finish(catalog, Some(info))
}

/// Writes `bytes` to a file of the test's own and opens it.
fn open(name: &str, bytes: &[u8]) -> Result<Box<dyn ViewerDocument>, String> {
    let dir = std::env::temp_dir().join(format!("kalem-pdf-viewer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    PdfViewer
        .open(FileHandle::new(&path))
        .map_err(|e| e.to_string())
}

fn bitmap(
    doc: &mut dyn ViewerDocument,
    unit: usize,
    request: RenderRequest,
) -> kalem_viewer::Bitmap {
    let Rendered::Bitmap(b) = doc.render(unit, request).expect("renders");
    b
}

#[test]
fn detection() {
    let v = PdfViewer;
    assert_eq!(v.detect("a.bin", b"%PDF-1.7\n"), Detection::Magic);
    assert_eq!(
        v.detect("a.bin", b"junk before it %PDF-1.4"),
        Detection::Magic
    );
    assert_eq!(
        v.detect("Report.PDF", b"not yet written"),
        Detection::Extension
    );
    assert_eq!(v.detect("notes.txt", b"hello"), Detection::No);
}

#[test]
fn pages_labels_and_outline() {
    let doc = open("sample.pdf", &sample()).unwrap();
    let s = doc.structure();
    let labels: Vec<&str> = s.units.iter().map(|u| u.label.as_str()).collect();
    assert_eq!(labels, ["i", "ii", "A-5"]);
    assert!(s.units.iter().all(|u| u.kind == UnitKind::Page));
    let outline: Vec<(&str, usize, u8)> = s
        .outline
        .iter()
        .map(|e| (e.title.as_str(), e.unit, e.level))
        .collect();
    assert_eq!(
        outline,
        [
            ("Chapter 1", 0, 1),
            ("\u{2014} Section", 1, 2),
            ("Bölüm 2", 2, 1)
        ]
    );
}

#[test]
fn text_search_and_links() {
    let doc = open("text.pdf", &sample()).unwrap();
    assert_eq!(doc.text(0), "Hello World\nSecond line");
    assert_eq!(doc.text(1), "Second page");
    assert_eq!(doc.text(2), "Turned");
    let hits = doc.search("SECOND");
    assert_eq!(hits, [(0, 12..18), (1, 0..6)]);
    assert_eq!(&doc.text(0)[12..18], "Second");

    let links = doc.links(0);
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].target, "https://example.org/");
    assert_eq!(links[1].target, "#2");
    // In the page's pixels, y down: [72 690 200 720] on a 792 pt page.
    let r = links[0].rect;
    assert!(
        (r[0] - 72.0).abs() < 0.01 && (r[1] - 72.0).abs() < 0.01,
        "{r:?}"
    );
    assert!(
        (r[2] - 128.0).abs() < 0.01 && (r[3] - 30.0).abs() < 0.01,
        "{r:?}"
    );
}

#[test]
fn information() {
    let doc = open("info.pdf", &sample()).unwrap();
    let info: Vec<(String, String)> = doc.info().into_iter().map(|f| (f.label, f.value)).collect();
    let get = |l: &str| info.iter().find(|(k, _)| k == l).map(|(_, v)| v.as_str());
    assert_eq!(get("Format"), Some("PDF 1.7"));
    assert_eq!(get("Title"), Some("Şiir"));
    assert_eq!(get("Author"), Some("Kalem"));
    assert_eq!(get("Pages"), Some("3"));
    assert_eq!(
        get("Page size"),
        Some("612 × 792 pt, 216 × 279 mm (Letter)")
    );
    assert_eq!(get("Created"), Some("2026-10-03 12:00 +03:00"));
    assert_eq!(get("Encrypted"), None);
}

#[test]
fn rendering_follows_scale_rotation_and_theme() {
    let mut doc = open("render.pdf", &sample()).unwrap();
    let b = bitmap(doc.as_mut(), 0, RenderRequest::default());
    assert_eq!((b.width, b.height), (612, 792));
    // White paper and black text: the text's row has dark pixels.
    let px = |b: &kalem_viewer::Bitmap, x: u32, y: u32| {
        let i = ((y * b.width + x) * 4) as usize;
        [b.rgba[i], b.rgba[i + 1], b.rgba[i + 2]]
    };
    assert_eq!(px(&b, 5, 5), [255, 255, 255]);
    let row = 792 - 700 - 8;
    assert!((72..300).any(|x| px(&b, x, row)[0] < 64), "text is drawn");

    let b2 = bitmap(
        doc.as_mut(),
        0,
        RenderRequest {
            scale: 2.0,
            ..Default::default()
        },
    );
    assert_eq!((b2.width, b2.height), (1224, 1584));

    // The turned page is as wide as its MediaBox is high.
    let b3 = bitmap(doc.as_mut(), 2, RenderRequest::default());
    assert_eq!((b3.width, b3.height), (612, 792));

    let dark = Theme {
        dark: true,
        background: [20, 20, 20],
        foreground: [230, 230, 230],
    };
    let d = bitmap(
        doc.as_mut(),
        0,
        RenderRequest {
            scale: 1.0,
            theme: dark,
        },
    );
    assert_eq!(px(&d, 5, 5), [20, 20, 20]);
}

#[test]
fn a_huge_page_stays_in_the_budget() {
    let mut w = Writer::new();
    let catalog = w.add("");
    let pages = w.add("");
    let content = w.stream("0 0 1 rg 0 0 14400 14400 re f");
    let page = w.add(format!(
        "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 14400 14400] /Contents {content} 0 R >>"
    ));
    w.set(
        pages,
        format!("<< /Type /Pages /Kids [{page} 0 R] /Count 1 >>"),
    );
    w.set(catalog, format!("<< /Type /Catalog /Pages {pages} 0 R >>"));
    let mut doc = open("huge.pdf", &w.finish(catalog, None)).unwrap();
    let b = bitmap(
        doc.as_mut(),
        0,
        RenderRequest {
            scale: 4.0,
            ..Default::default()
        },
    );
    assert!(b.width as f32 <= MAX_SIDE && b.height as f32 <= MAX_SIDE);
    assert!((b.width as u64) * (b.height as u64) <= 40_000_000);
    assert_eq!(&b.rgba[..4], &[0, 0, 255, 255]);
}

#[test]
fn an_outline_with_a_cycle_ends() {
    let mut w = Writer::new();
    let catalog = w.add("");
    let pages = w.add("");
    let page = w.add(format!(
        "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 100 100] >>"
    ));
    w.set(
        pages,
        format!("<< /Type /Pages /Kids [{page} 0 R] /Count 1 >>"),
    );
    let outlines = w.add("");
    let a = w.add("");
    let b = w.add("");
    // a's next is b, b's next is a, and b is its own first child.
    w.set(
        a,
        format!("<< /Title (A) /Next {b} 0 R /Dest [{page} 0 R /Fit] >>"),
    );
    w.set(b, format!("<< /Title (B) /Next {a} 0 R /First {b} 0 R >>"));
    w.set(outlines, format!("<< /First {a} 0 R >>"));
    w.set(
        catalog,
        format!("<< /Type /Catalog /Pages {pages} 0 R /Outlines {outlines} 0 R >>"),
    );
    let doc = open("cycle.pdf", &w.finish(catalog, None)).unwrap();
    let titles: Vec<String> = doc
        .structure()
        .outline
        .into_iter()
        .map(|e| e.title)
        .collect();
    assert_eq!(titles, ["A", "B"]);
    assert_eq!(doc.text(0), "");
}

#[test]
fn malformed_files_fail_without_panicking() {
    assert!(open("empty.pdf", b"").is_err());
    assert!(open("junk.pdf", b"%PDF-1.7\nthis is not a PDF at all").is_err());
    let full = sample();
    // Cut anywhere: an error or a document, never a panic.
    for cut in (0..full.len()).step_by(97) {
        if let Ok(mut doc) = open("cut.pdf", &full[..cut]) {
            let n = doc.structure().units.len();
            for u in 0..n {
                let _ = doc.render(u, RenderRequest::default());
                let _ = doc.text(u);
                let _ = doc.links(u);
            }
        }
    }
}
