//! Threaded comments, as Excel 365 writes them: a sheet's
//! `threadedComments` part (a comment, its replies naming it by
//! `parentId`, `done` when resolved), the workbook's `persons` part naming
//! their authors, and for each thread a note (`<comment>` authored
//! `tc={id}`) that older readers show. A sheet tab's color
//! (`<sheetPr><tabColor>`).

use super::*;
use sha2::{Digest, Sha256};

const THREADS_TYPE: &str = "application/vnd.ms-excel.threadedcomments+xml";
const THREADS_REL: &str =
    "http://schemas.microsoft.com/office/2017/10/relationships/threadedComment";
const PERSONS_TYPE: &str = "application/vnd.ms-excel.person+xml";
const PERSONS_REL: &str = "http://schemas.microsoft.com/office/2017/10/relationships/person";
const NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments";
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// One comment of a thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadEntry {
    /// Its `id`, a GUID in braces.
    pub id: String,
    /// Its author's `personId`.
    pub person: String,
    /// Its author's name, from the persons part.
    pub author: String,
    /// Its text.
    pub text: String,
    /// When (`dT`).
    pub time: String,
}

/// A cell's thread: the comment, then its replies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// The cell.
    pub cell: CellRef,
    /// Resolved.
    pub done: bool,
    /// The comments.
    pub comments: Vec<ThreadEntry>,
}

/// A GUID in braces made from `seed`.
fn guid(seed: &str) -> String {
    let h = Sha256::digest(seed.as_bytes());
    let x: String = h.iter().take(16).map(|b| format!("{b:02X}")).collect();
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &x[0..8],
        &x[8..12],
        &x[12..16],
        &x[16..20],
        &x[20..32]
    )
}

/// Now as a threaded comment's `dT` (`2026-10-10T12:00:00.00`, UTC): for
/// comments made through the annotations, which bring no time.
pub fn now_dt() -> String {
    let secs = crate::time::SystemTime::now()
        .duration_since(crate::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) = crate::numfmt::civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.00",
        t / 3600,
        t % 3600 / 60,
        t % 60
    )
}

/// The persons of a `personList`: id → display name.
fn parse_persons(text: &str) -> Vec<(String, String)> {
    let mut r = Reader::new(text);
    let mut out = Vec::new();
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "person"
            && let Some(id) = tag.attr("id")
        {
            out.push((
                id.into_owned(),
                tag.attr("displayName")
                    .map(|v| v.into_owned())
                    .unwrap_or_default(),
            ));
        }
    }
    out
}

/// A threaded comments part's threads, in order.
fn parse_threads(text: &str, persons: &[(String, String)]) -> Vec<Thread> {
    let mut r = Reader::new(text);
    let mut out: Vec<Thread> = Vec::new();
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != "threadedComment" {
            continue;
        }
        let Some(cell) = tag.attr("ref").and_then(|v| CellRef::parse(v.trim())) else {
            continue;
        };
        let attr = |k: &str| tag.attr(k).map(|v| v.into_owned()).unwrap_or_default();
        let (id, person, time, parent) = (
            attr("id"),
            attr("personId"),
            attr("dT"),
            tag.attr("parentId"),
        );
        let done = matches!(tag.attr("done").as_deref(), Some("1" | "true"));
        let mut text = String::new();
        if !tag.empty {
            let mut depth = 0;
            while let Some(t) = r.next_token() {
                match t {
                    Token::Start(k) if k.name == "text" && !k.empty => {
                        text = r.text_until_end("text").0;
                    }
                    Token::Start(k) if !k.empty => depth += 1,
                    Token::End { name, .. } => {
                        if depth == 0 && name == "threadedComment" {
                            break;
                        }
                        depth -= 1;
                    }
                    _ => {}
                }
            }
        }
        let author = persons
            .iter()
            .find(|(i, _)| *i == person)
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        let entry = ThreadEntry {
            id,
            person,
            author,
            text,
            time,
        };
        match parent {
            Some(p) => {
                if let Some(t) = out
                    .iter_mut()
                    .find(|t| t.comments.first().is_some_and(|c| c.id == *p))
                {
                    t.comments.push(entry);
                }
            }
            None => out.push(Thread {
                cell,
                done,
                comments: vec![entry],
            }),
        }
    }
    out
}

