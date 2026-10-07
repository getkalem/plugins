//! A command line over the plugin's library, for trying it before Kalem's
//! plugin host runs it (DESIGN.md, section 8, phase 0):
//!
//! ```sh
//! cargo run -p kalem-plugin-git --example git -- status [--open] [--ascii]
//! cargo run -p kalem-plugin-git --example git -- log [PATH] [-n N] [--graph]
//! cargo run -p kalem-plugin-git --example git -- show REV
//! cargo run -p kalem-plugin-git --example git -- blame PATH [REV]
//! cargo run -p kalem-plugin-git --example git -- stage TARGET...
//! cargo run -p kalem-plugin-git --example git -- unstage TARGET...
//! cargo run -p kalem-plugin-git --example git -- discard TARGET... [--yes]
//! cargo run -p kalem-plugin-git --example git -- delete TARGET... [--yes]
//! cargo run -p kalem-plugin-git --example git -- patch TARGET
//! cargo run -p kalem-plugin-git --example git -- commit MESSAGE
//! cargo run -p kalem-plugin-git --example git -- url PATH [LINE[-LINE]]
//! cargo run -p kalem-plugin-git --example git -- url --commit REV
//! ```
//!
//! `-C DIR` before the command runs it in another repository; `--trace`
//! prints the git commands that ran, as the process log shows them. A
//! TARGET is a region key of the status document (`section:untracked`,
//! `file:unstaged:src/lib.rs`, `hunk:unstaged:0:src/lib.rs`), or
//! `lines:unstaged:0:3,4:src/lib.rs` for lines 3 and 4 of hunk 0 (lines
//! counted from 0 under the `@@` line). What cannot be undone asks for
//! `--yes`.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;

use kalem_plugin_git::actions::{self, Act, Settings};
use kalem_plugin_git::git::cmd::{self, CommitKind, LogSpec};
use kalem_plugin_git::git::{blame, diff, log};
use kalem_plugin_git::model::{Folds, Repo, Section};
use kalem_plugin_git::native::{self, NativeRunner};
use kalem_plugin_git::patch::{self, Mode, Pick};
use kalem_plugin_git::refresh::{self, Options, Runner};
use kalem_plugin_git::target::{Target, file_key};
use kalem_plugin_git::url;
use kalem_plugin_git::views::{self, Glyphs, ViewOptions};

type Res<T> = Result<T, String>;

struct Cli {
    git: NativeRunner,
    root: String,
    args: Vec<String>,
    flags: Vec<String>,
}

impl Cli {
    fn flag(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }

    fn value(&self, f: &str) -> Option<String> {
        let i = self.flags.iter().position(|x| x == f)?;
        self.flags.get(i + 1).cloned()
    }

    fn opts(&self) -> ViewOptions {
        ViewOptions {
            glyphs: if self.flag("--ascii") {
                Glyphs::Ascii
            } else {
                Glyphs::Unicode
            },
            ..ViewOptions::default()
        }
    }

    fn repo(&mut self, folds: &Folds) -> Res<Repo> {
        let mut repo = refresh::load(&mut self.git, &self.root, 1, Options::default())?;
        refresh::load_diffs(&mut self.git, &mut repo, folds, 3)?;
        Ok(repo)
    }
}

