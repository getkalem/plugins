//! The files of a graph, reached through [`Files`]: Kalem's `fs` interface
//! in the component, the file system natively. Paths are absolute and
//! written with `/`; a folder in a listing ends with `/`.

/// The largest file Kalem's `fs.read` reads, 16 MB; the readers here
/// keep to it, so that the tests see what Kalem gives.
pub const MAX_BYTES: usize = 16 << 20;

/// Why a file over [`MAX_BYTES`] is not read, as Kalem says it.
fn too_large(path: &str) -> String {
    format!("{path} is larger than {} MB", MAX_BYTES >> 20)
}

/// Reading files and folders.
pub trait Files {
    /// A text file's text.
    fn read(&self, path: &str) -> Result<String, String>;
    /// A folder's entries, as absolute paths, folders ending with `/`.
    fn list(&self, dir: &str) -> Result<Vec<String>, String>;
    /// Writes a text file, its folder made when missing.
    fn write(&self, path: &str, text: &str) -> Result<(), String>;
}

/// `path` with `/` for every separator and no `/` at its end (but the
/// root's).
pub fn normalize(path: &str) -> String {
    let p = path.replace('\\', "/");
    if p.len() > 1 && p.ends_with('/') && !p.ends_with(":/") {
        p.trim_end_matches('/').to_string()
    } else {
        p
    }
}

/// `dir` and `name` joined.
pub fn join(dir: &str, name: &str) -> String {
    let name = name.trim_start_matches('/');
    if dir.ends_with('/') || dir.is_empty() {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The folder `path` is in, when it has one.
pub fn parent(path: &str) -> Option<&str> {
    let p = path.trim_end_matches('/');
    let i = p.rfind('/')?;
    Some(if i == 0 { "/" } else { &p[..i] })
}

/// The last part of `path`.
pub fn file_name(path: &str) -> &str {
    let p = path.trim_end_matches('/');
    p.rsplit('/').next().unwrap_or(p)
}

/// The file name without its extension, and the extension (lower case).
pub fn stem_ext(path: &str) -> (&str, String) {
    let name = file_name(path);
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], name[i + 1..].to_lowercase()),
        _ => (name, String::new()),
    }
}

/// `path` relative to `root`, when it is under it.
pub fn relative<'a>(root: &str, path: &'a str) -> Option<&'a str> {
    let rest = path.strip_prefix(root)?;
    if root.ends_with('/') {
        return Some(rest);
    }
    rest.strip_prefix('/').or((rest.is_empty()).then_some(""))
}

/// The file system, for the tests and the command line.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

#[cfg(not(target_arch = "wasm32"))]
impl Files for Native {
    fn read(&self, path: &str) -> Result<String, String> {
        let len = std::fs::metadata(path)
            .map_err(|e| format!("{path}: {e}"))?
            .len();
        if len > MAX_BYTES as u64 {
            return Err(too_large(path));
        }
        std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, String> {
        let mut out: Vec<String> = std::fs::read_dir(dir)
            .map_err(|e| format!("{dir}: {e}"))?
            .filter_map(Result::ok)
            .map(|e| {
                let p = normalize(&e.path().to_string_lossy());
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    format!("{p}/")
                } else {
                    p
                }
            })
            .collect();
        out.sort();
        Ok(out)
    }

    fn write(&self, path: &str, text: &str) -> Result<(), String> {
        if let Some(dir) = parent(path) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{path}: {e}"))?;
        }
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))
    }
}

/// Files kept in memory, for the tests: paths to texts.
#[derive(Debug, Default)]
pub struct Memory {
    files: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
}

impl Memory {
    /// The files `(path, text)`.
    pub fn new(files: &[(&str, &str)]) -> Memory {
        Memory {
            files: std::cell::RefCell::new(
                files
                    .iter()
                    .map(|(p, t)| (p.to_string(), t.to_string()))
                    .collect(),
            ),
        }
    }

    /// A file's text, when there is one.
    pub fn get(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }

    /// Removes a file.
    pub fn remove(&self, path: &str) {
        self.files.borrow_mut().remove(path);
    }
}

impl Files for Memory {
    fn read(&self, path: &str) -> Result<String, String> {
        let text = self
            .get(path)
            .ok_or_else(|| format!("{path}: no such file"))?;
        if text.len() > MAX_BYTES {
            return Err(too_large(path));
        }
        Ok(text)
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, String> {
        let dir = dir.trim_end_matches('/');
        let prefix = format!("{dir}/");
        let mut out = std::collections::BTreeSet::new();
        for p in self.files.borrow().keys() {
            if let Some(rest) = p.strip_prefix(&prefix) {
                match rest.find('/') {
                    Some(i) => out.insert(format!("{prefix}{}/", &rest[..i])),
                    None => out.insert(p.clone()),
                };
            }
        }
        if out.is_empty() {
            return Err(format!("{dir}: no such folder"));
        }
        Ok(out.into_iter().collect())
    }

    fn write(&self, path: &str, text: &str) -> Result<(), String> {
        self.files
            .borrow_mut()
            .insert(path.to_string(), text.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(normalize("C:\\notes\\pages\\"), "C:/notes/pages");
        assert_eq!(normalize("/"), "/");
        assert_eq!(join("/g", "pages/a.md"), "/g/pages/a.md");
        assert_eq!(parent("/g/pages/a.md"), Some("/g/pages"));
        assert_eq!(parent("/a"), Some("/"));
        assert_eq!(file_name("/g/pages/a.md"), "a.md");
        assert_eq!(stem_ext("/g/pages/a.b.MD"), ("a.b", "md".to_string()));
        assert_eq!(stem_ext("/g/.hidden"), (".hidden", String::new()));
        assert_eq!(relative("/g", "/g/pages/a.md"), Some("pages/a.md"));
        assert_eq!(relative("/g", "/gx/a.md"), None);
        assert_eq!(relative("/g", "/g"), Some(""));
    }

    #[test]
    fn a_folder_through_a_link_keeps_the_path_asked() {
        // Kalem's `fs.list` answers with real paths; the index joins the
        // names to the folder it asked for.
        assert_eq!(
            join(
                "/tmp/vault",
                file_name(&normalize("/private/tmp/vault/Home.md"))
            ),
            "/tmp/vault/Home.md"
        );
        assert_eq!(
            join(
                "/tmp/vault",
                file_name(&normalize("/private/tmp/vault/Daily/"))
            ),
            "/tmp/vault/Daily"
        );
    }

    #[test]
    fn memory_lists_folders() {
        let m = Memory::new(&[("/g/a.md", ""), ("/g/p/b.md", ""), ("/g/p/q/c.md", "")]);
        assert_eq!(m.list("/g").unwrap(), ["/g/a.md", "/g/p/"]);
        assert_eq!(m.list("/g/p/").unwrap(), ["/g/p/b.md", "/g/p/q/"]);
        assert!(m.list("/x").is_err());
    }
}
