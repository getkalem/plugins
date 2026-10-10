//! A graph: the folder, whose application keeps it, and how it is laid
//! out, read from Logseq's `logseq/config.edn` or from Obsidian's three
//! settings files (`.obsidian/daily-notes.json`, `app.json`,
//! `templates.json`), read-only (the owner's decision of 2026-10-10).
//! Nothing under `logseq/` or `.obsidian/` is ever written.

use crate::date::{Dialect, Pattern};
use crate::edn::{self, Value};
use crate::files::{self, Files};
use crate::names::FileNameFormat;

/// Whose graph a folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// A Logseq graph: `logseq/config.edn`.
    Logseq,
    /// An Obsidian vault: `.obsidian/`.
    Obsidian,
}

impl Kind {
    /// `logseq`, `obsidian`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Logseq => "logseq",
            Kind::Obsidian => "obsidian",
        }
    }

    /// The application's name.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Logseq => "Logseq",
            Kind::Obsidian => "Obsidian",
        }
    }
}

/// The format new pages are written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Markdown, `.md`.
    Markdown,
    /// Org, `.org`.
    Org,
}

impl Format {
    /// `md`, `org`.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Org => "org",
        }
    }
}

/// Logseq's task keywords: `LATER`/`NOW` or `TODO`/`DOING`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workflow {
    /// `:now`: LATER, NOW, DONE.
    Now,
    /// `:todo`: TODO, DOING, DONE.
    Todo,
}

/// Where Obsidian makes a new note (`app.json`'s `newFileLocation`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewFile {
    /// The vault's root.
    Root,
    /// The current note's folder.
    Current,
    /// A folder, relative to the root.
    Folder(String),
}

/// How Obsidian writes a link's path (`app.json`'s `newLinkFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFormat {
    /// The shortest path that names the note alone.
    Shortest,
    /// Relative to the note the link is in.
    Relative,
    /// From the vault's root.
    Absolute,
}

/// The plugin's settings that stand in for Obsidian's files when they are
/// missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fallbacks {
    /// `obsidian.daily_notes`: the daily notes' folder.
    pub daily_notes: Option<String>,
    /// `obsidian.daily_format`: their names' date format, Moment's.
    pub daily_format: Option<String>,
    /// `obsidian.new_notes`: the folder of new notes.
    pub new_notes: Option<String>,
    /// `obsidian.link_style`: `shortest`, `relative`, `absolute`,
    /// `markdown`.
    pub link_style: Option<String>,
}

/// A graph's folder and layout.
#[derive(Debug, Clone)]
pub struct Graph {
    /// The folder, absolute, without a `/` at its end.
    pub root: String,
    /// Whose it is.
    pub kind: Kind,
    /// The format of new pages.
    pub format: Format,
    /// The pages' folder, relative to the root (`pages`; Obsidian's is
    /// the root, ``).
    pub pages_dir: String,
    /// The journals' folder, relative (`journals`; Obsidian's daily notes'
    /// folder).
    pub journals_dir: String,
    /// A journal's file name.
    pub journal_file: Pattern,
    /// A journal's title, as references name it (Logseq's
    /// `:journal/page-title-format`; Obsidian names a daily note by its
    /// file).
    pub journal_title: Pattern,
    /// How titles are written as file names.
    pub file_names: FileNameFormat,
    /// Folders not read, relative to the root.
    pub hidden: Vec<String>,
    /// The template of a new journal: Logseq's template's name, Obsidian's
    /// template file (relative to the root, without `.md`).
    pub journal_template: Option<String>,
    /// Logseq's task keywords.
    pub workflow: Workflow,
    /// Properties Logseq hides besides its own.
    pub hidden_properties: Vec<String>,
    /// Properties whose values are page references split at commas.
    pub comma_properties: Vec<String>,
    /// Org links to pages written as `[[file:./page.org][Page]]`.
    pub org_file_links: bool,
    /// Logseq clocks a task's time in its `:LOGBOOK:`.
    pub timetracking: bool,
    /// Where a new note goes (Obsidian).
    pub new_file: NewFile,
    /// The attachments' folder (Obsidian).
    pub attachments: String,
    /// Links written as Markdown links, not wiki links (Obsidian).
    pub markdown_links: bool,
    /// How a link's path is written (Obsidian).
    pub link_format: LinkFormat,
    /// The templates' folder (Obsidian's core Templates).
    pub templates_dir: Option<String>,
    /// Logseq's whiteboards' folder (`whiteboards`), relative.
    pub whiteboards_dir: Option<String>,
    /// Never written: the Markdown Mirror of a Logseq database graph,
    /// which Logseq writes over from its database.
    pub read_only: bool,
    /// What could not be read, for one notice.
    pub problems: Vec<String>,
}

