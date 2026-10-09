//! The plugin through Kalem's viewer contract: detection, the text and
//! outline Kalem shows, the information panel, a password to open, and a
//! save as itself.

use std::path::PathBuf;

use kalem_plugin_docx::DocxViewer;
use kalem_viewer::{Detection, FileHandle, UnitKind, Viewer, ViewerError};

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name)
}

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("docx-viewer");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn detected_by_extension_and_signature() {
    let v = DocxViewer;
    let head = std::fs::read(path("handmade-features.docx")).unwrap();
    assert_eq!(v.detect("a.docx", &head[..64]), Detection::Magic);
    assert_eq!(v.detect("A.DOCM", &head[..64]), Detection::Magic);
    assert_eq!(
        v.detect("locked.docx", &[0xD0, 0xCF, 0x11, 0xE0]),
        Detection::Extension
    );
    assert_eq!(v.detect("a.xlsx", &head[..64]), Detection::No);
    assert_eq!(v.detect("a.doc", &[0xD0, 0xCF, 0x11, 0xE0]), Detection::No);
}

#[test]
fn opened_with_its_text_outline_and_information() {
    let v = DocxViewer;
    let mut doc = v
        .open(FileHandle::new(path("handmade-features.docx")))
        .unwrap();
    let s = doc.structure();
    assert_eq!(s.units.len(), 1);
    assert_eq!(s.units[0].kind, UnitKind::Page);
    assert_eq!(s.outline[0].title, "Introduction");
    assert_eq!(s.outline[0].level, 1);
    let text = doc.text(0);
    assert!(text.contains("1.1.\tOne point one\n"));
    assert_eq!(doc.search("one point").len(), 2);
    let info = doc.info();
    let get = |l: &str| info.iter().find(|f| f.label == l).map(|f| f.value.clone());
    assert_eq!(get("Title").as_deref(), Some("Kalem features"));
    assert_eq!(get("Footnotes").as_deref(), Some("1"));
    assert_eq!(get("Tables").as_deref(), Some("2"));
    assert_eq!(get("Sections").as_deref(), Some("2"));
    // No picture yet: the render says why.
    let e = doc.render(0, Default::default()).unwrap_err();
    assert!(e.to_string().contains("lay Word documents out"));
    assert!(!doc.modified());
    let saved = doc.save().unwrap();
    assert_eq!(
        saved.bytes,
        std::fs::read(path("handmade-features.docx")).unwrap()
    );
    assert!(saved.losses.is_empty());
}

#[test]
fn a_password_to_open() {
    let plain = std::fs::read(path("python-docx-basic.docx")).unwrap();
    let mut n = 7u64;
    let mut random = move || {
        n = n
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        n
    };
    let locked = kalem_ooxml::crypto::encrypt(&plain, "parola", &mut random).unwrap();
    let file = scratch("locked.docx", &locked);
    let v = DocxViewer;
    let e = v.open(FileHandle::new(&file)).err().unwrap();
    assert_eq!(e, ViewerError::needs_password());
    assert_eq!(
        v.open_with_password(FileHandle::new(&file), "yanlış")
            .err()
            .unwrap(),
        ViewerError::needs_password()
    );
    let mut doc = v
        .open_with_password(FileHandle::new(&file), "parola")
        .unwrap();
    assert!(doc.text(0).contains("Türkçe ğüşıöç İ"));
    // Saved encrypted again with the same password.
    let again = doc.save().unwrap().bytes;
    assert!(again.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]));
    assert_eq!(
        kalem_ooxml::crypto::decrypt(&again, "parola").unwrap(),
        plain
    );
}

#[test]
fn a_file_that_is_not_one_fails_with_a_message() {
    let file = scratch("broken.docx", b"PK\x03\x04 not really");
    let e = DocxViewer.open(FileHandle::new(&file)).err().unwrap();
    assert!(!e.to_string().is_empty());
}
