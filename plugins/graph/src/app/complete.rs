//! Completion in a graph's notes (GR8b, Kalem's plugin API 0.2.11): after
//! `[[` the graph's pages and aliases, written as the graph writes links;
//! after `#` its tags; after `((` its blocks that have an id (Logseq);
//! after `<` Logseq's `#+BEGIN_` blocks; after `::` the properties the
//! graph uses, and after `key::` that property's values; after `/`
//! Logseq's commands that write text. From the index
//! only: a graph not indexed yet offers nothing, and Kalem's own
//! completers answer.

use std::collections::BTreeMap;
use std::rc::Rc;

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

/// The properties a graph uses: each key (in lower case) with how often
/// it is used and its values with theirs.
#[derive(Debug, Default)]
pub struct Properties {
    keys: BTreeMap<String, (usize, BTreeMap<String, usize>)>,
}

impl Properties {
    /// Counted over every page's and block's properties, the values of a
    /// property separated by commas (`tags::`, `alias::`) one by one.
    fn of(index: &Index) -> Properties {
        let g = &index.graph;
        let mut keys: BTreeMap<String, (usize, BTreeMap<String, usize>)> = BTreeMap::new();
        let mut add = |k: &str, v: &str| {
            if crate::layer::hidden_property(k, &g.hidden_properties) {
                return;
            }
            let e = keys.entry(k.to_string()).or_default();
            e.0 += 1;
            let values: Vec<&str> = if g.comma_properties.iter().any(|c| c == k) {
                v.split(',').collect()
            } else {
                vec![v]
            };
            for v in values.into_iter().map(str::trim).filter(|v| !v.is_empty()) {
                *e.1.entry(v.to_string()).or_default() += 1;
            }
        };
        for (_, d) in index.files() {
            for (k, v) in &d.scanned.props {
                add(k, v);
            }
            for b in &d.scanned.blocks {
                for (k, v) in &b.props {
                    add(k, v);
                }
            }
        }
        Properties { keys }
    }
}

