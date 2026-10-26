//! The layer over Kalem's Markdown and Org (Kalem's plugin API 0.2.10,
//! `graph_todo.md` GR4 to GR7): what a note shows as Logseq and Obsidian
//! show it, as overlays by byte range that Kalem applies to its own
//! drawing of the note. Kalem's Markdown draws the Markdown (emphasis,
//! links, wiki links, tables, lists); the layer hides what the
//! applications hide (a block's `id::` and `collapsed::` lines, a `^id`, a
//! `%%comment%%`), shows a block reference as its block's text, styles
//! tags, task keywords and dates, folds a `:LOGBOOK:` and shows a
//! block's number of references at its first line's end.
//!
//! Pure: the text and, when the graph is indexed, the index in, the
//! overlays out; the component turns them into the API's records.

use crate::config::Kind;
use crate::index::Index;
use crate::rows::{self, Row};
use crate::scan::{self, Flavor, MARKERS, RefKind};

/// A style's parts, added to what Kalem's mode draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Struck through.
    pub strike: bool,
    /// Monospace on a background.
    pub code: bool,
    /// A link.
    pub link: bool,
    /// Dimmed.
    pub dim: bool,
    /// A tag.
    pub tag: bool,
    /// An open task's keyword.
    pub todo: bool,
    /// A done task's keyword.
    pub done: bool,
    /// A date.
    pub timestamp: bool,
    /// A priority.
    pub priority: bool,
}

/// What a span becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Hidden away from the cursor.
    Hide,
    /// Shown as other text away from the cursor.
    Replace(String, Look),
    /// Drawn in a style.
    Style(Look),
}

/// A span and what it becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// From.
    pub start: usize,
    /// To (left out).
    pub end: usize,
    /// What it becomes.
    pub effect: Effect,
}

/// What whole lines become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEffect {
    /// Not shown away from the cursor.
    Hidden,
    /// Folded to their first line away from the cursor.
    Folded,
}

/// Whole lines and what they become.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lines {
    /// A line's start.
    pub start: usize,
    /// The start of the line after the last.
    pub end: usize,
    /// What they become.
    pub effect: LineEffect,
}

/// Rows drawn under the line holding byte `at`, not part of the text
/// (Kalem's API 0.2.13): an embed's blocks, a query's results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Below {
    /// A byte of the line.
    pub at: usize,
    /// The rows.
    pub rows: Vec<Row>,
}

/// Text shown at byte `at` of a line, after what ends there, not part of
/// the text (Kalem's API 0.2.20): a block's number of references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insert {
    /// The byte.
    pub at: usize,
    /// The text.
    pub text: String,
    /// Its look.
    pub look: Look,
}

/// The overlays of a note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overlays {
    /// Spans, in order, not overlapping.
    pub spans: Vec<Span>,
    /// Lines, in order, not overlapping.
    pub lines: Vec<Lines>,
    /// Rows under lines, in order.
    pub below: Vec<Below>,
    /// Text at places of lines, in order; those at one place in the
    /// order given.
    pub inserts: Vec<Insert>,
}

impl Overlays {
    /// Rows under the line holding byte `at`, when there are any.
    fn rows(&mut self, at: usize, rows: Vec<Row>) {
        if !rows.is_empty() {
            self.below.push(Below { at, rows });
        }
    }

    fn span(&mut self, start: usize, end: usize, effect: Effect) {
        if start < end {
            self.spans.push(Span { start, end, effect });
        }
    }

    /// A span over the ones it overlaps, which are left out.
    fn put(&mut self, start: usize, end: usize, effect: Effect) {
        self.spans.retain(|s| s.end <= start || s.start >= end);
        self.span(start, end, effect);
    }

    /// In order, the later of two overlapping ones left out.
    fn finish(mut self) -> Overlays {
        self.spans.sort_by_key(|s| (s.start, s.end));
        let mut end = 0;
        self.spans.retain(|s| {
            let keep = s.start >= end;
            if keep {
                end = s.end;
            }
            keep
        });
        self.below.sort_by_key(|b| b.at);
        self.inserts.sort_by_key(|i| i.at);
        self.lines.sort_by_key(|l| l.start);
        let mut end = 0;
        self.lines.retain(|l| {
            let keep = l.start >= end;
            if keep {
                end = l.end;
            }
            keep
        });
        self
    }
}

/// The properties Logseq hides in a block.
const HIDDEN: &[&str] = &[
    "id",
    "collapsed",
    "heading",
    "created-at",
    "updated-at",
    "query-table",
    "query-properties",
    "query-sort-by",
    "query-sort-desc",
    "card-last-interval",
    "card-repeats",
    "card-ease-factor",
    "card-next-schedule",
    "card-last-reviewed",
    "card-last-score",
    "custom-id",
    "background-color",
    "last-modified-at",
    "created_at",
    "last_modified_at",
    // A PDF highlight's (`hls__` pages).
    "ls-type",
    "hl-type",
    "hl-page",
    "hl-stamp",
    "hl-color",
];

/// Whether property `key` is one Logseq hides.
pub(crate) fn hidden_property(key: &str, extra: &[String]) -> bool {
    HIDDEN.contains(&key) || key.starts_with("logseq.") || extra.iter().any(|e| e == key)
}

/// The lines of `text` with their starts.
fn lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    for l in text.split_inclusive('\n') {
        out.push((at, l.strip_suffix('\n').unwrap_or(l)));
        at += l.len();
    }
    if text.is_empty() || text.ends_with('\n') {
        out.push((text.len(), ""));
    }
    out
}

/// The end of line `l` with its line feed.
fn line_end(text: &str, start: usize, line: &str) -> usize {
    (start + line.len() + 1).min(text.len())
}

