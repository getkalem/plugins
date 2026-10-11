//! Logseq's advanced queries (`#+BEGIN_QUERY` … `#+END_QUERY`): a map of
//! EDN whose `:query` is a Datascript query, run as Logseq's file version
//! runs it, over the index rather than a database.
//!
//! The graph is seen as Logseq's schema has it: a page is an entity with
//! `:block/name`, `:block/original-name`, `:block/journal?`,
//! `:block/journal-day`, `:block/properties`, `:block/tags`,
//! `:block/alias` and `:block/namespace`; a block one with
//! `:block/content`, `:block/marker`, `:block/priority`, `:block/page`,
//! `:block/parent`, `:block/refs`, `:block/path-refs`,
//! `:block/properties`, `:block/scheduled`, `:block/deadline`,
//! `:block/uuid` and `:block/collapsed?`. A query's `:where` holds data
//! patterns, predicates (`=`, `not=`, `<`, `>`, `<=`, `>=`, `contains?`,
//! `clojure.string/includes?`, `starts-with?`, `ends-with?`,
//! `blank?`), functions binding a value (`get`, `get-else`, `str`,
//! `missing?`), `not`, `not-join`, `or`, `or-join`, `and`, and Logseq's
//! own rules (`page-ref`, `task`, `priority`, `property`,
//! `page-property`, `page-tags`, `all-page-tags`, `between`, `namespace`,
//! `page`, `block-content`); `:in` with `:inputs` takes `:today`,
//! `:yesterday`, `:tomorrow`, `:-7d`, `:+7d`, `:7d-before`, `:7d-after`,
//! `:current-page` and values. `:find` takes a variable, `(pull ?b [*])`
//! or `(count ?b)`. What a query asks beyond these is said, not guessed;
//! `:result-transform` and `:view`, Clojure functions, are not run.

use std::collections::{BTreeSet, HashMap};

use crate::date::Date;
use crate::edn::Value as Edn;
use crate::index::{Index, Page};
use crate::query::{Answer, Hit};
use crate::scan::Block;

/// A query as its map gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Advanced {
    /// `:title`, when it is a string.
    pub title: Option<String>,
    /// `:query`.
    query: Edn,
    /// `:inputs`.
    inputs: Vec<Edn>,
    /// What of the map is not run (`:result-transform`, `:view`).
    pub ignored: Vec<String>,
}

