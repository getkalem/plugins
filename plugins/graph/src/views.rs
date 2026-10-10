//! The documents the plugin writes (backlinks, all pages, journals, tags,
//! tasks, the graph) and the backlinks panel's tree, from the index.

use std::collections::BTreeMap;

use crate::config::Kind;
use crate::content::{Content, Style, Target};
use crate::date::Date;
use crate::index::{BackRef, Index};
use crate::scan::{self, RefKind};

/// The longest first line a document shows.
const WIDTH: usize = 120;

/// `text` cut to `WIDTH` characters.
fn cut(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= WIDTH {
        return t.to_string();
    }
    let mut s: String = t.chars().take(WIDTH - 1).collect();
    s.push('…');
    s
}

/// The first line of block `block` of file `rel`, for a list.
pub fn block_text(index: &Index, rel: &str, block: Option<usize>, line: u32) -> String {
    let Some(d) = index.file(rel) else {
        return String::new();
    };
    let b = match block {
        Some(i) => d.scanned.blocks.get(i),
        None => d
            .scanned
            .blocks
            .iter()
            .find(|b| b.line <= line && line < b.end),
    };
    match b {
        Some(b) => {
            let mut t = String::new();
            if let Some(m) = &b.marker {
                t.push_str(m);
                t.push(' ');
            }
            t.push_str(&b.text);
            if t.trim().is_empty() {
                "(the page's properties)".into()
            } else {
                cut(&t)
            }
        }
        None => "(the page's properties)".into(),
    }
}

/// The file name of a path a property names (`../assets/paper.pdf`).
fn files_name(path: &str) -> String {
    path.trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// The title of the page of file `rel`.
fn title_of(index: &Index, rel: &str) -> String {
    index
        .page_of(rel)
        .map_or_else(|| rel.to_string(), |p| p.title.clone())
}

/// References grouped by their file: journals newest first, then the
/// other pages by title.
fn grouped<'a>(index: &Index, refs: &'a [BackRef]) -> Vec<(String, Vec<&'a BackRef>)> {
    let mut by: BTreeMap<String, Vec<&BackRef>> = BTreeMap::new();
    for r in refs {
        by.entry(r.path.clone()).or_default().push(r);
    }
    let mut groups: Vec<(String, Vec<&BackRef>)> = by.into_iter().collect();
    groups.sort_by_key(|(rel, _)| {
        let page = index.page_of(rel);
        let journal = page.and_then(|p| p.journal);
        (
            journal.is_none(),
            std::cmp::Reverse(journal.map(Date::to_days).unwrap_or(0)),
            title_of(index, rel).to_lowercase(),
        )
    });
    for (_, refs) in &mut groups {
        refs.sort_by_key(|r| r.line);
        // A block that names the page twice is listed once.
        refs.dedup_by_key(|r| (r.block, r.line));
    }
    groups
}

/// The fold marks: `▾` and `▸`, or `v` and `>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    /// Unfolded.
    pub open: &'static str,
    /// A list item.
    pub bullet: &'static str,
}

impl Glyphs {
    /// `unicode` or `ascii`.
    pub fn new(setting: &str) -> Glyphs {
        if setting == "ascii" {
            Glyphs {
                open: "v",
                bullet: "*",
            }
        } else {
            Glyphs {
                open: "▾",
                bullet: "•",
            }
        }
    }
}

fn section(c: &mut Content, index: &Index, title: &str, refs: &[BackRef], g: Glyphs) {
    let groups = grouped(index, refs);
    let n: usize = groups.iter().map(|(_, r)| r.len()).sum();
    c.line(&[(&format!("{title} ({n})"), Style::Heading)], None);
    for (rel, refs) in groups {
        let path = index.graph.path(&rel);
        c.line(
            &[
                (g.open, Style::Muted),
                (" ", Style::Normal),
                (&title_of(index, &rel), Style::Link),
                (&format!(" ({})", refs.len()), Style::Muted),
            ],
            Some(Target::File {
                path: path.clone(),
                line: 0,
            }),
        );
        for r in refs {
            let text = block_text(index, &rel, r.block, r.line);
            c.line(
                &[
                    ("    ", Style::Normal),
                    (g.bullet, Style::Muted),
                    (" ", Style::Normal),
                    (&text, Style::Normal),
                ],
                Some(Target::File {
                    path: path.clone(),
                    line: r.line,
                }),
            );
        }
    }
    c.blank();
}

