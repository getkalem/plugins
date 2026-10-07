//! A document's content as the plugin writes it (DESIGN.md, 3.2 and
//! appendix A.2): its text, the styles of some stretches, and its regions,
//! the stretches that stand for something the plugin acts on (a section, a
//! file, a hunk, a commit), nested. The views write content top to bottom;
//! the commands ask which regions hold the cursor.

/// How a stretch of text is shown: the `text-style` of Kalem's `ui`
/// interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// As the theme shows text.
    Normal,
    /// Bold.
    Strong,
    /// Italic.
    Emphasis,
    /// Dimmed.
    Muted,
    /// As code.
    Code,
    /// As an error.
    Error,
    /// As a heading.
    Heading,
}

/// A styled stretch, in bytes of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Styled {
    /// Its start.
    pub start: usize,
    /// Its end, left out.
    pub end: usize,
    /// Its style.
    pub style: Style,
}

/// A stretch of the text that stands for something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Its start, in bytes: the start of its first line.
    pub start: usize,
    /// Its end, left out: after its last line's newline.
    pub end: usize,
    /// The plugin's name for it: `file:unstaged:src/lib.rs`.
    pub key: String,
    /// It folds.
    pub foldable: bool,
    /// It is folded: its first line alone is written.
    pub folded: bool,
}

/// A document's content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Content {
    /// The text.
    pub text: String,
    /// Styled stretches, in order of their starts.
    pub styles: Vec<Styled>,
    /// The regions, in order of their starts; an enclosing region before
    /// the ones inside it.
    pub regions: Vec<Region>,
    open: Vec<usize>,
}

impl Content {
    /// An empty content.
    pub fn new() -> Content {
        Content::default()
    }

    /// Appends text in a style; the caller ends lines with `\n`.
    pub fn push(&mut self, text: &str, style: Style) {
        let start = self.text.len();
        self.text.push_str(text);
        if style != Style::Normal && !text.is_empty() {
            self.styles.push(Styled {
                start,
                end: self.text.len(),
                style,
            });
        }
    }

    /// Ends the line.
    pub fn newline(&mut self) {
        self.text.push('\n');
    }

    /// Appends a whole line in one style.
    pub fn line(&mut self, text: &str, style: Style) {
        self.push(text, style);
        self.newline();
    }

    /// Opens a region at the current position, which must start a line.
    pub fn open(&mut self, key: impl Into<String>, foldable: bool, folded: bool) {
        self.regions.push(Region {
            start: self.text.len(),
            end: self.text.len(),
            key: key.into(),
            foldable,
            folded,
        });
        self.open.push(self.regions.len() - 1);
    }

    /// Closes the innermost open region.
    pub fn close(&mut self) {
        if let Some(i) = self.open.pop() {
            self.regions[i].end = self.text.len();
        }
    }

    /// The regions holding `offset`, outermost first.
    pub fn regions_at(&self, offset: usize) -> Vec<&Region> {
        self.regions
            .iter()
            .filter(|r| {
                r.start <= offset && (offset < r.end || (r.start == r.end && r.start == offset))
            })
            .collect()
    }

    /// The innermost region holding `offset`.
    pub fn region_at(&self, offset: usize) -> Option<&Region> {
        self.regions_at(offset).pop()
    }

    /// The region named `key`.
    pub fn region(&self, key: &str) -> Option<&Region> {
        self.regions.iter().find(|r| r.key == key)
    }

    /// The line `offset` is on, from 0.
    pub fn line_of(&self, offset: usize) -> usize {
        let end = offset.min(self.text.len());
        self.text.as_bytes()[..end]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
    }

    /// Where line `line` (from 0) starts; the text's end past the last.
    pub fn line_start(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        self.text
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .nth(line - 1)
            .map_or(self.text.len(), |(i, _)| i + 1)
    }

    /// The style at `offset`, for the tests and the terminal snapshot.
    pub fn style_at(&self, offset: usize) -> Style {
        self.styles
            .iter()
            .rev()
            .find(|s| s.start <= offset && offset < s.end)
            .map_or(Style::Normal, |s| s.style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_nest_and_are_found_by_offset() {
        let mut c = Content::new();
        c.line("Head: main", Style::Strong);
        c.newline();
        c.open("section:unstaged", true, false);
        c.line("Unstaged changes (1)", Style::Heading);
        c.open("file:unstaged:a", true, false);
        c.line("modified a", Style::Normal);
        c.open("hunk:unstaged:a:0", true, false);
        c.line("@@ -1 +1 @@", Style::Normal);
        c.line("-x", Style::Normal);
        c.close();
        c.close();
        c.close();
        let x = c.text.find("-x").unwrap();
        let keys: Vec<&str> = c.regions_at(x).iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            ["section:unstaged", "file:unstaged:a", "hunk:unstaged:a:0"]
        );
        assert_eq!(c.region_at(0), None);
        assert_eq!(c.style_at(0), Style::Strong);
        assert_eq!(c.line_of(x), 5);
        assert_eq!(c.line_start(5), x);
        assert_eq!(c.line_start(99), c.text.len());
        assert_eq!(c.region("file:unstaged:a").unwrap().end, c.text.len());
    }
}
