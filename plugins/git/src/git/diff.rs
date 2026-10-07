//! Unified diffs as `git diff` and `git show` print them, read into files,
//! hunks and lines. A line keeps its bytes as they are (a carriage return,
//! text that is not UTF-8), so a patch made from it applies exactly
//! (DESIGN.md, 3.6); it is shown lossily.

/// A diff: what preceded the first file (a commit's header and stat in
/// `git show`), then the files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    /// The lines before the first `diff ` line.
    pub preamble: Vec<Vec<u8>>,
    /// The files, in order.
    pub files: Vec<FileDiff>,
}

/// One file's diff.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    /// Its header lines, from `diff --git` to `+++`, as printed.
    pub header: Vec<Vec<u8>>,
    /// The path before; none for a new file.
    pub old_path: Option<String>,
    /// The path after; none for a deleted file.
    pub new_path: Option<String>,
    /// Made by this change.
    pub new_file: bool,
    /// Removed by this change.
    pub deleted: bool,
    /// Moved by this change.
    pub renamed: bool,
    /// Its mode changed (`old mode`, `new mode`).
    pub mode_changed: bool,
    /// Binary: no hunks.
    pub binary: bool,
    /// A merge's combined diff (`diff --cc`): shown, never applied.
    pub combined: bool,
    /// Its hunks.
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// The path the status names it by: the new one, or the old one for
    /// a deleted file.
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or_default()
    }

    /// Lines added and removed.
    pub fn counts(&self) -> (u32, u32) {
        self.hunks.iter().fold((0, 0), |(a, r), h| {
            let (ha, hr) = h.counts();
            (a + ha, r + hr)
        })
    }
}

/// A hunk: where it stands on each side, and its lines.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hunk {
    /// Its first line on the old side, from 1 (0 for an empty side).
    pub old_start: u32,
    /// Its lines on the old side.
    pub old_count: u32,
    /// Its first line on the new side.
    pub new_start: u32,
    /// Its lines on the new side.
    pub new_count: u32,
    /// What follows the second `@@`: the function or heading it is in,
    /// with its leading space.
    pub heading: Vec<u8>,
    /// Its `@@` line as git printed it; empty for a hunk made by a patch.
    pub printed: Vec<u8>,
    /// Its lines.
    pub lines: Vec<Line>,
}

impl Hunk {
    /// The `@@` line, as git prints it.
    pub fn header_line(&self) -> Vec<u8> {
        let mut v = format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_count, self.new_start, self.new_count
        )
        .into_bytes();
        v.extend_from_slice(&self.heading);
        v
    }

    /// The `@@` line to show: as git printed it.
    pub fn shown_header(&self) -> Vec<u8> {
        if self.printed.is_empty() {
            self.header_line()
        } else {
            self.printed.clone()
        }
    }

    /// Lines added and removed.
    pub fn counts(&self) -> (u32, u32) {
        self.lines.iter().fold((0, 0), |(a, r), l| match l.kind {
            LineKind::Added => (a + 1, r),
            LineKind::Removed => (a, r + 1),
            _ => (a, r),
        })
    }

    /// The line of the new side that line `i` of the hunk stands at, for
    /// opening the file there: a context or added line's own; for a
    /// removed line, the line that follows the removal.
    pub fn new_line_of(&self, i: usize) -> u32 {
        let mut line = self.new_start.max(1);
        for l in self.lines.iter().take(i) {
            if matches!(l.kind, LineKind::Context | LineKind::Added) {
                line += 1;
            }
        }
        line
    }
}

/// What a line of a hunk is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// On both sides (` `).
    Context,
    /// Added (`+`).
    Added,
    /// Removed (`-`).
    Removed,
    /// `\ No newline at end of file`, about the line before it.
    NoNewline,
    /// A line of a combined diff, its columns kept in the text.
    Raw,
}

