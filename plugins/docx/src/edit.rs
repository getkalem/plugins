//! Edits in a paragraph, written as Word writes them (the docx list's
//! WP8): typed text goes into the run before it, so it takes that run's
//! formatting; a deletion shortens, removes or empties the runs it
//! covers; Enter splits the paragraph and the elements open where it
//! splits; a paragraph joins its next sibling. Each edit rewrites the
//! paragraphs it touches and nothing else of the part, as one
//! [`Splice`].

use std::ops::Range;

use kalem_ooxml::xml::{self, Reader, Token};

use crate::flow::{COLUMN_BREAK, LINE_BREAK, Layout, Lock, OBJECT, PAGE_BREAK, Seg};
use crate::props::Span;
use crate::story::needs_preserve;

/// A replacement of bytes of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    /// The part.
    pub part: String,
    /// The bytes replaced.
    pub range: Range<usize>,
    /// What replaces them.
    pub text: String,
}

/// What text typed is written as.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Tab,
    Break(&'static str),
}

/// Typed text as the run content Word writes for it: text, `w:tab` for a
/// tab, `w:br` for a line break ('\n' or '\u{B}'), a page break ('\u{C}')
/// or a column break ('\u{E}'). Characters XML forbids, and the object
/// character, are refused.
fn pieces(s: &str) -> Result<Vec<Piece>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        let p = match c {
            '\t' => Piece::Tab,
            '\n' | '\r' | LINE_BREAK => Piece::Break(""),
            PAGE_BREAK => Piece::Break("page"),
            COLUMN_BREAK => Piece::Break("column"),
            OBJECT => return Err("An object is not typed; insert it instead".into()),
            c if !xml::allowed_char(c) => {
                return Err(format!(
                    "U+{:04X} cannot be written in a Word document",
                    c as u32
                ));
            }
            c => {
                cur.push(c);
                continue;
            }
        };
        if !cur.is_empty() {
            out.push(Piece::Text(std::mem::take(&mut cur)));
        }
        out.push(p);
    }
    if !cur.is_empty() {
        out.push(Piece::Text(cur));
    }
    Ok(out)
}

/// The prefix of an element's name as written at `at` (`w:`), or empty.
fn prefix_at(src: &str, at: usize) -> String {
    let s = &src[at + 1..];
    let end = s
        .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
        .unwrap_or(s.len());
    xml::prefix(&s[..end]).to_owned()
}

fn text_tag(p: &str, text: &str) -> String {
    let space = if needs_preserve(text) {
        " xml:space=\"preserve\""
    } else {
        ""
    };
    format!("<{p}t{space}>{}</{p}t>", xml::escape(text))
}

/// Pieces as run content with the prefix `p`.
fn render(p: &str, pieces: &[Piece]) -> String {
    let mut out = String::new();
    for piece in pieces {
        match piece {
            Piece::Text(t) => out.push_str(&text_tag(p, t)),
            Piece::Tab => out.push_str(&format!("<{p}tab/>")),
            Piece::Break("") => out.push_str(&format!("<{p}br/>")),
            Piece::Break(kind) => out.push_str(&format!("<{p}br {p}type=\"{kind}\"/>")),
        }
    }
    out
}

/// A `w:t` with new text: its own start tag kept (its `xml:space` set
/// when the text needs it), further pieces after it.
fn render_text_atom(src: &str, atom: &Span, inner: &Span, pieces: &[Piece]) -> String {
    let start = &src[atom.start..inner.start];
    let end = &src[inner.end..atom.end];
    let p = prefix_at(src, atom.start);
    let mut out = String::new();
    let mut first = true;
    for piece in pieces {
        match piece {
            Piece::Text(t) if first => {
                let tag = if needs_preserve(t) {
                    xml::set_attr(start, "xml:space", "preserve")
                } else {
                    start.to_owned()
                };
                // A self-closed `<w:t/>` gets content and an end tag.
                if end.is_empty() {
                    let open = tag.trim_end_matches("/>").trim_end().to_owned() + ">";
                    out.push_str(&format!("{open}{}</{p}t>", xml::escape(t)));
                } else {
                    out.push_str(&format!("{tag}{}{end}", xml::escape(t)));
                }
                first = false;
            }
            other => out.push_str(&render(&p, std::slice::from_ref(other))),
        }
    }
    out
}

/// A run's properties as a new run of the same look takes them: the
/// tracked changes in them left out (a paragraph mark's `w:ins`, a
/// `w:rPrChange`).
fn clean_rpr(src: &str, span: &Span) -> String {
    let text = &src[span.clone()];
    let mut out = String::new();
    let mut at = 0;
    let mut r = Reader::new(text);
    let mut depth = 0usize;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1
                    && matches!(
                        tag.name,
                        "ins" | "del" | "moveFrom" | "moveTo" | "rPrChange"
                    )
                {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    out.push_str(&text[at..tag.span.start]);
                    at = end;
                    continue;
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => depth = depth.saturating_sub(1),
            Token::Text { .. } => {}
        }
    }
    out.push_str(&text[at..]);
    // `<w:rPr></w:rPr>` with nothing left is no properties.
    let inner_empty = {
        let mut r = Reader::new(&out);
        let mut n = 0;
        while let Some(t) = r.next_token() {
            if let Token::Start(_) = t {
                n += 1;
            }
        }
        n <= 1
    };
    if inner_empty { String::new() } else { out }
}

