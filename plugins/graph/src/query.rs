//! Logseq's simple queries (`{{query …}}`, GR9a), over the index: what a
//! query names read from its text, and the blocks or pages that match.
//!
//! The clauses: `[[page]]`, `#tag` (blocks referring to the page, or
//! under a block that does), `"text"` and `(full-text-search "text")`,
//! `(and …)`, `(or …)`, `(not …)`, `(task TODO DOING)` (`todo` too),
//! `(priority A B)`, `(between -7d today)` (a journal's day, a
//! `SCHEDULED:` or a `DEADLINE:`), `(property key value)`,
//! `(page-property key value)`, `(page-tags tag …)`, `(page name)`,
//! `(namespace name)`, `(sort-by key asc|desc)` and `(sample n)`. A query
//! of page clauses only finds pages. Advanced queries (Datalog) are not
//! run.

use std::collections::BTreeSet;

use crate::date::{self, Date};
use crate::index::Index;
use crate::names;
use crate::scan;

/// A clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// All of them.
    And(Vec<Expr>),
    /// Any of them.
    Or(Vec<Expr>),
    /// Not it.
    Not(Box<Expr>),
    /// Blocks referring to the page (or under one that does).
    Ref(String),
    /// Blocks whose text holds this, ignoring case.
    Text(String),
    /// Blocks with one of these task keywords.
    Task(Vec<String>),
    /// Blocks with one of these priorities.
    Priority(Vec<char>),
    /// Blocks of a day between these two, both included.
    Between(String, String),
    /// Blocks with this property (and value).
    Property(String, Option<String>),
    /// Pages with this property (and value).
    PageProperty(String, Option<String>),
    /// Pages with one of these tags.
    PageTags(Vec<String>),
    /// Blocks of this page.
    Page(String),
    /// Pages in this namespace.
    Namespace(String),
    /// A clause Kalem does not read, by its name: matches nothing.
    Unknown(String),
}

/// A query read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// What it asks.
    pub expr: Expr,
    /// `(sort-by key desc)`: the key and whether descending.
    pub sort: Option<(String, bool)>,
    /// `(sample n)`.
    pub sample: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Ref(String),
    Str(String),
    Word(String),
}

fn tokens(s: &str) -> Result<Vec<Token>, String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' | b',' => i += 1,
            b'(' => {
                out.push(Token::Open);
                i += 1;
            }
            b')' => {
                out.push(Token::Close);
                i += 1;
            }
            b'"' => {
                let end = s[i + 1..].find('"').ok_or("a string not closed")? + i + 1;
                out.push(Token::Str(s[i + 1..end].to_string()));
                i = end + 1;
            }
            b'[' if s[i..].starts_with("[[") => {
                let end = close_brackets(s, i).ok_or("a [[ not closed")?;
                out.push(Token::Ref(s[i + 2..end - 2].to_string()));
                i = end;
            }
            b'#' if s[i + 1..].starts_with("[[") => {
                let end = close_brackets(s, i + 1).ok_or("a [[ not closed")?;
                out.push(Token::Ref(s[i + 3..end - 2].to_string()));
                i = end;
            }
            b'#' => {
                let n = s[i + 1..]
                    .find(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
                    .map_or(s.len(), |p| i + 1 + p);
                out.push(Token::Ref(s[i + 1..n].to_string()));
                i = n;
            }
            _ => {
                let n = s[i..]
                    .find(|c: char| {
                        c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == '"'
                    })
                    .map_or(s.len(), |p| i + p);
                out.push(Token::Word(s[i..n].trim_start_matches(':').to_string()));
                i = n.max(i + 1);
            }
        }
    }
    Ok(out)
}

