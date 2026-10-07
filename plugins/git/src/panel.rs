//! The Git panel (DESIGN.md, 2.9) as a tree of widgets, described here
//! without Kalem's types so that the state machine ([`crate::app`]) and
//! its tests need none; the component turns it into `kalem_plugin::ui`'s
//! tree.

/// How a label's text shows (Kalem's `text-style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStyle {
    /// As text.
    Normal,
    /// Bold.
    Strong,
    /// Dimmed.
    Muted,
    /// As an error.
    Error,
}

/// A widget and the widgets under it.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// Its children one under another.
    Column(Vec<Node>),
    /// Its children side by side.
    Row(Vec<Node>),
    /// A text.
    Label {
        /// The text.
        text: String,
        /// How it shows.
        style: TextStyle,
    },
    /// A button, heard by its key.
    Button {
        /// Its key.
        key: String,
        /// Its label.
        label: String,
    },
    /// A box to tick, heard by its key.
    Checkbox {
        /// Its key.
        key: String,
        /// Its label.
        label: String,
        /// Ticked.
        checked: bool,
    },
    /// An entry, heard by its key, with entries under it.
    Item {
        /// Its key.
        key: String,
        /// Its label.
        label: String,
        /// A second, dimmer text.
        detail: Option<String>,
        /// Whether its children show; none for an entry without any.
        expanded: Option<bool>,
        /// The entries under it.
        children: Vec<Node>,
    },
    /// A bar of progress; `None` while unknown.
    Progress {
        /// From 0 to 1.
        value: Option<f32>,
        /// What it is.
        label: Option<String>,
    },
}

impl Node {
    /// A label.
    pub fn label(text: impl Into<String>, style: TextStyle) -> Node {
        Node::Label {
            text: text.into(),
            style,
        }
    }

    /// Every widget's text, depth first, for the tests: a checkbox as
    /// `[x] label`, an entry as `label — detail`.
    pub fn texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&mut |n| match n {
            Node::Label { text, .. } => out.push(text.clone()),
            Node::Button { label, .. } => out.push(format!("({label})")),
            Node::Checkbox { label, checked, .. } => {
                out.push(format!("[{}] {label}", if *checked { 'x' } else { ' ' }))
            }
            Node::Item { label, detail, .. } => out.push(match detail {
                Some(d) => format!("{label} — {d}"),
                None => label.clone(),
            }),
            Node::Progress { label, .. } => {
                out.push(format!("… {}", label.as_deref().unwrap_or("")))
            }
            Node::Column(_) | Node::Row(_) => {}
        });
        out
    }

    /// The keys of the widgets the user acts on, depth first.
    pub fn keys(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&mut |n| match n {
            Node::Button { key, .. } | Node::Checkbox { key, .. } | Node::Item { key, .. } => {
                out.push(key.clone())
            }
            _ => {}
        });
        out
    }

    fn walk(&self, f: &mut dyn FnMut(&Node)) {
        f(self);
        match self {
            Node::Column(c) | Node::Row(c) | Node::Item { children: c, .. } => {
                for n in c {
                    n.walk(f);
                }
            }
            _ => {}
        }
    }
}
