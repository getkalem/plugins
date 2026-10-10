//! Paragraph formatting and lists written as Word writes them (the docx
//! list's WP9, plugin API 0.2.8's `flow-2`): alignment, indents, spacing
//! and line spacing in `w:pPr`; a list by `w:numPr`, continuing the list
//! before or a new one of Word's own definitions, the numbering part made
//! when there is none; a list's level; the look given directly cleared.
//! Each is one step, and undoing it saves the input byte for byte.

use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_ooxml::rels;
use kalem_plugin_docx::flow::{self, Align, VPara};
use kalem_plugin_docx::{Document, ParaAt, StoryId};
use kalem_viewer::{FlowAlign, LineSpacing, ListKind, ParagraphChange};

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

fn part(bytes: &[u8], name: &str) -> String {
    String::from_utf8(Package::read(bytes.to_vec()).unwrap().part(name).unwrap()).unwrap()
}

fn paras(doc: &Document) -> Vec<VPara> {
    doc.view().body_paragraphs().into_iter().cloned().collect()
}

fn index_of(doc: &Document, text: &str) -> usize {
    paras(doc)
        .iter()
        .position(|p| flow::para_text(p).contains(text))
        .unwrap_or_else(|| panic!("no paragraph {text:?}"))
}

fn label(doc: &Document, i: usize) -> Option<String> {
    paras(doc)[i].label.as_ref().map(|l| l.0.clone())
}

fn set(d: &mut Document, from: usize, to: usize, changes: &[ParagraphChange]) {
    let ps: Vec<ParaAt> = (from..=to).map(body).collect();
    d.set_paragraph_format(&ps, changes).unwrap();
}

/// The paragraphs' edit texts: what formatting leaves as it is.
fn texts(doc: &Document) -> Vec<String> {
    doc.view()
        .body_paragraphs()
        .iter()
        .map(|p| p.layout.text.clone())
        .collect()
}

