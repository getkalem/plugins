//! The main journal and the tool (PT3c): which file the tool, the
//! servers and every report are given, and which program reads it. A
//! journal's `include`s make one file the root; checking or reporting an
//! included file alone gives false errors (an account opened in another
//! file "unknown").
//!
//! The main journal is the plugin's setting `journal` when set (a path,
//! absolute or relative to the folder of the file the command runs in);
//! else, for hledger, the journal `LEDGER_FILE` names when it includes the
//! file (`hledger files` without `-f` lists it and its includes); else the
//! file itself. The tool is the setting `tool` (`auto`, `hledger`,
//! `ledger`, `beancount`); `auto` is the dialect's, and a `.ledger` file,
//! which hledger users name so too, goes to Ledger where it is installed
//! and to hledger where only hledger is (hledger does not read Ledger's
//! value expressions).

use crate::Dialect;

/// The setting `tool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolChoice {
    /// The dialect's own.
    Auto,
    /// This one, for hledger's and Ledger's files (a Beancount file is
    /// always Beancount's).
    Only(Dialect),
}

impl ToolChoice {
    /// The setting's value; anything else is `auto`.
    pub fn parse(s: &str) -> ToolChoice {
        match s.trim() {
            "hledger" => ToolChoice::Only(Dialect::Hledger),
            "ledger" => ToolChoice::Only(Dialect::Ledger),
            "beancount" => ToolChoice::Only(Dialect::Beancount),
            _ => ToolChoice::Auto,
        }
    }
}

/// Which tools are installed, as far as the plugin knows: `None` until a
/// run said.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Installed {
    /// hledger answered `--version`.
    pub hledger: Option<bool>,
    /// Ledger answered `--version`.
    pub ledger: Option<bool>,
}

/// The tool that reads `path`, of `dialect`, under `choice`.
pub fn tool(choice: ToolChoice, dialect: Dialect, path: &str, installed: Installed) -> Dialect {
    match (dialect, choice) {
        (Dialect::Beancount, _) => Dialect::Beancount,
        (_, ToolChoice::Only(Dialect::Beancount)) | (_, ToolChoice::Auto) => {
            if dialect == Dialect::Ledger
                && extension(path) == "ledger"
                && installed.ledger == Some(false)
                && installed.hledger == Some(true)
            {
                Dialect::Hledger
            } else {
                dialect
            }
        }
        (_, ToolChoice::Only(d)) => d,
    }
}

/// Whether `tool` must be asked if it is installed before [`tool`]
/// answers for `path`: a `.ledger` file under `auto`.
pub fn needs_installed(choice: ToolChoice, dialect: Dialect, path: &str) -> bool {
    dialect == Dialect::Ledger
        && matches!(
            choice,
            ToolChoice::Auto | ToolChoice::Only(Dialect::Beancount)
        )
        && extension(path) == "ledger"
}

fn extension(path: &str) -> String {
    name(path)
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The file name of `path`.
pub fn name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The folder of `path`, without its separator.
pub fn folder(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or("", |(d, _)| d)
}

/// Why the main journal is the one it is, said in the status bar item's
/// tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The setting `journal`.
    Setting,
    /// `LEDGER_FILE`, which includes the file.
    LedgerFile,
    /// The file itself.
    Itself,
}

/// The main journal of the file at `current`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Main {
    /// Its path.
    pub path: String,
    /// Why it is the one.
    pub why: Why,
}

/// Whether `p` is absolute, or starts at the home folder (`~/`), which
/// the tools expand.
fn absolute(p: &str) -> bool {
    p.starts_with('/')
        || p.starts_with('\\')
        || p.starts_with('~')
        || p.as_bytes().get(1) == Some(&b':')
}

/// The main journal of `current` from the setting `journal` (empty when
/// unset), and the files `hledger files` listed for `LEDGER_FILE` (run in
/// `current`'s folder; `None` when it was not asked or failed).
pub fn main_journal(setting: &str, current: &str, ledger_file: Option<&[String]>) -> Main {
    let setting = setting.trim();
    if !setting.is_empty() {
        let path = if absolute(setting) {
            setting.to_string()
        } else {
            join(folder(current), setting)
        };
        return Main {
            path,
            why: Why::Setting,
        };
    }
    let dir = folder(current);
    if let Some(files) = ledger_file
        && let Some(first) = files.first()
        && files
            .iter()
            .any(|f| same_file(&resolve(dir, f), &resolve(dir, current)))
    {
        return Main {
            path: resolve(dir, first),
            why: Why::LedgerFile,
        };
    }
    Main {
        path: current.to_string(),
        why: Why::Itself,
    }
}

fn join(dir: &str, rel: &str) -> String {
    let rel = rel.strip_prefix("./").unwrap_or(rel);
    if dir.is_empty() {
        rel.to_string()
    } else {
        format!("{dir}/{rel}")
    }
}

