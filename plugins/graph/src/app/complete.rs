//! Completion in a graph's notes (GR8b, Kalem's plugin API 0.2.11): after
//! `[[` the graph's pages and aliases, written as the graph writes links;
//! after `#` its tags; after `((` its blocks that have an id (Logseq);
//! after `<` Logseq's `#+BEGIN_` blocks. From the index only: a graph not
//! indexed yet offers nothing, and Kalem's own completers answer.

use super::App;
use crate::config::Kind;
use crate::files;
use crate::index::Index;

/// The most items offered at once.
const MOST: usize = 50;

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// A page or a block.
    Link,
    /// A tag.
    Tag,
    /// A block of text to fill.
    Snippet,
}

/// A completion: `insert` in place of the text from byte `start` of the
/// document to the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// What the menu shows.
    pub label: String,
    /// What replaces the text.
    pub insert: String,
    /// Where the replaced text starts, a byte of the document.
    pub start: usize,
    /// Where the cursor goes in `insert`, when not after it.
    pub cursor: Option<usize>,
    /// What it is.
    pub what: What,
    /// A line more about it.
    pub detail: String,
}

/// Logseq's `<` blocks: the name shown, the block's type, its option.
const BLOCKS: &[(&str, &str, &str)] = &[
    ("Quote", "QUOTE", ""),
    ("Src", "SRC", ""),
    ("Query", "QUERY", ""),
    ("Latex export", "EXPORT", " latex"),
    ("Note", "NOTE", ""),
    ("Tip", "TIP", ""),
    ("Important", "IMPORTANT", ""),
    ("Caution", "CAUTION", ""),
    ("Pinned", "PINNED", ""),
    ("Warning", "WARNING", ""),
    ("Example", "EXAMPLE", ""),
    ("Export", "EXPORT", ""),
    ("Verse", "VERSE", ""),
    ("Ascii", "EXPORT", " ascii"),
    ("Center", "CENTER", ""),
    ("Comment", "COMMENT", ""),
];

/// How well `name` matches what was typed, `typed` in lower case: a
/// start first, then anywhere; `None` for no match.
fn matches(name: &str, typed: &str) -> Option<u8> {
    let n = name.to_lowercase();
    if n.starts_with(typed) {
        Some(0)
    } else if n.contains(typed) {
        Some(1)
    } else {
        None
    }
}

impl App {
    /// The items of completer `completer` (the manifest's `refs`) for
    /// the note at `path` whose text from byte `base` is `text`, the
    /// cursor at byte `cursor` of the document.
    pub fn complete(
        &self,
        completer: &str,
        path: Option<&str>,
        text: &str,
        base: usize,
        cursor: usize,
    ) -> Vec<Suggestion> {
        if completer != "refs" {
            return Vec::new();
        }
        let Some(path) = path.map(files::normalize) else {
            return Vec::new();
        };
        let Some((index, rel)) = self
            .indexes
            .iter()
            .filter(|(r, _)| files::relative(r, &path).is_some())
            .max_by_key(|(r, _)| r.len())
            .map(|(r, i)| (i, files::relative(r, &path).unwrap_or("").to_string()))
        else {
            return Vec::new();
        };
        let at = cursor.saturating_sub(base).min(text.len());
        if !text.is_char_boundary(at) {
            return Vec::new();
        }
        let before = &text[..at];
        let line = &before[before.rfind('\n').map_or(0, |i| i + 1)..];
        let after = &text[at..];
        let org = rel.to_lowercase().ends_with(".org");
        if let Some(open) = line.rfind("[[") {
            let typed = &line[open + 2..];
            if !typed.contains("]]") && !typed.contains('|') && !typed.contains('#') {
                return self.pages(index, &rel, typed, cursor, after.starts_with("]]"));
            }
        }
        if index.graph.kind == Kind::Logseq
            && let Some(open) = line.rfind("((")
        {
            let typed = &line[open + 2..];
            if !typed.contains("))") {
                return blocks(index, typed, cursor, after.starts_with("))"));
            }
        }
        if let Some(hash) = line.rfind('#') {
            let typed = &line[hash + 1..];
            let starts = line[..hash]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
            if starts && !typed.contains(char::is_whitespace) && !typed.contains(['[', ']', '#']) {
                return tags(index, typed, cursor);
            }
        }
        if index.graph.kind == Kind::Logseq
            && !org
            && let Some(lt) = line.rfind('<')
        {
            let typed = &line[lt + 1..];
            let starts = line[..lt]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
            if starts && typed.chars().all(char::is_alphanumeric) {
                return block_types(line, typed, cursor);
            }
        }
        Vec::new()
    }

