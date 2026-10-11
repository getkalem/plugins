//! Templates of new pages and journals: Logseq's (a block with
//! `template:: NAME`, its children copied as the new page's blocks, its
//! dynamic variables written as their values) and Obsidian's (a note in
//! the templates' folder, `{{title}}` and `{{date}}` filled in).

use crate::config::Kind;
use crate::date::{Date, Dialect, Pattern};
use crate::files::Files;
use crate::index::Index;

/// What a template's variables are written as.
#[derive(Debug, Clone)]
pub struct Values {
    /// The new page's title.
    pub title: String,
    /// Today, when known.
    pub today: Option<Date>,
    /// The day the page is for (a journal's), else today.
    pub date: Option<Date>,
    /// The local time of day in seconds, when known (Kalem's plugin API
    /// 0.2.11 gives the time zone's offset).
    pub time: Option<u32>,
}

/// The time of day `secs` written in Moment's tokens (`HH:mm`, `h:mm A`,
/// `HH:mm:ss`); other characters as they are.
fn time_text(secs: u32, format: &str) -> String {
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    let mut out = String::new();
    let mut rest = format;
    while let Some(c) = rest.chars().next() {
        let tokens: [(&str, String); 9] = [
            ("HH", format!("{h:02}")),
            ("H", h.to_string()),
            ("hh", format!("{h12:02}")),
            ("h", h12.to_string()),
            ("mm", format!("{m:02}")),
            ("m", m.to_string()),
            ("ss", format!("{s:02}")),
            ("A", if h < 12 { "AM" } else { "PM" }.to_string()),
            ("a", if h < 12 { "am" } else { "pm" }.to_string()),
        ];
        match tokens.iter().find(|(t, _)| rest.starts_with(t)) {
            Some((t, v)) => {
                out.push_str(v);
                rest = &rest[t.len()..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// The lines of block `i` of `lines` and of every block under it: from its
/// first line to the end of its last descendant.
fn subtree(blocks: &[crate::scan::Block], i: usize) -> (u32, u32) {
    let mut end = blocks[i].end;
    for (j, b) in blocks.iter().enumerate().skip(i + 1) {
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
        end = end.max(blocks[j].end);
    }
    (blocks[i].line, end)
}

/// The properties a template's copy leaves out.
fn dropped(line: &str) -> bool {
    let t = line.trim_start().to_lowercase();
    ["template::", "template-including-parent::", "id::"]
        .iter()
        .any(|p| t.starts_with(p))
}

/// Logseq's template `name`, filled in: `None` when the graph has no
/// block with `template:: name`.
pub fn logseq(index: &Index, files: &dyn Files, name: &str, v: &Values) -> Option<String> {
    let want = name.trim().to_lowercase();
    let (rel, i) = index.files().find_map(|(rel, d)| {
        d.scanned
            .blocks
            .iter()
            .position(|b| {
                b.props
                    .iter()
                    .any(|(k, val)| k == "template" && val.trim().to_lowercase() == want)
            })
            .map(|i| (rel.clone(), i))
    })?;
    let d = index.file(&rel)?;
    let blocks = &d.scanned.blocks;
    let text = files.read(&index.graph.path(&rel)).ok()?;
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let (start, end) = subtree(blocks, i);
    let with_parent = blocks[i]
        .props
        .iter()
        .any(|(k, val)| k == "template-including-parent" && val.trim() == "true");
    let first_child = blocks
        .get(i + 1)
        .filter(|b| b.parent == Some(i))
        .map(|b| b.line);
    let from = if with_parent { start } else { first_child? };
    let mut body: Vec<&str> = lines
        .get(from as usize..(end as usize).min(lines.len()))?
        .iter()
        .copied()
        .filter(|l| !dropped(l))
        .collect();
    while body.last().is_some_and(|l| l.trim().is_empty()) {
        body.pop();
    }
    // The copied blocks start at the top level.
    let indent: String = body
        .first()
        .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect())
        .unwrap_or_default();
    let mut out = String::new();
    for l in body {
        out.push_str(l.strip_prefix(indent.as_str()).unwrap_or(l));
        out.push('\n');
    }
    Some(logseq_variables(&out, index, v))
}

fn logseq_variables(text: &str, index: &Index, v: &Values) -> String {
    let title = &index.graph.journal_title;
    let day = |d: Option<Date>| {
        d.map(|d| format!("[[{}]]", title.format(d)))
            .unwrap_or_default()
    };
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find("<%") {
        let Some(q) = rest[p..].find("%>") else { break };
        out.push_str(&rest[..p]);
        let var = rest[p + 2..p + q].trim().to_lowercase();
        let value = match var.as_str() {
            "today" => day(v.today),
            "yesterday" => day(v.today.map(|d| d.add_days(-1))),
            "tomorrow" => day(v.today.map(|d| d.add_days(1))),
            "current page" => format!("[[{}]]", v.title),
            // Logseq's 24-hour `HH:mm`; nothing where the local time
            // is not known.
            "time" => v.time.map(|t| time_text(t, "HH:mm")).unwrap_or_default(),
            _ => rest[p..p + q + 2].to_string(),
        };
        out.push_str(&value);
        rest = &rest[p + q + 2..];
    }
    out.push_str(rest);
    out
}

/// Obsidian's template note `name` (relative to the root, without
/// `.md`, or in the templates' folder), filled in.
pub fn obsidian(index: &Index, files: &dyn Files, name: &str, v: &Values) -> Option<String> {
    let g = &index.graph;
    let candidates = [
        format!("{name}.md"),
        g.templates_dir
            .as_ref()
            .map(|d| format!("{d}/{name}.md"))
            .unwrap_or_default(),
    ];
    let text = candidates
        .iter()
        .filter(|c| !c.is_empty())
        .find_map(|c| files.read(&g.path(c)).ok())?;
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(p) = rest.find("{{") {
        let Some(q) = rest[p..].find("}}") else { break };
        out.push_str(&rest[..p]);
        let var = rest[p + 2..p + q].trim();
        let (name, format) = match var.split_once(':') {
            Some((n, f)) => (n.trim().to_lowercase(), Some(f.trim())),
            None => (var.to_lowercase(), None),
        };
        let value = match name.as_str() {
            "title" => v.title.clone(),
            "date" => v
                .date
                .or(v.today)
                .map(|d| Pattern::new(format.unwrap_or("YYYY-MM-DD"), Dialect::Moment).format(d))
                .unwrap_or_default(),
            "time" => v
                .time
                .map(|t| time_text(t, format.unwrap_or("HH:mm")))
                .unwrap_or_default(),
            _ => rest[p..p + q + 2].to_string(),
        };
        out.push_str(&value);
        rest = &rest[p + q + 2..];
    }
    out.push_str(rest);
    Some(out)
}

/// The template of a new journal, filled in, when the graph names one.
pub fn journal(index: &Index, files: &dyn Files, v: &Values) -> Option<String> {
    let name = index.graph.journal_template.clone()?;
    match index.graph.kind {
        Kind::Logseq => logseq(index, files, &name, v),
        Kind::Obsidian => obsidian(index, files, &name, v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Fallbacks, Graph};
    use crate::files::Memory;

    #[test]
    fn logseq_templates() {
        let m = Memory::new(&[
            (
                "/g/logseq/config.edn",
                "{:default-templates {:journals \"Daily\"}}",
            ),
            (
                "/g/pages/Templates.md",
                "- Daily\n  template:: daily\n  id:: 11111111-2222-3333-4444-555555555555\n\t- ## Morning <% today %>\n\t\t- TODO plan\n\t- Notes on <% current page %> at <% time %>\n- Other\n",
            ),
            (
                "/g/pages/T2.md",
                "- Meeting\n  template:: meeting\n  template-including-parent:: true\n  type:: meeting\n\t- agenda\n",
            ),
        ]);
        let i = Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        );
        let mut v = Values {
            title: "Oct 10th, 2026".into(),
            today: Date::new(2026, 10, 10),
            date: Date::new(2026, 10, 10),
            time: None,
        };
        assert_eq!(
            journal(&i, &m, &v).unwrap(),
            "- ## Morning [[Oct 10th, 2026]]\n\t- TODO plan\n- Notes on [[Oct 10th, 2026]] at \n"
        );
        // With the local time: Logseq's `HH:mm`.
        v.time = Some(9 * 3600 + 5 * 60 + 7);
        assert!(journal(&i, &m, &v).unwrap().ends_with(" at 09:05\n"));
        v.time = None;
        assert_eq!(
            logseq(&i, &m, "Meeting", &v).unwrap(),
            "- Meeting\n  type:: meeting\n\t- agenda\n"
        );
        assert!(logseq(&i, &m, "none", &v).is_none());
    }

    #[test]
    fn obsidian_templates() {
        let m = Memory::new(&[
            (
                "/v/.obsidian/daily-notes.json",
                r#"{"template":"Templates/Daily"}"#,
            ),
            ("/v/.obsidian/templates.json", r#"{"folder":"Templates"}"#),
            (
                "/v/Templates/Daily.md",
                "# {{title}}\nCreated {{date}} ({{date:dddd}}) {{time}} {{other}}\n",
            ),
        ]);
        let i = Index::build(
            &m,
            Graph::load(&m, "/v", Kind::Obsidian, &Fallbacks::default()),
        );
        let mut v = Values {
            title: "2026-10-10".into(),
            today: Date::new(2026, 10, 10),
            date: Date::new(2026, 10, 10),
            time: None,
        };
        assert_eq!(
            journal(&i, &m, &v).unwrap(),
            "# 2026-10-10\nCreated 2026-10-10 (Saturday)  {{other}}\n"
        );
        // Obsidian's `{{time}}`, `HH:mm` unless its format says otherwise.
        v.time = Some(14 * 3600 + 30 * 60 + 9);
        assert_eq!(
            journal(&i, &m, &v).unwrap(),
            "# 2026-10-10\nCreated 2026-10-10 (Saturday) 14:30 {{other}}\n"
        );
        assert_eq!(
            time_text(14 * 3600 + 30 * 60 + 9, "h:mm:ss A"),
            "2:30:09 PM"
        );
        assert_eq!(time_text(0, "hh:mm a"), "12:00 am");
        assert_eq!(obsidian(&i, &m, "Daily", &v).unwrap().lines().count(), 2);
    }
}
