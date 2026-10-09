//! Documents of the corpus opened, shown, edited and saved.
//!
//! The rules under test are those of Kalem's task T3.7.5: a save without
//! edits is byte-identical; after an edit only `word/document.xml`
//! differs, and in it only the runs the edit touched; every other entry
//! of the package is copied byte for byte; undoing everything saves the
//! input again.

use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_plugin_docx::flow::{self, Piece, VBlock, VPara, Vert};
use kalem_plugin_docx::{Document, Kind, ParaAt, StoryId};

fn corpus(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

const FILES: [&str; 4] = [
    "handmade-features.docx",
    "libreoffice-features.docx",
    "python-docx-basic.docx",
    "libreoffice-basic.docx",
];

fn body(i: usize) -> ParaAt {
    ParaAt {
        story: StoryId::Body,
        index: i,
    }
}

/// The parts whose inflated bytes differ between two packages, and the
/// parts present in one only.
fn part_diff(a: &[u8], b: &[u8]) -> Vec<String> {
    let (pa, pb) = (
        Package::read(a.to_vec()).unwrap(),
        Package::read(b.to_vec()).unwrap(),
    );
    let mut out = Vec::new();
    for n in pa.names() {
        if !pb.contains(&n) {
            out.push(format!("-{n}"));
        } else if pa.part(&n).unwrap() != pb.part(&n).unwrap() {
            out.push(n);
        }
    }
    for n in pb.names() {
        if !pa.contains(&n) {
            out.push(format!("+{n}"));
        }
    }
    out.sort();
    out
}

/// The `w:r` elements of a part, as written.
fn runs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(p) = rest.find("<w:r>").or_else(|| rest.find("<w:r ")) {
        let from = &rest[p..];
        let end = from.find("</w:r>").map_or(from.len(), |e| e + 6);
        out.push(from[..end].to_owned());
        rest = &from[end..];
    }
    out
}

fn document_xml(bytes: &[u8]) -> String {
    String::from_utf8(
        Package::read(bytes.to_vec())
            .unwrap()
            .part("word/document.xml")
            .unwrap(),
    )
    .unwrap()
}

/// The runs that differ between two saves, after checking that
/// everything outside the edited paragraph is the same.
fn changed_runs(a: &[u8], b: &[u8]) -> (Vec<String>, Vec<String>) {
    let (ra, rb) = (runs(&document_xml(a)), runs(&document_xml(b)));
    let only_a = ra.iter().filter(|r| !rb.contains(r)).cloned().collect();
    let only_b = rb.iter().filter(|r| !ra.contains(r)).cloned().collect();
    (only_a, only_b)
}

fn paras(doc: &Document) -> Vec<VPara> {
    doc.view().body_paragraphs().into_iter().cloned().collect()
}

fn find<'a>(ps: &'a [VPara], text: &str) -> &'a VPara {
    ps.iter()
        .find(|p| flow::para_text(p).contains(text))
        .unwrap_or_else(|| panic!("no paragraph with {text:?}"))
}

#[test]
fn every_file_round_trips_byte_for_byte() {
    for f in FILES {
        let bytes = corpus(f);
        let mut doc = Document::open(bytes.clone()).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!(doc.kind(), Kind::Document);
        assert!(!doc.is_dirty());
        let _ = doc.view();
        assert_eq!(doc.save().unwrap(), bytes, "{f}");
    }
}

