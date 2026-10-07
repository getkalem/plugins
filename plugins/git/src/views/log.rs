//! The log document (DESIGN.md, 2.4): one commit a line, unfolded to its
//! message and stat; a last line loads more.

use std::collections::BTreeMap;

use crate::content::{Content, Style};
use crate::git::log::{Commit, LogLine};
use crate::model::Folds;
use crate::views::ViewOptions;

/// A log as the document shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Log {
    /// What it is the log of: `main`, `src/lib.rs`.
    pub title: String,
    /// Its lines, as loaded so far.
    pub lines: Vec<LogLine>,
    /// Every commit is loaded: no "Load more" line.
    pub complete: bool,
    /// The message and stat of the commits unfolded, by hash.
    pub details: BTreeMap<String, String>,
}

impl Log {
    /// Its commits.
    pub fn commits(&self) -> impl Iterator<Item = &Commit> {
        self.lines.iter().filter_map(|l| match l {
            LogLine::Commit(c) => Some(c),
            LogLine::Graph(_) => None,
        })
    }

    /// Adds a loaded page of `requested` lines; fewer means the end.
    pub fn extend(&mut self, page: Vec<LogLine>, requested: u32) {
        let commits = page
            .iter()
            .filter(|l| matches!(l, LogLine::Commit(_)))
            .count();
        self.complete = commits < requested as usize;
        self.lines.extend(page);
    }
}

/// The log document.
pub fn render(log: &Log, folds: &Folds, opts: &ViewOptions) -> Content {
    let _ = opts;
    let mut c = Content::new();
    let width = log
        .commits()
        .map(|c| c.author.chars().count())
        .max()
        .unwrap_or(0)
        .min(20);
    for line in &log.lines {
        match line {
            LogLine::Graph(g) => c.line(g, Style::Muted),
            LogLine::Commit(commit) => {
                let key = format!("commit:{}", commit.hash);
                let folded = folds.folded(&key);
                c.open(&key, true, folded);
                c.push(&commit.graph, Style::Muted);
                c.push(&commit.short, Style::Code);
                c.push("  ", Style::Normal);
                c.push(commit.day(), Style::Muted);
                c.push("  ", Style::Normal);
                let author: String = commit.author.chars().take(width).collect();
                c.push(&format!("{author:<width$}"), Style::Muted);
                c.push("  ", Style::Normal);
                c.push(&commit.subject, Style::Normal);
                if !commit.refs.is_empty() {
                    c.push(&format!("  ({})", commit.refs.join(", ")), Style::Strong);
                }
                c.newline();
                if !folded {
                    match log.details.get(&commit.hash) {
                        Some(d) => {
                            for l in d.trim_end().lines() {
                                c.line(&format!("    {l}"), Style::Normal);
                            }
                        }
                        None => c.line("    Loading…", Style::Muted),
                    }
                }
                c.close();
            }
        }
    }
    if !log.complete {
        c.open("more", false, false);
        c.line("Load more…", Style::Muted);
        c.close();
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::{self, Target};

    fn commit(hash: &str, author: &str, subject: &str) -> LogLine {
        LogLine::Commit(Commit {
            hash: hash.into(),
            short: hash[..3].into(),
            author: author.into(),
            date: "2026-10-07T10:00:00Z".into(),
            subject: subject.into(),
            ..Commit::default()
        })
    }

    #[test]
    fn commits_details_and_more() {
        let mut log = Log::default();
        log.extend(
            vec![commit("aaaa", "Ayşe", "One"), commit("bbbb", "Bo", "Two")],
            2,
        );
        assert!(!log.complete);
        log.details
            .insert("aaaa".into(), "One\n\n x | 1 +\n".into());
        let mut folds = Folds::default();
        folds.set("commit:aaaa", false);
        let c = render(&log, &folds, &ViewOptions::default());
        assert_eq!(
            c.text,
            "aaa  2026-10-07  Ayşe  One\n    One\n    \n     x | 1 +\nbbb  2026-10-07  Bo    Two\nLoad more…\n"
        );
        assert_eq!(target::at(&c, 0), Some(Target::Commit("aaaa".into())));
        let more = c.text.find("Load").unwrap();
        assert_eq!(c.region_at(more).unwrap().key, "more");
        log.extend(vec![commit("cccc", "C", "Three")], 2);
        assert!(log.complete);
    }
}
