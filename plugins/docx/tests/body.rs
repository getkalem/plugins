//! The body kept edit by edit (the docx list's WP14a): after each of
//! many edits of every kind, here and there in documents of the corpus
//! and in a long one, undone and redone, the body as kept is the body
//! walked whole; and the flow Kalem holds, brought up to date with what
//! the plugin says changed (plugin API 0.2.9), is the flow of the
//! document read anew.

use std::path::PathBuf;

use kalem_ooxml::package::Package;
use kalem_plugin_docx::{Document, DocxViewer, ParaAt, StoryId};
use kalem_viewer::{
    Anchor, FileHandle, FlowItem, FlowPlace, ListKind, MarkChange, ParagraphChange, Viewer,
    ViewerDocument,
};

fn corpus(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The handmade document with `n` paragraphs more: headings, numbered
/// items, paragraphs of three runs, a table now and then.
fn long(n: usize) -> Vec<u8> {
    let mut pkg = Package::read(corpus("handmade-features.docx")).unwrap();
    let doc = String::from_utf8(pkg.part("word/document.xml").unwrap()).unwrap();
    let sect = doc.rfind("<w:sectPr").unwrap();
    let mut out = String::new();
    for i in 0..n {
        out.push_str(&match i % 23 {
            0 => format!(
                r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Chapter {i}</w:t></w:r></w:p>"#
            ),
            1..=4 => format!(
                r#"<w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Item {i}</w:t></w:r></w:p>"#
            ),
            11 => format!(
                r#"<w:tbl><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>Cell {i}</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Next</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#
            ),
            _ => format!(
                r#"<w:p><w:r><w:t xml:space="preserve">Paragraph {i} begins, </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>goes on</w:t></w:r><w:r><w:t xml:space="preserve"> and ends.</w:t></w:r></w:p>"#
            ),
        });
    }
    let doc = format!("{}{out}{}", &doc[..sect], &doc[sect..]);
    pkg.set_part("word/document.xml", doc.into_bytes());
    pkg.write().unwrap()
}

/// A little generator of numbers, the same each run.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

/// The body's paragraphs Kalem edits: each one's place and edit text,
/// read one by one as edits read them (the view would move every
/// paragraph's bytes where the text is first).
fn paragraphs(d: &Document) -> Vec<(ParaAt, String)> {
    (0..)
        .map(|index| ParaAt {
            story: StoryId::Body,
            index,
        })
        .map_while(|at| d.layout(&at).ok().map(|l| (at, l.text)))
        .collect()
}

/// A character boundary of `t` near its middle.
fn middle(t: &str) -> usize {
    (0..=t.len() / 2)
        .rev()
        .find(|i| t.is_char_boundary(*i))
        .unwrap_or(0)
}

fn check(d: &Document, what: &str) {
    let (kept, whole) = d.body_both_ways();
    if kept != whole {
        let n = kept.0.len().min(whole.0.len());
        let first = (0..n).find(|i| kept.0[*i] != whole.0[*i]);
        panic!(
            "after {what}: the body kept differs from the body walked whole \
             ({} blocks kept, {} walked; first differing block {first:?}; \
             notes the same: {})\nkept: {:#?}\nwhole: {:#?}",
            kept.0.len(),
            whole.0.len(),
            kept.1 == whole.1,
            first.map(|i| &kept.0[i]),
            first.map(|i| &whole.0[i]),
        );
    }
}

/// `steps` edits of every kind at places `seed` picks, each checked.
fn edit_many(input: Vec<u8>, seed: u64, steps: usize, name: &str) {
    let mut d = Document::open(input).unwrap();
    d.set_revision_author("Ayşe", Some("2026-10-10T09:00:00Z"));
    let mut g = Lcg(seed);
    check(&d, "opening");
    for step in 0..steps {
        let ps = paragraphs(&d);
        if ps.is_empty() {
            break;
        }
        let i = g.below(ps.len());
        let (at, text) = ps[i].clone();
        let next = ps.get(i + 1).map(|p| p.0.clone());
        let kind = g.below(14);
        let what = format!("{name}, step {step}: edit {kind} at paragraph {}", at.index);
        // Refused edits are fine: what matters is the body afterwards.
        let _ = match kind {
            0 | 1 => d.replace(&at, 0..0, "x"),
            2 => d.replace(&at, text.len()..text.len(), " typed"),
            3 => {
                let m = middle(&text);
                let end = text[m..].chars().next().map_or(m, |c| m + c.len_utf8());
                d.replace(&at, m..end, "")
            }
            4 => d.split(&at, middle(&text)),
            5 => d.split(&at, text.len()),
            6 => d.join(&at),
            7 => match &next {
                Some(n) => d.delete_between((&at, middle(&text)), (n, 0)),
                None => Ok(()),
            },
            8 => d.set_marks(&[(at.clone(), 0..middle(&text))], &[MarkChange::Bold(true)]),
            9 => {
                let style = if g.below(2) == 0 {
                    "Heading1"
                } else {
                    "Normal"
                };
                d.set_paragraph_style(std::slice::from_ref(&at), style)
            }
            10 => {
                let paras: Vec<ParaAt> = ps[i..(i + 3).min(ps.len())]
                    .iter()
                    .map(|p| p.0.clone())
                    .collect();
                let kind = match g.below(3) {
                    0 => Some(ListKind::Numbered("decimal".into())),
                    1 => Some(ListKind::Bullet),
                    _ => None,
                };
                d.set_paragraph_format(&paras, &[ParagraphChange::List(kind)])
            }
            11 => d
                .add_comment((&at, 0), (&at, middle(&text)), "A remark")
                .map(|_| ()),
            12 => {
                let on = !d.tracks_changes();
                d.set_track_changes(on)
            }
            _ => {
                if g.below(2) == 0 {
                    d.undo();
                } else {
                    d.redo();
                }
                Ok(())
            }
        };
        // Checked now and then, so that edits follow edits whose bytes
        // the paragraphs after have not been moved by yet.
        if step % 3 == 2 {
            check(&d, &what);
        }
    }
    check(&d, &format!("{name}: the last step"));
    // Undone to the start and redone to the end, step by step.
    let mut undone = 0;
    while d.undo() {
        undone += 1;
        check(&d, &format!("{name}: undoing, {undone} undone"));
    }
    while d.redo() {
        check(&d, &format!("{name}: redoing"));
    }
}

#[test]
fn the_corpus_edited_many_ways() {
    for (n, name) in [
        "handmade-features.docx",
        "libreoffice-features.docx",
        "python-docx-basic.docx",
        "libreoffice-basic.docx",
    ]
    .into_iter()
    .enumerate()
    {
        edit_many(corpus(name), 7 + n as u64, 120, name);
    }
}

#[test]
fn a_long_document_edited_many_ways() {
    for seed in [1, 2, 3] {
        edit_many(long(160), seed, 120, "the long one");
    }
}

/// A unit's whole flow.
fn all_items(d: &mut dyn ViewerDocument) -> Vec<FlowItem> {
    let n = d.flow(0).unwrap().items;
    d.flow_items(0, 0, n)
}

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("docx-body");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

/// `steps` edits through Kalem's contract, each followed by what the
/// plugin says changed applied to the items held, which are then the
/// whole flow, and the flow of the document saved and opened again.
fn flow_edited_many(input: &[u8], seed: u64, steps: usize, name: &str) {
    let file = scratch(&format!("{seed}-{name}"), input);
    let mut d = DocxViewer.open(FileHandle::new(file.clone())).unwrap();
    d.set_author("Ayşe");
    let mut held = all_items(&mut *d);
    let mut version = d.flow(0).unwrap().version;
    let mut g = Lcg(seed);
    for step in 0..steps {
        let paras: Vec<(u32, String)> = held
            .iter()
            .filter_map(|i| match i {
                FlowItem::Paragraph(p) => Some((p.index?, p.text.clone())),
                _ => None,
            })
            .collect();
        if paras.is_empty() {
            break;
        }
        let (i, text) = paras[g.below(paras.len())].clone();
        let at = |offset: usize| FlowPlace {
            paragraph: i,
            offset: offset as u32,
        };
        let end = text.len();
        let m = middle(&text);
        let kind = g.below(12);
        let _ = match kind {
            0 | 1 => d.flow_replace(0, i, 0..0, "x"),
            2 => d.flow_replace(0, i, end as u32..end as u32, " typed"),
            3 => d.flow_split(0, at(m)),
            4 => d.flow_join(0, i),
            5 => d.flow_delete(
                0,
                at(m),
                FlowPlace {
                    paragraph: i + 2,
                    offset: 0,
                },
            ),
            6 => d.flow_set_marks(0, at(0), at(m), &[MarkChange::Italic(true)]),
            7 => d.flow_set_style(0, i, i, "Heading1"),
            8 => d.flow_set_paragraphs(
                0,
                i,
                i + 2,
                &[ParagraphChange::List(Some(ListKind::Numbered(
                    "decimal".into(),
                )))],
            ),
            9 => d
                .comment(
                    Anchor::Flow {
                        unit: 0,
                        from: at(0),
                        to: at(m),
                    },
                    "A remark",
                )
                .map(|_| ()),
            10 => {
                let on = d.tracking() != Some(true);
                d.set_tracking(on)
            }
            _ => {
                if g.below(2) == 0 {
                    d.undo().map(|_| ())
                } else {
                    d.redo().map(|_| ())
                }
            }
        };
        let what = format!("{name}, step {step}: edit {kind} at paragraph {i}");
        let now = d.flow(0).unwrap();
        let c = d
            .flow_changes(0, version)
            .unwrap_or_else(|| panic!("{what}: no change told"));
        let (from, to) = (c.from as usize, (c.from + c.removed) as usize);
        let mut fresh = d.flow_items(0, c.from, c.added);
        for item in &mut held[to..] {
            if let FlowItem::Paragraph(p) = item
                && let Some(x) = &mut p.index
            {
                *x = x.saturating_add_signed(c.shift);
            }
        }
        held.splice(from..to, fresh.drain(..));
        version = now.version;
        let whole = all_items(&mut *d);
        assert_eq!(held.len(), whole.len(), "{what}: {c:?}");
        if let Some(k) = (0..held.len()).find(|k| held[*k] != whole[*k]) {
            panic!(
                "{what}: {c:?}; item {k} held {:#?}, whole {:#?}",
                held[k], whole[k]
            );
        }
        // The flow made whole of the same document.
        let saved = d.save().unwrap().bytes;
        let again = scratch(&format!("{seed}-{name}-again"), &saved);
        let mut e = DocxViewer.open(FileHandle::new(again)).unwrap();
        let anew = all_items(&mut *e);
        if let Some(k) = (0..held.len().max(anew.len())).find(|k| held.get(*k) != anew.get(*k)) {
            panic!(
                "{what}: item {k} kept {:#?}, read anew {:#?}",
                held.get(k),
                anew.get(k)
            );
        }
        assert_eq!(d.annotations(Some(0)), e.annotations(Some(0)), "{what}");
    }
}

#[test]
fn the_flow_brought_up_to_date_with_what_changed() {
    for (n, name) in [
        "handmade-features.docx",
        "libreoffice-features.docx",
        "python-docx-basic.docx",
    ]
    .into_iter()
    .enumerate()
    {
        flow_edited_many(&corpus(name), 11 + n as u64, 60, name);
    }
    flow_edited_many(&long(120), 5, 80, "long.docx");
}
