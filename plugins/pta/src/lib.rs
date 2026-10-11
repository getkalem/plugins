//! Plain text accounting in Kalem: hledger, Ledger and Beancount
//! journals. The work list is `pta_todo.md`.
//!
//! The rule the plugin keeps (the git plugin's G1): **the tool does the
//! accounting**. hledger, Ledger and Beancount parse, balance and check;
//! the plugin asks them and shows what they say. This crate is the
//! component's logic, the same natively and in Kalem:
//!
//! - [`Dialect`]: the three journal formats, and the tool of each.
//!
//! - [`journal`]: the main journal and the tool that reads it (PT3c).
//!
//! - [`align`]: amounts aligned at a column, nothing else moved (PT4).
//!
//! - [`check`]: the tool's check, its problems and their document (PT5).
//!
//! - [`write`]: transactions written, toggled, copied, dated and sorted
//!   (PT6).
//!
//! - `component` (built for WebAssembly only): Kalem's `extension` world.

pub mod align;
pub mod check;
#[cfg(target_arch = "wasm32")]
mod component;
pub mod journal;
pub mod write;

/// The manifest, embedded so that the component carries its own
/// description.
pub const MANIFEST: &str = include_str!("../plugin.json");

/// The text types of a journal, for the commands' scope: the languages'
/// ids, and the extensions themselves, which Kalem takes as a plain text
/// file's type (`textType` is `journal` for `2026.journal`) until a
/// plugin's language names the type of the files it serves (K7).
pub const TEXT_TYPES: &[&str] = &[
    "hledger",
    "journal",
    "j",
    "ledger",
    "ldg",
    "beancount",
    "bean",
];

/// The when-clause of a journal: its text type is one of [`TEXT_TYPES`].
pub fn journal_when() -> String {
    let types: Vec<String> = TEXT_TYPES
        .iter()
        .map(|t| format!("textType == {t}"))
        .collect();
    format!("({})", types.join(" || "))
}

/// A journal's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// hledger's journal (`.journal`, `.hledger`, `.j`).
    Hledger,
    /// Ledger's (`.ledger`, `.ldg`).
    Ledger,
    /// Beancount's (`.beancount`, `.bean`).
    Beancount,
}

impl Dialect {
    /// All three, hledger first.
    pub const ALL: [Dialect; 3] = [Dialect::Hledger, Dialect::Ledger, Dialect::Beancount];

    /// The dialect of Kalem's language `id`, as the manifest names it.
    pub fn of_language(id: &str) -> Option<Dialect> {
        match id {
            "hledger" => Some(Dialect::Hledger),
            "ledger" => Some(Dialect::Ledger),
            "beancount" => Some(Dialect::Beancount),
            _ => None,
        }
    }

    /// The dialect of a file, by its name: the manifest's extensions and
    /// file names.
    pub fn of_path(path: &str) -> Option<Dialect> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        if name == ".hledger.journal" {
            return Some(Dialect::Hledger);
        }
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        match ext.as_str() {
            "journal" | "hledger" | "j" => Some(Dialect::Hledger),
            "ledger" | "ldg" => Some(Dialect::Ledger),
            "beancount" | "bean" => Some(Dialect::Beancount),
            _ => None,
        }
    }

    /// Kalem's language id.
    pub fn language(self) -> &'static str {
        match self {
            Dialect::Hledger => "hledger",
            Dialect::Ledger => "ledger",
            Dialect::Beancount => "beancount",
        }
    }

    /// The program that checks a journal of this dialect, as the
    /// manifest's `subprocess:NAME` names it.
    pub fn checker(self) -> &'static str {
        match self {
            Dialect::Hledger => "hledger",
            Dialect::Ledger => "ledger",
            Dialect::Beancount => "bean-check",
        }
    }

    /// How the tool is installed, for the notice when it is missing.
    pub fn install(self) -> &'static str {
        match self {
            Dialect::Hledger => {
                "hledger is not installed: brew install hledger, or a release from https://hledger.org/install"
            }
            Dialect::Ledger => {
                "Ledger is not installed: brew install ledger, or your system's package of ledger"
            }
            Dialect::Beancount => {
                "Beancount is not installed: pip install beancount (bean-check comes with it)"
            }
        }
    }
}

/// What a tool's `--version` printed, its first line, or why it says
/// nothing: the tool missing, or failing.
pub fn version_said(dialect: Dialect, status: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    let first = |b: &[u8]| {
        String::from_utf8_lossy(b)
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
    };
    match status {
        Some(0) => first(stdout)
            .or_else(|| first(stderr))
            .unwrap_or_else(|| format!("{} answered nothing", dialect.checker())),
        _ => match first(stderr) {
            Some(e) => format!("{} failed: {e}", dialect.checker()),
            None => format!("{} failed", dialect.checker()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialects_by_language_and_file() {
        assert_eq!(Dialect::of_path("/x/2026.journal"), Some(Dialect::Hledger));
        assert_eq!(
            Dialect::of_path("/x/.hledger.journal"),
            Some(Dialect::Hledger)
        );
        assert_eq!(Dialect::of_path("b.LEDGER"), Some(Dialect::Ledger));
        assert_eq!(Dialect::of_path("h.bean"), Some(Dialect::Beancount));
        // `.dat` is not claimed: too many files are `.dat`.
        assert_eq!(Dialect::of_path("x.dat"), None);
        for d in Dialect::ALL {
            assert_eq!(Dialect::of_language(d.language()), Some(d));
        }
    }

    #[test]
    fn what_a_version_says() {
        assert_eq!(
            version_said(
                Dialect::Hledger,
                Some(0),
                b"hledger 1.52.4, mac-aarch64\n",
                b""
            ),
            "hledger 1.52.4, mac-aarch64"
        );
        assert_eq!(
            version_said(Dialect::Beancount, Some(0), b"", b"Beancount 3.2.3\n"),
            "Beancount 3.2.3"
        );
        assert_eq!(
            version_said(Dialect::Ledger, Some(1), b"", b"\nboom\n"),
            "ledger failed: boom"
        );
    }
}
