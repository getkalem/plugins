//! Lines drawn as shapes, as documents draw separators: a paragraph
//! holding nothing but them is a rule; one among text shows as a line,
//! not as "[Shape]"; another shape stays a placeholder.

use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_plugin_docx::flow::{self, LINE_SHAPE, Piece, VBlock};
use kalem_plugin_docx::{Document, DocxViewer};
use kalem_viewer::{FileHandle, FlowItem, Viewer};

fn corpus(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// A floating DrawingML shape of `prst`, `cx` by `cy` EMU.
fn shape(prst: &str, cx: i64, cy: i64) -> String {
    format!(
        r#"<w:r><w:drawing><wp:anchor><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="9" name="Shape"/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:spPr><a:prstGeom prst="{prst}"><a:avLst/></a:prstGeom></wps:spPr><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#
    )
}

/// The handmade document with paragraphs of shapes before "Plain, bold":
/// two lines (a line, and a bar 1 pt thick across the page), a
/// horizontal line of VML beside text, and an ellipse.
fn with_shapes() -> Vec<u8> {
    let bytes = corpus("handmade-features.docx");
    let mut pkg = Package::read(bytes.clone()).unwrap();
    let doc = String::from_utf8(pkg.part("word/document.xml").unwrap()).unwrap();
    let at = doc
        .find("<w:p><w:r><w:t xml:space=\"preserve\">Plain, </w:t>")
        .unwrap();
    let paragraphs = format!(
        r##"<w:p>{}{}</w:p><w:p><w:r><w:t>Above</w:t></w:r><w:r><w:pict><v:rect style="width:0;height:1.5pt" o:hr="t" o:hrstd="t" fillcolor="#a0a0a0" stroked="f"/></w:pict></w:r></w:p><w:p>{}</w:p>"##,
        shape("line", 5_943_600, 0),
        shape("rect", 5_943_600, 12_700),
        shape("ellipse", 914_400, 914_400),
    );
    let doc = format!("{}{paragraphs}{}", &doc[..at], &doc[at..]);
    pkg.set_part("word/document.xml", doc.into_bytes());
    pkg.write().unwrap()
}

#[test]
fn lines_drawn_as_shapes_are_rules() {
    let input = with_shapes();
    let d = Document::open(input.clone()).unwrap();
    let body = d.view().body;
    let i = body
        .iter()
        .position(|b| matches!(b, VBlock::Rule))
        .expect("a rule");
    // The paragraph of two lines: a rule; the next one has its text and
    // a line; the ellipse stays a shape.
    let VBlock::Para(text) = &body[i + 1] else {
        panic!("{:?}", body[i + 1])
    };
    assert_eq!(flow::para_text(text), format!("Above{LINE_SHAPE}"));
    assert!(
        text.runs
            .iter()
            .any(|r| matches!(&r.piece, Piece::Placeholder(t) if t == LINE_SHAPE))
    );
    let VBlock::Para(ellipse) = &body[i + 2] else {
        panic!("{:?}", body[i + 2])
    };
    assert_eq!(flow::para_text(ellipse), "[Shape]");
    // In Kalem's flow: a rule of a line.
    let dir = std::env::temp_dir().join(format!("kalem-docx-shapes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.docx");
    std::fs::write(&file, &input).unwrap();
    let mut doc = DocxViewer.open(FileHandle::new(file)).unwrap();
    let l = doc.flow(0).unwrap();
    let items = doc.flow_items(0, 0, l.items);
    assert!(items.iter().any(|it| *it == FlowItem::Rule("line".into())));
    let _ = std::fs::remove_dir_all(&dir);
    // Nothing of the file changes for being shown so.
    let mut d = d;
    assert_eq!(d.save().unwrap(), input);
}
