//! What a note shows under its lines, not part of its text (Kalem's
//! plugin API 0.2.13, `graph_todo.md` GR14c): an embedded block's
//! children, an embedded page's blocks, a query's results, as Logseq and
//! Obsidian draw them in place. Each row is styled text, so many columns
//! in from the line's own text; a long list ends with how many more there
//! are, which Follow Reference opens.

use crate::index::Index;
use crate::layer::Look;
use crate::query::{Answer, Hit};
use crate::scan::{self, Block, Flavor, RefKind};

/// The rows one embed or query shows at most.
pub const MAX: usize = 24;

/// How many embeds deep an embed's rows go (an embed in an embed in …),
/// as Logseq 0.10.9 draws them; deeper, a row says so, as Logseq does.
const DEPTH: usize = 6;

/// A line drawn under a line of the note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Columns in from the line's text.
    pub indent: u32,
    /// Its text, in parts with their looks.
    pub parts: Vec<(String, Look)>,
    /// The block it shows, by its file (relative to the graph's folder)
    /// and its first line, when it shows one.
    pub block: Option<(String, u32)>,
    /// The text of a file it shows, typed into where it is drawn (Kalem's
    /// plugin API 0.2.15): the block's first line, set by the app from
    /// `block` as the file holds it.
    pub source: Option<Source>,
    /// For a row that shows a block: the least depth Shift+Tab may leave
    /// it at (an embed keeps its blocks under its own block, a page's embed
    /// at the page's top level); none: any.
    pub floor: Option<u16>,
    /// The block an embed shows: Enter makes new blocks under it, which
    /// the embed shows, not after it.
    pub root: bool,
}

/// The text of a file a row shows (Kalem's `layer-3`): in file `path`,
/// line `line` (from 0) from byte `column` to the line's end, which holds
/// `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// A whole path.
    pub path: String,
    /// From 0.
    pub line: u32,
    /// A byte of the line.
    pub column: u32,
    /// The text, to the line's end.
    pub text: String,
    /// The block of the file's outline the line starts (Kalem's plugin API
    /// 0.2.18, `layer-4`), for a Logseq note's block.
    pub outline: Option<Outline>,
}

/// The block of a file's outline a row's line starts (Kalem's `layer-4`):
/// what Enter, Tab and Shift+Tab do where the row is typed into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    /// The line after the block and the blocks under it, from 0: Tab and
    /// Shift+Tab move the lines from the block's to it.
    pub end: u32,
    /// The line Enter puts a new block at.
    pub next: u32,
    /// What the new block's line starts with.
    pub bullet: String,
    /// A step of indentation as the file writes it.
    pub step: String,
    /// Whether Tab may indent the block (a block before it at its level).
    pub indent: bool,
    /// Whether Shift+Tab may take it out a step.
    pub outdent: bool,
}

/// The block of its file's outline that starts at line `line` of `text`, a
/// Logseq note's (its blocks as scanned in `blocks`), as a row showing it
/// gives it to Kalem (plugin API 0.2.18): Enter makes a block after it and
/// the blocks under it, or the first block under it when it has some or is
/// the block an embed shows (`root`), as Logseq does; Tab puts it under
/// the block before it at its level; Shift+Tab takes it out a step, not to
/// a depth below `floor`. None for a line that starts no block.
pub fn outline(
    blocks: &[Block],
    text: &str,
    line: u32,
    floor: Option<u16>,
    root: bool,
) -> Option<Outline> {
    let i = blocks.iter().position(|b| b.line == line)?;
    let b = &blocks[i];
    let raw = text.lines().nth(line as usize)?;
    let column = content_column(raw);
    let lead = raw.len() - raw.trim_start().len();
    if column <= lead {
        return None;
    }
    // A step of indentation as the file writes a block's under another: a
    // tab, as Logseq does, or the spaces one takes.
    let bullets = ["- ", "* ", "+ "];
    let step = text
        .lines()
        .map(|l| (l, l.len() - l.trim_start().len()))
        .find(|(l, w)| *w > 0 && bullets.iter().any(|p| l[*w..].starts_with(p)))
        .map_or_else(
            || "\t".to_string(),
            |(l, w)| {
                if l.starts_with('\t') {
                    "\t".to_string()
                } else {
                    l[..w].to_string()
                }
            },
        );
    let (_, end) = crate::edit::subtree(blocks, i);
    let own = b.end.max(b.line + 1);
    let (next, bullet) = if root || end > own {
        (own, format!("{}{step}{}", &raw[..lead], &raw[lead..column]))
    } else {
        (end, raw[..column].to_string())
    };
    let before = blocks[..i].iter().rev().find(|x| x.depth <= b.depth);
    Some(Outline {
        end,
        next,
        bullet,
        step,
        indent: before.is_some_and(|x| x.depth == b.depth),
        outdent: b.depth > 0 && floor.is_none_or(|f| b.depth > f),
    })
}