/// The text a block reference shows: its block's first line, its own
/// references kept as written.
fn block_text(index: Option<&Index>, id: &str) -> Option<String> {
    let (_, b) = index?.block(id)?;
    let t = b.text.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// The title of the page block `id` is in: what an embed of the block
/// names on its line, the block itself shown under it.
fn block_page(index: Option<&Index>, id: &str) -> Option<String> {
    let index = index?;
    let (rel, _) = index.block_place(id)?;
    index.page_of(rel).map(|p| p.shown_title().to_string())
}

/// A task keyword's look.
fn keyword_look(marker: &str) -> Look {
    if scan::is_closed(marker) {
        Look {
            done: true,
            ..Look::default()
        }
    } else {
        Look {
            todo: true,
            ..Look::default()
        }
    }
}

/// The references, tags and macros of one line of content starting at
/// byte `at`.
fn inline(
    o: &mut Overlays,
    line: &str,
    at: usize,
    flavor: Flavor,
    index: Option<&Index>,
    rel: Option<&str>,
) {
    for r in scan_refs(line, flavor) {
        let (start, end) = (at + r.start as usize, at + r.end as usize);
        match r.kind {
            RefKind::Tag => o.span(
                start,
                end,
                Effect::Style(Look {
                    tag: true,
                    ..Look::default()
                }),
            ),
            RefKind::Block if flavor != Flavor::Obsidian => {
                let text = block_text(index, &r.target);
                match text {
                    Some(t) => o.span(
                        start,
                        end,
                        Effect::Replace(
                            t,
                            Look {
                                link: true,
                                ..Look::default()
                            },
                        ),
                    ),
                    None => o.span(
                        start,
                        end,
                        Effect::Style(Look {
                            dim: true,
                            ..Look::default()
                        }),
                    ),
                }
            }
            RefKind::EmbedBlock | RefKind::EmbedPage => {
                let shown = match (r.kind, flavor) {
                    (RefKind::EmbedBlock, Flavor::Obsidian) => {
                        let key = index.zip(rel).map(|(i, rel)| i.resolve(&r.target, rel));
                        let id = key.map(|k| {
                            format!("{k}#^{}", r.anchor.as_deref().unwrap_or("").to_lowercase())
                        });
                        if let (Some(i), Some(id)) = (index, &id) {
                            o.rows(start, rows::embedded_block(i, id));
                        }
                        id.and_then(|id| block_page(index, &id))
                            .unwrap_or_else(|| r.target.clone())
                    }
                    (RefKind::EmbedBlock, _) => {
                        if let Some(i) = index {
                            o.rows(start, rows::embedded_block(i, &r.target));
                        }
                        block_page(index, &r.target).unwrap_or_else(|| "a block".into())
                    }
                    _ => {
                        if let (Some(i), Some(rel)) = (index, rel) {
                            let key = i.resolve(&r.target, rel);
                            o.rows(start, rows::embedded_page(i, &key, r.anchor.as_deref()));
                        }
                        match &r.anchor {
                            Some(a) => format!("{} › {a}", r.target),
                            None => r.target.clone(),
                        }
                    }
                };
                o.span(
                    start,
                    end,
                    Effect::Replace(
                        format!("↳ {shown}"),
                        Look {
                            link: true,
                            ..Look::default()
                        },
                    ),
                );
            }
            _ => {}
        }
    }
    if flavor != Flavor::Obsidian {
        highlights(o, line, at, "^^");
    }
}

/// A line's highlights between `mark`s (Logseq's `^^`, Obsidian's
/// `==`): the marks hidden, the text bold.
fn highlights(o: &mut Overlays, line: &str, at: usize, mark: &str) {
    let w = mark.len();
    let mut from = 0;
    while let Some(p) = line[from..].find(mark) {
        let s = from + p;
        let Some(q) = line[s + w..].find(mark) else {
            break;
        };
        let e = s + w + q;
        if q > 0 && !line[s + w..e].starts_with(' ') {
            o.span(at + s, at + s + w, Effect::Hide);
            o.span(
                at + s + w,
                at + e,
                Effect::Style(Look {
                    bold: true,
                    ..Look::default()
                }),
            );
            o.span(at + e, at + e + w, Effect::Hide);
        }
        from = e + w;
    }
}

/// An admonition of Logseq's (`#+BEGIN_NOTE`) as it is shown: its icon
/// and name.
fn admonition(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "note" => "✎ Note",
        "tip" => "✦ Tip",
        "important" => "❗ Important",
        "caution" => "⚠ Caution",
        "warning" => "⚠ Warning",
        "pinned" => "📌 Pinned",
        _ => return None,
    })
}

/// A `#+BEGIN_…` or `#+END_…` line's marker at byte `at` (`marker`
/// trimmed): an admonition's beginning shown as its icon and name and
/// its end's line hidden, as Logseq draws the box; another block's
/// marker dimmed. False when `marker` is neither.
fn block_marker(
    o: &mut Overlays,
    text: &str,
    line: (usize, &str),
    at: usize,
    marker: &str,
) -> bool {
    let upper = marker.to_ascii_uppercase();
    let (name, begin) = if let Some(n) = upper.strip_prefix("#+BEGIN_") {
        (n, true)
    } else if let Some(n) = upper.strip_prefix("#+END_") {
        (n, false)
    } else {
        return false;
    };
    let name = name.split_whitespace().next().unwrap_or("");
    match admonition(name) {
        Some(label) if begin => o.span(
            at,
            at + marker.len(),
            Effect::Replace(
                label.into(),
                Look {
                    bold: true,
                    ..Look::default()
                },
            ),
        ),
        Some(_) if line.1.trim() == marker => o.lines.push(Lines {
            start: line.0,
            end: line_end(text, line.0, line.1),
            effect: LineEffect::Hidden,
        }),
        _ => o.span(
            at,
            at + marker.len(),
            Effect::Style(Look {
                dim: true,
                ..Look::default()
            }),
        ),
    }
    true
}