/// "Backlinks: PAGE": what links to the page `key`, what names it without
/// a link, and what refers to its blocks.
pub fn backlinks(index: &Index, key: &str, g: Glyphs) -> Content {
    let mut c = Content::new();
    let Some(page) = index.page(key) else {
        c.line(&[("No such page.", Style::Error)], None);
        return c;
    };
    c.line(
        &[
            ("Backlinks: ", Style::Heading),
            (&page.title, Style::Heading),
        ],
        None,
    );
    let where_ = match &page.path {
        Some(p) => p.clone(),
        None => "not written yet".into(),
    };
    c.line(
        &[(
            &format!(
                "{} · {} · {where_}",
                index.graph.name(),
                index.graph.kind.title()
            ),
            Style::Muted,
        )],
        page.path.as_ref().map(|p| Target::File {
            path: index.graph.path(p),
            line: 0,
        }),
    );
    c.blank();
    section(&mut c, index, "Linked references", index.backlinks(key), g);
    // References to the page's blocks.
    if let Some(rel) = &page.path
        && let Some(d) = index.file(rel)
    {
        let mut refs = Vec::new();
        for b in &d.scanned.blocks {
            if let Some(id) = &b.id {
                let id = match index.graph.kind {
                    Kind::Logseq => id.clone(),
                    Kind::Obsidian => format!("{key}#^{id}"),
                };
                refs.extend(index.block_backlinks(&id).iter().cloned());
            }
        }
        if !refs.is_empty() {
            section(&mut c, index, "Its blocks referenced", &refs, g);
        }
    }
    section(
        &mut c,
        index,
        "Unlinked references",
        &index.unlinked(key),
        g,
    );
    c
}

/// "Tag: NAME": where a tag is used (Obsidian's tags are not pages).
pub fn tag(index: &Index, key: &str, g: Glyphs) -> Content {
    let mut c = Content::new();
    let (name, refs) = index
        .tags()
        .get(key)
        .map_or((key.to_string(), &[][..]), |(n, r)| {
            (n.clone(), r.as_slice())
        });
    c.line(&[("Tag: #", Style::Heading), (&name, Style::Heading)], None);
    c.blank();
    section(&mut c, index, "Used in", refs, g);
    c
}

/// "All pages": every page with its links and blocks, then the pages only
/// referenced.
pub fn pages(index: &Index) -> Content {
    let mut c = Content::new();
    let (files, journals, blocks, _) = index.counts();
    c.line(
        &[
            ("All pages: ", Style::Heading),
            (index.graph.name(), Style::Heading),
        ],
        None,
    );
    c.line(
        &[(
            &format!(
                "{} · {} pages, {journals} of them journals · {blocks} blocks",
                index.graph.kind.title(),
                files
            ),
            Style::Muted,
        )],
        None,
    );
    c.blank();
    let drawn = |p: &crate::index::Page| {
        p.path
            .as_deref()
            .is_some_and(|r| crate::index::drawn_elsewhere(&index.graph, r))
    };
    let mut written: Vec<_> = index
        .pages()
        .filter(|p| p.path.is_some() && p.journal.is_none() && !drawn(p))
        .collect();
    written.sort_by_key(|p| p.title.to_lowercase());
    let width = written
        .iter()
        .map(|p| p.title.chars().count())
        .max()
        .unwrap_or(0)
        .min(48);
    c.line(
        &[(&format!("Pages ({})", written.len()), Style::Heading)],
        None,
    );
    for p in &written {
        let links = index.backlinks(&p.key).len();
        let n = p
            .path
            .as_ref()
            .and_then(|r| index.file(r))
            .map_or(0, |d| d.scanned.blocks.len());
        let pad = width.saturating_sub(p.title.chars().count());
        // Logseq's PDF highlights: a page per PDF, its blocks the
        // highlights (drawn by Logseq over the PDF).
        let pdf = p
            .title
            .starts_with("hls__")
            .then(|| {
                p.props
                    .iter()
                    .find(|(k, _)| k == "file-path")
                    .map(|(_, v)| files_name(v))
            })
            .flatten()
            .map_or(String::new(), |f| format!("  highlights of {f}"));
        c.line(
            &[
                (&p.title, Style::Link),
                (&" ".repeat(pad + 2), Style::Normal),
                (
                    &format!("{links:>4} links  {n:>5} blocks{pdf}"),
                    Style::Muted,
                ),
            ],
            p.path.as_ref().map(|r| Target::File {
                path: index.graph.path(r),
                line: 0,
            }),
        );
    }
    c.blank();
    let mut drawings: Vec<_> = index.pages().filter(|p| drawn(p)).collect();
    if !drawings.is_empty() {
        drawings.sort_by_key(|p| p.title.to_lowercase());
        let (what, app) = match index.graph.kind {
            Kind::Logseq => ("Whiteboards", "Logseq"),
            Kind::Obsidian => ("Canvases", "Obsidian"),
        };
        c.line(
            &[
                (&format!("{what} ({})", drawings.len()), Style::Heading),
                (
                    &format!("  drawn by {app}: Enter shows the file, to open it there"),
                    Style::Muted,
                ),
            ],
            None,
        );
        for p in drawings {
            let links = index.backlinks(&p.key).len();
            c.line(
                &[
                    (&p.title, Style::Link),
                    (&format!("  {links} links · opens in {app}"), Style::Muted),
                ],
                p.path.as_ref().map(|r| Target::File {
                    path: index.graph.path(r),
                    line: 0,
                }),
            );
        }
        c.blank();
    }
    let mut unwritten: Vec<_> = index.pages().filter(|p| p.path.is_none()).collect();
    unwritten.sort_by_key(|p| p.title.to_lowercase());
    c.line(
        &[(
            &format!("Not written yet ({})", unwritten.len()),
            Style::Heading,
        )],
        None,
    );
    for p in unwritten {
        let links = index.backlinks(&p.key).len();
        c.line(
            &[
                (&p.title, Style::Link),
                (&format!("  {links} links"), Style::Muted),
            ],
            Some(Target::Page(p.key.clone())),
        );
    }
    c
}