#[test]
fn styles_lists_and_toggles_shown() {
    for f in ["handmade-features.docx", "libreoffice-features.docx"] {
        let doc = Document::open(corpus(f)).unwrap();
        let ps = paras(&doc);
        let title = find(&ps, "Kalem features");
        assert_eq!(title.style, "Title", "{f}");
        assert!((title.runs[0].look.size - 28.0).abs() < 0.01, "{f}");
        assert_eq!(&*title.runs[0].look.face, "Calibri Light", "{f}");
        let h1 = find(&ps, "Introduction");
        assert_eq!(
            (h1.style.as_str(), h1.outline),
            ("Heading 1", Some(0)),
            "{f}"
        );
        assert_eq!(h1.runs[0].look.color, Some(0x2F5496), "{f}");
        // Heading 2 is based on Heading 1: its color, its own size.
        let h2 = find(&ps, "Styles");
        assert_eq!(h2.outline, Some(1));
        assert_eq!(h2.runs[0].look.color, Some(0x2F5496), "{f}");
        assert!((h2.runs[0].look.size - 13.0).abs() < 0.01, "{f}");
        // Emphasis inside an italic quote is upright.
        let quote = find(&ps, "A quote with");
        let emph = quote.runs.iter().find(|r| r.text == "emphasis").unwrap();
        assert!(quote.runs[0].look.italic && !emph.look.italic, "{f}");
        assert_eq!(quote.runs[0].look.color, Some(0x404040), "{f}");
        let labels: Vec<String> = ps
            .iter()
            .filter_map(|p| p.label.as_ref().map(|l| l.0.clone()))
            .collect();
        assert_eq!(
            labels,
            [
                "1.", "1.1.", "1.2.", "1.2.1.", "2.", "•", "o", "▪", "1.", "a)", "i.", "ii.", "b)"
            ],
            "{f}"
        );
        let normal = find(&ps, "Plain, ");
        assert!((normal.runs[0].look.size - 11.0).abs() < 0.01);
        assert_eq!(&*normal.runs[0].look.face, "Calibri");
        let bold = normal.runs.iter().find(|r| r.text == "bold").unwrap();
        assert!(bold.look.bold);
        let note = normal
            .runs
            .iter()
            .find(|r| matches!(r.piece, Piece::FootnoteRef(_)))
            .unwrap();
        assert_eq!(
            (note.text.as_str(), note.look.vert),
            ("1", Vert::Super),
            "{f}"
        );
        let end = normal
            .runs
            .iter()
            .find(|r| matches!(r.piece, Piece::EndnoteRef(_)))
            .unwrap();
        assert_eq!(end.text, "i", "{f}");
    }
}

#[test]
fn changes_fields_links_and_notes_shown() {
    let doc = Document::open(corpus("handmade-features.docx")).unwrap();
    let v = doc.view();
    let ps: Vec<VPara> = v.body_paragraphs().into_iter().cloned().collect();
    let changed = find(&ps, "Changed:");
    let ins = changed.runs.iter().find(|r| r.text == "inserted").unwrap();
    assert_eq!(ins.inserted.as_ref().unwrap().author, "Ayşe Yılmaz");
    let del = changed.runs.iter().find(|r| r.text == "deleted").unwrap();
    assert!(del.deleted.is_some());
    let bold = changed.runs.iter().find(|r| r.text == "made bold").unwrap();
    assert!(bold.look.bold && bold.format_changed.is_some());
    // Deleted text is not part of the text read.
    assert_eq!(flow::para_text(changed), "Changed: inserted  made bold");
    let fields = find(&ps, "Date:");
    assert_eq!(
        flow::para_text(fields),
        "Date: 2026-10-09; see Introduction; Kalem and the start."
    );
    let date = fields.runs.iter().find(|r| r.text == "2026-10-09").unwrap();
    assert!(date.field.as_deref().unwrap().contains("DATE"));
    let link = fields.runs.iter().find(|r| r.text == "Kalem").unwrap();
    assert_eq!(
        link.link.as_deref(),
        Some("https://github.com/getkalem/kalem")
    );
    let anchor = fields.runs.iter().find(|r| r.text == "the start").unwrap();
    assert_eq!(anchor.link.as_deref(), Some("#intro"));
    let commented = find(&ps, "Commented text");
    assert_eq!(commented.runs[0].comments, ["0"]);
    assert_eq!(v.comments[0].author, "Ayşe Yılmaz");
    assert_eq!(v.footnotes.len(), 1);
    assert_eq!(v.endnotes[0].mark, "i");
    let text = v.text();
    assert!(text.starts_with("Kalem test header\n"));
    assert!(text.contains("Page 1\n"));
    assert!(text.contains("[1] A footnote, with italics.\n[i] An endnote, with italics."));
    let chars = find(&ps, "Tab:");
    assert_eq!(
        flow::para_text(chars),
        "Tab:\tafter\nnext line e\u{2011}mail; ✓ softhyphen"
    );
    assert_eq!(flow::para_text(find(&ps, "Done")), "☒ Done");
    // Hidden text is left out, capitals shown as such.
    assert_eq!(
        flow::para_text(find(&ps, "Theme colored")),
        "Theme colored, shown, CAPS, Cambria and on green"
    );
    let outline: Vec<(String, u8)> = v.outline();
    assert_eq!(outline[0], ("Introduction".into(), 1));
    assert!(outline.contains(&("Lists".into(), 2)));
}