/// The references of a line, as the scanner finds them.
fn scan_refs(line: &str, flavor: Flavor) -> Vec<scan::Ref> {
    scan::references(line, flavor)
}

/// A line's `key:: value` property: the key's bytes (with `::`) and the
/// key in lower case.
fn property(line: &str) -> Option<(usize, usize, String)> {
    let start = line.len() - line.trim_start().len();
    let t = &line[start..];
    let p = t.find("::")?;
    let key = &t[..p];
    let rest = &t[p + 2..];
    let ok = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '?' | '!' | '/' | '*'))
        && (rest.is_empty() || rest.starts_with([' ', '\t']));
    ok.then(|| (start, start + p + 2, key.to_lowercase()))
}

/// A Logseq note in Markdown.
pub fn logseq_markdown(text: &str, index: Option<&Index>, rel: Option<&str>) -> Overlays {
    let extra: Vec<String> = index
        .map(|i| i.graph.hidden_properties.clone())
        .unwrap_or_default();
    let mut o = Overlays::default();
    let ls = lines(text);
    let mut fence = false;
    let mut drawer: Option<usize> = None;
    // A block's first line seen, its properties may follow.
    let mut in_props = false;
    for (n, (start, line)) in ls.iter().enumerate() {
        let (start, line) = (*start, *line);
        let indent = line.len() - line.trim_start().len();
        let body = &line[indent..];
        let trimmed = body.trim_end();
        if fence {
            if trimmed.starts_with("```") {
                fence = false;
            }
            continue;
        }
        if let Some(d) = drawer {
            if trimmed == ":END:" {
                o.lines.push(Lines {
                    start: d,
                    end: line_end(text, start, line),
                    effect: LineEffect::Folded,
                });
                o.span(
                    ls[n].0 + indent,
                    ls[n].0 + indent + trimmed.len(),
                    Effect::Style(Look {
                        dim: true,
                        ..Look::default()
                    }),
                );
                drawer = None;
            }
            continue;
        }
        if trimmed == ":LOGBOOK:" {
            drawer = Some(start);
            o.span(
                start + indent,
                start + indent + trimmed.len(),
                Effect::Style(Look {
                    dim: true,
                    ..Look::default()
                }),
            );
            continue;
        }
        // A property as a list item, as the Markdown Mirror of Logseq's
        // database graphs writes it: drawn as a property line is.
        if let Some(after) = body.strip_prefix("* ")
            && let Some((ks, ke, key)) = property(after)
        {
            let base = start + indent + 2;
            property_look(&mut o, text, base, ks, ke, &key, &extra, line, start, false);
            if !hidden_property(&key, &extra) {
                inline(
                    &mut o,
                    &after[ke..],
                    base + ke,
                    Flavor::LogseqMarkdown,
                    index,
                    rel,
                );
            }
            continue;
        }
        let bullet = trimmed == "-" || body.starts_with("- ");
        if bullet {
            in_props = true;
            let content_at = indent + 2.min(body.len());
            let content = &line[content_at.min(line.len())..];
            if content.trim_start().starts_with("```") && !content.trim_start()[3..].contains("```")
            {
                fence = true;
            }
            // The task keyword and the priority.
            let mut at = content_at;
            for m in MARKERS {
                if let Some(after) = content.strip_prefix(m)
                    && (after.is_empty() || after.starts_with(' '))
                {
                    o.span(
                        start + at,
                        start + at + m.len(),
                        Effect::Style(keyword_look(m)),
                    );
                    at += m.len() + 1;
                    break;
                }
            }
            let rest = line.get(at..).unwrap_or("");
            if rest.len() >= 4 && rest.starts_with("[#") && rest.as_bytes()[3] == b']' {
                o.span(
                    start + at,
                    start + at + 4,
                    Effect::Style(Look {
                        priority: true,
                        ..Look::default()
                    }),
                );
            }
            if block_marker(&mut o, text, (start, line), start + at, rest.trim_end()) {
                continue;
            }
            if let Some((ks, ke, key)) = property(content) {
                // A pre-block's first property.
                property_look(
                    &mut o,
                    text,
                    start + content_at,
                    ks,
                    ke,
                    &key,
                    &extra,
                    line,
                    start,
                    true,
                );
            } else {
                inline(
                    &mut o,
                    content,
                    start + content_at,
                    Flavor::LogseqMarkdown,
                    index,
                    rel,
                );
            }
            continue;
        }
        if trimmed.starts_with("```") {
            fence = true;
            in_props = false;
            continue;
        }
        if in_props && let Some((ks, ke, key)) = property(line) {
            property_look(
                &mut o, text, start, ks, ke, &key, &extra, line, start, false,
            );
            continue;
        }
        if n == 0 || ls[..n].iter().all(|(_, l)| property(l).is_some()) {
            // The page's properties before the first bullet.
            if let Some((ks, ke, key)) = property(line) {
                property_look(
                    &mut o, text, start, ks, ke, &key, &extra, line, start, false,
                );
                continue;
            }
        }
        if trimmed.starts_with("SCHEDULED:") || trimmed.starts_with("DEADLINE:") {
            o.span(
                start + indent,
                start + indent + trimmed.len(),
                Effect::Style(Look {
                    timestamp: true,
                    dim: true,
                    ..Look::default()
                }),
            );
            continue;
        }
        in_props = false;
        if block_marker(&mut o, text, (start, line), start + indent, trimmed) {
            continue;
        }
        inline(
            &mut o,
            body,
            start + indent,
            Flavor::LogseqMarkdown,
            index,
            rel,
        );
    }
    // A block with `collapsed:: true` shows its first lines only: the
    // blocks under it hidden away from the cursor, as Logseq folds it.
    let s = scan::scan(text, Flavor::LogseqMarkdown, &scan::Options::default());
    let starts: Vec<usize> = ls.iter().map(|(s, _)| *s).collect();
    let at = |n: u32| starts.get(n as usize).copied().unwrap_or(text.len());
    numbered(&mut o, &s.blocks, &ls);
    ref_counts(&mut o, &s.blocks, &ls, index);
    for (i, b) in s.blocks.iter().enumerate() {
        if b.collapsed && s.blocks.get(i + 1).is_some_and(|c| c.parent == Some(i)) {
            // Its bullet a closed arrow, as Logseq marks a folded block.
            let ls = at(b.line);
            let line = &text[ls..at(b.line + 1).max(ls)];
            let lead = line.len() - line.trim_start().len();
            if line[lead..].starts_with("- ") || line[lead..].starts_with("* ") {
                o.span(
                    ls + lead,
                    ls + lead + 2,
                    Effect::Replace(
                        "▸ ".into(),
                        Look {
                            dim: true,
                            ..Look::default()
                        },
                    ),
                );
            }
            let (_, end) = crate::edit::subtree(&s.blocks, i);
            let from = at(s.blocks[i + 1].line);
            let to = at(end);
            if from < to {
                o.lines.push(Lines {
                    start: from,
                    end: to,
                    effect: LineEffect::Hidden,
                });
            }
        }
    }
    o.finish()
}

