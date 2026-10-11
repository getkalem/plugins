//! A document's text as the plugin writes it: lines with styles on parts
//! of them, and for each line where Enter takes the user.

/// How a stretch of a document looks; the component maps it to the
/// theme's colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// The text's own.
    Normal,
    /// Bold.
    Strong,
    /// Dimmed.
    Muted,
    /// A section's heading.
    Heading,
    /// A link or a page's name.
    Link,
    /// A tag.
    Tag,
    /// An open task's keyword.
    Todo,
    /// A done task's keyword.
    Done,
    /// What went wrong.
    Error,
}

/// A styled stretch: bytes `start` to `end` of the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Styled {
    /// From.
    pub start: usize,
    /// To (left out).
    pub end: usize,
    /// How it looks.
    pub style: Style,
}

/// Where Enter on a line goes: a file (absolute) at a line (from 0), a
/// page's backlinks, or a command of the plugin's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A command of the plugin's run with its arguments (JSON): an entry
    /// of the graph's menu.
    Command {
        /// `graph.NAME`.
        id: String,
        /// Its arguments.
        args: String,
    },
    /// A file at a line.
    File {
        /// Absolute.
        path: String,
        /// From 0.
        line: u32,
    },
    /// The backlinks of the page with this key.
    Page(String),
    /// A block, at its first line: opened as [`Target::File`] is, and
    /// what `t` cycles the keyword of in the Tasks and Query documents.
    Block {
        /// Absolute.
        path: String,
        /// From 0.
        line: u32,
    },
}

/// A node of a document drawn as a graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnNode {
    /// What opening it hands back (a page's key).
    pub id: String,
    /// What is written beside it.
    pub label: String,
    /// Its group, which its color follows.
    pub group: String,
    /// How large it is drawn, 1 the least.
    pub weight: u32,
}

/// A document drawn as a graph by Kalem (plugin API 0.2.12), its text the
/// source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drawing {
    /// The nodes.
    pub nodes: Vec<DrawnNode>,
    /// The edges, by the nodes' ids.
    pub edges: Vec<(String, String)>,
    /// The node kept in the middle.
    pub focus: Option<String>,
    /// The plugin's command run with `{"node": id}` when one is opened.
    pub command: String,
}

/// A document's text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Content {
    /// The text.
    pub text: String,
    /// Its styles, in order.
    pub styles: Vec<Styled>,
    /// Where Enter goes, by line.
    pub targets: Vec<Option<Target>>,
    /// The graph it is drawn as, when it is one.
    pub drawing: Option<Drawing>,
}

impl Content {
    /// An empty document.
    pub fn new() -> Content {
        Content::default()
    }

    /// Adds a line of `parts`, each with its style, and where Enter on it
    /// goes.
    pub fn line(&mut self, parts: &[(&str, Style)], target: Option<Target>) {
        for (text, style) in parts {
            let start = self.text.len();
            self.text.push_str(text);
            if *style != Style::Normal && !text.is_empty() {
                self.styles.push(Styled {
                    start,
                    end: self.text.len(),
                    style: *style,
                });
            }
        }
        self.text.push('\n');
        self.targets.push(target);
    }

    /// An empty line.
    pub fn blank(&mut self) {
        self.line(&[], None);
    }

    /// Where Enter on line `n` (from 0) goes.
    pub fn target(&self, n: usize) -> Option<&Target> {
        self.targets.get(n).and_then(Option::as_ref)
    }

    /// The line a byte of the text is on.
    pub fn line_of(&self, byte: usize) -> usize {
        self.text[..byte.min(self.text.len())]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
    }
}