impl LineKind {
    /// Its first column.
    pub fn prefix(self) -> &'static [u8] {
        match self {
            LineKind::Context => b" ",
            LineKind::Added => b"+",
            LineKind::Removed => b"-",
            LineKind::NoNewline => b"\\",
            LineKind::Raw => b"",
        }
    }
}

/// A line of a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// What it is.
    pub kind: LineKind,
    /// Its text after the first column, without the newline.
    pub text: Vec<u8>,
}

impl Line {
    /// The line as the diff prints it.
    pub fn raw(&self) -> Vec<u8> {
        let mut v = self.kind.prefix().to_vec();
        v.extend_from_slice(&self.text);
        v
    }
}

/// Reads a diff.
pub fn parse(out: &[u8]) -> Result<Diff, String> {
    let mut lines: Vec<&[u8]> = out.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut diff = Diff::default();
    let mut i = 0;
    while i < lines.len() && !is_file_start(lines[i]) {
        diff.preamble.push(lines[i].to_vec());
        i += 1;
    }
    while i < lines.len() {
        let (file, next) = file(&lines, i)?;
        diff.files.push(file);
        i = next;
    }
    Ok(diff)
}

fn is_file_start(line: &[u8]) -> bool {
    line.starts_with(b"diff --git ")
        || line.starts_with(b"diff --cc ")
        || line.starts_with(b"diff --combined ")
}

fn file(lines: &[&[u8]], start: usize) -> Result<(FileDiff, usize), String> {
    let mut f = FileDiff {
        combined: !lines[start].starts_with(b"diff --git "),
        ..FileDiff::default()
    };
    f.header.push(lines[start].to_vec());
    if let Some((a, b)) = git_line_paths(lines[start]) {
        f.old_path = Some(a);
        f.new_path = Some(b);
    }
    let mut i = start + 1;
    while i < lines.len() && !lines[i].starts_with(b"@@") && !is_file_start(lines[i]) {
        let l = lines[i];
        f.header.push(l.to_vec());
        if l.starts_with(b"new file mode") {
            f.new_file = true;
        } else if l.starts_with(b"deleted file mode") {
            f.deleted = true;
        } else if l.starts_with(b"old mode") || l.starts_with(b"new mode") {
            f.mode_changed = true;
        } else if let Some(p) = l.strip_prefix(b"rename from ") {
            f.renamed = true;
            f.old_path = Some(unquote(p));
        } else if let Some(p) = l.strip_prefix(b"rename to ") {
            f.renamed = true;
            f.new_path = Some(unquote(p));
        } else if let Some(p) = l.strip_prefix(b"--- ") {
            f.old_path = side_path(p, b"a/");
        } else if let Some(p) = l.strip_prefix(b"+++ ") {
            f.new_path = side_path(p, b"b/");
        } else if l.starts_with(b"Binary files ") || l.starts_with(b"GIT binary patch") {
            f.binary = true;
        }
        i += 1;
    }
    if f.new_file {
        f.old_path = None;
    }
    if f.deleted {
        f.new_path = None;
    }
    while i < lines.len() && lines[i].starts_with(b"@@") {
        if f.combined {
            let (h, next) = combined_hunk(lines, i);
            f.hunks.push(h);
            i = next;
        } else {
            let (h, next) = hunk(lines, i)?;
            f.hunks.push(h);
            i = next;
        }
    }
    // Whatever follows a file's hunks and is not another file (the end of
    // `git show`'s output) belongs to no file.
    while i < lines.len() && !is_file_start(lines[i]) {
        i += 1;
    }
    Ok((f, i))
}