/// The note older readers show for a thread, as Excel words it.
fn mirror_text(t: &Thread) -> String {
    let mut s = String::from(
        "[Threaded comment]\n\nYour version of Excel allows you to read this threaded comment; however, any edits to it will get removed if the file is opened in a newer version of Excel. Learn more: https://go.microsoft.com/fwlink/?linkid=870924\n\nComment:",
    );
    for (i, c) in t.comments.iter().enumerate() {
        if i > 0 {
            s.push_str("\nReply:");
        }
        s.push_str("\n    ");
        s.push_str(&c.text);
    }
    s
}

impl Workbook {
    /// The workbook's persons part, if it has one.
    fn persons_part(&self) -> Option<String> {
        self.workbook_rels
            .iter()
            .find(|r| r.kind == "person" && !r.external)
            .map(|r| rels::resolve(&self.workbook_part, &r.target))
            .filter(|p| self.pkg.contains(p))
    }

    fn persons(&self) -> Vec<(String, String)> {
        self.persons_part()
            .and_then(|p| text_of(self.pkg.part(&p).ok()?, &p).ok())
            .map(|t| parse_persons(&t))
            .unwrap_or_default()
    }

    /// The threads of sheet `idx`.
    pub fn threads(&mut self, idx: usize) -> Vec<Thread> {
        if let Some((g, v)) = self.thread_cache.get(&idx)
            && *g == self.generation
        {
            return v.clone();
        }
        let v = self.read_threads(idx);
        self.thread_cache.insert(idx, (self.generation, v.clone()));
        v
    }

    fn read_threads(&mut self, idx: usize) -> Vec<Thread> {
        let Ok(Some(part)) = self.sheet_part_of_kind(idx, "threadedComment") else {
            return Vec::new();
        };
        let Some(text) = self
            .pkg
            .part(&part)
            .ok()
            .and_then(|b| text_of(b, &part).ok())
        else {
            return Vec::new();
        };
        parse_threads(&text, &self.persons())
    }

    fn sheet_part_of_kind(&self, idx: usize, kind: &str) -> Result<Option<String>> {
        self.check_index(idx)?;
        Ok(self
            .sheet_parts(idx)?
            .into_iter()
            .find(|(k, _)| k == kind)
            .map(|(_, p)| p))
    }

