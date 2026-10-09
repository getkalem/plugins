//! The document through Kalem's `flow` and `annotations` interfaces
//! (plugin API 0.2.7), as the Rust contract the component exports: its
//! items, the edits by paragraph index, comments and tracked changes,
//! accepted and rejected.

use std::path::PathBuf;

use kalem_plugin_docx::DocxViewer;
use kalem_viewer::{
    AnnotationKind, AsideKind, FileHandle, FlowItem, FlowParagraph, FlowPlace, FlowRole, Piece,
    Viewer, ViewerDocument,
};

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name)
}

fn open(name: &str) -> Box<dyn ViewerDocument> {
    DocxViewer.open(FileHandle::new(corpus(name))).unwrap()
}

fn items(d: &mut Box<dyn ViewerDocument>) -> Vec<FlowItem> {
    let l = d.flow(0).expect("a flow");
    d.flow_items(0, 0, l.items)
}

fn paras(items: &[FlowItem]) -> Vec<&FlowParagraph> {
    items
        .iter()
        .filter_map(|i| match i {
            FlowItem::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn find<'a>(ps: &'a [&FlowParagraph], text: &str) -> &'a FlowParagraph {
    ps.iter()
        .find(|p| p.text.contains(text))
        .unwrap_or_else(|| panic!("no paragraph with {text:?}"))
}

#[test]
fn the_document_as_a_flow() {
    let mut d = open("handmade-features.docx");
    assert!(d.flow(1).is_none());
    let l = d.flow(0).unwrap();
    assert!(l.editable);
    let all = items(&mut d);
    // The header first, then the body.
    assert!(matches!(&all[0], FlowItem::AsideStart(a) if a.kind == AsideKind::Header));
    let ps = paras(&all);
    let title = find(&ps, "Kalem features");
    assert_eq!(title.role, FlowRole::Title);
    let h1 = find(&ps, "Introduction");
    assert_eq!(
        (h1.role, h1.level, h1.style.as_str()),
        (FlowRole::Heading, 1, "Heading 1")
    );
    assert_eq!(find(&ps, "Styles").level, 2);
    let item = find(&ps, "One point one");
    assert_eq!((item.role, item.level), (FlowRole::ListItem, 2));
    assert_eq!(item.label.as_ref().unwrap().0, "1.1.");
    assert_eq!(find(&ps, "A quote with").role, FlowRole::Quote);
    let plain = find(&ps, "Plain, ");
    let bold = plain.runs.iter().find(|r| r.text == "bold").unwrap();
    assert!(bold.marks.bold);
    assert_eq!(bold.marks.size, Some(11.0));
    assert_eq!(bold.marks.face.as_deref(), Some("Calibri"));
    let note = plain
        .runs
        .iter()
        .find(|r| matches!(r.piece, Piece::NoteMark(_)))
        .unwrap();
    assert_eq!(note.piece, Piece::NoteMark("f1".into()));
    assert_eq!(note.text, "1");
    assert!(note.locked.is_some());
    // Every edited paragraph has an index, in order.
    let indices: Vec<u32> = ps.iter().filter_map(|p| p.index).collect();
    assert_eq!(indices, (0..indices.len() as u32).collect::<Vec<_>>());
    // A table of rows of cells, a text box as a frame, the notes last.
    assert!(
        all.iter()
            .any(|i| matches!(i, FlowItem::TableStart(t) if t.style == "Kalem Table"))
    );
    assert!(
        all.iter()
            .any(|i| matches!(i, FlowItem::AsideStart(a) if a.kind == AsideKind::Frame))
    );
    let notes: Vec<&FlowItem> = all
        .iter()
        .filter(|i| matches!(i, FlowItem::AsideStart(a) if matches!(a.kind, AsideKind::Footnote | AsideKind::Endnote)))
        .collect();
    assert_eq!(notes.len(), 2);
    assert!(matches!(notes[0], FlowItem::AsideStart(a) if a.id == "f1" && a.label == "1"));
    // A link, a field's result locked.
    let fields = find(&ps, "Date:");
    assert!(
        fields
            .runs
            .iter()
            .any(|r| r.link.as_deref() == Some("https://github.com/getkalem/kalem"))
    );
    assert!(
        fields
            .runs
            .iter()
            .find(|r| r.text == "2026-10-09")
            .unwrap()
            .locked
            .is_some()
    );
    // Comments and tracked changes, anchored where their runs are.
    let a = d.annotations(None);
    let comment = a
        .iter()
        .find(|x| x.kind == AnnotationKind::Comment)
        .unwrap();
    assert_eq!(
        (comment.author.as_str(), comment.text.as_str()),
        ("Ayşe Yılmaz", "Check this.")
    );
    let commented = find(&ps, "Commented text");
    assert!(commented.runs[0].annotations.contains(&comment.id));
    assert!(
        matches!(comment.anchors[0], kalem_viewer::Anchor::Flow { from, .. } if from.paragraph == commented.index.unwrap())
    );
    let ins = a
        .iter()
        .find(|x| x.kind == AnnotationKind::Insertion)
        .unwrap();
    let del = a
        .iter()
        .find(|x| x.kind == AnnotationKind::Deletion)
        .unwrap();
    assert!(a.iter().any(|x| x.kind == AnnotationKind::Formatting));
    assert_eq!((ins.id.as_str(), del.id.as_str()), ("r10", "r11"));
    let changed = find(&ps, "Changed:");
    assert!(
        changed
            .runs
            .iter()
            .find(|r| r.text == "deleted")
            .unwrap()
            .annotations
            .contains(&del.id)
    );
    assert_eq!(d.tracking(), Some(false));
    assert!(
        d.flow_styles()
            .iter()
            .any(|s| s.name == "Heading 1" && s.shown)
    );
}

#[test]
fn edits_by_paragraph_index() {
    let mut d = open("handmade-features.docx");
    let all = items(&mut d);
    let ps = paras(&all);
    let plain = find(&ps, "Plain, ").index.unwrap();
    let v0 = d.flow(0).unwrap().version;
    d.flow_replace(0, plain, 0..5, "Simple").unwrap();
    assert!(d.flow(0).unwrap().version > v0);
    let all = items(&mut d);
    assert!(find(&paras(&all), "Simple, bold").index == Some(plain));
    d.flow_split(
        0,
        FlowPlace {
            paragraph: plain,
            offset: 6,
        },
    )
    .unwrap();
    let all = items(&mut d);
    let ps = paras(&all);
    assert_eq!(
        ps.iter().find(|p| p.index == Some(plain)).unwrap().text,
        "Simple"
    );
    assert!(
        ps.iter()
            .find(|p| p.index == Some(plain + 1))
            .unwrap()
            .text
            .starts_with(", bold")
    );
    d.flow_join(0, plain).unwrap();
    // A paragraph of the header and one of the body are not joined.
    assert!(d.flow_join(0, 0).is_err());
    // Through the batch Kalem makes of several edits: one undo.
    d.begin_batch();
    d.flow_replace(0, plain, 0..0, "A ").unwrap();
    d.flow_replace(0, plain, 0..0, "B ").unwrap();
    d.end_batch();
    assert!(
        paras(&items(&mut d))
            .iter()
            .any(|p| p.text.starts_with("B A Simple"))
    );
    assert!(d.undo().unwrap());
    assert!(
        paras(&items(&mut d))
            .iter()
            .any(|p| p.text.starts_with("Simple, bold"))
    );
    // A note's paragraph is edited in its own part.
    let all = items(&mut d);
    let note = paras(&all)
        .into_iter()
        .find(|p| p.text.contains("A footnote"))
        .unwrap();
    // After the note's own mark, an object of three bytes.
    let at = note.text.find("A footnote").unwrap() as u32;
    let note = note.index.unwrap();
    d.flow_replace(0, note, at..at + 1, "One").unwrap();
    assert!(
        paras(&items(&mut d))
            .iter()
            .any(|p| p.text.contains("One footnote"))
    );
    while d.undo().unwrap() {}
    assert!(!d.modified());
}

#[test]
fn tracked_changes_accepted_and_rejected() {
    let mut d = open("handmade-features.docx");
    d.set_author("Zeynep Kaya");
    // Accepting the insertion keeps its text; rejecting the deletion
    // brings its text back as ordinary text.
    d.accept("r10").unwrap();
    d.reject("r11").unwrap();
    let all = items(&mut d);
    let ps = paras(&all);
    let changed = find(&ps, "Changed:");
    let ins = changed.runs.iter().find(|r| r.text == "inserted").unwrap();
    assert!(ins.annotations.is_empty());
    let back = changed.runs.iter().find(|r| r.text == "deleted").unwrap();
    assert!(back.annotations.is_empty() && back.locked.is_none());
    // The formatting change rejected: not bold any more.
    d.reject("r12").unwrap();
    let all = items(&mut d);
    let made = find(&paras(&all), "Changed:")
        .runs
        .iter()
        .find(|r| r.text == "made bold")
        .cloned()
        .unwrap();
    assert!(!made.marks.bold);
    assert!(
        d.annotations(None)
            .iter()
            .all(|a| a.kind == AnnotationKind::Comment)
    );
    assert!(d.accept("r99").is_err());
    // Tracking on: typing is an insertion by the author, Kalem's set one.
    d.set_tracking(true).unwrap();
    assert_eq!(d.tracking(), Some(true));
    let plain = find(&paras(&items(&mut d)), "Plain, ").index.unwrap();
    d.flow_replace(0, plain, 0..0, "Very ").unwrap();
    let a = d.annotations(None);
    let typed = a
        .iter()
        .find(|x| x.kind == AnnotationKind::Insertion)
        .unwrap();
    assert_eq!(typed.author, "Zeynep Kaya");
    // Rejected, the text is as it was; all of it then, accepted.
    d.reject(&typed.id.clone()).unwrap();
    assert!(
        find(&paras(&items(&mut d)), "Plain, ")
            .text
            .starts_with("Plain, ")
    );
    d.flow_replace(0, plain, 0..0, "Very ").unwrap();
    d.accept_all(None).unwrap();
    assert!(
        d.annotations(None)
            .iter()
            .all(|a| a.kind == AnnotationKind::Comment)
    );
    assert!(
        find(&paras(&items(&mut d)), "Very Plain").runs[0]
            .annotations
            .is_empty()
    );
    let out = d.save().unwrap().bytes;
    let again = kalem_plugin_docx::Document::open(out).unwrap();
    assert!(again.view().text().contains("Very Plain, bold"));
}

#[test]
fn a_tracked_enter_rejected_joins_again() {
    let mut d = open("handmade-features.docx");
    d.set_tracking(true).unwrap();
    let plain = find(&paras(&items(&mut d)), "Plain, ").index.unwrap();
    d.flow_split(
        0,
        FlowPlace {
            paragraph: plain,
            offset: 5,
        },
    )
    .unwrap();
    let a = d.annotations(None);
    let mark = a
        .iter()
        .find(|x| x.kind == AnnotationKind::Insertion)
        .unwrap()
        .id
        .clone();
    assert_eq!(
        paras(&items(&mut d))
            .iter()
            .find(|p| p.index == Some(plain))
            .unwrap()
            .text,
        "Plain"
    );
    d.reject(&mark).unwrap();
    let all = items(&mut d);
    assert!(find(&paras(&all), "Plain, bold").index == Some(plain));
}

#[test]
fn comments_added_and_answered_through_the_interface() {
    let mut d = open("handmade-features.docx");
    d.set_author("Ayşe Nur");
    let is = items(&mut d);
    let ps = paras(&is);
    let p = find(&ps, "Plain, bold").index.unwrap();
    let on = kalem_viewer::Anchor::Flow {
        unit: 0,
        from: FlowPlace {
            paragraph: p,
            offset: 0,
        },
        to: FlowPlace {
            paragraph: p,
            offset: 5,
        },
    };
    let id = d.comment(on.clone(), "Why plain?").unwrap();
    let all = d.annotations(None);
    let c = all.iter().find(|a| a.id == id).unwrap();
    assert_eq!(c.kind, AnnotationKind::Comment);
    assert_eq!(
        (c.author.as_str(), c.text.as_str()),
        ("Ayşe Nur", "Why plain?")
    );
    assert_eq!(c.anchors, [on]);
    assert_eq!(c.parent, None);
    // The flow says it: the commented run carries the comment.
    let is = items(&mut d);
    let ps = paras(&is);
    let runs = &find(&ps, "Plain, bold").runs;
    assert_eq!(runs[0].text, "Plain");
    assert!(runs[0].annotations.contains(&id));
    let answer = d.reply(&id, "Because.").unwrap();
    let all = d.annotations(None);
    let a = all.iter().find(|a| a.id == answer).unwrap();
    assert_eq!(a.parent.as_deref(), Some(id.as_str()));
    assert_eq!(a.anchors, c_anchors(&all, &id));
    // Each one step of the document's history.
    assert!(d.undo().unwrap() && d.undo().unwrap());
    assert!(
        !d.annotations(None)
            .iter()
            .any(|a| a.id == id || a.id == answer)
    );
    // Refused with a reason Kalem shows.
    let e = d.reply("c999", "x").unwrap_err();
    assert!(e.0.contains("no comment 999"), "{}", e.0);
}

fn c_anchors(all: &[kalem_viewer::Annotation], id: &str) -> Vec<kalem_viewer::Anchor> {
    all.iter().find(|a| a.id == id).unwrap().anchors.clone()
}

#[test]
fn a_new_document_typed_into() {
    // New Document through the contract (`formats.new-file`): a blank
    // `.docx` that opens with one empty paragraph in Word's Normal style,
    // takes typing and Enter, and saves as itself.
    let bytes = DocxViewer.new_file("docx", &[]).unwrap();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("docx-new");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("Document1.docx");
    std::fs::write(&f, &bytes).unwrap();
    let mut d = DocxViewer.open(FileHandle::new(&f)).unwrap();
    let all = items(&mut d);
    let ps = paras(&all);
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].text, "");
    let at = ps[0].index.unwrap();
    assert!(!d.modified());
    d.flow_replace(0, at, 0..0, "Merhaba dünya").unwrap();
    d.flow_split(
        0,
        FlowPlace {
            paragraph: at,
            offset: "Merhaba dünya".len() as u32,
        },
    )
    .unwrap();
    d.flow_replace(0, at + 1, 0..0, "İkinci satır").unwrap();
    let saved = d.save().unwrap();
    assert!(saved.losses.is_empty());
    std::fs::write(&f, &saved.bytes).unwrap();
    let again = DocxViewer.open(FileHandle::new(&f)).unwrap();
    assert_eq!(again.text(0), "Merhaba dünya\nİkinci satır\n");
    // Each kind, and the styles Kalem offers there.
    for ext in ["docm", "dotx", "dotm"] {
        let f = dir.join(format!("Document1.{ext}"));
        std::fs::write(&f, DocxViewer.new_file(ext, &[]).unwrap()).unwrap();
        let mut d = DocxViewer.open(FileHandle::new(&f)).unwrap();
        let styles: Vec<String> = d.flow_styles().into_iter().map(|s| s.name).collect();
        assert!(styles.iter().any(|s| s == "Heading 1"), "{ext}: {styles:?}");
    }
    assert!(DocxViewer.new_file("doc", &[]).is_err());
}