/// `p` absolute, a relative one taken from `dir`, its `.` and `..` parts
/// resolved.
fn resolve(dir: &str, p: &str) -> String {
    let whole = if absolute(p) {
        p.to_string()
    } else {
        join(dir, p)
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in whole.split('/') {
        match part {
            "." => {}
            "" if !parts.is_empty() => {}
            ".." if parts.len() > 1 => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

/// Whether `a` and `b` name one file: macOS's `/private/tmp` and `/tmp`
/// (the tools print the first, the editor may hold the second) taken as
/// one.
fn same_file(a: &str, b: &str) -> bool {
    let strip = |p: &str| -> String {
        for top in ["/private/tmp/", "/private/var/", "/private/etc/"] {
            if let Some(rest) = p.strip_prefix(top) {
                return format!("/{}/{rest}", &top[9..top.len() - 1]);
            }
        }
        p.to_string()
    };
    strip(a) == strip(b)
}

/// The files `hledger files` printed, one a line.
pub fn files_listed(stdout: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// The status bar item's text: the tool and the main journal's name.
pub fn status_text(tool: Dialect, main: &Main) -> String {
    format!("{} · {}", tool.language(), name(&main.path))
}

/// The status bar item's tooltip.
pub fn status_tooltip(tool: Dialect, main: &Main) -> String {
    let why = match main.why {
        Why::Setting => "the setting journal",
        Why::LedgerFile => "LEDGER_FILE, which includes this file",
        Why::Itself => "this file; the setting journal names another",
    };
    format!(
        "Main journal: {} ({why}); read by {}",
        main.path,
        tool.checker()
    )
}

/// The plugin's settings this module reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// `journal`: the main journal's path, empty when unset.
    pub journal: String,
    /// `tool`.
    pub tool: String,
}

impl Settings {
    /// The settings through `own` (the plugin's own setting as JSON).
    pub fn read(own: impl Fn(&str) -> Option<String>) -> Settings {
        let text = |k: &str| {
            own(k)
                .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default()
        };
        Settings {
            journal: text("journal"),
            tool: text("tool"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: Installed = Installed {
        hledger: Some(true),
        ledger: Some(true),
    };
    const HLEDGER_ONLY: Installed = Installed {
        hledger: Some(true),
        ledger: Some(false),
    };

    #[test]
    fn the_tool_of_each_file() {
        use Dialect::*;
        let auto = ToolChoice::Auto;
        assert_eq!(tool(auto, Hledger, "/j/2026.journal", BOTH), Hledger);
        assert_eq!(tool(auto, Beancount, "/j/h.beancount", BOTH), Beancount);
        // A `.ledger` file: Ledger's where it is installed, hledger's
        // where only hledger is.
        assert_eq!(tool(auto, Ledger, "/j/b.ledger", BOTH), Ledger);
        assert_eq!(tool(auto, Ledger, "/j/b.ledger", HLEDGER_ONLY), Hledger);
        assert_eq!(tool(auto, Ledger, "/j/b.ldg", HLEDGER_ONLY), Ledger);
        assert_eq!(
            tool(auto, Ledger, "/j/b.ledger", Installed::default()),
            Ledger
        );
        // The setting, for hledger's and Ledger's files only.
        let h = ToolChoice::parse("hledger");
        assert_eq!(tool(h, Ledger, "/j/b.ledger", BOTH), Hledger);
        assert_eq!(tool(h, Beancount, "/j/h.bean", BOTH), Beancount);
        let b = ToolChoice::parse("beancount");
        assert_eq!(tool(b, Hledger, "/j/x.journal", BOTH), Hledger);
        assert_eq!(ToolChoice::parse("nonsense"), ToolChoice::Auto);
        assert!(needs_installed(auto, Ledger, "/j/b.ledger"));
        assert!(!needs_installed(auto, Ledger, "/j/b.ldg"));
        assert!(!needs_installed(h, Ledger, "/j/b.ledger"));
    }

    #[test]
    fn the_main_journal() {
        // The setting: absolute, from home, or beside the file.
        let m = main_journal("/money/main.journal", "/j/2026.journal", None);
        assert_eq!(
            (m.path.as_str(), m.why),
            ("/money/main.journal", Why::Setting)
        );
        let m = main_journal("~/main.journal", "/j/2026.journal", None);
        assert_eq!(m.path, "~/main.journal");
        let m = main_journal("../all.journal", "/j/y/2026.journal", None);
        assert_eq!(m.path, "/j/y/../all.journal");
        // LEDGER_FILE, as `hledger files` lists it (its first line as
        // given, the includes resolved, macOS's /private before /tmp).
        let listed = files_listed(
            b"./2026.journal\n/private/tmp/m/accounts.journal\n/private/tmp/m/2025.journal\n",
        );
        let m = main_journal("", "/tmp/m/2025.journal", Some(&listed));
        assert_eq!(
            (m.path.as_str(), m.why),
            ("/tmp/m/2026.journal", Why::LedgerFile)
        );
        let m = main_journal("", "/tmp/m/2026.journal", Some(&listed));
        assert_eq!(m.why, Why::LedgerFile);
        // A file LEDGER_FILE does not include is its own.
        let m = main_journal("", "/elsewhere/x.journal", Some(&listed));
        assert_eq!(
            (m.path.as_str(), m.why),
            ("/elsewhere/x.journal", Why::Itself)
        );
        let m = main_journal("  ", "/j/x.journal", None);
        assert_eq!(m.why, Why::Itself);
    }

    #[test]
    fn the_status_item() {
        let m = Main {
            path: "/j/2026.journal".into(),
            why: Why::LedgerFile,
        };
        assert_eq!(status_text(Dialect::Hledger, &m), "hledger · 2026.journal");
        assert_eq!(
            status_tooltip(Dialect::Hledger, &m),
            "Main journal: /j/2026.journal (LEDGER_FILE, which includes this file); read by hledger"
        );
        let m = Main {
            path: "/j/h.beancount".into(),
            why: Why::Itself,
        };
        assert_eq!(
            status_text(Dialect::Beancount, &m),
            "beancount · h.beancount"
        );
    }

    #[test]
    fn the_settings_read() {
        let s = Settings::read(|k| match k {
            "journal" => Some("\"~/money/main.journal\"".into()),
            "tool" => Some("\"ledger\"".into()),
            _ => None,
        });
        assert_eq!(s.journal, "~/money/main.journal");
        assert_eq!(
            ToolChoice::parse(&s.tool),
            ToolChoice::Only(Dialect::Ledger)
        );
        assert_eq!(Settings::read(|_| None), Settings::default());
    }
}