fn check_range(l: &Layout, range: &Range<usize>) -> Result<(), String> {
    if range.start > range.end
        || range.end > l.text.len()
        || !l.text.is_char_boundary(range.start)
        || !l.text.is_char_boundary(range.end)
    {
        return Err(format!(
            "{}..{} is not a range of the paragraph's text",
            range.start, range.end
        ));
    }
    Ok(())
}

/// Where typed text goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// Into a text piece, at a byte of its text.
    IntoText(usize, usize),
    /// After a piece that is not text, in its run.
    After(usize),
    /// Before a piece that is not text, in its run.
    Before(usize),
    /// In a new run at this byte of the part.
    NewRun(usize),
}

/// Where text typed over `range` goes: over a selection, into its first
/// character's text (it takes that look); else into the text before it,
/// or after the piece before it; else before the piece after it; else a
/// run of its own.
fn choose_target(l: &Layout, range: &Range<usize>) -> Option<Target> {
    let usable = |s: &Seg| {
        s.lock.is_none() && l.runs[s.run].lock.is_none() && !l.runs[s.run].start_tag.is_empty()
    };
    let pos = range.start;
    let over = (range.start < range.end)
        .then(|| {
            l.segs.iter().position(|s| {
                s.text.is_some()
                    && s.range.start < range.end
                    && s.range.end > range.start
                    && usable(s)
            })
        })
        .flatten();
    if let Some(i) = over {
        return Some(Target::IntoText(
            i,
            pos.saturating_sub(l.segs[i].range.start),
        ));
    }
    if let Some(i) = l
        .segs
        .iter()
        .position(|s| s.text.is_some() && s.range.start < pos && pos < s.range.end && usable(s))
    {
        return Some(Target::IntoText(i, pos - l.segs[i].range.start));
    }
    let prev = l.segs.iter().rposition(|s| s.range.end <= pos);
    let next = l.segs.iter().position(|s| s.range.start >= range.end);
    Some(match (prev, next) {
        (Some(p), _) if usable(&l.segs[p]) => {
            if l.segs[p].text.is_some() {
                Target::IntoText(p, pos.min(l.segs[p].range.end) - l.segs[p].range.start)
            } else {
                Target::After(p)
            }
        }
        (_, Some(n)) if usable(&l.segs[n]) => {
            if l.segs[n].text.is_some() {
                Target::IntoText(n, 0)
            } else {
                Target::Before(n)
            }
        }
        (Some(p), _) => Target::NewRun(l.tops[l.runs[l.segs[p].run].top].end),
        (None, Some(n)) => Target::NewRun(l.tops[l.runs[l.segs[n].run].top].start),
        (None, None) => Target::NewRun(l.content_end),
    })
}

/// Replaces `range` of a paragraph's edit text with `insert`; the splice
/// rewriting the paragraph. `src` is its part's text.
pub fn replace(l: &Layout, src: &str, range: Range<usize>, insert: &str) -> Result<Splice, String> {
    check_range(l, &range)?;
    let typed = pieces(insert)?;
    // What is deleted must be editable.
    for s in &l.segs {
        if s.range.start < range.end
            && s.range.end > range.start
            && let Some(lock) = s.lock
        {
            return Err(lock.reason().into());
        }
    }
    // Each text atom's new text, each atom removed, and new content after
    // or before an atom.
    let mut new_text: Vec<(usize, String)> = Vec::new();
    let mut removed: Vec<Span> = Vec::new();
    for (i, s) in l.segs.iter().enumerate() {
        if !(s.range.start < range.end && s.range.end > range.start) {
            continue;
        }
        match &s.text {
            Some(_) => {
                let t = &l.text[s.range.clone()];
                let a = range.start.max(s.range.start) - s.range.start;
                let b = range.end.min(s.range.end) - s.range.start;
                new_text.push((i, format!("{}{}", &t[..a], &t[b..])));
            }
            None => removed.push(s.atom.clone()),
        }
    }
    let target = (!typed.is_empty())
        .then(|| choose_target(l, &range))
        .flatten();
    // The splices, in part coordinates.
    let mut edits: Vec<(Span, String)> = Vec::new();
    let mut into_text = None;
    let mut new_run_at = None;
    match target {
        Some(Target::IntoText(i, off)) => into_text = Some((i, off)),
        Some(Target::After(i)) => {
            let a = &l.segs[i].atom;
            edits.push((a.end..a.end, render(&prefix_at(src, a.start), &typed)));
        }
        Some(Target::Before(i)) => {
            let a = &l.segs[i].atom;
            edits.push((a.start..a.start, render(&prefix_at(src, a.start), &typed)));
        }
        Some(Target::NewRun(at)) => new_run_at = Some(at),
        None => {}
    }
    if let Some((i, _)) = into_text
        && !new_text.iter().any(|(j, _)| *j == i)
    {
        new_text.push((i, l.text[l.segs[i].range.clone()].to_owned()));
    }
    for (i, t) in &new_text {
        let s = &l.segs[*i];
        let (inner, _) = s.text.clone().unwrap_or_default();
        let mut ps: Vec<Piece> = Vec::new();
        if let Some((j, off)) = into_text
            && j == *i
        {
            // `off` is in the text before the deletion; the deleted part
            // of this atom starts at or after it.
            let (head, tail) = t.split_at(off.min(t.len()));
            if !head.is_empty() {
                ps.push(Piece::Text(head.into()));
            }
            ps.extend(typed.iter().cloned());
            if !tail.is_empty() {
                ps.push(Piece::Text(tail.into()));
            }
        } else if !t.is_empty() {
            ps.push(Piece::Text(t.clone()));
        }
        // Text pieces next to each other are one.
        let mut merged: Vec<Piece> = Vec::new();
        for p in ps {
            match (merged.last_mut(), p) {
                (Some(Piece::Text(a)), Piece::Text(b)) => a.push_str(&b),
                (_, p) => merged.push(p),
            }
        }
        if merged.is_empty() {
            removed.push(s.atom.clone());
        } else {
            edits.push((
                s.atom.clone(),
                render_text_atom(src, &s.atom, &inner, &merged),
            ));
        }
    }
    for a in &removed {
        edits.push((a.clone(), String::new()));
    }
    // A run left with nothing goes as a whole.
    for run in &l.runs {
        let gone = |a: &Span| removed.contains(a);
        let added = edits
            .iter()
            .any(|(s, t)| !t.is_empty() && s.start >= run.span.start && s.end <= run.span.end);
        if !run.atoms.is_empty() && run.atoms.iter().all(gone) && !added {
            edits.retain(|(s, _)| !(s.start >= run.span.start && s.end <= run.span.end));
            edits.push((run.span.clone(), String::new()));
        }
    }
    if let Some(at) = new_run_at {
        let p = prefix_at(src, l.start_tag.start);
        let near = l
            .segs
            .iter()
            .rposition(|s| s.range.end <= range.start)
            .or_else(|| l.segs.iter().position(|s| s.range.start >= range.end));
        let rpr = near
            .and_then(|i| l.runs[l.segs[i].run].rpr.clone())
            .or_else(|| l.mark_rpr.clone())
            .map(|s| clean_rpr(src, &s))
            .unwrap_or_default();
        edits.push((at..at, format!("<{p}r>{rpr}{}</{p}r>", render(&p, &typed))));
    }
    finish(l, src, edits)
}

