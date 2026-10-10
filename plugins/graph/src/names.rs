//! Page names: how a title is compared, and how Logseq writes a title as
//! a file name and reads it back (`:file/name-format`).

/// How Logseq writes a page's title as its file's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileNameFormat {
    /// `:triple-lowbar`, Logseq's since 0.8: `/` as `___`, the characters
    /// file systems refuse percent-encoded.
    TripleLowbar,
    /// The format before it: `/` as `.` or `%2F`; read, never written.
    Legacy,
}

/// A page's name as names compare: lower case, trimmed.
pub fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// The characters a file name cannot hold on some system, or that Logseq
/// encodes: written as `%XX`.
fn reserved(c: char) -> bool {
    matches!(
        c,
        '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*' | '#' | '%'
    ) || c.is_control()
}

/// The file name (without its extension) Logseq writes for `title`.
pub fn title_to_file(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().chars() {
        if c == '/' {
            out.push_str("___");
        } else if reserved(c) {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    // A name of dots only is no file name.
    if out.chars().all(|c| c == '.') {
        out = out.replace('.', "%2E");
    }
    out
}

/// Percent-decoded, where the escapes are valid UTF-8.
fn percent_decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// The title a file name (without its extension) stands for.
pub fn file_to_title(stem: &str, format: FileNameFormat) -> String {
    match format {
        FileNameFormat::TripleLowbar => percent_decode(&stem.replace("___", "/")),
        // Every `.` was a namespace's `/`, and so was `%2F`.
        FileNameFormat::Legacy => percent_decode(&stem.replace('.', "/")),
    }
}

/// The namespace a title is in: `a/b` for `a/b/c`.
pub fn namespace(title: &str) -> Option<&str> {
    title
        .rfind('/')
        .map(|i| &title[..i])
        .filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_files() {
        assert_eq!(title_to_file("Project/Kalem"), "Project___Kalem");
        assert_eq!(
            title_to_file("What? A: \"name\""),
            "What%3F A%3A %22name%22"
        );
        assert_eq!(title_to_file("C#"), "C%23");
        assert_eq!(
            file_to_title("Project___Kalem", FileNameFormat::TripleLowbar),
            "Project/Kalem"
        );
        assert_eq!(
            file_to_title("What%3F A%3A %22name%22", FileNameFormat::TripleLowbar),
            "What? A: \"name\""
        );
        assert_eq!(file_to_title("a.b", FileNameFormat::Legacy), "a/b");
        assert_eq!(file_to_title("a%2Fb", FileNameFormat::Legacy), "a/b");
        assert_eq!(file_to_title("100%", FileNameFormat::TripleLowbar), "100%");
        for t in ["Plain", "a/b/c", "çay #1 ?", "50% off"] {
            assert_eq!(
                file_to_title(&title_to_file(t), FileNameFormat::TripleLowbar),
                t
            );
        }
        assert_eq!(key("  Kalem "), "kalem");
        assert_eq!(namespace("a/b/c"), Some("a/b"));
        assert_eq!(namespace("a"), None);
    }
}