#[test]
fn alignment_indents_and_spacing() {
    let input = corpus("handmade-features.docx");
    let mut d = Document::open(input.clone()).unwrap();
    let i = index_of(&d, "Plain, bold");
    let before = texts(&d);
    set(
        &mut d,
        i,
        i,
        &[
            ParagraphChange::Align(FlowAlign::Center),
            ParagraphChange::IndentStart(36.0),
            ParagraphChange::FirstLine(-18.0),
            ParagraphChange::SpaceBefore(12.0),
            ParagraphChange::SpaceAfter(6.0),
            ParagraphChange::LineSpacing(LineSpacing::Multiple(1.5)),
        ],
    );
    assert_eq!(texts(&d), before);
    let p = &paras(&d)[i];
    assert_eq!(p.align, Align::Center);
    assert_eq!(p.indent.0, 36.0);
    assert_eq!(p.spacing, (12.0, 6.0));
    let out = d.save().unwrap();
    assert_eq!(d.changed_parts(), ["word/document.xml"]);
    let xml = part(&out, "word/document.xml");
    assert!(
        xml.contains(r#"<w:p><w:pPr><w:spacing w:before="240" w:after="120" w:line="360" w:lineRule="auto"/><w:ind w:left="720" w:hanging="360"/><w:jc w:val="center"/></w:pPr><w:r><w:t xml:space="preserve">Plain, </w:t>"#),
        "{xml}"
    );
    // Left again: the property taken away, the style's left kept.
    set(&mut d, i, i, &[ParagraphChange::Align(FlowAlign::Start)]);
    assert_eq!(paras(&d)[i].align, Align::Start);
    assert!(
        !part(&d.save().unwrap(), "word/document.xml")
            .contains(r#"<w:jc w:val="left"/><w:r><w:t xml:space="preserve">Plain"#)
    );
    // Cleared: back to the style's look.
    set(&mut d, i, i, &[ParagraphChange::Clear]);
    assert_eq!(paras(&d)[i].indent.0, 0.0);
    assert!(d.undo() && d.undo() && d.undo());
    assert_eq!(d.save().unwrap(), input);
}

#[test]
fn bullets_and_numbers_as_word_makes_them() {
    let input = corpus("handmade-features.docx");
    let mut d = Document::open(input.clone()).unwrap();
    let i = index_of(&d, "Plain, bold");
    // Bullets on two paragraphs: one list.
    set(
        &mut d,
        i,
        i + 1,
        &[ParagraphChange::List(Some(ListKind::Bullet))],
    );
    assert_eq!(label(&d, i).as_deref(), Some("•"));
    assert_eq!(label(&d, i + 1).as_deref(), Some("•"));
    assert_eq!(paras(&d)[i].style_id, "ListParagraph");
    // A level deeper: Word's second bullet.
    set(&mut d, i + 1, i + 1, &[ParagraphChange::ListLevel(1)]);
    assert_eq!(paras(&d)[i + 1].list_level, Some(1));
    assert_eq!(label(&d, i + 1).as_deref(), Some("o"));
    // Out of the list again: no label, the Normal style back.
    set(&mut d, i, i + 1, &[ParagraphChange::List(None)]);
    assert_eq!(label(&d, i), None);
    assert_eq!(paras(&d)[i].style_id, "Normal");
    // Numbers: a new list from 1, then one more paragraph continuing it.
    set(
        &mut d,
        i,
        i,
        &[ParagraphChange::List(Some(ListKind::Numbered(
            "decimal".into(),
        )))],
    );
    set(
        &mut d,
        i + 1,
        i + 1,
        &[ParagraphChange::List(Some(ListKind::Numbered(
            "decimal".into(),
        )))],
    );
    assert_eq!(label(&d, i).as_deref(), Some("1."));
    assert_eq!(label(&d, i + 1).as_deref(), Some("2."));
    // Roman numbers of their own.
    let q = index_of(&d, "Theme colored");
    set(
        &mut d,
        q,
        q,
        &[ParagraphChange::List(Some(ListKind::Numbered(
            "upper-roman".into(),
        )))],
    );
    assert_eq!(label(&d, q).as_deref(), Some("I."));
    let e = d
        .set_paragraph_format(
            &[body(q)],
            &[ParagraphChange::List(Some(ListKind::Numbered(
                "hebrew".into(),
            )))],
        )
        .unwrap_err();
    assert!(e.to_string().contains("numbering"), "{e}");
    let out = d.save().unwrap();
    let numbering = part(&out, "word/numbering.xml");
    // Definitions before instances, as the schema orders them.
    let last_abstract = numbering.rfind("</w:abstractNum>").unwrap();
    let first_num = numbering.find("<w:num ").unwrap();
    assert!(last_abstract < first_num);
    let again = Document::open(out).unwrap();
    assert_eq!(label(&again, i + 1).as_deref(), Some("2."));
    assert_eq!(label(&again, q).as_deref(), Some("I."));
    while d.undo() {}
    assert_eq!(d.save().unwrap(), input);
}

#[test]
fn a_list_from_a_style_ended() {
    let mut d = Document::open(corpus("python-docx-basic.docx")).unwrap();
    let i = index_of(&d, "First bullet");
    assert!(label(&d, i).is_some(), "a list by its style");
    set(&mut d, i, i, &[ParagraphChange::List(None)]);
    assert_eq!(label(&d, i), None);
    let xml = part(&d.save().unwrap(), "word/document.xml");
    assert!(
        xml.contains(r#"<w:numPr><w:numId w:val="0"/></w:numPr>"#),
        "{xml}"
    );
}

/// The handmade document without its numbering part, its relationship
/// and content type.
fn without_numbering() -> Vec<u8> {
    let bytes = corpus("handmade-features.docx");
    let mut pkg = Package::read(bytes.clone()).unwrap();
    pkg.remove_part("word/numbering.xml");
    let r = part(&bytes, "word/_rels/document.xml.rels");
    let id = rels::parse(&r)
        .into_iter()
        .find(|x| x.kind == "numbering")
        .unwrap()
        .id;
    pkg.set_part(
        "word/_rels/document.xml.rels",
        rels::remove(&r, &id).into_bytes(),
    );
    let types = part(&bytes, "[Content_Types].xml");
    pkg.set_part(
        "[Content_Types].xml",
        rels::remove_override(&types, "word/numbering.xml").into_bytes(),
    );
    pkg.write().unwrap()
}

#[test]
fn the_numbering_part_made_when_there_is_none() {
    let input = without_numbering();
    let mut d = Document::open(input.clone()).unwrap();
    let i = index_of(&d, "Plain, bold");
    set(
        &mut d,
        i,
        i + 1,
        &[ParagraphChange::List(Some(ListKind::Numbered(
            "decimal".into(),
        )))],
    );
    assert_eq!(label(&d, i).as_deref(), Some("1."));
    assert_eq!(label(&d, i + 1).as_deref(), Some("2."));
    let out = d.save().unwrap();
    let mut changed = d.changed_parts();
    changed.sort();
    assert_eq!(
        changed,
        [
            "[Content_Types].xml",
            "word/_rels/document.xml.rels",
            "word/document.xml",
            "word/numbering.xml"
        ]
    );
    assert!(part(&out, "[Content_Types].xml").contains("/word/numbering.xml"));
    assert_eq!(
        label(&Document::open(out).unwrap(), i).as_deref(),
        Some("1.")
    );
    assert!(d.undo());
    assert_eq!(d.save().unwrap(), input);
    // Undone, the lists read again: none.
    assert_eq!(label(&d, i), None);
}

#[test]
fn tracked_paragraph_formatting() {
    let mut d = Document::open(corpus("handmade-features.docx")).unwrap();
    d.set_track_changes(true).unwrap();
    let i = index_of(&d, "Plain, bold");
    set(&mut d, i, i, &[ParagraphChange::Align(FlowAlign::End)]);
    let xml = part(&d.save().unwrap(), "word/document.xml");
    assert!(
        xml.contains(r#"<w:pPr><w:jc w:val="right"/><w:pPrChange w:id=""#),
        "{xml}"
    );
}

#[test]
fn every_file_listed_aligned_and_undone() {
    for name in [
        "handmade-features.docx",
        "libreoffice-features.docx",
        "python-docx-basic.docx",
        "libreoffice-basic.docx",
    ] {
        let input = corpus(name);
        let mut d = Document::open(input.clone()).unwrap();
        let before = texts(&d);
        let ps: Vec<ParaAt> = d
            .view()
            .body_paragraphs()
            .into_iter()
            .filter_map(|p| p.at.clone())
            .take(4)
            .collect();
        d.set_paragraph_format(
            &ps,
            &[
                ParagraphChange::Align(FlowAlign::Justify),
                ParagraphChange::List(Some(ListKind::Bullet)),
            ],
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let out = d.save().unwrap();
        assert_eq!(texts(&Document::open(out).unwrap()), before, "{name}");
        assert!(d.undo(), "{name}");
        assert_eq!(d.save().unwrap(), input, "{name}");
    }
}