/// The paragraph with `edits` applied, as one splice of its span; an
/// empty `<w:p/>` given content gets an end tag.
fn finish(l: &Layout, src: &str, mut edits: Vec<(Span, String)>) -> Result<Splice, String> {
    edits.sort_by_key(|(s, _)| (s.start, s.end));
    for w in edits.windows(2) {
        if w[0].0.end > w[1].0.start {
            return Err("The edit overlaps itself (a bug): nothing was changed".into());
        }
    }
    let mut out = String::new();
    let mut at = l.span.start;
    let empty_open = l.empty && !edits.is_empty();
    for (s, t) in &edits {
        if empty_open && s.start == l.start_tag.end && at <= l.start_tag.start {
            // Content for `<w:p/>`: the tag opened, the content, an end.
            let tag = &src[l.start_tag.clone()];
            let p = prefix_at(src, l.start_tag.start);
            out.push_str(tag.trim_end_matches("/>").trim_end());
            out.push('>');
            out.push_str(t);
            out.push_str(&format!("</{p}p>"));
            at = l.start_tag.end;
            continue;
        }
        out.push_str(&src[at..s.start]);
        out.push_str(t);
        at = s.end;
    }
    out.push_str(&src[at..l.span.end]);
    Ok(Splice {
        part: l.part.clone(),
        range: l.span.clone(),
        text: out,
    })
}

/// Who makes tracked changes, when, and the next `w:id` to give one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// `w:author`.
    pub author: String,
    /// `w:date`, ISO 8601.
    pub date: String,
    /// The next free `w:id`.
    pub next_id: u64,
}

impl Track {
    /// The attributes of a new change, its ID taken.
    fn attrs(&mut self, p: &str) -> String {
        let id = self.next_id;
        self.next_id += 1;
        format!(
            "{p}id=\"{id}\" {p}author=\"{}\" {p}date=\"{}\"",
            xml::escape(&self.author),
            xml::escape(&self.date)
        )
    }
}

/// A piece of a run being written again.
enum Item {
    /// Kept as written.
    Raw(String),
    /// Text kept.
    Text(String),
    /// An element deleted (a tracked change): its XML as written.
    DelRaw(String),
    /// Text deleted.
    DelText(String),
    /// Typed content inserted.
    Ins(String),
}

/// The element at `span` with its text elements as deleted ones:
/// `w:t` → `w:delText`, `w:instrText` → `w:delInstrText`.
fn as_deleted(src: &str, span: &Span) -> String {
    let t = &src[span.clone()];
    let p = prefix_at(src, span.start);
    t.replace(&format!("<{p}t>"), &format!("<{p}delText>"))
        .replace(&format!("<{p}t "), &format!("<{p}delText "))
        .replace(&format!("</{p}t>"), &format!("</{p}delText>"))
        .replace(&format!("<{p}instrText"), &format!("<{p}delInstrText"))
        .replace(&format!("</{p}instrText>"), &format!("</{p}delInstrText>"))
}