fn hunk(lines: &[&[u8]], start: usize) -> Result<(Hunk, usize), String> {
    let mut h = parse_header(lines[start])?;
    let (mut old, mut new) = (h.old_count, h.new_count);
    let mut i = start + 1;
    while i < lines.len() {
        let l = lines[i];
        let kind = match l.first() {
            Some(b'\\') => LineKind::NoNewline,
            _ if old == 0 && new == 0 => break,
            Some(b' ') => LineKind::Context,
            Some(b'+') => LineKind::Added,
            Some(b'-') => LineKind::Removed,
            // An empty context line, as some tools leave it.
            None => LineKind::Context,
            Some(_) => break,
        };
        match kind {
            LineKind::Context => {
                old = old.saturating_sub(1);
                new = new.saturating_sub(1);
            }
            LineKind::Added => new = new.saturating_sub(1),
            LineKind::Removed => old = old.saturating_sub(1),
            _ => {}
        }
        h.lines.push(Line {
            kind,
            text: l.get(1..).unwrap_or_default().to_vec(),
        });
        i += 1;
    }
    if old != 0 || new != 0 {
        return Err(format!(
            "a hunk shorter than its header: {}",
            String::from_utf8_lossy(lines[start])
        ));
    }
    Ok((h, i))
}

fn combined_hunk(lines: &[&[u8]], start: usize) -> (Hunk, usize) {
    let mut h = Hunk {
        printed: lines[start].to_vec(),
        ..Hunk::default()
    };
    let mut i = start + 1;
    while i < lines.len() && !lines[i].starts_with(b"@@") && !is_file_start(lines[i]) {
        h.lines.push(Line {
            kind: LineKind::Raw,
            text: lines[i].to_vec(),
        });
        i += 1;
    }
    (h, i)
}

/// `@@ -a,b +c,d @@ heading`; a count left out is 1.
fn parse_header(line: &[u8]) -> Result<Hunk, String> {
    let bad = || format!("not a hunk header: {}", String::from_utf8_lossy(line));
    let rest = line.strip_prefix(b"@@ -").ok_or_else(bad)?;
    let end = rest.windows(3).position(|w| w == b" @@").ok_or_else(bad)?;
    let ranges = std::str::from_utf8(&rest[..end]).map_err(|_| bad())?;
    let (old, new) = ranges.split_once(" +").ok_or_else(bad)?;
    let range = |r: &str| -> Option<(u32, u32)> {
        match r.split_once(',') {
            Some((s, c)) => Some((s.parse().ok()?, c.parse().ok()?)),
            None => Some((r.parse().ok()?, 1)),
        }
    };
    let (old_start, old_count) = range(old).ok_or_else(bad)?;
    let (new_start, new_count) = range(new).ok_or_else(bad)?;
    Ok(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        heading: rest[end + 3..].to_vec(),
        printed: line.to_vec(),
        lines: Vec::new(),
    })
}

/// The two paths of `diff --git a/X b/Y` when they can be told apart:
/// quoted, or the same path twice.
fn git_line_paths(line: &[u8]) -> Option<(String, String)> {
    let rest = line.strip_prefix(b"diff --git ")?;
    if rest.first() == Some(&b'"') {
        let (a, used) = c_unquote(rest)?;
        let b = rest.get(used + 1..)?;
        let b = if b.first() == Some(&b'"') {
            c_unquote(b)?.0
        } else {
            b.to_vec()
        };
        return Some((strip_side(&a, b"a/")?, strip_side(&b, b"b/")?));
    }
    // Unquoted: `a/P b/P` with the same P when the path did not change.
    let n = rest.len();
    if n >= 5 && (n - 1) % 2 == 0 {
        let half = (n - 1) / 2;
        let (a, b) = (&rest[..half], &rest[half + 1..]);
        if rest[half] == b' ' && a.get(2..) == b.get(2..) {
            return Some((strip_side(a, b"a/")?, strip_side(b, b"b/")?));
        }
    }
    None
}

fn strip_side(p: &[u8], side: &[u8]) -> Option<String> {
    Some(String::from_utf8_lossy(p.strip_prefix(side)?).into_owned())
}

/// The path of a `---` or `+++` line; none for `/dev/null`.
fn side_path(p: &[u8], side: &[u8]) -> Option<String> {
    // A path with a space may be followed by a tab git adds.
    let p = match p.iter().position(|&b| b == b'\t') {
        Some(t) if p.first() != Some(&b'"') => &p[..t],
        _ => p,
    };
    let p = if p.first() == Some(&b'"') {
        c_unquote(p)?.0
    } else {
        p.to_vec()
    };
    if p == b"/dev/null" {
        return None;
    }
    Some(String::from_utf8_lossy(p.strip_prefix(side).unwrap_or(&p)).into_owned())
}