/// The text between `#+BEGIN_QUERY` and `#+END_QUERY`, read.
pub fn parse(text: &str) -> Result<Advanced, String> {
    let body = text
        .lines()
        .map(str::trim)
        .filter(|l| {
            let u = l.to_uppercase();
            !u.starts_with("#+BEGIN_QUERY") && !u.starts_with("#+END_QUERY")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let v = crate::edn::parse(&body)?;
    let Edn::Map(_) = v else {
        return Err("an advanced query is a map: {:query [...]}".into());
    };
    let query = v.get("query").cloned().ok_or("the map has no :query")?;
    let title = match v.get("title") {
        Some(Edn::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let inputs = match v.get("inputs") {
        Some(Edn::Vector(v) | Edn::List(v)) => v.clone(),
        _ => Vec::new(),
    };
    let ignored = ["result-transform", "view", "rules"]
        .iter()
        .filter(|k| v.get(k).is_some())
        .map(|k| format!(":{k}"))
        .collect();
    Ok(Advanced {
        title,
        query,
        inputs,
        ignored,
    })
}

/// A value of the query: an entity, or a datum.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum V {
    /// A page or a block, by its number in [`Db`].
    E(u32),
    Int(i64),
    Str(String),
    Bool(bool),
    Kw(String),
    Nil,
    Set(Vec<V>),
    Map(Vec<(String, V)>),
}

impl V {
    fn text(&self, db: &Db<'_>) -> String {
        match self {
            V::E(e) => db.name(*e),
            V::Int(i) => i.to_string(),
            V::Str(s) => s.clone(),
            V::Bool(b) => b.to_string(),
            V::Kw(k) => format!(":{k}"),
            V::Nil => "nil".into(),
            V::Set(v) => v.iter().map(|x| x.text(db)).collect::<Vec<_>>().join(", "),
            V::Map(m) => m
                .iter()
                .map(|(k, v)| format!(":{k} {}", v.text(db)))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// A value of EDN as one of the query's.
fn value(e: &Edn) -> V {
    match e {
        Edn::Nil => V::Nil,
        Edn::Bool(b) => V::Bool(*b),
        Edn::Int(i) => V::Int(*i),
        Edn::Float(f) => V::Int(*f as i64),
        Edn::Str(s) => V::Str(s.clone()),
        Edn::Char(c) => V::Str(c.to_string()),
        Edn::Keyword(k) => V::Kw(k.clone()),
        Edn::Symbol(s) => V::Str(s.clone()),
        Edn::Set(v) | Edn::Vector(v) | Edn::List(v) => V::Set(v.iter().map(value).collect()),
        Edn::Map(m) => V::Map(
            m.iter()
                .map(|(k, v)| (k.as_str().unwrap_or("").to_string(), value(v)))
                .collect(),
        ),
        Edn::Tagged(_, v) => value(v),
    }
}

/// A day as Logseq keeps a journal's: 20261011.
fn day_int(d: Date) -> i64 {
    i64::from(d.year) * 10_000 + i64::from(d.month) * 100 + i64::from(d.day)
}

/// The graph as entities: its pages, then its blocks.
struct Db<'a> {
    pages: Vec<&'a Page>,
    page_id: HashMap<&'a str, u32>,
    /// Each block: its file, its place there, the block and its page.
    blocks: Vec<(&'a str, usize, &'a Block, u32)>,
    /// A file's first block's number.
    first: HashMap<&'a str, u32>,
    /// Each block's references, by page number.
    refs: Vec<Vec<u32>>,
    /// The blocks referring to each page.
    referred: HashMap<u32, Vec<u32>>,
    /// Each page's blocks.
    of_page: HashMap<u32, Vec<u32>>,
}

impl<'a> Db<'a> {
    fn new(index: &'a Index) -> Db<'a> {
        let pages: Vec<&Page> = index.pages().collect();
        let page_id: HashMap<&str, u32> = pages
            .iter()
            .enumerate()
            .map(|(i, p)| (p.key.as_str(), i as u32))
            .collect();
        let mut blocks = Vec::new();
        let mut first = HashMap::new();
        let mut of_page: HashMap<u32, Vec<u32>> = HashMap::new();
        let base = pages.len() as u32;
        for (rel, d) in index.files() {
            let Some(&p) = page_id.get(d.key.as_str()) else {
                continue;
            };
            first.insert(rel.as_str(), base + blocks.len() as u32);
            for (i, b) in d.scanned.blocks.iter().enumerate() {
                of_page
                    .entry(p)
                    .or_default()
                    .push(base + blocks.len() as u32);
                blocks.push((rel.as_str(), i, b, p));
            }
        }
        let mut refs = vec![Vec::new(); blocks.len()];
        let mut referred: HashMap<u32, Vec<u32>> = HashMap::new();
        for (key, &p) in &page_id {
            for r in index.backlinks(key) {
                let (Some(i), Some(&f)) = (r.block, first.get(&*r.path)) else {
                    continue;
                };
                let b = f + i as u32;
                if let Some(v) = refs.get_mut((b - base) as usize)
                    && !v.contains(&p)
                {
                    v.push(p);
                    referred.entry(p).or_default().push(b);
                }
            }
        }
        Db {
            pages,
            page_id,
            blocks,
            first,
            refs,
            referred,
            of_page,
        }
    }

    fn page(&self, e: u32) -> Option<&'a Page> {
        self.pages.get(e as usize).copied()
    }

    fn block(&self, e: u32) -> Option<(&'a str, usize, &'a Block, u32)> {
        let i = e.checked_sub(self.pages.len() as u32)?;
        self.blocks.get(i as usize).copied()
    }

    fn page_named(&self, name: &str) -> Option<u32> {
        let key = crate::names::key(name.trim().trim_start_matches("[[").trim_end_matches("]]"));
        self.page_id.get(key.as_str()).copied()
    }

    fn name(&self, e: u32) -> String {
        match (self.page(e), self.block(e)) {
            (Some(p), _) => p.title.clone(),
            (_, Some((_, _, b, _))) => b.text.clone(),
            _ => String::new(),
        }
    }

    /// The block's parent's number.
    fn parent(&self, e: u32) -> Option<u32> {
        let (rel, _, b, _) = self.block(e)?;
        b.parent.map(|p| self.first[rel] + p as u32)
    }

    /// The pages a block and the blocks above it refer to, and its page.
    fn path_refs(&self, e: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut at = Some(e);
        while let Some(b) = at {
            if let Some(r) = self.refs.get((b - self.pages.len() as u32) as usize) {
                out.extend(r);
            }
            at = self.parent(b);
        }
        if let Some((_, _, _, p)) = self.block(e) {
            out.push(p);
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn props(&self, props: &[(String, String)]) -> V {
        V::Map(
            props
                .iter()
                .map(|(k, v)| (k.clone(), V::Str(v.trim().to_string())))
                .collect(),
        )
    }

    /// Entity `e`'s values of `attr` (`block/marker`).
    fn attr(&self, e: u32, attr: &str) -> Vec<V> {
        if let Some(p) = self.page(e) {
            let pages = |names: Vec<&str>| -> Vec<V> {
                names
                    .into_iter()
                    .filter_map(|n| self.page_named(n).map(V::E))
                    .collect()
            };
            let prop = |k: &str| {
                p.props
                    .iter()
                    .find(|(pk, _)| pk == k)
                    .map(|(_, v)| v.split(',').map(str::trim).collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            return match attr {
                "block/name" => vec![V::Str(p.key.clone())],
                "block/original-name" => vec![V::Str(p.title.clone())],
                "block/journal?" => vec![V::Bool(p.journal.is_some())],
                "block/journal-day" => p.journal.map(|d| V::Int(day_int(d))).into_iter().collect(),
                "block/properties" => vec![self.props(&p.props)],
                "block/tags" => pages(prop("tags")),
                "block/alias" => p
                    .aliases
                    .iter()
                    .filter_map(|a| self.page_named(a).map(V::E))
                    .collect(),
                "block/namespace" => p
                    .title
                    .rsplit_once('/')
                    .and_then(|(ns, _)| self.page_named(ns))
                    .map(V::E)
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            };
        }
        let Some((_, _, b, page)) = self.block(e) else {
            return Vec::new();
        };
        match attr {
            "block/content" => vec![V::Str(match b.marker {
                Some(m) => format!("{m} {}", b.text),
                None => b.text.clone(),
            })],
            "block/marker" => b.marker.map(|m| V::Str(m.into())).into_iter().collect(),
            "block/priority" => b
                .priority
                .map(|c| V::Str(c.to_string()))
                .into_iter()
                .collect(),
            "block/page" => vec![V::E(page)],
            "block/parent" => vec![V::E(self.parent(e).unwrap_or(page))],
            "block/refs" => self.refs[(e - self.pages.len() as u32) as usize]
                .iter()
                .map(|p| V::E(*p))
                .collect(),
            "block/path-refs" => self.path_refs(e).into_iter().map(V::E).collect(),
            "block/properties" => vec![self.props(&b.props)],
            "block/scheduled" => b
                .scheduled
                .map(|d| V::Int(day_int(d)))
                .into_iter()
                .collect(),
            "block/deadline" => b.deadline.map(|d| V::Int(day_int(d))).into_iter().collect(),
            "block/uuid" => b.id.clone().map(V::Str).into_iter().collect(),
            "block/collapsed?" => vec![V::Bool(b.collapsed)],
            _ => Vec::new(),
        }
    }

    /// The entities that may have `attr`, those with value `v` when it is
    /// given and an index finds them.
    fn having(&self, attr: &str, v: Option<&V>) -> Vec<u32> {
        let pages = || (0..self.pages.len() as u32).collect::<Vec<_>>();
        let base = self.pages.len() as u32;
        let blocks = || (base..base + self.blocks.len() as u32).collect::<Vec<_>>();
        match (attr, v) {
            ("block/refs", Some(V::E(p))) => self.referred.get(p).cloned().unwrap_or_default(),
            ("block/page", Some(V::E(p))) => self.of_page.get(p).cloned().unwrap_or_default(),
            ("block/name", Some(V::Str(n))) => self.page_named(n).into_iter().collect(),
            ("block/marker", _) => blocks()
                .into_iter()
                .filter(|e| self.block(*e).is_some_and(|b| b.2.marker.is_some()))
                .collect(),
            (
                "block/name"
                | "block/original-name"
                | "block/journal?"
                | "block/journal-day"
                | "block/tags"
                | "block/alias"
                | "block/namespace",
                _,
            ) => pages(),
            ("block/properties", _) => {
                let mut v = pages();
                v.extend(blocks());
                v
            }
            _ => blocks(),
        }
    }
}

/// What a query found: entities, or rows of values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Pages or blocks, as a simple query's.
    Hits(Answer),
    /// A table: its columns and rows.
    Table(Vec<String>, Vec<Vec<String>>),
}

type Row = HashMap<String, V>;

/// A term of a clause.
enum Term {
    Var(String),
    Any,
    Value(V),
}

fn term(e: &Edn) -> Term {
    match e {
        Edn::Symbol(s) if s == "_" => Term::Any,
        Edn::Symbol(s) if s.starts_with('?') => Term::Var(s.clone()),
        other => Term::Value(value(other)),
    }
}

/// `t`'s value in `row`, if bound.
fn bound(t: &Term, row: &Row) -> Option<V> {
    match t {
        Term::Var(v) => row.get(v).cloned(),
        Term::Value(v) => Some(v.clone()),
        Term::Any => None,
    }
}

struct Run<'a, 'b> {
    db: &'b Db<'a>,
}

impl Run<'_, '_> {
    fn solve(&self, clauses: &[Edn], rows: Vec<Row>) -> Result<Vec<Row>, String> {
        let mut rows = rows;
        for c in clauses {
            rows = self.clause(c, rows)?;
            if rows.is_empty() {
                break;
            }
        }
        Ok(rows)
    }

    fn clause(&self, c: &Edn, rows: Vec<Row>) -> Result<Vec<Row>, String> {
        match c {
            Edn::Vector(v) => {
                let v: Vec<&Edn> = v
                    .iter()
                    .skip_while(|e| matches!(e, Edn::Symbol(s) if s.starts_with('$')))
                    .collect();
                match v.first() {
                    Some(Edn::List(call)) => self.function(call, v.get(1).copied(), rows),
                    Some(_) => self.pattern(&v, rows),
                    None => Ok(rows),
                }
            }
            Edn::List(l) => {
                let head = l.first().and_then(Edn::as_str).unwrap_or("");
                let rest = &l[1..];
                match head {
                    "not" => self.not(rest, rows),
                    "not-join" => self.not(rest.get(1..).unwrap_or(&[]), rows),
                    "and" => self.solve(rest, rows),
                    "or" => self.or(rest, rows),
                    "or-join" => self.or(rest.get(1..).unwrap_or(&[]), rows),
                    _ => self.rule(head, rest, rows),
                }
            }
            other => Err(format!("a clause Kalem does not read: {}", show(other))),
        }
    }

    /// `[?e :attr ?v]`.
    fn pattern(&self, v: &[&Edn], rows: Vec<Row>) -> Result<Vec<Row>, String> {
        let e = term(v[0]);
        let Some(Edn::Keyword(attr)) = v.get(1) else {
            return Err(format!(
                "a pattern's attribute is a keyword: {}",
                v.iter().map(|e| show(e)).collect::<Vec<_>>().join(" ")
            ));
        };
        let val = v.get(2).map_or(Term::Any, |x| term(x));
        let mut out = Vec::new();
        for row in rows {
            let want = bound(&val, &row);
            let entities: Vec<u32> = match bound(&e, &row) {
                Some(V::E(x)) => vec![x],
                Some(_) => continue,
                None => self.db.having(attr, want.as_ref()),
            };
            for x in entities {
                for got in self.db.attr(x, attr) {
                    if want.as_ref().is_some_and(|w| *w != got) {
                        continue;
                    }
                    let mut r = row.clone();
                    if let Term::Var(n) = &e {
                        r.insert(n.clone(), V::E(x));
                    }
                    if let Term::Var(n) = &val {
                        r.insert(n.clone(), got);
                    }
                    out.push(r);
                }
            }
        }
        Ok(out)
    }

    /// `[(pred …)]` and `[(f …) ?out]`.
    fn function(
        &self,
        call: &[Edn],
        out: Option<&Edn>,
        rows: Vec<Row>,
    ) -> Result<Vec<Row>, String> {
        let name = call.first().and_then(Edn::as_str).unwrap_or("");
        let name = name.rsplit('/').next().unwrap_or(name);
        let args: Vec<Term> = call[1..].iter().map(term).collect();
        let mut kept = Vec::new();
        for row in rows {
            // `$` and the attribute of `get-else` and `missing?` stay as
            // written; the rest are values.
            let vals: Vec<Option<V>> = args.iter().map(|a| bound(a, &row)).collect();
            let need = |i: usize| -> Result<V, String> {
                vals.get(i)
                    .cloned()
                    .flatten()
                    .ok_or_else(|| format!("({name} …) uses a variable not bound before it"))
            };
            let result: V = match name {
                "=" | "not=" | "<" | ">" | "<=" | ">=" => {
                    let (a, b) = (need(0)?, need(1)?);
                    V::Bool(match name {
                        "=" => a == b,
                        "not=" => a != b,
                        "<" => a < b,
                        ">" => a > b,
                        "<=" => a <= b,
                        _ => a >= b,
                    })
                }
                "contains?" => {
                    let (set, x) = (need(0)?, need(1)?);
                    V::Bool(match set {
                        V::Set(v) => v.contains(&x),
                        V::Map(m) => {
                            matches!(&x, V::Kw(k) | V::Str(k) if m.iter().any(|(mk, _)| mk == k))
                        }
                        V::Str(s) => matches!(&x, V::Str(t) if s.contains(t.as_str())),
                        _ => false,
                    })
                }
                "includes?" | "starts-with?" | "ends-with?" => {
                    let (s, t) = (need(0)?.text(self.db), need(1)?.text(self.db));
                    let (s, t) = (s.to_lowercase(), t.to_lowercase());
                    V::Bool(match name {
                        "includes?" => s.contains(&t),
                        "starts-with?" => s.starts_with(&t),
                        _ => s.ends_with(&t),
                    })
                }
                "blank?" => V::Bool(need(0)?.text(self.db).trim().is_empty()),
                "get" => {
                    let (m, k) = (need(0)?, need(1)?);
                    let key = match &k {
                        V::Kw(k) | V::Str(k) => k.clone(),
                        _ => String::new(),
                    };
                    match m {
                        V::Map(m) => m.into_iter().find(|(mk, _)| *mk == key).map_or_else(
                            || vals.get(2).cloned().flatten().unwrap_or(V::Nil),
                            |(_, v)| v,
                        ),
                        _ => V::Nil,
                    }
                }
                "get-else" => {
                    // `(get-else $ ?e :attr default)`.
                    let e = need(1)?;
                    let Some(Edn::Keyword(attr)) = call.get(3) else {
                        return Err("(get-else $ ?e :attribute default)".into());
                    };
                    match e {
                        V::E(x) => self
                            .db
                            .attr(x, attr)
                            .into_iter()
                            .next()
                            .unwrap_or_else(|| vals.get(4).cloned().flatten().unwrap_or(V::Nil)),
                        _ => V::Nil,
                    }
                }
                "missing?" => {
                    let e = need(1)?;
                    let Some(Edn::Keyword(attr)) = call.get(3) else {
                        return Err("(missing? $ ?e :attribute)".into());
                    };
                    V::Bool(matches!(e, V::E(x) if self.db.attr(x, attr).is_empty()))
                }
                "str" => V::Str(
                    (0..vals.len())
                        .map(|i| need(i).map(|v| v.text(self.db)))
                        .collect::<Result<Vec<_>, _>>()?
                        .concat(),
                ),
                "lower-case" => V::Str(need(0)?.text(self.db).to_lowercase()),
                "upper-case" => V::Str(need(0)?.text(self.db).to_uppercase()),
                other => return Err(format!("({other} …) is a function Kalem does not run")),
            };
            match out.map(term) {
                // A predicate: kept when true.
                None => {
                    if result != V::Bool(false) && result != V::Nil {
                        kept.push(row);
                    }
                }
                Some(Term::Var(v)) => {
                    let mut r = row;
                    match r.get(&v) {
                        Some(have) if *have != result => continue,
                        _ => {
                            r.insert(v, result);
                        }
                    }
                    kept.push(r);
                }
                Some(Term::Any) => kept.push(row),
                Some(Term::Value(want)) => {
                    if want == result {
                        kept.push(row);
                    }
                }
            }
        }
        Ok(kept)
    }

    fn not(&self, clauses: &[Edn], rows: Vec<Row>) -> Result<Vec<Row>, String> {
        let mut out = Vec::new();
        for row in rows {
            if self.solve(clauses, vec![row.clone()])?.is_empty() {
                out.push(row);
            }
        }
        Ok(out)
    }

    fn or(&self, branches: &[Edn], rows: Vec<Row>) -> Result<Vec<Row>, String> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for row in rows {
            for b in branches {
                let found = match b {
                    Edn::List(l) if l.first().and_then(Edn::as_str) == Some("and") => {
                        self.solve(&l[1..], vec![row.clone()])?
                    }
                    other => self.clause(other, vec![row.clone()])?,
                };
                for r in found {
                    let mut key: Vec<(String, V)> = r.clone().into_iter().collect();
                    key.sort();
                    if seen.insert(key) {
                        out.push(r);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Logseq's rules, as its simple queries' (`frontend.db.rules`).
    fn rule(&self, name: &str, args: &[Edn], rows: Vec<Row>) -> Result<Vec<Row>, String> {
        let a: Vec<Term> = args.iter().map(term).collect();
        let var = |i: usize| match a.get(i) {
            Some(Term::Var(v)) => Ok(v.clone()),
            _ => Err(format!("({name} ?var …)")),
        };
        let lit = |i: usize, row: &Row| -> Option<V> { a.get(i).and_then(|t| bound(t, row)) };
        let strings = |v: Option<V>| -> Vec<String> {
            match v {
                Some(V::Set(v)) => v.iter().map(|x| x.text(self.db).to_lowercase()).collect(),
                Some(x) => vec![x.text(self.db).to_lowercase()],
                None => Vec::new(),
            }
        };
        let blocks_of = |row: &Row, v: &str| -> Vec<u32> {
            match row.get(v) {
                Some(V::E(x)) => vec![*x],
                Some(_) => Vec::new(),
                None => {
                    let base = self.db.pages.len() as u32;
                    (base..base + self.db.blocks.len() as u32).collect()
                }
            }
        };
        let pages_of = |row: &Row, v: &str| -> Vec<u32> {
            match row.get(v) {
                Some(V::E(x)) => vec![*x],
                Some(_) => Vec::new(),
                None => (0..self.db.pages.len() as u32).collect(),
            }
        };
        let prop_matches = |props: &[(String, String)], key: &str, want: &[String]| {
            props.iter().any(|(k, v)| {
                k == key
                    && (want.is_empty()
                        || v.split(',').any(|x| {
                            let x = x
                                .trim()
                                .trim_start_matches("[[")
                                .trim_end_matches("]]")
                                .to_lowercase();
                            want.contains(&x)
                        }))
            })
        };
        let key_of = |i: usize, row: &Row| -> String {
            match lit(i, row) {
                Some(V::Kw(k) | V::Str(k)) => k.to_lowercase(),
                _ => String::new(),
            }
        };
        let mut out = Vec::new();
        for row in rows {
            let v0 = var(0)?;
            let keep = |e: u32, out: &mut Vec<Row>| {
                let mut r = row.clone();
                r.insert(v0.clone(), V::E(e));
                out.push(r);
            };
            match name {
                "page-ref" => {
                    let Some(p) = lit(1, &row).and_then(|v| self.db.page_named(&v.text(self.db)))
                    else {
                        continue;
                    };
                    for b in blocks_of(&row, &v0) {
                        if self.db.path_refs(b).contains(&p) {
                            keep(b, &mut out);
                        }
                    }
                }
                "task" | "priority" => {
                    let want: Vec<String> = strings(lit(1, &row))
                        .into_iter()
                        .map(|s| s.to_uppercase())
                        .collect();
                    for b in blocks_of(&row, &v0) {
                        let Some((_, _, blk, _)) = self.db.block(b) else {
                            continue;
                        };
                        let have = if name == "task" {
                            blk.marker.map(str::to_string)
                        } else {
                            blk.priority.map(|c| c.to_string())
                        };
                        if have.is_some_and(|h| want.contains(&h)) {
                            keep(b, &mut out);
                        }
                    }
                }
                "property" => {
                    let key = key_of(1, &row);
                    let want = strings(lit(2, &row));
                    for b in blocks_of(&row, &v0) {
                        if self
                            .db
                            .block(b)
                            .is_some_and(|x| prop_matches(&x.2.props, &key, &want))
                        {
                            keep(b, &mut out);
                        }
                    }
                }
                "page-property" => {
                    let key = key_of(1, &row);
                    let want = strings(lit(2, &row));
                    for p in pages_of(&row, &v0) {
                        if self
                            .db
                            .page(p)
                            .is_some_and(|x| prop_matches(&x.props, &key, &want))
                        {
                            keep(p, &mut out);
                        }
                    }
                }
                "page-tags" | "all-page-tags" => {
                    let want = strings(lit(1, &row));
                    for p in pages_of(&row, &v0) {
                        let has = self.db.page(p).is_some_and(|x| {
                            prop_matches(&x.props, "tags", &want)
                                && (name == "page-tags" || !x.props.is_empty())
                        });
                        if has {
                            keep(p, &mut out);
                        }
                    }
                }
                "between" => {
                    let (Some(V::Int(s)), Some(V::Int(e))) = (lit(1, &row), lit(2, &row)) else {
                        return Err(
                            "(between ?b ?start ?end) with days bound (:inputs [:-7d :today])"
                                .into(),
                        );
                    };
                    for b in blocks_of(&row, &v0) {
                        let Some((_, _, blk, p)) = self.db.block(b) else {
                            continue;
                        };
                        let days = [
                            self.db.page(p).and_then(|x| x.journal),
                            blk.scheduled,
                            blk.deadline,
                        ];
                        if days
                            .iter()
                            .flatten()
                            .any(|d| (s..=e).contains(&day_int(*d)))
                        {
                            keep(b, &mut out);
                        }
                    }
                }
                "namespace" => {
                    let Some(ns) = lit(1, &row).map(|v| v.text(self.db).to_lowercase()) else {
                        continue;
                    };
                    for p in pages_of(&row, &v0) {
                        if self
                            .db
                            .page(p)
                            .is_some_and(|x| x.key.starts_with(&format!("{ns}/")))
                        {
                            keep(p, &mut out);
                        }
                    }
                }
                "page" => {
                    let want = strings(lit(1, &row));
                    for b in blocks_of(&row, &v0) {
                        let page = self.db.block(b).and_then(|x| self.db.page(x.3));
                        if page.is_some_and(|p| want.contains(&p.key)) {
                            keep(b, &mut out);
                        }
                    }
                }
                "block-content" => {
                    let want = lit(1, &row)
                        .map(|v| v.text(self.db).to_lowercase())
                        .unwrap_or_default();
                    for b in blocks_of(&row, &v0) {
                        if self
                            .db
                            .block(b)
                            .is_some_and(|x| x.2.text.to_lowercase().contains(&want))
                        {
                            keep(b, &mut out);
                        }
                    }
                }
                other => return Err(format!("({other} …): a rule Kalem does not know")),
            }
        }
        Ok(out)
    }
}

/// An EDN form, written back briefly.
fn show(e: &Edn) -> String {
    match e {
        Edn::Keyword(k) => format!(":{k}"),
        Edn::Symbol(s) => s.clone(),
        Edn::Str(s) => format!("{s:?}"),
        Edn::List(v) => format!("({})", v.iter().map(show).collect::<Vec<_>>().join(" ")),
        Edn::Vector(v) => format!("[{}]", v.iter().map(show).collect::<Vec<_>>().join(" ")),
        other => format!("{other:?}"),
    }
}

/// An input of `:inputs`: a day as a journal's, the current page, or the
/// value itself.
fn input(e: &Edn, db: &Db<'_>, today: Option<Date>, current: Option<&str>) -> Result<V, String> {
    let Edn::Keyword(k) = e else {
        return Ok(value(e));
    };
    let day = |n: i64| -> Result<V, String> {
        let t = today.ok_or("the query counts from today, which is not known yet")?;
        Ok(V::Int(day_int(t.add_days(n))))
    };
    let num = |s: &str| s.parse::<i64>().ok();
    match k.as_str() {
        "today" => day(0),
        "yesterday" => day(-1),
        "tomorrow" => day(1),
        "current-page" => Ok(current
            .and_then(|c| db.page_id.get(c).copied())
            .map_or(V::Nil, |_| V::Str(current.unwrap_or("").to_string()))),
        other => {
            if let Some(n) = other.strip_suffix('d').and_then(|s| {
                s.strip_prefix('-')
                    .and_then(num)
                    .map(|n| -n)
                    .or_else(|| s.strip_prefix('+').and_then(num))
            }) {
                return day(n);
            }
            if let Some(n) = other.strip_suffix("d-before").and_then(num) {
                return day(-n);
            }
            if let Some(n) = other.strip_suffix("d-after").and_then(num) {
                return day(n);
            }
            Ok(V::Kw(other.to_string()))
        }
    }
}

/// Runs query `a` over `index`; `current` is the key of the page it is in.
pub fn run(
    a: &Advanced,
    index: &Index,
    today: Option<Date>,
    current: Option<&str>,
) -> Result<Found, String> {
    let db = Db::new(index);
    // `[:find … :in … :where …]` or `{:find […] :in […] :where […]}`.
    let mut parts: HashMap<String, Vec<Edn>> = HashMap::new();
    match &a.query {
        Edn::Vector(v) | Edn::List(v) => {
            let mut at = String::new();
            for e in v {
                match e {
                    Edn::Keyword(k) => {
                        at = k.clone();
                        parts.entry(at.clone()).or_default();
                    }
                    other => parts.entry(at.clone()).or_default().push(other.clone()),
                }
            }
        }
        Edn::Map(m) => {
            for (k, v) in m {
                if let (Some(k), Edn::Vector(v) | Edn::List(v)) = (k.as_str(), v) {
                    parts.insert(k.to_string(), v.clone());
                }
            }
        }
        other => return Err(format!(":query is a vector: {}", show(other))),
    }
    let find = parts.get("find").cloned().unwrap_or_default();
    let mut row = Row::new();
    let ins: Vec<&Edn> = parts
        .get("in")
        .map(|v| {
            v.iter()
                .filter(|e| !matches!(e, Edn::Symbol(s) if s.starts_with('$') || s == "%"))
                .collect()
        })
        .unwrap_or_default();
    for (i, name) in ins.iter().enumerate() {
        let Edn::Symbol(n) = name else { continue };
        let Some(given) = a.inputs.get(i) else {
            return Err(format!("{n} of :in has no value in :inputs"));
        };
        row.insert(n.clone(), input(given, &db, today, current)?);
    }
    let run = Run { db: &db };
    let rows = run.solve(parts.get("where").map_or(&[][..], Vec::as_slice), vec![row])?;
    // What is found: one variable or a pull of one, or several values.
    let var_of = |e: &Edn| -> Option<(String, bool)> {
        match e {
            Edn::Symbol(s) if s.starts_with('?') => Some((s.clone(), false)),
            Edn::List(l) => match (l.first().and_then(Edn::as_str), l.get(1)) {
                (Some("pull"), Some(Edn::Symbol(s))) => Some((s.clone(), false)),
                (Some("count" | "count-distinct"), Some(Edn::Symbol(s))) => Some((s.clone(), true)),
                _ => None,
            },
            _ => None,
        }
    };
    let finds: Vec<(String, bool)> = find.iter().filter_map(var_of).collect();
    if finds.is_empty() {
        return Err(":find names no variable".into());
    }
    if let Some((v, _)) = finds
        .iter()
        .find(|(v, _)| rows.first().is_some_and(|r| !r.contains_key(v)))
    {
        return Err(format!("{v} is bound by no clause of :where"));
    }
    if finds.iter().any(|(_, count)| *count) {
        let columns: Vec<String> = find.iter().map(show).collect();
        let n = rows.len();
        let mut distinct: Vec<V> = rows
            .iter()
            .filter_map(|r| r.get(&finds[0].0).cloned())
            .collect();
        distinct.sort();
        distinct.dedup();
        let counted = if find.iter().any(|e| show(e).starts_with("(count-distinct")) {
            distinct.len()
        } else {
            n
        };
        return Ok(Found::Table(columns, vec![vec![counted.to_string()]]));
    }
    let entities: Vec<u32> = {
        let mut seen = BTreeSet::new();
        rows.iter()
            .filter_map(|r| match r.get(&finds[0].0) {
                Some(V::E(e)) => Some(*e),
                _ => None,
            })
            .filter(|e| seen.insert(*e))
            .collect()
    };
    if finds.len() == 1 && !entities.is_empty() {
        let mut hits: Vec<Hit> = entities
            .iter()
            .filter_map(|&e| match (db.page(e), db.block(e)) {
                (Some(p), _) => Some(Hit::Page(p.key.clone())),
                (_, Some((rel, i, _, _))) => Some(Hit::Block {
                    rel: rel.to_string(),
                    block: i,
                }),
                _ => None,
            })
            .collect();
        let pages = hits.iter().all(|h| matches!(h, Hit::Page(_)));
        crate::query::sort_hits(index, &mut hits);
        return Ok(Found::Hits(Answer {
            hits,
            pages,
            notes: a
                .ignored
                .iter()
                .map(|k| format!("{k} is a Clojure function Kalem does not run"))
                .collect(),
        }));
    }
    // Rows of values, each once.
    let columns: Vec<String> = find.iter().map(show).collect();
    let mut seen = BTreeSet::new();
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            finds
                .iter()
                .map(|(v, _)| r.get(v).map_or(String::new(), |x| x.text(&db)))
                .collect::<Vec<_>>()
        })
        .filter(|r| seen.insert(r.clone()))
        .collect();
    Ok(Found::Table(columns, table))
}

/// What an advanced query shows on its line in the note: how many it
/// finds and the first two, or why it does not run.
pub fn summary(
    index: &Index,
    text: &str,
    today: Option<Date>,
    current: Option<&str>,
    mark: &str,
) -> String {
    let found = parse(text).and_then(|a| {
        let title = a.title.clone();
        run(&a, index, today, current).map(|f| (title, f))
    });
    match found {
        Err(e) => format!("{mark} advanced query not run: {e}"),
        Ok((title, Found::Hits(a))) => {
            let n = a.hits.len();
            let first: Vec<String> = a
                .hits
                .iter()
                .take(2)
                .map(|h| crate::query::hit_text(index, h).chars().take(40).collect())
                .collect();
            let what = if a.pages { "page" } else { "block" };
            let s = if n == 1 { "" } else { "s" };
            let head = title.map_or(String::new(), |t| format!("{t} · "));
            if first.is_empty() {
                format!("{mark} {head}no {what}")
            } else {
                format!("{mark} {head}{n} {what}{s}: {}", first.join(" · "))
            }
        }
        Ok((title, Found::Table(cols, rows))) => {
            let head = title.map_or(String::new(), |t| format!("{t} · "));
            match (cols.len(), rows.first()) {
                (1, Some(r)) if rows.len() == 1 => format!("{mark} {head}{} = {}", cols[0], r[0]),
                _ => format!("{mark} {head}{} rows", rows.len()),
            }
        }
    }
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
                "/g/pages/Kalem.md",
                "type:: program\ntags:: editor, rust\n\n- An editor\n\t- NOW write the [[Graph]] plugin\n\t- LATER [#A] read\n",
            ),
            (
                "/g/pages/Graph.md",
                "- TODO index [[Kalem]]\n  SCHEDULED: <2026-10-12 Mon>\n- DONE draw\n",
            ),
            ("/g/journals/2026_10_10.md", "- DOING a day of [[Kalem]]\n"),
            ("/g/pages/Project___Plugins.md", "- plugins\n"),
        ]);
        Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        )
    }

    fn texts(index: &Index, f: &Found) -> Vec<String> {
        match f {
            Found::Hits(a) => a
                .hits
                .iter()
                .map(|h| crate::query::hit_text(index, h))
                .collect(),
            Found::Table(_, rows) => rows.iter().map(|r| r.join(" | ")).collect(),
        }
    }

    fn ask(q: &str) -> Vec<String> {
        let i = index();
        let a = parse(q).unwrap();
        let today = Date::new(2026, 10, 11);
        texts(&i, &run(&a, &i, today, Some("graph")).unwrap())
    }

    #[test]
    fn tasks_by_marker_and_day() {
        // Logseq's own NOW query, with its inputs.
        let q = r#"#+BEGIN_QUERY
{:title "🔨 NOW"
 :query [:find (pull ?h [*])
         :in $ ?start ?today
         :where
         [?h :block/marker ?marker]
         [(contains? #{"NOW" "DOING"} ?marker)]
         [?h :block/page ?p]
         [?p :block/journal? true]
         [?p :block/journal-day ?d]
         [(>= ?d ?start)]
         [(<= ?d ?today)]]
 :inputs [:-14d :today]
 :result-transform (fn [result] (sort-by (fn [h] (get h :block/priority "Z")) result))
 :collapsed? false}
#+END_QUERY"#;
        assert_eq!(ask(q), ["DOING a day of [[Kalem]]"]);
        let a = parse(q).unwrap();
        assert_eq!(a.title.as_deref(), Some("🔨 NOW"));
        assert_eq!(a.ignored, [":result-transform"]);
    }

    #[test]
    fn references_rules_and_negation() {
        // Blocks referring to a page, through the rule and the pattern.
        // As Logseq's rule counts them (`:block/path-refs`): the page's own
        // blocks and those under them too; journals first.
        let by_rule = ask("{:query [:find (pull ?b [*]) :where (page-ref ?b \"kalem\")]}");
        assert_eq!(by_rule.len(), 5, "{by_rule:?}");
        assert_eq!(by_rule[0], "DOING a day of [[Kalem]]");
        assert!(by_rule.contains(&"TODO index [[Kalem]]".to_string()));
        let by_pattern = ask(
            "{:query [:find (pull ?b [*]) :where [?p :block/name \"kalem\"] [?b :block/refs ?p] (not [?b :block/marker \"DOING\"])]}",
        );
        assert_eq!(by_pattern, ["TODO index [[Kalem]]"]);
        // Tasks of either marker, by the rule.
        let tasks = ask("{:query [:find (pull ?b [*]) :where (task ?b #{\"LATER\" \"TODO\"})]}");
        assert_eq!(tasks.len(), 2, "{tasks:?}");
        // `or`, priorities.
        let a = ask(
            "{:query [:find (pull ?b [*]) :where (or (priority ?b #{\"A\"}) [?b :block/marker \"DONE\"])]}",
        );
        assert_eq!(a.len(), 2, "{a:?}");
    }

    #[test]
    fn pages_properties_values_and_counts() {
        let pages = ask(
            "{:query [:find (pull ?p [*]) :where [?p :block/properties ?props] [(get ?props :type) ?t] [(= ?t \"program\")]]}",
        );
        assert_eq!(pages, ["Kalem"]);
        assert_eq!(
            ask("{:query [:find (pull ?p [*]) :where (page-property ?p :type \"program\")]}"),
            ["Kalem"]
        );
        assert_eq!(
            ask("{:query [:find (pull ?p [*]) :where (page-tags ?p #{\"rust\"})]}"),
            ["Kalem"]
        );
        assert_eq!(
            ask("{:query [:find (pull ?p [*]) :where (namespace ?p \"project\")]}"),
            ["Project/Plugins"]
        );
        // Values: a table.
        let names = ask(
            "{:query [:find ?name :where [?b :block/marker \"DONE\"] [?b :block/page ?p] [?p :block/original-name ?name]]}",
        );
        assert_eq!(names, ["Graph"]);
        assert_eq!(
            ask("{:query [:find (count ?b) :where [?b :block/marker _]]}"),
            ["5"]
        );
        // Scheduled within days, the current page.
        assert_eq!(
            ask(
                "{:query [:find (pull ?b [*]) :in $ ?a ?z :where (between ?b ?a ?z) [?b :block/marker _]] :inputs [:today :+7d]}"
            ),
            ["TODO index [[Kalem]]"]
        );
        assert_eq!(
            ask(
                "{:query [:find (pull ?b [*]) :in $ ?page :where [?p :block/name ?page] [?b :block/page ?p] [?b :block/content ?c] [(clojure.string/includes? ?c \"draw\")]] :inputs [:current-page]}"
            ),
            ["DONE draw"]
        );
    }

    #[test]
    fn what_is_not_read_is_said() {
        let i = index();
        let a = parse("{:query [:find ?b :where [(re-pattern \"x\") ?r]]}").unwrap();
        let e = run(&a, &i, None, None).unwrap_err();
        assert!(e.contains("re-pattern"), "{e}");
        assert!(parse("[:find ?b]").is_err());
        let s = summary(
            &i,
            "{:query [:find (pull ?b [*]) :where (task ?b #{\"NOW\"})]}",
            None,
            None,
            "⌕",
        );
        assert_eq!(s, "⌕ 1 block: NOW write the [[Graph]] plugin");
    }
}
