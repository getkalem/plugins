//! The documents the plugin writes (DESIGN.md, 3.2): each turns what git
//! said into [`crate::content::Content`].

pub mod blame;
pub mod commit;
pub mod log;
pub mod process;
pub mod status;

/// The marks the documents draw (the setting `glyphs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Glyphs {
    /// `▸ ▾ ↑ ↓ → −`.
    #[default]
    Unicode,
    /// `> v` and words, for a terminal font without the others.
    Ascii,
}

impl Glyphs {
    /// The setting's value.
    pub fn parse(s: &str) -> Option<Glyphs> {
        match s {
            "unicode" => Some(Glyphs::Unicode),
            "ascii" => Some(Glyphs::Ascii),
            _ => None,
        }
    }

    /// A fold mark.
    pub fn fold(self, folded: bool) -> &'static str {
        match (self, folded) {
            (Glyphs::Unicode, true) => "▸",
            (Glyphs::Unicode, false) => "▾",
            (Glyphs::Ascii, true) => ">",
            (Glyphs::Ascii, false) => "v",
        }
    }

    /// A rename's arrow.
    pub fn arrow(self) -> &'static str {
        match self {
            Glyphs::Unicode => "→",
            Glyphs::Ascii => "->",
        }
    }

    /// Ahead and behind: `↑1 ↓2`, or `+1 ahead, -2 behind`.
    pub fn ahead_behind(self, ahead: u32, behind: u32) -> String {
        let mut parts = Vec::new();
        match self {
            Glyphs::Unicode => {
                if ahead > 0 {
                    parts.push(format!("↑{ahead}"));
                }
                if behind > 0 {
                    parts.push(format!("↓{behind}"));
                }
                parts.join(" ")
            }
            Glyphs::Ascii => {
                if ahead > 0 {
                    parts.push(format!("+{ahead} ahead"));
                }
                if behind > 0 {
                    parts.push(format!("-{behind} behind"));
                }
                parts.join(", ")
            }
        }
    }

    /// Lines added and removed: `+12 −3`.
    pub fn counts(self, added: u32, removed: u32) -> String {
        match self {
            Glyphs::Unicode => format!("+{added} −{removed}"),
            Glyphs::Ascii => format!("+{added} -{removed}"),
        }
    }
}

/// How the documents are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// The marks.
    pub glyphs: Glyphs,
    /// A file's diff with more lines than this is not unfolded in the
    /// status but offered as a document of its own (3.5).
    pub big_diff: usize,
}

impl Default for ViewOptions {
    fn default() -> ViewOptions {
        ViewOptions {
            glyphs: Glyphs::Unicode,
            big_diff: 10_000,
        }
    }
}

/// A line of bytes as the document shows it: lossily, a carriage return
/// at its end left out, other control characters but the tab shown as
/// `^X`, so that the document's lines are the diff's.
pub fn lossy_line(bytes: &[u8]) -> String {
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    let text = String::from_utf8_lossy(bytes);
    if !text.chars().any(|c| c.is_control() && c != '\t') {
        return text.into_owned();
    }
    text.chars()
        .flat_map(|c| -> Vec<char> {
            if c.is_control() && c != '\t' {
                let caret = char::from_u32((c as u32 ^ 0x40) & 0x7f).unwrap_or('?');
                vec!['^', caret]
            } else {
                vec![c]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_shown_safely() {
        assert_eq!(lossy_line(b"a\tb\r"), "a\tb");
        assert_eq!(lossy_line(b"x\ry\x1b[0m"), "x^My^[[0m");
        assert_eq!(lossy_line(b"\xe9t\xc3\xa9"), "\u{fffd}té");
    }
}
