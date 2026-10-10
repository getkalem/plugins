//! The index of a graph: its pages (files, journals, and names only
//! referenced), their blocks, and every reference resolved, so that a
//! page knows what links to it. Built from the files, kept current file by
//! file; nothing is written.

use std::collections::{BTreeMap, HashMap};

use crate::config::{Graph, Kind, NewFile};
use crate::date::Date;
use crate::files::{self, Files};
use crate::names;
use crate::scan::{self, Block, Flavor, RefKind, Scanned};

/// A page of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// How the index names it: its title in lower case (Logseq), its path
    /// without `.md` in lower case (Obsidian).
    pub key: String,
    /// Its title.
    pub title: String,
    /// Its file, relative to the root; none for a page only referenced.
    pub path: Option<String>,
    /// Its aliases.
    pub aliases: Vec<String>,
    /// Its day, when it is a journal.
    pub journal: Option<Date>,
    /// Its properties.
    pub props: Vec<(String, String)>,
}

/// A file of the graph, scanned.
#[derive(Debug, Clone)]
pub struct FileData {
    /// Its page's key.
    pub key: String,
    /// How it was scanned.
    pub flavor: Flavor,
    /// What it holds.
    pub scanned: Scanned,
}

/// A reference to a page or a block, where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackRef {
    /// The file it is in, relative to the root.
    pub path: String,
    /// The block it is in.
    pub block: Option<usize>,
    /// Its line.
    pub line: u32,
    /// What it is.
    pub kind: RefKind,
}

/// A task of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// The file, relative.
    pub path: String,
    /// The block.
    pub block: usize,
    /// Its keyword.
    pub marker: String,
}

/// The index of one graph.
#[derive(Debug, Clone)]
pub struct Index {
    /// The graph.
    pub graph: Graph,
    pages: BTreeMap<String, Page>,
    files: BTreeMap<String, FileData>,
    /// Names and aliases (Logseq), in lower case: their page.
    names: HashMap<String, String>,
    /// File names without extension (Obsidian), in lower case: the pages.
    stems: HashMap<String, Vec<String>>,
    /// Block ids: the file and the block.
    blocks: HashMap<String, (String, usize)>,
    incoming: HashMap<String, Vec<BackRef>>,
    block_incoming: HashMap<String, Vec<BackRef>>,
    tags: BTreeMap<String, (String, Vec<BackRef>)>,
    /// Files not read, and why.
    pub problems: Vec<String>,
    /// The whiteboards and canvases: pages drawn by their application.
    drawings: std::collections::BTreeSet<String>,
    /// Changed with every change of the index, across indexes.
    version: u64,
}

/// The versions given out.
static VERSIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Whether the file `rel` is a page its application draws and Kalem does
/// not: a Logseq whiteboard (`whiteboards/*.edn`), an Obsidian canvas
/// (`*.canvas`). Indexed by name, so that links to it resolve; never
/// read.
pub fn drawn_elsewhere(graph: &Graph, rel: &str) -> bool {
    let (_, ext) = files::stem_ext(rel);
    match graph.kind {
        Kind::Logseq => {
            ext == "edn"
                && graph
                    .whiteboards_dir
                    .as_deref()
                    .is_some_and(|d| rel.starts_with(&format!("{d}/")))
        }
        Kind::Obsidian => ext == "canvas",
    }
}

/// The files the index reads, by extension.
fn readable(kind: Kind, ext: &str) -> Option<Flavor> {
    match (kind, ext) {
        (Kind::Logseq, "md" | "markdown") => Some(Flavor::LogseqMarkdown),
        (Kind::Logseq, "org") => Some(Flavor::LogseqOrg),
        (Kind::Obsidian, "md") => Some(Flavor::Obsidian),
        _ => None,
    }
}

