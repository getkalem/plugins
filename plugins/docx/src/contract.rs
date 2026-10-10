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
    /// Each index's edit text's length.
    pub lens: Vec<usize>,
    /// Its comments and tracked changes.
    pub annotations: Vec<kv::Annotation>,
    /// Each block of the body: its first item and its first paragraph's
    /// index; then the item and the index after the body.
    body: Vec<(usize, usize)>,
    /// Each block of the body: whether comments or tracked changes are
    /// in it.
    marked: Vec<bool>,
}

/// Whether comments or tracked changes are in `blocks`.
fn marked(blocks: &[VBlock]) -> bool {
    blocks.iter().any(|b| match b {
        VBlock::Para(p) => {
            p.mark_change.is_some()
                || p.runs.iter().any(|r| {
                    !r.comments.is_empty()
                        || r.inserted.is_some()
                        || r.deleted.is_some()
                        || r.format_changed.is_some()
                })
        }
        VBlock::Table(t) => t
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .any(|c| marked(&c.blocks)),
        VBlock::Frame(f) => marked(f),
        VBlock::Placeholder(_) | VBlock::Rule => false,
    })
}

/// What changed in a flow's items: those from `from` on, `removed` of
/// them, are `added` items now; the paragraphs' indexes of the items
/// after them moved by `shift` (plugin API 0.2.9, `flow-3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemChange {
    /// The first item changed.
    pub from: usize,
    /// The items it replaced.
    pub removed: usize,
    /// The items it put there.
    pub added: usize,
    /// How the indexes after moved.
    pub shift: isize,
}

impl ItemChange {
    /// This change, then `next`: the two as one.
    pub fn then(self, next: ItemChange) -> ItemChange {
        let first = (self.removed > 0 || self.added > 0).then_some((
            self.from,
            self.from + self.removed,
            self.from + self.added,
        ));
        if next.removed == 0 && next.added == 0 {
            return ItemChange {
                shift: self.shift + next.shift,
                ..self
            };
        }
        let (lo, old, new) =
            crate::body::then(first, next.from..next.from + next.removed, next.added);
        ItemChange {
            from: lo,
            removed: old - lo,
            added: new - lo,
            shift: self.shift + next.shift,
        }
    }
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

/// What builds the items: those made, the paragraphs numbered from
/// `base` on.
#[derive(Default)]
struct Builder {
    items: Vec<kv::FlowItem>,
    paras: Vec<ParaAt>,
    lens: Vec<usize>,
    base: usize,
}

fn lock_reason(l: Lock) -> String {
    l.reason().to_string()
}

/// A run's annotations: its comments', then its tracked changes'.
fn run_ids(r: &VRun) -> Vec<String> {
    let mut ids: Vec<String> = r.comments.iter().map(|c| format!("c{c}")).collect();
    for rev in [&r.inserted, &r.deleted, &r.format_changed]
        .into_iter()
        .flatten()
    {
        if !rev.id.is_empty() {
            ids.push(format!("r{}", rev.id));
        }
    }
    ids
}

/// Where each annotation is, the paragraphs numbered as the items number
/// them; the tracked changes in the order they come.
#[derive(Default)]
struct Places {
    seen: HashMap<String, (kv::FlowPlace, kv::FlowPlace)>,
    revisions: Vec<(String, kv::AnnotationKind, Revision)>,
    next: u32,
}

impl Places {
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

    fn blocks(&mut self, blocks: &[VBlock]) {
        for b in blocks {
            match b {
                VBlock::Para(p) => self.para(p),
                VBlock::Table(t) => {
                    for c in t.rows.iter().flat_map(|r| r.cells.iter()) {
                        self.blocks(&c.blocks);
                    }
                }
                VBlock::Frame(f) => self.blocks(f),
                VBlock::Placeholder(_) | VBlock::Rule => {}
            }
        }
    }

