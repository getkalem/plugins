//! What a note shows under its lines, not part of its text (Kalem's
//! plugin API 0.2.13, `graph_todo.md` GR14c): an embedded block's
//! children, an embedded page's blocks, a query's results, as Logseq and
//! Obsidian draw them in place. Each row is styled text, so many columns
//! in from the line's own text; a long list ends with how many more there
//! are, which Follow Reference opens.

use crate::index::Index;
use crate::layer::Look;
use crate::query::{Answer, Hit};
use crate::scan::{self, Block};

/// The rows one embed or query shows at most.
pub const MAX: usize = 24;

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
        if let Some((name, n)) = pair("[[", "]]") {
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
/// into where it shows.
pub fn embedded_block(index: &Index, id: &str) -> Vec<Row> {
    let Some((rel, i)) = index.block_place(id) else {
        return Vec::new();
    };
    let Some(blocks) = index.file(rel).map(|f| &f.scanned.blocks) else {
        return Vec::new();
    };
    let Some(depth) = blocks.get(i).map(|b| b.depth) else {
        return Vec::new();
    };
    let rows = blocks[i..]
        .iter()
        .enumerate()
        .take_while(|(k, b)| *k == 0 || b.depth > depth)
        .map(|(_, b)| block_row(index, rel, b, 2 + 2 * u32::from(b.depth - depth)))
        .collect();
    capped(rows, 4)
}

/// What `{{embed [[page]]}}` shows under its line: the page's blocks, or
/// those under its heading `anchor` (Obsidian's `![[note#Heading]]`).
pub fn embedded_page(index: &Index, key: &str, anchor: Option<&str>) -> Vec<Row> {
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
    let rows = blocks
        .iter()
        .map(|b| block_row(index, rel, b, 2 + 2 * u32::from(b.depth - top)))
        .collect();
    capped(rows, 2)
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
