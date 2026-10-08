//! What a command acts on: the thing under the cursor in a git document,
//! named by its region's key (DESIGN.md, appendix D), or the lines a
//! selection covers.

use crate::content::Content;
use crate::model::Section;

/// The thing a command acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The `Head:` line.
    Head,
    /// The `Upstream:` line.
    Upstream,
    /// The `Tags:` line.
    Tag,
    /// The line of the operation under way.
    Operation,
    /// A section's heading.
    Section(Section),
    /// A file of a section.
    File {
        /// Its section.
        section: Section,
        /// Its path.
        path: String,
    },
    /// A hunk of a file.
    Hunk {
        /// Its file's section.
        section: Section,
        /// Its file.
        path: String,
        /// Its index among the file's hunks.
        hunk: usize,
    },
    /// Lines of a hunk.
    Lines {
        /// Its file's section.
        section: Section,
        /// Its file.
        path: String,
        /// The hunk's index.
        hunk: usize,
        /// The lines, by index in the hunk.
        lines: Vec<usize>,
    },
    /// A stash.
    Stash(u32),
    /// A commit.
    Commit(String),
    /// A local branch, by its name.
    Branch(String),
    /// A changed path in the Files panel, staged or not.
    Path(String),
    /// A folder of the Files panel's tree.
    Dir(String),
    /// A tab's name in the tabs' line, by its key (`files`).
    Tab(String),
    /// The line of keys at the top.
    Help,
}

impl Target {
    /// The region key it is written under; lines are their hunk's.
    pub fn key(&self) -> String {
        match self {
            Target::Head => "head".into(),
            Target::Upstream => "upstream".into(),
            Target::Tag => "tag".into(),
            Target::Operation => "operation".into(),
            Target::Section(s) => format!("section:{}", s.key()),
            Target::File { section, path } => file_key(*section, path),
            Target::Hunk {
                section,
                path,
                hunk,
            }
            | Target::Lines {
                section,
                path,
                hunk,
                ..
            } => hunk_key(*section, *hunk, path),
            Target::Stash(n) => format!("stash:{n}"),
            Target::Commit(h) => format!("commit:{h}"),
            Target::Branch(b) => format!("branch:{b}"),
            Target::Path(p) => path_key(p),
            Target::Dir(d) => format!("dir:{d}"),
            Target::Tab(t) => format!("tab:{t}"),
            Target::Help => "help".into(),
        }
    }

    /// The thing a region key names. A large diff's line (`big:`) names
    /// its file.
    pub fn from_key(key: &str) -> Option<Target> {
        let (kind, rest) = key.split_once(':').unwrap_or((key, ""));
        Some(match kind {
            "head" => Target::Head,
            "upstream" => Target::Upstream,
            "tag" => Target::Tag,
            "operation" => Target::Operation,
            "section" => Target::Section(Section::from_key(rest)?),
            "file" | "big" => {
                let (s, path) = rest.split_once(':')?;
                Target::File {
                    section: Section::from_key(s)?,
                    path: path.to_string(),
                }
            }
            "hunk" => {
                let mut f = rest.splitn(3, ':');
                let (s, n, path) = (f.next()?, f.next()?, f.next()?);
                Target::Hunk {
                    section: Section::from_key(s)?,
                    path: path.to_string(),
                    hunk: n.parse().ok()?,
                }
            }
            "stash" => Target::Stash(rest.parse().ok()?),
            "commit" => Target::Commit(rest.to_string()),
            "branch" => Target::Branch(rest.to_string()),
            "path" => Target::Path(rest.to_string()),
            "dir" => Target::Dir(rest.to_string()),
            "tab" => Target::Tab(rest.to_string()),
            "help" => Target::Help,
            _ => return None,
        })
    }

    /// The file it is in, with its section.
    pub fn file(&self) -> Option<(Section, &str)> {
        match self {
            Target::File { section, path }
            | Target::Hunk { section, path, .. }
            | Target::Lines { section, path, .. } => Some((*section, path)),
            _ => None,
        }
    }
}

/// The key of a file's region.
pub fn file_key(section: Section, path: &str) -> String {
    format!("file:{}:{path}", section.key())
}

/// The key of a changed path's region in the Files panel.
pub fn path_key(path: &str) -> String {
    format!("path:{path}")
}

/// The key of a hunk's region; the index before the path, which may hold
/// a colon.
pub fn hunk_key(section: Section, hunk: usize, path: &str) -> String {
    format!("hunk:{}:{hunk}:{path}", section.key())
}

/// The thing under the cursor at `offset`: the innermost region's. Inside
/// a hunk it is the hunk; [`line_in_hunk`] says which line.
pub fn at(content: &Content, offset: usize) -> Option<Target> {
    content
        .regions_at(offset)
        .iter()
        .rev()
        .find_map(|r| Target::from_key(&r.key))
}

