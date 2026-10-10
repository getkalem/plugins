//! The plugin as Kalem runs it: the graphs found, their indexes, the
//! documents and the panel open, and what each command, event and answer
//! does, as effects the component carries out. Files are read and new
//! ones written through [`Files`] at once; everything that reaches the
//! editor is an [`Effect`].

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{self, Fallbacks, Graph, Kind, LinkFormat};
use crate::content::{Content, Target};
use crate::date::{self, Date};
use crate::files::{self, Files};
use crate::index::{Index, Page};
use crate::scan::{self, Flavor, RefKind};
use crate::template;
use crate::views::{self, Glyphs, PanelNode};

/// The backlinks panel's ID.
pub const PANEL: &str = "graph.backlinks";
/// The status bar item's ID.
pub const STATUS: &str = "graph.status";
/// Where Doom's leader keys apply: Vim's command mode.
pub const LEADER_WHEN: &str = "vimCommand";
/// The documents' kinds.
pub const KINDS: &[&str] = &[
    "graph-backlinks",
    "graph-pages",
    "graph-journals",
    "graph-tags",
    "graph-tasks",
    "graph-graph",
];
/// Where the documents' keys apply.
pub const DOC_WHEN: &str = "(textType == graph-backlinks || textType == graph-pages || textType == graph-journals || textType == graph-tags || textType == graph-tasks || textType == graph-graph) && (vimCommand || !vimActive)";

/// A command of the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    /// `graph.NAME`.
    pub id: &'static str,
    /// For the palette and the menus.
    pub title: &'static str,
    /// Doom's keys under the leader, as `keymap.json` writes them.
    pub keys: &'static [&'static str],
    /// Its keys in the plugin's documents.
    pub doc_keys: &'static [&'static str],
}

const fn c(id: &'static str, title: &'static str, keys: &'static [&'static str]) -> CommandInfo {
    CommandInfo {
        id,
        title,
        keys,
        doc_keys: &[],
    }
}

/// The commands, on Doom's `SPC n r` map (org-roam's): `r` the backlinks
/// buffer, `f` find, `i` insert, `s` sync, `d` the dailies.
pub const COMMANDS: &[CommandInfo] = &[
    c("graph.backlinks", "Backlinks Panel", &["space n r r"]),
    c(
        "graph.backlinksDocument",
        "Backlinks",
        &["space n r shift+r"],
    ),
    c("graph.follow", "Follow Reference", &["space n r o"]),
    c("graph.findPage", "Find Page", &["space n r f"]),
    c("graph.insertLink", "Insert Link", &["space n r i"]),
    c(
        "graph.insertBlockRef",
        "Insert Block Reference",
        &["space n r b"],
    ),
    c("graph.today", "Today's Journal", &["space n r d t"]),
    c("graph.yesterday", "Yesterday's Journal", &["space n r d y"]),
    c("graph.tomorrow", "Tomorrow's Journal", &["space n r d m"]),
    c("graph.journalOn", "Journal of a Date", &["space n r d d"]),
    c("graph.nextJournal", "Next Journal", &["space n r d n"]),
    c(
        "graph.previousJournal",
        "Previous Journal",
        &["space n r d p"],
    ),
    c("graph.pages", "All Pages", &["space n r p"]),
    c("graph.journals", "Journals", &["space n r j"]),
    c("graph.tags", "Tags", &["space n r shift+t"]),
    c("graph.tasks", "Tasks", &["space n r t"]),
    c("graph.graph", "Graph", &["space n r g"]),
    c("graph.reindex", "Index Again", &["space n r s"]),
    CommandInfo {
        id: "graph.open",
        title: "Open the Line's Page",
        keys: &[],
        doc_keys: &["enter"],
    },
    c("graph.insertText", "Insert Text", &[]),
];

/// How much a notification matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// News.
    Info,
    /// Something to look at.
    Warning,
    /// Something failed.
    Error,
}

/// The plugin's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Folders that are graphs without a marker: `(path, kind)`.
    pub graphs: Vec<(String, Kind)>,
    /// What stands in for Obsidian's settings files.
    pub fallbacks: Fallbacks,
    /// `unicode` or `ascii`.
    pub glyphs: String,
    /// How many journals the Journals document shows.
    pub journals_shown: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            graphs: Vec::new(),
            fallbacks: Fallbacks::default(),
            glyphs: "unicode".into(),
            journals_shown: 30,
        }
    }
}

/// The settings the plugin reads, watched.
pub const SETTINGS: &[&str] = &[
    "graphs",
    "obsidian.daily_notes",
    "obsidian.daily_format",
    "obsidian.new_notes",
    "obsidian.link_style",
    "glyphs",
    "journals_shown",
];

