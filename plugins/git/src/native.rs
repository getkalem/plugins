//! Git run with `std::process`, for the example and the tests: what Kalem's
//! `process` interface does for the component (DESIGN.md, 9.1), done here
//! and waited for.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::git::cmd::{self, GitCommand};
use crate::refresh::{Output, Runner};
use crate::views::process::{ProcessLog, Run};

/// Runs git in one folder, keeping the runs in a process log.
#[derive(Debug)]
pub struct NativeRunner {
    /// The git program.
    pub program: PathBuf,
    /// The folder the commands run in.
    pub cwd: PathBuf,
    /// Variables added to each run's environment, after the command's
    /// own (the tests isolate git from the user's configuration here).
    pub env: Vec<(String, String)>,
    /// The runs.
    pub log: ProcessLog,
}

impl NativeRunner {
    /// A runner of the `git` on the PATH in `cwd`.
    pub fn new(cwd: impl Into<PathBuf>) -> NativeRunner {
        NativeRunner {
            program: PathBuf::from("git"),
            cwd: cwd.into(),
            env: Vec::new(),
            log: ProcessLog::default(),
        }
    }
}

impl Runner for NativeRunner {
    fn run(&mut self, command: &GitCommand) -> Result<Output, String> {
        let started = Instant::now();
        let result = spawn(&self.program, &self.cwd, command, &self.env);
        let millis = u64::try_from(started.elapsed().as_millis()).ok();
        let (status, stdout, stderr, error) = match &result {
            Ok(o) => (o.status, o.stdout.clone(), o.stderr.clone(), None),
            Err(e) => (None, Vec::new(), Vec::new(), Some(e.clone())),
        };
        self.log.push(Run {
            number: 0,
            command: command.to_string(),
            cwd: self.cwd.display().to_string(),
            millis,
            status,
            stdout,
            stderr,
            error,
        });
        result
    }
}

fn spawn(
    program: &Path,
    cwd: &Path,
    command: &GitCommand,
    extra: &[(String, String)],
) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(command.argv())
        .current_dir(cwd)
        .envs(command.env())
        .envs(extra.iter().cloned())
        .stdin(if command.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    // Written on its own thread, so that a command that writes before it
    // has read everything cannot block both sides.
    let writer = match (child.stdin.take(), command.stdin.clone()) {
        (Some(mut stdin), Some(bytes)) => Some(std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        })),
        _ => None,
    };
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if let Some(w) = writer {
        let _ = w.join();
    }
    Ok(Output {
        status: out.status.code(),
        stdout: out.stdout,
        stderr: out.stderr,
    })
}

/// The root of the repository holding `dir`.
pub fn find_root(runner: &mut NativeRunner) -> Result<String, String> {
    let out = runner.run(&cmd::toplevel())?;
    if !out.success() {
        return Err(out.message());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Runs a plan's commands in order; the first failure's message.
pub fn run_plan(runner: &mut impl Runner, commands: &[GitCommand]) -> Result<(), String> {
    for c in commands {
        let out = runner.run(c)?;
        if !out.success() {
            return Err(out.message());
        }
    }
    Ok(())
}
