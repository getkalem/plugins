//! A1 cell references (ECMA-376 part 1, 18.17.2.3 and the `ST_CellRef` type).

use std::fmt;

/// The last row and column a worksheet may hold (Excel 2007 and later).
pub const MAX_ROW: u32 = 1_048_576;
/// The last column, `XFD`.
pub const MAX_COL: u32 = 16_384;

/// A cell's position, zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellRef {
    /// Zero-based row: row 1 is 0.
    pub row: u32,
    /// Zero-based column: column A is 0.
    pub col: u32,
}

impl CellRef {
    /// A position from zero-based row and column.
    pub const fn new(row: u32, col: u32) -> Self {
        Self { row, col }
    }

    /// Parses `B12` (and `$B$12`); `None` past `XFD1048576` or when malformed.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let b = s.as_bytes();
        let mut i = usize::from(b.first() == Some(&b'$'));
        let col_start = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        let col = column_index(&s[col_start..i])?;
        if b.get(i) == Some(&b'$') {
            i += 1;
        }
        let digits = &s[i..];
        if digits.is_empty()
            || !digits.bytes().all(|c| c.is_ascii_digit())
            || digits.starts_with('0')
        {
            return None;
        }
        let row: u32 = digits.parse().ok()?;
        (row <= MAX_ROW).then(|| Self::new(row - 1, col))
    }
}

impl fmt::Display for CellRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", column_name(self.col), self.row + 1)
    }
}

/// `A` is 0, `Z` 25, `AA` 26; `None` for anything else or past `XFD`.
pub fn column_index(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n: u32 = 0;
    for c in letters.bytes() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + u32::from(c.to_ascii_uppercase() - b'A') + 1;
    }
    (n <= MAX_COL).then(|| n - 1)
}

/// The letters of a zero-based column.
pub fn column_name(mut col: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII")
}

/// A rectangle of cells, both corners inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    /// The top left cell.
    pub start: CellRef,
    /// The bottom right cell.
    pub end: CellRef,
}

impl Range {
    /// Parses `A1:C3` or a single `B2`.
    pub fn parse(s: &str) -> Option<Self> {
        let (a, b) = s.split_once(':').unwrap_or((s, s));
        let (a, b) = (CellRef::parse(a)?, CellRef::parse(b)?);
        Some(Self {
            start: CellRef::new(a.row.min(b.row), a.col.min(b.col)),
            end: CellRef::new(a.row.max(b.row), a.col.max(b.col)),
        })
    }

    /// Whether the range holds a cell.
    pub fn contains(&self, c: CellRef) -> bool {
        (self.start.row..=self.end.row).contains(&c.row)
            && (self.start.col..=self.end.col).contains(&c.col)
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            write!(f, "{}", self.start)
        } else {
            write!(f, "{}:{}", self.start, self.end)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns() {
        for (i, n) in [
            (0, "A"),
            (25, "Z"),
            (26, "AA"),
            (701, "ZZ"),
            (702, "AAA"),
            (16_383, "XFD"),
        ] {
            assert_eq!(column_name(i), n);
            assert_eq!(column_index(n), Some(i));
        }
        assert_eq!(column_index("XFE"), None);
    }

    #[test]
    fn refs() {
        assert_eq!(CellRef::parse("B12"), Some(CellRef::new(11, 1)));
        assert_eq!(CellRef::parse("$B$12"), Some(CellRef::new(11, 1)));
        assert_eq!(CellRef::parse("B0"), None);
        assert_eq!(CellRef::parse("B"), None);
        assert_eq!(
            CellRef::parse("XFD1048576").unwrap().to_string(),
            "XFD1048576"
        );
        assert_eq!(CellRef::parse("A1048577"), None);
        let r = Range::parse("C3:A1").unwrap();
        assert_eq!(r.to_string(), "A1:C3");
        assert!(r.contains(CellRef::new(1, 1)));
    }
}
