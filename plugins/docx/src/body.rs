//! The body as last walked, kept up to date edit by edit (the docx
//! list's WP14a). A document of twenty thousand paragraphs takes a tenth
//! of a second to walk, its styles resolved and its lists counted, and a
//! keystroke cannot walk it again. The walk keeps, for each element of
//! the body, where it is in the part, its first block and what had been
//! counted before it, and every few elements the walk's whole state
//! ([`WalkState`]). After an edit the body is walked again from the last
//! state kept before the edit until, past it, the walk comes to an
//! element whose state was kept and is the same again: the blocks from
//! there on are those of before, moved ([`Shift`]). Their bytes are moved
//! when they are next read, so that a keystroke does not go through the
//! twenty thousand paragraphs after it.

use std::ops::Range;
use std::sync::Arc;

use kalem_ooxml::xml::{Reader, Token};

use crate::flow::{Resume, Shift, VBlock, VPara, WalkState, Walker};

/// How often the walk's state is kept: every this many elements.
const EVERY: usize = 16;

/// An element of the body as walked.
#[derive(Debug, Clone)]
struct Top {
    /// Its span in the part.
    span: Range<usize>,
    /// Its first block.
    block: usize,
    /// The paragraphs counted before it.
    paragraphs: usize,
    /// The containers numbered before it.
    containers: usize,
    /// The footnotes and endnotes referred to before it.
    notes: (usize, usize),
    /// The walk's state before it, kept every [`EVERY`] elements.
    state: Option<Box<WalkState>>,
    /// The body's own section properties (its last element).
    section: bool,
    /// Section properties among its paragraphs'.
    sects: bool,
    /// How far its blocks' edit coordinates are behind the text, in
    /// bytes: moved when they are read.
    behind: isize,
}

/// Whether a paragraph of `blocks` ends a section.
fn has_sects(blocks: &[VBlock]) -> bool {
    blocks.iter().any(|b| match b {
        VBlock::Para(p) => p.layout.sect.is_some(),
        VBlock::Table(t) => t
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .any(|c| has_sects(&c.blocks)),
        VBlock::Frame(_) | VBlock::Placeholder(_) | VBlock::Rule => false,
    })
}

/// A run of things replaced by others: `lo..old` of them then, `lo..new`
/// now, those after moved.
pub type Replaced = (usize, usize, usize);

/// `prev`, then `at` of what it gave replaced by `len` others: the two as
/// one.
pub fn then(prev: Option<Replaced>, at: Range<usize>, len: usize) -> Replaced {
    let (a, e) = (at.start, at.end);
    match prev {
        None => (a, e, a + len),
        Some((lo, old, new)) => {
            // What comes after the first is where it was, moved by it.
            let old = if e > new { old + (e - new) } else { old };
            (lo.min(a), old, new.max(e) - (e - a) + len)
        }
    }
}

/// What changed in the body's blocks since last asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    /// Nothing.
    Nothing,
    /// Some blocks replaced by others.
    Blocks(Replaced),
    /// Everything: walked again whole.
    All,
}

/// The body as walked.
#[derive(Debug, Clone)]
pub struct Body {
    /// Its blocks.
    pub blocks: Arc<Vec<VBlock>>,
    /// The footnotes referred to, in order, with their marks.
    pub footnotes: Vec<(String, String)>,
    /// The endnotes referred to.
    pub endnotes: Vec<(String, String)>,
    /// What changed in its blocks since last asked.
    pub changed: Changed,
    tops: Vec<Top>,
    /// Its content: from after its start tag to its end tag.
    content: Range<usize>,
    /// Its container's number.
    container: usize,
}

impl Default for Body {
    fn default() -> Body {
        Body {
            blocks: Arc::default(),
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            changed: Changed::All,
            tops: Vec::new(),
            content: 0..0,
            container: 0,
        }
    }
}

/// Where a walk stopped.
enum End {
    /// At an element of before (its index), the walk where it was then.
    Met(usize),
    /// At the body's end tag (where it starts).
    Body(usize),
}

/// What a walk gave.
struct Walked {
    tops: Vec<Top>,
    blocks: Vec<VBlock>,
    end: End,
}

