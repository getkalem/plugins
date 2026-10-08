//! The component: Kalem's `extension` world over [`crate::app`]. The
//! commands, the panel, the events and the settings become the state
//! machine's inputs; its effects become `kalem_plugin` calls, and the
//! answers to them (a git run's end, the user's answer) come back as
//! inputs, later, through the closures the calls keep.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};

use kalem_plugin::kalem::{self, Event, EventKind, Plugin, Reply, Scope};
use kalem_plugin::{decorations, documents, editor, process, settings, ui};

use crate::app::{self, Answer, App, Doc, Effect, Input, Level, PanelEvent, Settings};
use crate::panel::{Node, TextStyle};
use crate::refresh::Output;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<Input>> = const { RefCell::new(VecDeque::new()) };
    static PUMPING: Cell<bool> = const { Cell::new(false) };
    static STATUS: RefCell<Option<kalem::Disposable>> = const { RefCell::new(None) };
    /// The documents open, by repository and, for a commit view, the
    /// commit's hash: their numbers.
    static DOCS: RefCell<BTreeMap<(String, Option<String>), u64>> = const { RefCell::new(BTreeMap::new()) };
}

/// The settings `Settings::read` reads, watched.
const SETTINGS: &[&str] = &[
    "gutter",
    "gutter_base",
    "confirm_discard",
    "pull",
    "fetch_prune",
    "untracked",
    "signoff",
    "glyphs",
];

/// Hands `input` to the plugin and does what follows, each effect's own
/// inputs (a run refused at once) after it.
fn feed(input: Input) {
    QUEUE.with(|q| q.borrow_mut().push_back(input));
    if PUMPING.with(|p| p.replace(true)) {
        return;
    }
    while let Some(i) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
        let effects = APP.with(|a| {
            a.borrow_mut()
                .as_mut()
                .map(|app| app.handle(i))
                .unwrap_or_default()
        });
        for e in effects {
            apply(e);
        }
    }
    PUMPING.with(|p| p.set(false));
}

fn later(input: Input) {
    QUEUE.with(|q| q.borrow_mut().push_back(input));
}

fn apply(effect: Effect) {
    match effect {
        Effect::Run {
            token,
            cwd,
            command,
        } => {
            let mut c = process::Command::new("git").args(command.argv());
            for (k, v) in command.env() {
                c = c.env(&k, &v);
            }
            if let Some(dir) = cwd {
                c = c.cwd(&dir);
            }
            if let Some(input) = command.stdin {
                c = c.stdin(input);
            }
            let started = process::run(&c, move |result| {
                feed(Input::Done {
                    token,
                    result: result.map(|e| Output {
                        status: e.status,
                        stdout: e.stdout,
                        stderr: e.stderr,
                    }),
                })
            });
            if let Err(e) = started {
                later(Input::Done {
                    token,
                    result: Err(e),
                });
            }
        }
        Effect::Notify(message, level) => ui::notify(
            &message,
            match level {
                Level::Info => ui::Level::Info,
                Level::Warning => ui::Level::Warning,
                Level::Error => ui::Level::Error,
            },
        ),
        Effect::Status(Some(text)) => {
            let options = ui::StatusOptions {
                tooltip: Some(
                    "Git: the branch, ahead and behind the upstream, the changed files".into(),
                ),
                command: Some("git.status".into()),
                alignment: ui::Alignment::Left,
                priority: 10,
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
        Effect::Panel(node) => {
            let mut tree = ui::Tree::new(ui::WidgetKind::Column);
            if let Node::Column(children) = node {
                for n in children {
                    add(&mut tree, ui::Tree::ROOT, n);
                }
            } else {
                add(&mut tree, ui::Tree::ROOT, node);
            }
            let _ = ui::set_panel(app::PANEL, &tree);
        }
        Effect::ShowPanel => {
            let _ = kalem::run(
                "view.pluginPanel",
                &format!("{{\"id\":\"{}\"}}", app::PANEL),
            );
        }
        Effect::Confirm { token, message } => ui::confirm(&message, move |yes| {
            feed(Input::Answer {
                token,
                answer: Answer::Confirmed(yes),
            })
        }),
        Effect::Prompt {
            token,
            title,
            value,
        } => ui::prompt(
            &title,
            &ui::PromptOptions {
                value,
                placeholder: Some("Enter commits, Escape cancels".into()),
                password: false,
            },
            move |text| {
                feed(Input::Answer {
                    token,
                    answer: Answer::Text(text),
                })
            },
        ),
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
                    feed(Input::Answer {
                        token,
                        answer: Answer::Picked(picked),
                    })
                },
            );
        }
        Effect::RunCommand { id, args } => {
            if let Err(e) = kalem::run(&id, &args) {
                ui::notify(&e, ui::Level::Error);
            }
        }
        Effect::SetSetting { key, value } => {
            if let Err(e) = settings::set(&key, &value) {
                ui::notify(&e, ui::Level::Warning);
            }
        }
        Effect::Document {
            root,
            commit,
            title,
            text,
            cursor,
            show,
            styles,
        } => {
            let cursor = cursor.map(|c| c as u64);
            let styles = styled(&styles);
            let key = (root.clone(), commit.clone());
            let open = DOCS.with(|d| d.borrow().get(&key).copied());
            if show {
                let spec = match &commit {
                    Some(hash) => documents::Spec::new(
                        app::COMMIT_DOC,
                        &format!("{root}#{hash}"),
                        &title,
                        app::COMMIT_KIND,
                    ),
                    None => documents::Spec::new(app::STATUS_DOC, &root, &title, app::STATUS_KIND),
                }
                .language("diff");
                match documents::open_styled(&spec, &text, cursor, &styles) {
                    Ok(n) => DOCS.with(|d| {
                        d.borrow_mut().insert(key, n);
                    }),
                    Err(e) => ui::notify(&e, ui::Level::Error),
                }
            } else if let Some(n) = open
                && documents::set_styled(n, &text, cursor, &styles).is_err()
            {
                // The user closed it.
                DOCS.with(|d| d.borrow_mut().remove(&key));
                later(Input::DocumentClosed { root, commit });
            }
        }
        Effect::CloseDocument { root, commit } => {
            if let Some(n) = DOCS.with(|d| d.borrow_mut().remove(&(root, commit))) {
                documents::close(n);
            }
        }
        Effect::Gutter { path, marks } => {
            use crate::gutter::Mark;
            let marks: Vec<(u32, decorations::Mark)> = marks
                .into_iter()
                .map(|(line, m)| {
                    let kind = match m {
                        Mark::Added => decorations::Mark::Added,
                        Mark::Changed => decorations::Mark::Changed,
                        Mark::Removed => decorations::Mark::Removed,
                    };
                    (line, kind)
                })
                .collect();
            let _ = decorations::set_gutter(&path, &marks);
        }
        Effect::ClearGutters => decorations::clear_gutter(None),
    }
}