fn main() -> ExitCode {
    let mut all: Vec<String> = std::env::args().skip(1).collect();
    let mut dir = ".".to_string();
    if all.first().is_some_and(|a| a == "-C") && all.len() >= 2 {
        dir = all.remove(1);
        all.remove(0);
    }
    let (flags, args): (Vec<String>, Vec<String>) = {
        let mut flags = Vec::new();
        let mut args = Vec::new();
        let mut it = all.into_iter();
        while let Some(a) = it.next() {
            if a == "-n" || a == "--commit" {
                flags.push(a);
                flags.extend(it.next());
            } else if a.starts_with("--") {
                flags.push(a);
            } else {
                args.push(a);
            }
        }
        (flags, args)
    };
    let mut git = NativeRunner::new(&dir);
    let root = match native::find_root(&mut git) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    git.cwd = root.clone().into();
    let mut cli = Cli {
        git,
        root,
        args,
        flags,
    };
    let result = run(&mut cli);
    if cli.flag("--trace") {
        let c = views::process::render(&cli.git.log, &Folds::default(), &cli.opts());
        eprint!("{}", c.text);
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &mut Cli) -> Res<()> {
    let Some(command) = cli.args.first().cloned() else {
        return Err("a command: status, log, show, blame, stage, unstage, discard, delete, patch, commit, url".into());
    };
    let rest: Vec<String> = cli.args[1..].to_vec();
    match command.as_str() {
        "status" => {
            let mut folds = Folds::default();
            if cli.flag("--open") {
                let repo = cli.repo(&folds)?;
                for s in [
                    Section::Unmerged,
                    Section::Untracked,
                    Section::Unstaged,
                    Section::Staged,
                ] {
                    for e in repo.entries(s) {
                        folds.set(&file_key(s, &e.path), false);
                    }
                }
            }
            let repo = cli.repo(&folds)?;
            print!("{}", views::status::render(&repo, &folds, &cli.opts()).text);
        }
        "log" => {
            let count = cli.value("-n").and_then(|n| n.parse().ok()).unwrap_or(30);
            let spec = LogSpec {
                count,
                graph: cli.flag("--graph"),
                path: rest.first().cloned(),
                ..LogSpec::default()
            };
            let out = ok(cli.git.run(&cmd::log(&spec))?)?;
            let mut l = views::log::Log::default();
            l.extend(log::parse(&out)?, count);
            print!(
                "{}",
                views::log::render(&l, &Folds::default(), &cli.opts()).text
            );
        }
        "show" => {
            let rev = rest.first().map_or("HEAD", String::as_str);
            let out = ok(cli.git.run(&cmd::show(rev, 3))?)?;
            let d = diff::parse(&out)?;
            print!(
                "{}",
                views::commit::render(&d, &Folds::default(), &cli.opts()).text
            );
        }
        "blame" => {
            let path = rest.first().ok_or("blame PATH [REV]")?;
            let out = ok(cli
                .git
                .run(&cmd::blame(rest.get(1).map(String::as_str), path))?)?;
            let b = blame::parse(&out)?;
            print!("{}", views::blame::render(&b, &cli.opts()).text);
        }
        "stage" | "unstage" | "discard" | "delete" => {
            let act = match command.as_str() {
                "stage" => Act::Stage,
                "unstage" => Act::Unstage,
                "discard" => Act::Discard,
                _ => Act::Delete,
            };
            let targets = rest
                .iter()
                .map(|t| parse_target(t))
                .collect::<Res<Vec<_>>>()?;
            let folds = folds_for(&targets);
            let repo = cli.repo(&folds)?;
            let plan = actions::plan(act, &targets, &repo, Settings::default())?;
            if let Some(q) = &plan.confirm
                && !cli.flag("--yes")
            {
                return Err(format!("{q}\n(run again with --yes)"));
            }
            native::run_plan(&mut cli.git, &plan.commands)?;
            println!("{}", plan.done);
        }
        "patch" => {
            let t = parse_target(rest.first().ok_or("patch TARGET")?)?;
            let folds = folds_for(std::slice::from_ref(&t));
            let repo = cli.repo(&folds)?;
            let (section, path) = t.file().ok_or("a hunk or lines")?;
            let d = repo.diff(section, path).ok_or("no diff")?;
            let mode = if section == Section::Staged {
                Mode::Reverse
            } else {
                Mode::Forward
            };
            let pick = match &t {
                Target::Hunk { hunk, .. } => (*hunk, Pick::All),
                Target::Lines { hunk, lines, .. } => (*hunk, Pick::Lines(lines.clone())),
                _ => return Err("a hunk or lines".into()),
            };
            let p = patch::build(d, &[pick], mode).map_err(|e| e.to_string())?;
            print!("{}", String::from_utf8_lossy(&p));
        }
        "commit" => {
            let message = rest.first().ok_or("commit MESSAGE")?;
            let repo = cli.repo(&Folds::default())?;
            let plan = actions::commit(&format!("{message}\n"), &CommitKind::New, false, &repo)?;
            native::run_plan(&mut cli.git, &plan.commands)?;
            println!("{}", plan.done);
        }
        "url" => {
            let remote_name = {
                let out = cli.git.run(&cmd::upstream())?;
                let upstream = String::from_utf8_lossy(&out.stdout).trim().to_string();
                upstream
                    .split_once('/')
                    .map_or("origin".to_string(), |(r, _)| r.to_string())
            };
            let out = ok(cli.git.run(&cmd::remote_url(&remote_name))?)?;
            let remote_url = String::from_utf8_lossy(&out).trim().to_string();
            let remote = url::parse_remote(&remote_url)
                .ok_or(format!("{remote_url} is not a web site's"))?;
            let forge = url::forge(&remote.host, &[])
                .ok_or(format!("no web address is known for {}", remote.host))?;
            let link = if let Some(rev) = cli.value("--commit") {
                let out =
                    ok(cli
                        .git
                        .run(&cmd::GitCommand::read(["rev-parse", "--verify", &rev]))?)?;
                let hash = String::from_utf8_lossy(&out).trim().to_string();
                url::web_url(&remote, forge, &url::Link::Commit(&hash))
            } else {
                let path = rest.first().ok_or("url PATH [LINE[-LINE]]")?;
                let lines = rest.get(1).and_then(|l| {
                    let (a, b) = l.split_once('-').unwrap_or((l, l));
                    Some((a.parse().ok()?, b.parse().ok()?))
                });
                let out = ok(cli.git.run(&cmd::GitCommand::read([
                    "rev-parse",
                    "--abbrev-ref",
                    "HEAD",
                ]))?)?;
                let branch = String::from_utf8_lossy(&out).trim().to_string();
                url::web_url(
                    &remote,
                    forge,
                    &url::Link::File {
                        rev: &branch,
                        is_commit: false,
                        path,
                        lines,
                    },
                )
            };
            println!("{link}");
        }
        other => return Err(format!("unknown command `{other}`")),
    }
    Ok(())
}

fn ok(out: refresh::Output) -> Res<Vec<u8>> {
    if out.success() {
        Ok(out.stdout)
    } else {
        Err(out.message())
    }
}

/// A region key, or `lines:SECTION:HUNK:L1,L2:PATH`.
fn parse_target(s: &str) -> Res<Target> {
    if let Some(rest) = s.strip_prefix("lines:") {
        let mut f = rest.splitn(4, ':');
        let (section, hunk, lines, path) = (f.next(), f.next(), f.next(), f.next());
        let (Some(section), Some(hunk), Some(lines), Some(path)) = (section, hunk, lines, path)
        else {
            return Err(format!("`{s}`: lines:SECTION:HUNK:L1,L2:PATH"));
        };
        return Ok(Target::Lines {
            section: Section::from_key(section).ok_or(format!("no section `{section}`"))?,
            path: path.to_string(),
            hunk: hunk
                .parse()
                .map_err(|_| format!("`{hunk}` is not a hunk's number"))?,
            lines: lines
                .split(',')
                .map(|l| {
                    l.parse()
                        .map_err(|_| format!("`{l}` is not a line's number"))
                })
                .collect::<Res<Vec<usize>>>()?,
        });
    }
    Target::from_key(s).ok_or_else(|| format!("`{s}` names nothing"))
}

/// The folds that load the targets' diffs.
fn folds_for(targets: &[Target]) -> Folds {
    let mut folds = Folds::default();
    for t in targets {
        if let Some((s, p)) = t.file() {
            folds.set(&file_key(s, p), false);
        }
    }
    folds
}
