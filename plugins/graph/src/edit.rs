//! Editing a note as an outliner (`graph_todo.md` GR8a): what a command
//! does to the text, as replacements of byte ranges. A block moves,
//! indents and outdents with the blocks under it; its task keyword
//! cycles, its priority, schedule and deadline are written as Logseq
//! writes them (Org's own spelling in Org pages); `collapsed:: true`
//! folds it, the layer hiding its children. Every other byte of the note
//! stays.
//!
//! Pure: the text and the cursor in, the edits out; the component applies
//! them through Kalem's `editor` as one undo step.

use crate::date::Date;
use crate::scan::{self, Block, Flavor, Scanned};

/// A replacement: bytes `start` to `end` of the text become `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// From.
    pub start: usize,
    /// To (left out).
    pub end: usize,
    /// By.
    pub text: String,
}

/// What a command does: its edits, in order and not overlapping, and the
/// line (from 0, after the edits) the cursor goes to, when it moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The edits.
    pub edits: Vec<Edit>,
    /// The cursor's line afterwards.
    pub line: Option<u32>,
}

/// The starts of the lines of `text`, and the text's length after them.
fn starts(text: &str) -> Vec<usize> {
    let mut out = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            out.push(i + 1);
        }
    }
    out
}

/// The byte where line `n` starts (the text's end past the last).
fn line_start(starts: &[usize], text: &str, n: u32) -> usize {
    starts.get(n as usize).copied().unwrap_or(text.len())
}

/// The line holding byte `at`.
fn line_of(starts: &[usize], at: usize) -> u32 {
    (starts.partition_point(|s| *s <= at).saturating_sub(1)) as u32
}

/// The block at line `line`: the one whose own lines hold it.
pub fn block_at(s: &Scanned, line: u32) -> Option<usize> {
    s.blocks
        .iter()
        .rposition(|b| b.line <= line && line < b.end.max(b.line + 1))
}

/// Block `i` with the blocks under it: its first line and the line after
/// its last descendant's.
pub fn subtree(blocks: &[Block], i: usize) -> (u32, u32) {
    let mut end = blocks[i].end.max(blocks[i].line + 1);
    for b in &blocks[i + 1..] {
        let mut p = b.parent;
        let mut under = false;
        while let Some(x) = p {
            if x == i {
                under = true;
                break;
            }
            p = blocks[x].parent;
        }
        if !under {
            break;
        }
        end = end.max(b.end.max(b.line + 1));
    }
    (blocks[i].line, end)
}

/// The note scanned, and the line and block of the cursor.
fn at(text: &str, flavor: Flavor, cursor: usize) -> Option<(Scanned, Vec<usize>, usize)> {
    let s = scan::scan(text, flavor, &scan::Options::default());
    let st = starts(text);
    let i = block_at(&s, line_of(&st, cursor.min(text.len())))?;
    Some((s, st, i))
}