/// After the first line of each block with an id, the number of blocks
/// referring to it, as Logseq shows it beside the block (not where the
/// block is embedded): a block referring to it twice, or embedding it,
/// counts once (Kalem's API 0.2.20, the work list's K6).
fn ref_counts(
    o: &mut Overlays,
    blocks: &[scan::Block],
    ls: &[(usize, &str)],
    index: Option<&Index>,
) {
    let Some(index) = index else {
        return;
    };
    for b in blocks {
        let Some(id) = &b.id else {
            continue;
        };
        let refs = index.block_backlinks(id);
        let Some((start, line)) = ls.get(b.line as usize) else {
            continue;
        };
        if refs.is_empty() {
            continue;
        }
        // Each block once; a reference in no block, each line once.
        let from: std::collections::HashSet<(&str, Option<usize>, u32)> = refs
            .iter()
            .map(|r| {
                (
                    &*r.path,
                    r.block,
                    if r.block.is_some() { 0 } else { r.line },
                )
            })
            .collect();
        let at = start + line.strip_suffix('\r').unwrap_or(line).len();
        o.inserts.push(Insert {
            at,
            text: " ".into(),
            look: Look::default(),
        });
        o.inserts.push(Insert {
            at,
            text: from.len().to_string(),
            look: Look {
                code: true,
                ..Look::default()
            },
        });
    }
}

/// The blocks with `logseq.order-list-type:: number` numbered as Logseq
/// numbers them: their bullets shown as `1.`, counting the siblings
/// before with the property; nested in such blocks, `a.`, then `I.`.
fn numbered(o: &mut Overlays, blocks: &[scan::Block], ls: &[(usize, &str)]) {
    let number = |b: &scan::Block| {
        b.props
            .iter()
            .any(|(k, v)| k == "logseq.order-list-type" && v.trim().eq_ignore_ascii_case("number"))
    };
    let mut idx = vec![0usize; blocks.len()];
    // The last block seen under each parent (`None` at the top level).
    let mut last: std::collections::HashMap<Option<usize>, usize> = Default::default();
    for (i, b) in blocks.iter().enumerate() {
        let prev = last.insert(b.parent, i);
        if !number(b) {
            continue;
        }
        idx[i] = prev.filter(|&p| idx[p] > 0).map_or(1, |p| idx[p] + 1);
        let mut depth = 0;
        let mut p = b.parent;
        while let Some(q) = p.filter(|&q| number(&blocks[q])) {
            depth += 1;
            p = blocks[q].parent;
        }
        let label = match depth % 3 {
            0 => idx[i].to_string(),
            1 => letters(idx[i]),
            _ => roman(idx[i]),
        };
        let Some((start, line)) = ls.get(b.line as usize) else {
            continue;
        };
        let indent = line.len() - line.trim_start().len();
        if line[indent..].starts_with('-') {
            o.put(
                start + indent,
                start + indent + 1,
                Effect::Replace(format!("{label}."), Look::default()),
            );
        }
    }
}