    /// The id of person `name`, added to the persons part (made when the
    /// workbook has none) when new.
    fn person_id(&mut self, name: &str) -> Result<String> {
        if let Some((id, _)) = self.persons().into_iter().find(|(_, n)| n == name) {
            return Ok(id);
        }
        let id = guid(&format!("person {name}"));
        let el = format!(
            "<person displayName=\"{}\" id=\"{id}\" userId=\"{}\" providerId=\"None\"/>",
            xml::escape(name),
            xml::escape(name)
        );
        match self.persons_part() {
            Some(p) => {
                let text = text_of(self.pkg.part(&p)?, &p)?;
                let new = match text.rfind("</personList>") {
                    Some(at) => splice(&text, vec![(at..at, el)]),
                    None => text.replacen("/>", &format!(">{el}</personList>"), 1),
                };
                self.pkg.set_part(&p, new.into_bytes());
            }
            None => {
                let part = "xl/persons/person.xml".to_owned();
                self.add_part(
                    &part,
                    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<personList xmlns=\"{NS}\" xmlns:x=\"{MAIN}\">{el}</personList>"),
                    PERSONS_TYPE,
                )?;
                let wb = self.workbook_part.clone();
                self.add_rel_typed(&wb, PERSONS_REL, &part)?;
            }
        }
        Ok(id)
    }

    /// Writes sheet `idx`'s threads (its part made when new), and the
    /// notes mirroring the threads of `cells`.
    fn write_threads(&mut self, idx: usize, threads: &[Thread], cells: &[CellRef]) -> Result<()> {
        self.generation += 1;
        let mut items = String::new();
        for t in threads {
            let first = t.comments.first().map(|c| c.id.clone()).unwrap_or_default();
            for (i, c) in t.comments.iter().enumerate() {
                let parent = if i == 0 {
                    String::new()
                } else {
                    format!(" parentId=\"{first}\"")
                };
                let done = if i == 0 && t.done { " done=\"1\"" } else { "" };
                items.push_str(&format!(
                    "<threadedComment ref=\"{}\" dT=\"{}\" personId=\"{}\" id=\"{}\"{parent}{done}><text>{}</text></threadedComment>",
                    t.cell,
                    xml::escape(&c.time),
                    c.person,
                    c.id,
                    xml::escape(&c.text)
                ));
            }
        }
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<ThreadedComments xmlns=\"{NS}\" xmlns:x=\"{MAIN}\">{items}</ThreadedComments>"
        );
        match self.sheet_part_of_kind(idx, "threadedComment")? {
            Some(p) => self.pkg.set_part(&p, body.into_bytes()),
            None => {
                let part = self.free_part("xl/threadedComments/threadedComment");
                self.add_part(&part, body, THREADS_TYPE)?;
                let sheet = self.sheets[idx].part.clone();
                self.add_rel_typed(&sheet, THREADS_REL, &part)?;
            }
        }
        for at in cells {
            match threads.iter().find(|t| t.cell == *at) {
                Some(t) => {
                    self.set_comment_now(idx, *at, Some(&mirror_text(t)))?;
                    let first = t.comments.first().map(|c| c.id.clone()).unwrap_or_default();
                    self.set_note_author(idx, *at, &format!("tc={first}"))?;
                }
                None => self.set_comment_now(idx, *at, None)?,
            }
        }
        Ok(())
    }

    /// A comment on cell `at`: its thread's first, or a reply. One undo
    /// step.
    pub fn add_thread_comment(
        &mut self,
        idx: usize,
        at: CellRef,
        author: &str,
        text: &str,
        time: &str,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if text.trim().is_empty() {
            return Err(Error::Refused("A comment needs some text".into()));
        }
        let mut threads = self.threads(idx);
        if !threads.iter().any(|t| t.cell == at)
            && self
                .comments(idx)?
                .iter()
                .any(|c| c.cell == at && !c.author.starts_with("tc="))
        {
            return Err(Error::Refused(format!(
                "{at} has a note; a cell has a note or a comment, not both"
            )));
        }
        let author = if author.trim().is_empty() {
            "Kalem"
        } else {
            author.trim()
        };
        let sheet = self.sheets[idx].name.clone();
        let count: usize = threads.iter().map(|t| t.comments.len()).sum();
        self.in_one_step(|wb| {
            let person = wb.person_id(author)?;
            let entry = ThreadEntry {
                id: guid(&format!("{sheet} {at} {time} {count} {text}")),
                person,
                author: author.to_owned(),
                text: text.to_owned(),
                time: time.to_owned(),
            };
            match threads.iter_mut().find(|t| t.cell == at) {
                Some(t) => t.comments.push(entry),
                None => threads.push(Thread {
                    cell: at,
                    done: false,
                    comments: vec![entry],
                }),
            }
            wb.write_threads(idx, &threads, &[at])?;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Marks cell `at`'s thread resolved, or open again. One undo step.
    pub fn resolve_thread(&mut self, idx: usize, at: CellRef, done: bool) -> Result<()> {
        let mut threads = self.threads(idx);
        let Some(t) = threads.iter_mut().find(|t| t.cell == at) else {
            return Err(Error::Refused(format!("{at} has no comment")));
        };
        if t.done == done {
            return Ok(());
        }
        t.done = done;
        self.in_one_step(|wb| {
            wb.write_threads(idx, &threads, &[])?;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Deletes comment `index` of cell `at`'s thread; the first takes the
    /// whole thread. One undo step.
    pub fn delete_thread_comment(&mut self, idx: usize, at: CellRef, index: usize) -> Result<()> {
        let mut threads = self.threads(idx);
        let Some(pos) = threads.iter().position(|t| t.cell == at) else {
            return Err(Error::Refused(format!("{at} has no comment")));
        };
        if index >= threads[pos].comments.len() {
            return Err(Error::Refused("No such reply".into()));
        }
        if index == 0 {
            threads.remove(pos);
        } else {
            threads[pos].comments.remove(index);
        }
        self.in_one_step(|wb| {
            wb.write_threads(idx, &threads, &[at])?;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Changes the text of comment `index` of cell `at`'s thread (and of
    /// the note mirroring it for older readers). One undo step.
    pub fn set_thread_comment_text(
        &mut self,
        idx: usize,
        at: CellRef,
        index: usize,
        text: &str,
    ) -> Result<()> {
        if text.trim().is_empty() {
            return Err(Error::Refused("A comment needs some text".into()));
        }
        let mut threads = self.threads(idx);
        let Some(entry) = threads
            .iter_mut()
            .find(|t| t.cell == at)
            .and_then(|t| t.comments.get_mut(index))
        else {
            return Err(Error::Refused(format!("{at} has no such comment")));
        };
        if entry.text == text {
            return Ok(());
        }
        entry.text = text.to_owned();
        self.in_one_step(|wb| {
            wb.write_threads(idx, &threads, &[at])?;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Sheet `idx`'s tab color.
    pub fn tab_color(&mut self, idx: usize) -> Option<styles::Rgb> {
        self.load(idx).ok()?;
        let text = &self.loaded[&idx].0;
        let mut r = Reader::new(text);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "tabColor" => return styles::color(&tag, &self.theme),
                "sheetData" => return None,
                _ => {}
            }
        }
        None
    }

    /// Sets (or takes away) sheet `idx`'s tab color. One undo step.
    pub fn set_tab_color(&mut self, idx: usize, color: Option<[u8; 3]>) -> Result<()> {
        self.load(idx)?;
        let text = self.loaded[&idx].0.clone();
        let p = self.loaded[&idx].1.prefix.clone();
        let child = color.map_or(String::new(), |[r, g, b]| {
            format!("<{p}tabColor rgb=\"FF{r:02X}{g:02X}{b:02X}\"/>")
        });
        let mut reader = Reader::new(&text);
        let mut found = None;
        while let Some(t) = reader.next_token() {
            let Token::Start(tag) = t else { continue };
            if tag.name == "sheetPr" {
                let start = tag.span.start;
                let (end, empty) = if tag.empty {
                    (tag.span.end, true)
                } else {
                    (reader.skip_element(), false)
                };
                found = Some((start, end, empty));
                break;
            }
            if matches!(
                tag.name,
                "dimension" | "sheetViews" | "sheetFormatPr" | "cols" | "sheetData"
            ) {
                break;
            }
        }
        let new = match found {
            Some((start, end, empty)) => {
                let el = if empty {
                    let open = text[start..end].trim_end_matches("/>").trim_end();
                    format!("{open}></{p}sheetPr>")
                } else {
                    text[start..end].to_owned()
                };
                let el = crate::chart::set_child(&el, &["tabColor"], &child, &[]);
                splice(&text, vec![(start..end, el)])
            }
            None if color.is_none() => return Ok(()),
            None => insert_top_level(
                &text,
                &[
                    "dimension",
                    "sheetViews",
                    "sheetFormatPr",
                    "cols",
                    "sheetData",
                ],
                &format!("<{p}sheetPr>{child}</{p}sheetPr>"),
            ),
        };
        self.in_one_step(|wb| {
            wb.replace_sheet_text(idx, new);
            wb.batch_changed = true;
            Ok(())
        })
    }
}