/// "Journals": the last `count` journals, newest first, with their
/// blocks.
pub fn journals(index: &Index, count: usize, g: Glyphs) -> Content {
    let mut c = Content::new();
    c.line(
        &[
            ("Journals: ", Style::Heading),
            (index.graph.name(), Style::Heading),
        ],
        None,
    );
    c.blank();
    let all = index.journals();
    for (_, page) in all.iter().rev().take(count) {
        let Some(rel) = &page.path else { continue };
        let path = index.graph.path(rel);
        c.line(
            &[
                (g.open, Style::Muted),
                (" ", Style::Normal),
                (&page.title, Style::Strong),
            ],
            Some(Target::File {
                path: path.clone(),
                line: 0,
            }),
        );
        if let Some(d) = index.file(rel) {
            for b in &d.scanned.blocks {
                let mut parts: Vec<(String, Style)> = vec![
                    ("  ".repeat(usize::from(b.depth) + 2), Style::Normal),
                    (g.bullet.to_string(), Style::Muted),
                    (" ".into(), Style::Normal),
                ];
                if let Some(m) = &b.marker {
                    let style = if scan::is_closed(m) {
                        Style::Done
                    } else {
                        Style::Todo
                    };
                    parts.push((format!("{m} "), style));
                }
                parts.push((cut(&b.text), Style::Normal));
                let parts: Vec<(&str, Style)> =
                    parts.iter().map(|(s, st)| (s.as_str(), *st)).collect();
                c.line(
                    &parts,
                    Some(Target::File {
                        path: path.clone(),
                        line: b.line,
                    }),
                );
            }
        }
        c.blank();
    }
    if all.len() > count {
        c.line(
            &[(
                &format!("{} earlier journals", all.len() - count),
                Style::Muted,
            )],
            None,
        );
    }
    if all.is_empty() {
        c.line(
            &[("No journal yet: SPC n r d t writes today's.", Style::Muted)],
            None,
        );
    }
    c
}

/// "Tags": every tag with how often it is used.
pub fn tags(index: &Index) -> Content {
    let mut c = Content::new();
    c.line(
        &[
            ("Tags: ", Style::Heading),
            (index.graph.name(), Style::Heading),
        ],
        None,
    );
    c.blank();
    let mut tags: Vec<_> = index.tags().iter().collect();
    tags.sort_by_key(|(k, (_, refs))| (std::cmp::Reverse(refs.len()), (*k).clone()));
    for (key, (name, refs)) in tags {
        let target = match index.graph.kind {
            Kind::Logseq => Target::Page(key.clone()),
            Kind::Obsidian => Target::Page(format!("#{key}")),
        };
        c.line(
            &[
                ("#", Style::Tag),
                (name, Style::Tag),
                (&format!("  {}", refs.len()), Style::Muted),
            ],
            Some(target),
        );
    }
    if index.tags().is_empty() {
        c.line(&[("No tags.", Style::Muted)], None);
    }
    c
}