fn unquote(p: &[u8]) -> String {
    match p.first() {
        Some(b'"') => c_unquote(p).map_or_else(
            || String::from_utf8_lossy(p).into_owned(),
            |(v, _)| String::from_utf8_lossy(&v).into_owned(),
        ),
        _ => String::from_utf8_lossy(p).into_owned(),
    }
}

/// A C-quoted string at the start of `s` (`"a\tb\303\251"`): its bytes
/// and the length it took, quotes included.
fn c_unquote(s: &[u8]) -> Option<(Vec<u8>, usize)> {
    if s.first() != Some(&b'"') {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < s.len() {
        match s[i] {
            b'"' => return Some((out, i + 1)),
            b'\\' => {
                let c = *s.get(i + 1)?;
                i += 2;
                match c {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'v' => out.push(11),
                    b'0'..=b'7' => {
                        let digits = s.get(i - 1..i + 2)?;
                        let v = std::str::from_utf8(digits).ok()?;
                        out.push(u8::from_str_radix(v, 8).ok()?);
                        i += 2;
                    }
                    c => out.push(c),
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    None
}

/// One file's counts from `git diff --numstat -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumStat {
    /// The path (the new one for a rename).
    pub path: String,
    /// Lines added and removed; none for a binary file.
    pub counts: Option<(u32, u32)>,
}

/// Reads `git diff --numstat -z`.
pub fn parse_numstat(out: &[u8]) -> Result<Vec<NumStat>, String> {
    let mut v = Vec::new();
    let mut records = out.split(|&b| b == 0);
    while let Some(r) = records.next() {
        if r.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(r);
        let mut f = text.splitn(3, '\t');
        let (a, d, path) = (f.next(), f.next(), f.next());
        let (Some(a), Some(d), Some(path)) = (a, d, path) else {
            return Err(format!("a numstat line unreadable: {text}"));
        };
        let path = if path.is_empty() {
            // A rename: the old path, then the new one, each on its own.
            let _old = records.next();
            records
                .next()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .ok_or_else(|| format!("a rename without its paths: {text}"))?
        } else {
            path.to_string()
        };
        let counts = match (a.parse(), d.parse()) {
            (Ok(a), Ok(d)) => Some((a, d)),
            _ => None,
        };
        v.push(NumStat { path, counts });
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_FILES: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,5 @@ fn main() {
 one
-two
+TWO
+two and a half
 three
 four
@@ -10,2 +11,2 @@
 ten
-eleven
\\ No newline at end of file
+eleven
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hello
";

    #[test]
    fn files_hunks_and_lines() {
        let d = parse(TWO_FILES.as_bytes()).unwrap();
        assert!(d.preamble.is_empty());
        assert_eq!(d.files.len(), 2);
        let f = &d.files[0];
        assert_eq!(f.path(), "src/lib.rs");
        assert_eq!(f.header.len(), 4);
        assert_eq!(f.hunks.len(), 2);
        let h = &f.hunks[0];
        assert_eq!(
            (h.old_start, h.old_count, h.new_start, h.new_count),
            (1, 4, 1, 5)
        );
        assert_eq!(h.heading, b" fn main() {");
        assert_eq!(h.lines.len(), 6);
        assert_eq!(h.lines[1].kind, LineKind::Removed);
        assert_eq!(h.lines[1].text, b"two");
        assert_eq!(h.counts(), (2, 1));
        assert_eq!(h.header_line(), b"@@ -1,4 +1,5 @@ fn main() {");
        let h = &f.hunks[1];
        assert_eq!(h.lines[2].kind, LineKind::NoNewline);
        assert_eq!(f.counts(), (3, 2));
        let n = &d.files[1];
        assert!(n.new_file);
        assert_eq!(n.old_path, None);
        assert_eq!(n.path(), "new.txt");
        assert_eq!(n.hunks[0].old_count, 0);
        assert_eq!(n.hunks[0].new_count, 1);
    }

    #[test]
    fn the_line_a_hunk_line_opens() {
        let d = parse(TWO_FILES.as_bytes()).unwrap();
        let h = &d.files[0].hunks[0];
        assert_eq!(h.new_line_of(0), 1);
        assert_eq!(h.new_line_of(1), 2); // the removed "two": where TWO is
        assert_eq!(h.new_line_of(3), 3);
        assert_eq!(h.new_line_of(4), 4);
    }

    #[test]
    fn a_line_that_looks_like_a_header_inside_a_hunk() {
        let text = "\
diff --git a/x b/x
--- a/x
+++ b/x
@@ -1,2 +1,2 @@
--- a/y
+++ b/y
 keep
";
        let d = parse(text.as_bytes()).unwrap();
        let h = &d.files[0].hunks[0];
        assert_eq!(h.lines.len(), 3);
        assert_eq!(h.lines[0].text, b"-- a/y");
        assert_eq!(h.lines[1].text, b"++ b/y");
    }

    #[test]
    fn renames_binaries_modes_and_quoted_paths() {
        let text = "\
diff --git a/old.rs b/new.rs
similarity index 90%
rename from old.rs
rename to new.rs
index 1..2 100644
--- a/old.rs
+++ b/new.rs
@@ -1 +1 @@
-a
+b
diff --git a/pic.png b/pic.png
index 1..2 100644
Binary files a/pic.png and b/pic.png differ
diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
diff --git \"a/t\\tab \\303\\251\" \"b/t\\tab \\303\\251\"
deleted file mode 100644
--- \"a/t\\tab \\303\\251\"
+++ /dev/null
@@ -1 +0,0 @@
-x
";
        let d = parse(text.as_bytes()).unwrap();
        assert_eq!(d.files.len(), 4);
        assert!(d.files[0].renamed);
        assert_eq!(d.files[0].old_path.as_deref(), Some("old.rs"));
        assert_eq!(d.files[0].new_path.as_deref(), Some("new.rs"));
        assert!(d.files[1].binary && d.files[1].hunks.is_empty());
        assert!(d.files[2].mode_changed && d.files[2].hunks.is_empty());
        assert_eq!(d.files[2].path(), "run.sh");
        assert!(d.files[3].deleted);
        assert_eq!(d.files[3].path(), "t\tab é");
    }

    #[test]
    fn a_commit_header_before_the_diff() {
        let text = "commit abc\nAuthor: A <a@b>\n\n    Subject\n\n x | 1 +\n\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -0,0 +1 @@\n+y\n";
        let d = parse(text.as_bytes()).unwrap();
        assert_eq!(d.preamble.len(), 7);
        assert_eq!(d.files.len(), 1);
    }

    #[test]
    fn carriage_returns_and_other_bytes_are_kept() {
        let mut text =
            b"diff --git a/w b/w\n--- a/w\n+++ b/w\n@@ -1 +1 @@\n-a\r\n+\xe9\r\n".to_vec();
        text.extend_from_slice(b"");
        let d = parse(&text).unwrap();
        let h = &d.files[0].hunks[0];
        assert_eq!(h.lines[0].text, b"a\r");
        assert_eq!(h.lines[1].text, b"\xe9\r");
    }

    #[test]
    fn a_hunk_cut_short_is_refused() {
        let text = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n";
        assert!(parse(text.as_bytes()).is_err());
    }

    #[test]
    fn numstat() {
        let out = b"3\t1\tsrc/lib.rs\0-\t-\tpic.png\x000\t0\t\0old.rs\0new.rs\0";
        let v = parse_numstat(out).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].counts, Some((3, 1)));
        assert_eq!(v[1].counts, None);
        assert_eq!(v[2].path, "new.rs");
    }
}