/// A run written again from its items: kept pieces as runs of its look,
/// deleted ones in `w:del`, typed ones in `w:ins`, each change with an ID
/// of its own.
fn write_run(run_start: &str, rpr: &str, p: &str, items: Vec<Item>, track: &mut Track) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum K {
        Keep,
        Del,
        Ins,
    }
    let mut groups: Vec<(K, String)> = Vec::new();
    for it in items {
        let (k, xml) = match it {
            Item::Raw(x) => (K::Keep, x),
            Item::Text(t) if t.is_empty() => continue,
            Item::Text(t) => (K::Keep, text_tag(p, &t)),
            Item::DelRaw(x) => (K::Del, x),
            Item::DelText(t) if t.is_empty() => continue,
            Item::DelText(t) => {
                let space = if needs_preserve(&t) {
                    " xml:space=\"preserve\""
                } else {
                    ""
                };
                (
                    K::Del,
                    format!("<{p}delText{space}>{}</{p}delText>", xml::escape(&t)),
                )
            }
            Item::Ins(x) => (K::Ins, x),
        };
        match groups.last_mut() {
            Some((gk, g)) if *gk == k => g.push_str(&xml),
            _ => groups.push((k, xml)),
        }
    }
    let ins_rpr = clean_rpr_text(rpr);
    let mut out = String::new();
    for (k, content) in groups {
        match k {
            K::Keep => out.push_str(&format!("{run_start}{rpr}{content}</{p}r>")),
            K::Del => {
                let a = track.attrs(p);
                out.push_str(&format!(
                    "<{p}del {a}>{run_start}{rpr}{content}</{p}r></{p}del>"
                ));
            }
            K::Ins => {
                let a = track.attrs(p);
                out.push_str(&format!(
                    "<{p}ins {a}><{p}r>{ins_rpr}{content}</{p}r></{p}ins>"
                ));
            }
        }
    }
    out
}

fn clean_rpr_text(rpr: &str) -> String {
    if rpr.is_empty() {
        String::new()
    } else {
        clean_rpr(rpr, &(0..rpr.len()))
    }
}

/// [`replace`] as a tracked change (`w:trackRevisions`): typed text is
/// written in `w:ins`, deleted text kept in `w:del` as `w:delText`, as
/// Word writes them; text of an insertion deleted again is taken away.
pub fn replace_tracked(
    l: &Layout,
    src: &str,
    range: Range<usize>,
    insert: &str,
    track: &mut Track,
) -> Result<Splice, String> {
    check_range(l, &range)?;
    let typed = pieces(insert)?;
    for s in &l.segs {
        // Text deleted already stays deleted.
        if s.range.start < range.end
            && s.range.end > range.start
            && let Some(lock) = s.lock
            && lock != Lock::Deleted
        {
            return Err(lock.reason().into());
        }
        if s.range.start < range.end
            && s.range.end > range.start
            && s.lock.is_none()
            && l.runs[s.run].start_tag.is_empty()
            && !l.runs[s.run].inserted
        {
            return Err(
                "An object outside runs (an equation) is not deleted as a tracked change yet"
                    .into(),
            );
        }
    }
    let target = (!typed.is_empty())
        .then(|| choose_target(l, &range))
        .flatten();
    let p = prefix_at(src, l.start_tag.start);
    let typed_xml = render(&p, &typed);
    let mut edits: Vec<(Span, String)> = Vec::new();
    for (ri, run) in l.runs.iter().enumerate() {
        if run.lock == Some(Lock::Deleted) {
            continue;
        }
        if run.start_tag.is_empty() {
            // An object outside runs, inside an insertion: taken away.
            let gone = l
                .segs
                .iter()
                .any(|s| s.run == ri && s.range.start < range.end && s.range.end > range.start);
            if gone {
                edits.push((run.span.clone(), String::new()));
            }
            continue;
        }
        let segs: Vec<(usize, &Seg)> = l
            .segs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.run == ri)
            .collect();
        let deletes = segs
            .iter()
            .any(|(_, s)| s.range.start < range.end && s.range.end > range.start);
        let hosts = match target {
            Some(Target::IntoText(i, _) | Target::After(i) | Target::Before(i)) => {
                l.segs[i].run == ri
            }
            _ => false,
        };
        if !deletes && !hosts {
            continue;
        }
        let tracked = !run.inserted;
        let mut items = Vec::new();
        for atom in &run.atoms {
            let seg = segs.iter().find(|(_, s)| &s.atom == atom);
            let Some((si, s)) = seg else {
                items.push(Item::Raw(src[atom.clone()].to_owned()));
                continue;
            };
            let ins_here = |t: Option<Target>, which: fn(Target, usize) -> bool| {
                t.is_some_and(|t| which(t, *si))
            };
            if ins_here(target, |t, i| t == Target::Before(i)) {
                items.push(if tracked {
                    Item::Ins(typed_xml.clone())
                } else {
                    Item::Raw(typed_xml.clone())
                });
            }
            match &s.text {
                Some(_) => {
                    let t = &l.text[s.range.clone()];
                    let (a, b) = if s.range.end <= range.start {
                        (t.len(), t.len())
                    } else if s.range.start >= range.end {
                        (0, 0)
                    } else {
                        (
                            range.start.max(s.range.start) - s.range.start,
                            range.end.min(s.range.end) - s.range.start,
                        )
                    };
                    let into = matches!(target, Some(Target::IntoText(i, _)) if i == *si);
                    let whole = a == 0 && b == 0 && !into || a == t.len() && b == t.len() && !into;
                    if whole {
                        items.push(Item::Raw(src[atom.clone()].to_owned()));
                    } else {
                        items.push(Item::Text(t[..a].to_owned()));
                        if into {
                            items.push(if tracked {
                                Item::Ins(typed_xml.clone())
                            } else {
                                Item::Raw(typed_xml.clone())
                            });
                        }
                        if tracked {
                            items.push(Item::DelText(t[a..b].to_owned()));
                        }
                        items.push(Item::Text(t[b..].to_owned()));
                    }
                }
                None => {
                    let covered = s.range.start < range.end && s.range.end > range.start;
                    if !covered {
                        items.push(Item::Raw(src[atom.clone()].to_owned()));
                    } else if tracked {
                        items.push(Item::DelRaw(as_deleted(src, atom)));
                    }
                }
            }
            if ins_here(target, |t, i| t == Target::After(i)) {
                items.push(if tracked {
                    Item::Ins(typed_xml.clone())
                } else {
                    Item::Raw(typed_xml.clone())
                });
            }
        }
        let run_start = src[run.start_tag.clone()].to_owned();
        let run_start = if run_start.ends_with("/>") {
            run_start.trim_end_matches("/>").trim_end().to_owned() + ">"
        } else {
            run_start
        };
        let rpr = run
            .rpr
            .as_ref()
            .map_or(String::new(), |r| src[r.clone()].to_owned());
        edits.push((
            run.span.clone(),
            write_run(&run_start, &rpr, &p, items, track),
        ));
    }
    if let Some(Target::NewRun(at)) = target {
        let near = l
            .segs
            .iter()
            .rposition(|s| s.range.end <= range.start)
            .or_else(|| l.segs.iter().position(|s| s.range.start >= range.end));
        let rpr = near
            .and_then(|i| l.runs[l.segs[i].run].rpr.clone())
            .or_else(|| l.mark_rpr.clone())
            .map(|s| clean_rpr(src, &s))
            .unwrap_or_default();
        let a = track.attrs(&p);
        edits.push((
            at..at,
            format!("<{p}ins {a}><{p}r>{rpr}{typed_xml}</{p}r></{p}ins>"),
        ));
    }
    finish(l, src, edits)
}

