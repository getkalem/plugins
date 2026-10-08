//! The component: Kalem's `extension` world over [`crate::app`]. The
//! commands, the panel, the events and the settings become the state
//! machine's inputs; its effects become `kalem_plugin` calls, and the
//! answers to them (a git run's end, the user's answer) come back as
//! inputs, later, through the closures the calls keep.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use kalem_plugin::kalem::{self, Event, EventKind, Plugin, Reply, Scope};
use kalem_plugin::{editor, process, settings, ui};

use crate::app::{self, Answer, App, Doc, Effect, Input, Level, PanelEvent, Settings};
use crate::panel::{Node, TextStyle};
use crate::refresh::Output;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<Input>> = const { RefCell::new(VecDeque::new()) };
    static PUMPING: Cell<bool> = const { Cell::new(false) };
    static STATUS: RefCell<Option<kalem::Disposable>> = const { RefCell::new(None) };
}

/// The settings `Settings::read` reads, watched.
const SETTINGS: &[&str] = &[
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
        Effect::Prompt { token, title } => ui::prompt(
            &title,
            &ui::PromptOptions {
                value: None,
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
    }
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
/// and for `git.blameLine` its text.
fn document(command: &str) -> Option<Doc> {
    let info = editor::document()?;
    let head = editor::selected().head;
    let before = editor::text(Some(editor::Range {
        start: 0,
        end: head,
    }));
    let line = before.bytes().filter(|b| *b == b'\n').count() as u32 + 1;
    let text = (command == "git.blameLine").then(|| editor::text(None));
    Some(Doc {
        path: info.path,
        line,
        text,
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
            let mut spec = kalem::spec(c.id, c.title, Scope::all());
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