/// 1 as `a`, 27 as `aa`.
fn letters(mut n: usize) -> String {
    let mut s = Vec::new();
    while n > 0 {
        let t = (n - 1) % 26;
        s.push(b'a' + t as u8);
        n = (n - t) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// 4 as `IV`.
fn roman(mut n: usize) -> String {
    const R: &[(usize, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for &(v, r) in R {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// A property line: hidden whole when Logseq hides it, else its key
/// dimmed and its value's references styled.
#[allow(clippy::too_many_arguments)]
fn property_look(
    o: &mut Overlays,
    text: &str,
    base: usize,
    ks: usize,
    ke: usize,
    key: &str,
    extra: &[String],
    line: &str,
    line_start: usize,
    on_bullet: bool,
) {
    if hidden_property(key, extra) && !on_bullet {
        o.lines.push(Lines {
            start: line_start,
            end: line_end(text, line_start, line),
            effect: LineEffect::Hidden,
        });
        return;
    }
    o.span(
        base + ks,
        base + ke,
        Effect::Style(Look {
            dim: true,
            ..Look::default()
        }),
    );
}

/// An Obsidian note.
pub fn obsidian(text: &str, index: Option<&Index>, rel: Option<&str>) -> Overlays {
    let mut o = Overlays::default();
    let ls = lines(text);
    let mut fence = false;
    let mut comment: Option<usize> = None;
    let mut front = false;
    for (n, (start, line)) in ls.iter().enumerate() {
        let (start, line) = (*start, *line);
        let trimmed = line.trim();
        if n == 0 && trimmed == "---" {
            front = true;
            continue;
        }
        if front {
            if trimmed == "---" || trimmed == "..." {
                front = false;
            }
            continue;
        }
        if let Some(c) = comment {
            if let Some(p) = line.find("%%") {
                // A comment over lines: hidden whole when its last line
                // ends with it, else up to its `%%`.
                if line[p + 2..].trim().is_empty() {
                    o.lines.push(Lines {
                        start: c,
                        end: line_end(text, start, line),
                        effect: LineEffect::Hidden,
                    });
                } else {
                    o.span(start, start + p + 2, Effect::Hide);
                }
                comment = None;
                inline_obsidian(&mut o, &line[p + 2..], start + p + 2, index, rel);
            }
            continue;
        }
        if fence {
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fence = false;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = true;
            continue;
        }
        // `%%` on this line.
        let mut at = 0;
        let mut rest = line;
        while let Some(p) = rest.find("%%") {
            match rest[p + 2..].find("%%") {
                Some(q) => {
                    let s = start + at + p;
                    let e = s + 2 + q + 2;
                    o.span(s, e, Effect::Hide);
                    at += p + 2 + q + 2;
                    rest = &line[at..];
                }
                None => {
                    if line[..at + p].trim().is_empty() {
                        comment = Some(start);
                    } else {
                        comment = Some(start);
                        o.span(start + at + p, start + line.len(), Effect::Hide);
                    }
                    break;
                }
            }
        }
        if comment.is_some() {
            continue;
        }
        // A callout's first line: `> [!type]` shown as its type.
        let q = line.trim_start();
        if let Some(after) = q.strip_prefix('>') {
            let after_t = after.trim_start();
            if let Some(body) = after_t.strip_prefix("[!")
                && let Some(close) = body.find(']')
            {
                let kind = &body[..close];
                let marker_start =
                    start + (line.len() - q.len()) + 1 + (after.len() - after_t.len());
                let mut marker_end = marker_start + 2 + close + 1;
                let fold = body[close + 1..].chars().next();
                if matches!(fold, Some('-') | Some('+')) {
                    marker_end += 1;
                }
                o.span(
                    marker_start,
                    marker_end,
                    Effect::Replace(
                        callout_label(kind),
                        Look {
                            bold: true,
                            ..Look::default()
                        },
                    ),
                );
                if fold == Some('-') {
                    let end = ls[n + 1..]
                        .iter()
                        .take_while(|(_, l)| l.trim_start().starts_with('>'))
                        .last()
                        .map_or(line_end(text, start, line), |(s, l)| line_end(text, *s, l));
                    o.lines.push(Lines {
                        start,
                        end,
                        effect: LineEffect::Folded,
                    });
                }
                inline_obsidian(&mut o, &line[marker_end - start..], marker_end, index, rel);
                continue;
            }
        }
        // Dataview's `key:: value`.
        if let Some((ks, ke, _)) = property(line) {
            o.span(
                start + ks,
                start + ke,
                Effect::Style(Look {
                    dim: true,
                    ..Look::default()
                }),
            );
        }
        inline_obsidian(&mut o, line, start, index, rel);
    }
    o.finish()
}

/// A callout type's label.
fn callout_label(kind: &str) -> String {
    let k = kind.trim().to_lowercase();
    let (icon, name) = match k.as_str() {
        "note" => ("✎", "Note"),
        "abstract" | "summary" | "tldr" => ("☰", "Summary"),
        "info" => ("ℹ", "Info"),
        "todo" => ("☐", "To do"),
        "tip" | "hint" | "important" => ("✦", "Tip"),
        "success" | "check" | "done" => ("✓", "Done"),
        "question" | "help" | "faq" => ("?", "Question"),
        "warning" | "caution" | "attention" => ("⚠", "Warning"),
        "failure" | "fail" | "missing" => ("✗", "Failure"),
        "danger" | "error" => ("⚡", "Danger"),
        "bug" => ("🐞", "Bug"),
        "example" => ("▤", "Example"),
        "quote" | "cite" => ("❝", "Quote"),
        _ => ("•", ""),
    };
    if name.is_empty() {
        let mut c = kind.trim().chars();
        let title: String = c
            .next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        format!("{icon} {title}")
    } else {
        format!("{icon} {name}")
    }
}

/// One line of an Obsidian note: references, tags, `==highlights==` and a
/// `^id` at its end.
fn inline_obsidian(
    o: &mut Overlays,
    line: &str,
    at: usize,
    index: Option<&Index>,
    rel: Option<&str>,
) {
    inline(o, line, at, Flavor::Obsidian, index, rel);
    highlights(o, line, at, "==");
    // A block's `^id` at the end of its line.
    let t = line.trim_end();
    if let Some(p) = t.rfind(" ^") {
        let id = &t[p + 2..];
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            o.span(at + p, at + t.len(), Effect::Hide);
        }
    }
}

/// A Logseq note in Org: Org's own drawing keeps its drawers, `TODO`
/// and `DONE`; the layer styles Logseq's other task keywords (`LATER`,
/// `NOW`, `DOING`, `WAITING`, `CANCELED`…), which Org does not know
/// without a `#+TODO:` line, and adds block references and tags.
pub fn logseq_org(text: &str, index: Option<&Index>, rel: Option<&str>) -> Overlays {
    let mut o = Overlays::default();
    let mut src = false;
    for (start, line) in lines(text) {
        let upper = line.trim().to_uppercase();
        if src {
            if upper.starts_with("#+END_") {
                src = false;
            }
            continue;
        }
        if upper.starts_with("#+BEGIN_SRC") || upper.starts_with("#+BEGIN_EXAMPLE") {
            src = true;
            continue;
        }
        let stars = line.len() - line.trim_start_matches('*').len();
        if stars > 0 && line[stars..].starts_with(' ') {
            let at = stars + 1;
            for m in MARKERS.iter().filter(|m| !matches!(**m, "TODO" | "DONE")) {
                if let Some(after) = line[at..].strip_prefix(m)
                    && (after.is_empty() || after.starts_with(' '))
                {
                    o.span(
                        start + at,
                        start + at + m.len(),
                        Effect::Style(keyword_look(m)),
                    );
                    break;
                }
            }
        }
        inline(&mut o, line, start, Flavor::LogseqOrg, index, rel);
    }
    let s = scan::scan(text, Flavor::LogseqOrg, &scan::Options::default());
    ref_counts(&mut o, &s.blocks, &lines(text), index);
    o.finish()
}
/// The blocks under the block at line `line` of a Logseq note in
/// Markdown shown though it is collapsed: the block the note is zoomed
/// into, whose children Logseq shows.
pub fn unfold(o: &mut Overlays, text: &str, line: u32) {
    let s = scan::scan(text, Flavor::LogseqMarkdown, &scan::Options::default());
    let Some(i) = s.blocks.iter().position(|b| b.line == line) else {
        return;
    };
    let Some(child) = s.blocks.get(i + 1).filter(|c| c.parent == Some(i)) else {
        return;
    };
    let from: usize = text
        .split_inclusive('\n')
        .take(child.line as usize)
        .map(str::len)
        .sum();
    o.lines
        .retain(|l| !(l.start == from && l.effect == LineEffect::Hidden));
}

/// The overlays of a note of a graph of `kind` at `rel` (relative to the
/// root) with `text`.
pub fn overlays(kind: Kind, text: &str, index: Option<&Index>, rel: Option<&str>) -> Overlays {
    let org = rel.is_some_and(|r| r.to_lowercase().ends_with(".org"));
    match (kind, org) {
        (Kind::Obsidian, _) => obsidian(text, index, rel),
        (Kind::Logseq, true) => logseq_org(text, index, rel),
        (Kind::Logseq, false) => logseq_markdown(text, index, rel),
    }
}

/// `rows` under the last line of `text`, after those drawn there already:
/// what Logseq shows under today's journal.
pub fn rows_at_end(o: &mut Overlays, text: &str, rows: Vec<Row>) {
    let last = text
        .trim_end_matches(['\n', '\r'])
        .rfind('\n')
        .map_or(0, |i| i + 1);
    match o.below.last_mut() {
        Some(b) if b.at >= last => b.rows.extend(rows),
        _ => o.rows(last, rows),
    }
}

/// The `{{query …}}` macros of a Logseq note shown as what `summary`
/// says of each, and an advanced query (`#+BEGIN_QUERY` … `#+END_QUERY`)
/// as what `advanced` says of it, its lines hidden (away from the
/// cursor, as Logseq shows a query's results until it is clicked); over
/// what the note's layer gave.
pub fn queries(
    mut o: Overlays,
    text: &str,
    summary: &dyn Fn(&str) -> (String, Vec<Row>),
    advanced: &dyn Fn(&str) -> (String, Vec<Row>),
) -> Overlays {
    let mut fence = false;
    let mut src = false;
    let mut added = false;
    // The end of the advanced query being skipped.
    let mut inside: Option<usize> = None;
    for (start, line) in lines(text) {
        if inside.is_some_and(|e| start < e) {
            continue;
        }
        inside = None;
        let trimmed = line.trim();
        let upper = trimmed.to_uppercase();
        if fence {
            fence = !trimmed.starts_with("```");
            continue;
        }
        if src {
            src = !upper.starts_with("#+END_");
            continue;
        }
        let body = trimmed.trim_start_matches(['-', '*', ' ', '\t']);
        if body.starts_with("```") {
            fence = true;
            continue;
        }
        if upper.starts_with("#+BEGIN_SRC") || upper.starts_with("#+BEGIN_EXAMPLE") {
            src = true;
            continue;
        }
        if let Some(p) = scan::find_ci(line, "#+BEGIN_QUERY") {
            let open = start + p.start;
            // In the text itself: a copy in capitals is not as long where
            // a letter's capital is not (`ı`, `İ`).
            let close = scan::find_ci(&text[open..], "#+END_QUERY").map(|q| open + q.end);
            let Some(close) = close else {
                continue;
            };
            let block_end = text[close..]
                .find('\n')
                .map_or(text.len(), |n| close + n + 1);
            let (shown, results) = advanced(&text[open..close]);
            o.put(
                open,
                open + "#+BEGIN_QUERY".len(),
                Effect::Replace(
                    shown,
                    Look {
                        link: true,
                        ..Look::default()
                    },
                ),
            );
            // Under its first line, which shows its summary; the rest is
            // hidden away from the cursor.
            o.rows(open, results);
            let body = line_end(text, start, line);
            if body < block_end {
                o.lines.push(Lines {
                    start: body,
                    end: block_end,
                    effect: LineEffect::Hidden,
                });
            }
            inside = Some(block_end);
            added = true;
            continue;
        }
        for (s, e, inner) in crate::query::macros(line) {
            let (shown, results) = summary(inner);
            o.put(
                start + s,
                start + e,
                Effect::Replace(
                    shown,
                    Look {
                        link: true,
                        ..Look::default()
                    },
                ),
            );
            o.rows(start + s, results);
            added = true;
        }
    }
    if added { o.finish() } else { o }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Fallbacks, Graph};
    use crate::files::Memory;

    fn shown(text: &str, o: &Overlays) -> String {
        // The text as drawn away from the cursor: hidden lines and spans
        // out, replacements in.
        let mut out = String::new();
        let mut at = 0;
        let mut spans = o.spans.iter().peekable();
        while at < text.len() {
            if let Some(l) = o
                .lines
                .iter()
                .find(|l| l.start == at && l.effect == LineEffect::Hidden)
            {
                at = l.end;
                continue;
            }
            if let Some(s) = spans.peek()
                && s.start == at
            {
                match &s.effect {
                    Effect::Hide => {}
                    Effect::Replace(t, _) => out.push_str(t),
                    Effect::Style(_) => out.push_str(&text[s.start..s.end]),
                }
                at = s.end;
                spans.next();
                continue;
            }
            let c = text[at..].chars().next().unwrap();
            out.push(c);
            at += c.len_utf8();
        }
        out
    }

    #[test]
    fn highlights_numbered_lists_and_admonitions() {
        let text = "- a ^^bright^^ idea\n\
- one\n  logseq.order-list-type:: number\n\
- two\n  logseq.order-list-type:: number\n\
\t- under\n\t  logseq.order-list-type:: number\n\
\t\t- deeper\n\t\t  logseq.order-list-type:: number\n\
\t- plain\n\
\t- again\n\t  logseq.order-list-type:: number\n\
- #+BEGIN_NOTE\n  Read **this**\n  #+END_NOTE\n\
- #+BEGIN_QUOTE\n  said\n  #+END_QUOTE\n";
        let o = logseq_markdown(text, None, None);
        assert_eq!(
            shown(text, &o),
            "- a bright idea\n\
1. one\n\
2. two\n\
\ta. under\n\
\t\tI. deeper\n\
\t- plain\n\
\ta. again\n\
- ✎ Note\n  Read **this**\n\
- #+BEGIN_QUOTE\n  said\n  #+END_QUOTE\n"
        );
        assert_eq!(letters(27), "aa");
        assert_eq!(roman(1994), "MCMXCIV");
        // Org's highlights too.
        let org = "* a ^^lit^^ word\n";
        assert_eq!(shown(org, &logseq_org(org, None, None)), "* a lit word\n");
        // Logseq's keywords Org does not know, styled; Org's own left to it.
        let org = "* LATER plan\n** NOW go\n* TODO org's\n* NOWHERE\n";
        let o = logseq_org(org, None, None);
        let styled: Vec<&str> = o
            .spans
            .iter()
            .filter(|s| matches!(s.effect, Effect::Style(l) if l.todo))
            .map(|s| &org[s.start..s.end])
            .collect();
        assert_eq!(styled, ["LATER", "NOW"]);
    }

    #[test]
    fn an_advanced_query_shown_as_its_results() {
        let text = "- before\n- #+BEGIN_QUERY\n  {:query [:find ?b]}\n  #+END_QUERY\n- after\n";
        let row = |t: &str| Row {
            indent: 2,
            parts: vec![(t.to_string(), Look::default())],
            block: None,
            source: None,
            floor: None,
            root: false,
        };
        let o = queries(
            Overlays::default(),
            text,
            &|_| (String::new(), Vec::new()),
            &|q| (format!("⌕ {} bytes", q.len()), vec![row("a result")]),
        );
        assert_eq!(shown(text, &o), "- before\n- ⌕ 49 bytes\n- after\n");
        // Its results under its first line, which shows its summary.
        assert_eq!(o.below.len(), 1);
        let at = o.below[0].at;
        let line_start = text[..at].rfind('\n').map_or(0, |n| n + 1);
        assert!(
            text[line_start..].starts_with("- #+BEGIN_QUERY"),
            "{:?}",
            &text[line_start..]
        );
        assert_eq!(o.below[0].rows, [row("a result")]);
    }

    #[test]
    fn a_logseq_note() {
        let m = Memory::new(&[
            ("/g/logseq/config.edn", "{}"),
            (
                "/g/pages/A.md",
                "- The core\n  id:: 6512c0de-0001-4000-8000-000000000001\n",
            ),
        ]);
        let i = Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        );
        let text = "alias:: X\n\n- TODO [#A] see ((6512c0de-0001-4000-8000-000000000001)) #tag\n  id:: 6512c0de-0001-4000-8000-000000000002\n  collapsed:: true\n  type:: book\n  SCHEDULED: <2026-10-12 Mon>\n  :LOGBOOK:\n  CLOCK: [2026-10-10 Sat 09:00]\n  :END:\n\t- {{embed ((6512c0de-0001-4000-8000-000000000001))}}\n- ```\n  ((6512c0de-0001-4000-8000-000000000001))\n  ```\n";
        let o = logseq_markdown(text, Some(&i), Some("pages/B.md"));
        assert_eq!(
            shown(text, &o),
            "alias:: X\n\n▸ TODO [#A] see The core #tag\n  type:: book\n  SCHEDULED: <2026-10-12 Mon>\n  :LOGBOOK:\n  CLOCK: [2026-10-10 Sat 09:00]\n  :END:\n- ```\n  ((6512c0de-0001-4000-8000-000000000001))\n  ```\n"
        );
        // The block is collapsed: its child, an embed, is hidden, and
        // names the page of its block, shown under it.
        assert!(o.spans.iter().any(|s| s.effect
            == Effect::Replace(
                "↳ A".into(),
                Look {
                    link: true,
                    ..Look::default()
                }
            )));
        let looks: Vec<(&str, Look)> = o
            .spans
            .iter()
            .filter_map(|s| match &s.effect {
                Effect::Style(l) => Some((&text[s.start..s.end], *l)),
                _ => None,
            })
            .collect();
        assert!(looks.contains(&(
            "TODO",
            Look {
                todo: true,
                ..Look::default()
            }
        )));
        assert!(looks.contains(&(
            "[#A]",
            Look {
                priority: true,
                ..Look::default()
            }
        )));
        assert!(looks.contains(&(
            "#tag",
            Look {
                tag: true,
                ..Look::default()
            }
        )));
        assert!(looks.contains(&(
            "type::",
            Look {
                dim: true,
                ..Look::default()
            }
        )));
        assert!(looks.contains(&(
            "alias::",
            Look {
                dim: true,
                ..Look::default()
            }
        )));
        // The logbook folds to its first line.
        let lb = text.find("  :LOGBOOK:").unwrap();
        assert!(o.lines.iter().any(|l| l.start == lb
            && l.effect == LineEffect::Folded
            && text[l.start..l.end].ends_with(":END:\n")));
        // Without the index, a block reference is only dimmed.
        let o = logseq_markdown(
            "- see ((6512c0de-0001-4000-8000-000000000001))\n",
            None,
            None,
        );
        assert!(matches!(
            o.spans[0].effect,
            Effect::Style(Look { dim: true, .. })
        ));
    }

    #[test]
    fn a_block_shows_the_number_of_blocks_referring_to_it() {
        // As Logseq 0.10.9 counts them beside the block: a block that
        // refers to it twice, and one that embeds it, once each.
        let id = "6512c0de-0001-4000-8000-000000000010";
        let a = format!(
            "- Counted\n  id:: {id}\n- Plain\n- twice (({id})) (({id}))\n\t- a child (({id}))\n"
        );
        let b = format!("- another (({id}))\n- {{{{embed (({id}))}}}}\n");
        let c = "* Org\n:PROPERTIES:\n:id: 6512c0de-0001-4000-8000-000000000011\n:END:\n* cites ((6512c0de-0001-4000-8000-000000000011))\n";
        let m = Memory::new(&[
            ("/g/logseq/config.edn", "{}"),
            ("/g/pages/A.md", &a),
            ("/g/pages/B.md", &b),
            ("/g/pages/C.org", c),
        ]);
        let i = Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        );
        let count = |n: &str, at: usize| {
            vec![
                Insert {
                    at,
                    text: " ".into(),
                    look: Look::default(),
                },
                Insert {
                    at,
                    text: n.into(),
                    look: Look {
                        code: true,
                        ..Look::default()
                    },
                },
            ]
        };
        let o = logseq_markdown(&a, Some(&i), Some("pages/A.md"));
        assert_eq!(o.inserts, count("4", "- Counted".len()));
        // In Org, at the end of the headline.
        let o = logseq_org(c, Some(&i), Some("pages/C.org"));
        assert_eq!(o.inserts, count("1", "* Org".len()));
        // Without the index, none.
        assert!(logseq_markdown(&a, None, None).inserts.is_empty());
    }

    #[test]
    fn an_obsidian_note() {
        let text = "---\ntags: [a]\n---\n> [!warning]- Careful\n> inside\n\n> [!note] Plain\n\nA ==bright== idea ^b1 #tag %%hidden%% end\n%%\nmany\nlines\n%%\nstatus:: done\n";
        let o = obsidian(text, None, None);
        assert_eq!(
            shown(text, &o),
            "---\ntags: [a]\n---\n> ⚠ Warning Careful\n> inside\n\n> ✎ Note Plain\n\nA bright idea ^b1 #tag  end\nstatus:: done\n"
        );
        let fold = text.find("> [!warning]").unwrap();
        let end = text.find("\n\n> [!note]").unwrap() + 1;
        assert!(o.lines.contains(&Lines {
            start: fold,
            end,
            effect: LineEffect::Folded
        }));
    }

    #[test]
    fn a_collapsed_block_hides_its_children() {
        let text = "- a\n  collapsed:: true\n\t- a1\n\t  id:: x\n\t\t- a2\n- b\n";
        let o = logseq_markdown(text, None, None);
        // Its bullet an arrow, its children hidden.
        assert_eq!(shown(text, &o), "▸ a\n- b\n");
        // One hidden range, from the first child to the end of the last.
        let first = text.find("\t- a1").unwrap();
        let end = text.find("- b").unwrap();
        assert!(o.lines.contains(&Lines {
            start: first,
            end,
            effect: LineEffect::Hidden
        }));
    }

    #[test]
    fn spans_never_overlap() {
        let text = "- #tag ((6512c0de-0001-4000-8000-000000000001)) ^^x^^\n";
        let o = logseq_markdown(text, None, None);
        for w in o.spans.windows(2) {
            assert!(w[0].end <= w[1].start);
        }
    }
}
#[cfg(test)]
mod mirror {
    /// A mirror's page: its id line hidden, its properties' keys dimmed
    /// as a file graph's are, their values' tags styled.
    #[test]
    fn a_mirror_page_drawn_as_a_graphs() {
        let t = "id:: 11111111-1111-4111-8111-111111111111\n* type:: [[Plan]]\n\n- Read\n  * owner:: #alice\n  * collapsed:: true\n";
        let o = super::logseq_markdown(t, None, None);
        let hidden: Vec<&str> = o.lines.iter().map(|l| &t[l.start..l.end]).collect();
        assert_eq!(
            hidden,
            [
                "id:: 11111111-1111-4111-8111-111111111111\n",
                "  * collapsed:: true\n"
            ]
        );
        let styled: Vec<&str> = o.spans.iter().map(|s| &t[s.start..s.end]).collect();
        assert_eq!(styled, ["type::", "owner::", "#alice"]);
    }
}