/// Whose graph `dir` is, by what it holds: `logseq/config.edn` or
/// `.obsidian/`.
pub fn kind_of(files: &dyn Files, dir: &str) -> Option<Kind> {
    let entries = files.list(dir).ok()?;
    let has = |name: &str| {
        entries
            .iter()
            .any(|e| files::file_name(e) == name && e.ends_with('/'))
    };
    if has("logseq")
        && files
            .list(&files::join(dir, "logseq"))
            .is_ok_and(|l| l.iter().any(|e| files::file_name(e) == "config.edn"))
    {
        return Some(Kind::Logseq);
    }
    if has(".obsidian") {
        return Some(Kind::Obsidian);
    }
    is_mirror(files, dir).then_some(Kind::Logseq)
}

/// Whether `dir` is the Markdown Mirror of a Logseq database graph:
/// `mirror/markdown/` in the graph's folder, with the mirror's
/// `.index.edn` (Logseq's ADR 0016). Its notes are Logseq Markdown, read
/// as a graph's and never written.
pub fn is_mirror(files: &dyn Files, dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    files::file_name(dir) == "markdown"
        && files::parent(dir).is_some_and(|p| files::file_name(p) == "mirror")
        && files.list(dir).is_ok_and(|l| {
            l.iter()
                .any(|e| files::file_name(e) == ".index.edn" && !e.ends_with('/'))
        })
}

/// What a mirror's graph is set as: Logseq's defaults, its file names
/// the pages' titles.
const MIRROR_CONFIG: &str = "{:file/name-format :triple-lowbar}";

/// The graph a file at `path` is in: the nearest folder above it that is
/// one of `known` or holds a graph's marker. The walk stops at a folder
/// that cannot be listed (outside the projects the plugin may read) or at
/// one of `not`, folders known to be no graph's root.
pub fn find_root(
    files: &dyn Files,
    path: &str,
    known: &[(String, Kind)],
    not: &mut std::collections::BTreeSet<String>,
) -> Option<(String, Kind)> {
    let mut dir = files::parent(path)?.to_string();
    loop {
        if let Some(k) = known.iter().find(|(r, _)| *r == dir) {
            return Some(k.clone());
        }
        if !not.contains(&dir) {
            if files.list(&dir).is_err() {
                return None;
            }
            if let Some(kind) = kind_of(files, &dir) {
                return Some((dir, kind));
            }
            not.insert(dir.clone());
        }
        let up = files::parent(&dir)?.to_string();
        if up == dir {
            return None;
        }
        dir = up;
    }
}

/// A folder name from a configuration: relative, no `/` at either end.
fn folder(s: &str) -> String {
    s.trim()
        .trim_matches('/')
        .trim_start_matches("./")
        .to_string()
}

impl Graph {
    /// The graph at `root`, its settings read.
    pub fn load(files: &dyn Files, root: &str, kind: Kind, fallbacks: &Fallbacks) -> Graph {
        let root = files::normalize(root);
        match kind {
            Kind::Logseq if is_mirror(files, &root) => {
                let mut g = Graph::logseq(&root, MIRROR_CONFIG);
                g.read_only = true;
                g
            }
            Kind::Logseq => {
                let path = files::join(&root, "logseq/config.edn");
                let text = files.read(&path);
                let mut g = Graph::logseq(&root, text.as_deref().unwrap_or("{}"));
                if let Err(e) = text {
                    g.problems.push(e);
                }
                g
            }
            Kind::Obsidian => {
                let read = |name: &str| {
                    files
                        .read(&files::join(&root, &format!(".obsidian/{name}")))
                        .ok()
                };
                Graph::obsidian(
                    &root,
                    read("daily-notes.json").as_deref(),
                    read("app.json").as_deref(),
                    read("templates.json").as_deref(),
                    fallbacks,
                )
            }
        }
    }