/// The order of the open keywords in the tasks list.
fn rank(marker: &str) -> Option<u8> {
    match marker {
        "NOW" | "DOING" | "IN-PROGRESS" | "STARTED" => Some(0),
        "LATER" | "TODO" => Some(1),
        "WAITING" | "WAIT" => Some(2),
        _ => None,
    }
}

/// "Tasks": what is being done, what is to do by priority, what waits,
/// and, when the day is known, what is scheduled or due within a week.
pub fn tasks(index: &Index, today: Option<Date>) -> Content {
    let mut c = Content::new();
    c.line(
        &[
            ("Tasks: ", Style::Heading),
            (index.graph.name(), Style::Heading),
        ],
        None,
    );
    c.blank();
    let mut open: Vec<_> = index
        .tasks()
        .into_iter()
        .filter_map(|t| rank(&t.marker).map(|r| (r, t)))
        .collect();
    let block = |t: &crate::index::Task| {
        index
            .file(&t.path)
            .and_then(|d| d.scanned.blocks.get(t.block))
            .cloned()
    };
    open.sort_by_key(|(r, t)| {
        let b = block(t);
        (
            *r,
            b.as_ref().and_then(|b| b.priority).unwrap_or('Z'),
            title_of(index, &t.path).to_lowercase(),
            b.map_or(0, |b| b.line),
        )
    });
    let line = |c: &mut Content, t: &crate::index::Task, extra: &str| {
        let Some(b) = block(t) else { return };
        let mut parts: Vec<(String, Style)> = vec![
            ("  ".into(), Style::Normal),
            (t.marker.clone(), Style::Todo),
            (" ".into(), Style::Normal),
        ];
        if let Some(p) = b.priority {
            parts.push((format!("[#{p}] "), Style::Strong));
        }
        parts.push((cut(&b.text), Style::Normal));
        parts.push((
            format!("  {extra}{}", title_of(index, &t.path)),
            Style::Muted,
        ));
        let parts: Vec<(&str, Style)> = parts.iter().map(|(s, st)| (s.as_str(), *st)).collect();
        c.line(
            &parts,
            Some(Target::Block {
                path: index.graph.path(&t.path),
                line: b.line,
            }),
        );
    };
    for (r, title) in [(0, "Now"), (1, "To do"), (2, "Waiting")] {
        let these: Vec<_> = open.iter().filter(|(x, _)| *x == r).collect();
        if these.is_empty() {
            continue;
        }
        c.line(
            &[(&format!("{title} ({})", these.len()), Style::Heading)],
            None,
        );
        for (_, t) in these {
            line(&mut c, t, "");
        }
        c.blank();
    }
    match today {
        Some(today) => {
            let mut dated: Vec<(Date, &str, &crate::index::Task)> = Vec::new();
            for (_, t) in &open {
                if let Some(b) = block(t) {
                    if let Some(d) = b.scheduled {
                        dated.push((d, "scheduled", t));
                    }
                    if let Some(d) = b.deadline {
                        dated.push((d, "deadline", t));
                    }
                }
            }
            dated.retain(|(d, _, _)| d.to_days() <= today.add_days(7).to_days());
            dated.sort_by_key(|(d, _, _)| *d);
            c.line(
                &[(
                    &format!("Scheduled and due within a week ({})", dated.len()),
                    Style::Heading,
                )],
                None,
            );
            for (d, what, t) in dated {
                let late = if d < today { "late, " } else { "" };
                line(&mut c, t, &format!("{late}{what} {} · ", d.iso()));
            }
        }
        None => c.line(
            &[(
                "The dates of tasks need today's date: SPC n r d t asks for it once.",
                Style::Muted,
            )],
            None,
        ),
    }
    if open.is_empty() {
        c.line(&[("No open task.", Style::Muted)], None);
    }
    c
}

