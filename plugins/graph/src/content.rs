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

/// Where Enter on a line goes: a file (absolute) at a line (from 0), or a
/// page's backlinks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A file at a line.
    File {
        /// Absolute.
        path: String,
        /// From 0.
        line: u32,
    },
    /// The backlinks of the page with this key.
    Page(String),
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