/// The bytes of block `i`'s first line after its bullet or stars: where
/// a keyword is written.
fn content_start(text: &str, st: &[usize], b: &Block, flavor: Flavor) -> usize {
    let start = line_start(st, text, b.line);
    let line = &text[start..line_start(st, text, b.line + 1).min(text.len())];
    let n = match flavor {
        Flavor::LogseqOrg => {
            let stars = line.bytes().take_while(|c| *c == b'*').count();
            stars + line[stars..].bytes().take_while(|c| *c == b' ').count()
        }
        _ => {
            let ws = line
                .bytes()
                .take_while(|c| *c == b' ' || *c == b'\t')
                .count();
            let rest = &line[ws..];
            let marker = match rest.as_bytes().first() {
                Some(b'-' | b'*' | b'+') if rest.len() == 1 || rest.as_bytes()[1] == b' ' => {
                    2.min(rest.len())
                }
                Some(c) if c.is_ascii_digit() => {
                    let d = rest.bytes().take_while(u8::is_ascii_digit).count();
                    if matches!(rest.as_bytes().get(d), Some(b'.' | b')')) {
                        d + 2
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            ws + marker.min(rest.len())
        }
    };
    start + n
}

/// The next keyword of the task cycle: none, the workflow's first, its
/// second, done, none. Logseq's `:now` workflow is `["LATER", "NOW",
/// "DONE"]`, `:todo` `["TODO", "DOING", "DONE"]`.
fn next_marker(current: Option<&str>, cycle: [&str; 3]) -> Option<&'static str> {
    // Logseq's cycle: each keyword to its own next, whatever the
    // workflow; a block without one, or WAITING or CANCELED, to the
    // workflow's first.
    match current {
        Some("TODO") => Some("DOING"),
        Some("LATER") => Some("NOW"),
        Some("NOW" | "DOING" | "IN-PROGRESS" | "STARTED") => Some("DONE"),
        Some("DONE") => None,
        _ if cycle[0] == "LATER" => Some("LATER"),
        _ => Some("TODO"),
    }
}

/// The task keyword of the block at `cursor` cycled. In an Obsidian note,
/// its check box: none, `[ ]`, `[x]`, none.
pub fn cycle_todo(text: &str, flavor: Flavor, cycle: [&str; 3], cursor: usize) -> Option<Change> {
    let (s, st, i) = at(text, flavor, cursor)?;
    let b = &s.blocks[i];
    let c = content_start(text, &st, b, flavor);
    let rest = &text[c..];
    if flavor == Flavor::Obsidian {
        let (end, new) = if rest.starts_with("[ ] ") {
            (c + 4, "[x] ")
        } else if rest.starts_with("[x] ") || rest.starts_with("[X] ") {
            (c + 4, "")
        } else if rest.starts_with("[/] ") || rest.starts_with("[-] ") {
            (c + 4, "[x] ")
        } else {
            (c, "[ ] ")
        };
        if c == line_start(&st, text, b.line) {
            // A paragraph, not a list item: made a task item.
            return Some(Change {
                edits: vec![Edit {
                    start: c,
                    end: c,
                    text: "- [ ] ".into(),
                }],
                line: None,
            });
        }
        return Some(Change {
            edits: vec![Edit {
                start: c,
                end,
                text: new.into(),
            }],
            line: None,
        });
    }
    let current = b.marker.as_deref();
    let len = current.map_or(0, |m| {
        let after = &rest[m.len()..];
        m.len() + usize::from(after.starts_with(' '))
    });
    let new = next_marker(current, cycle)
        .map(|m| format!("{m} "))
        .unwrap_or_default();
    Some(Change {
        edits: vec![Edit {
            start: c,
            end: c + len,
            text: new,
        }],
        line: None,
    })
}

/// The priority of the block at `cursor` set (`None` removes it): `[#A]`
/// after the task keyword.
pub fn set_priority(
    text: &str,
    flavor: Flavor,
    cursor: usize,
    priority: Option<char>,
) -> Option<Change> {
    if flavor == Flavor::Obsidian {
        return None;
    }
    let (s, st, i) = at(text, flavor, cursor)?;
    let b = &s.blocks[i];
    let mut c = content_start(text, &st, b, flavor);
    if let Some(m) = &b.marker {
        c += m.len();
        if text[c..].starts_with(' ') {
            c += 1;
        }
    }
    let rest = &text[c..];
    let old = if rest.len() >= 4 && rest.starts_with("[#") && rest.as_bytes()[3] == b']' {
        4 + usize::from(rest[4..].starts_with(' '))
    } else {
        0
    };
    let new = priority.map(|p| format!("[#{p}] ")).unwrap_or_default();
    Some(Change {
        edits: vec![Edit {
            start: c,
            end: c + old,
            text: new,
        }],
        line: None,
    })
}

/// The weekday's three letters, as Org writes a timestamp.
fn weekday(d: Date) -> &'static str {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][usize::from(d.weekday())]
}

/// The block's continuation indentation: its first line's, and two spaces
/// past its bullet (Logseq's), or none under an Org headline.
fn continuation(text: &str, st: &[usize], b: &Block, flavor: Flavor) -> String {
    if flavor == Flavor::LogseqOrg {
        return String::new();
    }
    let start = line_start(st, text, b.line);
    let line = &text[start..];
    let ws: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    format!("{ws}  ")
}

/// `SCHEDULED:` (`key`) or `DEADLINE:` of the block at `cursor` set to
/// `date`, or removed: the line replaced where it is, else written after
/// the block's first line and properties.
pub fn set_date(
    text: &str,
    flavor: Flavor,
    cursor: usize,
    key: &str,
    date: Option<Date>,
) -> Option<Change> {
    if flavor == Flavor::Obsidian {
        return None;
    }
    let (s, st, i) = at(text, flavor, cursor)?;
    let b = &s.blocks[i];
    let indent = continuation(text, &st, b, flavor);
    let stamp = date.map(|d| format!("{key}: <{} {}>", d.iso(), weekday(d)));
    // An existing line of the key, among the block's own lines.
    for n in b.line + 1..b.end {
        let ls = line_start(&st, text, n);
        let le = line_start(&st, text, n + 1);
        let line = &text[ls..le];
        let t = line.trim_start();
        if let Some(p) = t.find(&format!("{key}:")) {
            let col = ls + (line.len() - t.len()) + p;
            // To the end of the timestamp `<…>`.
            let end = text[col..le]
                .find('>')
                .map_or(le.saturating_sub(usize::from(line.ends_with('\n'))), |q| {
                    col + q + 1
                });
            let edit = match &stamp {
                Some(s) => Edit {
                    start: col,
                    end,
                    text: s.clone(),
                },
                None => {
                    // The line goes when it held only this.
                    let other = text[ls..col].trim().is_empty() && text[end..le].trim().is_empty();
                    if other {
                        Edit {
                            start: ls,
                            end: le,
                            text: String::new(),
                        }
                    } else {
                        Edit {
                            start: col,
                            end,
                            text: String::new(),
                        }
                    }
                }
            };
            return Some(Change {
                edits: vec![edit],
                line: None,
            });
        }
    }
    let stamp = stamp?;
    // After the first line and the property lines (Logseq), or right
    // after the headline (Org, whose planning line comes first).
    let mut n = b.line + 1;
    if flavor == Flavor::LogseqMarkdown {
        while n < b.end {
            let ls = line_start(&st, text, n);
            let le = line_start(&st, text, n + 1);
            let t = text[ls..le].trim();
            let prop = t.find("::").is_some_and(|p| p > 0 && !t[..p].contains(' '));
            let planning = t.starts_with("SCHEDULED:") || t.starts_with("DEADLINE:");
            if !(prop || planning) {
                break;
            }
            n += 1;
        }
    }
    let at = line_start(&st, text, n);
    let needs_feed = at == text.len() && !text.ends_with('\n') && !text.is_empty();
    let text_out = if needs_feed {
        format!("\n{indent}{stamp}")
    } else {
        format!("{indent}{stamp}\n")
    };
    Some(Change {
        edits: vec![Edit {
            start: at,
            end: at,
            text: text_out,
        }],
        line: None,
    })
}

/// The bytes of lines `a` to `b`, the last one ending with a line feed.
fn chunk(text: &str, st: &[usize], a: u32, b: u32) -> String {
    let s = &text[line_start(st, text, a)..line_start(st, text, b)];
    if s.ends_with('\n') || s.is_empty() {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// The block at `cursor` with the blocks under it moved above its
/// previous sibling (`up`) or below its next one.
pub fn move_block(text: &str, flavor: Flavor, cursor: usize, up: bool) -> Option<Change> {
    let (s, st, i) = at(text, flavor, cursor)?;
    let blocks = &s.blocks;
    let b = &blocks[i];
    let (start, end) = subtree(blocks, i);
    let sibling = |j: &usize| blocks[*j].parent == b.parent && blocks[*j].depth == b.depth;
    let (first, second) = if up {
        let j = (0..i).rev().find(sibling)?;
        let (ps, pe) = subtree(blocks, j);
        if pe != start {
            return None;
        }
        ((ps, pe), (start, end))
    } else {
        let j = (i + 1..blocks.len()).find(sibling)?;
        let (ns, ne) = subtree(blocks, j);
        if ns != end {
            return None;
        }
        ((start, end), (ns, ne))
    };
    // The text's end without a line feed: the swapped blocks keep that.
    let whole_end = line_start(&st, text, second.1);
    let ends_bare = whole_end == text.len() && !text.ends_with('\n');
    let mut swapped = chunk(text, &st, second.0, second.1) + &chunk(text, &st, first.0, first.1);
    if ends_bare {
        swapped.pop();
    }
    let moved_line = if up {
        first.0
    } else {
        first.0 + (second.1 - second.0)
    };
    Some(Change {
        edits: vec![Edit {
            start: line_start(&st, text, first.0),
            end: whole_end,
            text: swapped,
        }],
        line: Some(moved_line),
    })
}

/// One step of indentation in the note: a tab where its lists use tabs,
/// else as many spaces as a child is indented past its parent, else a
/// tab.
fn unit(text: &str, s: &Scanned, st: &[usize]) -> String {
    for b in &s.blocks {
        if let Some(p) = b.parent {
            let pl = &text[line_start(st, text, s.blocks[p].line)..];
            let cl = &text[line_start(st, text, b.line)..];
            let pw: String = pl.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            let cw: String = cl.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            if let Some(extra) = cw.strip_prefix(&pw)
                && !extra.is_empty()
            {
                return if extra.contains('\t') {
                    "\t".into()
                } else {
                    extra.to_string()
                };
            }
        }
    }
    "\t".into()
}

/// The block at `cursor` with the blocks under it one level deeper
/// (`deeper`: under its previous sibling, which it must have) or one
/// level out (its parent must exist); the blocks after it at its level
/// stay where they are, as Logseq outdents.
pub fn shift_block(text: &str, flavor: Flavor, cursor: usize, deeper: bool) -> Option<Change> {
    let (s, st, i) = at(text, flavor, cursor)?;
    let blocks = &s.blocks;
    let b = &blocks[i];
    if deeper {
        (0..i)
            .rev()
            .find(|j| blocks[*j].parent == b.parent && blocks[*j].depth == b.depth)?;
    } else if b.parent.is_none() {
        return None;
    }
    let (start, end) = subtree(blocks, i);
    let mut edits = Vec::new();
    if flavor == Flavor::LogseqOrg {
        for (j, x) in blocks.iter().enumerate() {
            if x.line < start || x.line >= end || x.heading.is_none() {
                continue;
            }
            let _ = j;
            let ls = line_start(&st, text, x.line);
            if deeper {
                edits.push(Edit {
                    start: ls,
                    end: ls,
                    text: "*".into(),
                });
            } else if text[ls..].starts_with("**") {
                edits.push(Edit {
                    start: ls,
                    end: ls + 1,
                    text: String::new(),
                });
            }
        }
    } else {
        let u = unit(text, &s, &st);
        for n in start..end {
            let ls = line_start(&st, text, n);
            let le = line_start(&st, text, n + 1);
            let line = &text[ls..le];
            if line.trim().is_empty() {
                continue;
            }
            if deeper {
                edits.push(Edit {
                    start: ls,
                    end: ls,
                    text: u.clone(),
                });
            } else {
                let remove = if line.starts_with('\t') {
                    1
                } else {
                    line.bytes()
                        .take(u.len().max(1))
                        .take_while(|c| *c == b' ')
                        .count()
                };
                if remove > 0 {
                    edits.push(Edit {
                        start: ls,
                        end: ls + remove,
                        text: String::new(),
                    });
                }
            }
        }
    }
    (!edits.is_empty()).then_some(Change { edits, line: None })
}

/// The block at `cursor` folded or unfolded as Logseq does it: its
/// `collapsed:: true` written after its first line and properties, or
/// taken away. A block without blocks under it does not fold.
pub fn toggle_fold(text: &str, cursor: usize) -> Result<Change, &'static str> {
    let (s, st, i) = at(text, Flavor::LogseqMarkdown, cursor).ok_or("no block at the cursor")?;
    let b = &s.blocks[i];
    let has_children = s.blocks.get(i + 1).is_some_and(|c| c.parent == Some(i));
    for n in b.line + 1..b.end {
        let ls = line_start(&st, text, n);
        let le = line_start(&st, text, n + 1);
        if text[ls..le]
            .trim_start()
            .to_lowercase()
            .starts_with("collapsed::")
        {
            return Ok(Change {
                edits: vec![Edit {
                    start: ls,
                    end: le,
                    text: String::new(),
                }],
                line: None,
            });
        }
    }
    if !has_children {
        return Err("the block has no blocks under it");
    }
    let mut n = b.line + 1;
    while n < b.end {
        let ls = line_start(&st, text, n);
        let le = line_start(&st, text, n + 1);
        let t = text[ls..le].trim();
        if !t.find("::").is_some_and(|p| p > 0 && !t[..p].contains(' ')) {
            break;
        }
        n += 1;
    }
    let at = line_start(&st, text, n);
    Ok(Change {
        edits: vec![Edit {
            start: at,
            end: at,
            text: format!(
                "{}collapsed:: true\n",
                continuation(text, &st, b, Flavor::LogseqMarkdown)
            ),
        }],
        line: None,
    })
}

/// The line `key:: value` written into the block whose first line is
/// `line`, after that line and its properties, as Logseq writes a
/// block's `id::`; `None` when no block starts there.
pub fn add_property(text: &str, line: u32, key: &str, value: &str) -> Option<Edit> {
    let s = scan::scan(text, Flavor::LogseqMarkdown, &scan::Options::default());
    let st = starts(text);
    let b = s.blocks.iter().find(|b| b.line == line)?;
    let mut n = b.line + 1;
    while n < b.end {
        let ls = line_start(&st, text, n);
        let le = line_start(&st, text, n + 1);
        let t = text[ls..le].trim();
        if !t.find("::").is_some_and(|p| p > 0 && !t[..p].contains(' ')) {
            break;
        }
        n += 1;
    }
    let at = line_start(&st, text, n);
    let indent = continuation(text, &st, b, Flavor::LogseqMarkdown);
    let bare = at == text.len() && !text.is_empty() && !text.ends_with('\n');
    Some(Edit {
        start: at,
        end: at,
        text: if bare {
            format!("\n{indent}{key}:: {value}")
        } else {
            format!("{indent}{key}:: {value}\n")
        },
    })
}

/// `text` with `edits` applied (for the tests and for files written).
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|e| e.start);
    for e in sorted {
        out.push_str(&text[at..e.start]);
        out.push_str(&e.text);
        at = e.end;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: [&str; 3] = ["LATER", "NOW", "DONE"];
    const TODO: [&str; 3] = ["TODO", "DOING", "DONE"];

    fn run(text: &str, f: impl Fn(usize) -> Option<Change>, cursor: usize) -> String {
        apply(text, &f(cursor).expect("a change").edits)
    }

    #[test]
    fn task_keywords_cycle() {
        let t = "- write\n- LATER read\n";
        let c = |t: &str, at| run(t, |c| cycle_todo(t, Flavor::LogseqMarkdown, NOW, c), at);
        assert_eq!(c(t, 2), "- LATER write\n- LATER read\n");
        assert_eq!(c(t, 10), "- write\n- NOW read\n");
        let t = "- NOW [#A] go\n";
        assert_eq!(c(t, 0), "- DONE [#A] go\n");
        assert_eq!(c("- DONE go\n", 0), "- go\n");
        // Each keyword to its own next whatever the workflow, as Logseq
        // cycles them.
        assert_eq!(c("- TODO go\n", 0), "- DOING go\n");
        assert_eq!(c("- WAITING go\n", 0), "- LATER go\n");
        assert_eq!(c("- CANCELED go\n", 0), "- LATER go\n");
        let t = "- go\n";
        assert_eq!(
            run(t, |c| cycle_todo(t, Flavor::LogseqMarkdown, TODO, c), 0),
            "- TODO go\n"
        );
        let t = "* TODO Read\n** Child\n";
        assert_eq!(
            run(t, |c| cycle_todo(t, Flavor::LogseqOrg, TODO, c), 3),
            "* DOING Read\n** Child\n"
        );
        assert_eq!(
            run(t, |c| cycle_todo(t, Flavor::LogseqOrg, TODO, c), 14),
            "* TODO Read\n** TODO Child\n"
        );
        // Obsidian's check boxes.
        let o = |t: &str| run(t, |c| cycle_todo(t, Flavor::Obsidian, TODO, c), 2);
        assert_eq!(o("- item\n"), "- [ ] item\n");
        assert_eq!(o("- [ ] item\n"), "- [x] item\n");
        assert_eq!(o("- [x] item\n"), "- item\n");
        assert_eq!(o("A paragraph\n"), "- [ ] A paragraph\n");
    }

    #[test]
    fn priorities_and_dates() {
        let t = "- TODO go\n  id:: 1\n- next\n";
        let p = |t: &str, x| run(t, |c| set_priority(t, Flavor::LogseqMarkdown, c, x), 3);
        assert_eq!(p(t, Some('A')), "- TODO [#A] go\n  id:: 1\n- next\n");
        assert_eq!(p("- TODO [#A] go\n", Some('B')), "- TODO [#B] go\n");
        assert_eq!(p("- TODO [#A] go\n", None), "- TODO go\n");
        assert_eq!(p("- go\n", Some('C')), "- [#C] go\n");
        let d = Date::new(2026, 10, 12);
        let s = |t: &str, k: &str, x: Option<Date>| {
            run(t, |c| set_date(t, Flavor::LogseqMarkdown, c, k, x), 3)
        };
        // After the properties.
        assert_eq!(
            s(t, "SCHEDULED", d),
            "- TODO go\n  id:: 1\n  SCHEDULED: <2026-10-12 Mon>\n- next\n"
        );
        // Replaced where it is, then removed with its line.
        let t2 = "- TODO go\n  SCHEDULED: <2026-10-01 Thu .+1d>\n- next\n";
        assert_eq!(
            s(t2, "SCHEDULED", d),
            "- TODO go\n  SCHEDULED: <2026-10-12 Mon>\n- next\n"
        );
        assert_eq!(s(t2, "SCHEDULED", None), "- TODO go\n- next\n");
        assert_eq!(
            s(t, "DEADLINE", d),
            "- TODO go\n  id:: 1\n  DEADLINE: <2026-10-12 Mon>\n- next\n"
        );
        // Nested, with a tab; the last line without a line feed.
        let t3 = "- a\n\t- TODO b";
        assert_eq!(
            run(
                t3,
                |c| set_date(t3, Flavor::LogseqMarkdown, c, "SCHEDULED", d),
                6
            ),
            "- a\n\t- TODO b\n\t  SCHEDULED: <2026-10-12 Mon>"
        );
        // Org: right after the headline.
        let o = "* TODO Read\n:PROPERTIES:\n:ID: x\n:END:\n";
        assert_eq!(
            run(o, |c| set_date(o, Flavor::LogseqOrg, c, "SCHEDULED", d), 2),
            "* TODO Read\nSCHEDULED: <2026-10-12 Mon>\n:PROPERTIES:\n:ID: x\n:END:\n"
        );
        assert!(set_date(t, Flavor::Obsidian, 0, "SCHEDULED", d).is_none());
    }

    #[test]
    fn blocks_move_with_their_children() {
        let t = "- a\n\t- a1\n- b\n  id:: x\n\t- b1\n- c\n";
        let up = move_block(t, Flavor::LogseqMarkdown, 13, true).unwrap();
        assert_eq!(
            apply(t, &up.edits),
            "- b\n  id:: x\n\t- b1\n- a\n\t- a1\n- c\n"
        );
        assert_eq!(up.line, Some(0));
        let down = move_block(t, Flavor::LogseqMarkdown, 13, false).unwrap();
        assert_eq!(
            apply(t, &down.edits),
            "- a\n\t- a1\n- c\n- b\n  id:: x\n\t- b1\n"
        );
        assert_eq!(down.line, Some(3));
        // No sibling there.
        assert!(move_block(t, Flavor::LogseqMarkdown, 0, true).is_none());
        assert!(
            move_block(t, Flavor::LogseqMarkdown, 6, true).is_none(),
            "a1 has no sibling"
        );
        // The last line without a line feed.
        let t = "- a\n- b";
        assert_eq!(
            apply(
                t,
                &move_block(t, Flavor::LogseqMarkdown, 5, true)
                    .unwrap()
                    .edits
            ),
            "- b\n- a"
        );
        // Org headlines with their children.
        let o = "* A\n** A1\n* B\n";
        assert_eq!(
            apply(
                o,
                &move_block(o, Flavor::LogseqOrg, 10, true).unwrap().edits
            ),
            "* B\n* A\n** A1\n"
        );
    }

    #[test]
    fn blocks_indent_and_outdent() {
        let t = "- a\n- b\n  id:: x\n\t- b1\n- c\n";
        let i = shift_block(t, Flavor::LogseqMarkdown, 5, true).unwrap();
        assert_eq!(
            apply(t, &i.edits),
            "- a\n\t- b\n\t  id:: x\n\t\t- b1\n- c\n"
        );
        assert!(
            shift_block(t, Flavor::LogseqMarkdown, 0, true).is_none(),
            "no previous sibling"
        );
        let back = apply(t, &i.edits);
        let o = shift_block(&back, Flavor::LogseqMarkdown, 6, false).unwrap();
        assert_eq!(apply(&back, &o.edits), t);
        assert!(
            shift_block(t, Flavor::LogseqMarkdown, 0, false).is_none(),
            "no parent"
        );
        // Spaces as the unit when the note uses them.
        let s = "- a\n  - a1\n  - a2\n";
        let i = shift_block(s, Flavor::LogseqMarkdown, 13, true).unwrap();
        assert_eq!(apply(s, &i.edits), "- a\n  - a1\n    - a2\n");
        // Org: one star more or less on each headline.
        let o = "* A\n* B\n** B1\n";
        assert_eq!(
            apply(
                o,
                &shift_block(o, Flavor::LogseqOrg, 5, true).unwrap().edits
            ),
            "* A\n** B\n*** B1\n"
        );
        let o2 = "* A\n** B\n";
        assert_eq!(
            apply(
                o2,
                &shift_block(o2, Flavor::LogseqOrg, 5, false).unwrap().edits
            ),
            "* A\n* B\n"
        );
    }

    #[test]
    fn folding_writes_collapsed() {
        let t = "- a\n  id:: x\n\t- a1\n- b\n";
        let f = toggle_fold(t, 1).unwrap();
        let folded = apply(t, &f.edits);
        assert_eq!(folded, "- a\n  id:: x\n  collapsed:: true\n\t- a1\n- b\n");
        assert_eq!(apply(&folded, &toggle_fold(&folded, 1).unwrap().edits), t);
        assert!(toggle_fold(t, 22).is_err(), "b has no children");
    }

    #[test]
    fn a_property_written_into_a_block() {
        let t = "- a\n  type:: x\n\t- b\n- c";
        let e = add_property(t, 0, "id", "u1").unwrap();
        assert_eq!(apply(t, &[e]), "- a\n  type:: x\n  id:: u1\n\t- b\n- c");
        let e = add_property(t, 2, "id", "u2").unwrap();
        assert_eq!(apply(t, &[e]), "- a\n  type:: x\n\t- b\n\t  id:: u2\n- c");
        let e = add_property(t, 3, "id", "u3").unwrap();
        assert_eq!(apply(t, &[e]), "- a\n  type:: x\n\t- b\n- c\n  id:: u3");
        assert!(add_property(t, 1, "id", "u").is_none());
    }

    #[test]
    fn a_code_block_moves_whole() {
        let t = "- a\n- ```\n  x\n\n  ```\n- b\n";
        let down = move_block(t, Flavor::LogseqMarkdown, 0, false).unwrap();
        assert_eq!(apply(t, &down.edits), "- ```\n  x\n\n  ```\n- a\n- b\n");
    }

    #[test]
    fn the_block_at_a_line() {
        let s = scan::scan(
            "- a\n  id:: x\n\t- b\n",
            Flavor::LogseqMarkdown,
            &scan::Options::default(),
        );
        assert_eq!(block_at(&s, 0), Some(0));
        assert_eq!(block_at(&s, 1), Some(0));
        assert_eq!(block_at(&s, 2), Some(1));
        assert_eq!(subtree(&s.blocks, 0), (0, 3));
    }
}