/// The line of its hunk the cursor at `offset` is on, by index in the
/// hunk's lines; none on the `@@` line or outside a hunk.
pub fn line_in_hunk(content: &Content, offset: usize) -> Option<usize> {
    let r = content.region_at(offset)?;
    if !r.key.starts_with("hunk:") {
        return None;
    }
    let n = content
        .line_of(offset)
        .checked_sub(content.line_of(r.start))?;
    n.checked_sub(1)
}

/// What a selection from `start` to `end` (left out) covers: the lines of
/// each hunk it touches, the files whose line it covers and whose diff it
/// does not touch, the stashes and commits whose line it covers.
pub fn in_selection(content: &Content, start: usize, end: usize) -> Vec<Target> {
    let (start, end) = (start.min(end), start.max(end));
    let first = content.line_of(start);
    let last = content.line_of(end.saturating_sub(1).max(start));
    let mut v: Vec<Target> = Vec::new();
    for r in &content.regions {
        let head = content.line_of(r.start);
        let tail = content.line_of(r.end.saturating_sub(1).max(r.start));
        let Some(t) = Target::from_key(&r.key) else {
            continue;
        };
        match t {
            Target::Hunk {
                section,
                path,
                hunk,
            } => {
                let (a, b) = (first.max(head + 1), last.min(tail));
                if a <= b {
                    v.push(Target::Lines {
                        section,
                        path,
                        hunk,
                        lines: (a - head - 1..=b - head - 1).collect(),
                    });
                }
            }
            Target::File { .. } | Target::Stash(_) | Target::Commit(_)
                if (first..=last).contains(&head) =>
            {
                v.push(t);
            }
            _ => {}
        }
    }
    // A file whose lines are taken is acted on by them.
    let with_lines: Vec<(Section, String)> = v
        .iter()
        .filter_map(|t| match t {
            Target::Lines { section, path, .. } => Some((*section, path.clone())),
            _ => None,
        })
        .collect();
    v.retain(|t| match t {
        Target::File { section, path } => !with_lines.contains(&(*section, path.clone())),
        _ => true,
    });
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Style;

    #[test]
    fn keys_round_trip() {
        for t in [
            Target::Head,
            Target::Section(Section::Staged),
            Target::File {
                section: Section::Unstaged,
                path: "a:b/c d".into(),
            },
            Target::Hunk {
                section: Section::Staged,
                path: "x:y".into(),
                hunk: 3,
            },
            Target::Stash(2),
            Target::Commit("abc".into()),
            Target::Branch("feature/a:b".into()),
            Target::Path("a:b/c d".into()),
            Target::Dir("crates/x".into()),
            Target::Help,
        ] {
            assert_eq!(Target::from_key(&t.key()), Some(t));
        }
        assert_eq!(
            Target::from_key("big:unstaged:p"),
            Some(Target::File {
                section: Section::Unstaged,
                path: "p".into()
            })
        );
    }

    fn content() -> Content {
        let mut c = Content::new();
        c.open("section:unstaged", true, false);
        c.line("Unstaged changes (2)", Style::Heading);
        c.open(file_key(Section::Unstaged, "a"), true, false);
        c.line("v modified a", Style::Normal);
        c.open(hunk_key(Section::Unstaged, 0, "a"), true, false);
        c.line("@@ -1,3 +1,3 @@", Style::Normal);
        c.line(" one", Style::Normal);
        c.line("-two", Style::Normal);
        c.line("+TWO", Style::Normal);
        c.line(" three", Style::Normal);
        c.close();
        c.close();
        c.open(file_key(Section::Unstaged, "b"), true, true);
        c.line("> modified b", Style::Normal);
        c.close();
        c.close();
        c
    }

    #[test]
    fn the_cursor_names_the_innermost_thing() {
        let c = content();
        let two = c.text.find("-two").unwrap();
        assert_eq!(
            at(&c, two),
            Some(Target::Hunk {
                section: Section::Unstaged,
                path: "a".into(),
                hunk: 0
            })
        );
        assert_eq!(line_in_hunk(&c, two), Some(1));
        assert_eq!(line_in_hunk(&c, c.text.find("@@").unwrap()), None);
        assert_eq!(at(&c, 0), Some(Target::Section(Section::Unstaged)));
    }

    #[test]
    fn a_selection_takes_lines_and_files() {
        let c = content();
        let s = c.text.find("-two").unwrap();
        let e = c.text.find(" three").unwrap();
        assert_eq!(
            in_selection(&c, s, e),
            [Target::Lines {
                section: Section::Unstaged,
                path: "a".into(),
                hunk: 0,
                lines: vec![1, 2]
            }]
        );
        // From file a's line to file b's line: a's lines are all taken,
        // b is taken whole.
        let s = c.text.find("v modified a").unwrap();
        let e = c.text.find("> modified b").unwrap() + 3;
        let v = in_selection(&c, s, e);
        assert_eq!(v.len(), 2);
        assert!(matches!(&v[0], Target::Lines { lines, .. } if lines == &[0, 1, 2, 3]));
        assert!(matches!(&v[1], Target::File { path, .. } if path == "b"));
    }
}
