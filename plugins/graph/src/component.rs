//! The component: Kalem's `extension` world over [`crate::app`]. The
//! commands, events, answers and panel clicks become the app's calls with
//! Kalem's `fs` as its files; its effects become `kalem_plugin` calls.

use std::cell::RefCell;
use std::collections::BTreeMap;

use kalem_plugin::kalem::{self, Event, EventKind, Plugin, Reply, Scope};
use kalem_plugin::{documents, editor, fs, settings, ui};

use crate::app::{self, Answer, App, Ctx, Effect, Level, Settings};
use crate::content::{Content, Style};
use crate::files::{self, Files};
use crate::views::PanelNode;

/// Kalem's `fs`, granted by `fs:read:workspace` and `fs:write:workspace`.
struct Fs;

impl Files for Fs {
    fn read(&self, path: &str) -> Result<String, String> {
        fs::read(path)
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, String> {
        fs::list(dir).map(|l| {
            l.into_iter()
                .map(|p| {
                    let dir = p.ends_with('/') || p.ends_with('\\');
                    let n = files::normalize(&p);
                    if dir { format!("{n}/") } else { n }
                })
                .collect()
        })
    }

    fn write(&self, path: &str, text: &str) -> Result<(), String> {
        fs::write(path, text)
    }
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static STATUS: RefCell<Option<kalem::Disposable>> = const { RefCell::new(None) };
    /// The documents open: `(id, key)` to Kalem's number.
    static DOCS: RefCell<BTreeMap<(String, String), u64>> = const { RefCell::new(BTreeMap::new()) };
}

fn with_app(f: impl FnOnce(&mut App) -> Vec<Effect>) {
    let effects = APP.with(|a| a.borrow_mut().as_mut().map(f).unwrap_or_default());
    for e in effects {
        apply(e);
    }
}

fn level(l: Level) -> ui::Level {
    match l {
        Level::Info => ui::Level::Info,
        Level::Warning => ui::Level::Warning,
        Level::Error => ui::Level::Error,
    }
}

fn apply(effect: Effect) {
    match effect {
        Effect::Notify(message, l) => ui::notify(&message, level(l)),
        Effect::Status(Some(text)) => {
            let options = ui::StatusOptions {
                tooltip: Some("The graph of this note: its pages; a click lists them".into()),
                command: Some("graph.pages".into()),
                alignment: ui::Alignment::Left,
                priority: 5,
            };
            if let Ok(d) = ui::status(app::STATUS, &text, &options) {
                STATUS.with(|s| *s.borrow_mut() = Some(d));
            }
        }
        Effect::Status(None) => {
            if let Some(d) = STATUS.with(|s| s.borrow_mut().take()) {
                d.dispose();
            }
        }
        Effect::Reveal { path, app } => {
            let name = crate::files::file_name(&path).to_string();
            // `file.reveal` takes a path (Kalem's plugin API 0.2.10).
            let args = serde_json::json!({ "path": path }).to_string();
            match kalem::run("file.reveal", &args) {
                Ok(()) => ui::notify(
                    &format!("{app} draws {name}: shown in the file manager, to open it there"),
                    ui::Level::Info,
                ),
                Err(e) => ui::notify(&e, ui::Level::Error),
            }
        }
        Effect::Open { path, line } => {
            let args = serde_json::json!({ "path": path, "line": line }).to_string();
            if let Err(e) = kalem::run("file.open", &args) {
                ui::notify(&e, ui::Level::Error);
            }
        }
        Effect::Document {
            id,
            key,
            title,
            kind,
            content,
            show,
        } => {
            let styles = styled(&content);
            let open = DOCS.with(|d| d.borrow().get(&(id.clone(), key.clone())).copied());
            if show {
                let spec = documents::Spec::new(&id, &key, &title, &kind);
                match documents::open_styled(&spec, &content.text, None, &styles) {
                    Ok(n) => DOCS.with(|d| {
                        d.borrow_mut().insert((id, key), n);
                    }),
                    Err(e) => ui::notify(&e, ui::Level::Error),
                }
            } else if let Some(n) = open
                && documents::set_styled(n, &content.text, None, &styles).is_err()
            {
                // The user closed it.
                DOCS.with(|d| d.borrow_mut().remove(&(id.clone(), key.clone())));
                APP.with(|a| {
                    if let Some(app) = a.borrow_mut().as_mut() {
                        app.closed(&id, &key);
                    }
                });
            }
        }
        Effect::Panel(nodes) => {
            let mut tree = ui::Tree::new(ui::WidgetKind::Column);
            if nodes.is_empty() {
                tree.add(
                    ui::Tree::ROOT,
                    "",
                    ui::WidgetKind::Label(ui::Label {
                        text: "Open a page of a Logseq graph or an Obsidian vault".into(),
                        style: ui::TextStyle::Muted,
                    }),
                );
            }
            for n in nodes {
                add(&mut tree, ui::Tree::ROOT, n);
            }
            let _ = ui::set_panel(app::PANEL, &tree);
        }
        Effect::ShowPanel => {
            let _ = kalem::run(
                "view.pluginPanel",
                &format!("{{\"id\":\"{}\"}}", app::PANEL),
            );
        }
        Effect::Pick {
            token,
            title,
            items,
        } => {
            let items: Vec<ui::PickItem> = items
                .into_iter()
                .map(|(label, detail)| ui::PickItem { label, detail })
                .collect();
            ui::quick_pick(
                &items,
                &ui::PickOptions {
                    title: Some(title),
                    placeholder: None,
                    many: false,
                },
                move |picked| {
                    let answer = Answer::Picked((!picked.is_empty()).then_some(picked));
                    with_app(|app| app.answer(&Fs, token, answer));
                },
            );
        }
        Effect::Prompt {
            token,
            title,
            value,
            placeholder,
        } => ui::prompt(
            &title,
            &ui::PromptOptions {
                value,
                placeholder,
                password: false,
            },
            move |text| with_app(|app| app.answer(&Fs, token, Answer::Text(text))),
        ),
        Effect::Run { id, args } => {
            if let Err(e) = kalem::run(&id, &args) {
                ui::notify(&e, ui::Level::Error);
            }
        }
        Effect::Insert(text) => editor::insert(None, &text),
        Effect::Edits { edits, label, line } => {
            // Places of the text as the command found it; Kalem applies
            // the edits in order when the command returns, as one undo step.
            for e in edits {
                let done = if e.start == e.end {
                    editor::insert(Some(e.start as u64), &e.text);
                    Ok(())
                } else {
                    editor::replace(
                        editor::Range {
                            start: e.start as u64,
                            end: e.end as u64,
                        },
                        &e.text,
                    )
                };
                if let Err(why) = done {
                    ui::notify(&why, ui::Level::Warning);
                }
            }
            editor::transact(&label);
            if let Some(n) = line {
                let _ = kalem::run("edit.gotoLine", &format!("{{\"line\":{}}}", n + 1));
            }
        }
        Effect::RefreshLayers => kalem_plugin::layer::refresh(),
    }
}

/// The content's styles as Kalem's: the theme's colors by name.
fn styled(content: &Content) -> Vec<documents::StyledSpan> {
    use documents::{Color, TextStyle};
    content
        .styles
        .iter()
        .filter_map(|s| {
            let style = match s.style {
                Style::Normal => return None,
                Style::Strong => TextStyle::color(Color::Default).bold(),
                Style::Muted => TextStyle::color(Color::Muted),
                Style::Heading => TextStyle::color(Color::Green).bold(),
                Style::Link => TextStyle::color(Color::Accent),
                Style::Tag => TextStyle::color(Color::Magenta),
                Style::Todo => TextStyle::color(Color::Red).bold(),
                Style::Done => TextStyle::color(Color::Muted),
                Style::Error => TextStyle::color(Color::Red),
            };
            Some(documents::StyledSpan {
                start: s.start as u64,
                end: s.end as u64,
                style,
            })
        })
        .collect()
}

/// Adds `node` and the nodes under it to `tree` under `parent`.
fn add(tree: &mut ui::Tree, parent: u32, node: PanelNode) {
    let at = tree.add(
        parent,
        &node.key,
        ui::WidgetKind::Item(ui::Item {
            label: node.label,
            detail: node.detail,
            expanded: node.expanded,
            selected: false,
        }),
    );
    for c in node.children {
        add(tree, at, c);
    }
}

/// The document the running command is in.
fn context(args: &str) -> Ctx {
    let info = editor::document();
    let doc = documents::current().and_then(|n| {
        DOCS.with(|d| {
            d.borrow()
                .iter()
                .find(|(_, number)| **number == n)
                .map(|(key, _)| key.clone())
        })
    });
    let text = info.as_ref().map(|_| editor::text(None));
    Ctx {
        path: info.and_then(|i| i.path).map(|p| files::normalize(&p)),
        text,
        cursor: editor::selected().head as usize,
        doc,
        args: args.to_string(),
    }
}

fn read_settings() -> Settings {
    Settings::read(settings::own)
}

/// The plugin.
struct Graph;

impl Plugin for Graph {
    fn activate() -> Result<(), String> {
        APP.with(|a| {
            *a.borrow_mut() = Some(App::new(read_settings()));
            // Block IDs from the clock's random bits, the views' day from its
            // time (API 0.2.10).
            if let Some(app) = a.borrow_mut().as_mut() {
                app.random = Some(kalem_plugin::clock::random);
                app.now = Some(kalem_plugin::clock::now);
            }
        });
        for c in app::COMMANDS {
            let mut spec = kalem::spec(c.id, c.title, Scope::all());
            spec.category = "Graph".into();
            let id = c.id;
            kalem::command(spec, move |args| {
                let ctx = context(args);
                with_app(|app| app.command(&Fs, id, &ctx));
                Ok("null".into())
            })?;
            for k in c.keys {
                kalem::keymap(k, c.id, Some(app::LEADER_WHEN))?;
            }
            for k in c.doc_keys {
                kalem::keymap(k, c.id, Some(app::DOC_WHEN))?;
            }
            for (k, when) in c.note_keys {
                kalem::keymap(k, c.id, Some(when))?;
            }
        }
        ui::panel(
            ui::PanelSpec {
                id: app::PANEL.into(),
                title: "Backlinks".into(),
                placement: ui::Placement::Side,
            },
            |key, event| {
                let key = key.to_string();
                match event {
                    ui::PanelEvent::Clicked => with_app(|app| app.panel_clicked(&key)),
                    ui::PanelEvent::Expanded(open) => {
                        let open = *open;
                        with_app(|app| app.panel_expanded(&key, open));
                    }
                    _ => {}
                }
            },
        )?;
        kalem::on(EventKind::DocumentOpen, |e| {
            if let Event::DocumentOpen(d) = e
                && let Some(p) = &d.path
            {
                let (n, p) = (d.document, p.clone());
                with_app(|app| {
                    app.track_open(n, &p);
                    app.opened(&Fs, &p)
                });
            }
            Reply::Proceed
        })?;
        // Which files Kalem holds with unsaved changes: never written by
        // the plugin.
        kalem::on(EventKind::DocumentChanged, |e| {
            if let Event::DocumentChanged(d) = e {
                let n = d.document;
                with_app(|app| {
                    app.track_changed(n);
                    Vec::new()
                });
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::DocumentClose, |e| {
            if let Event::DocumentClose(n) = e {
                let n = *n;
                with_app(|app| app.closed_document(&Fs, n));
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::DocumentAfterSave, |e| {
            if let Event::DocumentAfterSave(d) = e {
                let p = d.path.clone();
                with_app(|app| app.saved(&Fs, &p));
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::WorkspaceFileChanged, |e| {
            if let Event::WorkspaceFileChanged(p) = e {
                let p = p.clone();
                with_app(|app| app.saved(&Fs, &p));
            }
            Reply::Proceed
        })?;
        for key in app::SETTINGS {
            settings::watch(key, true, |_| {
                let s = read_settings();
                with_app(|app| app.settings(&Fs, s));
            })?;
        }
        apply(Effect::Panel(Vec::new()));
        Ok(())
    }

    /// The completer's items (plugin API 0.2.11).
    fn complete(
        completer: &str,
        request: &kalem_plugin::completer::Request,
    ) -> Vec<kalem_plugin::completer::Item> {
        use crate::app::complete::What;
        use kalem_plugin::completer::{Item, ItemKind};
        let found = APP
            .with(|a| {
                a.try_borrow().ok().and_then(|a| {
                    a.as_ref().map(|app| {
                        app.complete(
                            completer,
                            request.path.as_deref(),
                            &request.text,
                            request.base as usize,
                            request.cursor as usize,
                        )
                    })
                })
            })
            .unwrap_or_default();
        found
            .into_iter()
            .map(|s| Item {
                label: s.label,
                insert: s.insert,
                start: s.start as u64,
                cursor: s.cursor.map(|c| c as u32),
                kind: match s.what {
                    What::Link => ItemKind::Link,
                    What::Tag => ItemKind::Tag,
                    What::Snippet => ItemKind::Snippet,
                },
                detail: s.detail,
                documentation: None,
            })
            .collect()
    }

    /// The layer's overlays (plugin API 0.2.10).
    fn overlays(layer: &str, path: Option<&str>, text: &str) -> kalem_plugin::layer::OverlaySet {
        use crate::layer::{Effect as E, LineEffect as L};
        use kalem_plugin::layer as api;
        let o = APP
            .with(|a| {
                a.borrow()
                    .as_ref()
                    .map(|app| app.overlays(layer, path, text))
            })
            .unwrap_or_default();
        let look = |l: crate::layer::Look| {
            let mut f = api::SpanStyle::empty();
            for (on, flag) in [
                (l.bold, api::SpanStyle::BOLD),
                (l.italic, api::SpanStyle::ITALIC),
                (l.strike, api::SpanStyle::STRIKE),
                (l.code, api::SpanStyle::CODE),
                (l.link, api::SpanStyle::LINK),
                (l.dim, api::SpanStyle::DIM),
                (l.tag, api::SpanStyle::TAG),
                (l.todo, api::SpanStyle::TODO),
                (l.done, api::SpanStyle::DONE),
                (l.timestamp, api::SpanStyle::TIMESTAMP),
                (l.priority, api::SpanStyle::PRIORITY),
            ] {
                if on {
                    f |= flag;
                }
            }
            f
        };
        api::OverlaySet {
            spans: o
                .spans
                .into_iter()
                .map(|s| api::Span {
                    start: s.start as u64,
                    end: s.end as u64,
                    effect: match s.effect {
                        E::Hide => api::SpanEffect::Hide,
                        E::Replace(text, l) => api::SpanEffect::Replace(api::Replacement {
                            text,
                            style: look(l),
                        }),
                        E::Style(l) => api::SpanEffect::Style(look(l)),
                    },
                })
                .collect(),
            lines: o
                .lines
                .into_iter()
                .map(|l| api::Lines {
                    start: l.start as u64,
                    end: l.end as u64,
                    effect: match l.effect {
                        L::Hidden => api::LineEffect::Hidden,
                        L::Folded => api::LineEffect::Folded,
                    },
                })
                .collect(),
        }
    }
}

kalem_plugin::export_plugin!(Graph);