/// The content's styles as Kalem's: the theme's colors by name.
fn styled(styles: &[crate::content::Styled]) -> Vec<documents::StyledSpan> {
    use crate::content::{Color as C, Style};
    use documents::{Color, TextStyle};
    let color = |c: C| match c {
        C::Red => Color::Red,
        C::Green => Color::Green,
        C::Yellow => Color::Yellow,
        C::Blue => Color::Blue,
        C::Magenta => Color::Magenta,
        C::Cyan => Color::Cyan,
    };
    styles
        .iter()
        .filter_map(|s| {
            let style = match s.style {
                Style::Normal => return None,
                Style::Strong => TextStyle::color(Color::Default).bold(),
                Style::Emphasis => TextStyle {
                    italic: true,
                    ..TextStyle::color(Color::Default)
                },
                Style::Muted => TextStyle::color(Color::Muted),
                Style::Code => TextStyle::color(Color::Yellow),
                Style::Error => TextStyle::color(Color::Red),
                Style::Heading => TextStyle::color(Color::Green).bold(),
                Style::Color(c, true) => TextStyle::color(color(c)).bold(),
                Style::Color(c, false) => TextStyle::color(color(c)),
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
fn add(tree: &mut ui::Tree, parent: u32, node: Node) {
    let (key, kind, children) = match node {
        Node::Column(c) => (String::new(), ui::WidgetKind::Column, c),
        Node::Row(c) => (String::new(), ui::WidgetKind::Row, c),
        Node::Label { text, style } => (
            String::new(),
            ui::WidgetKind::Label(ui::Label {
                text,
                style: match style {
                    TextStyle::Normal => ui::TextStyle::Normal,
                    TextStyle::Strong => ui::TextStyle::Strong,
                    TextStyle::Muted => ui::TextStyle::Muted,
                    TextStyle::Error => ui::TextStyle::Error,
                },
            }),
            Vec::new(),
        ),
        Node::Button { key, label } => (
            key,
            ui::WidgetKind::Button(ui::Button {
                label,
                command: None,
            }),
            Vec::new(),
        ),
        Node::Checkbox {
            key,
            label,
            checked,
        } => (
            key,
            ui::WidgetKind::Checkbox(ui::Checkbox { label, checked }),
            Vec::new(),
        ),
        Node::Item {
            key,
            label,
            detail,
            expanded,
            children,
        } => (
            key,
            ui::WidgetKind::Item(ui::Item {
                label,
                detail,
                expanded,
                selected: false,
            }),
            children,
        ),
        Node::Progress { value, label } => (
            String::new(),
            ui::WidgetKind::Progress(ui::Progress { value, label }),
            Vec::new(),
        ),
    };
    let at = tree.add(parent, &key, kind);
    for c in children {
        add(tree, at, c);
    }
}

/// The document the running command is in: its file, the cursor's line,
/// the selection, for `git.blameLine` its text, and the repository when it
/// is a status document or the view of a commit.
fn document(command: &str) -> Option<Doc> {
    let info = editor::document()?;
    let selected = editor::selected();
    let before = editor::text(Some(editor::Range {
        start: 0,
        end: selected.head,
    }));
    let line = before.bytes().filter(|b| *b == b'\n').count() as u32 + 1;
    let text = (command == "git.blameLine").then(|| editor::text(None));
    let open = documents::current().and_then(|n| {
        DOCS.with(|d| {
            d.borrow()
                .iter()
                .find(|(_, number)| **number == n)
                .map(|(key, _)| key.clone())
        })
    });
    let (status, commit) = match open {
        Some((root, None)) => (Some(root), None),
        Some((root, Some(hash))) => (None, Some((root, hash))),
        None => (None, None),
    };
    Some(Doc {
        path: info.path,
        line,
        text,
        status,
        commit,
        anchor: selected.anchor as usize,
        head: selected.head as usize,
    })
}

fn read_settings() -> Settings {
    Settings::read(settings::own)
}

/// The plugin.
struct Git;

impl Plugin for Git {
    fn activate() -> Result<(), String> {
        APP.with(|a| *a.borrow_mut() = Some(App::new(read_settings())));
        for c in app::COMMANDS {
            let (leader, other) = app::split_leader_keys(c.keys);
            let scope = if c.in_status_only {
                Scope::only(&[app::STATUS_KIND])
            } else {
                Scope::all()
            };
            let mut spec = kalem::spec(c.id, c.title, scope);
            spec.category = "Git".into();
            spec.keys = other.iter().map(|k| k.to_string()).collect();
            let id = c.id;
            kalem::command(spec, move |_args| {
                feed(Input::Command {
                    id: id.to_string(),
                    doc: document(id),
                });
                Ok("null".into())
            })?;
            // Doom's keys in Vim's command mode only, as Kalem's own leader
            // keys: in insert mode Space types a space.
            for k in leader {
                kalem::keymap(k, c.id, Some(app::LEADER_WHEN))?;
            }
            // In the status document: magit's keys and the arrows.
            for k in c.doc_keys {
                let when = if *k == "escape" {
                    app::DOC_ESCAPE_WHEN
                } else {
                    app::DOC_WHEN
                };
                kalem::keymap(k, c.id, Some(when))?;
            }
        }
        ui::panel(
            ui::PanelSpec {
                id: app::PANEL.into(),
                title: "Git".into(),
                placement: ui::Placement::Side,
            },
            |key, event| {
                let event = match event {
                    ui::PanelEvent::Clicked => PanelEvent::Clicked,
                    ui::PanelEvent::Toggled(on) => PanelEvent::Toggled(*on),
                    ui::PanelEvent::Expanded(open) => PanelEvent::Expanded(*open),
                    ui::PanelEvent::Changed(_) | ui::PanelEvent::Submitted(_) => return,
                };
                feed(Input::Panel {
                    key: key.to_string(),
                    event,
                });
            },
        )?;
        kalem::on(EventKind::DocumentOpen, |e| {
            if let Event::DocumentOpen(d) = e
                && let Some(p) = &d.path
            {
                feed(Input::Opened(p.clone()));
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::DocumentAfterSave, |e| {
            if let Event::DocumentAfterSave(d) = e {
                feed(Input::Saved(d.path.clone()));
            }
            Reply::Proceed
        })?;
        kalem::on(EventKind::WorkspaceFileChanged, |e| {
            if let Event::WorkspaceFileChanged(p) = e {
                feed(Input::Changed(p.clone()));
            }
            Reply::Proceed
        })?;
        for key in SETTINGS {
            settings::watch(key, true, |_| feed(Input::Settings(read_settings())))?;
        }
        // The panel says what it is for before a repository is found.
        let empty = APP.with(|a| a.borrow().as_ref().map(App::panel));
        if let Some(p) = empty {
            apply(Effect::Panel(p));
        }
        Ok(())
    }
}

kalem_plugin::export_plugin!(Git);
