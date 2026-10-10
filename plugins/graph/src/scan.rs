//! The scanner: what the graph layer needs from a note, found line by
//! line, without parsing its Markdown or Org (the core's modes do that):
//! the page's properties (title, aliases, tags), its blocks with their
//! depth, id, task keyword, priority, dates and properties, its headings,
//! and every reference it makes to a page, a tag or a block, with where
//! it is. Three flavours: Logseq's Markdown, Logseq's Org, Obsidian's
//! Markdown.

use crate::date::Date;

/// Whose note, in which format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// A Logseq page in Markdown: every `- ` a block.
    LogseqMarkdown,
    /// A Logseq page in Org: every headline a block.
    LogseqOrg,
    /// An Obsidian note: paragraphs, list items and headings as blocks.
    Obsidian,
}

/// What a reference is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefKind {
    /// `[[page]]`.
    Page,
    /// `#tag`, `#[[two words]]`, a `tags::` value.
    Tag,
    /// `((uuid))`, `[[note#^id]]`.
    Block,
    /// `{{embed [[page]]}}`, `![[note]]`.
    EmbedPage,
    /// `{{embed ((uuid))}}`, `![[note#^id]]`.
    EmbedBlock,
    /// A page named by a property's value (`alias::`, a comma property).
    Property,
}

impl RefKind {
    /// Whether it names a block, not a page.
    pub fn to_block(self) -> bool {
        matches!(self, RefKind::Block | RefKind::EmbedBlock)
    }
}

/// A reference in a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// What it is.
    pub kind: RefKind,
    /// The page's name as written, or the block's id (lower case). For
    /// an Obsidian block reference, the note's name; the id is the
    /// anchor.
    pub target: String,
    /// Obsidian's `#Heading` or `#^id` after the name, without the `#`.
    pub anchor: Option<String>,
    /// The block it is in, by index into [`Scanned::blocks`].
    pub block: Option<usize>,
    /// Its line, from 0.
    pub line: u32,
    /// Its bytes in the line.
    pub start: u32,
    /// The end of its bytes.
    pub end: u32,
}

/// A block of a note.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Block {
    /// Its first line, from 0.
    pub line: u32,
    /// The line after its last.
    pub end: u32,
    /// 0 at the top level.
    pub depth: u16,
    /// The block it is under.
    pub parent: Option<usize>,
    /// Its id: Logseq's `id::` UUID or `:ID:`, Obsidian's `^id`.
    pub id: Option<String>,
    /// Its first line without the bullet, the keyword, the priority and
    /// a heading's marks.
    pub text: String,
    /// `TODO`, `DOING`, `DONE`, `LATER`, `NOW`, `WAITING`, `CANCELED`…
    pub marker: Option<String>,
    /// `A`, `B`, `C`.
    pub priority: Option<char>,
    /// `SCHEDULED: <…>`.
    pub scheduled: Option<Date>,
    /// `DEADLINE: <…>`.
    pub deadline: Option<Date>,
    /// Its properties, keys in lower case.
    pub props: Vec<(String, String)>,
    /// `collapsed:: true`.
    pub collapsed: bool,
    /// A heading's level, when the block is one.
    pub heading: Option<u8>,
}

/// A heading of a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// Its line, from 0.
    pub line: u32,
    /// 1 to 6.
    pub level: u8,
    /// Its text.
    pub text: String,
}

/// What a note holds, for the index.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scanned {
    /// The page's own title (`title::`, `#+title:`, front matter's
    /// `title`).
    pub title: Option<String>,
    /// The page's properties, keys in lower case.
    pub props: Vec<(String, String)>,
    /// Its aliases.
    pub aliases: Vec<String>,
    /// Its tags (`tags::`, front matter's `tags`).
    pub tags: Vec<String>,
    /// Its blocks, in order.
    pub blocks: Vec<Block>,
    /// Its references, in order.
    pub refs: Vec<Ref>,
    /// Its headings.
    pub headings: Vec<Heading>,
    /// Its lines.
    pub lines: u32,
}

/// What the scanner is told of the graph.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Properties whose values are page references split at commas.
    pub comma_properties: Vec<String>,
}

/// The task keywords Logseq knows.
pub const MARKERS: &[&str] = &[
    "TODO",
    "DOING",
    "DONE",
    "LATER",
    "NOW",
    "WAITING",
    "WAIT",
    "CANCELED",
    "CANCELLED",
    "IN-PROGRESS",
    "STARTED",
];

/// Whether `marker` is done or dropped.
pub fn is_closed(marker: &str) -> bool {
    matches!(marker, "DONE" | "CANCELED" | "CANCELLED")
}

/// `text` scanned as `flavor`.
pub fn scan(text: &str, flavor: Flavor, options: &Options) -> Scanned {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut s = match flavor {
        Flavor::LogseqMarkdown => logseq_markdown(&lines, options),
        Flavor::LogseqOrg => logseq_org(&lines, options),
        Flavor::Obsidian => obsidian(&lines),
    };
    s.lines = lines.len() as u32;
    s.refs.sort_by_key(|r| (r.line, r.start));
    s
}

/// A reference found in a line, before it is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Raw {
    kind: RefKind,
    target: String,
    anchor: Option<String>,
    start: usize,
    end: usize,
}