/// "Graph": the pages as a tree of namespaces, each with the links in and
/// out, then the pages nothing links to and that link nowhere.
pub fn graph(index: &Index) -> Content {
    let mut c = Content::new();
    c.line(
        &[
            ("Graph: ", Style::Heading),
            (index.graph.name(), Style::Heading),
        ],
        None,
    );
    c.line(&[("← links in · → links out", Style::Muted)], None);
    c.blank();
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for (rel, d) in index.files() {
        let n = d
            .scanned
            .refs
            .iter()
            .filter(|r| {
                !r.kind.to_block()
                    && !(index.graph.kind == Kind::Obsidian && r.kind == RefKind::Tag)
            })
            .filter(|r| index.resolve(&r.target, rel) != d.key)
            .count();
        out.insert(d.key.clone(), n);
    }
    let mut pages: Vec<_> = index
        .pages()
        .filter(|p| p.path.is_some() && p.journal.is_none())
        .collect();
    // A namespace's pages under it: sorted by their full title, indented
    // by its depth.
    let sort_key = |t: &str| match index.graph.kind {
        Kind::Logseq => t.to_lowercase(),
        Kind::Obsidian => t.to_lowercase(),
    };
    pages.sort_by_key(|p| match index.graph.kind {
        Kind::Logseq => sort_key(&p.title),
        Kind::Obsidian => p.key.clone(),
    });
    let mut orphans = Vec::new();
    for p in &pages {
        let incoming = index.backlinks(&p.key).len();
        let outgoing = out.get(&p.key).copied().unwrap_or(0);
        if incoming == 0 && outgoing == 0 {
            orphans.push(*p);
            continue;
        }
        let shown = match index.graph.kind {
            Kind::Logseq => p.title.clone(),
            Kind::Obsidian => p
                .path
                .clone()
                .unwrap_or_default()
                .trim_end_matches(".md")
                .to_string(),
        };
        let depth = shown.matches('/').count();
        let leaf = shown.rsplit('/').next().unwrap_or(&shown).to_string();
        c.line(
            &[
                (&"  ".repeat(depth), Style::Normal),
                (&leaf, Style::Link),
                (&format!("  ←{incoming} →{outgoing}"), Style::Muted),
            ],
            Some(Target::Page(p.key.clone())),
        );
    }
    c.blank();
    // The pages named without a link: where links are missing.
    let mut unlinked: Vec<(usize, &crate::index::Page)> = index
        .unlinked_counts()
        .into_iter()
        .filter_map(|(k, n)| index.page(&k).map(|p| (n, p)))
        .collect();
    unlinked.sort_by_key(|(n, p)| (std::cmp::Reverse(*n), p.title.to_lowercase()));
    c.line(
        &[
            (
                &format!("Unlinked mentions ({})", unlinked.len()),
                Style::Heading,
            ),
            ("  named without a link; Enter lists them", Style::Muted),
        ],
        None,
    );
    for (n, p) in unlinked {
        c.line(
            &[(&p.title, Style::Link), (&format!("  {n}"), Style::Muted)],
            Some(Target::Page(p.key.clone())),
        );
    }
    c.blank();
    c.line(
        &[(&format!("Orphans ({})", orphans.len()), Style::Heading)],
        None,
    );
    for p in orphans {
        c.line(
            &[(&p.title, Style::Link)],
            p.path.as_ref().map(|r| Target::File {
                path: index.graph.path(r),
                line: 0,
            }),
        );
    }
    c
}

/// An entry of the backlinks panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelNode {
    /// Its key, unique in the panel (Kalem asks so of what is clicked).
    pub key: String,
    /// Where a click goes.
    pub target: Option<Target>,
    /// Its text.
    pub label: String,
    /// A second text, dimmer.
    pub detail: Option<String>,
    /// Whether its children show; none without children.
    pub expanded: Option<bool>,
    /// The entries under it.
    pub children: Vec<PanelNode>,
}

impl PanelNode {
    /// Every key with its target, this node's and those under it.
    pub fn targets(&self, out: &mut Vec<(String, Target)>) {
        if let Some(t) = &self.target {
            out.push((self.key.clone(), t.clone()));
        }
        for c in &self.children {
            c.targets(out);
        }
    }
}

