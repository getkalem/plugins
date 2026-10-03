//! Opens a PDF as Kalem does and prints what the viewer sees of it.
//!
//! `cargo run --release --example pdf -- FILE [--text] [--page N] [--scale S] [--ppm OUT]`
//!
//! `--text` prints every page's text, as `pdftotext` would be compared
//! with; `--ppm` writes page N (from 1) rendered at scale S (1 is 72 dpi)
//! as a binary PPM, as `pdftoppm` writes it.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::time::Instant;

use kalem_plugin_pdf_viewer::PdfViewer;
use kalem_viewer::{FileHandle, RenderRequest, Rendered, Viewer};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: pdf FILE [--text] [--page N] [--scale S] [--ppm OUT]");
        std::process::exit(2);
    };
    let opt = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let page: usize = opt("--page").and_then(|p| p.parse().ok()).unwrap_or(1);
    let scale: f32 = opt("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);

    let t = Instant::now();
    let mut doc = match PdfViewer.open(FileHandle::new(path)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    };
    let structure = doc.structure();
    println!("opened in {:?}", t.elapsed());
    for f in doc.info() {
        println!("{}: {}", f.label, f.value);
    }
    let labels: Vec<&str> = structure.units.iter().map(|u| u.label.as_str()).collect();
    println!(
        "labels: {} … {}",
        labels[..labels.len().min(8)].join(" "),
        labels.last().unwrap_or(&"")
    );
    println!("outline: {} entries", structure.outline.len());
    for e in structure.outline.iter().take(12) {
        println!(
            "  {}{} → {}",
            "  ".repeat(e.level as usize - 1),
            e.title,
            e.unit + 1
        );
    }
    let links: usize = (0..structure.units.len()).map(|u| doc.links(u).len()).sum();
    println!("links: {links}");

    let t = Instant::now();
    let unit = page.saturating_sub(1);
    let Rendered::Bitmap(b) = doc
        .render(
            unit,
            RenderRequest {
                scale,
                ..Default::default()
            },
        )
        .expect("render");
    println!(
        "page {page} rendered {}x{} in {:?}",
        b.width,
        b.height,
        t.elapsed()
    );
    if let Some(out) = opt("--ppm") {
        let mut ppm = format!("P6\n{} {}\n255\n", b.width, b.height).into_bytes();
        for px in b.rgba.as_chunks::<4>().0 {
            ppm.extend_from_slice(&px[..3]);
        }
        std::fs::write(&out, ppm).expect("write");
    }
    if args.iter().any(|a| a == "--text") {
        let t = Instant::now();
        let hits = doc.search("the");
        eprintln!("search: {} hits in {:?}", hits.len(), t.elapsed());
        let t = Instant::now();
        for u in 0..structure.units.len() {
            print!("{}\u{c}", doc.text(u));
        }
        eprintln!("text of every page in {:?}", t.elapsed());
    }
}