/// Walks the body's elements from byte `pos` of `text`, the first of
/// them the `i`th of the body, their blocks numbered from `block`; till
/// the body's end, or an element `met` says the walk is where it was
/// before.
fn walk(
    w: &mut Walker<'_>,
    text: &str,
    pos: usize,
    (container, mut i): (usize, usize),
    block: usize,
    mut met: impl FnMut(usize, &Walker<'_>) -> Option<usize>,
) -> Walked {
    let mut r = Reader::at(text, pos);
    let mut tops: Vec<Top> = Vec::new();
    let mut blocks = Vec::new();
    let end = loop {
        match r.next_token() {
            Some(Token::Start(tag)) => {
                let at = tag.span.start;
                if let Some(m) = met(at, w) {
                    break End::Met(m);
                }
                let (paragraphs, containers) = w.counted();
                let state = tops
                    .len()
                    .is_multiple_of(EVERY)
                    .then(|| Box::new(w.state()));
                let first = block + blocks.len();
                let b = crate::story::block(&mut r, tag);
                tops.push(Top {
                    span: at..r.pos(),
                    block: first,
                    paragraphs,
                    containers,
                    notes: (w.footnotes.len(), w.endnotes.len()),
                    state,
                    section: matches!(b, crate::story::Block::Section(_)),
                    sects: false,
                    behind: 0,
                });
                w.top(&b, (container, i), &mut blocks);
                if let Some(t) = tops.last_mut() {
                    t.sects = has_sects(&blocks[first - block..]);
                }
                i += 1;
            }
            Some(Token::End { span, .. }) => break End::Body(span.start),
            Some(Token::Text { .. }) => {}
            None => break End::Body(text.len()),
        }
    };
    Walked { tops, blocks, end }
}

impl Body {
    /// The body of a main document part's text, walked whole.
    pub fn walk(w: &mut Walker<'_>, text: &str) -> Body {
        let mut r = Reader::new(text);
        let start = loop {
            match r.next_token() {
                Some(Token::Start(t)) if t.name == "body" => {
                    if t.empty {
                        return Body::default();
                    }
                    break t.span.end;
                }
                None => return Body::default(),
                _ => {}
            }
        };
        let container = w.container();
        let walked = walk(w, text, start, (container, 0), 0, |_, _| None);
        let end = match walked.end {
            End::Body(end) => end,
            End::Met(_) => text.len(),
        };
        Body {
            blocks: Arc::new(walked.blocks),
            footnotes: std::mem::take(&mut w.footnotes),
            endnotes: std::mem::take(&mut w.endnotes),
            changed: Changed::All,
            tops: walked.tops,
            content: start..end,
            container,
        }
    }

    /// Blocks `at` replaced by `len` others.
    fn replaced(&mut self, at: Range<usize>, len: usize) {
        self.changed = match self.changed {
            Changed::Nothing => Changed::Blocks(then(None, at, len)),
            Changed::Blocks(prev) => Changed::Blocks(then(Some(prev), at, len)),
            Changed::All => Changed::All,
        };
    }

    /// Brought up to date after an edit replaced bytes `lo..old` of the
    /// part with bytes `lo..new` of `text`, by `w`, a walker of the
    /// body; false when the body is to be walked again whole.
    pub fn update(
        &mut self,
        w: &mut Walker<'_>,
        text: &str,
        lo: usize,
        old: usize,
        new: usize,
    ) -> bool {
        if self.tops.is_empty() || lo < self.content.start || old > self.content.end {
            return false;
        }
        let delta = new as isize - old as isize;
        // The last element before the edit whose state was kept (the
        // first always is).
        let mut r = self
            .tops
            .partition_point(|t| t.span.start <= lo)
            .saturating_sub(1);
        while r > 0 && self.tops[r].state.is_none() {
            r -= 1;
        }
        let first = &self.tops[r];
        let Some(state) = first.state.as_deref() else {
            return false;
        };
        w.resume(
            state,
            Resume {
                paragraphs: first.paragraphs,
                containers: first.containers,
                footnotes: self.footnotes[..first.notes.0].to_vec(),
                endnotes: self.endnotes[..first.notes.1].to_vec(),
            },
        );
        let from = if r == 0 {
            self.content.start
        } else {
            first.span.start
        };
        let (tops, footnotes, endnotes) = (&self.tops, &self.footnotes, &self.endnotes);
        // Past the edit, an element of before whose state was kept: met
        // when the walk is where it was then.
        let walked = walk(w, text, from, (self.container, r), first.block, |at, w| {
            if at < new {
                return None;
            }
            let m = tops
                .binary_search_by_key(&at.checked_add_signed(-delta)?, |t| t.span.start)
                .ok()?;
            let t = &tops[m];
            let same = w.is_at(t.state.as_deref()?)
                && w.footnotes[..] == footnotes[..t.notes.0]
                && w.endnotes[..] == endnotes[..t.notes.1];
            same.then_some(m)
        });
        let first = self.tops[r].block;
        let blocks = Arc::make_mut(&mut self.blocks);
        match walked.end {
            End::Met(m) => {
                let met = &self.tops[m];
                let (paragraphs, containers) = w.counted();
                let shift = Shift {
                    bytes: delta,
                    paragraphs: paragraphs as isize - met.paragraphs as isize,
                    body: self.container,
                    tops: walked.tops.len() as isize - (m - r) as isize,
                    containers: containers as isize - met.containers as isize,
                };
                let n = walked.blocks.len();
                let moved = n as isize - (met.block - first) as isize;
                let replaced = first..met.block;
                blocks.splice(replaced.clone(), walked.blocks);
                // Places and numbers moved now (an edit adding or taking
                // away paragraphs, seldom), bytes when read.
                if shift.paragraphs != 0 || shift.tops != 0 || shift.containers != 0 {
                    Shift { bytes: 0, ..shift }.blocks(&mut blocks[first + n..]);
                }
                for t in &mut self.tops[m..] {
                    t.behind += delta;
                    t.span.start = t.span.start.saturating_add_signed(delta);
                    t.span.end = t.span.end.saturating_add_signed(delta);
                    t.block = t.block.saturating_add_signed(moved);
                    t.paragraphs = t.paragraphs.saturating_add_signed(shift.paragraphs);
                    t.containers = t.containers.saturating_add_signed(shift.containers);
                }
                self.tops.splice(r..m, walked.tops);
                self.content.end = self.content.end.saturating_add_signed(delta);
                self.replaced(replaced, n);
            }
            End::Body(end) => {
                let replaced = first..blocks.len();
                let n = walked.blocks.len();
                blocks.truncate(first);
                blocks.extend(walked.blocks);
                self.replaced(replaced, n);
                self.tops.truncate(r);
                self.tops.extend(walked.tops);
                self.footnotes = std::mem::take(&mut w.footnotes);
                self.endnotes = std::mem::take(&mut w.endnotes);
                self.content.end = end;
            }
        }
        true
    }

    /// Every `w:sectPr` of the body, in order: the paragraphs' that end a
    /// section, then the body's own.
    pub fn sections(&self) -> Vec<Range<usize>> {
        fn paras(blocks: &[VBlock], behind: isize, out: &mut Vec<Range<usize>>) {
            for b in blocks {
                match b {
                    VBlock::Para(p) => out.extend(p.layout.sect.as_ref().map(|s| {
                        s.start.saturating_add_signed(behind)..s.end.saturating_add_signed(behind)
                    })),
                    VBlock::Table(t) => {
                        for c in t.rows.iter().flat_map(|r| &r.cells) {
                            paras(&c.blocks, behind, out);
                        }
                    }
                    VBlock::Frame(_) | VBlock::Placeholder(_) | VBlock::Rule => {}
                }
            }
        }
        let mut out = Vec::new();
        for (i, t) in self.tops.iter().enumerate() {
            if t.section {
                out.push(t.span.clone());
            } else if t.sects {
                paras(&self.blocks[self.blocks_of(i)], t.behind, &mut out);
            }
        }
        out
    }

    /// The blocks of element `i`.
    fn blocks_of(&self, i: usize) -> Range<usize> {
        let end = self.tops.get(i + 1).map_or(self.blocks.len(), |n| n.block);
        self.tops[i].block..end
    }

    /// The element holding paragraph `index` of the body (its
    /// [`crate::ParaAt::index`]).
    fn top_of(&self, index: usize) -> Option<usize> {
        let t = self
            .tops
            .partition_point(|t| t.paragraphs <= index)
            .saturating_sub(1);
        (t < self.tops.len()).then_some(t)
    }

    /// Paragraph `at` of the body as shown, with its edit coordinates.
    pub fn paragraph(&self, at: &crate::ParaAt) -> Option<VPara> {
        let t = self.top_of(at.index)?;
        let mut p = crate::document::find_para(&self.blocks[self.blocks_of(t)], at)?.clone();
        let behind = self.tops[t].behind;
        if behind != 0 {
            Shift {
                bytes: behind,
                ..Shift::default()
            }
            .para(&mut p);
        }
        Some(p)
    }

    /// The blocks with their edit coordinates moved where the text is.
    pub fn settle(&mut self) {
        if self.tops.iter().all(|t| t.behind == 0) {
            return;
        }
        let blocks = Arc::make_mut(&mut self.blocks);
        for i in 0..self.tops.len() {
            let behind = std::mem::take(&mut self.tops[i].behind);
            if behind != 0 {
                let end = self.tops.get(i + 1).map_or(blocks.len(), |n| n.block);
                Shift {
                    bytes: behind,
                    ..Shift::default()
                }
                .blocks(&mut blocks[self.tops[i].block..end]);
            }
        }
    }
}