fn panel_section(index: &Index, n: usize, title: &str, refs: &[BackRef]) -> PanelNode {
    let groups = grouped(index, refs);
    let count: usize = groups.iter().map(|(_, r)| r.len()).sum();
    PanelNode {
        key: format!("{n}"),
        target: None,
        label: title.to_string(),
        detail: Some(count.to_string()),
        expanded: Some(true),
        children: groups
            .into_iter()
            .enumerate()
            .map(|(g, (rel, refs))| {
                let path = index.graph.path(&rel);
                PanelNode {
                    key: format!("{n}.{g}"),
                    target: Some(Target::File {
                        path: path.clone(),
                        line: 0,
                    }),
                    label: title_of(index, &rel),
                    detail: Some(refs.len().to_string()),
                    expanded: Some(true),
                    children: refs
                        .into_iter()
                        .enumerate()
                        .map(|(i, r)| PanelNode {
                            key: format!("{n}.{g}.{i}"),
                            target: Some(Target::File {
                                path: path.clone(),
                                line: r.line,
                            }),
                            label: block_text(index, &rel, r.block, r.line),
                            detail: None,
                            expanded: None,
                            children: Vec::new(),
                        })
                        .collect(),
                }
            })
            .collect(),
    }
}

/// The backlinks panel for the page `key`.
pub fn backlinks_panel(index: &Index, key: &str) -> Vec<PanelNode> {
    let Some(page) = index.page(key) else {
        return Vec::new();
    };
    vec![
        PanelNode {
            key: "page".into(),
            target: Some(Target::Page(key.to_string())),
            label: page.title.clone(),
            detail: Some(index.graph.kind.title().into()),
            expanded: None,
            children: Vec::new(),
        },
        panel_section(index, 1, "Linked references", index.backlinks(key)),
        panel_section(index, 2, "Unlinked references", &index.unlinked(key)),
    ]
}

/// The value of column `column` of a query's table for `hit`: `block`
/// (its first line), `page` (its page's title), a property of the block,
/// else of its page.
fn column_value(index: &Index, hit: &crate::query::Hit, column: &str) -> String {
    use crate::query::Hit;
    let (page, block) = match hit {
        Hit::Page(k) => (index.page(k), None),
        Hit::Block { rel, block } => (
            index.page_of(rel),
            index.file(rel).and_then(|d| d.scanned.blocks.get(*block)),
        ),
    };
    match column {
        "block" => crate::query::hit_text(index, hit),
        "page" => page.map_or(String::new(), |p| p.title.clone()),
        "created-at" | "updated-at" | "journal-day" => page
            .and_then(|p| p.journal)
            .map_or(String::new(), Date::iso),
        "priority" => block
            .and_then(|b| b.priority)
            .map_or(String::new(), |p| p.to_string()),
        "scheduled" => block
            .and_then(|b| b.scheduled)
            .map_or(String::new(), Date::iso),
        "deadline" => block
            .and_then(|b| b.deadline)
            .map_or(String::new(), Date::iso),
        other => block
            .and_then(|b| b.props.iter().find(|(k, _)| k == other))
            .or_else(|| page.and_then(|p| p.props.iter().find(|(k, _)| k == other)))
            .map_or(String::new(), |(_, v)| v.clone()),
    }
}

/// The line a hit's Enter opens.
fn hit_target(index: &Index, hit: &crate::query::Hit) -> Option<Target> {
    use crate::query::Hit;
    match hit {
        Hit::Page(k) => index
            .page(k)
            .and_then(|p| p.path.as_ref())
            .map(|rel| Target::File {
                path: index.graph.path(rel),
                line: 0,
            }),
        Hit::Block { rel, block } => index
            .file(rel)
            .and_then(|d| d.scanned.blocks.get(*block))
            .map(|b| Target::Block {
                path: index.graph.path(rel),
                line: b.line,
            }),
    }
}

/// `text` cut to `n` characters and padded to them.
fn cell(text: &str, n: usize) -> String {
    let t: String = if text.chars().count() > n {
        let mut s: String = text.chars().take(n.saturating_sub(1)).collect();
        s.push('…');
        s
    } else {
        text.to_string()
    };
    let pad = n.saturating_sub(t.chars().count());
    format!("{t}{}", " ".repeat(pad))
}

