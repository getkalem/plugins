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

/// Where block `b`'s task keyword is written: after its bullet or stars,
/// and in Logseq's Markdown after a heading's marks (`- ## LATER Plan`),
/// where Logseq writes it and the scanner reads it.
fn keyword_start(text: &str, st: &[usize], b: &Block, flavor: Flavor) -> usize {
    let c = content_start(text, st, b, flavor);
    if flavor != Flavor::LogseqMarkdown || b.heading.is_none() {
        return c;
    }
    let rest = &text[c..];
    let marks = rest.bytes().take_while(|x| *x == b'#').count();
    if (1..=6).contains(&marks) && rest[marks..].starts_with(' ') {
        c + marks + 1
    } else {
        c
    }
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

/// One level of a note's indentation: the tab Logseq writes, or the
/// spaces its children are indented by when it uses spaces.
fn indent_unit(s: &Scanned, text: &str, st: &[usize]) -> String {
    for b in &s.blocks {
        if b.depth == 1 {
            let start = line_start(st, text, b.line);
            let line = &text[start..];
            let ws: String = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            if ws.starts_with('\t') {
                return "\t".into();
            }
            if !ws.is_empty() {
                return ws;
            }
        }
    }
    "\t".into()
}

/// Enter in a Logseq note, as Logseq does it on a block's first line: at
/// its end a new block (its first child when it has children shown, else
/// its sibling after it and its children), in the middle the block split
/// there (the rest of the line the new block's), at its start an empty
/// block above it, and in an empty child block the block outdented. The
/// block's own lines (its properties, dates and drawers, and its children
/// when folded) stay its own: they go before the new block, written at
/// the cursor so that the cursor is in the new block. `None` elsewhere
/// (a property line, a code block, an Obsidian or Org note): Enter as
/// the mode has it.
pub fn new_block(text: &str, flavor: Flavor, cursor: usize) -> Option<Change> {
    if flavor != Flavor::LogseqMarkdown {
        return None;
    }
    let (s, st, i) = at(text, flavor, cursor)?;
    let b = &s.blocks[i];
    let first_start = line_start(&st, text, b.line);
    let first_end = text[first_start..]
        .find('\n')
        .map_or(text.len(), |p| first_start + p);
    if cursor < first_start || cursor > first_end {
        return None;
    }
    let ws = text[first_start..first_end]
        .bytes()
        .take_while(|c| *c == b' ' || *c == b'\t')
        .count();
    let indent = &text[first_start..first_start + ws];
    let c = content_start(text, &st, b, flavor);
    if c == first_start + ws {
        // No bullet: a page's text before its first block.
        return None;
    }
    let content = &text[c..first_end];
    // Inside a code block opened on the line.
    if content.trim_start().starts_with("```") {
        return None;
    }
    let cur = cursor.max(c);
    let before = &text[c..cur];
    let after = &text[cur..first_end];
    let children = s.blocks.get(i + 1).is_some_and(|n| n.parent == Some(i));
    if before.trim().is_empty() && !after.trim().is_empty() {
        return Some(Change {
            edits: vec![Edit {
                start: first_start,
                end: first_start,
                text: format!("{indent}- \n"),
            }],
            line: None,
        });
    }
    if content.trim().is_empty() && b.depth > 0 && b.end <= b.line + 1 && !children {
        return shift_block(text, flavor, cursor, false);
    }
    let (last, new_indent) = if children && !b.collapsed {
        (b.end, format!("{indent}{}", indent_unit(&s, text, &st)))
    } else if children {
        (subtree(&s.blocks, i).1, indent.to_string())
    } else {
        (b.end, indent.to_string())
    };
    // The lines that stay before the new block, without their last line
    // feed.
    let stop = line_start(&st, text, last.max(b.line + 1))
        .saturating_sub(1)
        .max(first_end)
        .min(text.len());
    let moved = &text[first_end..stop];
    // A split leaves no space at the end of the first part or at the
    // start of the second, as Logseq trims a block's text.
    let trail = before.len() - before.trim_end().len();
    let lead = after.len() - after.trim_start().len();
    let mut edits = Vec::new();
    if trail > 0 && !after.trim().is_empty() {
        edits.push(Edit {
            start: cur - trail,
            end: cur,
            text: String::new(),
        });
    }
    edits.push(Edit {
        start: cur,
        end: cur,
        text: format!("{moved}\n{new_indent}- "),
    });
    if lead > 0 && !after.trim().is_empty() {
        edits.push(Edit {
            start: cur,
            end: cur + lead,
            text: String::new(),
        });
    }
    if !moved.is_empty() {
        edits.push(Edit {
            start: first_end,
            end: stop,
            text: String::new(),
        });
    }
    Some(Change { edits, line: None })
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
    let c = keyword_start(text, &st, b, flavor);
    let rest = &text[c..];
    let current = b.marker;
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

/// The local moment a task's clock is read at: its stamp as Logseq
/// writes it (`2026-10-14 Wed 16:20:11`), and its seconds since 1970.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Now {
    /// `2026-10-14 Wed 16:20:11`.
    pub stamp: String,
    /// Seconds since 1970, of the local time.
    pub secs: i64,
}

impl Now {
    /// The moment `ms` milliseconds after 1970, in local time.
    pub fn at(ms: i64) -> Now {
        const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let secs = ms.div_euclid(1000);
        let d = Date::from_days(secs.div_euclid(86_400));
        let t = secs.rem_euclid(86_400);
        Now {
            stamp: format!(
                "{} {} {:02}:{:02}:{:02}",
                d.iso(),
                DAYS[usize::from(d.weekday())],
                t / 3600,
                t / 60 % 60,
                t % 60
            ),
            secs,
        }
    }
}

/// The seconds since 1970 of a clock's stamp (`2026-10-14 Wed 16:20:11`,
/// or without seconds, as older Logseqs wrote it).
fn stamp_secs(stamp: &str) -> Option<i64> {
    let d = Date::parse_iso(stamp.get(..10)?)?;
    let mut hms = stamp.split_whitespace().last()?.split(':');
    let h: i64 = hms.next()?.parse().ok()?;
    let m: i64 = hms.next()?.parse().ok()?;
    let s: i64 = hms.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some(d.to_days() * 86_400 + h * 3600 + m * 60 + s)
}

/// Logseq's clock (`:feature/enable-timetracking?`) for the block at
/// line `line` whose keyword goes from `old` to `new` at `now`, as Logseq
/// 0.10.9 keeps it: a task started (to NOW or DOING) gets a `CLOCK:` line
/// in its `:LOGBOOK:` (the drawer made after its properties and dates),
/// one stopped (to DONE, or back to LATER or TODO) its open clock closed
/// with the time it ran. `None` when the clock does not change.
pub fn clock(
    text: &str,
    flavor: Flavor,
    line: u32,
    old: Option<&str>,
    new: Option<&str>,
    now: &Now,
) -> Option<Edit> {
    let s = scan::scan(text, flavor, &scan::Options::default());
    let st = starts(text);
    let b = s.blocks.iter().find(|b| b.line == line)?;
    let line_text = |n: u32| {
        let a = line_start(&st, text, n);
        let e = line_start(&st, text, n + 1);
        text[a..e].trim_end_matches(['\n', '\r'])
    };
    let is = |m: Option<&str>, w: &str| m.is_some_and(|m| m.eq_ignore_ascii_case(w));
    let started = |m: Option<&str>| is(m, "NOW") || is(m, "DOING");
    // The block's logbook: its first and last lines.
    let logbook = (b.line + 1..b.end)
        .find(|n| line_text(*n).trim().eq_ignore_ascii_case(":LOGBOOK:"))
        .and_then(|first| {
            (first + 1..b.end)
                .find(|n| line_text(*n).trim().eq_ignore_ascii_case(":END:"))
                .map(|last| (first, last))
        });
    let same = old.zip(new).is_some_and(|(o, n)| o.eq_ignore_ascii_case(n));
    let clock_in = (old.is_none() && started(new))
        || (is(old, "TODO") && is(new, "DOING"))
        || (is(old, "LATER") && is(new, "NOW"))
        || (started(new) && same && logbook.is_none());
    let clock_out = (is(old, "DOING") && is(new, "TODO"))
        || (is(old, "NOW") && is(new, "LATER"))
        || (started(old) && is(new, "DONE"));
    let indent = continuation(text, &st, b, flavor);
    if clock_in {
        let clock = format!("{indent}CLOCK: [{}]\n", now.stamp);
        if let Some((_, last)) = logbook {
            let at = line_start(&st, text, last);
            return Some(Edit {
                start: at,
                end: at,
                text: clock,
            });
        }
        // After the block's properties (Org's drawer) and dates; after its
        // own lines, out of them, when its first line opens a code block,
        // a `#+BEGIN_`, a table or a quote.
        let mut n = b.line + 1;
        if flavor == Flavor::LogseqMarkdown
            && matches!(md_properties(text, &st, b).1, Spot::First(_))
        {
            n = b.end.max(n);
        }
        let mut drawer = false;
        while n < b.end {
            let t = line_text(n).trim();
            if t.eq_ignore_ascii_case(":PROPERTIES:") {
                drawer = true;
            } else if drawer {
                drawer = !t.eq_ignore_ascii_case(":END:");
            } else {
                let property = t.find("::").is_some_and(|p| p > 0 && !t[..p].contains(' '));
                if !(property || t.starts_with("SCHEDULED:") || t.starts_with("DEADLINE:")) {
                    break;
                }
            }
            n += 1;
        }
        let at = line_start(&st, text, n);
        let made = format!("{indent}:LOGBOOK:\n{clock}{indent}:END:\n");
        let bare = at == text.len() && !text.is_empty() && !text.ends_with('\n');
        return Some(Edit {
            start: at,
            end: at,
            text: if bare {
                format!("\n{}", made.trim_end_matches('\n'))
            } else {
                made
            },
        });
    }
    if clock_out {
        let (first, last) = logbook?;
        // The logbook's last clock, when still open.
        let n = (first + 1..last)
            .rev()
            .find(|n| line_text(*n).trim().starts_with("CLOCK:"))?;
        let whole = line_text(n);
        let t = whole.trim();
        if t.contains("--") {
            return None;
        }
        let begun = t.strip_prefix("CLOCK: [")?.strip_suffix(']')?;
        let span = (now.secs - stamp_secs(begun)?).max(0);
        let lead = &whole[..whole.len() - whole.trim_start().len()];
        let a = line_start(&st, text, n);
        return Some(Edit {
            start: a,
            end: a + whole.len(),
            text: format!(
                "{lead}CLOCK: [{begun}]--[{}] =>  {:02}:{:02}:{:02}",
                now.stamp,
                span / 3600,
                span / 60 % 60,
                span % 60
            ),
        });
    }
    None
}

/// [`cycle_todo`], and Logseq's clock kept as it does when `now` is given
/// (the graph tracks time): a task started or stopped clocked
/// ([`clock`]).
pub fn cycle_todo_clocked(
    text: &str,
    flavor: Flavor,
    cycle: [&str; 3],
    cursor: usize,
    now: Option<&Now>,
) -> Option<Change> {
    let mut change = cycle_todo(text, flavor, cycle, cursor)?;
    if let Some(now) = now.filter(|_| flavor != Flavor::Obsidian) {
        let (s, _, i) = at(text, flavor, cursor)?;
        let b = &s.blocks[i];
        let new = next_marker(b.marker, cycle);
        if let Some(e) = clock(text, flavor, b.line, b.marker, new, now) {
            change.edits.push(e);
        }
    }
    Some(change)
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
    let mut c = keyword_start(text, &st, b, flavor);
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

/// Whether line `t` is a `key:: value` property.
fn is_property(t: &str) -> bool {
    let t = t.trim();
    t.find("::").is_some_and(|p| p > 0 && !t[..p].contains(' '))
}

/// Where a new property of a Logseq block in Markdown is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spot {
    /// Before this line: the indentation, `key:: value` and a line feed.
    Line(u32),
    /// At this byte, where the block's first line's content starts:
    /// `key:: value`, a line feed and the indentation, the content going
    /// on under it.
    First(usize),
}

/// Block `b`'s property lines in Logseq's Markdown and where a new one
/// goes: the lines after its first that are properties (its first line
/// too when it is one, as Logseq writes it before a code block), then the
/// line after them. When its first line opens a code block, a `#+BEGIN_`,
/// a table or a quote, which Logseq gives no title, a property goes
/// before it, as the block's first line, as Logseq writes it there.
fn md_properties(text: &str, st: &[usize], b: &Block) -> (Vec<u32>, Spot) {
    let first = line_start(st, text, b.line);
    let c = content_start(text, st, b, Flavor::LogseqMarkdown);
    let content = &text[c..line_start(st, text, b.line + 1).min(text.len())];
    let ws = text[first..].len() - text[first..].trim_start_matches([' ', '\t']).len();
    let bullet = c > first + ws;
    if bullet && scan::opens_body(content) {
        return (Vec::new(), Spot::First(c));
    }
    let mut lines = Vec::new();
    if bullet && is_property(content) {
        lines.push(b.line);
    }
    let mut n = b.line + 1;
    while n < b.end && is_property(&text[line_start(st, text, n)..line_start(st, text, n + 1)]) {
        lines.push(n);
        n += 1;
    }
    (lines, Spot::Line(n))
}

/// The edit writing `line` (`key:: value`) as a new property of block `b`
/// at `spot`.
fn property_edit(text: &str, st: &[usize], b: &Block, spot: Spot, line: &str) -> Edit {
    let indent = continuation(text, st, b, Flavor::LogseqMarkdown);
    match spot {
        Spot::First(c) => Edit {
            start: c,
            end: c,
            text: format!("{line}\n{indent}"),
        },
        Spot::Line(n) => {
            let at = line_start(st, text, n);
            let bare = at == text.len() && !text.is_empty() && !text.ends_with('\n');
            Edit {
                start: at,
                end: at,
                text: if bare {
                    format!("\n{indent}{line}")
                } else {
                    format!("{indent}{line}\n")
                },
            }
        }
    }
}

/// The edit taking property line `n` of block `b` away: the line, or, on
/// its first line, the property and the next line's indentation, that
/// line its first then (`- collapsed:: true` before a code block).
fn property_removed(text: &str, st: &[usize], b: &Block, n: u32) -> Edit {
    let ls = line_start(st, text, n);
    let le = line_start(st, text, n + 1);
    if n != b.line {
        return Edit {
            start: ls,
            end: le,
            text: String::new(),
        };
    }
    let c = content_start(text, st, b, Flavor::LogseqMarkdown);
    let end = if n + 1 < b.end {
        le + text[le..].len() - text[le..].trim_start_matches([' ', '\t']).len()
    } else {
        text[c..le].trim_end_matches(['\n', '\r']).len() + c
    };
    Edit {
        start: c,
        end,
        text: String::new(),
    }
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
    // after the headline (Org, whose planning line comes first). A block
    // whose first line opens a code block, a `#+BEGIN_`, a table or a
    // quote has it after its own lines, out of them, where Logseq reads
    // a date line as the block's too.
    let mut n = b.line + 1;
    let body =
        flavor == Flavor::LogseqMarkdown && matches!(md_properties(text, &st, b).1, Spot::First(_));
    if body {
        n = b.end.max(n);
    } else if flavor == Flavor::LogseqMarkdown {
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
/// `collapsed:: true` written after its first line and properties (as its
/// first line before a code block, [`md_properties`]), or taken away. A
/// block without blocks under it does not fold.
pub fn toggle_fold(text: &str, cursor: usize) -> Result<Change, &'static str> {
    let (s, st, i) = at(text, Flavor::LogseqMarkdown, cursor).ok_or("no block at the cursor")?;
    let b = &s.blocks[i];
    let has_children = s.blocks.get(i + 1).is_some_and(|c| c.parent == Some(i));
    let c = content_start(text, &st, b, Flavor::LogseqMarkdown);
    for n in b.line..b.end {
        // Its first line from its content: `- collapsed:: true` before a
        // code block.
        let ls = if n == b.line {
            c
        } else {
            line_start(&st, text, n)
        };
        let le = line_start(&st, text, n + 1);
        if text[ls..le]
            .trim_start()
            .to_lowercase()
            .starts_with("collapsed::")
        {
            return Ok(Change {
                edits: vec![property_removed(text, &st, b, n)],
                line: None,
            });
        }
    }
    if !has_children {
        return Err("the block has no blocks under it");
    }
    let (_, spot) = md_properties(text, &st, b);
    Ok(Change {
        edits: vec![property_edit(text, &st, b, spot, "collapsed:: true")],
        line: None,
    })
}

/// The line `key:: value` written into the block whose first line is
/// `line`, after that line and its properties, as Logseq writes a
/// block's `id::` (as its first line before a code block,
/// [`md_properties`]); `None` when no block starts there.
pub fn add_property(text: &str, line: u32, key: &str, value: &str) -> Option<Edit> {
    let s = scan::scan(text, Flavor::LogseqMarkdown, &scan::Options::default());
    let st = starts(text);
    let b = s.blocks.iter().find(|b| b.line == line)?;
    let (_, spot) = md_properties(text, &st, b);
    Some(property_edit(
        text,
        &st,
        b,
        spot,
        &format!("{key}:: {value}"),
    ))
}

/// A block's id written as its note writes one: `id:: …` after a
/// Markdown block's first line and properties, `:ID: …` in an Org
/// headline's `:PROPERTIES:` drawer (made after its dates when it has
/// none), as Logseq writes them.
pub fn add_id(text: &str, flavor: Flavor, line: u32, id: &str) -> Option<Edit> {
    if flavor != Flavor::LogseqOrg {
        return add_property(text, line, "id", id);
    }
    let s = scan::scan(text, flavor, &scan::Options::default());
    let st = starts(text);
    let b = s.blocks.iter().find(|b| b.line == line)?;
    let line_text = |n: u32| {
        let a = line_start(&st, text, n);
        let e = line_start(&st, text, n + 1);
        text[a..e].trim()
    };
    // Its drawer, when it has one.
    let drawer = (b.line + 1..b.end)
        .find(|n| line_text(*n).eq_ignore_ascii_case(":PROPERTIES:"))
        .and_then(|first| (first + 1..b.end).find(|n| line_text(*n).eq_ignore_ascii_case(":END:")));
    if let Some(end) = drawer {
        let at = line_start(&st, text, end);
        return Some(Edit {
            start: at,
            end: at,
            text: format!(":ID: {id}\n"),
        });
    }
    let mut n = b.line + 1;
    while n < b.end && {
        let t = line_text(n);
        t.starts_with("SCHEDULED:") || t.starts_with("DEADLINE:") || t.starts_with("CLOSED:")
    } {
        n += 1;
    }
    let at = line_start(&st, text, n);
    let made = format!(":PROPERTIES:\n:ID: {id}\n:END:\n");
    let bare = at == text.len() && !text.is_empty() && !text.ends_with('\n');
    Some(Edit {
        start: at,
        end: at,
        text: if bare {
            format!("\n{}", made.trim_end_matches('\n'))
        } else {
            made
        },
    })
}

/// Properties `props` of the block at line `line` set as its note writes
/// them: a property it has written again in place, one it has not added
/// after its others (Markdown's `key:: value` after the block's first
/// line, or as its first before a code block, [`md_properties`]; Org's
/// `:key: value` in the headline's drawer, made after its dates when it
/// has none).
pub fn set_properties(
    text: &str,
    flavor: Flavor,
    line: u32,
    props: &[(&str, String)],
) -> Option<Vec<Edit>> {
    let s = scan::scan(text, flavor, &scan::Options::default());
    let st = starts(text);
    let b = s.blocks.iter().find(|b| b.line == line)?;
    // A line's text; the block's first from its content, after the
    // bullet.
    let content = content_start(text, &st, b, flavor);
    let from = |n: u32| {
        if n == b.line && flavor != Flavor::LogseqOrg {
            content
        } else {
            line_start(&st, text, n)
        }
    };
    let line_text = |n: u32| {
        let e = line_start(&st, text, n + 1);
        text[from(n)..e].trim_end_matches(['\n', '\r'])
    };
    let org = flavor == Flavor::LogseqOrg;
    // The lines its properties are on, and where a new one goes.
    let (lines, at, made): (Vec<u32>, Spot, bool) = if org {
        let first =
            (b.line + 1..b.end).find(|n| line_text(*n).trim().eq_ignore_ascii_case(":PROPERTIES:"));
        let end = first.and_then(|f| {
            (f + 1..b.end).find(|n| line_text(*n).trim().eq_ignore_ascii_case(":END:"))
        });
        match (first, end) {
            (Some(f), Some(e)) => ((f + 1..e).collect(), Spot::Line(e), false),
            _ => {
                let mut n = b.line + 1;
                while n < b.end && {
                    let t = line_text(n).trim();
                    t.starts_with("SCHEDULED:") || t.starts_with("DEADLINE:")
                } {
                    n += 1;
                }
                (Vec::new(), Spot::Line(n), true)
            }
        }
    } else {
        let (lines, spot) = md_properties(text, &st, b);
        (lines, spot, false)
    };
    let key_of = |n: u32| -> Option<String> {
        let t = line_text(n).trim();
        if org {
            let rest = t.strip_prefix(':')?;
            Some(rest[..rest.find(':')?].to_lowercase())
        } else {
            Some(t[..t.find("::")?].trim().to_lowercase())
        }
    };
    let indent = if org {
        String::new()
    } else {
        continuation(text, &st, b, flavor)
    };
    let written = |k: &str, v: &str| {
        if org {
            format!(":{k}: {v}")
        } else {
            format!("{k}:: {v}")
        }
    };
    let mut edits = Vec::new();
    let mut added = Vec::new();
    for (k, v) in props {
        match lines.iter().find(|n| key_of(**n).as_deref() == Some(*k)) {
            Some(n) => {
                let a = from(*n);
                // The block's indentation, but on its first line, after its
                // bullet.
                let lead = if *n == b.line && !org {
                    ""
                } else {
                    indent.as_str()
                };
                edits.push(Edit {
                    start: a,
                    end: a + line_text(*n).len(),
                    text: format!("{lead}{}", written(k, v)),
                });
            }
            None => added.push(written(k, v)),
        }
    }
    if added.is_empty() {
        return Some(edits);
    }
    match at {
        Spot::First(c) => edits.push(Edit {
            start: c,
            end: c,
            text: added.iter().map(|l| format!("{l}\n{indent}")).collect(),
        }),
        Spot::Line(at) => {
            let mut added: String = added.iter().map(|l| format!("{indent}{l}\n")).collect();
            if made {
                added = format!(":PROPERTIES:\n{added}:END:\n");
            }
            let a = line_start(&st, text, at);
            let bare = a == text.len() && !text.is_empty() && !text.ends_with('\n');
            edits.push(Edit {
                start: a,
                end: a,
                text: if bare {
                    format!("\n{}", added.trim_end_matches('\n'))
                } else {
                    added
                },
            });
        }
    }
    Some(edits)
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
    fn enter_makes_blocks_as_logseq() {
        let md = Flavor::LogseqMarkdown;
        let enter = |t: &str, at: usize| apply(t, &new_block(t, md, at).expect("a change").edits);
        // At the end of a block: a sibling after it.
        let t = "- one\n- two\n";
        assert_eq!(enter(t, 5), "- one\n- \n- two\n");
        // Its properties and dates stay its own, before the new block.
        let t = "- one\n  id:: 6512c0de-0000-4000-8000-000000000001\n  SCHEDULED: <2026-10-12 Mon>\n- two\n";
        assert_eq!(
            enter(t, 5),
            "- one\n  id:: 6512c0de-0000-4000-8000-000000000001\n  SCHEDULED: <2026-10-12 Mon>\n- \n- two\n"
        );
        // In the middle: split, the rest the new block's, the properties
        // the first's.
        let t = "- one two\n  id:: x\n";
        assert_eq!(enter(t, 5), "- one\n  id:: x\n- two\n");
        assert_eq!(enter(t, 6), "- one\n  id:: x\n- two\n");
        // With children shown: its first child, indented as the note is.
        let t = "- parent\n\t- child\n";
        assert_eq!(enter(t, 8), "- parent\n\t- \n\t- child\n");
        let t = "- parent\n  - child\n";
        assert_eq!(enter(t, 8), "- parent\n  - \n  - child\n");
        // With children folded: a sibling after them.
        let t = "- parent\n  collapsed:: true\n\t- child\n- next\n";
        assert_eq!(
            enter(t, 8),
            "- parent\n  collapsed:: true\n\t- child\n- \n- next\n"
        );
        // At its start: an empty block above.
        let t = "\t- item\n";
        assert_eq!(enter(t, 3), "\t- \n\t- item\n");
        // An empty child block: outdented.
        let t = "- a\n\t- \n";
        assert_eq!(enter(t, 7), "- a\n- \n");
        // Elsewhere, the mode's Enter.
        let t = "- one\n  id:: x\n";
        assert!(new_block(t, md, 12).is_none(), "a property line");
        assert!(new_block("Text\n", md, 4).is_none(), "no bullet");
        assert!(new_block("- one\n", Flavor::Obsidian, 5).is_none());
        // The cursor lands in the new block: the insertion is at it.
        let t = "- one\n  id:: x\n";
        let c = new_block(t, md, 5).unwrap();
        let insert = c.edits.iter().find(|e| e.start == e.end).unwrap();
        assert_eq!(insert.start, 5);
        assert!(insert.text.ends_with("- "));
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
    fn a_heading_blocks_keyword_and_priority_go_after_its_marks() {
        // Logseq writes them after `## ` (`- ## Plan` to `- ## LATER
        // Plan`), where the scanner reads them: before, `- ## TODO Plan`
        // cycled to `- DOING ODO Plan`.
        let md = Flavor::LogseqMarkdown;
        let c = |t: &str| run(t, |c| cycle_todo(t, md, TODO, c), 3);
        assert_eq!(c("- ## Plan\n"), "- ## TODO Plan\n");
        assert_eq!(c("- ## TODO Plan\n"), "- ## DOING Plan\n");
        assert_eq!(c("\t- ### DONE Plan\n"), "\t- ### Plan\n");
        let cycled = c("- ## TODO Plan\n");
        let s = scan::scan(&cycled, md, &scan::Options::default());
        assert_eq!(
            (s.blocks[0].marker, s.blocks[0].heading),
            (Some("DOING"), Some(2))
        );
        let p = |t: &str, x| run(t, |c| set_priority(t, md, c, x), 3);
        assert_eq!(p("- ## TODO Plan\n", Some('A')), "- ## TODO [#A] Plan\n");
        assert_eq!(p("- # Plan\n", Some('B')), "- # [#B] Plan\n");
        assert_eq!(p("- ## TODO [#A] Plan\n", None), "- ## TODO Plan\n");
        // A heading by its property has no marks; a `#tag` first is no
        // heading.
        assert_eq!(c("- Plan\n  heading:: 2\n"), "- TODO Plan\n  heading:: 2\n");
        assert_eq!(c("- #tag Plan\n"), "- TODO #tag Plan\n");
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
    fn properties_go_first_in_a_block_that_a_code_block_opens() {
        // As Logseq writes a property into a block whose first line opens
        // a code block, a `#+BEGIN_`, a table or a quote (it has no
        // title): as its first line, the line it opened under it. Before,
        // `id::` went into the code and `collapsed::` into the quote.
        let md = Flavor::LogseqMarkdown;
        let read = |t: &str| scan::scan(t, md, &scan::Options::default());
        let t = "- ```rust\n  let x = 1;\n  ```\n\t- child\n- next\n";
        let with_id = apply(t, &[add_id(t, md, 0, "u1").unwrap()]);
        assert_eq!(
            with_id,
            "- id:: u1\n  ```rust\n  let x = 1;\n  ```\n\t- child\n- next\n"
        );
        let b = &read(&with_id).blocks[0];
        assert_eq!((b.id.as_deref(), b.text.as_str()), (Some("u1"), "```rust"));
        // Then after it, folded and unfolded there.
        assert_eq!(
            apply(
                &with_id,
                &[add_property(&with_id, 0, "type", "code").unwrap()]
            ),
            "- id:: u1\n  type:: code\n  ```rust\n  let x = 1;\n  ```\n\t- child\n- next\n"
        );
        let folded = apply(&with_id, &toggle_fold(&with_id, 3).unwrap().edits);
        assert_eq!(
            folded,
            "- id:: u1\n  collapsed:: true\n  ```rust\n  let x = 1;\n  ```\n\t- child\n- next\n"
        );
        assert_eq!(
            apply(&folded, &toggle_fold(&folded, 3).unwrap().edits),
            with_id
        );
        // A quote folded, then unfolded: its first line again.
        let q = "- > quoted\n\t- child\n";
        let f = apply(q, &toggle_fold(q, 2).unwrap().edits);
        assert_eq!(f, "- collapsed:: true\n  > quoted\n\t- child\n");
        assert!(read(&f).blocks[0].collapsed);
        assert_eq!(apply(&f, &toggle_fold(&f, 2).unwrap().edits), q);
        // A table: set, then set again in place, its first line too.
        let tb = "\t- | a | b |\n\t  | 1 | 2 |\n";
        let set =
            |t: &str, props: &[(&str, String)]| apply(t, &set_properties(t, md, 0, props).unwrap());
        let once = set(tb, &[("card-repeats", "1".into()), ("id", "u2".into())]);
        assert_eq!(
            once,
            "\t- card-repeats:: 1\n\t  id:: u2\n\t  | a | b |\n\t  | 1 | 2 |\n"
        );
        assert_eq!(
            set(
                &once,
                &[
                    ("card-repeats", "2".into()),
                    ("card-last-score", "5".into())
                ]
            ),
            "\t- card-repeats:: 2\n\t  id:: u2\n\t  card-last-score:: 5\n\t  | a | b |\n\t  | 1 | 2 |\n"
        );
        // A `#+BEGIN_` block.
        let n = "- #+BEGIN_NOTE\n  hi\n  #+END_NOTE\n";
        assert_eq!(
            apply(n, &[add_id(n, md, 0, "u3").unwrap()]),
            "- id:: u3\n  #+BEGIN_NOTE\n  hi\n  #+END_NOTE\n"
        );
        // A date after the block's own lines, out of its code.
        let d = Date::new(2026, 10, 12);
        let dated = run(t, |c| set_date(t, md, c, "SCHEDULED", d), 2);
        assert_eq!(
            dated,
            "- ```rust\n  let x = 1;\n  ```\n  SCHEDULED: <2026-10-12 Mon>\n\t- child\n- next\n"
        );
        assert_eq!(read(&dated).blocks[0].scheduled, d);
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

    #[test]
    fn logseqs_clock_on_a_task_started_and_done() {
        let start = Now::at(1_792_000_000_000);
        assert_eq!(start.stamp, "2026-10-14 Wed 17:46:40");
        let later = Now::at(1_792_000_000_000 + 3_929_000);
        let cycled = |t: &str, now: &Now, flavor: Flavor| {
            let c = cycle_todo_clocked(t, flavor, NOW, 2, Some(now)).unwrap();
            apply(t, &c.edits)
        };
        // Started: a logbook after its properties and dates.
        let t = "- LATER read\n  id:: 1\n  SCHEDULED: <2026-10-14 Tue>\n  a note\n- next\n";
        let started = cycled(t, &start, Flavor::LogseqMarkdown);
        assert_eq!(
            started,
            "- NOW read\n  id:: 1\n  SCHEDULED: <2026-10-14 Tue>\n  :LOGBOOK:\n  CLOCK: [2026-10-14 Wed 17:46:40]\n  :END:\n  a note\n- next\n"
        );
        // Done: its clock closed with the time it ran.
        assert_eq!(
            cycled(&started, &later, Flavor::LogseqMarkdown),
            "- DONE read\n  id:: 1\n  SCHEDULED: <2026-10-14 Tue>\n  :LOGBOOK:\n  CLOCK: [2026-10-14 Wed 17:46:40]--[2026-10-14 Wed 18:52:09] =>  01:05:29\n  :END:\n  a note\n- next\n"
        );
        // Started again: another clock in the same logbook.
        let again = "\t- LATER x\n\t  :LOGBOOK:\n\t  CLOCK: [2026-10-13 Mon 09:00:00]--[2026-10-13 Mon 09:30:00] =>  00:30:00\n\t  :END:\n";
        let c = cycle_todo_clocked(again, Flavor::LogseqMarkdown, NOW, 4, Some(&start)).unwrap();
        assert_eq!(
            apply(again, &c.edits),
            "\t- NOW x\n\t  :LOGBOOK:\n\t  CLOCK: [2026-10-13 Mon 09:00:00]--[2026-10-13 Mon 09:30:00] =>  00:30:00\n\t  CLOCK: [2026-10-14 Wed 17:46:40]\n\t  :END:\n"
        );
        // A file's last block without a line feed; Org, after its drawer.
        assert_eq!(
            cycled("- TODO y", &start, Flavor::LogseqMarkdown),
            "- DOING y\n  :LOGBOOK:\n  CLOCK: [2026-10-14 Wed 17:46:40]\n  :END:"
        );
        let org = "* TODO z\n:PROPERTIES:\n:ID: 1\n:END:\nbody\n";
        let c = cycle_todo_clocked(org, Flavor::LogseqOrg, TODO, 3, Some(&start)).unwrap();
        assert_eq!(
            apply(org, &c.edits),
            "* DOING z\n:PROPERTIES:\n:ID: 1\n:END:\n:LOGBOOK:\nCLOCK: [2026-10-14 Wed 17:46:40]\n:END:\nbody\n"
        );
        // Without a clock (a graph that does not track time): the keyword only.
        let c = cycle_todo_clocked(t, Flavor::LogseqMarkdown, NOW, 2, None).unwrap();
        assert_eq!(c.edits.len(), 1);
    }

    #[test]
    fn properties_set_in_place_or_added_after_the_others() {
        let set = |text: &str, flavor, line, props: &[(&str, String)]| {
            apply(text, &set_properties(text, flavor, line, props).unwrap())
        };
        let props = [
            ("card-repeats", "2".to_string()),
            ("card-last-score", "5".into()),
        ];
        // Markdown: one written again in place, one added after the
        // others, before the children.
        assert_eq!(
            set(
                "- Q #card\n  card-repeats:: 1\n  id:: 1\n  - A\n",
                Flavor::LogseqMarkdown,
                0,
                &props
            ),
            "- Q #card\n  card-repeats:: 2\n  id:: 1\n  card-last-score:: 5\n  - A\n"
        );
        // A tab's child; the file's last line without a line feed.
        assert_eq!(
            set("- x\n\t- Q", Flavor::LogseqMarkdown, 1, &props[..1]),
            "- x\n\t- Q\n\t  card-repeats:: 2"
        );
        // Org: in the headline's drawer, made after its dates.
        assert_eq!(
            set(
                "* Q #card\nSCHEDULED: <2026-10-20 Tue>\n** A\n",
                Flavor::LogseqOrg,
                0,
                &props
            ),
            "* Q #card\nSCHEDULED: <2026-10-20 Tue>\n:PROPERTIES:\n:card-repeats: 2\n:card-last-score: 5\n:END:\n** A\n"
        );
        assert_eq!(
            set(
                "* Q\n:PROPERTIES:\n:card-repeats: 1\n:ID: 1\n:END:\nbody\n",
                Flavor::LogseqOrg,
                0,
                &props
            ),
            "* Q\n:PROPERTIES:\n:card-repeats: 2\n:ID: 1\n:card-last-score: 5\n:END:\nbody\n"
        );
        assert!(set_properties("- x\n", Flavor::LogseqMarkdown, 5, &props).is_none());
    }
}