/// Logseq's `/` commands that write text (its `commands-map`): the name,
/// what is written, where the cursor goes from its end, and whether only
/// at a block's start (a keyword, a priority, a heading).
const SLASH: &[(&str, &str, usize, bool)] = &[
    ("Page reference", "[[]]", 2, false),
    ("Page embed", "{{embed [[]]}}", 4, false),
    ("Block reference", "(())", 2, false),
    ("Block embed", "{{embed (())}}", 4, false),
    ("Link", "[]()", 3, false),
    ("Image link", "![]()", 3, false),
    ("Underline", "<ins></ins>", 6, false),
    ("h1", "# ", 0, true),
    ("h2", "## ", 0, true),
    ("h3", "### ", 0, true),
    ("h4", "#### ", 0, true),
    ("h5", "##### ", 0, true),
    ("h6", "###### ", 0, true),
    ("LATER", "LATER ", 0, true),
    ("NOW", "NOW ", 0, true),
    ("TODO", "TODO ", 0, true),
    ("DOING", "DOING ", 0, true),
    ("DONE", "DONE ", 0, true),
    ("WAITING", "WAITING ", 0, true),
    ("CANCELED", "CANCELED ", 0, true),
    ("A", "[#A] ", 0, true),
    ("B", "[#B] ", 0, true),
    ("C", "[#C] ", 0, true),
    ("Query", "{{query }}", 2, false),
    ("Query function", "{{function }}", 2, false),
    ("Embed Video URL", "{{video }}", 2, false),
    ("Embed Twitter tweet", "{{tweet }}", 2, false),
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
            if !typed.contains("]]") && !typed.contains('|') {
                let closed = after.starts_with("]]");
                match typed.split_once('#') {
                    None => return self.pages(index, &rel, typed, cursor, closed),
                    Some((note, part)) if index.graph.kind == Kind::Obsidian => {
                        return anchors(index, &rel, note, part, cursor, closed);
                    }
                    Some(_) => {}
                }
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
        if !org && let Some(found) = self.properties(index, line, cursor) {
            return found;
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
            && let Some(found) = self.slash(index, line, cursor)
        {
            return found;
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

    /// After `::` at the start of a block's text or a line of its
    /// properties, the keys the graph uses, written `key:: `; after
    /// `key::`, that key's values. `None` when the line is neither.
    fn properties(&self, index: &Index, line: &str, cursor: usize) -> Option<Vec<Suggestion>> {
        let content = line.trim_start();
        let content = content.strip_prefix("- ").unwrap_or(content);
        let (key, after) = content.split_once("::")?;
        let is_key = |k: &str| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        };
        let stats = self.property_stats(index);
        if key.is_empty() {
            // `::typed`: a key.
            if !after.is_empty() && !is_key(after) {
                return None;
            }
            let lower = after.to_lowercase();
            let mut found: Vec<(u8, usize, &String)> = stats
                .keys
                .iter()
                .filter_map(|(k, (n, _))| Some((matches(k, &lower)?, *n, k)))
                .collect();
            found.sort_by(|a, b| {
                (a.0, std::cmp::Reverse(a.1), a.2).cmp(&(b.0, std::cmp::Reverse(b.1), b.2))
            });
            found.truncate(MOST);
            let start = cursor - after.len() - 2;
            return Some(
                found
                    .into_iter()
                    .map(|(_, n, k)| Suggestion {
                        label: k.clone(),
                        insert: format!("{k}:: "),
                        start,
                        cursor: None,
                        what: What::Snippet,
                        detail: format!("{n} uses"),
                    })
                    .collect(),
            );
        }
        let key = key.trim().to_lowercase();
        if !is_key(&key) || !(after.is_empty() || after.starts_with(' ')) {
            return None;
        }
        // `key:: typed`: a value, after the last comma of a property
        // separated by commas.
        let comma = index.graph.comma_properties.contains(&key);
        let typed = if comma {
            after.rsplit(',').next().unwrap_or("")
        } else {
            after
        }
        .trim_start();
        let (_, values) = stats.keys.get(&key)?;
        let lower = typed.to_lowercase();
        let mut found: Vec<(u8, usize, &String)> = values
            .iter()
            .filter(|(v, _)| v.as_str() != typed)
            .filter_map(|(v, n)| Some((matches(v, &lower)?, *n, v)))
            .collect();
        found.sort_by(|a, b| {
            (a.0, std::cmp::Reverse(a.1), a.2).cmp(&(b.0, std::cmp::Reverse(b.1), b.2))
        });
        found.truncate(MOST);
        let start = cursor - typed.len();
        Some(
            found
                .into_iter()
                .map(|(_, n, v)| Suggestion {
                    label: v.clone(),
                    insert: if after.is_empty() {
                        format!(" {v}")
                    } else {
                        v.clone()
                    },
                    start,
                    cursor: None,
                    what: What::Snippet,
                    detail: format!("{n} uses"),
                })
                .collect(),
        )
    }

    /// After a `/` that starts a word, Logseq's commands that write text
    /// whose name holds what follows it, in place of the `/`. `None`
    /// when the line has no such `/`.
    fn slash(&self, index: &Index, line: &str, cursor: usize) -> Option<Vec<Suggestion>> {
        let at = line.rfind('/')?;
        let before = &line[..at];
        if !before.chars().next_back().is_none_or(char::is_whitespace) {
            return None;
        }
        let typed = &line[at + 1..];
        if typed.len() > 24
            || typed.contains("  ")
            || !typed.chars().all(|c| c.is_alphanumeric() || c == ' ')
        {
            return None;
        }
        let content = before.trim();
        let at_start = matches!(content, "" | "-");
        let lead = line.len() - line.trim_start().len();
        let indent = if content == "-" {
            format!("{}  ", &line[..lead])
        } else {
            line[..lead].to_string()
        };
        let journal = |d: Option<crate::date::Date>| {
            d.map(|d| format!("[[{}]]", index.graph.journal_title.format(d)))
        };
        let today = self.known_today();
        let time = self.local_now.map(|now| {
            let s = now().rem_euclid(86_400_000) / 1000;
            format!("{:02}:{:02}", s / 3600, s / 60 % 60)
        });
        let mut all: Vec<(String, String, usize)> = SLASH
            .iter()
            .filter(|(_, _, _, start)| !start || at_start)
            .map(|(name, text, back, _)| (name.to_string(), text.to_string(), *back))
            .collect();
        for (name, text) in [
            ("Tomorrow", journal(today.map(|d| d.add_days(1)))),
            ("Yesterday", journal(today.map(|d| d.add_days(-1)))),
            ("Today", journal(today)),
            ("Current time", time),
        ] {
            if let Some(t) = text {
                all.push((name.to_string(), t, 0));
            }
        }
        // The code block, its fences on lines of their own.
        all.push((
            "Code block".into(),
            format!("```\n{indent}```"),
            4 + indent.len(),
        ));
        let lower = typed.trim().to_lowercase();
        let mut found: Vec<(u8, usize, (String, String, usize))> = all
            .into_iter()
            .enumerate()
            .filter_map(|(i, c)| Some((matches(&c.0, &lower)?, i, c)))
            .collect();
        found.sort_by_key(|f| (f.0, f.1));
        let start = cursor - typed.len() - 1;
        Some(
            found
                .into_iter()
                .map(|(_, _, (name, text, back))| Suggestion {
                    cursor: (back > 0).then(|| text.len() - back),
                    detail: text.replace('\n', " "),
                    label: name,
                    insert: text,
                    start,
                    what: What::Snippet,
                })
                .collect(),
        )
    }

    /// The graph's properties, counted again when its index changed.
    fn property_stats(&self, index: &Index) -> Rc<Properties> {
        let root = &index.graph.root;
        if let Some((v, p)) = self.properties.borrow().get(root)
            && *v == index.version()
        {
            return p.clone();
        }
        let p = Rc::new(Properties::of(index));
        self.properties
            .borrow_mut()
            .insert(root.clone(), (index.version(), p.clone()));
        p
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

/// In a vault, after `[[note#` the note's headings, after `[[note#^` its
/// blocks with an id, written in place of what follows `#`.
fn anchors(
    index: &Index,
    rel: &str,
    note: &str,
    part: &str,
    cursor: usize,
    closed: bool,
) -> Vec<Suggestion> {
    // `[[#Heading]]` is a heading of the note itself.
    let key = if note.is_empty() {
        index.page_of(rel).map(|p| p.key.clone())
    } else {
        Some(index.resolve(note, rel))
    };
    let Some(file) = key
        .as_deref()
        .and_then(|k| index.page(k))
        .and_then(|p| p.path.as_deref())
        .and_then(|p| index.file(p))
    else {
        return Vec::new();
    };
    let close = if closed { "" } else { "]]" };
    let start = cursor - part.len();
    if let Some(typed) = part.strip_prefix('^') {
        let lower = typed.to_lowercase();
        return file
            .scanned
            .blocks
            .iter()
            .filter_map(|b| {
                let id = b.id.as_deref()?;
                let first = b.text.lines().next().unwrap_or("").trim();
                (id.to_lowercase().contains(&lower) || first.to_lowercase().contains(&lower)).then(
                    || Suggestion {
                        label: first.chars().take(80).collect(),
                        insert: format!("^{id}{close}"),
                        start,
                        cursor: None,
                        what: What::Link,
                        detail: format!("^{id}"),
                    },
                )
            })
            .take(MOST)
            .collect();
    }
    let lower = part.to_lowercase();
    file.scanned
        .headings
        .iter()
        .filter(|h| matches(&h.text, &lower).is_some())
        .take(MOST)
        .map(|h| Suggestion {
            label: h.text.clone(),
            insert: format!("{}{close}", h.text),
            start,
            cursor: None,
            what: What::Link,
            detail: "#".repeat(h.level as usize),
        })
        .collect()
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