    /// A Logseq graph with `config` (the text of `logseq/config.edn`).
    pub fn logseq(root: &str, config: &str) -> Graph {
        let mut problems = Vec::new();
        let c = edn::parse(config).unwrap_or_else(|e| {
            problems.push(format!(
                "logseq/config.edn: {e}; Logseq's defaults are used"
            ));
            Value::Map(Vec::new())
        });
        let s = |key: &str| c.get(key).and_then(Value::as_str).map(str::to_string);
        let format = match s("preferred-format").map(|f| f.to_lowercase()).as_deref() {
            Some("org") => Format::Org,
            _ => Format::Markdown,
        };
        let workflow = match s("preferred-workflow").map(|f| f.to_lowercase()).as_deref() {
            Some("todo") => Workflow::Todo,
            _ => Workflow::Now,
        };
        let mut comma: Vec<String> = c
            .get("property/separated-by-commas")
            .map(Value::strings)
            .unwrap_or_default();
        for k in ["alias", "tags"] {
            if !comma.iter().any(|c| c == k) {
                comma.push(k.to_string());
            }
        }
        Graph {
            root: root.to_string(),
            kind: Kind::Logseq,
            format,
            pages_dir: s("pages-directory").map_or_else(|| "pages".into(), |p| folder(&p)),
            journals_dir: s("journals-directory").map_or_else(|| "journals".into(), |p| folder(&p)),
            journal_file: Pattern::new(
                &s("journal/file-name-format").unwrap_or_else(|| "yyyy_MM_dd".into()),
                Dialect::Logseq,
            ),
            journal_title: Pattern::new(
                &s("journal/page-title-format").unwrap_or_else(|| "MMM do, yyyy".into()),
                Dialect::Logseq,
            ),
            file_names: match s("file/name-format").as_deref() {
                Some("triple-lowbar") => FileNameFormat::TripleLowbar,
                _ => FileNameFormat::Legacy,
            },
            hidden: c
                .get("hidden")
                .map(Value::strings)
                .unwrap_or_default()
                .iter()
                .map(|h| folder(h))
                .filter(|h| !h.is_empty())
                .collect(),
            journal_template: c
                .get("default-templates")
                .and_then(|t| t.get("journals"))
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .map(str::to_string),
            workflow,
            hidden_properties: c
                .get("block-hidden-properties")
                .map(Value::strings)
                .unwrap_or_default(),
            comma_properties: comma,
            org_file_links: c
                .get("org-mode/insert-file-link?")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            timetracking: c
                .get("feature/enable-timetracking?")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            new_file: NewFile::Root,
            attachments: "assets".into(),
            markdown_links: false,
            link_format: LinkFormat::Shortest,
            templates_dir: None,
            whiteboards_dir: Some(
                s("whiteboards-directory").map_or_else(|| "whiteboards".into(), |p| folder(&p)),
            ),
            read_only: false,
            problems,
        }
    }

