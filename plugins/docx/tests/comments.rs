//! Comments added and answered as Word writes them (the docx list's
//! WP10a): in a document without comments, the comments part made with
//! its relationship and content type; the range's start and end around
//! the text, a run split where the range starts or ends inside it, the
//! reference run after the end; an answer anchored beside the comment it
//! answers and named in the extended comments part. Each is one step,
//! and undoing it saves the input byte for byte.

use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_ooxml::rels;
use kalem_plugin_docx::comments;
use kalem_plugin_docx::flow::{self, VPara};
use kalem_plugin_docx::{Document, ParaAt, StoryId};

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

/// `name` of the corpus without its comments: their markers, part,
/// relationship and content type taken out.
fn without_comments(name: &str) -> Vec<u8> {
    let bytes = corpus(name);
    let mut pkg = Package::read(bytes.clone()).unwrap();
    let mut doc = part(&bytes, "word/document.xml");
    let ids: Vec<String> = comments::comment_paragraphs(&part(&bytes, "word/comments.xml"))
        .into_keys()
        .collect();
    for id in ids {
        let m = comments::markers(&doc, &id);
        let mut spans: Vec<_> = [m.start, m.end, m.reference_run]
            .into_iter()
            .flatten()
            .collect();
        spans.sort_by_key(|s| std::cmp::Reverse(s.start));
        for s in spans {
            doc.replace_range(s, "");
        }
    }
    pkg.set_part("word/document.xml", doc.into_bytes());
    pkg.remove_part("word/comments.xml");
    let r = part(&bytes, "word/_rels/document.xml.rels");
    let id = rels::parse(&r)
        .into_iter()
        .find(|x| x.kind == "comments")
        .unwrap()
        .id;
    pkg.set_part(
        "word/_rels/document.xml.rels",
        rels::remove(&r, &id).into_bytes(),
    );
    let types = part(&bytes, "[Content_Types].xml");
    pkg.set_part(
        "[Content_Types].xml",
        rels::remove_override(&types, "word/comments.xml").into_bytes(),
    );
    pkg.write().unwrap()
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

/// The text of each run of paragraph `i` and the comments it is in.
fn commented(doc: &Document, i: usize) -> Vec<(String, Vec<String>)> {
    paras(doc)[i]
        .runs
        .iter()
        .filter(|r| !r.text.is_empty())
        .map(|r| (r.text.clone(), r.comments.clone()))
        .collect()
}

fn opened(bytes: Vec<u8>) -> Document {
    let mut d = Document::open(bytes).unwrap();
    d.set_revision_author("Ayşe Nur", Some("2026-10-09T10:00:00Z"));
    d
}

#[test]
fn a_comment_made_where_there_were_none() {
    let input = without_comments("handmade-features.docx");
    let mut d = opened(input.clone());
    assert!(d.view().comments.is_empty());
    let i = index_of(&d, "Commented text");
    let id = d
        .add_comment((&body(i), 0), (&body(i), 9), "Is this right?\nSay so.")
        .unwrap();
    let view = d.view();
    assert_eq!(view.comments.len(), 1);
    let c = &view.comments[0];
    assert_eq!(
        (c.id.as_str(), c.author.as_str()),
        (id.as_str(), "Ayşe Nur")
    );
    assert_eq!(c.date.as_deref(), Some("2026-10-09T10:00:00Z"));
    let mut text = String::new();
    flow::text(&c.blocks, &mut text);
    assert_eq!(text.trim(), "Is this right?\nSay so.");
    // The run split where the range ends; the text is the same.
    assert_eq!(
        commented(&d, i),
        [
            ("Commented".to_string(), vec![id.clone()]),
            (" text".to_string(), vec![])
        ]
    );
    let out = d.save().unwrap();
    let mut changed = d.changed_parts();
    changed.sort();
    assert_eq!(
        changed,
        [
            "[Content_Types].xml",
            "word/_rels/document.xml.rels",
            "word/comments.xml",
            "word/document.xml"
        ]
    );
    let xml = part(&out, "word/document.xml");
    let expected = format!(
        r#"<w:commentRangeStart w:id="{id}"/><w:r><w:t>Commented</w:t></w:r><w:commentRangeEnd w:id="{id}"/><w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="{id}"/></w:r><w:r><w:t xml:space="preserve"> text</w:t></w:r>"#
    );
    assert!(xml.contains(&expected), "{xml}");
    let c = part(&out, "word/comments.xml");
    assert!(c.starts_with("<?xml"));
    assert!(c.contains(&format!(
        r#"<w:comment w:id="{id}" w:author="Ayşe Nur" w:date="2026-10-09T10:00:00Z" w:initials="AN">"#
    )));
    assert!(c.contains(r#"<w:pStyle w:val="CommentText"/>"#));
    assert!(c.contains("<w:annotationRef/>"));
    assert_eq!(comments::para_ids(&c).count(), 2);
    assert!(part(&out, "[Content_Types].xml").contains(
        r#"<Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/>"#
    ));
    assert!(
        part(&out, "word/_rels/document.xml.rels")
            .contains(r#"relationships/comments" Target="comments.xml"/>"#)
    );
    // Opened again: the comment and its range.
    let again = opened(out.clone());
    assert_eq!(again.view().comments.len(), 1);
    assert_eq!(commented(&again, i)[0].1, std::slice::from_ref(&id));
    // Undone: the input byte for byte; redone: the same file again.
    assert!(d.undo());
    assert!(d.view().comments.is_empty());
    assert_eq!(d.save().unwrap(), input);
    assert!(d.redo());
    assert_eq!(d.save().unwrap(), out);
}

#[test]
fn a_range_across_runs_and_paragraphs() {
    let input = corpus("handmade-features.docx");
    let mut d = opened(input.clone());
    let i = index_of(&d, "Plain, bold");
    let before: Vec<String> = paras(&d).iter().map(flow::para_text).collect();
    // From inside "Plain" to inside the next paragraph's first word.
    let id = d
        .add_comment((&body(i), 2), (&body(i + 1), 3), "Both")
        .unwrap();
    let after: Vec<String> = paras(&d).iter().map(flow::para_text).collect();
    assert_eq!(before, after, "the text is unchanged");
    let first = commented(&d, i);
    assert_eq!(first[0], ("Pl".to_string(), vec![]));
    assert!(first[1..].iter().all(|(_, c)| c.contains(&id)), "{first:?}");
    let second = commented(&d, i + 1);
    assert!(second[0].1.contains(&id), "{second:?}");
    assert!(second.last().unwrap().1.is_empty(), "{second:?}");
    // Two comments now; the old part had one.
    assert_eq!(d.view().comments.len(), 2);
    let out = d.save().unwrap();
    assert_eq!(opened(out).view().comments.len(), 2);
    assert!(d.undo());
    assert_eq!(d.save().unwrap(), input);
}

#[test]
fn answers_thread_under_the_first_comment() {
    let input = corpus("handmade-features.docx");
    let mut d = opened(input.clone());
    let first = d.view().comments[0].id.clone();
    let answer = d.reply_comment(&first, "Yes.").unwrap();
    let again = d.reply_comment(&answer, "Done.").unwrap();
    let threads = d.comment_threads();
    assert_eq!(threads[&answer], (Some(first.clone()), false));
    assert_eq!(threads[&again], (Some(first.clone()), false));
    let view = d.view();
    assert_eq!(view.comments.len(), 3);
    assert_eq!(view.comments[1].parent.as_deref(), Some(first.as_str()));
    // Anchored on the first comment's text.
    let i = index_of(&d, "Commented text");
    let runs = commented(&d, i);
    assert_eq!(runs[0].1, [first.clone(), answer.clone(), again.clone()]);
    let out = d.save().unwrap();
    let xml = part(&out, "word/document.xml");
    let starts = format!(
        r#"<w:commentRangeStart w:id="{first}"/><w:commentRangeStart w:id="{answer}"/><w:commentRangeStart w:id="{again}"/>"#
    );
    assert!(xml.contains(&starts), "{xml}");
    let ends = format!(
        r#"<w:commentReference w:id="{first}"/></w:r><w:commentRangeEnd w:id="{answer}"/>"#
    );
    assert!(xml.contains(&ends), "{xml}");
    // The extended part made, with its relationship and content type;
    // the answered comment's paragraph given an ID.
    let mut changed = d.changed_parts();
    changed.sort();
    assert_eq!(
        changed,
        [
            "[Content_Types].xml",
            "word/_rels/document.xml.rels",
            "word/comments.xml",
            "word/commentsExtended.xml",
            "word/document.xml"
        ]
    );
    let ex = part(&out, "word/commentsExtended.xml");
    assert_eq!(ex.matches("w15:paraIdParent=").count(), 2, "{ex}");
    let reopened = opened(out);
    assert_eq!(
        reopened.comment_threads()[&again],
        (Some(first.clone()), false)
    );
    // Two steps, undone: the input byte for byte.
    assert!(d.undo() && d.undo());
    assert_eq!(d.save().unwrap(), input);
}

#[test]
fn refused_where_word_keeps_none() {
    let mut d = opened(corpus("handmade-features.docx"));
    let header = ParaAt {
        story: StoryId::Header("word/header1.xml".into()),
        index: 0,
    };
    let e = d.add_comment((&header, 0), (&header, 1), "x").unwrap_err();
    assert!(e.to_string().contains("headers and footers"), "{e}");
    let e = d
        .add_comment((&body(1), 0), (&body(1), 1), "  ")
        .unwrap_err();
    assert!(e.to_string().contains("needs text"), "{e}");
    let e = d.add_comment((&body(1), 0), (&header, 1), "x").unwrap_err();
    assert!(e.to_string().contains("one story"), "{e}");
    let e = d.reply_comment("999", "x").unwrap_err();
    assert!(e.to_string().contains("no comment 999"), "{e}");
    assert!(!d.is_dirty());
}

#[test]
fn every_file_takes_a_comment_and_an_answer() {
    for name in [
        "handmade-features.docx",
        "libreoffice-features.docx",
        "python-docx-basic.docx",
        "libreoffice-basic.docx",
    ] {
        let input = corpus(name);
        let mut d = opened(input.clone());
        let ps = paras(&d);
        let i = ps
            .iter()
            .position(|p| p.at.is_some() && flow::para_text(p).len() > 4)
            .unwrap();
        let id = d.add_comment((&body(i), 1), (&body(i), 4), "Note").unwrap();
        let answer = d.reply_comment(&id, "Answer").unwrap();
        let out = d.save().unwrap();
        let again = opened(out);
        assert_eq!(
            again.comment_threads()[&answer].0.as_deref(),
            Some(id.as_str()),
            "{name}"
        );
        assert!(d.undo() && d.undo(), "{name}");
        assert_eq!(d.save().unwrap(), input, "{name}");
    }
}