    fn para(&mut self, p: &VPara) {
        let index = p.at.is_some().then(|| {
            self.next += 1;
            self.next - 1
        });
        for r in &p.runs {
            if r.comments.is_empty()
                && r.inserted.is_none()
                && r.deleted.is_none()
                && r.format_changed.is_none()
            {
                continue;
            }
            let mut ids: Vec<String> = r.comments.iter().map(|c| format!("c{c}")).collect();
            for (rev, kind) in [
                (&r.inserted, kv::AnnotationKind::Insertion),
                (&r.deleted, kv::AnnotationKind::Deletion),
                (&r.format_changed, kv::AnnotationKind::Formatting),
            ] {
                if let Some(rev) = rev
                    && let Some(id) = self.revision(kind, rev)
                {
                    ids.push(id);
                }
            }
            if let Some(i) = index {
                let place = |offset: usize| kv::FlowPlace {
                    paragraph: i,
                    offset: offset as u32,
                };
                for id in ids {
                    self.note(id, place(r.source.start), place(r.source.end));
                }
            }
        }
        if let Some((inserted, rev)) = &p.mark_change {
            let kind = if *inserted {
                kv::AnnotationKind::Insertion
            } else {
                kv::AnnotationKind::Deletion
            };
            if let Some(id) = self.revision(kind, rev)
                && let Some(i) = index
            {
                let end = kv::FlowPlace {
                    paragraph: i,
                    offset: p.layout.text.len() as u32,
                };
                self.note(id, end, end);
            }
        }
    }
}

impl Builder {
    fn run(&self, r: &VRun, layout: &Layout) -> kv::FlowRun {
        let ids = run_ids(r);
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
            self.paras.push(at.clone());
            self.lens.push(p.layout.text.len());
            (self.base + self.paras.len() - 1) as u32
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
        let runs = p.runs.iter().map(|r| self.run(r, &p.layout)).collect();
        let annotations = p
            .mark_change
            .iter()
            .filter(|(_, rev)| !rev.id.is_empty())
            .map(|(_, rev)| format!("r{}", rev.id))
            .collect();
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
                    self.items.push(kv::FlowItem::Paragraph(fp));
                }
                VBlock::Table(t) => {
                    self.items.push(kv::FlowItem::TableStart(kv::FlowTable {
                        columns: t.grid.clone(),
                        style: t.style.clone().unwrap_or_default(),
                    }));
                    for row in &t.rows {
                        self.items
                            .push(kv::FlowItem::RowStart(kv::FlowRow { header: row.header }));
                        for c in &row.cells {
                            let border = |b: &Option<crate::flow::VBorder>| {
                                b.as_ref().map(|b| kv::FlowBorder {
                                    color: b.color.map(rgb),
                                    width: b.width,
                                    style: b.style.clone(),
                                })
                            };
                            self.items.push(kv::FlowItem::CellStart(kv::FlowCell {
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
                            self.items.push(kv::FlowItem::CellEnd);
                        }
                        self.items.push(kv::FlowItem::RowEnd);
                    }
                    self.items.push(kv::FlowItem::TableEnd);
                }
                VBlock::Frame(f) => {
                    self.items.push(kv::FlowItem::AsideStart(kv::Aside {
                        kind: kv::AsideKind::Frame,
                        id: String::new(),
                        label: "Text box".into(),
                    }));
                    self.blocks(f);
                    self.items.push(kv::FlowItem::AsideEnd);
                }
                VBlock::Placeholder(p) => self.items.push(kv::FlowItem::Placeholder(p.clone())),
                VBlock::Rule => self.items.push(kv::FlowItem::Rule("line".into())),
            }
        }
    }

    fn aside(&mut self, kind: kv::AsideKind, id: String, label: String, blocks: &[VBlock]) {
        if blocks.is_empty() {
            return;
        }
        self.items
            .push(kv::FlowItem::AsideStart(kv::Aside { kind, id, label }));
        self.blocks(blocks);
        self.items.push(kv::FlowItem::AsideEnd);
    }
}

impl Builder {
    /// The items of the header: an aside.
    fn head(&mut self, view: &DocView) {
        self.aside(
            kv::AsideKind::Header,
            "header".into(),
            String::new(),
            &view.header,
        );
    }

    /// The items of blocks of the body; each block's first item and
    /// paragraph (of these).
    fn body(&mut self, blocks: &[VBlock]) -> Vec<(usize, usize)> {
        let mut firsts = Vec::with_capacity(blocks.len() + 1);
        for b in blocks {
            firsts.push((self.items.len(), self.paras.len()));
            self.blocks(std::slice::from_ref(b));
        }
        firsts
    }

