//! The layer over Kalem's Markdown and Org (Kalem's plugin API 0.2.10,
//! `graph_todo.md` GR4 to GR7): what a note shows as Logseq and Obsidian
//! show it, as overlays by byte range that Kalem applies to its own
//! drawing of the note. Kalem's Markdown draws the Markdown (emphasis,
//! links, wiki links, tables, lists); the layer hides what the
//! applications hide (a block's `id::` and `collapsed::` lines, a `^id`, a
//! `%%comment%%`), shows a block reference as its block's text, styles
//! tags, task keywords and dates, and folds a `:LOGBOOK:`.
//!
//! Pure: the text and, when the graph is indexed, the index in, the
//! overlays out; the component turns them into the API's records.

use crate::config::Kind;
use crate::index::Index;
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

/// The overlays of a note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overlays {
    /// Spans, in order, not overlapping.
    pub spans: Vec<Span>,
    /// Lines, in order, not overlapping.
    pub lines: Vec<Lines>,
}

impl Overlays {
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
fn hidden_property(key: &str, extra: &[String]) -> bool {
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
                        id.and_then(|id| block_text(index, &id))
                            .unwrap_or_else(|| r.target.clone())
                    }
                    (RefKind::EmbedBlock, _) => {
                        block_text(index, &r.target).unwrap_or_else(|| "a block".into())
                    }
                    _ => match &r.anchor {
                        Some(a) => format!("{} › {a}", r.target),
                        None => r.target.clone(),
                    },
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
        if trimmed.starts_with("#+BEGIN_") || trimmed.starts_with("#+END_") {
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
    for (i, b) in s.blocks.iter().enumerate() {
        if b.collapsed && s.blocks.get(i + 1).is_some_and(|c| c.parent == Some(i)) {
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
    // `==highlight==`: the markers hidden, the text bold.
    let mut from = 0;
    while let Some(p) = line[from..].find("==") {
        let s = from + p;
        let Some(q) = line[s + 2..].find("==") else {
            break;
        };
        let e = s + 2 + q;
        if q > 0 && !line[s + 2..e].starts_with(' ') {
            o.span(at + s, at + s + 2, Effect::Hide);
            o.span(
                at + s + 2,
                at + e,
                Effect::Style(Look {
                    bold: true,
                    ..Look::default()
                }),
            );
            o.span(at + e, at + e + 2, Effect::Hide);
        }
        from = e + 2;
    }
    // A block's `^id` at the end of its line.
    let t = line.trim_end();
    if let Some(p) = t.rfind(" ^") {
        let id = &t[p + 2..];
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            o.span(at + p, at + t.len(), Effect::Hide);
        }
    }
}

/// A Logseq note in Org: Org's own drawing keeps its drawers and
/// keywords; the layer adds block references and tags.
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
        inline(&mut o, line, start, Flavor::LogseqOrg, index, rel);
    }
    o.finish()
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

/// The `{{query …}}` macros of a Logseq note shown as what `summary`
/// says of each (away from the cursor), and an advanced query's
/// `#+BEGIN_QUERY` marked as one Kalem does not run; over what the
/// note's layer gave.
pub fn queries(mut o: Overlays, text: &str, summary: &dyn Fn(&str) -> String) -> Overlays {
    let mut fence = false;
    let mut src = false;
    let mut added = false;
    for (start, line) in lines(text) {
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
        if let Some(p) = line
            .find("#+BEGIN_QUERY")
            .or_else(|| line.find("#+begin_query"))
        {
            let end = start + p + "#+BEGIN_QUERY".len();
            o.put(
                start + p,
                end,
                Effect::Replace(
                    "#+BEGIN_QUERY · an advanced query, not run by Kalem".into(),
                    Look {
                        dim: true,
                        ..Look::default()
                    },
                ),
            );
            added = true;
            continue;
        }
        for (s, e, inner) in crate::query::macros(line) {
            o.put(
                start + s,
                start + e,
                Effect::Replace(
                    summary(inner),
                    Look {
                        link: true,
                        ..Look::default()
                    },
                ),
            );
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
            "alias:: X\n\n- TODO [#A] see The core #tag\n  type:: book\n  SCHEDULED: <2026-10-12 Mon>\n  :LOGBOOK:\n  CLOCK: [2026-10-10 Sat 09:00]\n  :END:\n- ```\n  ((6512c0de-0001-4000-8000-000000000001))\n  ```\n"
        );
        // The block is collapsed: its child, an embed, is hidden, and
        // shown as its block's text where it shows.
        assert!(o.spans.iter().any(|s| s.effect
            == Effect::Replace(
                "↳ The core".into(),
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
        assert_eq!(shown(text, &o), "- a\n- b\n");
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