/// Where a block's text starts in its first line `raw`: after its
/// indentation and its bullet (`- `, `* `, `+ `, `1. `) or an Org
/// headline's stars.
pub fn content_column(raw: &str) -> usize {
    let w = raw.len() - raw.trim_start().len();
    let rest = &raw[w..];
    for bullet in ["- ", "* ", "+ "] {
        if rest.starts_with(bullet) {
            return w + 2;
        }
    }
    if rest == "-" || rest == "*" {
        return raw.len();
    }
    let stars = rest.len() - rest.trim_start_matches('*').len();
    if stars > 0 && rest[stars..].starts_with(' ') {
        return w + stars + 1;
    }
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 && (rest[digits..].starts_with(". ") || rest[digits..].starts_with(") ")) {
        return w + digits + 2;
    }
    w
}

impl Row {
    /// The row's text, without its looks.
    pub fn text(&self) -> String {
        self.parts.iter().map(|(t, _)| t.as_str()).collect()
    }
}

fn look(f: impl FnOnce(&mut Look)) -> Look {
    let mut l = Look::default();
    f(&mut l);
    l
}

/// The rows past [`MAX`] dropped, a last one saying how many.
fn capped(mut rows: Vec<Row>, indent: u32) -> Vec<Row> {
    if rows.len() > MAX {
        let more = rows.len() - (MAX - 1);
        rows.truncate(MAX - 1);
        rows.push(Row {
            indent,
            parts: vec![(
                format!("… {more} more (Follow Reference opens them all)"),
                look(|l| l.dim = true),
            )],
            block: None,
            source: None,
            floor: None,
            root: false,
        });
    }
    rows
}

/// A block as a row: its bullet, keyword, priority and text.
fn block_row(index: &Index, rel: &str, b: &Block, indent: u32) -> Row {
    let mut parts = vec![("• ".to_string(), look(|l| l.dim = true))];
    if let Some(m) = b.marker {
        let closed = scan::is_closed(m);
        parts.push((
            format!("{m} "),
            look(|l| {
                l.todo = !closed;
                l.done = closed;
            }),
        ));
    }
    if let Some(p) = b.priority {
        parts.push((format!("[#{p}] "), look(|l| l.priority = true)));
    }
    let text = b.text.lines().next().unwrap_or("").trim();
    let done = b.marker.is_some_and(scan::is_closed);
    for (t, mut l) in pieces(index, text) {
        l.strike |= done;
        parts.push((t, l));
    }
    Row {
        indent,
        parts,
        block: Some((rel.to_string(), b.line)),
        source: None,
        floor: None,
        root: false,
    }
}