    /// The items after the body: its footer, its notes, asides.
    fn tail(&mut self, view: &DocView) {
        self.aside(
            kv::AsideKind::Footer,
            "footer".into(),
            String::new(),
            &view.footer,
        );
        for n in &view.footnotes {
            self.aside(
                kv::AsideKind::Footnote,
                format!("f{}", n.id),
                n.mark.clone(),
                &n.blocks,
            );
        }
        for n in &view.endnotes {
            self.aside(
                kv::AsideKind::Endnote,
                format!("e{}", n.id),
                n.mark.clone(),
                &n.blocks,
            );
        }
    }
}

/// The comments and tracked changes of a document's view, anchored where
/// its items are: in the body, in the blocks `marked` (each `marked`
/// block's first paragraph's index with it), the paragraphs after the
/// body numbered from `after`.
fn annotations(
    view: &DocView,
    marked: impl Iterator<Item = (usize, usize)>,
    after: usize,
) -> Vec<kv::Annotation> {
    let mut p = Places::default();
    p.blocks(&view.header);
    for (k, first) in marked {
        p.next = first as u32;
        p.blocks(std::slice::from_ref(&view.body[k]));
    }
    p.next = after as u32;
    p.blocks(&view.footer);
    for n in view.footnotes.iter().chain(&view.endnotes) {
        p.blocks(&n.blocks);
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
    // An answer is resolved with its thread, as Word shows it.
    let done: HashMap<&str, bool> = view
        .comments
        .iter()
        .map(|c| (c.id.as_str(), c.done))
        .collect();
    let mut annotations: Vec<kv::Annotation> = view
        .comments
        .iter()
        .map(|c| {
            let mut text = String::new();
            crate::flow::text(&c.blocks, &mut text);
            let id = format!("c{}", c.id);
            kv::Annotation {
                anchors: anchor(&p.seen, &id),
                id,
                kind: kv::AnnotationKind::Comment,
                author: c.author.clone(),
                date: c.date.clone(),
                text: text.trim().to_string(),
                parent: c.parent.as_ref().map(|p| format!("c{p}")),
                resolved: c.done
                    || c.parent
                        .as_deref()
                        .is_some_and(|p| done.get(p).copied().unwrap_or(false)),
            }
        })
        .collect();
    for (id, kind, rev) in &p.revisions {
        annotations.push(kv::Annotation {
            anchors: anchor(&p.seen, id),
            id: id.clone(),
            kind: *kind,
            author: rev.author.clone(),
            date: rev.date.clone(),
            text: String::new(),
            parent: None,
            resolved: false,
        });
    }
    annotations
}

/// The paragraphs' indexes of `items` moved by `by`.
fn shift_indexes(items: &mut [kv::FlowItem], by: isize) {
    if by == 0 {
        return;
    }
    for i in items {
        if let kv::FlowItem::Paragraph(p) = i
            && let Some(x) = &mut p.index
        {
            *x = x.saturating_add_signed(by as i32);
        }
    }
}

/// What changed from `old` items to `new` ones, found by comparing them:
/// the items between those the same at the start and those the same at
/// the end, the latter's indexes moved. For a flow made whole again.
pub fn compare(old: &[kv::FlowItem], new: &[kv::FlowItem]) -> ItemChange {
    let count = |items: &[kv::FlowItem]| {
        items
            .iter()
            .filter(|i| matches!(i, kv::FlowItem::Paragraph(p) if p.index.is_some()))
            .count() as isize
    };
    let shift = count(new) - count(old);
    let (same, end) = alike(old, new, shift);
    ItemChange {
        from: same,
        removed: old.len() - same - end,
        added: new.len() - same - end,
        shift,
    }
}

/// How many of `old` and `new` items are the same at the start, and how
/// many at the end, there with their paragraphs' indexes moved by
/// `shift`.
fn alike(old: &[kv::FlowItem], new: &[kv::FlowItem], shift: isize) -> (usize, usize) {
    let same = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - same;
    let moved_same = |a: &kv::FlowItem, b: &kv::FlowItem| match (a, b) {
        (kv::FlowItem::Paragraph(p), kv::FlowItem::Paragraph(q)) => {
            p.index.map(|x| x.saturating_add_signed(shift as i32)) == q.index
                && kv::FlowParagraph {
                    index: q.index,
                    ..p.clone()
                } == *q
        }
        _ => a == b,
    };
    let end = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(room)
        .take_while(|(a, b)| moved_same(a, b))
        .count();
    (same, end)
}

/// The flow and the annotations of a document's view.
pub fn build(view: &DocView) -> FlowCache {
    let mut b = Builder::default();
    b.head(view);
    let mut body = b.body(&view.body);
    body.push((b.items.len(), b.paras.len()));
    b.tail(view);
    let mut cache = FlowCache {
        items: b.items,
        paras: b.paras,
        lens: b.lens,
        annotations: Vec::new(),
        body,
        marked: view
            .body
            .iter()
            .map(|b| marked(std::slice::from_ref(b)))
            .collect(),
    };
    cache.annotate(view);
    cache
}

impl FlowCache {
    /// Brought up to date with `view`, whose body's blocks are those of
    /// before but for `blocks`, replaced (the others moved): the header
    /// and what follows the body made again, the body's blocks replaced
    /// made again, the annotations found again. What changed in the
    /// items.
    pub fn update(&mut self, view: &DocView, blocks: Option<crate::body::Replaced>) -> ItemChange {
        let (Some(&(head_items, head_paras)), Some(&(body_end, body_paras_end))) =
            (self.body.first(), self.body.last())
        else {
            let removed = self.items.len();
            *self = build(view);
            return ItemChange {
                from: 0,
                removed,
                added: self.items.len(),
                shift: 0,
            };
        };
        // The header, made again; kept when it is the same.
        let mut h = Builder::default();
        h.head(view);
        let head = (h.items != self.items[..head_items]).then_some(h);
        let dh = head
            .as_ref()
            .map_or(0, |h| h.paras.len() as isize - head_paras as isize);
        // The body's blocks replaced, made again, numbered as before.
        let mid = blocks
            .filter(|(lo, old, new)| lo != old || lo != new)
            .map(|(lo, old, new)| {
                let mut b = Builder {
                    base: self.body[lo].1,
                    ..Builder::default()
                };
                let firsts = b.body(&view.body[lo..new]);
                (lo, old, b, firsts)
            });
        let dm = mid.as_ref().map_or(0, |(lo, old, b, _)| {
            b.paras.len() as isize - (self.body[*old].1 - self.body[*lo].1) as isize
        });
        // What follows the body, made again, numbered as before; kept
        // when it is the same.
        let mut t = Builder {
            base: body_paras_end,
            ..Builder::default()
        };
        t.tail(view);
        let tail = (t.items != self.items[body_end..]).then_some(t);
        // The change: from the first part changed to the last, the
        // indexes after the last moved as it moved them.
        let mut regions: Vec<(usize, usize, usize, isize)> = Vec::new();
        if let Some(h) = &head {
            regions.push((0, head_items, h.items.len(), dh));
        }
        if let Some((lo, old, b, _)) = &mid {
            // The blocks walked again around the edit that came out the
            // same left out.
            let (at, end) = (self.body[*lo].0, self.body[*old].0);
            let (same, tail) = alike(&self.items[at..end], &b.items, dm);
            if same + tail < (end - at).max(b.items.len()) {
                regions.push((at + same, end - tail, b.items.len() - same - tail, dh + dm));
            }
        }
        if let Some(t) = &tail {
            regions.push((body_end, self.items.len(), t.items.len(), dh + dm));
        }
        let change = match (regions.first(), regions.last()) {
            (Some(first), Some(last)) => {
                let grown: isize = regions
                    .iter()
                    .map(|(at, end, n, _)| *n as isize - (end - at) as isize)
                    .sum();
                ItemChange {
                    from: first.0,
                    removed: last.1 - first.0,
                    added: last.1.saturating_add_signed(grown) - first.0,
                    shift: last.3,
                }
            }
            _ => ItemChange::default(),
        };
        // Made, from the end so that the places before stay.
        match tail {
            Some(mut t) => {
                shift_indexes(&mut t.items, dh + dm);
                self.items.splice(body_end.., t.items);
                self.paras.splice(body_paras_end.., t.paras);
                self.lens.splice(body_paras_end.., t.lens);
            }
            None => shift_indexes(&mut self.items[body_end..], dh + dm),
        }
        match mid {
            Some((lo, old, mut b, firsts)) => {
                let new = lo + firsts.len();
                let (at, p) = self.body[lo];
                let (end, pend) = self.body[old];
                shift_indexes(&mut self.items[end..body_end], dh + dm);
                shift_indexes(&mut b.items, dh);
                shift_indexes(&mut self.items[head_items..at], dh);
                let (n, np) = (b.items.len(), b.paras.len());
                self.items.splice(at..end, b.items);
                self.paras.splice(p..pend, b.paras);
                self.lens.splice(p..pend, b.lens);
                // The body's paragraphs after, as the body moved them.
                let after = p + np..body_paras_end.saturating_add_signed(dm);
                for x in &mut self.paras[after] {
                    x.index = x.index.saturating_add_signed(dm);
                }
                let di = n as isize - (end - at) as isize;
                for e in &mut self.body[old..] {
                    e.0 = e.0.saturating_add_signed(di);
                    e.1 = e.1.saturating_add_signed(dm);
                }
                self.body
                    .splice(lo..old, firsts.into_iter().map(|(i, q)| (at + i, p + q)));
                self.marked.splice(
                    lo..old,
                    view.body[lo..new]
                        .iter()
                        .map(|b| marked(std::slice::from_ref(b))),
                );
            }
            None => shift_indexes(&mut self.items[head_items..body_end], dh),
        }
        if let Some(h) = head {
            let di = h.items.len() as isize - head_items as isize;
            self.items.splice(..head_items, h.items);
            self.paras.splice(..head_paras, h.paras);
            self.lens.splice(..head_paras, h.lens);
            for e in &mut self.body {
                e.0 = e.0.saturating_add_signed(di);
                e.1 = e.1.saturating_add_signed(dh);
            }
        }
        self.annotate(view);
        change
    }

    /// The annotations found again, in the body only where they are.
    fn annotate(&mut self, view: &DocView) {
        let after = self.body.last().map_or(0, |b| b.1);
        let marked = self
            .marked
            .iter()
            .zip(&self.body)
            .enumerate()
            .filter(|(_, (m, _))| **m)
            .map(|(k, (_, b))| (k, b.1));
        self.annotations = annotations(view, marked, after);
    }
}
