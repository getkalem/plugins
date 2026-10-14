//! Documents whose XML is broken (cut short, its tags not nesting) are
//! read as far as they go and edited without a panic: an edit is made or
//! refused, as a panic would take the plugin down with the document.

use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_plugin_docx::{Document, ParaAt, StoryId};
use kalem_viewer::MarkChange;

/// The LibreOffice document with its body's XML changed by `change`.
fn broken(change: impl Fn(&str) -> String) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/libreoffice-basic.docx");
    let mut pkg = Package::read(std::fs::read(p).unwrap()).unwrap();
    let xml = String::from_utf8(pkg.part("word/document.xml").unwrap()).unwrap();
    pkg.set_part("word/document.xml", change(&xml).into_bytes());
    pkg.write().unwrap()
}

fn paragraphs(d: &Document) -> Vec<(ParaAt, String)> {
    (0..)
        .map(|index| ParaAt {
            story: StoryId::Body,
            index,
        })
        .map_while(|at| d.layout(&at).ok().map(|l| (at, l.text)))
        .collect()
}

/// Typing, Enter, joining and bold at and next to the start and the end
/// of every paragraph and at its middle, each undone before the next: how
/// many were made (the others refused).
fn every_edit_everywhere(input: &[u8]) -> usize {
    let mut d = Document::open(input.to_vec()).unwrap();
    let mut made = 0;
    for (at, text) in paragraphs(&d) {
        let mut places: Vec<usize> = [
            0,
            1,
            text.len() / 2,
            text.len().saturating_sub(1),
            text.len(),
        ]
        .into_iter()
        .filter(|&o| o <= text.len() && text.is_char_boundary(o))
        .collect();
        places.dedup();
        for o in places {
            for kind in 0..4 {
                let done = match kind {
                    0 => d.replace(&at, o..o, "x"),
                    1 => d.split(&at, o),
                    2 => d.join(&at),
                    _ => d.set_marks(&[(at.clone(), 0..o)], &[MarkChange::Bold(true)]),
                };
                if done.is_ok() {
                    made += 1;
                    // An edit that changed nothing has nothing to undo.
                    d.undo();
                }
            }
        }
    }
    made
}

#[test]
fn a_body_cut_short_inside_a_run() {
    // The last run's text, end tags and the section's properties gone.
    let input = broken(|x| x[..x.find("Second bullet").unwrap() + 3].to_owned());
    assert!(every_edit_everywhere(&input) > 0);
}

#[test]
fn a_body_cut_short_inside_a_tag() {
    // The part ends with a `<`, the start of a tag never written.
    let input = broken(|x| {
        let title = x.find(r#"<w:pStyle w:val="Title"/>"#).unwrap();
        let cut = title + x[title..].find("</w:pPr><w").unwrap() + "</w:pPr><".len();
        x[..cut].to_owned()
    });
    assert!(every_edit_everywhere(&input) > 0);
}

#[test]
fn a_body_whose_tags_do_not_nest() {
    // A text element's end tag made a run's start, and the start of a
    // later run an end tag of text.
    let input = broken(|x| {
        x.replacen("Kalem basic</w:t>", "Kalem basic<w:r>", 1)
            .replacen("<w:r><w:rPr><w:i/>", "</w:t><w:rPr><w:i/>", 1)
    });
    assert!(every_edit_everywhere(&input) > 0);
}