/// A block's text as it reads: a link's name, a block reference's text,
/// a tag, bold and code marks taken away.
pub fn pieces(index: &Index, text: &str) -> Vec<(String, Look)> {
    let mut out: Vec<(String, Look)> = Vec::new();
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut Vec<(String, Look)>| {
        if !plain.is_empty() {
            out.push((std::mem::take(plain), Look::default()));
        }
    };
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let pair = |open: &str, close: &str| -> Option<(&str, usize)> {
            let inner = rest.strip_prefix(open)?;
            let e = inner.find(close)?;
            Some((&inner[..e], open.len() + e + close.len()))
        };
        if let Some((inner, n)) = pair("{{embed ", "}}") {
            // An embed, shown under its row: its block's page or its page.
            flush(&mut plain, &mut out);
            let inner = inner.trim();
            let shown = match inner.strip_prefix("((").and_then(|i| i.strip_suffix("))")) {
                Some(id) => index
                    .block_place(&id.to_lowercase())
                    .and_then(|(rel, _)| index.page_of(rel))
                    .map_or_else(|| "a block".to_string(), |p| p.shown_title().to_string()),
                None => inner
                    .trim_start_matches("[[")
                    .trim_end_matches("]]")
                    .to_string(),
            };
            out.push((format!("↳ {shown}"), look(|l| l.link = true)));
            rest = &rest[n..];
        } else if let Some((name, n)) = pair("![[", "]]") {
            // Obsidian's embed.
            flush(&mut plain, &mut out);
            let name = name.split_once('|').map_or(name, |(note, _)| note);
            out.push((format!("↳ {name}"), look(|l| l.link = true)));
            rest = &rest[n..];
        } else if let Some((name, n)) = pair("[[", "]]") {
            flush(&mut plain, &mut out);
            let shown = name.split_once('|').map_or(name, |(_, label)| label);
            out.push((shown.to_string(), look(|l| l.link = true)));
            rest = &rest[n..];
        } else if let Some((id, n)) = pair("((", "))").filter(|(id, _)| scan::is_uuid(id)) {
            flush(&mut plain, &mut out);
            let shown = index
                .block(&id.to_lowercase())
                .map_or(id.to_string(), |(_, b)| {
                    b.text.lines().next().unwrap_or("").trim().to_string()
                });
            out.push((shown, look(|l| l.link = true)));
            rest = &rest[n..];
        } else if let Some((t, n)) = pair("**", "**") {
            flush(&mut plain, &mut out);
            out.push((t.to_string(), look(|l| l.bold = true)));
            rest = &rest[n..];
        } else if let Some((t, n)) = pair("`", "`") {
            flush(&mut plain, &mut out);
            out.push((t.to_string(), look(|l| l.code = true)));
            rest = &rest[n..];
        } else if c == '#'
            && (plain.is_empty() || plain.ends_with(' '))
            && rest[1..]
                .chars()
                .next()
                .is_some_and(|d| d.is_alphanumeric() || d == '[')
        {
            flush(&mut plain, &mut out);
            let n = if rest[1..].starts_with("[[") {
                rest.find("]]").map_or(rest.len(), |e| e + 2)
            } else {
                rest.find(|d: char| d.is_whitespace() || d == ',')
                    .unwrap_or(rest.len())
            };
            out.push((rest[..n].to_string(), look(|l| l.tag = true)));
            rest = &rest[n..];
        } else {
            plain.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    flush(&mut plain, &mut out);
    out
}

/// What `{{embed ((id))}}` shows under its line, which names the block's
/// page: the block, then its children a level in for each, each typed
/// into where it shows, and under a block the embeds in it.
pub fn embedded_block(index: &Index, id: &str) -> Vec<Row> {
    capped(block_rows(index, id, 1), 4)
}

/// [`embedded_block`]'s rows, not capped, for an embed `depth` embeds deep.
fn block_rows(index: &Index, id: &str, depth: usize) -> Vec<Row> {
    let Some((rel, i)) = index.block_place(id) else {
        return Vec::new();
    };
    let Some(blocks) = index.file(rel).map(|f| &f.scanned.blocks) else {
        return Vec::new();
    };
    let Some(level) = blocks.get(i).map(|b| b.depth) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for (k, b) in blocks[i..]
        .iter()
        .enumerate()
        .take_while(|(k, b)| *k == 0 || b.depth > level)
    {
        let mut r = block_row(index, rel, b, 2 + 2 * u32::from(b.depth - level));
        // Its own block, and the blocks under it, kept the embed's.
        r.root = k == 0;
        r.floor = Some(if k == 0 { level } else { level + 1 });
        let indent = r.indent;
        rows.push(r);
        rows.extend(nested(index, rel, b, indent, depth));
    }
    rows
}

/// What `{{embed [[page]]}}` shows under its line: the page's blocks, or
/// those under its heading `anchor` (Obsidian's `![[note#Heading]]`), and
/// under a block the embeds in it.
pub fn embedded_page(index: &Index, key: &str, anchor: Option<&str>) -> Vec<Row> {
    capped(page_rows(index, key, anchor, 1), 2)
}

/// [`embedded_page`]'s rows, not capped, for an embed `depth` embeds deep.
fn page_rows(index: &Index, key: &str, anchor: Option<&str>, depth: usize) -> Vec<Row> {
    let Some(rel) = index.page(key).and_then(|p| p.path.as_deref()) else {
        return Vec::new();
    };
    let Some(file) = index.file(rel) else {
        return Vec::new();
    };
    let s = &file.scanned;
    let (from, to) = match anchor.filter(|a| !a.starts_with('^')) {
        Some(a) => {
            let Some(h) = s
                .headings
                .iter()
                .position(|h| h.text.eq_ignore_ascii_case(a))
            else {
                return Vec::new();
            };
            let level = s.headings[h].level;
            let end = s.headings[h + 1..]
                .iter()
                .find(|n| n.level <= level)
                .map_or(u32::MAX, |n| n.line);
            (s.headings[h].line + 1, end)
        }
        None => (0, u32::MAX),
    };
    let blocks: Vec<&Block> = s
        .blocks
        .iter()
        .filter(|b| b.line >= from && b.line < to && !b.text.trim().is_empty())
        .collect();
    let top = blocks.iter().map(|b| b.depth).min().unwrap_or(0);
    let mut rows = Vec::new();
    for b in blocks {
        let mut r = block_row(index, rel, b, 2 + 2 * u32::from(b.depth - top));
        r.floor = Some(top);
        let indent = r.indent;
        rows.push(r);
        rows.extend(nested(index, rel, b, indent, depth));
    }
    rows
}

/// The rows of the embeds in the first line of block `b` of file `rel`,
/// itself in an embed `depth` embeds deep, under its row at `indent`: an
/// embed in an embed, drawn as Logseq draws it; past [`DEPTH`], a row
/// saying so (a block that embeds itself ends there).
fn nested(index: &Index, rel: &str, b: &Block, indent: u32, depth: usize) -> Vec<Row> {
    let line = b.text.lines().next().unwrap_or("");
    if !line.contains("{{") && !line.contains("![[") {
        return Vec::new();
    }
    let flavor = match index.graph.kind {
        crate::config::Kind::Obsidian => Flavor::Obsidian,
        _ if rel.to_lowercase().ends_with(".org") => Flavor::LogseqOrg,
        _ => Flavor::LogseqMarkdown,
    };
    let mut out = Vec::new();
    for r in scan::references(line, flavor) {
        if !matches!(r.kind, RefKind::EmbedBlock | RefKind::EmbedPage) {
            continue;
        }
        let rows = if depth >= DEPTH {
            vec![Row {
                indent: 2,
                parts: vec![(
                    "Embed depth is too deep".to_string(),
                    look(|l| l.dim = true),
                )],
                block: None,
                source: None,
                floor: None,
                root: false,
            }]
        } else if r.kind == RefKind::EmbedBlock {
            let id = match flavor {
                Flavor::Obsidian => format!(
                    "{}#^{}",
                    index.resolve(&r.target, rel),
                    r.anchor.as_deref().unwrap_or("").to_lowercase()
                ),
                _ => r.target.clone(),
            };
            block_rows(index, &id, depth + 1)
        } else {
            let key = index.resolve(&r.target, rel);
            page_rows(index, &key, r.anchor.as_deref(), depth + 1)
        };
        out.extend(rows.into_iter().map(|mut row| {
            row.indent += indent;
            row
        }));
    }
    out
}

/// A query's results under its line: its pages, or its blocks under the
/// pages they are in.
pub fn hits(index: &Index, answer: &Answer) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut page: Option<&str> = None;
    for h in &answer.hits {
        match h {
            Hit::Page(k) => {
                let title = index.page(k).map_or(k.as_str(), |p| p.title.as_str());
                rows.push(Row {
                    indent: 2,
                    parts: vec![
                        ("• ".into(), look(|l| l.dim = true)),
                        (title.to_string(), look(|l| l.link = true)),
                    ],
                    block: None,
                    source: None,
                    floor: None,
                    root: false,
                });
            }
            Hit::Block { rel, block } => {
                let Some(file) = index.file(rel) else {
                    continue;
                };
                let Some(b) = file.scanned.blocks.get(*block) else {
                    continue;
                };
                if page != Some(file.key.as_str()) {
                    page = Some(file.key.as_str());
                    let title = index
                        .page(&file.key)
                        .map_or(file.key.as_str(), |p| p.title.as_str());
                    rows.push(Row {
                        indent: 2,
                        parts: vec![(
                            title.to_string(),
                            look(|l| {
                                l.link = true;
                                l.bold = true;
                            }),
                        )],
                        block: None,
                        source: None,
                        floor: None,
                        root: false,
                    });
                }
                rows.push(block_row(index, rel, b, 4));
            }
            // A page's properties: the page, as the block that names.
            Hit::Props { rel } => {
                let title = index
                    .page_of(rel)
                    .map_or(rel.as_str(), |p| p.title.as_str());
                rows.push(Row {
                    indent: 2,
                    parts: vec![
                        (
                            title.to_string(),
                            look(|l| {
                                l.link = true;
                                l.bold = true;
                            }),
                        ),
                        (
                            format!("  {}", crate::query::hit_text(index, h)),
                            look(|l| l.dim = true),
                        ),
                    ],
                    block: None,
                    source: None,
                    floor: None,
                    root: false,
                });
            }
        }
    }
    capped(rows, 2)
}

