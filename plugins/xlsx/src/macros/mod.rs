//! VBA macros run inside the plugin (Kalem's task T3.7.4b).
//!
//! The module sources come from the workbook's VBA project ([`crate::vba`]);
//! the project part is never rewritten. A macro runs only when the user asks
//! for it by name: nothing runs on open (`Workbook_Open` and `Auto_Open`
//! are listed, not run). Its writes are the plugin's cell edits, one undo
//! step for the whole run. It reaches the workbook and nothing else: file,
//! network, shell, `Declare` and other objects' automation stop the macro
//! with a message naming the call and the line. A step and time budget
//! stops endless loops.

pub mod ast;
pub mod builtins;
pub mod constants;
pub mod excel;
pub mod interp;
pub mod lexer;
pub mod parser;
pub mod value;

use std::fmt;

use crate::vba::{ModuleKind, Project};
use crate::workbook::Workbook;

/// What a macro asks of the user and tells them: the host's dialogs.
pub trait MacroHost {
    /// `MsgBox`: shows `prompt` with VBA's `buttons` flags; returns the
    /// button pressed (`vbOK` 1, `vbCancel` 2, `vbYes` 6, `vbNo` 7).
    fn msg_box(&mut self, prompt: &str, buttons: i64, title: &str) -> i64;
    /// `InputBox`: `None` when cancelled.
    fn input_box(&mut self, prompt: &str, title: &str, default: &str) -> Option<String>;
    /// `Debug.Print`.
    fn print(&mut self, _line: &str) {}
    /// `Application.StatusBar = …`.
    fn status(&mut self, _text: &str) {}
    /// The workbook's file name (`ThisWorkbook.Name`).
    fn workbook_name(&mut self) -> String {
        "Book1.xlsm".into()
    }
}

/// A host that answers every question with its default: OK, the default
/// text; for batch runs and tests.
#[derive(Debug, Default)]
pub struct QuietHost {
    /// The `MsgBox` prompts shown.
    pub messages: Vec<String>,
    /// Answers for `InputBox`, in order; then the default.
    pub answers: Vec<String>,
}

impl MacroHost for QuietHost {
    fn msg_box(&mut self, prompt: &str, buttons: i64, _title: &str) -> i64 {
        self.messages.push(prompt.to_owned());
        // OK for OK buttons, Yes for Yes/No.
        if buttons & 7 == 4 || buttons & 7 == 3 {
            6
        } else {
            1
        }
    }

    fn input_box(&mut self, _prompt: &str, _title: &str, default: &str) -> Option<String> {
        Some(if self.answers.is_empty() {
            default.to_owned()
        } else {
            self.answers.remove(0)
        })
    }
}

/// How far a macro may run.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Statements executed.
    pub steps: u64,
    /// Wall time in milliseconds.
    pub millis: u64,
    /// Nested calls.
    pub depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            steps: 50_000_000,
            millis: 60_000,
            depth: 256,
        }
    }
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    /// `Debug.Print` lines.
    pub output: Vec<String>,
    /// `MsgBox` prompts shown.
    pub messages: Vec<String>,
    /// Statements not applied (formatting), with their lines.
    pub skipped: Vec<String>,
    /// Whether cells changed.
    pub changed: bool,
    /// `ThisWorkbook.Save` was called: the host saves.
    pub save_requested: bool,
}

/// Why a macro did not run or stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroError {
    /// The module, when known.
    pub module: String,
    /// One-based line, 0 when not known.
    pub line: usize,
    /// The message.
    pub message: String,
    /// What the run did before it stopped (its edits are kept, as Excel keeps them).
    pub report: Box<RunReport>,
}

impl fmt::Display for MacroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.module.is_empty(), self.line) {
            (false, l) if l > 0 => write!(f, "{}, line {}: {}", self.module, l, self.message),
            (false, _) => write!(f, "{}: {}", self.module, self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for MacroError {}

/// A macro the user can run: a public `Sub` without required parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroInfo {
    /// `Module1.Name`.
    pub qualified: String,
    /// The module.
    pub module: String,
    /// The procedure.
    pub name: String,
    /// An event handler (`Workbook_Open`, `Worksheet_Change`): listed, never
    /// run by itself.
    pub event: bool,
}

fn parse_all(project: &Project) -> Result<Vec<ast::Module>, MacroError> {
    project
        .modules
        .iter()
        .map(|m| {
            parser::parse_module(&m.name, &m.source).map_err(|e| MacroError {
                module: m.name.clone(),
                line: e.line,
                message: e.message,
                report: Box::default(),
            })
        })
        .collect()
}

/// The macros of a project, as Excel's Macros dialog lists them.
pub fn list_macros(project: &Project) -> Result<Vec<MacroInfo>, MacroError> {
    let modules = parse_all(project)?;
    let mut out = Vec::new();
    for (m, src) in modules.iter().zip(&project.modules) {
        for p in &m.procs {
            if p.function
                || p.private
                || p.params
                    .iter()
                    .any(|x| x.optional.is_none() && !x.param_array)
            {
                continue;
            }
            let lower = p.name.to_ascii_lowercase();
            let event = lower.starts_with("workbook_")
                || lower.starts_with("worksheet_")
                || lower == "auto_open"
                || lower == "auto_close"
                || (src.kind == ModuleKind::Class && lower.contains('_'));
            out.push(MacroInfo {
                qualified: format!("{}.{}", m.name, p.name),
                module: m.name.clone(),
                name: p.name.clone(),
                event,
            });
        }
    }
    Ok(out)
}

/// Runs a macro (`Name` or `Module1.Name`) on the workbook. Its edits are
/// one undo step; on an error, the edits made before it are kept.
pub fn run_macro(
    wb: &mut Workbook,
    project: &Project,
    name: &str,
    host: &mut dyn MacroHost,
    limits: Limits,
) -> Result<RunReport, MacroError> {
    let modules = parse_all(project)?;
    let refused: Vec<(String, usize, String)> = modules
        .iter()
        .flat_map(|m| {
            m.refused
                .iter()
                .map(|(l, w)| (m.name.clone(), *l, w.clone()))
        })
        .collect();
    wb.begin_batch().map_err(|e| MacroError {
        module: String::new(),
        line: 0,
        message: e.to_string(),
        report: Box::default(),
    })?;
    let result = (|| {
        let mut it = interp::Interp::new(wb, host, modules, limits)
            .map_err(|e| Box::new((String::new(), e, RunReport::default())))?;
        let r = it.run(name, Vec::new());
        let report = std::mem::take(&mut it.report);
        let module = it.current_module();
        match r {
            Ok(_) => Ok(report),
            // `End`.
            Err(e) if e.number == -1 && e.fatal => Ok(report),
            Err(e) => Err(Box::new((module, e, report))),
        }
    })();
    let ended = wb.end_batch();
    match result {
        Ok(mut report) => {
            if let Ok(changed) = ended {
                report.changed |= changed;
            }
            if !refused.is_empty() {
                for (m, l, w) in refused {
                    report.skipped.push(format!(
                        "{m}, line {l}: {w} is not available to macros in Kalem"
                    ));
                }
            }
            Ok(report)
        }
        Err(boxed) => {
            let (module, e, report) = *boxed;
            let mut message = e.to_string();
            if let Some(stripped) = message.strip_suffix(&format!(" (line {})", e.line)) {
                message = stripped.to_owned();
            }
            Err(MacroError {
                module,
                line: e.line,
                message,
                report: Box::new(report),
            })
        }
    }
}