/// A paragraph's mark marked inserted or deleted (a tracked change): its
/// `w:pPr/w:rPr` gets a `w:ins` or `w:del`, first in it as the schema
/// orders them; the properties are made when the paragraph has none.
pub fn mark_change(
    l: &Layout,
    src: &str,
    deleted: bool,
    track: &mut Track,
) -> Result<Splice, String> {
    let p = prefix_at(src, l.start_tag.start);
    let a = track.attrs(&p);
    let mark = format!("<{p}{} {a}/>", if deleted { "del" } else { "ins" });
    let start = &src[l.start_tag.clone()];
    let open = if l.empty {
        start.trim_end_matches("/>").trim_end().to_owned() + ">"
    } else {
        start.to_owned()
    };
    let ppr = match (&l.ppr, &l.mark_rpr) {
        (Some(ppr), Some(rpr)) => {
            let rpr_text = &src[rpr.clone()];
            let new_rpr = if rpr_text.ends_with("/>") {
                format!(
                    "{}>{mark}</{p}rPr>",
                    rpr_text.trim_end_matches("/>").trim_end()
                )
            } else {
                let gt = rpr_text.find('>').map_or(rpr_text.len(), |g| g + 1);
                format!("{}{mark}{}", &rpr_text[..gt], &rpr_text[gt..])
            };
            format!(
                "{}{new_rpr}{}",
                &src[ppr.start..rpr.start],
                &src[rpr.end..ppr.end]
            )
        }
        (Some(ppr), None) => {
            let ppr_text = &src[ppr.clone()];
            let rpr = format!("<{p}rPr>{mark}</{p}rPr>");
            if ppr_text.ends_with("/>") {
                format!(
                    "{}>{rpr}</{p}pPr>",
                    ppr_text.trim_end_matches("/>").trim_end()
                )
            } else {
                // The mark's properties come before `w:sectPr` and
                // `w:pPrChange`, after everything else.
                let at = [format!("<{p}sectPr"), format!("<{p}pPrChange")]
                    .iter()
                    .filter_map(|n| ppr_text.find(n.as_str()))
                    .min()
                    .unwrap_or_else(|| ppr_text.rfind("</").unwrap_or(ppr_text.len()));
                format!("{}{rpr}{}", &ppr_text[..at], &ppr_text[at..])
            }
        }
        (None, _) => format!("<{p}pPr><{p}rPr>{mark}</{p}rPr></{p}pPr>"),
    };
    let rest = if l.empty {
        format!("</{p}p>")
    } else {
        let c0 = l.ppr.as_ref().map_or(l.start_tag.end, |s| s.end);
        src[c0..l.span.end].to_owned()
    };
    Ok(Splice {
        part: l.part.clone(),
        range: l.span.clone(),
        text: format!("{open}{ppr}{rest}"),
    })
}

/// `settings.xml` with `w:trackRevisions` on or off, put where the
/// schema's order puts it.
pub fn set_track_revisions(settings: &str, on: bool) -> String {
    const BEFORE: &[&str] = &[
        "writeProtection",
        "view",
        "zoom",
        "removePersonalInformation",
        "removeDateAndTime",
        "doNotDisplayPageBoundaries",
        "displayBackgroundShape",
        "printPostScriptOverText",
        "printFractionalCharacterWidth",
        "printFormsData",
        "embedTrueTypeFonts",
        "embedSystemFonts",
        "saveSubsetFonts",
        "saveFormsData",
        "mirrorMargins",
        "alignBordersAndEdges",
        "bordersDoNotSurroundHeader",
        "bordersDoNotSurroundFooter",
        "gutterAtTop",
        "hideSpellingErrors",
        "hideGrammaticalErrors",
        "activeWritingStyle",
        "proofState",
        "formsDesign",
        "attachedTemplate",
        "linkStyles",
        "stylePaneFormatFilter",
        "stylePaneSortMethod",
        "documentType",
        "mailMerge",
        "revisionView",
    ];
    let mut r = Reader::new(settings);
    let mut depth = 0;
    let mut insert_at = None;
    let mut root_end = None;
    let mut prefix = String::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 0 {
                    prefix = xml::prefix(tag.qname).to_owned();
                }
                if depth == 1 {
                    if tag.name == "trackRevisions" {
                        let end = if tag.empty {
                            tag.span.end
                        } else {
                            r.skip_element()
                        };
                        return if on {
                            settings.to_owned()
                        } else {
                            format!("{}{}", &settings[..tag.span.start], &settings[end..])
                        };
                    }
                    if insert_at.is_none() && !BEFORE.contains(&tag.name) {
                        insert_at = Some(tag.span.start);
                    }
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { span, .. } => {
                depth -= 1;
                if depth == 0 {
                    root_end = Some(span.start);
                }
            }
            Token::Text { .. } => {}
        }
    }
    if !on {
        return settings.to_owned();
    }
    let at = insert_at.or(root_end).unwrap_or(settings.len());
    format!(
        "{}<{prefix}trackRevisions/>{}",
        &settings[..at],
        &settings[at..]
    )
}