impl Settings {
    /// The settings from the plugin's own, as JSON text by key; a value
    /// missing or of another type is the default.
    pub fn read(own: impl Fn(&str) -> Option<String>) -> Settings {
        let d = Settings::default();
        let json =
            |k: &str| own(k).and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok());
        let text = |k: &str| {
            json(k)
                .and_then(|v| v.as_str().map(str::to_string))
                .filter(|s| !s.trim().is_empty())
        };
        let graphs = json("graphs")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|g| {
                let (path, kind) = match &g {
                    serde_json::Value::String(p) => (p.clone(), None),
                    serde_json::Value::Object(o) => (
                        o.get("path")?.as_str()?.to_string(),
                        o.get("kind").and_then(|k| k.as_str()).map(str::to_string),
                    ),
                    _ => return None,
                };
                let kind = match kind.as_deref() {
                    Some("obsidian") => Kind::Obsidian,
                    _ => Kind::Logseq,
                };
                Some((files::normalize(&path), kind))
            })
            .collect();
        Settings {
            graphs,
            fallbacks: Fallbacks {
                daily_notes: text("obsidian.daily_notes"),
                daily_format: text("obsidian.daily_format"),
                new_notes: text("obsidian.new_notes"),
                link_style: text("obsidian.link_style"),
            },
            glyphs: text("glyphs").unwrap_or(d.glyphs),
            journals_shown: json("journals_shown")
                .and_then(|v| v.as_u64())
                .map_or(d.journals_shown, |n| n.clamp(1, 1000) as usize),
        }
    }
}

/// The document a command runs in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ctx {
    /// Its file, absolute.
    pub path: Option<String>,
    /// Its text.
    pub text: Option<String>,
    /// The cursor, a byte of the text.
    pub cursor: usize,
    /// The plugin's document it is: `(id, key)`.
    pub doc: Option<(String, String)>,
    /// The command's arguments, as JSON.
    pub args: String,
}

/// An answer the user gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A prompt's text, `None` when cancelled.
    Text(Option<String>),
    /// A pick's items, `None` when cancelled.
    Picked(Option<Vec<u32>>),
}

/// What the plugin asks of the editor.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A message.
    Notify(String, Level),
    /// The status bar item's text, or none.
    Status(Option<String>),
    /// Opens a file at a line, from 1 (`file.open`); a file that does not
    /// exist opens empty and is written when saved.
    Open {
        /// Absolute.
        path: String,
        /// From 1.
        line: u32,
    },
    /// Shows a document of the plugin (`show`), or writes it anew when it
    /// is open.
    Document {
        /// The document's ID (`graph.backlinks`).
        id: String,
        /// What tells documents of one ID apart.
        key: String,
        /// Its title.
        title: String,
        /// Its text type.
        kind: String,
        /// Its text.
        content: Content,
        /// Shown, or only written anew when open.
        show: bool,
    },
    /// The backlinks panel's entries.
    Panel(Vec<PanelNode>),
    /// Shows the backlinks panel.
    ShowPanel,
    /// Offers a list; the answer comes with `token`.
    Pick {
        /// The answer's.
        token: u64,
        /// Above the list.
        title: String,
        /// Label and detail.
        items: Vec<(String, Option<String>)>,
    },
    /// Asks for a line of text.
    Prompt {
        /// The answer's.
        token: u64,
        /// The question.
        title: String,
        /// What the line starts with.
        value: Option<String>,
        /// Shown while it is empty.
        placeholder: Option<String>,
    },
    /// Runs an editor's command later (`kalem.run`).
    Run {
        /// Its ID.
        id: String,
        /// Its arguments, JSON.
        args: String,
    },
    /// Inserts text at the cursor of the document the command runs in.
    Insert(String),
}

