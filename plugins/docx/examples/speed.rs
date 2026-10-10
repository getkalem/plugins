//! The plugin's speed on a large document, as Kalem uses it (the docx
//! list's WP14a): a document of N paragraphs (headings, list items and
//! paragraphs of three runs, about a page of A4 every 20) made from the
//! corpus' handmade one, then opened, its flow fetched whole, characters
//! typed, Enter pressed, a word made bold and the file saved, each change
//! followed by the items that changed fetched, as Kalem's host does
//! (plugin API 0.2.9).
//!
//! ```sh
//! cargo run --release -p kalem-plugin-docx --example speed -- 20000 [large.docx]
//! ```

#![allow(clippy::print_stdout)]

use std::time::Instant;

use kalem_ooxml::package::Package;
use kalem_plugin_docx::DocxViewer;
use kalem_viewer::{FileHandle, FlowItem, FlowPlace, MarkChange, Viewer, ViewerDocument};

/// The handmade document with `n` paragraphs of its own kinds in its
/// body, before its section's properties.
fn large(n: usize) -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/corpus/handmade-features.docx"
    );
    let bytes = std::fs::read(path).unwrap();
    let mut pkg = Package::read(bytes).unwrap();
    let doc = String::from_utf8(pkg.part("word/document.xml").unwrap()).unwrap();
    let body = doc.find("<w:body>").unwrap() + "<w:body>".len();
    let sect = doc.rfind("<w:sectPr").unwrap();
    let mut out = String::with_capacity(n * 260);
    for i in 0..n {
        if i % 50 == 0 {
            out.push_str(&format!(
                r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Chapter {i}</w:t></w:r></w:p>"#
            ));
        } else if i % 10 < 3 {
            out.push_str(&format!(
                r#"<w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Item {i} of a list</w:t></w:r></w:p>"#
            ));
        } else {
            out.push_str(&format!(
                r#"<w:p><w:r><w:t xml:space="preserve">Paragraph {i} begins plainly, </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>goes on in bold</w:t></w:r><w:r><w:t xml:space="preserve"> and ends with words enough to fill a line or two of a page.</w:t></w:r></w:p>"#
            ));
        }
    }
    let doc = format!("{}{out}{}", &doc[..body], &doc[sect..]);
    pkg.set_part("word/document.xml", doc.into_bytes());
    pkg.write().unwrap()
}

/// What changed since `version`, fetched as Kalem's host does (plugin API
/// 0.2.9): the items changed; how many, and the version now.
fn changes(d: &mut Box<dyn ViewerDocument>, version: u64) -> (usize, u64) {
    let l = d.flow(0).unwrap();
    let c = d.flow_changes(0, version).expect("told");
    (d.flow_items(0, c.from, c.added).len(), l.version)
}

/// The whole flow, as Kalem's host fetches it: its items, 2,048 at a
/// time.
fn fetch(d: &mut Box<dyn ViewerDocument>) -> Vec<FlowItem> {
    let l = d.flow(0).unwrap();
    let mut items = Vec::with_capacity(l.items as usize);
    let mut from = 0;
    while from < l.items {
        let chunk = d.flow_items(0, from, 2048);
        if chunk.is_empty() {
            break;
        }
        from += chunk.len() as u32;
        items.extend(chunk);
    }
    items
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(20_000);
    let bytes = large(n);
    // A second argument: the document written there too (for measuring
    // Kalem with it).
    if let Some(out) = std::env::args().nth(2) {
        std::fs::write(out, &bytes).unwrap();
    }
    let dir = std::env::temp_dir().join(format!("kalem-docx-speed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("large.docx");
    std::fs::write(&file, &bytes).unwrap();
    println!("{n} paragraphs, {} kB of file", bytes.len() / 1024);
    // Where a whole read goes: the XML parsed, the view resolved, Kalem's
    // flow built from it.
    let doc = kalem_plugin_docx::Document::open(bytes.clone()).unwrap();
    let xml = doc.part_text("word/document.xml").unwrap().to_owned();
    let t = Instant::now();
    let tree = kalem_plugin_docx::story::PartTree::parse(&xml);
    println!("  parsed {:?} ({} blocks)", t.elapsed(), tree.blocks.len());
    let t = Instant::now();
    let view = doc.view();
    println!("  viewed {:?}", t.elapsed());
    let t = Instant::now();
    let cache = kalem_plugin_docx::contract::build(&view);
    println!("  built {:?} ({} items)", t.elapsed(), cache.items.len());
    let t = Instant::now();
    let mut d = DocxViewer.open(FileHandle::new(file.clone())).unwrap();
    let opened = t.elapsed();
    let t = Instant::now();
    let items = fetch(&mut d);
    println!(
        "open {opened:?}, the flow ({} items) {:?}",
        items.len(),
        t.elapsed()
    );
    // A paragraph in the middle: its index.
    let middle = items
        .iter()
        .filter_map(|i| match i {
            FlowItem::Paragraph(p) => p.index,
            _ => None,
        })
        .nth(n / 2)
        .unwrap();
    let mut version = d.flow(0).unwrap().version;
    for _ in 0..3 {
        let t = Instant::now();
        d.flow_replace(0, middle, 0..0, "x").unwrap();
        let typed = t.elapsed();
        let t = Instant::now();
        let (n, v) = changes(&mut d, version);
        version = v;
        println!(
            "a character typed {typed:?}, what changed ({n} items) {:?}",
            t.elapsed()
        );
    }
    let t = Instant::now();
    d.flow_split(
        0,
        FlowPlace {
            paragraph: middle,
            offset: 4,
        },
    )
    .unwrap();
    let split = t.elapsed();
    let t = Instant::now();
    let (n, v) = changes(&mut d, version);
    version = v;
    println!(
        "Enter {split:?}, what changed ({n} items) {:?}",
        t.elapsed()
    );
    let t = Instant::now();
    let at = |offset| FlowPlace {
        paragraph: middle,
        offset,
    };
    d.flow_set_marks(0, at(1), at(3), &[MarkChange::Bold(true)])
        .unwrap();
    let bold = t.elapsed();
    let t = Instant::now();
    let (n, _) = changes(&mut d, version);
    println!(
        "a word made bold {bold:?}, what changed ({n} items) {:?}",
        t.elapsed()
    );
    let t = Instant::now();
    fetch(&mut d);
    println!("the whole flow again {:?}", t.elapsed());
    let t = Instant::now();
    d.save().unwrap();
    println!("saved {:?}", t.elapsed());
    let _ = std::fs::remove_dir_all(&dir);
}