/// The open elements at a place of a paragraph's content, outermost
/// first: each its name, its start tag, and its properties' element
/// (`w:rPr` of a run) when it came before the place.
fn open_at(src: &str, from: usize, to: usize) -> Result<Vec<(String, String, String)>, String> {
    let text = &src[from..to];
    let mut stack: Vec<(String, String, String)> = Vec::new();
    let mut r = Reader::new(text);
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                let is_props = matches!(tag.name, "rPr" | "smartTagPr" | "customXmlPr" | "sdtPr");
                if is_props && !stack.is_empty() {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    if let Some(top) = stack.last_mut() {
                        top.2.push_str(&text[tag.span.start..end]);
                    }
                    continue;
                }
                if !tag.empty {
                    stack.push((
                        tag.name.to_owned(),
                        text[tag.span.clone()].to_owned(),
                        String::new(),
                    ));
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    for (name, _, _) in &stack {
        if matches!(
            name.as_str(),
            "sdt" | "sdtContent" | "fldSimple" | "AlternateContent" | "Choice" | "Fallback"
        ) {
            return Err(format!("A paragraph is not split inside a {name} yet"));
        }
    }
    Ok(stack)
}

/// The highest numeric `w:id` of a part, so that a new tracked change or
/// comment gets one of its own.
pub fn max_id(src: &str) -> u64 {
    let mut max = 0;
    let mut rest = src;
    while let Some(p) = rest.find(":id=\"") {
        rest = &rest[p + 5..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if rest[digits.len()..].starts_with('"')
            && let Ok(n) = digits.parse::<u64>()
        {
            max = max.max(n);
        }
    }
    max
}

/// The `w:pPr` text with its `w:sectPr` taken out.
fn without_sect(src: &str, l: &Layout) -> String {
    let Some(ppr) = &l.ppr else {
        return String::new();
    };
    match &l.sect {
        Some(s) => format!("{}{}", &src[ppr.start..s.start], &src[s.end..ppr.end]),
        None => src[ppr.clone()].to_owned(),
    }
}

/// The `w:pPr` text with its paragraph style set to `style`.
fn with_style(ppr: &str, prefix: &str, style: &str) -> String {
    if ppr.is_empty() {
        return format!(
            "<{prefix}pPr><{prefix}pStyle {prefix}val=\"{}\"/></{prefix}pPr>",
            xml::escape(style)
        );
    }
    let mut r = Reader::new(ppr);
    let mut first_child = None;
    let mut depth = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == "pStyle" {
                    let new = xml::set_attr(&ppr[tag.span.clone()], &format!("{prefix}val"), style);
                    return format!("{}{new}{}", &ppr[..tag.span.start], &ppr[tag.span.end..]);
                }
                if depth == 1 && first_child.is_none() {
                    first_child = Some(tag.span.start);
                }
                if depth == 0 && tag.empty {
                    // `<w:pPr/>`.
                    let open = ppr.trim_end_matches("/>").trim_end();
                    return format!(
                        "{open}><{prefix}pStyle {prefix}val=\"{}\"/></{prefix}pPr>",
                        xml::escape(style)
                    );
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { span, .. } => {
                if depth == 1 && first_child.is_none() {
                    first_child = Some(span.start);
                }
                depth -= 1;
            }
            Token::Text { .. } => {}
        }
    }
    let at = first_child.unwrap_or(ppr.len());
    format!(
        "{}<{prefix}pStyle {prefix}val=\"{}\"/>{}",
        &ppr[..at],
        xml::escape(style),
        &ppr[at..]
    )
}

/// The `w:pPr` text without its paragraph style; nothing when nothing is
/// left in it.
fn without_style(ppr: &str) -> String {
    let mut r = Reader::new(ppr);
    let mut depth = 0;
    let mut out = ppr.to_owned();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == "pStyle" {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    out = format!("{}{}", &ppr[..tag.span.start], &ppr[end..]);
                    break;
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => depth -= 1,
            Token::Text { .. } => {}
        }
    }
    // `<w:pPr></w:pPr>` is no properties.
    let mut r = Reader::new(&out);
    let mut n = 0;
    while let Some(t) = r.next_token() {
        if let Token::Start(_) = t {
            n += 1;
        }
    }
    if n <= 1 { String::new() } else { out }
}

