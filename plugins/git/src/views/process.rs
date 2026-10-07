//! The process log (DESIGN.md, 2.7): the git commands the plugin ran, the
//! newest first, each with its folder, time, exit status, and its output
//! folded under it.

use std::collections::VecDeque;

use crate::content::{Content, Style};
use crate::model::Folds;
use crate::views::{ViewOptions, lossy_line};

/// Runs kept at most.
pub const KEPT: usize = 100;

/// Output lines kept per stream of a run.
const OUTPUT_LINES: usize = 200;

/// A git command the plugin ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// Its number, counting from 1 since the plugin started.
    pub number: u64,
    /// The command line, as [`crate::git::cmd::GitCommand`] shows it.
    pub command: String,
    /// The folder it ran in.
    pub cwd: String,
    /// How long it took, when known.
    pub millis: Option<u64>,
    /// Its exit status; none when it could not start or was stopped.
    pub status: Option<i32>,
    /// What it printed, cut to its first lines.
    pub stdout: Vec<u8>,
    /// What it printed as errors.
    pub stderr: Vec<u8>,
    /// Why it could not start.
    pub error: Option<String>,
}

/// The last runs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessLog {
    runs: VecDeque<Run>,
    next: u64,
}

impl ProcessLog {
    /// Keeps a run, the oldest dropped past [`KEPT`]; its number.
    pub fn push(&mut self, mut run: Run) -> u64 {
        self.next += 1;
        run.number = self.next;
        run.stdout = first_lines(&run.stdout);
        run.stderr = first_lines(&run.stderr);
        self.runs.push_front(run);
        self.runs.truncate(KEPT);
        self.next
    }

    /// The runs, the newest first.
    pub fn runs(&self) -> impl Iterator<Item = &Run> {
        self.runs.iter()
    }
}

fn first_lines(out: &[u8]) -> Vec<u8> {
    match out
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .nth(OUTPUT_LINES - 1)
    {
        Some((i, _)) => {
            let mut v = out[..=i].to_vec();
            v.extend_from_slice("…\n".as_bytes());
            v
        }
        None => out.to_vec(),
    }
}

/// The process log document.
pub fn render(log: &ProcessLog, folds: &Folds, opts: &ViewOptions) -> Content {
    let mut c = Content::new();
    if log.runs.is_empty() {
        c.line("No git command has run yet.", Style::Muted);
        return c;
    }
    for run in log.runs() {
        let key = format!("run:{}", run.number);
        let folded = folds.folded(&key);
        c.open(&key, true, folded);
        c.push(opts.glyphs.fold(folded), Style::Muted);
        c.push(" ", Style::Normal);
        c.push(&run.command, Style::Code);
        let ended = match (&run.error, run.status) {
            (Some(e), _) => format!("could not start: {e}"),
            (None, Some(0)) => "ok".to_string(),
            (None, Some(s)) => format!("status {s}"),
            (None, None) => "stopped".to_string(),
        };
        let time = run.millis.map(|m| format!("{m} ms, ")).unwrap_or_default();
        let style = if run.status == Some(0) {
            Style::Muted
        } else {
            Style::Error
        };
        c.line(&format!("   ({time}{ended})"), style);
        if !folded {
            c.line(&format!("    in {}", run.cwd), Style::Muted);
            for (out, style) in [(&run.stderr, Style::Error), (&run.stdout, Style::Normal)] {
                for l in out.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
                    c.line(&format!("    {}", lossy_line(l)), style);
                }
            }
        }
        c.close();
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(command: &str, status: Option<i32>) -> Run {
        Run {
            number: 0,
            command: command.into(),
            cwd: "/r".into(),
            millis: Some(4),
            status,
            stdout: b"out\n".to_vec(),
            stderr: b"err\n".to_vec(),
            error: None,
        }
    }

    #[test]
    fn newest_first_folded() {
        let mut log = ProcessLog::default();
        log.push(run("git status", Some(0)));
        let n = log.push(run("git push", Some(1)));
        let mut folds = Folds::default();
        folds.set(&format!("run:{n}"), false);
        let c = render(&log, &folds, &ViewOptions::default());
        assert_eq!(
            c.text,
            "▾ git push   (4 ms, status 1)\n    in /r\n    err\n    out\n▸ git status   (4 ms, ok)\n"
        );
    }

    #[test]
    fn only_the_last_runs_and_lines_are_kept() {
        let mut log = ProcessLog::default();
        for _ in 0..KEPT + 5 {
            log.push(run("git x", Some(0)));
        }
        assert_eq!(log.runs().count(), KEPT);
        assert_eq!(log.runs().next().unwrap().number, (KEPT + 5) as u64);
        let long = "l\n".repeat(OUTPUT_LINES + 10);
        let mut r = run("git log", Some(0));
        r.stdout = long.into_bytes();
        log.push(r);
        let kept = &log.runs().next().unwrap().stdout;
        assert_eq!(
            kept.iter().filter(|b| **b == b'\n').count(),
            OUTPUT_LINES + 1
        );
    }
}
