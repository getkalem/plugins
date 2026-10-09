//! The document as Kalem's `flow` and `annotations` interfaces give it
//! (plugin API 0.2.7): the view's blocks as flow items in no format's
//! vocabulary, every story's paragraphs numbered in one sequence for the
//! edits, and the comments and tracked changes as annotations.
//!
//! The items are the header of the first section (an aside), the body,
//! its footer, then the footnotes and endnotes in the order they are
//! referred to (asides of their own). A paragraph's index is its place
//! among the edited paragraphs of that order; [`FlowCache::paras`] maps it
//! back to its story. A comment's ID is `c` and its `w:id`, a tracked
//! change's `r` and its `w:id`.

use std::collections::HashMap;

use kalem_viewer as kv;

use crate::document::DocView;
use crate::flow::{Align, Layout, Lock, Look, ParaAt, Piece, VBlock, VPara, VRun, Vert};
use crate::props::Revision;

/// The flow of a document, as last given.
#[derive(Debug, Clone, Default)]
pub struct FlowCache {
    /// Its items.
    pub items: Vec<kv::FlowItem>,
    /// Each index's paragraph.
    pub paras: Vec<ParaAt>,
    /// Its comments and tracked changes.
    pub annotations: Vec<kv::Annotation>,
}

fn rgb(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// A run's look as marks.
pub fn marks(l: &Look) -> kv::Marks {
    kv::Marks {
        bold: l.bold,
        italic: l.italic,
        underline: l.underline.clone(),
        strike: l.strike,
        double_strike: l.double_strike,
        caps: l.caps,
        small_caps: l.small_caps,
        hidden: l.hidden,
        script: match l.vert {
            Vert::Baseline => kv::Script::Baseline,
            Vert::Super => kv::Script::Superscript,
            Vert::Sub => kv::Script::Subscript,
        },
        color: l.color.map(rgb),
        highlight: l.highlight.or(l.shading).map(rgb),
        size: Some(l.size),
        face: Some(l.face.to_string()),
    }
}

/// What builds the cache: the index of the next edited paragraph, and
/// where each annotation was seen.
#[derive(Default)]
struct Builder {
    cache: FlowCache,
    /// Per annotation: its first and last place, and what it is.
    seen: HashMap<String, (kv::FlowPlace, kv::FlowPlace)>,
    revisions: Vec<(String, kv::AnnotationKind, Revision)>,
}

fn lock_reason(l: Lock) -> String {
    l.reason().to_string()
}

impl Builder {
    fn note(&mut self, id: String, at: kv::FlowPlace, end: kv::FlowPlace) {
        self.seen
            .entry(id)
            .and_modify(|(_, to)| *to = end)
            .or_insert((at, end));
    }

    fn revision(&mut self, kind: kv::AnnotationKind, rev: &Revision) -> Option<String> {
        if rev.id.is_empty() {
            return None;
        }
        let id = format!("r{}", rev.id);
        if !self.revisions.iter().any(|(i, _, _)| *i == id) {
            self.revisions.push((id.clone(), kind, rev.clone()));
        }
        Some(id)
    }

    fn run(&mut self, r: &VRun, layout: &Layout, index: Option<u32>) -> kv::FlowRun {
        let mut ids: Vec<String> = r.comments.iter().map(|c| format!("c{c}")).collect();
        if let Some(rev) = &r.inserted
            && let Some(id) = self.revision(kv::AnnotationKind::Insertion, rev)
        {
            ids.push(id);
        }
        if let Some(rev) = &r.deleted
            && let Some(id) = self.revision(kv::AnnotationKind::Deletion, rev)
        {
            ids.push(id);
        }
        if let Some(rev) = &r.format_changed
            && let Some(id) = self.revision(kv::AnnotationKind::Formatting, rev)
        {
            ids.push(id);
        }
        if let Some(i) = index {
            let from = kv::FlowPlace {
                paragraph: i,
                offset: r.source.start as u32,
            };
            let to = kv::FlowPlace {
                paragraph: i,
                offset: r.source.end as u32,
            };
            for id in &ids {
                self.note(id.clone(), from, to);
            }
        }
        let locked = layout
            .segs
            .iter()
            .find(|s| {
                s.range.start < r.source.end.max(r.source.start + 1) && s.range.end > r.source.start
            })
            .and_then(|s| s.lock)
            .map(lock_reason);
        let piece = match &r.piece {
            Piece::Text => kv::Piece::Text,
            Piece::Tab => kv::Piece::Tab,
            Piece::LineBreak => kv::Piece::LineBreak,
            Piece::PageBreak => kv::Piece::PageBreak,
            Piece::ColumnBreak => kv::Piece::ColumnBreak,
            Piece::FootnoteRef(id) => kv::Piece::NoteMark(format!("f{id}")),
            Piece::EndnoteRef(id) => kv::Piece::NoteMark(format!("e{id}")),
            Piece::Picture {
                target, size, alt, ..
            } => kv::Piece::Picture(kv::FlowPicture {
                id: target.clone().unwrap_or_default(),
                width: size.0,
                height: size.1,
                alt: alt.clone(),
            }),
            Piece::Placeholder(_) => kv::Piece::Placeholder,
        };
        kv::FlowRun {
            text: r.text.clone(),
            piece,
            marks: marks(&r.look),
            source: r.source.start as u32..r.source.end as u32,
            link: r.link.clone(),
            annotations: ids,
            locked,
        }
    }

    fn para(&mut self, p: &VPara) -> kv::FlowParagraph {
        let index = p.at.as_ref().map(|at| {
            self.cache.paras.push(at.clone());
            (self.cache.paras.len() - 1) as u32
        });
        let style = p.style.as_str();
        let (role, level) = if let Some(o) = p.outline {
            (kv::FlowRole::Heading, o.saturating_add(1))
        } else if style.eq_ignore_ascii_case("Title") {
            (kv::FlowRole::Title, 0)
        } else if style.eq_ignore_ascii_case("Subtitle") {
            (kv::FlowRole::Subtitle, 0)
        } else if let Some(l) = p.list_level {
            (kv::FlowRole::ListItem, l.saturating_add(1))
        } else if style.contains("Quote") {
            (kv::FlowRole::Quote, 0)
        } else if style.eq_ignore_ascii_case("Caption") {
            (kv::FlowRole::Caption, 0)
        } else if style.contains("Code") || style.eq_ignore_ascii_case("HTML Preformatted") {
            (kv::FlowRole::Code, 0)
        } else {
            (kv::FlowRole::Body, 0)
        };
        let runs = p
            .runs
            .iter()
            .map(|r| self.run(r, &p.layout, index))
            .collect();
        let mut annotations = Vec::new();
        if let Some((inserted, rev)) = &p.mark_change {
            let kind = if *inserted {
                kv::AnnotationKind::Insertion
            } else {
                kv::AnnotationKind::Deletion
            };
            if let Some(id) = self.revision(kind, rev) {
                if let Some(i) = index {
                    let end = kv::FlowPlace {
                        paragraph: i,
                        offset: p.layout.text.len() as u32,
                    };
                    self.note(id.clone(), end, end);
                }
                annotations.push(id);
            }
        }
        kv::FlowParagraph {
            index,
            role,
            level,
            style: p.style.clone(),
            label: p
                .label
                .as_ref()
                .map(|(t, _, look)| (t.clone(), marks(look))),
            align: match p.align {
                Align::Start => kv::FlowAlign::Start,
                Align::Center => kv::FlowAlign::Center,
                Align::End => kv::FlowAlign::End,
                Align::Justify => kv::FlowAlign::Justify,
            },
            indent: p.indent,
            spacing: p.spacing,
            background: p.shading.map(rgb),
            text: p.layout.text.clone(),
            runs,
            annotations,
        }
    }

    fn blocks(&mut self, blocks: &[VBlock]) {
        for b in blocks {
            match b {
                VBlock::Para(p) => {
                    let fp = self.para(p);
                    self.cache.items.push(kv::FlowItem::Paragraph(fp));
                }
                VBlock::Table(t) => {
                    self.cache
                        .items
                        .push(kv::FlowItem::TableStart(kv::FlowTable {
                            columns: t.grid.clone(),
                            style: t.style.clone().unwrap_or_default(),
                        }));
                    for row in &t.rows {
                        self.cache
                            .items
                            .push(kv::FlowItem::RowStart(kv::FlowRow { header: row.header }));
                        for c in &row.cells {
                            let border = |b: &Option<crate::flow::VBorder>| {
                                b.as_ref().map(|b| kv::FlowBorder {
                                    color: b.color.map(rgb),
                                    width: b.width,
                                    style: b.style.clone(),
                                })
                            };
                            self.cache.items.push(kv::FlowItem::CellStart(kv::FlowCell {
                                columns: c.columns,
                                merged: c.merge == Some(crate::props::VMerge::Continue),
                                background: c.fill.map(rgb),
                                borders: [
                                    border(&c.borders[0]),
                                    border(&c.borders[1]),
                                    border(&c.borders[2]),
                                    border(&c.borders[3]),
                                ],
                            }));
                            self.blocks(&c.blocks);
                            self.cache.items.push(kv::FlowItem::CellEnd);
                        }
                        self.cache.items.push(kv::FlowItem::RowEnd);
                    }
                    self.cache.items.push(kv::FlowItem::TableEnd);
                }
                VBlock::Frame(f) => {
                    self.cache.items.push(kv::FlowItem::AsideStart(kv::Aside {
                        kind: kv::AsideKind::Frame,
                        id: String::new(),
                        label: "Text box".into(),
                    }));
                    self.blocks(f);
                    self.cache.items.push(kv::FlowItem::AsideEnd);
                }
                VBlock::Placeholder(p) => {
                    self.cache.items.push(kv::FlowItem::Placeholder(p.clone()))
                }
            }
        }
    }

    fn aside(&mut self, kind: kv::AsideKind, id: String, label: String, blocks: &[VBlock]) {
        if blocks.is_empty() {
            return;
        }
        self.cache
            .items
            .push(kv::FlowItem::AsideStart(kv::Aside { kind, id, label }));
        self.blocks(blocks);
        self.cache.items.push(kv::FlowItem::AsideEnd);
    }
}

/// The flow and the annotations of a document's view (made with its
/// edit coordinates: [`crate::Document::view_with`]).
pub fn build(view: &DocView) -> FlowCache {
    let mut b = Builder::default();
    b.aside(
        kv::AsideKind::Header,
        "header".into(),
        String::new(),
        &view.header,
    );
    b.blocks(&view.body);
    b.aside(
        kv::AsideKind::Footer,
        "footer".into(),
        String::new(),
        &view.footer,
    );
    for n in &view.footnotes {
        b.aside(
            kv::AsideKind::Footnote,
            format!("f{}", n.id),
            n.mark.clone(),
            &n.blocks,
        );
    }
    for n in &view.endnotes {
        b.aside(
            kv::AsideKind::Endnote,
            format!("e{}", n.id),
            n.mark.clone(),
            &n.blocks,
        );
    }
    let anchor = |seen: &HashMap<String, (kv::FlowPlace, kv::FlowPlace)>, id: &str| {
        seen.get(id)
            .map(|(from, to)| {
                vec![kv::Anchor::Flow {
                    unit: 0,
                    from: *from,
                    to: *to,
                }]
            })
            .unwrap_or_default()
    };
    let mut annotations: Vec<kv::Annotation> = view
        .comments
        .iter()
        .map(|c| {
            let mut text = String::new();
            crate::flow::text(&c.blocks, &mut text);
            let id = format!("c{}", c.id);
            kv::Annotation {
                anchors: anchor(&b.seen, &id),
                id,
                kind: kv::AnnotationKind::Comment,
                author: c.author.clone(),
                date: c.date.clone(),
                text: text.trim().to_string(),
                parent: None,
                resolved: false,
            }
        })
        .collect();
    for (id, kind, rev) in &b.revisions {
        annotations.push(kv::Annotation {
            anchors: anchor(&b.seen, id),
            id: id.clone(),
            kind: *kind,
            author: rev.author.clone(),
            date: rev.date.clone(),
            text: String::new(),
            parent: None,
            resolved: false,
        });
    }
    b.cache.annotations = annotations;
    b.cache
}