impl Index {
    /// An empty index of `graph`.
    pub fn new(graph: Graph) -> Index {
        Index {
            graph,
            pages: BTreeMap::new(),
            files: BTreeMap::new(),
            names: HashMap::new(),
            stems: HashMap::new(),
            blocks: HashMap::new(),
            incoming: HashMap::new(),
            block_incoming: HashMap::new(),
            tags: BTreeMap::new(),
            problems: Vec::new(),
            drawings: std::collections::BTreeSet::new(),
            version: VERSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// A number that changes whenever the index does, and no two indexes
    /// share: what was computed from the index is good while it stays.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The index of `graph`, its files read.
    pub fn build(files: &dyn Files, graph: Graph) -> Index {
        let mut index = Index::new(graph);
        let folders: Vec<String> = match index.graph.kind {
            Kind::Logseq => {
                let mut f = vec![
                    index.graph.pages_dir.clone(),
                    index.graph.journals_dir.clone(),
                ];
                f.extend(index.graph.whiteboards_dir.clone());
                f.dedup();
                f
            }
            Kind::Obsidian => vec![String::new()],
        };
        let mut seen = std::collections::BTreeSet::new();
        for folder in folders {
            let mut todo = vec![index.graph.path(&folder)];
            while let Some(dir) = todo.pop() {
                let Ok(entries) = files.list(&dir) else {
                    continue;
                };
                for e in entries {
                    // The entry under the folder asked for: Kalem lists a
                    // folder by its real path (`/private/tmp` for `/tmp`
                    // on macOS, a link's target), and the graph's files
                    // keep its root's.
                    let path = files::join(&dir, files::file_name(&files::normalize(&e)));
                    let Some(rel) = files::relative(&index.graph.root, &path).map(str::to_string)
                    else {
                        continue;
                    };
                    if index.graph.is_hidden(&rel) || !seen.insert(rel.clone()) {
                        continue;
                    }
                    if e.ends_with('/') {
                        todo.push(path);
                        continue;
                    }
                    let (_, ext) = files::stem_ext(&rel);
                    if drawn_elsewhere(&index.graph, &rel) {
                        index.add_drawing(&rel);
                        continue;
                    }
                    if readable(index.graph.kind, &ext).is_none() {
                        continue;
                    }
                    match files.read(&path) {
                        Ok(text) => index.add(&rel, &text),
                        Err(e) => index.problems.push(e),
                    }
                }
            }
        }
        index.derive();
        index
    }

    /// Reads `path` (absolute) again, or forgets it when it is gone or no
    /// longer the graph's; whether the index changed.
    pub fn update(&mut self, files: &dyn Files, path: &str) -> bool {
        let path = files::normalize(path);
        let Some(rel) = files::relative(&self.graph.root, &path).map(str::to_string) else {
            return false;
        };
        let (_, ext) = files::stem_ext(&rel);
        if drawn_elsewhere(&self.graph, &rel) && !self.graph.is_hidden(&rel) {
            let there = files.read(&path).is_ok() || files.list(&path).is_ok();
            let known = self.drawings.contains(&rel);
            match (there, known) {
                (true, false) => self.add_drawing(&rel),
                (false, true) => {
                    self.remove(&rel);
                }
                _ => return false,
            }
            self.derive();
            return true;
        }
        let ours = readable(self.graph.kind, &ext).is_some()
            && !self.graph.is_hidden(&rel)
            && self.in_folders(&rel);
        let changed = match files.read(&path) {
            Ok(text) if ours => {
                self.remove(&rel);
                self.add(&rel, &text);
                true
            }
            _ => self.remove(&rel),
        };
        if changed {
            self.derive();
        }
        changed
    }

    /// The text of `path` (relative) as it is in the editor, unsaved.
    pub fn update_text(&mut self, rel: &str, text: &str) {
        self.remove(rel);
        self.add(rel, text);
        self.derive();
    }

    fn in_folders(&self, rel: &str) -> bool {
        match self.graph.kind {
            Kind::Obsidian => true,
            Kind::Logseq => [&self.graph.pages_dir, &self.graph.journals_dir]
                .iter()
                .any(|d| d.is_empty() || rel.starts_with(&format!("{d}/"))),
        }
    }

    fn remove(&mut self, rel: &str) -> bool {
        if self.drawings.remove(rel) {
            self.pages.retain(|_, p| p.path.as_deref() != Some(rel));
            return true;
        }
        match self.files.remove(rel) {
            Some(data) => {
                if self.pages.get(&data.key).and_then(|p| p.path.as_deref()) == Some(rel) {
                    self.pages.remove(&data.key);
                }
                true
            }
            None => false,
        }
    }

    /// The page a file is, its title and day.
    fn page_for(&self, rel: &str, s: &Scanned) -> Page {
        let g = &self.graph;
        let (stem, _) = files::stem_ext(rel);
        match g.kind {
            Kind::Logseq => {
                let in_journals = rel.starts_with(&format!("{}/", g.journals_dir));
                let journal = if in_journals {
                    g.journal_file.parse(stem)
                } else {
                    None
                };
                let title = match (journal, &s.title) {
                    (Some(d), _) => g.journal_title.format(d),
                    (None, Some(t)) if !t.is_empty() => t.clone(),
                    _ => names::file_to_title(stem, g.file_names),
                };
                Page {
                    key: names::key(&title),
                    title,
                    path: Some(rel.to_string()),
                    aliases: s.aliases.clone(),
                    journal,
                    props: s.props.clone(),
                }
            }
            Kind::Obsidian => {
                let no_ext = &rel[..rel.len() - 3];
                let journal = match files::relative(&g.journals_dir, no_ext) {
                    Some(r) if !g.journals_dir.is_empty() => g.journal_file.parse(r),
                    _ if g.journals_dir.is_empty() => g.journal_file.parse(no_ext),
                    _ => None,
                };
                Page {
                    key: no_ext.to_lowercase(),
                    title: stem.to_string(),
                    path: Some(rel.to_string()),
                    aliases: s.aliases.clone(),
                    journal,
                    props: s.props.clone(),
                }
            }
        }
    }

    /// Adds the whiteboard or canvas `rel` as a page without blocks;
    /// [`Index::derive`] must follow.
    fn add_drawing(&mut self, rel: &str) {
        let g = &self.graph;
        let (stem, _) = files::stem_ext(rel);
        let page = match g.kind {
            Kind::Logseq => {
                let title = names::file_to_title(stem, g.file_names);
                Page {
                    key: names::key(&title),
                    title,
                    path: Some(rel.to_string()),
                    aliases: Vec::new(),
                    journal: None,
                    props: Vec::new(),
                }
            }
            // Linked with its extension, `[[Board.canvas]]`.
            Kind::Obsidian => Page {
                key: rel.to_lowercase(),
                title: files::file_name(rel).to_string(),
                path: Some(rel.to_string()),
                aliases: Vec::new(),
                journal: None,
                props: Vec::new(),
            },
        };
        if self.pages.get(&page.key).is_some_and(|p| p.path.is_some()) {
            return;
        }
        self.drawings.insert(rel.to_string());
        self.pages.insert(page.key.clone(), page);
    }

    /// The whiteboards and canvases, by path relative to the root.
    pub fn drawings(&self) -> impl Iterator<Item = &String> {
        self.drawings.iter()
    }

    /// Adds the file `rel` with `text`; [`Index::derive`] must follow.
    fn add(&mut self, rel: &str, text: &str) {
        let (_, ext) = files::stem_ext(rel);
        let Some(flavor) = readable(self.graph.kind, &ext) else {
            return;
        };
        let options = scan::Options {
            comma_properties: self.graph.comma_properties.clone(),
        };
        let scanned = scan::scan(text, flavor, &options);
        let page = self.page_for(rel, &scanned);
        let key = page.key.clone();
        match self.pages.get(&key) {
            Some(other) if other.path.is_some() && other.path.as_deref() != Some(rel) => {
                self.problems.push(format!(
                    "{rel}: the page {} is also {}; its references count for that file",
                    page.title,
                    other.path.as_deref().unwrap_or_default()
                ));
            }
            _ => {
                self.pages.insert(key.clone(), page);
            }
        }
        self.files.insert(
            rel.to_string(),
            FileData {
                key,
                flavor,
                scanned,
            },
        );
    }

    /// The maps built from the files: names, block ids, the pages only
    /// referenced, what links where.
    fn derive(&mut self) {
        self.version = VERSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pages.retain(|_, p| p.path.is_some());
        self.names.clear();
        self.stems.clear();
        self.blocks.clear();
        self.incoming.clear();
        self.block_incoming.clear();
        self.tags.clear();
        for (key, p) in &self.pages {
            if self.graph.kind == Kind::Logseq {
                self.names.insert(names::key(&p.title), key.clone());
                if let Some(path) = &p.path {
                    let (stem, _) = files::stem_ext(path);
                    let from_file = names::key(&names::file_to_title(stem, self.graph.file_names));
                    self.names.entry(from_file).or_insert_with(|| key.clone());
                    self.names
                        .entry(names::key(stem))
                        .or_insert_with(|| key.clone());
                }
            } else if let Some(path) = &p.path {
                let (stem, ext) = files::stem_ext(path);
                let name = if ext == "md" {
                    stem.to_lowercase()
                } else {
                    files::file_name(path).to_lowercase()
                };
                self.stems.entry(name).or_default().push(key.clone());
            }
        }
        if self.graph.kind == Kind::Logseq {
            for (key, p) in &self.pages {
                for a in &p.aliases {
                    self.names
                        .entry(names::key(a))
                        .or_insert_with(|| key.clone());
                }
            }
        }
        for (rel, data) in &self.files {
            for (i, b) in data.scanned.blocks.iter().enumerate() {
                if let Some(id) = &b.id {
                    let id = match self.graph.kind {
                        Kind::Logseq => id.clone(),
                        Kind::Obsidian => format!("{}#^{id}", data.key),
                    };
                    self.blocks.entry(id).or_insert_with(|| (rel.clone(), i));
                }
            }
        }
        let mut virtual_pages: BTreeMap<String, Page> = BTreeMap::new();
        let files: Vec<(String, String, Vec<scan::Ref>)> = self
            .files
            .iter()
            .map(|(rel, d)| (rel.clone(), d.key.clone(), d.scanned.refs.clone()))
            .collect();
        for (rel, own, refs) in files {
            for r in refs {
                let back = BackRef {
                    path: rel.clone(),
                    block: r.block,
                    line: r.line,
                    kind: r.kind,
                };
                if self.graph.kind == Kind::Obsidian && r.kind == RefKind::Tag {
                    let k = r.target.to_lowercase();
                    self.tags
                        .entry(k)
                        .or_insert_with(|| (r.target.clone(), Vec::new()))
                        .1
                        .push(back);
                    continue;
                }
                if self.graph.kind == Kind::Logseq && r.kind.to_block() {
                    self.block_incoming
                        .entry(r.target.clone())
                        .or_default()
                        .push(back);
                    continue;
                }
                let key = self.resolve(&r.target, &rel);
                if r.kind == RefKind::Tag {
                    self.tags
                        .entry(key.clone())
                        .or_insert_with(|| (r.target.clone(), Vec::new()))
                        .1
                        .push(back.clone());
                }
                if self.graph.kind == Kind::Obsidian
                    && r.kind.to_block()
                    && let Some(a) = &r.anchor
                {
                    self.block_incoming
                        .entry(format!("{key}#^{}", a.to_lowercase()))
                        .or_default()
                        .push(back.clone());
                }
                if !self.pages.contains_key(&key) {
                    virtual_pages.entry(key.clone()).or_insert_with(|| Page {
                        key: key.clone(),
                        title: r.target.clone(),
                        path: None,
                        aliases: Vec::new(),
                        journal: self.graph.journal_title.parse(&r.target),
                        props: Vec::new(),
                    });
                }
                if key != own {
                    self.incoming.entry(key).or_default().push(back);
                }
            }
        }
        self.pages.extend(virtual_pages);
    }

    /// The key of the page `target` names from the file `from`
    /// (relative): an existing page, or the key a page of that name would
    /// have.
    pub fn resolve(&self, target: &str, from: &str) -> String {
        match self.graph.kind {
            Kind::Logseq => {
                let k = names::key(target);
                if let Some(p) = self.names.get(&k) {
                    return p.clone();
                }
                if let Some(d) = self.graph.journal_title.parse(target.trim()) {
                    return names::key(&self.graph.journal_title.format(d));
                }
                k
            }
            Kind::Obsidian => {
                let t = target.trim().trim_start_matches('/');
                let t = t.strip_suffix(".md").unwrap_or(t).to_lowercase();
                if t.contains('/') {
                    if self.pages.get(&t).is_some_and(|p| p.path.is_some()) {
                        return t;
                    }
                    let here = files::parent(from).map_or(String::new(), |d| {
                        if d == "/" {
                            String::new()
                        } else {
                            d.to_lowercase()
                        }
                    });
                    let joined = normalize_dots(&files::join(&here, &t));
                    if self.pages.get(&joined).is_some_and(|p| p.path.is_some()) {
                        return joined;
                    }
                    let suffix = format!("/{t}");
                    let mut found: Vec<&String> = self
                        .pages
                        .iter()
                        .filter(|(k, p)| p.path.is_some() && k.ends_with(&suffix))
                        .map(|(k, _)| k)
                        .collect();
                    found.sort_by_key(|k| (k.len(), (*k).clone()));
                    return found.first().map_or(t, |k| (*k).clone());
                }
                match self.stems.get(&t) {
                    Some(c) if c.len() == 1 => c[0].clone(),
                    Some(c) => {
                        let here = files::parent(from)
                            .filter(|d| *d != "/")
                            .unwrap_or("")
                            .to_lowercase();
                        let mut c = c.clone();
                        c.sort_by_key(|k| {
                            let dir = files::parent(k)
                                .filter(|d| *d != "/")
                                .unwrap_or("")
                                .to_string();
                            (dir != here, k.len(), k.clone())
                        });
                        c[0].clone()
                    }
                    None => t,
                }
            }
        }
    }

    /// A page by its key.
    pub fn page(&self, key: &str) -> Option<&Page> {
        self.pages.get(key)
    }

    /// Every page, by key.
    pub fn pages(&self) -> impl Iterator<Item = &Page> {
        self.pages.values()
    }

    /// The page of the file `rel`.
    pub fn page_of(&self, rel: &str) -> Option<&Page> {
        self.files.get(rel).and_then(|d| self.pages.get(&d.key))
    }

    /// A file's data.
    pub fn file(&self, rel: &str) -> Option<&FileData> {
        self.files.get(rel)
    }

    /// Every file, by path.
    pub fn files(&self) -> impl Iterator<Item = (&String, &FileData)> {
        self.files.iter()
    }

    /// What links to the page `key`.
    pub fn backlinks(&self, key: &str) -> &[BackRef] {
        self.incoming.get(key).map_or(&[], Vec::as_slice)
    }

    /// What refers to the block `id` (Logseq's UUID, or Obsidian's
    /// `page#^id`).
    pub fn block_backlinks(&self, id: &str) -> &[BackRef] {
        self.block_incoming.get(id).map_or(&[], Vec::as_slice)
    }

    /// The block with the id `id`: its file and the block.
    pub fn block(&self, id: &str) -> Option<(&str, &Block)> {
        let (rel, i) = self.blocks.get(id)?;
        let b = self.files.get(rel)?.scanned.blocks.get(*i)?;
        Some((rel.as_str(), b))
    }

    /// Every block that has an id: the id, the file and the block.
    pub fn blocks_with_ids(&self) -> impl Iterator<Item = (&str, &str, &Block)> {
        self.blocks.iter().filter_map(|(id, (rel, i))| {
            let b = self.files.get(rel)?.scanned.blocks.get(*i)?;
            Some((id.as_str(), rel.as_str(), b))
        })
    }

    /// The tags, by name in lower case: the name as first written and the
    /// references.
    pub fn tags(&self) -> &BTreeMap<String, (String, Vec<BackRef>)> {
        &self.tags
    }

    /// The journals that have a file, oldest first.
    pub fn journals(&self) -> Vec<(Date, &Page)> {
        let mut out: Vec<(Date, &Page)> = self
            .pages
            .values()
            .filter(|p| p.path.is_some())
            .filter_map(|p| p.journal.map(|d| (d, p)))
            .collect();
        out.sort_by_key(|(d, _)| *d);
        out
    }

    /// The journal of `date`: its page when it has a file.
    pub fn journal(&self, date: Date) -> Option<&Page> {
        self.journals()
            .into_iter()
            .find(|(d, _)| *d == date)
            .map(|(_, p)| p)
    }

    /// The file a new journal of `date` is written to, relative.
    pub fn journal_path(&self, date: Date) -> String {
        let g = &self.graph;
        let name = format!("{}.{}", g.journal_file.format(date), g.format.extension());
        let name = match g.kind {
            Kind::Logseq => name,
            Kind::Obsidian => format!("{}.md", g.journal_file.format(date)),
        };
        files::join(&g.journals_dir, &name)
            .trim_start_matches('/')
            .to_string()
    }

    /// The file a new page called `title` is written to, relative; `from`
    /// is the note the user is in (Obsidian's "current folder").
    pub fn page_path(&self, title: &str, from: Option<&str>) -> String {
        let g = &self.graph;
        match g.kind {
            Kind::Logseq => files::join(
                &g.pages_dir,
                &format!("{}.{}", names::title_to_file(title), g.format.extension()),
            ),
            Kind::Obsidian => {
                // Obsidian keeps `/` as folders.
                let name = format!("{}.md", title.trim().trim_end_matches(".md"));
                if title.contains('/') {
                    return name;
                }
                let dir = match &g.new_file {
                    NewFile::Root => String::new(),
                    NewFile::Folder(f) => f.clone(),
                    NewFile::Current => from
                        .and_then(files::parent)
                        .filter(|d| *d != "/")
                        .unwrap_or("")
                        .to_string(),
                };
                files::join(&dir, &name)
            }
        }
        .trim_start_matches('/')
        .to_string()
    }

    /// The tasks: blocks with a keyword, but those of a template (a block
    /// with `template::` and the blocks under it), which are copied, not
    /// done.
    pub fn tasks(&self) -> Vec<Task> {
        let mut out = Vec::new();
        for (rel, d) in &self.files {
            let blocks = &d.scanned.blocks;
            let in_template = |mut i: usize| loop {
                if blocks[i].props.iter().any(|(k, _)| k == "template") {
                    return true;
                }
                match blocks[i].parent {
                    Some(p) => i = p,
                    None => return false,
                }
            };
            for (i, b) in blocks.iter().enumerate() {
                if let Some(m) = &b.marker
                    && !in_template(i)
                {
                    out.push(Task {
                        path: rel.clone(),
                        block: i,
                        marker: m.clone(),
                    });
                }
            }
        }
        out
    }

    /// How many blocks name each page without linking to it, for every
    /// page with a file: as [`Index::unlinked`] counts them, in one pass
    /// over the blocks (the names looked up by their first word).
    pub fn unlinked_counts(&self) -> BTreeMap<String, usize> {
        let first_word =
            |n: &str| -> String { n.chars().take_while(|c| c.is_alphanumeric()).collect() };
        let mut by_word: HashMap<String, Vec<(String, &str)>> = HashMap::new();
        for p in self.pages.values().filter(|p| p.path.is_some()) {
            let mut names: Vec<String> = std::iter::once(p.title.clone())
                .chain(p.aliases.iter().cloned())
                .map(|n| n.to_lowercase())
                .filter(|n| n.chars().count() >= 3)
                .collect();
            names.sort();
            names.dedup();
            for n in names {
                by_word
                    .entry(first_word(&n))
                    .or_default()
                    .push((n, p.key.as_str()));
            }
        }
        let mut out: BTreeMap<String, usize> = BTreeMap::new();
        for (rel, d) in &self.files {
            for (i, b) in d.scanned.blocks.iter().enumerate() {
                let text = b.text.to_lowercase();
                let mut here: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
                let mut prev: Option<char> = None;
                for (at, ch) in text.char_indices() {
                    let boundary = !prev.is_some_and(char::is_alphanumeric);
                    prev = Some(ch);
                    if !boundary {
                        continue;
                    }
                    let word = if ch.is_alphanumeric() {
                        first_word(&text[at..])
                    } else {
                        String::new()
                    };
                    let Some(cands) = by_word.get(&word) else {
                        continue;
                    };
                    let inside_link = text[..at].ends_with("[[") || text[..at].ends_with('#');
                    if inside_link {
                        continue;
                    }
                    for (name, key) in cands {
                        if text[at..].starts_with(name.as_str())
                            && !text[at + name.len()..]
                                .chars()
                                .next()
                                .is_some_and(char::is_alphanumeric)
                        {
                            here.insert(key);
                        }
                    }
                }
                for key in here {
                    let page = &self.pages[key];
                    if page.path.as_deref() == Some(rel.as_str()) {
                        continue;
                    }
                    let linked = d
                        .scanned
                        .refs
                        .iter()
                        .any(|r| r.block == Some(i) && self.resolve(&r.target, rel) == key);
                    if !linked {
                        *out.entry(key.to_string()).or_default() += 1;
                    }
                }
            }
        }
        out
    }

    /// The blocks whose first line names the page `key` (its title or an
    /// alias, as a whole word, ignoring case) without linking to it,
    /// outside its own file.
    pub fn unlinked(&self, key: &str) -> Vec<BackRef> {
        let Some(page) = self.pages.get(key) else {
            return Vec::new();
        };
        let mut wanted: Vec<String> = std::iter::once(page.title.clone())
            .chain(page.aliases.iter().cloned())
            .map(|n| n.to_lowercase())
            .filter(|n| n.chars().count() >= 3)
            .collect();
        wanted.sort();
        wanted.dedup();
        let mut out = Vec::new();
        for (rel, d) in &self.files {
            if page.path.as_deref() == Some(rel.as_str()) {
                continue;
            }
            for (i, b) in d.scanned.blocks.iter().enumerate() {
                let text = b.text.to_lowercase();
                let linked = d
                    .scanned
                    .refs
                    .iter()
                    .any(|r| r.block == Some(i) && self.resolve(&r.target, rel) == key);
                if linked {
                    continue;
                }
                if wanted.iter().any(|w| whole_word(&text, w)) {
                    out.push(BackRef {
                        path: rel.clone(),
                        block: Some(i),
                        line: b.line,
                        kind: RefKind::Page,
                    });
                }
            }
        }
        out
    }

    /// Counts: pages with a file, journals, blocks, references.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let files = self.pages.values().filter(|p| p.path.is_some()).count();
        let journals = self.journals().len();
        let blocks = self.files.values().map(|d| d.scanned.blocks.len()).sum();
        let refs = self.files.values().map(|d| d.scanned.refs.len()).sum();
        (files, journals, blocks, refs)
    }
}

/// `a/./b/../c` as `a/c`.
fn normalize_dots(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out.join("/")
}

/// Whether `word` is in `text` with no letter or digit on either side.
fn whole_word(text: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(p) = text[from..].find(word) {
        let start = from + p;
        let end = start + word.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        let inside_link = text[..start].ends_with("[[") || text[..start].ends_with('#');
        if !before.is_some_and(char::is_alphanumeric)
            && !after.is_some_and(char::is_alphanumeric)
            && !inside_link
        {
            return true;
        }
        from = start + word.len().max(1);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Fallbacks;
    use crate::files::Memory;

    fn logseq() -> (Memory, Index) {
        let m = Memory::new(&[
            ("/g/logseq/config.edn", "{:file/name-format :triple-lowbar}"),
            (
                "/g/pages/Kalem.md",
                "alias:: Editor\n\n- An editor [[Org]]\n  id:: 11111111-2222-3333-4444-555555555555\n",
            ),
            (
                "/g/pages/Project___Plugins.md",
                "- links [[kalem]] and [[EDITOR]] #idea\n- TODO write ((11111111-2222-3333-4444-555555555555))\n",
            ),
            (
                "/g/pages/Named.md",
                "title:: Real Title\n\n- a kalem mention without a link\n",
            ),
            (
                "/g/journals/2026_10_03.md",
                "- met [[Project/Plugins]] and [[Oct 4th, 2026]]\n",
            ),
            ("/g/journals/2026_10_04.md", "- DONE ship\n"),
            ("/g/logseq/bak/pages/Kalem.md", "- [[ignored]]"),
            ("/g/assets/x.md", "- [[ignored]]"),
        ]);
        let g = Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default());
        let i = Index::build(&m, g);
        (m, i)
    }

    #[test]
    fn logseq_pages_names_and_backlinks() {
        let (_, i) = logseq();
        assert_eq!(
            i.counts().0,
            5,
            "{:?}",
            i.pages().map(|p| &p.key).collect::<Vec<_>>()
        );
        assert_eq!(
            i.page("project/plugins").unwrap().path.as_deref(),
            Some("pages/Project___Plugins.md")
        );
        assert_eq!(
            i.page("real title").unwrap().path.as_deref(),
            Some("pages/Named.md")
        );
        assert!(i.page("ignored").is_none());
        // `[[kalem]]` and the alias `[[EDITOR]]` both reach Kalem.
        let back = i.backlinks("kalem");
        assert_eq!(back.len(), 2);
        assert!(back.iter().all(|b| b.path == "pages/Project___Plugins.md"));
        // The journals and the reference to one by its title.
        let j = i.journals();
        assert_eq!(j.len(), 2);
        assert_eq!(j[0].1.title, "Oct 3rd, 2026");
        assert_eq!(i.backlinks("oct 4th, 2026").len(), 1);
        assert_eq!(
            i.backlinks("project/plugins")[0].path,
            "journals/2026_10_03.md"
        );
        // A page only referenced.
        let org = i.page("org").unwrap();
        assert!(org.path.is_none());
        assert_eq!(i.backlinks("org").len(), 1);
        // Tags are pages.
        assert_eq!(i.backlinks("idea").len(), 1);
        assert_eq!(i.tags().get("idea").unwrap().1.len(), 1);
        // Block references.
        let (rel, b) = i.block("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(
            (rel, b.text.as_str()),
            ("pages/Kalem.md", "An editor [[Org]]")
        );
        assert_eq!(
            i.block_backlinks("11111111-2222-3333-4444-555555555555")
                .len(),
            1
        );
        // Unlinked mentions.
        let u = i.unlinked("kalem");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].path, "pages/Named.md");
        // Tasks.
        assert_eq!(i.tasks().len(), 2);
        // New files.
        assert_eq!(
            i.journal_path(Date::new(2026, 10, 10).unwrap()),
            "journals/2026_10_10.md"
        );
        assert_eq!(i.page_path("A/B?", None), "pages/A___B%3F.md");
    }

    #[test]
    fn logseq_updates() {
        let (m, mut i) = logseq();
        m.write("/g/pages/New.md", "- [[Kalem]] again\n").unwrap();
        assert!(i.update(&m, "/g/pages/New.md"));
        assert_eq!(i.backlinks("kalem").len(), 3);
        m.remove("/g/pages/Project___Plugins.md");
        assert!(i.update(&m, "/g/pages/Project___Plugins.md"));
        assert_eq!(i.backlinks("kalem").len(), 1);
        // The journal still names the removed page, which is now virtual.
        assert!(i.page("project/plugins").unwrap().path.is_none());
        assert!(!i.update(&m, "/elsewhere/x.md"));
        assert!(!i.update(&m, "/g/logseq/config.edn"));
    }

    #[test]
    fn obsidian_resolution() {
        let m = Memory::new(&[
            ("/v/.obsidian/daily-notes.json", r#"{"folder":"Daily"}"#),
            (
                "/v/Home.md",
                "---\naliases: [Start]\n---\n[[Note]] [[Deep/Note]] [[Other#^b1]] #tag [[Missing]]\n",
            ),
            ("/v/Note.md", "x"),
            ("/v/Deep/Note.md", "y [[Note]]"),
            ("/v/Deep/Other.md", "a paragraph ^b1\n"),
            ("/v/Daily/2026-10-03.md", "- [[Start]] #Tag\n"),
            ("/v/.trash/Old.md", "[[Home]]"),
        ]);
        let g = Graph::load(&m, "/v", Kind::Obsidian, &Fallbacks::default());
        let i = Index::build(&m, g);
        assert_eq!(i.resolve("Note", "Home.md"), "note");
        assert_eq!(
            i.resolve("Note", "Deep/Other.md"),
            "deep/note",
            "the same folder first"
        );
        assert_eq!(i.resolve("Deep/Note", "Home.md"), "deep/note");
        assert_eq!(i.resolve("note.md", "Home.md"), "note");
        assert_eq!(i.backlinks("note").len(), 1);
        assert_eq!(i.backlinks("deep/note").len(), 1);
        // Obsidian's aliases link nowhere: [[Start]] is a page not written.
        assert!(i.page("start").unwrap().path.is_none());
        assert!(i.page("missing").unwrap().path.is_none());
        // Tags are not pages; they compare ignoring case.
        assert!(i.page("tag").is_none());
        assert_eq!(i.tags().get("tag").unwrap().1.len(), 2);
        // A block reference reaches the note and the block.
        assert_eq!(i.backlinks("deep/other").len(), 1);
        assert_eq!(i.block_backlinks("deep/other#^b1").len(), 1);
        assert_eq!(i.block("deep/other#^b1").unwrap().0, "Deep/Other.md");
        // The daily note.
        let j = i.journals();
        assert_eq!(j.len(), 1);
        assert_eq!(j[0].0, Date::new(2026, 10, 3).unwrap());
        assert_eq!(
            i.journal_path(Date::new(2026, 10, 10).unwrap()),
            "Daily/2026-10-10.md"
        );
        assert!(i.page("home").is_some());
        assert!(i.page_of(".trash/Old.md").is_none());
    }

    #[test]
    fn words() {
        assert!(whole_word("about kalem today", "kalem"));
        assert!(!whole_word("kalemler", "kalem"));
        assert!(!whole_word("see [[kalem", "kalem"));
        assert_eq!(normalize_dots("a/./b/../c"), "a/c");
    }
}
