//! Formatting written as Word writes it (the docx list's WP9): the look
//! of a range of text (bold, italic, underline, strike, raised and
//! lowered, color, highlight, size, typeface, cleared) in the runs'
//! properties, a run cut where the range starts or ends inside it; a
//! paragraph's style; tracked as formatting changes when the document
//! tracks changes. Each is one step, and undoing it saves the input byte
//! for byte.

use std::ops::Range;
use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_plugin_docx::flow::{self, VPara, Vert};
use kalem_plugin_docx::{Document, ParaAt, StoryId};
use kalem_viewer::{MarkChange, Script};

fn corpus(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn body(i: usize) -> ParaAt {
    ParaAt {
        story: StoryId::Body,
        index: i,
    }
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

fn paras(doc: &Document) -> Vec<VPara> {
    doc.view().body_paragraphs().into_iter().cloned().collect()
}

fn index_of(doc: &Document, text: &str) -> usize {
    paras(doc)
        .iter()
        .position(|p| flow::para_text(p).starts_with(text))
        .unwrap_or_else(|| panic!("no paragraph {text:?}"))
}

/// Each run of text of paragraph `i`: its text and look.
fn runs(doc: &Document, i: usize) -> Vec<(String, flow::Look)> {
    paras(doc)[i]
        .runs
        .iter()
        .filter(|r| !r.text.is_empty() && r.piece == flow::Piece::Text)
        .map(|r| (r.text.clone(), r.look.clone()))
        .collect()
}

fn texts(doc: &Document) -> Vec<String> {
    paras(doc).iter().map(flow::para_text).collect()
}

fn marks(d: &mut Document, i: usize, range: Range<usize>, changes: &[MarkChange]) {
    d.set_marks(&[(body(i), range)], changes).unwrap();
}

#[test]
fn bold_on_a_word_inside_a_run() {
    let input = corpus("handmade-features.docx");
    let mut d = Document::open(input.clone()).unwrap();
    let before = texts(&d);
    let i = index_of(&d, "Plain, bold");
    marks(&mut d, i, 0..5, &[MarkChange::Bold(true)]);
    assert_eq!(texts(&d), before, "the text is the same");
    let r = runs(&d, i);
    assert_eq!(r[0].0, "Plain");
    assert!(r[0].1.bold);
    assert_eq!(r[1].0, ", ");
    assert!(!r[1].1.bold);
    let out = d.save().unwrap();
    assert_eq!(d.changed_parts(), ["word/document.xml"]);
    assert!(document_xml(&out).contains(r#"<w:r><w:rPr><w:b/><w:bCs/></w:rPr><w:t xml:space="preserve">Plain</w:t></w:r><w:r><w:t xml:space="preserve">, </w:t></w:r>"#));
    // Off again: the property taken away, the runs as Word leaves them.
    marks(&mut d, i, 0..5, &[MarkChange::Bold(false)]);
    assert!(!runs(&d, i)[0].1.bold);
    assert!(!document_xml(&d.save().unwrap()).contains("<w:b/><w:bCs/></w:rPr><w:t>Plain"));
    assert!(d.undo() && d.undo());
    assert_eq!(d.save().unwrap(), input);
}

#[test]
fn a_toggle_a_style_turns_on_written_off() {
    let mut d = Document::open(corpus("handmade-features.docx")).unwrap();
    let i = index_of(&d, "A quote with");
    assert!(runs(&d, i)[0].1.italic, "the Quote style is italic");
    marks(&mut d, i, 0..7, &[MarkChange::Italic(false)]);
    let r = runs(&d, i);
    assert_eq!(r[0].0, "A quote");
    assert!(!r[0].1.italic);
    assert!(r[1].1.italic);
    let xml = document_xml(&d.save().unwrap());
    assert!(
        xml.contains(
            r#"<w:i w:val="0"/><w:iCs w:val="0"/></w:rPr><w:t xml:space="preserve">A quote</w:t>"#
        ),
        "{xml}"
    );
}

#[test]
fn a_range_across_paragraphs_and_every_mark() {
    let input = corpus("handmade-features.docx");
    let mut d = Document::open(input.clone()).unwrap();
    let before = texts(&d);
    let i = index_of(&d, "Plain, bold");
    let next = &before[i + 1];
    let len = |d: &Document, i: usize| d.layout(&body(i)).unwrap().text.len();
    let (end, next_end) = (len(&d, i), len(&d, i + 1).min(4));
    let _ = next;
    d.set_marks(
        &[(body(i), 7..end), (body(i + 1), 0..next_end)],
        &[
            MarkChange::Underline(Some("double".into())),
            MarkChange::Color(Some([0x12, 0x34, 0x56])),
            MarkChange::Highlight(Some([0, 0xFF, 0])),
            MarkChange::Size(Some(14.5)),
            MarkChange::Face(Some("Georgia".into())),
            MarkChange::Script(Script::Superscript),
            MarkChange::Strike(true),
        ],
    )
    .unwrap();
    assert_eq!(texts(&d), before);
    for (text, look) in runs(&d, i).into_iter().skip(1) {
        assert_eq!(look.underline.as_deref(), Some("double"), "{text}");
        assert_eq!(look.color, Some(0x123456), "{text}");
        assert_eq!(look.highlight, Some(0x00FF00), "{text}");
        assert_eq!(look.size, 14.5, "{text}");
        assert_eq!(&*look.face, "Georgia", "{text}");
        assert_eq!(look.vert, Vert::Super, "{text}");
        assert!(look.strike, "{text}");
    }
    assert!(runs(&d, i)[0].1.underline.is_none(), "Plain, stays");
    let first = &runs(&d, i + 1)[0];
    assert!(first.1.strike && first.0.len() == 4, "{first:?}");
    // Cleared: the look the styles give.
    d.set_marks(
        &[(body(i), 7..len(&d, i)), (body(i + 1), 0..4)],
        &[MarkChange::Clear],
    )
    .unwrap();
    for (text, look) in runs(&d, i).into_iter().skip(1) {
        assert!(
            look.underline.is_none() && !look.strike && look.color.is_none(),
            "{text}"
        );
        assert_eq!(look.vert, Vert::Baseline, "{text}");
    }
    let saved = d.save().unwrap();
    assert_eq!(texts(&Document::open(saved).unwrap()), before);
    assert!(d.undo() && d.undo());
    assert_eq!(d.save().unwrap(), input);
}

#[test]
fn tracked_formatting_accepted_and_rejected() {
    let mut d = Document::open(corpus("handmade-features.docx")).unwrap();
    d.set_track_changes(true).unwrap();
    d.set_revision_author("Ayşe", Some("2026-10-10T09:00:00Z"));
    let i = index_of(&d, "Plain, bold");
    marks(&mut d, i, 0..5, &[MarkChange::Bold(true)]);
    let r = paras(&d)[i].runs[0].clone();
    assert!(r.look.bold);
    let rev = r.format_changed.as_ref().expect("a formatting change");
    assert_eq!(rev.author, "Ayşe");
    let xml = document_xml(&d.save().unwrap());
    assert!(
        xml.contains(r#"<w:rPr><w:b/><w:bCs/><w:rPrChange w:id=""#),
        "{xml}"
    );
    // Rejected: not bold again, the record gone.
    d.decide(&rev.id, false).unwrap();
    let r = paras(&d)[i].runs[0].clone();
    assert!(!r.look.bold && r.format_changed.is_none());
    // Made again and accepted: bold, the record gone.
    marks(&mut d, i, 0..5, &[MarkChange::Bold(true)]);
    let id = paras(&d)[i].runs[0]
        .format_changed
        .as_ref()
        .unwrap()
        .id
        .clone();
    d.decide(&id, true).unwrap();
    let r = paras(&d)[i].runs[0].clone();
    assert!(r.look.bold && r.format_changed.is_none());
}

#[test]
fn paragraph_styles_given() {
    let input = corpus("handmade-features.docx");
    let mut d = Document::open(input.clone()).unwrap();
    let i = index_of(&d, "Plain, bold");
    d.set_paragraph_style(&[body(i), body(i + 1)], "Heading1")
        .unwrap();
    let ps = paras(&d);
    assert_eq!(ps[i].style_id, "Heading1");
    assert_eq!(ps[i + 1].style_id, "Heading1");
    assert_eq!(ps[i].outline, Some(0));
    let xml = document_xml(&d.save().unwrap());
    assert!(xml.contains(r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">Plain, </w:t>"#), "{xml}");
    // Normal, the default: written as no style, as Word does.
    d.set_paragraph_style(&[body(i)], "Normal").unwrap();
    assert_eq!(paras(&d)[i].style_id, "Normal");
    let e = d.set_paragraph_style(&[body(i)], "Nonesuch").unwrap_err();
    assert!(e.to_string().contains("no paragraph style"), "{e}");
    let e = d.set_paragraph_style(&[body(i)], "Strong").unwrap_err();
    assert!(e.to_string().contains("no paragraph style"), "{e}");
    assert!(d.undo() && d.undo());
    assert_eq!(d.save().unwrap(), input);
    // Tracked: the old properties kept.
    d.set_track_changes(true).unwrap();
    d.set_paragraph_style(&[body(i)], "Quote").unwrap();
    let xml = document_xml(&d.save().unwrap());
    assert!(
        xml.contains(r#"<w:pPr><w:pStyle w:val="Quote"/><w:pPrChange w:id=""#),
        "{xml}"
    );
}

#[test]
fn every_file_formatted_and_undone() {
    for name in [
        "handmade-features.docx",
        "libreoffice-features.docx",
        "python-docx-basic.docx",
        "libreoffice-basic.docx",
    ] {
        let input = corpus(name);
        let mut d = Document::open(input.clone()).unwrap();
        let before = texts(&d);
        // The first half of every paragraph edited, as the view with
        // edit coordinates gives them.
        let spans: Vec<(ParaAt, Range<usize>)> = d
            .view()
            .body_paragraphs()
            .into_iter()
            .filter_map(|p| {
                let t = &p.layout.text;
                let half = (0..=t.len() / 2).rev().find(|i| t.is_char_boundary(*i))?;
                p.at.clone().filter(|_| half > 0).map(|at| (at, 0..half))
            })
            .collect();
        d.set_marks(
            &spans,
            &[MarkChange::Bold(true), MarkChange::Size(Some(9.0))],
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let out = d.save().unwrap();
        assert_eq!(texts(&Document::open(out).unwrap()), before, "{name}");
        assert!(d.undo(), "{name}");
        assert_eq!(d.save().unwrap(), input, "{name}");
    }
}