/// "Query: …": what Logseq's simple query `text` finds in the graph,
/// the blocks grouped by page with their first lines, or the pages; a
/// table when `shape` asks for one. `today` for relative days.
pub fn query(
    index: &Index,
    text: &str,
    shape: &crate::query::Shape,
    today: Option<Date>,
    g: Glyphs,
) -> Content {
    use crate::query::{self, Hit};
    let mut c = Content::new();
    c.line(&[("Query: ", Style::Heading), (text, Style::Heading)], None);
    let mut q = match query::parse(text) {
        Ok(q) => q,
        Err(e) => {
            c.line(&[(&format!("Not read: {e}."), Style::Error)], None);
            c.line(
                &[(
                    "Kalem reads Logseq's simple queries: [[page]], #tag, \"text\", (and …), (or …), (not …), (task …), (priority …), (between …), (property …), (page-property …), (page-tags …), (page …), (namespace …), (sort-by …), (sample n).",
                    Style::Muted,
                )],
                None,
            );
            return c;
        }
    };
    if q.sort.is_none() {
        q.sort = shape.sort.clone();
    }
    let a = query::run(&q, index, today);
    let n = a.hits.len();
    let what = match (a.pages, n) {
        (true, 1) => "1 page".to_string(),
        (true, _) => format!("{n} pages"),
        (false, 1) => "1 block".to_string(),
        (false, _) => format!("{n} blocks"),
    };
    c.line(&[(&what, Style::Muted)], None);
    for note in &a.notes {
        c.line(&[(&format!("{note}."), Style::Muted)], None);
    }
    c.blank();
    if n == 0 {
        c.line(&[("No result.", Style::Muted)], None);
        return c;
    }
    if let Some(columns) = &shape.table {
        let mut columns = columns.clone();
        if columns.is_empty() {
            columns = if a.pages {
                let mut cols = vec!["page".to_string()];
                for h in &a.hits {
                    if let Hit::Page(k) = h
                        && let Some(p) = index.page(k)
                    {
                        for (key, _) in &p.props {
                            if key != "title" && !cols.contains(key) {
                                cols.push(key.clone());
                            }
                        }
                    }
                }
                cols
            } else {
                vec!["block".into(), "page".into()]
            };
        }
        let rows: Vec<Vec<String>> = a
            .hits
            .iter()
            .map(|h| {
                columns
                    .iter()
                    .map(|col| column_value(index, h, col))
                    .collect()
            })
            .collect();
        let widths: Vec<usize> = columns
            .iter()
            .enumerate()
            .map(|(i, col)| {
                rows.iter()
                    .map(|r| r[i].chars().count())
                    .chain(std::iter::once(col.chars().count()))
                    .max()
                    .unwrap_or(0)
                    .min(60)
            })
            .collect();
        let sep = if g.bullet == "*" { " | " } else { " │ " };
        let header: Vec<String> = columns
            .iter()
            .zip(&widths)
            .map(|(col, w)| cell(col, *w))
            .collect();
        c.line(&[(header.join(sep).trim_end(), Style::Heading)], None);
        for (h, row) in a.hits.iter().zip(&rows) {
            let cells: Vec<String> = row.iter().zip(&widths).map(|(v, w)| cell(v, *w)).collect();
            c.line(
                &[(cells.join(sep).trim_end(), Style::Normal)],
                hit_target(index, h),
            );
        }
        return c;
    }
    if a.pages {
        for h in &a.hits {
            c.line(
                &[
                    (g.bullet, Style::Muted),
                    (" ", Style::Normal),
                    (&query::hit_text(index, h), Style::Link),
                ],
                hit_target(index, h),
            );
        }
        return c;
    }
    // The blocks grouped by page, in the order found.
    let mut groups: Vec<(String, Vec<&Hit>)> = Vec::new();
    for h in &a.hits {
        let Hit::Block { rel, .. } = h else { continue };
        match groups.iter_mut().find(|(r, _)| r == rel) {
            Some((_, v)) => v.push(h),
            None => groups.push((rel.clone(), vec![h])),
        }
    }
    for (rel, hits) in groups {
        c.line(
            &[
                (g.open, Style::Muted),
                (" ", Style::Normal),
                (&title_of(index, &rel), Style::Link),
                (&format!(" ({})", hits.len()), Style::Muted),
            ],
            Some(Target::File {
                path: index.graph.path(&rel),
                line: 0,
            }),
        );
        for h in hits {
            let t = cut(&query::hit_text(index, h));
            let (marker, rest) = match t.split_once(' ') {
                Some((m, r)) if scan::MARKERS.contains(&m) => (m, r),
                _ => ("", t.as_str()),
            };
            let style = if marker.is_empty() {
                Style::Normal
            } else if scan::is_closed(marker) {
                Style::Done
            } else {
                Style::Todo
            };
            let mut parts = vec![
                ("    ", Style::Normal),
                (g.bullet, Style::Muted),
                (" ", Style::Normal),
            ];
            if !marker.is_empty() {
                parts.push((marker, style));
                parts.push((" ", Style::Normal));
            }
            parts.push((rest, Style::Normal));
            c.line(&parts, hit_target(index, h));
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Fallbacks, Graph};
    use crate::files::Memory;

    fn index() -> Index {
        let m = Memory::new(&[
            ("/g/logseq/config.edn", "{}"),
            ("/g/pages/Kalem.md", "- An editor\n"),
            (
                "/g/pages/Other.md",
                "- uses [[Kalem]]\n- TODO [#A] try [[Kalem]] again\n  SCHEDULED: <2026-10-12 Mon>\n",
            ),
            (
                "/g/journals/2026_10_03.md",
                "- NOW [[Kalem]] all day\n  - nested\n",
            ),
            ("/g/pages/Alone.md", "- nothing here\n"),
            ("/g/pages/Mention.md", "- kalem is good\n"),
        ]);
        Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        )
    }

    #[test]
    fn the_backlinks_document() {
        let i = index();
        let c = backlinks(&i, "kalem", Glyphs::new("unicode"));
        let expected = "Backlinks: Kalem\n\
            g · Logseq · pages/Kalem.md\n\
            \n\
            Linked references (3)\n\
            ▾ Oct 3rd, 2026 (1)\n    • NOW [[Kalem]] all day\n\
            ▾ Other (2)\n    • uses [[Kalem]]\n    • TODO try [[Kalem]] again\n\
            \n\
            Unlinked references (1)\n\
            ▾ Mention (1)\n    • kalem is good\n\n";
        assert_eq!(c.text, expected);
        assert_eq!(
            c.target(5),
            Some(&Target::File {
                path: "/g/journals/2026_10_03.md".into(),
                line: 0
            })
        );
        assert_eq!(
            c.target(8),
            Some(&Target::File {
                path: "/g/pages/Other.md".into(),
                line: 1
            })
        );
        assert_eq!(c.targets.len(), c.text.lines().count());
    }

    #[test]
    fn the_other_documents() {
        let i = index();
        let p = pages(&i);
        assert!(p.text.contains("Pages (4)\n"), "{}", p.text);
        assert!(p.text.contains("Kalem  "), "{}", p.text);
        let j = journals(&i, 30, Glyphs::new("ascii"));
        assert!(
            j.text
                .contains("v Oct 3rd, 2026\n    * NOW [[Kalem]] all day\n      * nested\n"),
            "{}",
            j.text
        );
        let t = tasks(&i, Date::new(2026, 10, 10));
        assert!(
            t.text
                .contains("Now (1)\n  NOW [[Kalem]] all day  Oct 3rd, 2026\n"),
            "{}",
            t.text
        );
        assert!(
            t.text
                .contains("To do (1)\n  TODO [#A] try [[Kalem]] again  Other\n"),
            "{}",
            t.text
        );
        assert!(t.text.contains("Scheduled and due within a week (1)\n  TODO [#A] try [[Kalem]] again  scheduled 2026-10-12 · Other\n"), "{}", t.text);
        let g = graph(&i);
        assert!(g.text.contains("Kalem  ←3 →0"), "{}", g.text);
        assert!(
            g.text.contains("Orphans (2)\nAlone\nMention\n"),
            "{}",
            g.text
        );
        let panel = backlinks_panel(&i, "kalem");
        assert_eq!(panel.len(), 3);
        assert_eq!(panel[1].detail.as_deref(), Some("3"));
        assert_eq!(panel[1].children[0].key, "1.0");
        assert_eq!(
            panel[1].children[1].children[1].target,
            Some(Target::File {
                path: "/g/pages/Other.md".into(),
                line: 1
            })
        );
        let mut keys = Vec::new();
        for n in &panel {
            n.targets(&mut keys);
        }
        let mut unique: Vec<&String> = keys.iter().map(|(k, _)| k).collect();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), keys.len(), "keys are unique");
    }
}