/// 8-4-4-4-12 hexadecimal digits.
pub fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The end of the `close` that matches the `open` at `at` (both two
/// bytes), counting nested pairs.
fn matching(b: &[u8], at: usize, open: &[u8; 2], close: &[u8; 2]) -> Option<usize> {
    let mut depth = 0;
    let mut i = at;
    while i + 1 < b.len() {
        if b[i..i + 2] == open[..] {
            depth += 1;
            i += 2;
        } else if b[i..i + 2] == close[..] {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(i);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// The extensions a wiki link to a note may carry; others name
/// attachments.
fn note_target(target: &str) -> bool {
    match target.rsplit_once('.') {
        Some((_, ext)) if !ext.contains('/') && !ext.contains(' ') => {
            matches!(ext.to_lowercase().as_str(), "md" | "org" | "markdown")
                || ext.chars().any(|c| !c.is_ascii_alphanumeric())
                || ext.len() > 5
        }
        _ => true,
    }
}

/// Characters a tag stops at.
fn tag_stop(c: char, flavor: Flavor) -> bool {
    if c.is_whitespace() {
        return true;
    }
    match flavor {
        Flavor::Obsidian => !(c.is_alphanumeric() || matches!(c, '_' | '-' | '/')),
        _ => matches!(
            c,
            ',' | ';'
                | '!'
                | '?'
                | '"'
                | '\''
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | '`'
        ),
    }
}

/// A wiki link's text as a reference: Obsidian's `name#anchor|alias`,
/// Logseq's `page`, Org's `target][label`.
fn wiki(inner: &str, flavor: Flavor, embed: bool) -> Option<(RefKind, String, Option<String>)> {
    let inner = inner.trim();
    match flavor {
        Flavor::LogseqOrg => {
            let target = inner.split("][").next().unwrap_or(inner).trim();
            if target.contains("://") || target.starts_with('*') || target.starts_with('#') {
                return None;
            }
            if let Some(file) = target.strip_prefix("file:") {
                let name = crate::files::file_name(file);
                let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
                return Some((RefKind::Page, stem.to_string(), Some("file".into())));
            }
            if let Some(id) = target.strip_prefix("id:") {
                return Some((RefKind::Block, id.to_lowercase(), None));
            }
            Some((RefKind::Page, target.to_string(), None))
        }
        Flavor::LogseqMarkdown => {
            if inner.is_empty() || inner.contains("://") {
                return None;
            }
            let kind = if embed {
                RefKind::EmbedPage
            } else {
                RefKind::Page
            };
            Some((kind, inner.to_string(), None))
        }
        Flavor::Obsidian => {
            let target = inner.split('|').next().unwrap_or(inner);
            let (name, anchor) = match target.split_once('#') {
                Some((n, a)) => (n.trim(), Some(a.trim().to_string())),
                None => (target.trim(), None),
            };
            if !note_target(name) || inner.contains("://") {
                return None;
            }
            let block = anchor.as_deref().is_some_and(|a| a.starts_with('^'));
            let kind = match (embed, block) {
                (true, true) => RefKind::EmbedBlock,
                (true, false) => RefKind::EmbedPage,
                (false, true) => RefKind::Block,
                (false, false) => RefKind::Page,
            };
            let anchor = anchor.map(|a| a.trim_start_matches('^').to_string());
            Some((kind, name.to_string(), anchor))
        }
    }
}

/// The references in `line`.
fn inline(line: &str, flavor: Flavor) -> Vec<Raw> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                let n = b[i..].iter().take_while(|c| **c == b'`').count();
                let fence = &line[i..i + n];
                i = match line[i + n..].find(fence) {
                    Some(p) => i + n + p + n,
                    None => i + n,
                };
            }
            b'{' if line[i..].starts_with("{{embed") => {
                let end = line[i..].find("}}").map_or(b.len(), |p| i + p + 2);
                let inner = &line[i + 7..end.saturating_sub(2).max(i + 7)];
                if let Some(p) = inner.find("((")
                    && let Some(q) = inner[p..].find("))")
                {
                    let id = inner[p + 2..p + q].trim();
                    if is_uuid(id) {
                        out.push(Raw {
                            kind: RefKind::EmbedBlock,
                            target: id.to_lowercase(),
                            anchor: None,
                            start: i,
                            end,
                        });
                    }
                } else if let Some(p) = inner.find("[[")
                    && let Some(q) = inner[p..].rfind("]]")
                    && let Some((kind, target, anchor)) = wiki(&inner[p + 2..p + q], flavor, true)
                {
                    out.push(Raw {
                        kind,
                        target,
                        anchor,
                        start: i,
                        end,
                    });
                }
                i = end;
            }
            b'!' if flavor == Flavor::Obsidian && line[i + 1..].starts_with("[[") => {
                match matching(b, i + 1, b"[[", b"]]") {
                    Some(end) => {
                        if let Some((kind, target, anchor)) =
                            wiki(&line[i + 3..end - 2], flavor, true)
                        {
                            out.push(Raw {
                                kind,
                                target,
                                anchor,
                                start: i,
                                end,
                            });
                        }
                        i = end;
                    }
                    None => i += 1,
                }
            }
            b'[' if line[i..].starts_with("[[") => match matching(b, i, b"[[", b"]]") {
                Some(end) => {
                    if let Some((kind, target, anchor)) = wiki(&line[i + 2..end - 2], flavor, false)
                    {
                        out.push(Raw {
                            kind,
                            target,
                            anchor,
                            start: i,
                            end,
                        });
                    }
                    // Logseq's `[[a [[b]]]]` names `b` too.
                    if flavor == Flavor::LogseqMarkdown {
                        let inner = &line[i + 2..end - 2];
                        if inner.contains("[[") {
                            for mut r in inline(inner, flavor) {
                                r.start += i + 2;
                                r.end += i + 2;
                                out.push(r);
                            }
                        }
                    }
                    i = end;
                }
                None => i += 2,
            },
            b'[' if flavor == Flavor::Obsidian => {
                // A Markdown link to a note: `[text](Note%20name.md)`.
                if let Some(close) = line[i..].find("](")
                    && let Some(end) = line[i + close..].find(')')
                {
                    let target = &line[i + close + 2..i + close + end];
                    let decoded = target.replace("%20", " ");
                    let (path, anchor) = match decoded.split_once('#') {
                        Some((p, a)) => (p.to_string(), Some(a.to_string())),
                        None => (decoded.clone(), None),
                    };
                    if !target.contains("://")
                        && !target.starts_with("mailto:")
                        && path.to_lowercase().ends_with(".md")
                    {
                        out.push(Raw {
                            kind: RefKind::Page,
                            target: path[..path.len() - 3].to_string(),
                            anchor,
                            start: i,
                            end: i + close + end + 1,
                        });
                        i += close + end + 1;
                        continue;
                    }
                }
                i += 1;
            }
            b'(' if line[i..].starts_with("((") => match line[i + 2..].find("))") {
                Some(p) => {
                    let id = line[i + 2..i + 2 + p].trim();
                    if is_uuid(id) {
                        out.push(Raw {
                            kind: RefKind::Block,
                            target: id.to_lowercase(),
                            anchor: None,
                            start: i,
                            end: i + 2 + p + 2,
                        });
                        i += 2 + p + 2;
                    } else {
                        // `(((uuid)))`: the next `((` may be the reference.
                        i += 1;
                    }
                }
                None => i += 2,
            },
            b'#' => {
                let before = line[..i].chars().next_back();
                let starts = before.is_none_or(|c| c.is_whitespace() || c == '(' || c == ',');
                let next = line[i + 1..].chars().next();
                // `#+BEGIN_QUOTE`, `#+title:`: Org's directives, which
                // Logseq writes in Markdown too, are no tags.
                if !starts || next.is_none_or(|c| c.is_whitespace() || c == '#' || c == '+') {
                    i += 1;
                    continue;
                }
                if flavor != Flavor::Obsidian
                    && line[i + 1..].starts_with("[[")
                    && let Some(end) = matching(b, i + 1, b"[[", b"]]")
                {
                    let name = line[i + 3..end - 2].trim();
                    if !name.is_empty() {
                        out.push(Raw {
                            kind: RefKind::Tag,
                            target: name.to_string(),
                            anchor: None,
                            start: i,
                            end,
                        });
                    }
                    i = end;
                    continue;
                }
                let rest = &line[i + 1..];
                let len = rest
                    .char_indices()
                    .find(|(_, c)| tag_stop(*c, flavor))
                    .map_or(rest.len(), |(p, _)| p);
                let name = rest[..len].trim_end_matches(['.', ':']);
                let valid = !name.is_empty()
                    && (flavor != Flavor::Obsidian || name.chars().any(|c| !c.is_ascii_digit()));
                if valid {
                    out.push(Raw {
                        kind: RefKind::Tag,
                        target: name.to_string(),
                        anchor: None,
                        start: i,
                        end: i + 1 + name.len(),
                    });
                }
                i += 1 + len.max(1).min(rest.len().max(1));
            }
            _ => {
                i += line[i..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }
    out
}

/// The reference under byte `column` of `line`, in `flavor`'s syntax.
pub fn reference_at(line: &str, column: usize, flavor: Flavor) -> Option<Ref> {
    inline(line, flavor)
        .into_iter()
        .find(|r| r.start <= column && column < r.end.max(r.start + 1))
        .map(|r| Ref {
            kind: r.kind,
            target: r.target,
            anchor: r.anchor,
            block: None,
            line: 0,
            start: r.start as u32,
            end: r.end as u32,
        })
}

/// A `key:: value` line (leading whitespace allowed): the key in lower
/// case and the value.
fn property(line: &str) -> Option<(String, &str)> {
    let t = line.trim_start();
    let p = t.find("::")?;
    let key = &t[..p];
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '?' | '!' | '/' | '*'))
    {
        return None;
    }
    let value = &t[p + 2..];
    if !value.is_empty() && !value.starts_with([' ', '\t']) {
        return None;
    }
    Some((key.to_lowercase(), value.trim()))
}

/// A property's value as page names: split at commas outside brackets,
/// each without `[[`, `]]` and `#`.
pub fn split_values(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in value.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            ',' if depth <= 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out.iter()
        .map(|v| {
            v.trim()
                .trim_start_matches('#')
                .trim_start_matches("[[")
                .trim_end_matches("]]")
                .trim()
                .to_string()
        })
        .filter(|v| !v.is_empty())
        .collect()
}

/// A task keyword and a priority at the start of `text`, and the rest.
fn task(text: &str) -> (Option<String>, Option<char>, &str) {
    let mut rest = text;
    let mut marker = None;
    for m in MARKERS {
        if let Some(after) = rest.strip_prefix(m)
            && (after.is_empty() || after.starts_with(' '))
        {
            marker = Some((*m).to_string());
            rest = after.trim_start();
            break;
        }
    }
    let mut priority = None;
    if rest.len() >= 4
        && rest.starts_with("[#")
        && rest.as_bytes()[3] == b']'
        && rest.as_bytes()[2].is_ascii_uppercase()
    {
        priority = Some(rest.as_bytes()[2] as char);
        rest = rest[4..].trim_start();
    }
    (marker, priority, rest)
}

/// The date of `SCHEDULED: <2026-10-12 Mon .+1d>` after `key`.
fn planning(line: &str, key: &str) -> Option<Date> {
    let p = line.find(key)?;
    let rest = line[p + key.len()..].trim_start();
    let rest = rest.strip_prefix(['<', '['])?;
    Date::parse_iso(rest.get(..10)?)
}

/// Columns of leading whitespace (a tab as four) and its bytes.
fn indent(line: &str) -> (usize, usize) {
    let mut cols = 0;
    let mut bytes = 0;
    for c in line.chars() {
        match c {
            ' ' => cols += 1,
            '\t' => cols += 4,
            _ => break,
        }
        bytes += 1;
    }
    (cols, bytes)
}

/// A markdown heading: its level and text.
fn md_heading(text: &str) -> Option<(u8, &str)> {
    let n = text.bytes().take_while(|b| *b == b'#').count();
    let rest = &text[n..];
    ((1..=6).contains(&n) && (rest.is_empty() || rest.starts_with(' ')))
        .then(|| (n as u8, rest.trim()))
}

/// Front matter's `key: value` lines between `---` lines at the start: the
/// pairs (a list's items joined with commas) and the line after it.
fn front_matter(lines: &[&str]) -> (Vec<(String, String)>, usize) {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return (Vec::new(), 0);
    }
    let Some(end) = lines.iter().skip(1).position(|l| {
        let t = l.trim_end();
        t == "---" || t == "..."
    }) else {
        return (Vec::new(), 0);
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for l in &lines[1..=end] {
        let t = l.trim_end();
        if let Some(item) = t.trim_start().strip_prefix("- ")
            && t.starts_with([' ', '\t', '-'])
            && let Some(last) = out.last_mut()
        {
            let item = item.trim().trim_matches(['"', '\'']);
            if !last.1.is_empty() {
                last.1.push_str(", ");
            }
            last.1.push_str(item);
            continue;
        }
        if let Some((k, v)) = t.split_once(':')
            && !k.starts_with([' ', '\t'])
            && !k.trim().is_empty()
        {
            let v = v.trim();
            let v = v
                .strip_prefix('[')
                .and_then(|v| v.strip_suffix(']'))
                .unwrap_or(v);
            let v = v
                .split(',')
                .map(|x| x.trim().trim_matches(['"', '\'']))
                .collect::<Vec<_>>()
                .join(", ");
            out.push((k.trim().to_lowercase(), v));
        }
    }
    (out, end + 2)
}

/// Places `raw` references of line `n` (whose text started `offset` bytes
/// into the line) under block `block`.
fn place(out: &mut Vec<Ref>, raw: Vec<Raw>, n: usize, offset: usize, block: Option<usize>) {
    for r in raw {
        out.push(Ref {
            kind: r.kind,
            target: r.target,
            anchor: r.anchor,
            block,
            line: n as u32,
            start: (r.start + offset) as u32,
            end: (r.end + offset) as u32,
        });
    }
}

/// The references in a `key:: value` line's value, placed at their
/// columns; none for the properties whose values are names split at
/// commas, which [`page_props`] reads.
fn value_refs(
    out: &mut Vec<Ref>,
    line: &str,
    key: &str,
    n: usize,
    flavor: Flavor,
    options: &Options,
    block: Option<usize>,
) {
    let listed = matches!(key, "alias" | "aliases" | "tags" | "title")
        || options.comma_properties.iter().any(|c| c == key);
    if listed {
        return;
    }
    let at = line.find("::").map_or(0, |p| p + 2);
    place(out, inline(&line[at..], flavor), n, at, block);
}

/// The page's properties taken from `pairs`: title, aliases, tags, and
/// references for the comma properties.
fn page_props(
    s: &mut Scanned,
    pairs: &[(String, String, usize)],
    options: &Options,
    alias_refs: bool,
) {
    for (k, v, line) in pairs {
        match k.as_str() {
            "title" => s.title = Some(v.trim().to_string()),
            "alias" | "aliases" => s.aliases.extend(split_values(v)),
            "tags" | "filetags" => {
                s.tags.extend(split_values(&v.replace(':', ",")));
            }
            _ => {}
        }
        let alias = k == "alias" || k == "aliases";
        if (alias && alias_refs) || k == "tags" || (!alias && options.comma_properties.contains(k))
        {
            for name in split_values(&v.replace(':', ",")) {
                s.refs.push(Ref {
                    kind: if k == "tags" {
                        RefKind::Tag
                    } else {
                        RefKind::Property
                    },
                    target: name,
                    anchor: None,
                    block: None,
                    line: *line as u32,
                    start: 0,
                    end: 0,
                });
            }
        }
        s.props.push((k.clone(), v.clone()));
    }
}

fn logseq_markdown(lines: &[&str], options: &Options) -> Scanned {
    let mut s = Scanned::default();
    let (fm, start) = front_matter(lines);
    let fm: Vec<(String, String, usize)> = fm.into_iter().map(|(k, v)| (k, v, 0)).collect();
    page_props(&mut s, &fm, options, true);
    // (columns, block) of the open blocks, outermost first.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut current: Option<usize> = None;
    let mut in_props = false;
    let mut fence = false;
    let mut drawer = false;
    let mut page: Vec<(String, String, usize)> = Vec::new();
    // The first block is the page's properties when it holds only them.
    let mut pre_block = false;
    for (n, line) in lines.iter().enumerate().skip(start) {
        let (cols, bytes) = indent(line);
        let rest = &line[bytes..];
        let trimmed = rest.trim_end();
        if fence {
            if trimmed.starts_with("```") {
                fence = false;
            }
            if let Some(c) = current {
                s.blocks[c].end = n as u32 + 1;
            }
            continue;
        }
        let bullet = trimmed == "-" || rest.starts_with("- ") || rest.starts_with("* ");
        if bullet {
            while stack.last().is_some_and(|(c, _)| *c >= cols) {
                stack.pop();
            }
            if let Some(c) = current {
                s.blocks[c].end = n as u32;
            }
            let content_at = bytes + 2.min(rest.len());
            let content = line.get(content_at..).unwrap_or("").trim_end();
            let first_block = s.blocks.is_empty() && stack.is_empty();
            pre_block = first_block && property(content).is_some();
            let mut block = Block {
                line: n as u32,
                end: n as u32 + 1,
                depth: stack.len() as u16,
                parent: stack.last().map(|(_, b)| *b),
                ..Block::default()
            };
            let (heading, after_heading) = match md_heading(content) {
                Some((level, t)) => (Some(level), t),
                None => (None, content),
            };
            let (marker, priority, text) = task(after_heading);
            block.heading = heading;
            block.marker = marker;
            block.priority = priority;
            block.text = text.to_string();
            let index = s.blocks.len();
            if pre_block {
                let (k, v) = property(content).unwrap_or_default();
                value_refs(
                    &mut s.refs,
                    line,
                    &k,
                    n,
                    Flavor::LogseqMarkdown,
                    options,
                    None,
                );
                page.push((k, v.to_string(), n));
                block.text.clear();
            } else {
                place(
                    &mut s.refs,
                    inline(content, Flavor::LogseqMarkdown),
                    n,
                    content_at,
                    Some(index),
                );
                if let Some(level) = heading {
                    s.headings.push(Heading {
                        line: n as u32,
                        level,
                        text: block.text.clone(),
                    });
                }
            }
            s.blocks.push(block);
            stack.push((cols, index));
            current = Some(index);
            in_props = true;
            let code = content.trim_start();
            if code.starts_with("```") && !code[3..].contains("```") {
                fence = true;
                in_props = false;
            }
            continue;
        }
        // A block's lines are its own up to its last line that is not
        // blank: a code block's and a drawer's included.
        if let Some(c) = current
            && !trimmed.is_empty()
        {
            s.blocks[c].end = n as u32 + 1;
        }
        if trimmed.starts_with("```") {
            fence = true;
            in_props = false;
            continue;
        }
        if drawer {
            if trimmed == ":END:" {
                drawer = false;
            }
            continue;
        }
        if trimmed == ":LOGBOOK:" || trimmed == ":PROPERTIES:" {
            drawer = true;
            continue;
        }
        let Some(c) = current else {
            // Before the first bullet: the page's properties, or text that
            // is a block of its own.
            if let Some((k, v)) = property(rest) {
                value_refs(
                    &mut s.refs,
                    line,
                    &k,
                    n,
                    Flavor::LogseqMarkdown,
                    options,
                    None,
                );
                page.push((k, v.to_string(), n));
            } else if !trimmed.is_empty() {
                let index = s.blocks.len();
                s.blocks.push(Block {
                    line: n as u32,
                    end: n as u32 + 1,
                    text: trimmed.to_string(),
                    ..Block::default()
                });
                place(
                    &mut s.refs,
                    inline(rest, Flavor::LogseqMarkdown),
                    n,
                    bytes,
                    Some(index),
                );
                stack.push((0, index));
                current = Some(index);
                in_props = false;
            }
            continue;
        };
        if in_props && let Some((k, v)) = property(rest) {
            if pre_block {
                value_refs(
                    &mut s.refs,
                    line,
                    &k,
                    n,
                    Flavor::LogseqMarkdown,
                    options,
                    None,
                );
                page.push((k, v.to_string(), n));
                continue;
            }
            let b = &mut s.blocks[c];
            match k.as_str() {
                "id" => b.id = Some(v.to_lowercase()),
                "collapsed" => b.collapsed = v == "true",
                "heading" => {
                    b.heading = v.parse().ok().or(Some(2));
                }
                _ => {}
            }
            if options.comma_properties.contains(&k) || k == "tags" || k == "alias" {
                for name in split_values(v) {
                    s.refs.push(Ref {
                        kind: if k == "tags" {
                            RefKind::Tag
                        } else {
                            RefKind::Property
                        },
                        target: name,
                        anchor: None,
                        block: Some(c),
                        line: n as u32,
                        start: 0,
                        end: 0,
                    });
                }
            } else {
                let at = line.len() - line.trim_start().len();
                place(
                    &mut s.refs,
                    inline(&line[at..], Flavor::LogseqMarkdown),
                    n,
                    at,
                    Some(c),
                );
            }
            s.blocks[c].props.push((k, v.to_string()));
            continue;
        }
        if trimmed.starts_with("SCHEDULED:") || trimmed.starts_with("DEADLINE:") {
            let b = &mut s.blocks[c];
            b.scheduled = b.scheduled.or(planning(trimmed, "SCHEDULED:"));
            b.deadline = b.deadline.or(planning(trimmed, "DEADLINE:"));
            continue;
        }
        in_props = false;
        pre_block = false;
        // A block that opens a `#+BEGIN_NOTE` shows its first line.
        let b = &mut s.blocks[c];
        if let Some(kind) = b.text.strip_prefix("#+BEGIN_")
            && !trimmed.starts_with("#+")
            && !trimmed.trim().is_empty()
        {
            b.text = format!("{}: {}", kind.trim().to_lowercase(), trimmed.trim());
        }
        place(
            &mut s.refs,
            inline(rest, Flavor::LogseqMarkdown),
            n,
            bytes,
            Some(c),
        );
    }
    if let Some(c) = current {
        let last = lines.len() as u32;
        let b = &mut s.blocks[c];
        // A file's last line is empty when it ends with a line feed.
        b.end = b.end.max(b.line + 1).min(last);
    }
    page_props(&mut s, &page, options, true);
    // A pre-block holds the page's properties, not a block.
    let pre = s.blocks.first().is_some_and(|b| {
        b.text.is_empty()
            && b.depth == 0
            && b.marker.is_none()
            && page.iter().any(|(_, _, l)| *l == b.line as usize)
    });
    if pre {
        s.blocks.remove(0);
        for b in &mut s.blocks {
            b.parent = b.parent.and_then(|p| p.checked_sub(1));
        }
        for r in &mut s.refs {
            r.block = r.block.and_then(|p| p.checked_sub(1));
        }
    }
    s
}

fn logseq_org(lines: &[&str], options: &Options) -> Scanned {
    let mut s = Scanned::default();
    let mut page: Vec<(String, String, usize)> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut current: Option<usize> = None;
    let mut drawer: Option<&str> = None;
    let mut src = false;
    for (n, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let upper = trimmed.to_uppercase();
        if src {
            if upper.starts_with("#+END_") {
                src = false;
            }
            if let Some(c) = current {
                s.blocks[c].end = n as u32 + 1;
            }
            continue;
        }
        let stars = line.bytes().take_while(|b| *b == b'*').count();
        if stars > 0 && line[stars..].starts_with(' ') {
            while stack.last().is_some_and(|(d, _)| *d >= stars) {
                stack.pop();
            }
            if let Some(c) = current {
                s.blocks[c].end = n as u32;
            }
            let content = line[stars..].trim();
            // Org's tags at the end: `:a:b:`.
            let content = match content.rfind(" :") {
                Some(p) if content.ends_with(':') && !content[p + 2..].contains(' ') => {
                    content[..p].trim_end()
                }
                _ => content,
            };
            let (marker, priority, text) = task(content);
            let index = s.blocks.len();
            s.blocks.push(Block {
                line: n as u32,
                end: n as u32 + 1,
                depth: stack.len() as u16,
                parent: stack.last().map(|(_, b)| *b),
                text: text.to_string(),
                marker,
                priority,
                heading: Some(stars.min(6) as u8),
                ..Block::default()
            });
            s.headings.push(Heading {
                line: n as u32,
                level: stars.min(6) as u8,
                text: text.to_string(),
            });
            let at = line.len() - line[stars..].trim_start().len();
            place(
                &mut s.refs,
                inline(&line[at..], Flavor::LogseqOrg),
                n,
                at,
                Some(index),
            );
            stack.push((stars, index));
            current = Some(index);
            continue;
        }
        if let Some(c) = current
            && !trimmed.is_empty()
        {
            s.blocks[c].end = n as u32 + 1;
        }
        if let Some(d) = drawer {
            if upper == ":END:" {
                drawer = None;
                continue;
            }
            if d == "PROPERTIES"
                && let Some(rest) = trimmed.strip_prefix(':')
                && let Some((k, v)) = rest.split_once(':')
            {
                let (k, v) = (k.to_lowercase(), v.trim().to_string());
                match current {
                    Some(c) => {
                        if k == "id" {
                            s.blocks[c].id = Some(v.to_lowercase());
                        }
                        if k == "collapsed" {
                            s.blocks[c].collapsed = v == "true";
                        }
                        s.blocks[c].props.push((k, v));
                    }
                    None => page.push((k, v, n)),
                }
            }
            continue;
        }
        if upper == ":PROPERTIES:" || upper == ":LOGBOOK:" {
            drawer = Some(if upper == ":PROPERTIES:" {
                "PROPERTIES"
            } else {
                "LOGBOOK"
            });
            continue;
        }
        if upper.starts_with("#+BEGIN_SRC")
            || upper.starts_with("#+BEGIN_EXAMPLE")
            || upper.starts_with("#+BEGIN_QUERY")
        {
            src = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#+")
            && let Some((k, v)) = rest.split_once(':')
            && current.is_none()
            && !k.contains(' ')
        {
            page.push((k.to_lowercase(), v.trim().to_string(), n));
            continue;
        }
        if let Some(c) = current
            && (upper.starts_with("SCHEDULED:")
                || upper.starts_with("DEADLINE:")
                || upper.starts_with("CLOSED:"))
        {
            let b = &mut s.blocks[c];
            b.scheduled = b.scheduled.or(planning(trimmed, "SCHEDULED:"));
            b.deadline = b.deadline.or(planning(trimmed, "DEADLINE:"));
            continue;
        }
        if current.is_none() && !trimmed.is_empty() && !trimmed.starts_with('#') {
            let index = s.blocks.len();
            s.blocks.push(Block {
                line: n as u32,
                end: n as u32 + 1,
                text: trimmed.to_string(),
                ..Block::default()
            });
            current = Some(index);
            stack.push((0, index));
        }
        let at = line.len() - line.trim_start().len();
        place(
            &mut s.refs,
            inline(&line[at..], Flavor::LogseqOrg),
            n,
            at,
            current,
        );
    }
    page_props(&mut s, &page, options, true);
    s
}

/// A list item's marker: `-`, `*`, `+`, `1.`, `1)`; its bytes with the
/// space after it.
fn list_marker(rest: &str) -> Option<usize> {
    let b = rest.as_bytes();
    match b.first()? {
        b'-' | b'*' | b'+' => (b.get(1) == Some(&b' ') || b.len() == 1).then_some(2.min(b.len())),
        c if c.is_ascii_digit() => {
            let n = b.iter().take_while(|c| c.is_ascii_digit()).count();
            (matches!(b.get(n), Some(b'.') | Some(b')')) && b.get(n + 1) == Some(&b' '))
                .then_some(n + 2)
        }
        _ => None,
    }
}

fn obsidian(lines: &[&str]) -> Scanned {
    let mut s = Scanned::default();
    let (fm, start) = front_matter(lines);
    let fm: Vec<(String, String, usize)> = fm.into_iter().map(|(k, v)| (k, v, 0)).collect();
    // Obsidian's aliases name the note; they link nowhere.
    page_props(&mut s, &fm, &Options::default(), false);
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut current: Option<usize> = None;
    let mut fence: Option<&'static str> = None;
    let mut comment = false;
    for (n, raw_line) in lines.iter().enumerate().skip(start) {
        // `%%…%%` comments, which may span lines, read as spaces so that
        // the columns stay.
        let mut line = String::with_capacity(raw_line.len());
        let mut rest = *raw_line;
        loop {
            if comment {
                match rest.find("%%") {
                    Some(p) => {
                        line.push_str(&" ".repeat(p + 2));
                        rest = &rest[p + 2..];
                        comment = false;
                    }
                    None => {
                        line.push_str(&" ".repeat(rest.len()));
                        break;
                    }
                }
            } else {
                match rest.find("%%") {
                    Some(p) => {
                        line.push_str(&rest[..p]);
                        line.push_str("  ");
                        rest = &rest[p + 2..];
                        comment = true;
                    }
                    None => {
                        line.push_str(rest);
                        break;
                    }
                }
            }
        }
        let line = line.as_str();
        let (cols, bytes) = indent(line);
        let body = &line[bytes..];
        let trimmed = body.trim_end();
        if let Some(f) = fence {
            if trimmed.starts_with(f) {
                fence = None;
            }
            if let Some(c) = current {
                s.blocks[c].end = n as u32 + 1;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(if trimmed.starts_with('`') {
                "```"
            } else {
                "~~~"
            });
            continue;
        }
        if trimmed.is_empty() {
            // A paragraph ends; a list goes on past blank lines.
            if current.is_some_and(|c| s.blocks[c].depth == 0 && stack.is_empty()) {
                current = None;
            }
            continue;
        }
        let quote = trimmed.trim_start_matches(['>', ' ']);
        let item = list_marker(body);
        let heading = md_heading(trimmed);
        let new_block = item.is_some() || heading.is_some() || current.is_none();
        if new_block {
            if item.is_some() {
                while stack.last().is_some_and(|(c, _)| *c >= cols) {
                    stack.pop();
                }
            } else {
                stack.clear();
            }
            if let Some(c) = current {
                s.blocks[c].end = s.blocks[c].end.max(n as u32).min(n as u32);
            }
            let content_at = bytes + item.unwrap_or(0);
            let mut text = line[content_at..].trim_end().to_string();
            let mut block = Block {
                line: n as u32,
                end: n as u32 + 1,
                depth: if item.is_some() {
                    stack.len() as u16
                } else {
                    0
                },
                parent: if item.is_some() {
                    stack.last().map(|(_, b)| *b)
                } else {
                    None
                },
                ..Block::default()
            };
            if item.is_some() {
                for (mark, kw) in [
                    ("[ ] ", "TODO"),
                    ("[x] ", "DONE"),
                    ("[X] ", "DONE"),
                    ("[/] ", "DOING"),
                    ("[-] ", "CANCELED"),
                ] {
                    if let Some(after) = text.strip_prefix(mark) {
                        block.marker = Some(kw.to_string());
                        text = after.to_string();
                        break;
                    }
                }
            }
            if let Some((level, t)) = heading {
                block.heading = Some(level);
                text = t.to_string();
                s.headings.push(Heading {
                    line: n as u32,
                    level,
                    text: t.trim_end().to_string(),
                });
            }
            if item.is_none() && heading.is_none() {
                text = quote.to_string();
            }
            block.text = text;
            let index = s.blocks.len();
            s.blocks.push(block);
            if item.is_some() {
                stack.push((cols, index));
            }
            current = if heading.is_some() { None } else { Some(index) };
            place(
                &mut s.refs,
                inline(&line[content_at..], Flavor::Obsidian),
                n,
                content_at,
                Some(index),
            );
            if heading.is_some() {
                // A heading is a block of its own line.
                block_id(&mut s, index, trimmed);
                continue;
            }
            block_id(&mut s, index, trimmed);
            inline_field(&mut s, index, &line[content_at..]);
            continue;
        }
        let Some(c) = current else { continue };
        s.blocks[c].end = n as u32 + 1;
        place(
            &mut s.refs,
            inline(&line[bytes..], Flavor::Obsidian),
            n,
            bytes,
            Some(c),
        );
        block_id(&mut s, c, trimmed);
        inline_field(&mut s, c, &line[bytes..]);
    }
    s
}

/// Obsidian's `^id` at the end of a line names its block.
fn block_id(s: &mut Scanned, block: usize, line: &str) {
    let t = line.trim_end();
    if let Some(p) = t
        .rfind(" ^")
        .map(|p| p + 1)
        .or_else(|| t.starts_with('^').then_some(0))
    {
        let id = &t[p + 1..];
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            let b = &mut s.blocks[block];
            b.id = Some(id.to_lowercase());
            if b.text.ends_with(&t[p..]) {
                let keep = b.text.len() - (t.len() - p);
                b.text.truncate(keep);
                let trimmed = b.text.trim_end().len();
                b.text.truncate(trimmed);
            }
        }
    }
}

/// Dataview's `key:: value` inline field on a line of its own.
fn inline_field(s: &mut Scanned, block: usize, line: &str) {
    if let Some((k, v)) = property(line) {
        s.blocks[block].props.push((k, v.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(text: &str) -> Scanned {
        scan(
            text,
            Flavor::LogseqMarkdown,
            &Options {
                comma_properties: vec!["alias".into(), "tags".into()],
            },
        )
    }

    fn targets(s: &Scanned) -> Vec<(RefKind, &str)> {
        s.refs.iter().map(|r| (r.kind, r.target.as_str())).collect()
    }

    #[test]
    fn logseq_blocks_and_properties() {
        let s = md(
            "title:: My Page\nalias:: MP, [[Other Name]]\ntags:: kalem, #notes\n\n- TODO [#A] Write [[Kalem]] docs #writing\n  id:: 6512ABCD-1111-2222-3333-444455556666\n  collapsed:: true\n  SCHEDULED: <2026-10-12 Mon .+1d>\n\t- child with ((6512abcd-1111-2222-3333-444455556666))\n\t  second line [[Inner]]\n- # A heading\n  heading:: 2\n-\n- DONE closed\n",
        );
        assert_eq!(s.title.as_deref(), Some("My Page"));
        assert_eq!(s.aliases, ["MP", "Other Name"]);
        assert_eq!(s.tags, ["kalem", "notes"]);
        assert_eq!(s.blocks.len(), 5);
        let b = &s.blocks[0];
        assert_eq!(b.marker.as_deref(), Some("TODO"));
        assert_eq!(b.priority, Some('A'));
        assert_eq!(b.text, "Write [[Kalem]] docs #writing");
        assert_eq!(
            b.id.as_deref(),
            Some("6512abcd-1111-2222-3333-444455556666")
        );
        assert!(b.collapsed);
        assert_eq!(b.scheduled, Date::new(2026, 10, 12));
        assert_eq!((b.line, b.end), (4, 8));
        let c = &s.blocks[1];
        assert_eq!((c.depth, c.parent), (1, Some(0)));
        assert_eq!((c.line, c.end), (8, 10));
        assert_eq!(s.blocks[2].heading, Some(2));
        assert_eq!(s.headings[0].text, "A heading");
        assert_eq!(s.blocks[3].text, "");
        assert_eq!(s.blocks[4].marker.as_deref(), Some("DONE"));
        assert_eq!(
            targets(&s),
            [
                (RefKind::Property, "MP"),
                (RefKind::Property, "Other Name"),
                (RefKind::Tag, "kalem"),
                (RefKind::Tag, "notes"),
                (RefKind::Page, "Kalem"),
                (RefKind::Tag, "writing"),
                (RefKind::Block, "6512abcd-1111-2222-3333-444455556666"),
                (RefKind::Page, "Inner"),
            ]
        );
        let r = s.refs.iter().find(|r| r.target == "Kalem").unwrap();
        assert_eq!((r.line, r.start, r.end, r.block), (4, 18, 27, Some(0)));
        let inner = s.refs.iter().find(|r| r.target == "Inner").unwrap();
        assert_eq!(inner.block, Some(1));
    }

    #[test]
    fn logseq_inline_forms() {
        let s = md(
            "- see [label]([[Target]]) and [x](((6512abcd-1111-2222-3333-444455556666)))\n- {{embed [[Page E]]}} {{embed ((6512abcd-1111-2222-3333-444455556666))}}\n- #[[two words]] a#b C# `[[code]]` http://x.com/#anchor [[a [[b]]]]\n- {{query (and [[q]] (task TODO))}}\n- ```\n  [[in fence]]\n  ```\n- after",
        );
        assert_eq!(
            targets(&s),
            [
                (RefKind::Page, "Target"),
                (RefKind::Block, "6512abcd-1111-2222-3333-444455556666"),
                (RefKind::EmbedPage, "Page E"),
                (RefKind::EmbedBlock, "6512abcd-1111-2222-3333-444455556666"),
                (RefKind::Tag, "two words"),
                (RefKind::Page, "a [[b]]"),
                (RefKind::Page, "b"),
                (RefKind::Page, "q"),
            ]
        );
        assert_eq!(s.blocks.last().unwrap().text, "after");
    }

    #[test]
    fn logseq_pre_block_with_a_bullet() {
        let s = md("- alias:: Short\n  type:: book\n- first\n");
        assert_eq!(s.aliases, ["Short"]);
        assert_eq!(
            s.props,
            [
                ("alias".into(), "Short".into()),
                ("type".into(), "book".into())
            ]
        );
        assert_eq!(s.blocks.len(), 1);
        assert_eq!(s.blocks[0].text, "first");
        assert_eq!(s.refs[0].target, "Short");
    }

    #[test]
    fn logseq_org() {
        let s = scan(
            "#+title: Org Page\n#+alias: OP\n\n* TODO [#B] Headline [[Link]] :tag:\n:PROPERTIES:\n:ID: 6512abcd-1111-2222-3333-444455556666\n:END:\nSCHEDULED: <2026-10-11 Sun>\n:LOGBOOK:\nCLOCK: [2026-10-10 Sat 10:00]\n:END:\n** Child ((6512abcd-1111-2222-3333-444455556666)) [[file:./other_page.org][Other]]\n#+BEGIN_SRC rust\n[[not]]\n#+END_SRC\n* Second #tag [[https://x.com][web]]\n",
            Flavor::LogseqOrg,
            &Options::default(),
        );
        assert_eq!(s.title.as_deref(), Some("Org Page"));
        assert_eq!(s.aliases, ["OP"]);
        assert_eq!(s.blocks.len(), 3);
        let b = &s.blocks[0];
        assert_eq!(b.text, "Headline [[Link]]");
        assert_eq!(b.marker.as_deref(), Some("TODO"));
        assert_eq!(b.priority, Some('B'));
        assert_eq!(
            b.id.as_deref(),
            Some("6512abcd-1111-2222-3333-444455556666")
        );
        assert_eq!(b.scheduled, Date::new(2026, 10, 11));
        assert_eq!(s.blocks[1].parent, Some(0));
        assert_eq!(
            targets(&s),
            [
                (RefKind::Property, "OP"),
                (RefKind::Page, "Link"),
                (RefKind::Block, "6512abcd-1111-2222-3333-444455556666"),
                (RefKind::Page, "other_page"),
                (RefKind::Tag, "tag"),
            ]
        );
    }

    #[test]
    fn obsidian_notes() {
        let s = scan(
            "---\naliases:\n  - Short\n  - \"Other\"\ntags: [a, b/c]\n---\n# Title\n\nA paragraph with [[Note#Heading|shown]] and [[Folder/Deep]]\ngoing on ![[pic.png|300]] ![[Note#^abc-1]] #tag #123 #nested/tag. ^para1\n\n- [ ] task [[T]]\n  - [x] done\n> [!note] A callout [[C]]\n%% hidden [[H]] %%visible [[V]]\nkey:: value\n[md link](Some%20Note.md#part) [web](https://x.com/a.md)\n",
            Flavor::Obsidian,
            &Options::default(),
        );
        assert_eq!(s.aliases, ["Short", "Other"]);
        assert_eq!(s.tags, ["a", "b/c"]);
        assert_eq!(s.headings[0].text, "Title");
        let p = &s.blocks[1];
        assert_eq!(p.id.as_deref(), Some("para1"));
        assert_eq!(
            p.text,
            "A paragraph with [[Note#Heading|shown]] and [[Folder/Deep]]"
        );
        assert_eq!((p.line, p.end), (8, 10));
        let task = &s.blocks[2];
        assert_eq!(task.marker.as_deref(), Some("TODO"));
        assert_eq!(task.text, "task [[T]]");
        assert_eq!(s.blocks[3].marker.as_deref(), Some("DONE"));
        assert_eq!(s.blocks[3].parent, Some(2));
        let got: Vec<(RefKind, &str, Option<&str>)> = s
            .refs
            .iter()
            .map(|r| (r.kind, r.target.as_str(), r.anchor.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                (RefKind::Tag, "a", None),
                (RefKind::Tag, "b/c", None),
                (RefKind::Page, "Note", Some("Heading")),
                (RefKind::Page, "Folder/Deep", None),
                (RefKind::EmbedBlock, "Note", Some("abc-1")),
                (RefKind::Tag, "tag", None),
                (RefKind::Tag, "nested/tag", None),
                (RefKind::Page, "T", None),
                (RefKind::Page, "C", None),
                (RefKind::Page, "V", None),
                (RefKind::Page, "Some Note", Some("part")),
            ]
        );
        assert!(
            s.blocks
                .iter()
                .any(|b| b.props == [("key".to_string(), "value".to_string())])
        );
    }

    #[test]
    fn the_reference_at_a_column() {
        let line = "see [[Page]] and #tag and ((6512abcd-1111-2222-3333-444455556666))";
        let r = reference_at(line, 6, Flavor::LogseqMarkdown).unwrap();
        assert_eq!((r.kind, r.target.as_str()), (RefKind::Page, "Page"));
        assert!(reference_at(line, 2, Flavor::LogseqMarkdown).is_none());
        assert_eq!(
            reference_at(line, 18, Flavor::LogseqMarkdown).unwrap().kind,
            RefKind::Tag
        );
        assert_eq!(
            reference_at(line, 40, Flavor::LogseqMarkdown).unwrap().kind,
            RefKind::Block
        );
    }

    #[test]
    fn values_and_uuids() {
        assert_eq!(split_values("a, [[b, c]], #d"), ["a", "b, c", "d"]);
        assert!(is_uuid("6512abcd-1111-2222-3333-444455556666"));
        assert!(!is_uuid("6512abcd-1111-2222-3333-44445555666"));
        assert_eq!(property("  key:: v"), Some(("key".into(), "v")));
        assert_eq!(property("Key::"), Some(("key".into(), "")));
        assert_eq!(property("a b:: v"), None);
        assert_eq!(property("http::x"), None);
    }
}