/// What an answer is for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    /// Today's date, then what needed it.
    Today(Then),
    FindPage {
        root: String,
        keys: Vec<Option<String>>,
        from: Option<String>,
    },
    NewPage {
        root: String,
        from: Option<String>,
    },
    InsertLink {
        root: String,
        keys: Vec<String>,
        from: Option<String>,
    },
    InsertBlockRef {
        root: String,
        ids: Vec<String>,
    },
    JournalOn {
        root: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Then {
    Journal { root: String, offset: i64 },
}

/// The separator of a document key's parts.
const SEP: char = '\u{1f}';

/// The plugin's state.
#[derive(Debug)]
pub struct App {
    settings: Settings,
    known: Vec<(String, Kind)>,
    indexes: BTreeMap<String, Index>,
    not: BTreeSet<String>,
    /// The file of the current document.
    current: Option<String>,
    /// The graph used last, for commands outside any graph.
    last: Option<String>,
    today: Option<Date>,
    /// The documents open, with what they show.
    docs: BTreeMap<(String, String), Content>,
    pending: BTreeMap<u64, Pending>,
    next: u64,
    /// The page the panel shows: `(root, key)`.
    panel: Option<(String, String)>,
    /// Where a click on each of the panel's entries goes.
    panel_targets: BTreeMap<String, Target>,
}

impl App {
    /// The plugin with its settings.
    pub fn new(settings: Settings) -> App {
        App {
            settings,
            known: Vec::new(),
            indexes: BTreeMap::new(),
            not: BTreeSet::new(),
            current: None,
            last: None,
            today: None,
            docs: BTreeMap::new(),
            pending: BTreeMap::new(),
            next: 1,
            panel: None,
            panel_targets: BTreeMap::new(),
        }
    }

    /// The day the plugin was told is today; Kalem gives plugins no clock
    /// yet.
    pub fn set_today(&mut self, today: Date) {
        self.today = Some(today);
    }

    /// The index of the graph at `root`.
    pub fn index(&self, root: &str) -> Option<&Index> {
        self.indexes.get(root)
    }

    fn glyphs(&self) -> Glyphs {
        Glyphs::new(&self.settings.glyphs)
    }

    fn token(&mut self, p: Pending) -> u64 {
        let t = self.next;
        self.next += 1;
        self.pending.insert(t, p);
        t
    }

    /// The root of the graph `path` (absolute) is in, its index built the
    /// first time; notices of what could not be read go to `out`.
    fn graph_of(&mut self, files: &dyn Files, path: &str, out: &mut Vec<Effect>) -> Option<String> {
        let path = files::normalize(path);
        if let Some(root) = self
            .indexes
            .keys()
            .filter(|r| files::relative(r, &path).is_some())
            .max_by_key(|r| r.len())
        {
            return Some(root.clone());
        }
        let mut known = self.known.clone();
        known.extend(self.settings.graphs.iter().cloned());
        let (root, kind) = config::find_root(files, &path, &known, &mut self.not)?;
        Some(self.load(files, &root, kind, out))
    }

    fn load(&mut self, files: &dyn Files, root: &str, kind: Kind, out: &mut Vec<Effect>) -> String {
        let graph = Graph::load(files, root, kind, &self.settings.fallbacks);
        let index = Index::build(files, graph);
        let mut problems = index.graph.problems.clone();
        problems.extend(index.problems.iter().cloned());
        if let Some(first) = problems.first() {
            let more = if problems.len() > 1 {
                format!(" ({} more in the process log)", problems.len() - 1)
            } else {
                String::new()
            };
            out.push(Effect::Notify(format!("{first}{more}"), Level::Warning));
        }
        let root = index.graph.root.clone();
        if !self.known.iter().any(|(r, _)| *r == root) {
            self.known.push((root.clone(), kind));
        }
        self.indexes.insert(root.clone(), index);
        root
    }

    fn status(&self) -> Effect {
        let root = self.current.as_ref().and_then(|p| {
            self.indexes
                .keys()
                .filter(|r| files::relative(r, p).is_some())
                .max_by_key(|r| r.len())
        });
        Effect::Status(root.and_then(|r| self.indexes.get(r)).map(|i| {
            let (pages, ..) = i.counts();
            let mark = if self.settings.glyphs == "ascii" {
                "graph:"
            } else {
                "⌬"
            };
            format!("{mark} {} · {pages} pages", i.graph.name())
        }))
    }

    /// The page of `path`: its graph's root and its key.
    fn page_of(&self, path: &str) -> Option<(String, String)> {
        let path = files::normalize(path);
        let root = self
            .indexes
            .keys()
            .filter(|r| files::relative(r, &path).is_some())
            .max_by_key(|r| r.len())?;
        let index = &self.indexes[root];
        let rel = files::relative(root, &path)?;
        Some((root.clone(), index.page_of(rel)?.key.clone()))
    }

    fn panel_effects(&mut self) -> Vec<Effect> {
        let page = self.current.as_deref().and_then(|p| self.page_of(p));
        match page {
            Some((root, key)) => {
                let nodes = views::backlinks_panel(&self.indexes[&root], &key);
                let mut targets = Vec::new();
                for n in &nodes {
                    n.targets(&mut targets);
                }
                self.panel_targets = targets.into_iter().collect();
                self.panel = Some((root, key));
                vec![Effect::Panel(nodes)]
            }
            None if self.panel.is_some() => {
                self.panel = None;
                vec![Effect::Panel(Vec::new())]
            }
            None => Vec::new(),
        }
    }

    /// A document opened in the editor.
    pub fn opened(&mut self, files: &dyn Files, path: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        let path = files::normalize(path);
        self.current = Some(path.clone());
        if let Some(root) = self.graph_of(files, &path, &mut out) {
            self.last = Some(root);
        }
        out.extend(self.panel_effects());
        out.push(self.status());
        out
    }

    /// A file saved, or changed on disk.
    pub fn saved(&mut self, files: &dyn Files, path: &str) -> Vec<Effect> {
        let path = files::normalize(path);
        let roots: Vec<String> = self
            .indexes
            .keys()
            .filter(|r| files::relative(r, &path).is_some())
            .cloned()
            .collect();
        let mut out = Vec::new();
        for root in roots {
            let changed = self
                .indexes
                .get_mut(&root)
                .is_some_and(|i| i.update(files, &path));
            if changed {
                out.extend(self.refresh(&root));
            }
        }
        out
    }

    /// The settings changed: the graphs are read again.
    pub fn settings(&mut self, files: &dyn Files, settings: Settings) -> Vec<Effect> {
        self.settings = settings;
        self.not.clear();
        let roots: Vec<(String, Kind)> = self
            .indexes
            .values()
            .map(|i| (i.graph.root.clone(), i.graph.kind))
            .collect();
        let mut out = Vec::new();
        for (root, kind) in roots {
            self.load(files, &root, kind, &mut out);
            out.extend(self.refresh(&root));
        }
        out
    }

    /// The user closed a document of the plugin.
    pub fn closed(&mut self, id: &str, key: &str) {
        self.docs.remove(&(id.to_string(), key.to_string()));
    }

    /// The documents, panel and status of graph `root`, written anew.
    fn refresh(&mut self, root: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        let open: Vec<(String, String)> = self
            .docs
            .keys()
            .filter(|(_, k)| k.split(SEP).next() == Some(root))
            .cloned()
            .collect();
        for (id, key) in open {
            if let Some(e) = self.render(&id, &key, false) {
                out.push(e);
            }
        }
        if self.panel.as_ref().is_some_and(|(r, _)| r == root) || self.current.is_some() {
            out.extend(self.panel_effects());
        }
        out.push(self.status());
        out
    }

    /// Document `id` of `key` written, as an effect; kept for Enter.
    fn render(&mut self, id: &str, key: &str, show: bool) -> Option<Effect> {
        let mut parts = key.split(SEP);
        let root = parts.next()?;
        let page = parts.next();
        let index = self.indexes.get(root)?;
        let g = self.glyphs();
        let name = index.graph.name().to_string();
        let (title, kind, content) = match id {
            "graph.backlinks" => {
                let page = page?;
                match page.strip_prefix('#') {
                    Some(tag) if index.graph.kind == Kind::Obsidian => (
                        format!("Tag: #{tag}"),
                        "graph-backlinks",
                        views::tag(index, tag, g),
                    ),
                    _ => {
                        let title = index
                            .page(page)
                            .map_or(page.to_string(), |p| p.title.clone());
                        (
                            format!("Backlinks: {title}"),
                            "graph-backlinks",
                            views::backlinks(index, page, g),
                        )
                    }
                }
            }
            "graph.pages" => (
                format!("All pages: {name}"),
                "graph-pages",
                views::pages(index),
            ),
            "graph.journals" => (
                format!("Journals: {name}"),
                "graph-journals",
                views::journals(index, self.settings.journals_shown, g),
            ),
            "graph.tags" => (format!("Tags: {name}"), "graph-tags", views::tags(index)),
            "graph.tasks" => (
                format!("Tasks: {name}"),
                "graph-tasks",
                views::tasks(index, self.today),
            ),
            "graph.graph" => (format!("Graph: {name}"), "graph-graph", views::graph(index)),
            _ => return None,
        };
        self.docs
            .insert((id.to_string(), key.to_string()), content.clone());
        Some(Effect::Document {
            id: id.to_string(),
            key: key.to_string(),
            title,
            kind: kind.to_string(),
            content,
            show,
        })
    }

    /// The graph a command acts on: the document's, the plugin document's,
    /// else the one used last, else the only one known.
    fn root_for(&mut self, files: &dyn Files, ctx: &Ctx, out: &mut Vec<Effect>) -> Option<String> {
        if let Some((_, key)) = &ctx.doc {
            return key.split(SEP).next().map(str::to_string);
        }
        if let Some(p) = &ctx.path
            && let Some(r) = self.graph_of(files, p, out)
        {
            self.last = Some(r.clone());
            return Some(r);
        }
        if let Some(r) = &self.last {
            return Some(r.clone());
        }
        if self.indexes.len() == 1 {
            return self.indexes.keys().next().cloned();
        }
        out.push(Effect::Notify(
            "Open a note of a Logseq graph or an Obsidian vault first, or name its folder in the plugin's setting graphs".into(),
            Level::Warning,
        ));
        None
    }

    /// Runs command `id` in `ctx`.
    pub fn command(&mut self, files: &dyn Files, id: &str, ctx: &Ctx) -> Vec<Effect> {
        let mut out = Vec::new();
        if let Some(p) = &ctx.path {
            self.current = Some(files::normalize(p));
        }
        if id == "graph.insertText" {
            if let Some(t) = serde_json::from_str::<serde_json::Value>(&ctx.args)
                .ok()
                .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(str::to_string))
            {
                out.push(Effect::Insert(t));
            }
            return out;
        }
        if id == "graph.open" || (id == "graph.follow" && ctx.doc.is_some()) {
            return self.open_line(ctx);
        }
        let Some(root) = self.root_for(files, ctx, &mut out) else {
            return out;
        };
        let rel = ctx
            .path
            .as_deref()
            .and_then(|p| files::relative(&root, &files::normalize(p)).map(str::to_string));
        match id {
            "graph.backlinks" => {
                out.extend(self.panel_effects());
                out.push(Effect::ShowPanel);
            }
            "graph.backlinksDocument" => match ctx.path.as_deref().and_then(|p| self.page_of(p)) {
                Some((root, key)) => {
                    out.extend(self.render("graph.backlinks", &format!("{root}{SEP}{key}"), true));
                }
                None => out.push(Effect::Notify(
                    "This document is no page of a graph".into(),
                    Level::Info,
                )),
            },
            "graph.pages" | "graph.journals" | "graph.tags" | "graph.graph" => {
                out.extend(self.render(id, &root, true));
            }
            "graph.tasks" => {
                out.extend(self.render(id, &root, true));
            }
            "graph.reindex" => {
                let kind = self.indexes[&root].graph.kind;
                self.load(files, &root, kind, &mut out);
                let (pages, journals, blocks, refs) = self.indexes[&root].counts();
                out.push(Effect::Notify(
                    format!("{}: {pages} pages, {journals} of them journals, {blocks} blocks, {refs} references", self.indexes[&root].graph.name()),
                    Level::Info,
                ));
                out.extend(self.refresh(&root));
            }
            "graph.follow" => out.extend(self.follow(files, &root, rel.as_deref(), ctx)),
            "graph.today" | "graph.yesterday" | "graph.tomorrow" => {
                let offset = match id {
                    "graph.yesterday" => -1,
                    "graph.tomorrow" => 1,
                    _ => 0,
                };
                out.extend(self.with_today(files, Then::Journal { root, offset }));
            }
            "graph.journalOn" => {
                let t = self.token(Pending::JournalOn { root });
                out.push(Effect::Prompt {
                    token: t,
                    title: "The journal of".into(),
                    value: None,
                    placeholder: Some("2026-10-03, oct 3, yesterday, -3, monday".into()),
                });
            }
            "graph.nextJournal" | "graph.previousJournal" => {
                let index = &self.indexes[&root];
                let here = rel
                    .as_deref()
                    .and_then(|r| index.page_of(r))
                    .and_then(|p| p.journal);
                let journals = index.journals();
                let found = match (id, here.or(self.today)) {
                    ("graph.nextJournal", Some(d)) => journals.iter().find(|(x, _)| *x > d),
                    (_, Some(d)) => journals.iter().rev().find(|(x, _)| *x < d),
                    (_, None) => journals.last(),
                };
                match found.and_then(|(_, p)| p.path.clone()) {
                    Some(p) => out.push(Effect::Open {
                        path: index.graph.path(&p),
                        line: 1,
                    }),
                    None => out.push(Effect::Notify("No journal further".into(), Level::Info)),
                }
            }
            "graph.findPage" => {
                let index = &self.indexes[&root];
                let mut pages: Vec<_> = index.pages().collect();
                pages.sort_by_key(|p| {
                    (
                        p.path.is_none(),
                        p.journal.is_some(),
                        std::cmp::Reverse(p.journal),
                        p.title.to_lowercase(),
                    )
                });
                let mut items = vec![("Create a page…".to_string(), None)];
                let mut keys = vec![None];
                for p in pages {
                    let detail = match (&p.path, p.journal) {
                        (None, _) => {
                            format!("not written yet · {} links", index.backlinks(&p.key).len())
                        }
                        (Some(_), Some(_)) => "journal".into(),
                        (Some(path), None) if p.aliases.is_empty() => path.clone(),
                        (Some(path), None) => format!("{path} · also {}", p.aliases.join(", ")),
                    };
                    items.push((p.title.clone(), Some(detail)));
                    keys.push(Some(p.key.clone()));
                }
                let t = self.token(Pending::FindPage {
                    root,
                    keys,
                    from: rel,
                });
                out.push(Effect::Pick {
                    token: t,
                    title: "Find a page".into(),
                    items,
                });
            }
            "graph.insertLink" => {
                let index = &self.indexes[&root];
                let mut pages: Vec<_> = index.pages().filter(|p| p.path.is_some()).collect();
                pages.sort_by_key(|p| {
                    (
                        p.journal.is_some(),
                        std::cmp::Reverse(p.journal),
                        p.title.to_lowercase(),
                    )
                });
                let items = pages
                    .iter()
                    .map(|p| {
                        (
                            p.title.clone(),
                            (!p.aliases.is_empty())
                                .then(|| format!("also {}", p.aliases.join(", "))),
                        )
                    })
                    .collect();
                let keys = pages.iter().map(|p| p.key.clone()).collect();
                let t = self.token(Pending::InsertLink {
                    root,
                    keys,
                    from: rel,
                });
                out.push(Effect::Pick {
                    token: t,
                    title: "Insert a link to".into(),
                    items,
                });
            }
            "graph.insertBlockRef" => {
                let index = &self.indexes[&root];
                let mut blocks: Vec<(String, String, String)> = index
                    .blocks_with_ids()
                    .map(|(id, rel, b)| {
                        let title = index
                            .page_of(rel)
                            .map_or(rel.to_string(), |p| p.title.clone());
                        (id.to_string(), b.text.clone(), title)
                    })
                    .collect();
                blocks.sort_by(|a, b| (a.2.to_lowercase(), &a.1).cmp(&(b.2.to_lowercase(), &b.1)));
                if blocks.is_empty() {
                    out.push(Effect::Notify(
                        "No block has an id yet: a block gets one when it is first referenced, which this version of the plugin does not write yet".into(),
                        Level::Info,
                    ));
                    return out;
                }
                let items = blocks
                    .iter()
                    .map(|(_, text, title)| (text.clone(), Some(title.clone())))
                    .collect();
                let ids = blocks.into_iter().map(|(id, ..)| id).collect();
                let t = self.token(Pending::InsertBlockRef { root, ids });
                out.push(Effect::Pick {
                    token: t,
                    title: "Insert a reference to the block".into(),
                    items,
                });
            }
            _ => {}
        }
        out
    }

    /// Asks today's date once, then does `then`.
    fn with_today(&mut self, files: &dyn Files, then: Then) -> Vec<Effect> {
        if let Some(today) = self.today {
            return self.then(files, today, then);
        }
        let Then::Journal { root, .. } = &then;
        let guess = self
            .indexes
            .get(root)
            .and_then(|i| i.journals().last().map(|(d, _)| d.iso()));
        let t = self.token(Pending::Today(then));
        vec![Effect::Prompt {
            token: t,
            title: "Today's date (Kalem gives plugins no clock yet: asked once a session)".into(),
            value: guess,
            placeholder: Some("2026-10-10".into()),
        }]
    }

    fn then(&mut self, files: &dyn Files, today: Date, then: Then) -> Vec<Effect> {
        let Then::Journal { root, offset } = then;
        self.journal(files, &root, today.add_days(offset))
    }

    /// Opens the journal of `date`; a journal not written yet is made from
    /// the graph's template when it names one, else opened empty.
    fn journal(&mut self, files: &dyn Files, root: &str, date: Date) -> Vec<Effect> {
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        if let Some(p) = index.journal(date).and_then(|p| p.path.clone()) {
            return vec![Effect::Open {
                path: index.graph.path(&p),
                line: 1,
            }];
        }
        let rel = index.journal_path(date);
        let title = index.graph.journal_title.format(date);
        let values = template::Values {
            title,
            today: self.today,
            date: Some(date),
        };
        let text = template::journal(index, files, &values);
        self.create(files, root, &rel, text)
    }

    /// Opens the new file `rel` of graph `root`: written first with `text`
    /// when there is one and no file is there; opened empty otherwise, to
    /// be written when the user saves it.
    fn create(
        &mut self,
        files: &dyn Files,
        root: &str,
        rel: &str,
        text: Option<String>,
    ) -> Vec<Effect> {
        let mut out = Vec::new();
        let Some(index) = self.indexes.get(root) else {
            return out;
        };
        let path = index.graph.path(rel);
        if let Some(text) = text
            && files.read(&path).is_err()
        {
            match files.write(&path, &text) {
                Ok(()) => {
                    if let Some(i) = self.indexes.get_mut(root) {
                        i.update(files, &path);
                    }
                    out.extend(self.refresh(root));
                }
                Err(e) => out.push(Effect::Notify(e, Level::Error)),
            }
        }
        out.push(Effect::Open { path, line: 1 });
        out
    }

    /// Opens page `key` of graph `root`: its file, or a new one when the
    /// page is only referenced (a journal from its template).
    fn open_page(
        &mut self,
        files: &dyn Files,
        root: &str,
        key: &str,
        from: Option<&str>,
        anchor: Option<&str>,
    ) -> Vec<Effect> {
        let Some(index) = self.indexes.get(root) else {
            return Vec::new();
        };
        let page = index.page(key).cloned();
        match page {
            Some(p) if p.path.is_some() => {
                let rel = p.path.unwrap_or_default();
                let line = anchor
                    .and_then(|a| {
                        let d = index.file(&rel)?;
                        let a = a.trim().to_lowercase();
                        d.scanned
                            .headings
                            .iter()
                            .find(|h| h.text.to_lowercase() == a)
                            .map(|h| h.line + 1)
                    })
                    .unwrap_or(1);
                vec![Effect::Open {
                    path: index.graph.path(&rel),
                    line,
                }]
            }
            Some(Page {
                journal: Some(day), ..
            }) => self.journal(files, root, day),
            Some(p) => {
                let rel = index.page_path(&p.title, from);
                self.create(files, root, &rel, None)
            }
            None => {
                let rel = index.page_path(key, from);
                self.create(files, root, &rel, None)
            }
        }
    }

    /// Follows the reference under the cursor.
    fn follow(
        &mut self,
        files: &dyn Files,
        root: &str,
        rel: Option<&str>,
        ctx: &Ctx,
    ) -> Vec<Effect> {
        let (Some(text), Some(rel)) = (&ctx.text, rel) else {
            return vec![Effect::Notify(
                "No reference at the cursor".into(),
                Level::Info,
            )];
        };
        let cursor = ctx.cursor.min(text.len());
        let start = text[..cursor].rfind('\n').map_or(0, |p| p + 1);
        let end = text[cursor..].find('\n').map_or(text.len(), |p| cursor + p);
        let line = &text[start..end];
        let index = &self.indexes[root];
        let flavor = index.file(rel).map(|d| d.flavor).unwrap_or(
            match (index.graph.kind, files::stem_ext(rel).1.as_str()) {
                (Kind::Obsidian, _) => Flavor::Obsidian,
                (_, "org") => Flavor::LogseqOrg,
                _ => Flavor::LogseqMarkdown,
            },
        );
        let Some(r) = scan::reference_at(line, cursor - start, flavor) else {
            return vec![Effect::Notify(
                "No reference at the cursor".into(),
                Level::Info,
            )];
        };
        let kind = index.graph.kind;
        match (kind, r.kind) {
            (Kind::Logseq, RefKind::Block | RefKind::EmbedBlock) => match index.block(&r.target) {
                Some((file, b)) => vec![Effect::Open {
                    path: index.graph.path(file),
                    line: b.line + 1,
                }],
                None => vec![Effect::Notify(
                    format!("No block has the id {}", r.target),
                    Level::Warning,
                )],
            },
            (Kind::Obsidian, RefKind::Tag) => {
                let key = format!("{root}{SEP}#{}", r.target.to_lowercase());
                self.render("graph.backlinks", &key, true)
                    .into_iter()
                    .collect()
            }
            (Kind::Obsidian, RefKind::Block | RefKind::EmbedBlock) => {
                let key = index.resolve(&r.target, rel);
                let id = format!(
                    "{key}#^{}",
                    r.anchor.as_deref().unwrap_or("").to_lowercase()
                );
                match index.block(&id) {
                    Some((file, b)) => vec![Effect::Open {
                        path: index.graph.path(file),
                        line: b.line + 1,
                    }],
                    None => self.open_page(files, root, &key, Some(rel), None),
                }
            }
            _ => {
                let key = index.resolve(&r.target, rel);
                let anchor = r.anchor.clone().filter(|a| a != "file");
                self.open_page(files, root, &key, Some(rel), anchor.as_deref())
            }
        }
    }

    /// Enter on a line of the plugin's document.
    fn open_line(&mut self, ctx: &Ctx) -> Vec<Effect> {
        let Some((id, key)) = &ctx.doc else {
            return Vec::new();
        };
        let Some(content) = self.docs.get(&(id.clone(), key.clone())) else {
            return Vec::new();
        };
        let n = match &ctx.text {
            Some(t) => t[..ctx.cursor.min(t.len())]
                .bytes()
                .filter(|b| *b == b'\n')
                .count(),
            None => content.line_of(ctx.cursor),
        };
        match content.target(n).cloned() {
            Some(Target::File { path, line }) => vec![Effect::Open {
                path,
                line: line + 1,
            }],
            Some(Target::Page(page)) => {
                let root = key.split(SEP).next().unwrap_or_default().to_string();
                self.render("graph.backlinks", &format!("{root}{SEP}{page}"), true)
                    .into_iter()
                    .collect()
            }
            None => Vec::new(),
        }
    }

    /// A click in the backlinks panel.
    pub fn panel_clicked(&mut self, key: &str) -> Vec<Effect> {
        match self.panel_targets.get(key).cloned() {
            Some(Target::File { path, line }) => vec![Effect::Open {
                path,
                line: line + 1,
            }],
            Some(Target::Page(page)) => match &self.panel {
                Some((root, _)) => {
                    let k = format!("{root}{SEP}{page}");
                    self.render("graph.backlinks", &k, true)
                        .into_iter()
                        .collect()
                }
                None => Vec::new(),
            },
            None => Vec::new(),
        }
    }

    /// The link to page `key` from the file `from`, as the graph writes
    /// links.
    fn link_text(&self, root: &str, key: &str, from: Option<&str>) -> Option<String> {
        let index = self.indexes.get(root)?;
        let p = index.page(key)?;
        let g = &index.graph;
        let from_org = from.is_some_and(|f| files::stem_ext(f).1 == "org");
        Some(match g.kind {
            Kind::Logseq if from_org && g.org_file_links => {
                let target = p
                    .path
                    .as_deref()
                    .map_or(String::new(), |t| relative_path(from.unwrap_or(""), t));
                format!("[[file:{target}][{}]]", p.title)
            }
            Kind::Logseq => format!("[[{}]]", p.title),
            Kind::Obsidian => {
                let rel = p.path.clone().unwrap_or_else(|| format!("{}.md", p.title));
                let no_ext = rel.strip_suffix(".md").unwrap_or(&rel).to_string();
                let path = match g.link_format {
                    LinkFormat::Absolute => no_ext,
                    LinkFormat::Relative => relative_path(from.unwrap_or(""), &no_ext),
                    LinkFormat::Shortest => {
                        let stem = files::stem_ext(&rel).0.to_string();
                        let same_name = index
                            .pages()
                            .filter(|q| {
                                q.path.as_deref().is_some_and(|qp| {
                                    files::stem_ext(qp).0.eq_ignore_ascii_case(&stem)
                                })
                            })
                            .count();
                        if same_name == 1 { stem } else { no_ext }
                    }
                };
                if g.markdown_links {
                    format!("[{}]({}.md)", p.title, path.replace(' ', "%20"))
                } else {
                    format!("[[{path}]]")
                }
            }
        })
    }

    /// The user's answer to question `token`.
    pub fn answer(&mut self, files: &dyn Files, token: u64, answer: Answer) -> Vec<Effect> {
        let Some(p) = self.pending.remove(&token) else {
            return Vec::new();
        };
        let picked = |a: &Answer| match a {
            Answer::Picked(Some(v)) => v.first().map(|i| *i as usize),
            _ => None,
        };
        let text = |a: &Answer| match a {
            Answer::Text(Some(t)) => Some(t.trim().to_string()),
            _ => None,
        };
        match p {
            Pending::Today(then) => {
                let Some(t) = text(&answer) else {
                    return Vec::new();
                };
                match Date::parse_iso(&t) {
                    Some(d) => {
                        self.today = Some(d);
                        self.then(files, d, then)
                    }
                    None => vec![Effect::Notify(
                        format!("{t} is no date: write it as 2026-10-10"),
                        Level::Warning,
                    )],
                }
            }
            Pending::JournalOn { root } => {
                let Some(t) = text(&answer) else {
                    return Vec::new();
                };
                let date = Date::parse_iso(&t)
                    .or_else(|| self.today.and_then(|d| date::parse_input(&t, d)));
                match date {
                    Some(d) => self.journal(files, &root, d),
                    None if self.today.is_none() => vec![Effect::Notify(
                        format!(
                            "{t}: write the date as 2026-10-03; today is not known until SPC n r d t is told it"
                        ),
                        Level::Warning,
                    )],
                    None => vec![Effect::Notify(format!("{t} is no date"), Level::Warning)],
                }
            }
            Pending::FindPage { root, keys, from } => {
                match picked(&answer).and_then(|i| keys.get(i).cloned()) {
                    Some(Some(key)) => self.open_page(files, &root, &key, from.as_deref(), None),
                    Some(None) => {
                        let t = self.token(Pending::NewPage { root, from });
                        vec![Effect::Prompt {
                            token: t,
                            title: "The new page's title".into(),
                            value: None,
                            placeholder: None,
                        }]
                    }
                    None => Vec::new(),
                }
            }
            Pending::NewPage { root, from } => {
                let Some(title) = text(&answer).filter(|t| !t.is_empty()) else {
                    return Vec::new();
                };
                let Some(index) = self.indexes.get(&root) else {
                    return Vec::new();
                };
                let key = index.resolve(&title, from.as_deref().unwrap_or(""));
                if index.page(&key).is_some_and(|p| p.path.is_some()) {
                    return self.open_page(files, &root, &key, from.as_deref(), None);
                }
                let rel = index.page_path(&title, from.as_deref());
                self.create(files, &root, &rel, None)
            }
            Pending::InsertLink { root, keys, from } => {
                let Some(key) = picked(&answer).and_then(|i| keys.get(i).cloned()) else {
                    return Vec::new();
                };
                match self.link_text(&root, &key, from.as_deref()) {
                    Some(text) => vec![Effect::Run {
                        id: "graph.insertText".into(),
                        args: serde_json::json!({ "text": text }).to_string(),
                    }],
                    None => Vec::new(),
                }
            }
            Pending::InsertBlockRef { root, ids } => {
                let Some(id) = picked(&answer).and_then(|i| ids.get(i).cloned()) else {
                    return Vec::new();
                };
                let Some(index) = self.indexes.get(&root) else {
                    return Vec::new();
                };
                let text = match index.graph.kind {
                    Kind::Logseq => format!("(({id}))"),
                    Kind::Obsidian => {
                        let (key, block) = id.split_once("#^").unwrap_or((&id, ""));
                        let name = index
                            .page(key)
                            .and_then(|p| p.path.as_deref())
                            .map_or(key.to_string(), |p| files::stem_ext(p).0.to_string());
                        format!("[[{name}#^{block}]]")
                    }
                };
                vec![Effect::Run {
                    id: "graph.insertText".into(),
                    args: serde_json::json!({ "text": text }).to_string(),
                }]
            }
        }
    }
}

/// The path of `to` relative to the folder of `from`, both relative to
/// the root.
fn relative_path(from: &str, to: &str) -> String {
    let from_dir: Vec<&str> = files::parent(from)
        .filter(|d| *d != "/")
        .map(|d| d.split('/').filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let to_parts: Vec<&str> = to.split('/').collect();
    let common = from_dir
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<String> = vec!["..".to_string(); from_dir.len() - common];
    out.extend(to_parts[common..].iter().map(|s| s.to_string()));
    let joined = out.join("/");
    if joined.starts_with("..") {
        joined
    } else {
        format!("./{joined}")
    }
}

#[cfg(test)]
mod tests;