/// Splits a paragraph at `at` of its edit text (Enter): the elements open
/// there closed in the first paragraph and opened again in the second; a
/// tracked change opened again gets an ID of its own. `next_style` is the
/// style of the second paragraph when it differs (a heading's next
/// style, at its end): `Some(None)` for the default style, which Word
/// writes as no style at all.
pub fn split(
    l: &Layout,
    src: &str,
    at: usize,
    next_style: Option<Option<&str>>,
) -> Result<Splice, String> {
    check_range(l, &(at..at))?;
    let p = prefix_at(src, l.start_tag.start);
    let c0 = l
        .ppr
        .as_ref()
        .map_or(l.start_tag.end, |s| s.end)
        .min(l.content_end);
    // Where the content is cut, and the text atom cut in two.
    let inside = l
        .segs
        .iter()
        .position(|s| s.text.is_some() && s.range.start < at && at < s.range.end);
    let (x1, x2, left_mid, right_mid) = match inside {
        Some(i) => {
            let s = &l.segs[i];
            if let Some(lock) = s.lock {
                return Err(lock.reason().into());
            }
            let (inner, _) = s.text.clone().unwrap_or_default();
            let t = &l.text[s.range.clone()];
            let off = at - s.range.start;
            let left = render_text_atom(src, &s.atom, &inner, &[Piece::Text(t[..off].into())]);
            let right = render_text_atom(src, &s.atom, &inner, &[Piece::Text(t[off..].into())]);
            (s.atom.start, s.atom.end, left, right)
        }
        None => {
            let x = match l.segs.iter().rposition(|s| s.range.end <= at) {
                Some(i) => l.segs[i].atom.end,
                None => l
                    .segs
                    .iter()
                    .position(|s| s.range.start >= at)
                    .map_or(l.content_end, |i| l.segs[i].atom.start),
            };
            (x, x, String::new(), String::new())
        }
    };
    // At the very end everything stays in the first paragraph (a
    // bookmark ending there too), and at the start everything goes to the
    // second: no element is cut.
    let (x1, x2, left_mid, right_mid) = if at == l.text.len() {
        (l.content_end, l.content_end, String::new(), String::new())
    } else if at == 0 {
        (c0, c0, String::new(), String::new())
    } else {
        (x1, x2, left_mid, right_mid)
    };
    let mut x1 = x1.max(c0);
    let mut x2 = x2.max(x1);
    let mut stack = if l.empty {
        Vec::new()
    } else {
        open_at(src, c0, x1)?
    };
    // A cut at an element's end is a cut after it: no empty run or link is
    // opened again in the second paragraph.
    if left_mid.is_empty() && right_mid.is_empty() {
        while let Some((_, tag, _)) = stack.last() {
            let name_end = tag[1..]
                .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                .map_or(tag.len() - 1, |e| e + 1);
            let close = format!("</{}>", &tag[1..name_end]);
            if src[x1..].starts_with(&close) {
                x1 += close.len();
                x2 = x1;
                stack.pop();
            } else {
                break;
            }
        }
    }
    let mut next_id = max_id(src);
    let mut close = String::new();
    for (_, tag, _) in stack.iter().rev() {
        let name_end = tag[1..]
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .map_or(tag.len() - 1, |e| e + 1);
        close.push_str(&format!("</{}>", &tag[1..name_end]));
    }
    let mut reopen = String::new();
    for (name, tag, props) in &stack {
        let tag = if matches!(name.as_str(), "ins" | "del" | "moveFrom" | "moveTo") {
            next_id += 1;
            xml::set_attr(tag, &format!("{p}id"), &next_id.to_string())
        } else {
            tag.clone()
        };
        reopen.push_str(&tag);
        reopen.push_str(props);
    }
    let start_tag = &src[l.start_tag.clone()];
    let open_tag = if l.empty {
        start_tag.trim_end_matches("/>").trim_end().to_owned() + ">"
    } else {
        start_tag.to_owned()
    };
    // The second paragraph's own IDs are left to Word to give.
    let second_tag = xml::remove_attr(&xml::remove_attr(&open_tag, "w14:paraId"), "w14:textId");
    let ppr_left = without_sect(src, l);
    let ppr_right = l
        .ppr
        .as_ref()
        .map_or(String::new(), |s| src[s.clone()].to_owned());
    let ppr_right = match next_style {
        Some(Some(style)) => with_style(&ppr_right, &p, style),
        Some(None) => without_style(&ppr_right),
        None => ppr_right,
    };
    let (content_l, content_r) = if l.empty {
        (String::new(), String::new())
    } else {
        (
            format!("{}{left_mid}{close}", &src[c0..x1]),
            format!("{reopen}{right_mid}{}", &src[x2..l.content_end]),
        )
    };
    let text =
        format!("{open_tag}{ppr_left}{content_l}</{p}p>{second_tag}{ppr_right}{content_r}</{p}p>");
    Ok(Splice {
        part: l.part.clone(),
        range: l.span.clone(),
        text,
    })
}