    /// An Obsidian vault with its settings files' texts, when present.
    pub fn obsidian(
        root: &str,
        daily_notes: Option<&str>,
        app: Option<&str>,
        templates: Option<&str>,
        fallbacks: &Fallbacks,
    ) -> Graph {
        let mut problems = Vec::new();
        let mut json = |name: &str, text: Option<&str>| -> serde_json::Value {
            match text.map(serde_json::from_str::<serde_json::Value>) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    problems.push(format!(
                        ".obsidian/{name}: {e}; the plugin's settings are used"
                    ));
                    serde_json::Value::Null
                }
                None => serde_json::Value::Null,
            }
        };
        let daily = json("daily-notes.json", daily_notes);
        let app = json("app.json", app);
        let tpl = json("templates.json", templates);
        let text = |v: &serde_json::Value, k: &str| {
            v.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
        };
        let journals_dir = text(&daily, "folder")
            .or_else(|| fallbacks.daily_notes.clone())
            .map(|f| folder(&f))
            .unwrap_or_default();
        let format = text(&daily, "format")
            .or_else(|| fallbacks.daily_format.clone())
            .unwrap_or_else(|| "YYYY-MM-DD".into());
        let pattern = Pattern::new(&format, Dialect::Moment);
        let style = fallbacks.link_style.as_deref().unwrap_or("");
        let new_file = match text(&app, "newFileLocation").as_deref() {
            Some("current") => NewFile::Current,
            Some("folder") => {
                NewFile::Folder(folder(&text(&app, "newFileFolderPath").unwrap_or_default()))
            }
            Some(_) => NewFile::Root,
            None => match &fallbacks.new_notes {
                Some(f) if !folder(f).is_empty() => NewFile::Folder(folder(f)),
                _ => NewFile::Root,
            },
        };
        Graph {
            root: root.to_string(),
            kind: Kind::Obsidian,
            format: Format::Markdown,
            pages_dir: String::new(),
            journals_dir,
            journal_file: pattern.clone(),
            journal_title: pattern,
            file_names: FileNameFormat::TripleLowbar,
            hidden: vec![".trash".into()],
            journal_template: text(&daily, "template").map(|t| folder(t.trim_end_matches(".md"))),
            workflow: Workflow::Todo,
            hidden_properties: Vec::new(),
            comma_properties: Vec::new(),
            org_file_links: false,
            timetracking: false,
            new_file,
            attachments: text(&app, "attachmentFolderPath").unwrap_or_else(|| "/".into()),
            markdown_links: app
                .get("useMarkdownLinks")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(style == "markdown"),
            link_format: match text(&app, "newLinkFormat").as_deref().unwrap_or(style) {
                "relative" => LinkFormat::Relative,
                "absolute" => LinkFormat::Absolute,
                _ => LinkFormat::Shortest,
            },
            templates_dir: text(&tpl, "folder").map(|f| folder(&f)),
            whiteboards_dir: None,
            read_only: false,
            problems,
        }
    }

    /// The graph's name: its folder's.
    pub fn name(&self) -> &str {
        // A mirror by its database graph's folder, not `markdown`.
        if self.read_only
            && let Some(graph) = files::parent(&self.root).and_then(files::parent)
        {
            return files::file_name(graph);
        }
        files::file_name(&self.root)
    }

    /// The absolute path of `rel`, a path relative to the root.
    pub fn path(&self, rel: &str) -> String {
        if rel.is_empty() {
            self.root.clone()
        } else {
            files::join(&self.root, rel)
        }
    }

    /// Whether `rel`, relative to the root, is in a folder not read.
    pub fn is_hidden(&self, rel: &str) -> bool {
        rel.split('/')
            .any(|part| part.starts_with('.') && !part.is_empty())
            || rel.split('/').next() == Some("logseq") && self.kind == Kind::Logseq
            || self
                .hidden
                .iter()
                .any(|h| rel == h || rel.starts_with(&format!("{h}/")))
    }

    /// The task keywords that are not done, in the workflow's order.
    pub fn todo_cycle(&self) -> [&'static str; 3] {
        match self.workflow {
            Workflow::Now => ["LATER", "NOW", "DONE"],
            Workflow::Todo => ["TODO", "DOING", "DONE"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::Memory;

    #[test]
    fn logseq_defaults_and_settings() {
        let g = Graph::logseq("/g", "{}");
        assert_eq!(g.format, Format::Markdown);
        assert_eq!(g.pages_dir, "pages");
        assert_eq!(g.journals_dir, "journals");
        assert_eq!(g.journal_file.source(), "yyyy_MM_dd");
        assert_eq!(g.journal_title.source(), "MMM do, yyyy");
        assert_eq!(g.file_names, FileNameFormat::Legacy);
        assert_eq!(g.workflow, Workflow::Now);
        assert_eq!(g.comma_properties, ["alias", "tags"]);
        let g = Graph::logseq(
            "/g",
            r#"{:preferred-format :org :preferred-workflow :todo
               :pages-directory "notes/" :journals-directory "/days"
               :file/name-format :triple-lowbar :hidden ["/private" "drafts/"]
               :default-templates {:journals "Daily"}
               :property/separated-by-commas #{:topics}}"#,
        );
        assert_eq!(g.format, Format::Org);
        assert_eq!(g.workflow, Workflow::Todo);
        assert_eq!(g.pages_dir, "notes");
        assert_eq!(g.journals_dir, "days");
        assert_eq!(g.file_names, FileNameFormat::TripleLowbar);
        assert_eq!(g.hidden, ["private", "drafts"]);
        assert_eq!(g.journal_template.as_deref(), Some("Daily"));
        assert_eq!(g.comma_properties, ["topics", "alias", "tags"]);
        assert!(g.is_hidden("private/a.md"));
        assert!(g.is_hidden("logseq/bak/x.md"));
        assert!(!g.is_hidden("notes/a.md"));
        let bad = Graph::logseq("/g", "{:a");
        assert_eq!(bad.problems.len(), 1);
        assert_eq!(bad.pages_dir, "pages");
    }

    #[test]
    fn obsidian_files_then_settings_then_defaults() {
        let none = Fallbacks::default();
        let g = Graph::obsidian("/v", None, None, None, &none);
        assert_eq!(g.journals_dir, "");
        assert_eq!(g.journal_file.source(), "YYYY-MM-DD");
        assert_eq!(g.new_file, NewFile::Root);
        assert_eq!(g.link_format, LinkFormat::Shortest);
        assert!(!g.markdown_links);
        let settings = Fallbacks {
            daily_notes: Some("Journal".into()),
            daily_format: Some("YYYY/MM/DD".into()),
            new_notes: Some("Inbox".into()),
            link_style: Some("relative".into()),
        };
        let g = Graph::obsidian("/v", None, None, None, &settings);
        assert_eq!(g.journals_dir, "Journal");
        assert_eq!(g.new_file, NewFile::Folder("Inbox".into()));
        assert_eq!(g.link_format, LinkFormat::Relative);
        let g = Graph::obsidian(
            "/v",
            Some(r#"{"folder":"Daily/","format":"YYYY-MM-DD","template":"Templates/Daily.md"}"#),
            Some(
                r#"{"newFileLocation":"folder","newFileFolderPath":"Notes","useMarkdownLinks":true,"newLinkFormat":"absolute","attachmentFolderPath":"Files"}"#,
            ),
            Some(r#"{"folder":"Templates"}"#),
            &settings,
        );
        assert_eq!(g.journals_dir, "Daily");
        assert_eq!(g.journal_template.as_deref(), Some("Templates/Daily"));
        assert_eq!(g.new_file, NewFile::Folder("Notes".into()));
        assert!(g.markdown_links);
        assert_eq!(g.link_format, LinkFormat::Absolute);
        assert_eq!(g.attachments, "Files");
        assert_eq!(g.templates_dir.as_deref(), Some("Templates"));
        assert!(g.is_hidden(".trash/x.md"));
        assert!(g.is_hidden(".obsidian/app.json"));
        let bad = Graph::obsidian("/v", Some("{"), None, None, &settings);
        assert_eq!(bad.problems.len(), 1);
        assert_eq!(bad.journals_dir, "Journal");
    }

    #[test]
    fn roots_are_found_upwards() {
        let m = Memory::new(&[
            ("/w/notes/logseq/config.edn", "{}"),
            ("/w/notes/pages/a.md", ""),
            ("/w/vault/.obsidian/app.json", "{}"),
            ("/w/vault/x/y.md", ""),
            ("/w/other/z.md", ""),
        ]);
        let mut not = std::collections::BTreeSet::new();
        assert_eq!(
            find_root(&m, "/w/notes/pages/a.md", &[], &mut not),
            Some(("/w/notes".into(), Kind::Logseq))
        );
        assert_eq!(
            find_root(&m, "/w/vault/x/y.md", &[], &mut not),
            Some(("/w/vault".into(), Kind::Obsidian))
        );
        assert_eq!(find_root(&m, "/w/other/z.md", &[], &mut not), None);
        assert!(not.contains("/w/other"));
        assert_eq!(
            find_root(
                &m,
                "/w/other/z.md",
                &[("/w/other".into(), Kind::Obsidian)],
                &mut not
            ),
            Some(("/w/other".into(), Kind::Obsidian))
        );
    }
}