#[test]
fn tables_and_drawings_shown() {
    let doc = Document::open(corpus("handmade-features.docx")).unwrap();
    let v = doc.view();
    let table = v
        .body
        .iter()
        .find_map(|b| match b {
            VBlock::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.style.as_deref(), Some("Kalem Table"));
    assert_eq!(table.grid, [150.0, 150.0, 150.0]);
    let header = &table.rows[0];
    assert!(header.header);
    assert_eq!(header.cells[1].fill, Some(0x4472C4));
    let VBlock::Para(q1) = &header.cells[1].blocks[0] else {
        panic!()
    };
    assert!(q1.runs[0].look.bold);
    assert_eq!(q1.runs[0].look.color, Some(0xFFFFFF));
    // Banded rows: the first data row filled, the next one not.
    let band = table.rows[1].cells[1].fill.unwrap();
    let near = |a: u32, b: u32| (a as i32 - b as i32).abs() <= 1;
    assert!(near(band >> 16, 0xD9) && near((band >> 8) & 0xFF, 0xE2) && near(band & 0xFF, 0xF3));
    assert_eq!(table.rows[2].cells[1].fill, None);
    assert_eq!(table.rows[1].cells[1].columns, 2);
    // The first column is italic, a cell of the header row both.
    let VBlock::Para(item) = &header.cells[0].blocks[0] else {
        panic!()
    };
    assert!(item.runs[0].look.bold && item.runs[0].look.italic);
    let mut cell_text = String::new();
    flow::text(&table.rows[3].cells[2].blocks, &mut cell_text);
    assert_eq!(cell_text.trim(), "nested");
    let ps: Vec<VPara> = v.body_paragraphs().into_iter().cloned().collect();
    let pic = find(&ps, "A picture.");
    let Piece::Picture {
        target,
        size,
        alt,
        floating,
    } = &pic.runs[0].piece
    else {
        panic!("{:?}", pic.runs[0].piece)
    };
    assert_eq!(target.as_deref(), Some("word/media/image1.png"));
    assert_eq!(
        (*size, alt.as_str(), *floating),
        ((24.0, 24.0), "A red square", false)
    );
    assert!(
        doc.part_bytes("word/media/image1.png")
            .unwrap()
            .starts_with(b"\x89PNG")
    );
    // The text box's paragraph is shown after its anchor's, not edited.
    let boxed = find(&ps, "Inside a text box");
    assert!(boxed.at.is_none());
    assert_eq!(
        flow::para_text(find(&ps, "beside the box")),
        "[Text box]Text beside the box."
    );
}

#[test]
fn typing_rewrites_one_run() {
    let bytes = corpus("handmade-features.docx");
    let mut doc = Document::open(bytes.clone()).unwrap();
    // After "bold": typed into the bold run.
    let at = body(2);
    let l = doc.layout(&at).unwrap();
    let off = l.text.find("bold").unwrap() + 4;
    doc.replace(&at, off..off, " and strong").unwrap();
    assert!(doc.is_dirty());
    let out = doc.save().unwrap();
    assert_eq!(part_diff(&bytes, &out), ["word/document.xml"]);
    let (gone, new) = changed_runs(&bytes, &out);
    assert_eq!(gone, [r#"<w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>"#]);
    assert_eq!(
        new,
        [r#"<w:r><w:rPr><w:b/></w:rPr><w:t>bold and strong</w:t></w:r>"#]
    );
    // The other entries are copied byte for byte, local headers included.
    let (a, b) = (
        Package::read(bytes.clone()).unwrap(),
        Package::read(out.clone()).unwrap(),
    );
    for n in a.names() {
        if n != "word/document.xml" {
            assert_eq!(a.part(&n).unwrap(), b.part(&n).unwrap(), "{n}");
        }
    }
    let again = Document::open(out).unwrap();
    let ps = paras(&again);
    let p = find(&ps, "Plain, ");
    let strong = p.runs.iter().find(|r| r.text.contains("strong")).unwrap();
    assert!(strong.look.bold);
    // Undone, the file is the input.
    assert!(doc.undo());
    assert!(doc.is_dirty());
    assert_eq!(doc.save().unwrap(), bytes);
    assert!(doc.redo());
    assert!(flow::para_text(find(&paras(&doc), "Plain, ")).contains("bold and strong"));
}

#[test]
fn typing_at_edges_spaces_and_tabs() {
    let bytes = corpus("handmade-features.docx");
    let mut doc = Document::open(bytes.clone()).unwrap();
    let at = body(2);
    doc.replace(&at, 0..0, "  Very ").unwrap();
    let l = doc.layout(&at).unwrap();
    assert!(l.text.starts_with("  Very Plain, bold"));
    let end = l.text.len();
    doc.replace(&at, end..end, "\tend\u{B}line").unwrap();
    let out = doc.save().unwrap();
    let xml = document_xml(&out);
    assert!(
        xml.contains(r#"<w:t xml:space="preserve">  Very Plain, </w:t>"#),
        "{xml}"
    );
    assert!(
        xml.contains(r#"<w:t>.</w:t><w:tab/><w:t>end</w:t><w:br/><w:t>line</w:t>"#),
        "{xml}"
    );
    let again = Document::open(out).unwrap();
    assert_eq!(
        again.layout(&at).unwrap().text,
        format!(
            "  Very {}\tend\u{B}line",
            &doc.layout(&at).unwrap().text[7..end]
        )
    );
    // Characters XML forbids are refused, and so is an object.
    assert!(doc.replace(&at, 0..0, "\u{1}").is_err());
    assert!(doc.replace(&at, 0..0, "\u{FFFC}").is_err());
}

#[test]
fn deleting_across_runs_and_whole_runs() {
    let bytes = corpus("handmade-features.docx");
    let mut doc = Document::open(bytes.clone()).unwrap();
    let at = body(2);
    let l = doc.layout(&at).unwrap();
    // "Plain, bold, red" → "Pla red": parts of two runs and a whole one.
    let a = l.text.find("in, ").unwrap();
    let b = l.text.find(" italic").unwrap() - 4;
    doc.replace(&at, a..b, "").unwrap();
    let out = doc.save().unwrap();
    let ps = paras(&Document::open(out.clone()).unwrap());
    let p = find(&ps, "Pla");
    assert!(
        flow::para_text(p).starts_with("Pla red italic and a note"),
        "{}",
        flow::para_text(p)
    );
    let xml = document_xml(&out);
    assert!(!xml.contains("<w:t>bold</w:t>"));
    assert!(
        xml.contains(r#"<w:t xml:space="preserve">Pla</w:t>"#),
        "{xml}"
    );
    // Replacing a whole paragraph's text keeps its first run's look.
    let len = doc.layout(&body(1)).unwrap().text.len();
    doc.replace(&body(1), 0..len, "Giriş").unwrap();
    let ps = paras(&doc);
    let h = find(&ps, "Giriş");
    assert_eq!(h.style, "Heading 1");
    let xml = doc.part_text("word/document.xml").unwrap();
    assert!(xml.contains(r#"<w:bookmarkStart w:id="0" w:name="intro"/><w:r><w:t>Giriş</w:t></w:r><w:bookmarkEnd w:id="0"/>"#));
}

#[test]
fn locked_pieces_refused() {
    let mut doc = Document::open(corpus("handmade-features.docx")).unwrap();
    let ps = paras(&doc);
    let changed = find(&ps, "Changed:").at.clone().unwrap();
    let l = doc.layout(&changed).unwrap();
    let d = l.text.find("deleted").unwrap();
    let e = doc.replace(&changed, d..d + 3, "").unwrap_err().to_string();
    assert!(e.contains("tracked change"), "{e}");
    // Typing at the deleted text's start goes into the text before it.
    doc.replace(&changed, d..d, "x").unwrap();
    assert!(doc.layout(&changed).unwrap().text.contains(" xdeleted"));
    let fields = find(&ps, "Date:").at.clone().unwrap();
    let l = doc.layout(&fields).unwrap();
    let f = l.text.find("2026").unwrap();
    assert!(
        doc.replace(&fields, f..f + 4, "")
            .unwrap_err()
            .to_string()
            .contains("field")
    );
    let notes = find(&ps, "Plain, ").at.clone().unwrap();
    let l = doc.layout(&notes).unwrap();
    let mark = l.text.find('\u{FFFC}').unwrap();
    assert!(doc.replace(&notes, mark..mark + 3, "").is_err());
    // A picture is deleted as a whole.
    let pic = find(&ps, "A picture.").at.clone().unwrap();
    doc.replace(&pic, 0..3, "").unwrap();
    assert_eq!(doc.layout(&pic).unwrap().text, " A picture.");
    assert!(
        !doc.part_text("word/document.xml")
            .unwrap()
            .contains("rId20")
    );
}

#[test]
fn enter_splits_and_backspace_joins() {
    let bytes = corpus("handmade-features.docx");
    let mut doc = Document::open(bytes.clone()).unwrap();
    // Enter at the end of a heading: an empty Normal paragraph after it.
    doc.split(&body(1), "Introduction".len()).unwrap();
    let ps = paras(&doc);
    assert_eq!(ps[1].style, "Heading 1");
    assert_eq!(
        (ps[2].style.as_str(), flow::para_text(&ps[2]).as_str()),
        ("Normal", "")
    );
    let xml = doc.part_text("word/document.xml").unwrap();
    assert!(
        xml.contains(r#"<w:bookmarkEnd w:id="0"/></w:p><w:p></w:p>"#),
        "{xml}"
    );
    // Enter inside the bold run cuts it in two of the same look.
    let at = body(3);
    let l = doc.layout(&at).unwrap();
    let mid = l.text.find("bold").unwrap() + 2;
    doc.split(&at, mid).unwrap();
    let ps = paras(&doc);
    assert_eq!(flow::para_text(&ps[3]), "Plain, bo");
    assert!(flow::para_text(&ps[4]).starts_with("ld, red italic"));
    assert!(ps[3].runs.last().unwrap().look.bold && ps[4].runs[0].look.bold);
    // Joined back, and the empty paragraph taken away: the input again,
    // but for how the cut run is written.
    doc.join(&body(3)).unwrap();
    assert_eq!(
        flow::para_text(&paras(&doc)[3]),
        flow::para_text(&paras(&Document::open(bytes.clone()).unwrap())[2])
    );
    doc.join(&body(1)).unwrap();
    for _ in 0..4 {
        assert!(doc.undo());
    }
    assert!(!doc.undo());
    assert_eq!(doc.save().unwrap(), bytes);
}

#[test]
fn split_in_a_list_and_inside_a_link() {
    let mut doc = Document::open(corpus("handmade-features.docx")).unwrap();
    let ps = paras(&doc);
    let one = find(&ps, "One point one").at.clone().unwrap();
    doc.split(&one, 3).unwrap();
    let labels: Vec<String> = paras(&doc)
        .iter()
        .filter_map(|p| p.label.as_ref().map(|l| l.0.clone()))
        .take(6)
        .collect();
    assert_eq!(labels, ["1.", "1.1.", "1.2.", "1.3.", "1.3.1.", "2."]);
    // The list comes after the fields: their paragraph is where it was.
    let fields = find(&ps, "Date:").at.clone().unwrap();
    let l = doc.layout(&fields).unwrap();
    let k = l.text.find("Kalem").unwrap() + 2;
    doc.split(&fields, k).unwrap();
    let ps = paras(&doc);
    let left = &ps[fields.index];
    let right = &ps[fields.index + 1];
    assert_eq!(
        left.runs.last().unwrap().link.as_deref(),
        Some("https://github.com/getkalem/kalem")
    );
    assert_eq!(
        right.runs[0].link.as_deref(),
        Some("https://github.com/getkalem/kalem")
    );
    assert_eq!(right.runs[0].text, "lem");
}

#[test]
fn deleting_across_paragraphs() {
    let bytes = corpus("python-docx-basic.docx");
    let mut doc = Document::open(bytes.clone()).unwrap();
    let ps = paras(&doc);
    let from = find(&ps, "Türkçe").at.clone().unwrap();
    let to = find(&ps, "after a line").at.clone().unwrap();
    doc.delete_between((&from, "Türkçe".len()), (&to, 4))
        .unwrap();
    let ps = paras(&doc);
    // The line break was among the deleted characters.
    assert_eq!(flow::para_text(&ps[from.index]), "Türkçeafter a line break");
    assert_eq!(
        doc.part_text("word/document.xml")
            .map(|x| x.contains("spaced")),
        Some(false)
    );
    // One step undoes it.
    assert!(doc.undo());
    assert_eq!(doc.save().unwrap(), bytes);
}

fn tracking(bytes: Vec<u8>) -> Document {
    let mut doc = Document::open(bytes).unwrap();
    doc.set_revision_author("Zeynep Kaya", Some("2026-10-09T12:00:00Z"));
    doc.set_track_changes(true).unwrap();
    assert!(doc.tracks_changes());
    doc
}

#[test]
fn tracked_typing_and_deleting_as_word_writes_them() {
    let bytes = corpus("handmade-features.docx");
    let mut doc = tracking(bytes.clone());
    let at = body(2);
    let l = doc.layout(&at).unwrap();
    let off = l.text.find("bold").unwrap() + 4;
    doc.replace(&at, off..off, " and strong").unwrap();
    let out = doc.save().unwrap();
    assert_eq!(
        part_diff(&bytes, &out),
        ["word/document.xml", "word/settings.xml"]
    );
    let xml = document_xml(&out);
    let ins = r#"<w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r><w:ins w:id="13" w:author="Zeynep Kaya" w:date="2026-10-09T12:00:00Z"><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve"> and strong</w:t></w:r></w:ins>"#;
    assert!(xml.contains(ins), "{xml}");
    let ps = paras(&doc);
    let p = find(&ps, "Plain, ");
    let new = p.runs.iter().find(|r| r.text == " and strong").unwrap();
    assert_eq!(new.inserted.as_ref().unwrap().author, "Zeynep Kaya");
    assert!(new.look.bold);
    // Deleting "Plain": kept as deleted text in a run of its look.
    doc.replace(&at, 0..5, "").unwrap();
    let xml = doc.part_text("word/document.xml").unwrap().to_owned();
    assert!(xml.contains(r#"<w:del w:id="14" w:author="Zeynep Kaya" w:date="2026-10-09T12:00:00Z"><w:r><w:delText>Plain</w:delText></w:r></w:del><w:r><w:t xml:space="preserve">, </w:t></w:r>"#), "{xml}");
    let p = find(&paras(&doc), ", bold").clone();
    assert_eq!(
        flow::para_text(&p),
        ", bold and strong, red italic and a note1, an endnotei."
    );
    assert_eq!(doc.layout(&at).unwrap().text[..5].to_owned(), "Plain");
    // Deleting what was inserted takes it away.
    let l = doc.layout(&at).unwrap();
    let s = l.text.find(" and strong").unwrap();
    doc.replace(&at, s..s + 4, "").unwrap();
    let xml = doc.part_text("word/document.xml").unwrap();
    assert!(
        xml.contains(r#"<w:t xml:space="preserve"> strong</w:t></w:r></w:ins>"#),
        "{xml}"
    );
    // A deletion over text deleted already leaves it so.
    doc.replace(&at, 0..7, "").unwrap();
    assert!(
        doc.part_text("word/document.xml")
            .unwrap()
            .contains("<w:delText>Plain</w:delText>")
    );
    // Undone step by step, the file is the input but for the setting.
    for _ in 0..4 {
        assert!(doc.undo());
    }
    let out = doc.save().unwrap();
    assert_eq!(part_diff(&bytes, &out), ["word/settings.xml"]);
    assert!(doc.undo());
    assert_eq!(doc.save().unwrap(), bytes);
}

#[test]
fn tracked_enter_and_backspace_mark_the_paragraph_mark() {
    let bytes = corpus("handmade-features.docx");
    let mut doc = tracking(bytes.clone());
    let at = body(2);
    doc.split(&at, 5).unwrap();
    let ps = paras(&doc);
    assert_eq!(flow::para_text(&ps[2]), "Plain");
    let (inserted, rev) = ps[2].mark_change.clone().unwrap();
    assert!(inserted);
    assert_eq!(rev.author, "Zeynep Kaya");
    let xml = doc.part_text("word/document.xml").unwrap();
    assert!(xml.contains(r#"<w:p><w:pPr><w:rPr><w:ins w:id="13" w:author="Zeynep Kaya" w:date="2026-10-09T12:00:00Z"/></w:rPr></w:pPr><w:r><w:t xml:space="preserve">Plain</w:t></w:r></w:p>"#), "{xml}");
    // Backspace over a mark inserted takes it away: the paragraph whole
    // again.
    doc.join(&at).unwrap();
    assert_eq!(
        flow::para_text(&paras(&doc)[2]),
        "Plain, bold, red italic and a note1, an endnotei."
    );
    // Over a mark of the file, marks it deleted.
    doc.join(&body(1)).unwrap();
    let ps = paras(&doc);
    let (inserted, _) = ps[1].mark_change.clone().unwrap();
    assert!(!inserted);
    assert_eq!(
        ps.len(),
        paras(&Document::open(bytes.clone()).unwrap()).len()
    );
    let xml = doc.part_text("word/document.xml").unwrap();
    assert!(xml.contains(r#"<w:pPr><w:pStyle w:val="Heading1"/><w:rPr><w:del w:id="13" w:author="Zeynep Kaya" w:date="2026-10-09T12:00:00Z"/></w:rPr></w:pPr>"#), "{xml}");
}

#[test]
fn tracked_deletion_across_paragraphs_takes_nothing_away() {
    let bytes = corpus("python-docx-basic.docx");
    let mut doc = tracking(bytes.clone());
    let ps = paras(&doc);
    let from = find(&ps, "Türkçe").at.clone().unwrap();
    let to = find(&ps, "after a line").at.clone().unwrap();
    doc.delete_between((&from, "Türkçe".len()), (&to, 4))
        .unwrap();
    let ps = paras(&doc);
    assert_eq!(
        ps.len(),
        paras(&Document::open(bytes.clone()).unwrap()).len()
    );
    assert_eq!(
        flow::para_text(&ps[from.index]),
        "Türkçe ğüşıöç İ".replace(" ğüşıöç İ", "")
    );
    assert!(ps[from.index].mark_change.as_ref().is_some_and(|(i, _)| !i));
    assert!(
        ps[from.index + 1]
            .runs
            .iter()
            .all(|r| r.deleted.is_some() || r.text.is_empty())
    );
    assert_eq!(flow::para_text(&ps[to.index]), "after a line break");
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.save().unwrap(), bytes);
}

#[test]
fn notes_headers_and_comments_edited_in_their_parts() {
    let bytes = corpus("handmade-features.docx");
    let mut doc = Document::open(bytes.clone()).unwrap();
    let note = ParaAt {
        story: StoryId::Footnote("1".into()),
        index: 0,
    };
    let l = doc.layout(&note).unwrap();
    // The note's own mark is an object; the text after it is edited.
    assert!(l.text.starts_with('\u{FFFC}'));
    let at = l.text.find("A footnote").unwrap();
    doc.replace(&note, at..at + 1, "One").unwrap();
    let header = ParaAt {
        story: StoryId::Header("word/header1.xml".into()),
        index: 0,
    };
    doc.replace(&header, 0..5, "KALEM").unwrap();
    let comment = ParaAt {
        story: StoryId::Comment("0".into()),
        index: 0,
    };
    let len = doc.layout(&comment).unwrap().text.len();
    doc.replace(&comment, len..len, " Done.").unwrap();
    let out = doc.save().unwrap();
    assert_eq!(
        part_diff(&bytes, &out),
        [
            "word/comments.xml",
            "word/footnotes.xml",
            "word/header1.xml"
        ]
    );
    let v = Document::open(out).unwrap().view();
    let text = v.text();
    assert!(text.starts_with("KALEM test header\n"), "{text}");
    assert!(text.contains("[1] One footnote, with italics."), "{text}");
    let mut c = String::new();
    flow::text(&v.comments[0].blocks, &mut c);
    assert_eq!(c.trim(), "Check this. Done.");
}

#[test]
fn every_cut_of_a_file_fails_without_a_panic() {
    let bytes = corpus("handmade-features.docx");
    let step = (bytes.len() / 97).max(1);
    for n in (0..bytes.len()).step_by(step) {
        if let Ok(doc) = Document::open(bytes[..n].to_vec()) {
            let _ = doc.view();
        }
    }
    // Damaged XML in a part: read as far as it goes.
    let mut p = Package::read(bytes).unwrap();
    let xml = String::from_utf8(p.part("word/document.xml").unwrap()).unwrap();
    for cut in [10, xml.len() / 3, xml.len() / 2, xml.len() - 10] {
        let mut broken = xml[..cut].to_owned();
        while !xml.is_char_boundary(broken.len()) {
            broken.pop();
        }
        p.set_part("word/document.xml", broken.into_bytes());
        let doc = Document::open(p.write().unwrap()).unwrap();
        let _ = doc.view().text();
    }
}

#[test]
fn not_a_word_document() {
    let xlsx = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../xlsx/tests/corpus/openpyxl-empty.xlsx"),
    )
    .unwrap();
    let e = Document::open(xlsx).unwrap_err().to_string();
    assert!(e.starts_with("not a Word document"), "{e}");
    assert!(Document::open(b"hello".to_vec()).is_err());
}