/// The end of the `[[…]]` opening at `at`, nested ones counted.
fn close_brackets(s: &str, at: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0;
    let mut i = at;
    while i + 1 < b.len() {
        if &b[i..i + 2] == b"[[" {
            depth += 1;
            i += 2;
        } else if &b[i..i + 2] == b"]]" {
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

struct Parser {
    t: Vec<Token>,
    at: usize,
    sort: Option<(String, bool)>,
    sample: Option<usize>,
}

impl Parser {
    /// One clause; `None` for `sort-by` and `sample`, which go to the
    /// query.
    fn expr(&mut self) -> Result<Option<Expr>, String> {
        let Some(t) = self.t.get(self.at).cloned() else {
            return Err("a clause missing".into());
        };
        self.at += 1;
        match t {
            Token::Ref(r) => Ok(Some(Expr::Ref(r))),
            Token::Str(s) => Ok(Some(Expr::Text(s))),
            Token::Word(w) => Ok(Some(Expr::Text(w))),
            Token::Close => Err("a ) too many".into()),
            Token::Open => {
                let Some(Token::Word(head)) = self.t.get(self.at).cloned() else {
                    return Err("a clause without a name".into());
                };
                self.at += 1;
                let head = head.to_lowercase();
                // The arguments, as raw tokens for the simple clauses.
                let mut args: Vec<Token> = Vec::new();
                let mut subs: Vec<Expr> = Vec::new();
                let nested = matches!(head.as_str(), "and" | "or" | "not");
                loop {
                    match self.t.get(self.at).cloned() {
                        None => return Err(format!("({head} …) not closed")),
                        Some(Token::Close) => {
                            self.at += 1;
                            break;
                        }
                        Some(_) if nested => {
                            if let Some(e) = self.expr()? {
                                subs.push(e);
                            }
                        }
                        Some(Token::Open) => {
                            // A clause inside a simple one: skipped whole.
                            let mut depth = 0;
                            while let Some(t) = self.t.get(self.at) {
                                self.at += 1;
                                match t {
                                    Token::Open => depth += 1,
                                    Token::Close => {
                                        depth -= 1;
                                        if depth == 0 {
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(t) => {
                            args.push(t);
                            self.at += 1;
                        }
                    }
                }
                let text = |t: &Token| match t {
                    Token::Ref(s) | Token::Str(s) | Token::Word(s) => s.clone(),
                    _ => String::new(),
                };
                let words: Vec<String> = args.iter().map(text).collect();
                Ok(Some(match head.as_str() {
                    "and" => Expr::And(subs),
                    "or" => Expr::Or(subs),
                    "not" => Expr::Not(Box::new(match subs.len() {
                        1 => subs.remove(0),
                        _ => Expr::Or(subs),
                    })),
                    "task" | "todo" => Expr::Task(words.iter().map(|w| w.to_uppercase()).collect()),
                    "priority" => Expr::Priority(
                        words
                            .iter()
                            .filter_map(|w| w.trim_start_matches(['[', '#']).chars().next())
                            .map(|c| c.to_ascii_uppercase())
                            .collect(),
                    ),
                    "between" if words.len() >= 2 => {
                        Expr::Between(words[0].clone(), words[1].clone())
                    }
                    "between" if words.len() == 1 => {
                        Expr::Between(words[0].clone(), "today".into())
                    }
                    "property" => Expr::Property(
                        words.first().map(|k| k.to_lowercase()).unwrap_or_default(),
                        words.get(1).cloned(),
                    ),
                    "page-property" => Expr::PageProperty(
                        words.first().map(|k| k.to_lowercase()).unwrap_or_default(),
                        words.get(1).cloned(),
                    ),
                    "page-tags" => Expr::PageTags(words),
                    "page" => Expr::Page(words.first().cloned().unwrap_or_default()),
                    "namespace" => Expr::Namespace(words.first().cloned().unwrap_or_default()),
                    "full-text-search" => Expr::Text(words.join(" ")),
                    "sort-by" => {
                        let key = words.first().map(|k| k.to_lowercase()).unwrap_or_default();
                        let desc = words.get(1).is_some_and(|d| d.eq_ignore_ascii_case("desc"));
                        self.sort = Some((key, desc));
                        return Ok(None);
                    }
                    "sample" => {
                        self.sample = words.first().and_then(|n| n.parse().ok());
                        return Ok(None);
                    }
                    other => Expr::Unknown(other.to_string()),
                }))
            }
        }
    }
}

/// The query of `{{query …}}`'s inside.
pub fn parse(s: &str) -> Result<Query, String> {
    let mut p = Parser {
        t: tokens(s)?,
        at: 0,
        sort: None,
        sample: None,
    };
    let mut all = Vec::new();
    while p.at < p.t.len() {
        if let Some(e) = p.expr()? {
            all.push(e);
        }
    }
    let expr = match all.len() {
        0 => return Err("an empty query".into()),
        1 => all.remove(0),
        _ => Expr::And(all),
    };
    Ok(Query {
        expr,
        sort: p.sort,
        sample: p.sample,
    })
}

/// The `{{query …}}` macros of a line: their bytes and their insides.
pub fn macros(line: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = line[from..].find("{{query") {
        let s = from + p;
        let inner_start = s + "{{query".len();
        // The `}}` that closes it, past brackets and strings inside.
        let b = line.as_bytes();
        let mut i = inner_start;
        let mut depth = 0i32;
        let mut quoted = false;
        let mut end = None;
        while i < b.len() {
            match b[i] {
                b'"' => quoted = !quoted,
                b'[' if !quoted => depth += 1,
                b']' if !quoted => depth -= 1,
                b'}' if !quoted && depth <= 0 && line[i..].starts_with("}}") => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        let Some(e) = end else { break };
        out.push((s, e + 2, line[inner_start..e].trim()));
        from = e + 2;
    }
    out
}

/// What a query found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// A block: its file and index.
    Block {
        /// The file, relative.
        rel: String,
        /// The block.
        block: usize,
    },
    /// A page, by key.
    Page(String),
}

/// A query's answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    /// What matched, in order.
    pub hits: Vec<Hit>,
    /// It found pages, not blocks.
    pub pages: bool,
    /// Why some clause matched nothing.
    pub notes: Vec<String>,
}

/// Whether `e` asks of pages only.
fn pages_only(e: &Expr) -> bool {
    match e {
        Expr::And(v) | Expr::Or(v) => !v.is_empty() && v.iter().all(pages_only),
        Expr::Not(x) => pages_only(x),
        Expr::PageProperty(..) | Expr::PageTags(_) | Expr::Namespace(_) => true,
        _ => false,
    }
}

/// A day a query names: `today`, `yesterday`, `tomorrow`, `-7d`, `+2w`,
/// `-1m`, `-1y`, `[[Oct 3rd, 2026]]`, `2026-10-03`, `20261003`.
fn day(s: &str, index: &Index, today: Option<Date>) -> Option<Date> {
    let s = s.trim();
    if let Some(d) = Date::parse_iso(s).or_else(|| index.graph.journal_title.parse(s)) {
        return Some(d);
    }
    if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
        return Date::new(
            s[..4].parse().ok()?,
            s[4..6].parse().ok()?,
            s[6..].parse().ok()?,
        );
    }
    let today = today?;
    let lower = s.to_lowercase();
    if let Some(d) = relative(&lower, today) {
        return Some(d);
    }
    date::parse_input(&lower, today)
}

/// `-7d`, `+2w`, `1m`, `-1y` from `today`.
fn relative(s: &str, today: Date) -> Option<Date> {
    let (sign, rest) = match s.as_bytes().first()? {
        b'-' => (-1i64, &s[1..]),
        b'+' => (1, &s[1..]),
        _ => (1, s),
    };
    let unit = rest.chars().last()?;
    let n: i64 = rest[..rest.len() - unit.len_utf8()].parse().ok()?;
    let days = match unit {
        'd' => n,
        'w' => n * 7,
        'm' => n * 30,
        'y' => n * 365,
        _ => return None,
    };
    Some(today.add_days(sign * days))
}

/// A property's value as compared: without `[[`, `]]`, `#` and quotes,
/// in lower case.
fn bare(v: &str) -> String {
    v.trim()
        .trim_matches('"')
        .trim_start_matches('#')
        .trim_start_matches("[[")
        .trim_end_matches("]]")
        .trim()
        .to_lowercase()
}

fn value_matches(have: &str, want: &Option<String>) -> bool {
    match want {
        None => true,
        Some(w) => {
            let w = bare(w);
            bare(have) == w || scan::split_values(have).iter().any(|v| bare(v) == w)
        }
    }
}

/// What the evaluation knows of one block.
struct Ctx<'a> {
    index: &'a Index,
    rel: &'a str,
    block: Option<&'a scan::Block>,
    page: &'a str,
    refs: &'a BTreeSet<String>,
    today: Option<Date>,
}

fn eval(e: &Expr, c: &Ctx<'_>, notes: &mut BTreeSet<String>) -> bool {
    let page = c.index.page(c.page);
    match e {
        Expr::And(v) => v.iter().all(|x| eval(x, c, notes)),
        Expr::Or(v) => v.iter().any(|x| eval(x, c, notes)),
        Expr::Not(x) => !eval(x, c, notes),
        Expr::Ref(name) => c.refs.contains(&c.index.resolve(name, c.rel)),
        Expr::Text(t) => c
            .block
            .is_some_and(|b| b.text.to_lowercase().contains(&t.to_lowercase())),
        Expr::Task(list) => c
            .block
            .and_then(|b| b.marker.as_deref())
            .is_some_and(|m| list.iter().any(|x| x == m)),
        Expr::Priority(list) => c
            .block
            .and_then(|b| b.priority)
            .is_some_and(|p| list.contains(&p)),
        Expr::Between(a, z) => {
            let (Some(a), Some(z)) = (day(a, c.index, c.today), day(z, c.index, c.today)) else {
                notes.insert("(between …) needs today's date, which a journal command asks".into());
                return false;
            };
            let (a, z) = if a <= z { (a, z) } else { (z, a) };
            let within = |d: Option<Date>| d.is_some_and(|d| a <= d && d <= z);
            within(page.and_then(|p| p.journal))
                || c.block
                    .is_some_and(|b| within(b.scheduled) || within(b.deadline))
        }
        Expr::Property(k, v) => c.block.is_some_and(|b| {
            b.props
                .iter()
                .any(|(pk, pv)| pk == k && value_matches(pv, v))
        }),
        Expr::PageProperty(k, v) => page.is_some_and(|p| {
            p.props
                .iter()
                .any(|(pk, pv)| pk == k && value_matches(pv, v))
        }),
        Expr::PageTags(tags) => page.is_some_and(|p| {
            p.props
                .iter()
                .filter(|(k, _)| k == "tags")
                .flat_map(|(_, v)| scan::split_values(v))
                .any(|t| tags.iter().any(|w| bare(w) == bare(&t)))
        }),
        Expr::Page(name) => c.page == c.index.resolve(name, c.rel),
        Expr::Namespace(ns) => page.is_some_and(|p| {
            let ns = names::key(ns.trim_start_matches("[[").trim_end_matches("]]"));
            p.key.starts_with(&format!("{ns}/"))
        }),
        Expr::Unknown(name) => {
            notes.insert(format!("({name} …) is not read by Kalem"));
            false
        }
    }
}

/// Whether `e` names a page a block must refer to.
fn needs_refs(e: &Expr) -> bool {
    match e {
        Expr::And(v) | Expr::Or(v) => v.iter().any(needs_refs),
        Expr::Not(x) => needs_refs(x),
        Expr::Ref(_) => true,
        _ => false,
    }
}

/// The pages a block's references reach, as Logseq's path references
/// count them: its page, its own references, and those of the blocks it
/// is under.
fn path_refs(index: &Index, rel: &str) -> Vec<BTreeSet<String>> {
    let Some(d) = index.file(rel) else {
        return Vec::new();
    };
    let blocks = &d.scanned.blocks;
    let mut own: Vec<BTreeSet<String>> = vec![BTreeSet::new(); blocks.len()];
    for r in &d.scanned.refs {
        if r.kind.to_block() {
            continue;
        }
        if let Some(b) = r.block
            && b < own.len()
        {
            own[b].insert(index.resolve(&r.target, rel));
        }
    }
    let mut out: Vec<BTreeSet<String>> = Vec::with_capacity(blocks.len());
    for (i, b) in blocks.iter().enumerate() {
        let mut set = std::mem::take(&mut own[i]);
        match b.parent {
            Some(p) if p < out.len() => set.extend(out[p].iter().cloned()),
            _ => {
                set.insert(d.key.clone());
            }
        }
        out.push(set);
    }
    out
}

/// The query run over the graph; `today` for relative days.
pub fn run(q: &Query, index: &Index, today: Option<Date>) -> Answer {
    let mut notes = BTreeSet::new();
    let pages = pages_only(&q.expr);
    let mut hits: Vec<Hit> = Vec::new();
    let empty = BTreeSet::new();
    if pages {
        for p in index.pages().filter(|p| p.path.is_some()) {
            let c = Ctx {
                index,
                rel: p.path.as_deref().unwrap_or(""),
                block: None,
                page: &p.key,
                refs: &empty,
                today,
            };
            if eval(&q.expr, &c, &mut notes) {
                hits.push(Hit::Page(p.key.clone()));
            }
        }
    } else {
        let refs_needed = needs_refs(&q.expr);
        for (rel, d) in index.files() {
            let refs = if refs_needed {
                path_refs(index, rel)
            } else {
                Vec::new()
            };
            for (i, b) in d.scanned.blocks.iter().enumerate() {
                // A query block's own text never answers it.
                if b.text.contains("{{query") || b.props.iter().any(|(k, _)| k == "template") {
                    continue;
                }
                let c = Ctx {
                    index,
                    rel,
                    block: Some(b),
                    page: &d.key,
                    refs: refs.get(i).unwrap_or(&empty),
                    today,
                };
                if eval(&q.expr, &c, &mut notes) {
                    hits.push(Hit::Block {
                        rel: rel.clone(),
                        block: i,
                    });
                }
            }
        }
    }
    // Journals newest first, then pages by title, a page's blocks in
    // order; a sort asked for goes over it.
    let order = |h: &Hit| {
        let (page, line) = match h {
            Hit::Page(k) => (index.page(k), 0),
            Hit::Block { rel, block } => (
                index.page_of(rel),
                index
                    .file(rel)
                    .and_then(|d| d.scanned.blocks.get(*block))
                    .map_or(0, |b| b.line),
            ),
        };
        let journal = page.and_then(|p| p.journal);
        (
            journal.is_none(),
            std::cmp::Reverse(journal),
            page.map_or(String::new(), |p| p.title.to_lowercase()),
            line,
        )
    };
    hits.sort_by_cached_key(order);
    if let Some((key, desc)) = &q.sort {
        let sort_key = |h: &Hit| -> (Option<Date>, String) {
            let (page, block) = match h {
                Hit::Page(k) => (index.page(k), None),
                Hit::Block { rel, block } => (
                    index.page_of(rel),
                    index.file(rel).and_then(|d| d.scanned.blocks.get(*block)),
                ),
            };
            match key.as_str() {
                "page" => (None, page.map_or(String::new(), |p| p.title.to_lowercase())),
                "created-at" | "updated-at" | "journal-day" => {
                    (page.and_then(|p| p.journal), String::new())
                }
                "priority" => (
                    None,
                    block
                        .and_then(|b| b.priority)
                        .map_or("Z".into(), |c| c.to_string()),
                ),
                other => (
                    None,
                    block
                        .and_then(|b| b.props.iter().find(|(k, _)| k == other))
                        .or_else(|| page.and_then(|p| p.props.iter().find(|(k, _)| k == other)))
                        .map_or(String::new(), |(_, v)| bare(v)),
                ),
            }
        };
        hits.sort_by_cached_key(sort_key);
        if *desc {
            hits.reverse();
        }
    }
    if let Some(n) = q.sample {
        hits.truncate(n);
    }
    Answer {
        hits,
        pages,
        notes: notes.into_iter().collect(),
    }
}

/// How a query's answer is shown, from the properties of the block that
/// holds it: a list, or a table (`query-table:: true`) of the columns
/// `query-properties::` names (`[:block :page]`), sorted by the column
/// `query-sort-by::` names, descending when `query-sort-desc:: true`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shape {
    /// The table's columns, when it is a table; empty for the default ones.
    pub table: Option<Vec<String>>,
    /// The column sorted by, and whether descending.
    pub sort: Option<(String, bool)>,
}

impl Shape {
    /// From the properties of the block holding the query.
    pub fn of(props: &[(String, String)]) -> Shape {
        let get = |k: &str| props.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.trim());
        let table = get("query-table").is_some_and(|v| v == "true").then(|| {
            get("query-properties")
                .map(|v| {
                    v.trim_start_matches('[')
                        .trim_end_matches(']')
                        .split([' ', ','])
                        .map(|c| c.trim().trim_start_matches(':').to_lowercase())
                        .filter(|c| !c.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        });
        let sort = get("query-sort-by").map(|k| {
            (
                k.trim_start_matches(':').to_lowercase(),
                get("query-sort-desc").is_some_and(|d| d == "true"),
            )
        });
        Shape { table, sort }
    }

    /// As part of a document's key.
    pub fn encode(&self) -> String {
        let mut s = String::new();
        if let Some(t) = &self.table {
            s.push_str("table=");
            s.push_str(&t.join(","));
        }
        if let Some((k, desc)) = &self.sort {
            s.push_str(if *desc { ";sort=-" } else { ";sort=" });
            s.push_str(k);
        }
        s
    }

    /// Read back from [`Shape::encode`]'s text.
    pub fn decode(s: &str) -> Shape {
        let mut shape = Shape::default();
        for part in s.split(';') {
            if let Some(t) = part.strip_prefix("table=") {
                shape.table = Some(
                    t.split(',')
                        .filter(|c| !c.is_empty())
                        .map(str::to_string)
                        .collect(),
                );
            } else if let Some(k) = part.strip_prefix("sort=") {
                shape.sort = Some(match k.strip_prefix('-') {
                    Some(k) => (k.to_string(), true),
                    None => (k.to_string(), false),
                });
            }
        }
        shape
    }
}

/// The first line of a hit: a block's keyword and text, a page's title.
pub fn hit_text(index: &Index, hit: &Hit) -> String {
    match hit {
        Hit::Page(k) => index.page(k).map_or(k.clone(), |p| p.title.clone()),
        Hit::Block { rel, block } => {
            let Some(b) = index.file(rel).and_then(|d| d.scanned.blocks.get(*block)) else {
                return String::new();
            };
            let mut t = String::new();
            if let Some(m) = &b.marker {
                t.push_str(m);
                t.push(' ');
            }
            if let Some(p) = b.priority {
                t.push_str(&format!("[#{p}] "));
            }
            t.push_str(&block_refs_shown(
                index,
                b.text.lines().next().unwrap_or("").trim(),
            ));
            t
        }
    }
}

/// `((uuid))` in `text` as the text of the block it names, as Logseq
/// shows it.
fn block_refs_shown(index: &Index, text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find("((") {
        let after = &rest[p + 2..];
        let id = after.find("))").map(|e| &after[..e]);
        match id
            .filter(|id| scan::is_uuid(id))
            .and_then(|id| index.block(id).map(|(_, b)| (id, b)))
        {
            Some((id, b)) => {
                out.push_str(&rest[..p]);
                out.push_str(b.text.lines().next().unwrap_or("").trim());
                rest = &after[id.len() + 2..];
            }
            None => {
                out.push_str(&rest[..p + 2]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `text` cut to `n` characters.
fn cut(text: &str, n: usize) -> String {
    if text.chars().count() <= n {
        return text.to_string();
    }
    let mut s: String = text.chars().take(n.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// What `{{query …}}` shows in a note away from the cursor: how many
/// blocks or pages it finds and the first of them; `mark` before it.
pub fn summary(index: &Index, inner: &str, today: Option<Date>, mark: &str) -> String {
    let q = match parse(inner) {
        Ok(q) => q,
        Err(e) => return format!("{mark} query not read: {e}"),
    };
    let a = run(&q, index, today);
    let n = a.hits.len();
    let what = match (a.pages, n) {
        (true, 1) => "1 page".to_string(),
        (true, _) => format!("{n} pages"),
        (false, 1) => "1 block".to_string(),
        (false, _) => format!("{n} blocks"),
    };
    if n == 0 {
        return match a.notes.first() {
            Some(note) => format!("{mark} no result: {note}"),
            None => format!("{mark} no result"),
        };
    }
    let first: Vec<String> = a
        .hits
        .iter()
        .take(2)
        .map(|h| cut(&hit_text(index, h), 36))
        .collect();
    let more = if n > 2 { " · …" } else { "" };
    format!("{mark} {what}: {}{more}", first.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Fallbacks, Graph, Kind};
    use crate::files::Memory;

    fn index() -> Index {
        let m = Memory::new(&[
            ("/g/logseq/config.edn", "{:file/name-format :triple-lowbar}"),
            (
                "/g/pages/Project.md",
                "tags:: work, kalem\ntype:: plan\n\n- TODO [#A] write [[Kalem]] docs\n- DOING [#B] review\n  owner:: [[Ada]]\n- a note about kalem\n",
            ),
            (
                "/g/pages/Kalem.md",
                "- An editor\n\t- child of the editor\n- NOW ship #release\n",
            ),
            (
                "/g/pages/Kalem___Plugins.md",
                "tags:: kalem\n\n- LATER graph plugin\n  SCHEDULED: <2026-10-12 Mon>\n",
            ),
            (
                "/g/journals/2026_10_05.md",
                "- met about [[Kalem]]\n\t- decided things\n- TODO call\n",
            ),
            ("/g/journals/2026_10_09.md", "- DONE shipped [[Kalem]]\n"),
        ]);
        Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        )
    }

    fn texts(i: &Index, a: &Answer) -> Vec<String> {
        a.hits
            .iter()
            .map(|h| match h {
                Hit::Block { rel, block } => {
                    i.file(rel).unwrap().scanned.blocks[*block].text.clone()
                }
                Hit::Page(k) => i.page(k).unwrap().title.clone(),
            })
            .collect()
    }

    fn q(i: &Index, s: &str) -> Vec<String> {
        let mut t = texts(i, &run(&parse(s).unwrap(), i, Date::new(2026, 10, 10)));
        t.sort();
        t
    }

    #[test]
    fn clauses() {
        let i = index();
        // A page: its blocks, the blocks referring to it and those under
        // them.
        assert_eq!(
            q(&i, "[[Kalem]]"),
            [
                "An editor",
                "child of the editor",
                "decided things",
                "met about [[Kalem]]",
                "ship #release",
                "shipped [[Kalem]]",
                "write [[Kalem]] docs"
            ]
        );
        assert_eq!(q(&i, "#release"), ["ship #release"]);
        assert_eq!(
            q(&i, "(task TODO DOING)"),
            ["call", "review", "write [[Kalem]] docs"]
        );
        assert_eq!(
            q(&i, "(and [[Kalem]] (task TODO))"),
            ["write [[Kalem]] docs"]
        );
        assert_eq!(
            q(&i, "(and (task todo doing) (not [[Kalem]]))"),
            ["call", "review"]
        );
        assert_eq!(
            q(&i, "(and [[Project]] (task TODO DOING))"),
            ["review", "write [[Kalem]] docs"]
        );
        assert_eq!(
            q(&i, "(or #release (priority a))"),
            ["ship #release", "write [[Kalem]] docs"]
        );
        assert_eq!(q(&i, "(property owner [[Ada]])"), ["review"]);
        assert_eq!(q(&i, "\"note about\""), ["a note about kalem"]);
        assert_eq!(
            q(&i, "(full-text-search \"EDITOR\")"),
            ["An editor", "child of the editor"]
        );
        assert_eq!(
            q(&i, "(page [[Kalem]])"),
            ["An editor", "child of the editor", "ship #release"]
        );
        // Days: journals and dates within.
        assert_eq!(
            q(&i, "(between -7d today)"),
            [
                "call",
                "decided things",
                "met about [[Kalem]]",
                "shipped [[Kalem]]"
            ]
        );
        assert_eq!(q(&i, "(between today +7d)"), ["graph plugin"]);
        assert_eq!(
            q(&i, "(between [[Oct 9th, 2026]] [[Oct 9th, 2026]])"),
            ["shipped [[Kalem]]"]
        );
        // Pages.
        assert_eq!(q(&i, "(page-tags kalem)"), ["Kalem/Plugins", "Project"]);
        assert_eq!(q(&i, "(page-property type plan)"), ["Project"]);
        assert_eq!(q(&i, "(namespace kalem)"), ["Kalem/Plugins"]);
        // A page clause with a block clause: blocks of those pages.
        assert_eq!(q(&i, "(and (page-tags work) (task DOING))"), ["review"]);
    }

    #[test]
    fn sorting_sampling_and_notes() {
        let i = index();
        let a = run(
            &parse("(and (task TODO DOING NOW) (sort-by priority))").unwrap(),
            &i,
            None,
        );
        assert_eq!(
            texts(&i, &a)[..2],
            ["write [[Kalem]] docs".to_string(), "review".to_string()]
        );
        let a = run(
            &parse("(task TODO DOING NOW) (sample 2)").unwrap(),
            &i,
            None,
        );
        assert_eq!(a.hits.len(), 2);
        let a = run(&parse("(between -7d today)").unwrap(), &i, None);
        assert!(a.hits.is_empty() && a.notes[0].contains("today"));
        let a = run(&parse("(path-refs [[x]])").unwrap(), &i, None);
        assert!(a.notes[0].contains("path-refs"));
        assert!(parse("(and [[x]]").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn shapes_and_summaries() {
        let props = |v: &[(&str, &str)]| -> Vec<(String, String)> {
            v.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let s = Shape::of(&props(&[
            ("query-table", "true"),
            ("query-properties", "[:block :page :type]"),
            ("query-sort-by", "page"),
            ("query-sort-desc", "true"),
        ]));
        assert_eq!(
            s.table.as_deref(),
            Some(&["block".to_string(), "page".into(), "type".into()][..])
        );
        assert_eq!(s.sort, Some(("page".into(), true)));
        assert_eq!(Shape::decode(&s.encode()), s);
        assert_eq!(
            Shape::of(&props(&[("query-table", "false")])),
            Shape::default()
        );
        assert_eq!(
            Shape::decode(&Shape::of(&props(&[("query-table", "true")])).encode()).table,
            Some(vec![])
        );
        let i = index();
        assert_eq!(
            summary(&i, "(task TODO DOING)", None, "⌕"),
            "⌕ 3 blocks: TODO call · TODO [#A] write [[Kalem]] docs · …"
        );
        assert_eq!(
            summary(&i, "(namespace kalem)", None, "⌕"),
            "⌕ 1 page: Kalem/Plugins"
        );
        assert_eq!(summary(&i, "(task WAITING)", None, "⌕"), "⌕ no result");
        assert!(
            summary(&i, "(between -1d today)", None, "⌕")
                .starts_with("⌕ no result: (between …) needs")
        );
        assert!(summary(&i, "(and", None, "⌕").starts_with("⌕ query not read"));
    }

    #[test]
    fn macros_in_a_line() {
        let l = "- see {{query (and [[a b]] \"x}}y\")}} and {{query #t}}";
        let m = macros(l);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].2, "(and [[a b]] \"x}}y\")");
        assert_eq!(&l[m[0].0..m[0].1], "{{query (and [[a b]] \"x}}y\")}}");
        assert_eq!(m[1].2, "#t");
    }
}