    /// The pages whose title or alias holds `typed`, as links from the
    /// note `rel` replacing the `[[` and what follows it (a `]]` after the
    /// cursor kept).
    fn pages(
        &self,
        index: &Index,
        rel: &str,
        typed: &str,
        cursor: usize,
        closed: bool,
    ) -> Vec<Suggestion> {
        let lower = typed.to_lowercase();
        let mut found: Vec<(u8, bool, String, String, &str)> = Vec::new();
        for p in index.pages() {
            if let Some(m) = matches(&p.title, &lower) {
                found.push((
                    m,
                    p.journal.is_some(),
                    p.title.clone(),
                    String::new(),
                    &p.key,
                ));
            }
            for a in &p.aliases {
                if let Some(m) = matches(a, &lower) {
                    found.push((m, false, a.clone(), format!("alias of {}", p.title), &p.key));
                }
            }
        }
        found.sort_by(|a, b| {
            (a.0, a.1, a.2.len(), a.2.to_lowercase()).cmp(&(
                b.0,
                b.1,
                b.2.len(),
                b.2.to_lowercase(),
            ))
        });
        found.truncate(MOST);
        let start = cursor - typed.len() - 2;
        let mut out: Vec<(u8, bool, Suggestion)> = found
            .into_iter()
            .filter_map(|(m, journal, label, detail, key)| {
                let link = if detail.is_empty() {
                    self.link_text(&index.graph.root, key, Some(rel))?
                } else {
                    // An alias is written as typed: Logseq resolves it.
                    format!("[[{label}]]")
                };
                // A vault's notes of one name told apart by the path
                // their link names (`Deep/Note`).
                let label = match link.strip_prefix("[[").and_then(|l| l.strip_suffix("]]")) {
                    Some(target) if index.graph.kind == Kind::Obsidian && detail.is_empty() => {
                        target.to_string()
                    }
                    _ => label,
                };
                let insert = match link.strip_suffix("]]") {
                    Some(open) if closed => open.to_string(),
                    _ => link,
                };
                let detail = if detail.is_empty() {
                    index
                        .page(key)
                        .and_then(|p| p.path.clone())
                        .unwrap_or_else(|| "no file yet".into())
                } else {
                    detail
                };
                Some((
                    m,
                    journal,
                    Suggestion {
                        label,
                        insert,
                        start,
                        cursor: None,
                        what: What::Link,
                        detail,
                    },
                ))
            })
            .collect();
        out.sort_by_key(|(m, journal, s)| (*m, *journal, s.label.len()));
        out.into_iter().map(|(_, _, s)| s).collect()
    }
}

/// The blocks with an id whose first line holds `typed`, as `((uuid))`
/// after the `((`.
fn blocks(index: &Index, typed: &str, cursor: usize, closed: bool) -> Vec<Suggestion> {
    let lower = typed.trim().to_lowercase();
    let mut found: Vec<(u8, &str, &str, &str)> = index
        .blocks_with_ids()
        .filter_map(|(id, rel, b)| {
            let first = b.text.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                return None;
            }
            let m = if lower.is_empty() {
                Some(0)
            } else {
                matches(first, &lower)
            }?;
            Some((m, first, id, rel))
        })
        .collect();
    found.sort_by_key(|f| (f.0, f.1.len()));
    found.truncate(MOST);
    found
        .into_iter()
        .map(|(_, first, id, rel)| {
            let label: String = first.chars().take(80).collect();
            Suggestion {
                label,
                insert: if closed {
                    id.to_string()
                } else {
                    format!("{id}))")
                },
                start: cursor - typed.len(),
                cursor: None,
                what: What::Link,
                detail: index
                    .page_of(rel)
                    .map_or(rel.to_string(), |p| p.title.clone()),
            }
        })
        .collect()
}

/// The tags holding `typed`: Logseq's pages (a title with a space as
/// `#[[title]]`), Obsidian's tags.
fn tags(index: &Index, typed: &str, cursor: usize) -> Vec<Suggestion> {
    let lower = typed.to_lowercase();
    let mut found: Vec<(u8, usize, String)> = match index.graph.kind {
        Kind::Logseq => index
            .pages()
            .filter(|p| p.journal.is_none())
            .filter_map(|p| {
                let m = matches(&p.title, &lower)?;
                Some((m, index.backlinks(&p.key).len(), p.title.clone()))
            })
            .collect(),
        Kind::Obsidian => index
            .tags()
            .values()
            .filter_map(|(name, refs)| Some((matches(name, &lower)?, refs.len(), name.clone())))
            .collect(),
    };
    found.sort_by(|a, b| {
        (a.0, std::cmp::Reverse(a.1), &a.2).cmp(&(b.0, std::cmp::Reverse(b.1), &b.2))
    });
    found.truncate(MOST);
    found
        .into_iter()
        .map(|(_, uses, name)| Suggestion {
            insert: if name.contains(char::is_whitespace) {
                format!("[[{name}]]")
            } else {
                name.clone()
            },
            label: name,
            start: cursor - typed.len(),
            cursor: None,
            what: What::Tag,
            detail: format!("{uses} uses"),
        })
        .collect()
}

/// Logseq's `<` blocks matching `typed`, each written in place of the
/// `<` with its lines indented as the block's, the cursor on its empty
/// line, as Logseq writes them.
fn block_types(line: &str, typed: &str, cursor: usize) -> Vec<Suggestion> {
    let lower = typed.to_lowercase();
    let lead = line.len() - line.trim_start().len();
    let indent = if line.trim_start().starts_with("- ") {
        format!("{}  ", &line[..lead])
    } else {
        line[..lead].to_string()
    };
    let mut found: Vec<(u8, usize)> = BLOCKS
        .iter()
        .enumerate()
        .filter_map(|(i, (name, _, _))| Some((matches(name, &lower)?, i)))
        .collect();
    found.sort();
    found
        .into_iter()
        .map(|(_, i)| {
            let (name, kind, option) = BLOCKS[i];
            let (open, close) = if kind == "SRC" {
                ("``` ".to_string(), format!("{indent}```"))
            } else {
                (
                    format!("#+BEGIN_{kind}{option}"),
                    format!("{indent}#+END_{kind}"),
                )
            };
            let head = format!("{open}\n{indent}");
            Suggestion {
                label: name.to_string(),
                cursor: Some(head.len()),
                insert: format!("{head}\n{close}"),
                start: cursor - typed.len() - 1,
                what: What::Snippet,
                detail: if kind == "SRC" {
                    "```".into()
                } else {
                    format!("#+BEGIN_{kind}")
                },
            }
        })
        .collect()
}