/// Joins a paragraph and the next one (Backspace at the second's start):
/// the first's properties kept, the second's content after the first's,
/// a section the second ends ended by the joined one. With
/// `second_props`, the second's properties are kept instead: the first's
/// mark was one a tracked Enter inserted, and the paragraph is again the
/// one it was.
pub fn join(
    first: &Layout,
    second: &Layout,
    src: &str,
    second_props: bool,
) -> Result<Splice, String> {
    if first.part != second.part
        || first.sibling.0 != second.sibling.0
        || second.span.start < first.span.end
    {
        return Err("Only a paragraph and the next one beside it are joined".into());
    }
    if first.sect.is_some() {
        return Err(
            "A section ends with this paragraph: its section break is not deleted yet".into(),
        );
    }
    let between = &src[first.span.end..second.span.start];
    let mut r = Reader::new(between);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && !tag.name.ends_with("Start")
            && !tag.name.ends_with("End")
            && tag.name != "proofErr"
        {
            return Err("Only a paragraph and the next one beside it are joined".into());
        }
    }
    let p = prefix_at(src, first.start_tag.start);
    let start = &src[first.start_tag.clone()];
    let open = if first.empty {
        start.trim_end_matches("/>").trim_end().to_owned() + ">"
    } else {
        start.to_owned()
    };
    let mut ppr = first
        .ppr
        .as_ref()
        .map_or(String::new(), |s| src[s.clone()].to_owned());
    if second_props {
        ppr = second
            .ppr
            .as_ref()
            .map_or(String::new(), |s| src[s.clone()].to_owned());
    } else if let Some(sect) = &second.sect {
        let sect_text = &src[sect.clone()];
        ppr = if ppr.is_empty() {
            format!("<{p}pPr>{sect_text}</{p}pPr>")
        } else {
            // `w:sectPr` comes after the mark's `w:rPr`, before
            // `w:pPrChange`.
            let at = ppr
                .rfind(&format!("<{p}pPrChange"))
                .unwrap_or_else(|| ppr.rfind("</").unwrap_or(ppr.len()));
            format!("{}{sect_text}{}", &ppr[..at], &ppr[at..])
        };
    }
    let c1 = first.ppr.as_ref().map_or(first.start_tag.end, |s| s.end);
    let content1 = if first.empty {
        ""
    } else {
        &src[c1..first.content_end]
    };
    let c2 = second.ppr.as_ref().map_or(second.start_tag.end, |s| s.end);
    let content2 = if second.empty {
        ""
    } else {
        &src[c2..second.content_end]
    };
    Ok(Splice {
        part: first.part.clone(),
        range: first.span.start..second.span.end,
        text: format!("{open}{ppr}{content1}{content2}</{p}p>{between}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_as_run_content() {
        assert_eq!(
            pieces("a\tb\u{B}c\u{C}").unwrap(),
            [
                Piece::Text("a".into()),
                Piece::Tab,
                Piece::Text("b".into()),
                Piece::Break(""),
                Piece::Text("c".into()),
                Piece::Break("page")
            ]
        );
        assert!(pieces("\u{1}").is_err());
        assert!(pieces("\u{FFFC}").is_err());
        assert_eq!(
            render("w:", &[Piece::Text(" x ".into()), Piece::Break("page")]),
            r#"<w:t xml:space="preserve"> x </w:t><w:br w:type="page"/>"#
        );
    }

    #[test]
    fn styles_set_in_paragraph_properties() {
        assert_eq!(
            with_style("", "w:", "Normal"),
            r#"<w:pPr><w:pStyle w:val="Normal"/></w:pPr>"#
        );
        assert_eq!(
            with_style(
                r#"<w:pPr><w:pStyle w:val="Heading1"/><w:jc w:val="center"/></w:pPr>"#,
                "w:",
                "Normal"
            ),
            r#"<w:pPr><w:pStyle w:val="Normal"/><w:jc w:val="center"/></w:pPr>"#
        );
        assert_eq!(
            with_style(r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#, "w:", "Normal"),
            r#"<w:pPr><w:pStyle w:val="Normal"/><w:jc w:val="center"/></w:pPr>"#
        );
        assert_eq!(
            with_style("<w:pPr/>", "w:", "N"),
            r#"<w:pPr><w:pStyle w:val="N"/></w:pPr>"#
        );
        assert_eq!(
            with_style("<w:pPr></w:pPr>", "w:", "N"),
            r#"<w:pPr><w:pStyle w:val="N"/></w:pPr>"#
        );
        assert_eq!(
            without_style(r#"<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>"#),
            ""
        );
        assert_eq!(
            without_style(r#"<w:pPr><w:pStyle w:val="Heading1"/><w:jc w:val="center"/></w:pPr>"#),
            r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#
        );
    }

    #[test]
    fn run_properties_cleaned_of_changes() {
        let src = r#"<w:rPr><w:b/><w:ins w:id="1" w:author="A"/><w:rPrChange w:id="2"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr>"#;
        assert_eq!(clean_rpr(src, &(0..src.len())), "<w:rPr><w:b/></w:rPr>");
        let only = r#"<w:rPr><w:ins w:id="1"/></w:rPr>"#;
        assert_eq!(clean_rpr(only, &(0..only.len())), "");
    }

    #[test]
    fn track_changes_set_in_settings() {
        let s =
            r#"<w:settings><w:zoom w:percent="100"/><w:defaultTabStop w:val="720"/></w:settings>"#;
        let on = set_track_revisions(s, true);
        assert_eq!(
            on,
            r#"<w:settings><w:zoom w:percent="100"/><w:trackRevisions/><w:defaultTabStop w:val="720"/></w:settings>"#
        );
        assert_eq!(set_track_revisions(&on, true), on);
        assert_eq!(set_track_revisions(&on, false), s);
        assert_eq!(
            set_track_revisions("<w:settings></w:settings>", true),
            "<w:settings><w:trackRevisions/></w:settings>"
        );
    }

    #[test]
    fn ids_counted() {
        assert_eq!(
            max_id(r#"<w:ins w:id="7"/><w:bookmarkStart w:id="12" w:name="x"/><w:del w:id="x9"/>"#),
            12
        );
    }
}
