//! A document whose protection is enforced (the docx list's WP12b), kept
//! as Word keeps it: no edit, no tracked change accepted or rejected and
//! Track Changes not turned off under any protection, nor turned on
//! under read only, comments or forms protection; a document protected
//! for tracked changes keeps tracking on, and one protected for comments
//! takes comments. Each refusal leaves the document as it was.

use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_plugin_docx::flow::{self, VPara};
use kalem_plugin_docx::{Document, DocxViewer, ParaAt, StoryId};
use kalem_viewer::{AnnotationKind, FileHandle, Viewer};

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name)
}

fn read(name: &str) -> Vec<u8> {
    let p = corpus(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn body(i: usize) -> ParaAt {
    ParaAt {
        story: StoryId::Body,
        index: i,
    }
}

fn index_of(doc: &Document, text: &str) -> usize {
    let ps: Vec<VPara> = doc.view().body_paragraphs().into_iter().cloned().collect();
    ps.iter()
        .position(|p| flow::para_text(p).starts_with(text))
        .unwrap_or_else(|| panic!("no paragraph {text:?}"))
}

/// The features document with `edit` protection enforced, as Word writes
/// it without a password.
fn protected(edit: &str) -> Vec<u8> {
    let bytes = read("handmade-features.docx");
    let mut pkg = Package::read(bytes).unwrap();
    let settings = String::from_utf8(pkg.part("word/settings.xml").unwrap()).unwrap();
    let zoom = r#"<w:zoom w:percent="100"/>"#;
    assert!(settings.contains(zoom));
    let lock = format!(r#"{zoom}<w:documentProtection w:edit="{edit}" w:enforcement="1"/>"#);
    pkg.set_part(
        "word/settings.xml",
        settings.replace(zoom, &lock).into_bytes(),
    );
    pkg.write().unwrap()
}

/// The review refused: the tracked changes of the corpus (an insertion,
/// a deletion and a formatting change) neither accepted nor rejected,
/// one by one or all.
fn review_refused(d: &mut Document) {
    for id in ["10", "11", "12"] {
        for accept in [true, false] {
            let e = d.decide(id, accept).unwrap_err().to_string();
            assert!(e.contains("protected"), "{e}");
        }
    }
    assert!(d.decide_all(true).is_err());
    assert!(d.decide_all(false).is_err());
}

#[test]
fn protected_for_tracked_changes() {
    let bytes = read("handmade-protected.docx");
    let mut d = Document::open(bytes.clone()).unwrap();
    assert_eq!(d.protection(), Some("trackedChanges"));
    assert!(!d.editable());
    assert!(d.tracks_changes());
    review_refused(&mut d);
    // Tracking stays on: turning it off is refused, on is taken.
    assert!(d.set_track_changes(false).is_err());
    assert!(d.tracks_changes());
    d.set_track_changes(true).unwrap();
    assert!(d.tracks_changes());
    let i = index_of(&d, "Commented text");
    assert!(d.replace(&body(i), 0..0, "Very ").is_err());
    assert!(!d.is_dirty());
    assert_eq!(d.save().unwrap(), bytes);
}

#[test]
fn every_other_protection_refuses_the_review() {
    for edit in ["readOnly", "comments", "forms"] {
        let bytes = protected(edit);
        let mut d = Document::open(bytes.clone()).unwrap();
        assert_eq!(d.protection(), Some(edit));
        assert!(!d.editable());
        review_refused(&mut d);
        // Neither on nor off.
        assert!(d.set_track_changes(true).is_err(), "{edit}");
        assert!(!d.tracks_changes());
        assert!(d.set_track_changes(false).is_err(), "{edit}");
        let i = index_of(&d, "Commented text");
        assert!(d.replace(&body(i), 0..0, "Very ").is_err());
        assert!(!d.is_dirty(), "{edit}");
        assert_eq!(d.save().unwrap(), bytes, "{edit}");
        // A comment only where the protection is for comments.
        let made = d.add_comment((&body(i), 0), (&body(i), 9), "Is this right?");
        assert_eq!(made.is_ok(), edit == "comments", "{edit}");
    }
}

#[test]
fn protection_none_enforced_is_no_protection() {
    let mut d = Document::open(protected("none")).unwrap();
    assert_eq!(d.protection(), Some("none"));
    assert!(d.editable());
    d.set_track_changes(true).unwrap();
    d.decide("10", true).unwrap();
    d.decide_all(false).unwrap();
    let i = index_of(&d, "Commented text");
    d.replace(&body(i), 0..0, "Very ").unwrap();
    assert!(d.is_dirty());
}

#[test]
fn the_review_refused_through_the_interface() {
    let path = corpus("handmade-protected.docx");
    let mut d = DocxViewer.open(FileHandle::new(path)).unwrap();
    assert!(!d.flow(0).unwrap().editable);
    let changes: Vec<String> = d
        .annotations(None)
        .into_iter()
        .filter(|a| a.kind != AnnotationKind::Comment)
        .map(|a| a.id)
        .collect();
    assert_eq!(changes.len(), 3);
    for id in &changes {
        let e = d.accept(id).unwrap_err().to_string();
        assert!(e.contains("protected (trackedChanges)"), "{e}");
        assert!(d.reject(id).is_err());
    }
    assert!(d.accept_all(None).is_err());
    assert!(d.reject_all(None).is_err());
    assert!(d.set_tracking(false).is_err());
    d.set_tracking(true).unwrap();
    assert_eq!(d.tracking(), Some(true));
    assert!(!d.modified());
    let left = d
        .annotations(None)
        .into_iter()
        .filter(|a| a.kind != AnnotationKind::Comment)
        .count();
    assert_eq!(left, 3);
}