/// An advanced query's table of values under its line: the columns, then
/// a row each.
pub fn table(columns: &[String], values: &[Vec<String>]) -> Vec<Row> {
    let sep = || ("  │  ".to_string(), look(|l| l.dim = true));
    let line = |cells: &[String], l: Look| {
        let mut parts = Vec::new();
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                parts.push(sep());
            }
            parts.push((c.clone(), l));
        }
        Row {
            indent: 2,
            parts,
            block: None,
            source: None,
            floor: None,
            root: false,
        }
    };
    let mut rows = vec![line(columns, look(|l| l.bold = true))];
    rows.extend(values.iter().map(|v| line(v, Look::default())));
    capped(rows, 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Fallbacks, Graph, Kind};
    use crate::files::Memory;

    fn index() -> Index {
        let m = Memory::new(&[
            ("/g/logseq/config.edn", "{}"),
            (
                "/g/pages/A.md",
                "- The core\n  id:: 6512c0de-0001-4000-8000-000000000001\n\t- TODO [#A] its **first** part, see [[B]] #idea\n\t\t- deeper, `code`\n\t- DONE a second\n- after it\n",
            ),
            (
                "/g/pages/B.md",
                "- One\n- Two refers to ((6512c0de-0001-4000-8000-000000000001))\n",
            ),
        ]);
        Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        )
    }

    fn texts(rows: &[Row]) -> Vec<(u32, String)> {
        rows.iter().map(|r| (r.indent, r.text())).collect()
    }

    #[test]
    fn an_embedded_block_and_its_children() {
        let i = index();
        let rows = embedded_block(&i, "6512c0de-0001-4000-8000-000000000001");
        assert_eq!(
            texts(&rows),
            [
                (2, "• The core".to_string()),
                (4, "• TODO [#A] its first part, see B #idea".to_string()),
                (6, "• deeper, code".to_string()),
                (4, "• DONE a second".to_string()),
            ]
        );
        // Looks: the keyword, the link, the tag; a done block struck.
        let first = &rows[1].parts;
        assert!(first.iter().any(|(t, l)| t == "TODO " && l.todo));
        assert!(first.iter().any(|(t, l)| t == "first" && l.bold));
        assert!(first.iter().any(|(t, l)| t == "B" && l.link));
        assert!(first.iter().any(|(t, l)| t == "#idea" && l.tag));
        assert!(
            rows[3]
                .parts
                .iter()
                .any(|(t, l)| t == "a second" && l.strike)
        );
        assert!(embedded_block(&i, "6512c0de-0001-4000-8000-00000000dead").is_empty());
    }

    #[test]
    fn an_embedded_pages_blocks() {
        let i = index();
        assert_eq!(
            texts(&embedded_page(&i, "b", None)),
            [
                (2, "• One".to_string()),
                (2, "• Two refers to The core".to_string()),
            ]
        );
        assert!(embedded_page(&i, "nowhere", None).is_empty());
    }

    /// An embed in an embed, as Logseq 0.10.9 draws it: the block or page
    /// it embeds under the row of the block holding it; a block that
    /// embeds itself six deep, then a row saying so, as Logseq's.
    #[test]
    fn an_embed_in_an_embed() {
        let m = Memory::new(&[
            ("/g/logseq/config.edn", "{}"),
            (
                "/g/pages/Nest.md",
                "- Outer\n  id:: 6512c0de-0002-4000-8000-000000000001\n\t- {{embed ((6512c0de-0002-4000-8000-000000000002))}}\n\t- {{embed [[Plain]]}}\n- Self {{embed ((6512c0de-0002-4000-8000-000000000003))}}\n  id:: 6512c0de-0002-4000-8000-000000000003\n",
            ),
            (
                "/g/pages/Inner.md",
                "- Inner\n  id:: 6512c0de-0002-4000-8000-000000000002\n\t- its child\n",
            ),
            ("/g/pages/Plain.md", "- one\n- two\n"),
        ]);
        let i = Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        );
        assert_eq!(
            texts(&embedded_block(&i, "6512c0de-0002-4000-8000-000000000001")),
            [
                (2, "• Outer".to_string()),
                (4, "• ↳ Inner".to_string()),
                (6, "• Inner".to_string()),
                (8, "• its child".to_string()),
                (4, "• ↳ Plain".to_string()),
                (6, "• one".to_string()),
                (6, "• two".to_string()),
            ]
        );
        let rows = embedded_block(&i, "6512c0de-0002-4000-8000-000000000003");
        let selves: Vec<u32> = rows
            .iter()
            .filter(|r| r.text() == "• Self ↳ Nest")
            .map(|r| r.indent)
            .collect();
        assert_eq!(selves, [2, 4, 6, 8, 10, 12]);
        assert_eq!(
            rows.last().map(|r| (r.indent, r.text())),
            Some((14, "Embed depth is too deep".to_string()))
        );
        // The rows of a nested embed are its blocks', typed into in their
        // own page.
        let inner = &embedded_block(&i, "6512c0de-0002-4000-8000-000000000001")[2];
        assert_eq!(inner.block, Some(("pages/Inner.md".to_string(), 0)));
        assert!(inner.root);
    }

    #[test]
    fn query_results_under_their_pages_and_capped() {
        let i = index();
        let q = crate::query::parse("(task TODO DONE)").unwrap();
        let a = crate::query::run(&q, &i, None);
        assert_eq!(
            texts(&hits(&i, &a)),
            [
                (2, "A".to_string()),
                (4, "• TODO [#A] its first part, see B #idea".to_string()),
                (4, "• DONE a second".to_string()),
            ]
        );
        let many = Answer {
            hits: (0..40).map(|n| Hit::Page(format!("p{n}"))).collect(),
            pages: true,
            notes: Vec::new(),
        };
        let rows = hits(&i, &many);
        assert_eq!(rows.len(), MAX);
        assert_eq!(
            rows[MAX - 1].text(),
            "… 17 more (Follow Reference opens them all)"
        );
        let t = table(
            &["page".into(), "n".into()],
            &[vec!["A".into(), "3".into()]],
        );
        assert_eq!(
            texts(&t),
            [(2, "page  │  n".to_string()), (2, "A  │  3".to_string())]
        );
    }
}
