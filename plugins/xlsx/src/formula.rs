//! Formulas as text: references found, moved and adjusted (ECMA-376 part 1,
//! 18.17).
//!
//! A shared formula (`<f t="shared">`) is written once on its first cell;
//! every other cell of the group holds the formula moved by its offset from
//! that cell, relative references shifted and absolute ones kept ([`shift`]).
//! Inserting and deleting rows and columns adjusts every reference to the
//! sheet, absolute ones too ([`adjust`]), as Excel does.

use crate::cellref::{CellRef, MAX_COL, MAX_ROW, Range, column_index, column_name};

/// One end of a reference: a row, a column, or both, each maybe absolute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    /// Zero-based row and whether it is absolute; `None` in `A:C`.
    pub row: Option<(u32, bool)>,
    /// Zero-based column and whether it is absolute; `None` in `1:3`.
    pub col: Option<(u32, bool)>,
}

/// A reference: one cell, or an area between two ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    /// The first end.
    pub start: End,
    /// The second end; equal to `start` for one cell.
    pub end: End,
    /// Whether it was written as one cell.
    pub single: bool,
}

/// Where a reference points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context<'a> {
    /// The sheet named before `!`, unquoted; `None` for the formula's own.
    pub sheet: Option<&'a str>,
    /// Over several sheets (`Sheet1:Sheet3!A1`) or in another workbook (`[1]Sheet1!A1`).
    pub elsewhere: bool,
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '\\') || !c.is_ascii()
}

fn parse_cell_word(w: &str) -> Option<End> {
    let abs_col = w.starts_with('$');
    let rest = &w[usize::from(abs_col)..];
    let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    let after = &rest[letters..];
    let abs_row = after.starts_with('$');
    let digits = &after[usize::from(abs_row)..];
    if letters == 0 || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let cell = CellRef::parse(&format!("{}{}", &rest[..letters], digits))?;
    Some(End {
        row: Some((cell.row, abs_row)),
        col: Some((cell.col, abs_col)),
    })
}

fn parse_col_word(w: &str) -> Option<End> {
    let abs = w.starts_with('$');
    let col = column_index(&w[usize::from(abs)..])?;
    Some(End {
        row: None,
        col: Some((col, abs)),
    })
}

fn parse_row_word(w: &str) -> Option<End> {
    let abs = w.starts_with('$');
    let d = &w[usize::from(abs)..];
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let row: u32 = d.parse().ok()?;
    (1..=MAX_ROW).contains(&row).then(|| End {
        row: Some((row - 1, abs)),
        col: None,
    })
}

fn write_end(out: &mut String, e: End) {
    if let Some((c, abs)) = e.col {
        if abs {
            out.push('$');
        }
        out.push_str(&column_name(c));
    }
    if let Some((r, abs)) = e.row {
        if abs {
            out.push('$');
        }
        out.push_str(&(r + 1).to_string());
    }
}

/// A reference as text.
pub fn write_reference(r: &Reference) -> String {
    let mut out = String::new();
    write_end(&mut out, r.start);
    if !r.single {
        out.push(':');
        write_end(&mut out, r.end);
    }
    out
}

/// Rewrites every reference of a formula through `f`; `None` from `f` turns
/// the reference into `#REF!`, as Excel writes a reference that fell off the
/// sheet. String literals, structured references and function names are left
/// alone; a sheet prefix is kept and given to `f`.
pub fn map_refs(
    formula: &str,
    mut f: impl FnMut(&Context<'_>, Reference) -> Option<Reference>,
) -> String {
    map_refs_ex(formula, |c, r| f(c, r).map(|r| (None, r)))
}

/// A sheet's name as a formula writes it before `!`: quoted when it is not
/// a plain word, or when it reads as a cell.
pub fn quote_sheet(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && parse_cell_word(name).is_none()
        && parse_row_word(name).is_none();
    if plain {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// As [`map_refs`], `f` also giving the sheet the reference is to be
/// qualified with (`Some`), written before it in place of its own.
pub fn map_refs_ex(
    formula: &str,
    mut f: impl FnMut(&Context<'_>, Reference) -> Option<(Option<String>, Reference)>,
) -> String {
    let chars: Vec<(usize, char)> = formula.char_indices().collect();
    let mut out = String::with_capacity(formula.len() + 8);
    let word_at = |i: usize| -> usize {
        let mut j = i;
        while j < chars.len() && is_word(chars[j].1) {
            j += 1;
        }
        j
    };
    let slice = |a: usize, b: usize| -> &str {
        let s = chars.get(a).map_or(formula.len(), |c| c.0);
        let e = chars.get(b).map_or(formula.len(), |c| c.0);
        &formula[s..e]
    };
    let quoted_end = |i: usize, q: char| -> usize {
        let mut j = i + 1;
        while j < chars.len() {
            if chars[j].1 == q {
                if chars.get(j + 1).is_some_and(|n| n.1 == q) {
                    j += 2;
                    continue;
                }
                break;
            }
            j += 1;
        }
        (j + 1).min(chars.len())
    };
    let mut sheet: Option<String> = None;
    // Where the sheet prefix written before the coming reference starts.
    let mut qual_start: Option<usize> = None;
    let mut elsewhere = false;
    let mut after_bracket = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        let was_after_bracket = std::mem::take(&mut after_bracket);
        match c {
            '"' => {
                let end = quoted_end(i, '"');
                out.push_str(slice(i, end));
                i = end;
                sheet = None;
            }
            '\'' => {
                let end = quoted_end(i, '\'');
                let text = slice(i, end);
                let start = out.len();
                out.push_str(text);
                if chars.get(end).is_some_and(|n| n.1 == '!') {
                    qual_start = Some(start);
                    let inner = &text[1..text.len().saturating_sub(1)];
                    // A quoted `[1]Sheet` or `Sheet1:Sheet3` points elsewhere.
                    elsewhere = was_after_bracket || inner.starts_with('[') || inner.contains(':');
                    sheet = Some(inner.replace("''", "'"));
                    out.push('!');
                    i = end + 1;
                    continue;
                }
                i = end;
            }
            '[' => {
                let mut depth = 0;
                let mut j = i;
                while j < chars.len() {
                    match chars[j].1 {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        '\'' => j += 1,
                        _ => {}
                    }
                    j += 1;
                }
                let end = (j + 1).min(chars.len());
                out.push_str(slice(i, end));
                i = end;
                after_bracket = true;
            }
            c if is_word(c) => {
                let j = word_at(i);
                let w = slice(i, j);
                let next = chars.get(j).map(|c| c.1);
                if next == Some('(') {
                    out.push_str(w);
                    i = j;
                    sheet = None;
                    continue;
                }
                if next == Some('!') {
                    qual_start = Some(out.len());
                    out.push_str(w);
                    out.push('!');
                    elsewhere = was_after_bracket;
                    sheet = Some(w.to_owned());
                    i = j + 1;
                    continue;
                }
                // `Sheet1:Sheet3!A1`: a 3D reference.
                if next == Some(':') && j + 1 < chars.len() && is_word(chars[j + 1].1) {
                    let k = word_at(j + 1);
                    if chars.get(k).is_some_and(|c| c.1 == '!') {
                        qual_start = Some(out.len());
                        out.push_str(slice(i, k + 1));
                        sheet = Some(slice(i, k).to_owned());
                        elsewhere = true;
                        i = k + 1;
                        continue;
                    }
                }
                let ctx_sheet = sheet.take();
                let qs = qual_start.take();
                let emit = |out: &mut String, q: Option<String>, r: &Reference| {
                    if let Some(q) = q {
                        if let Some(qs) = qs {
                            out.truncate(qs);
                        }
                        out.push_str(&quote_sheet(&q));
                        out.push('!');
                    }
                    out.push_str(&write_reference(r));
                };
                let ctx = Context {
                    sheet: ctx_sheet.as_deref(),
                    elsewhere: std::mem::take(&mut elsewhere),
                };
                // `A1:B2`, `A:C`, `1:3` take both sides.
                if next == Some(':') && j + 1 < chars.len() && is_word(chars[j + 1].1) {
                    let k = word_at(j + 1);
                    let w2 = slice(j + 1, k);
                    let pair = match (parse_cell_word(w), parse_cell_word(w2)) {
                        (Some(a), Some(b)) => Some((a, b)),
                        _ => match (parse_col_word(w), parse_col_word(w2)) {
                            (Some(a), Some(b)) => Some((a, b)),
                            _ => match (parse_row_word(w), parse_row_word(w2)) {
                                (Some(a), Some(b)) => Some((a, b)),
                                _ => None,
                            },
                        },
                    };
                    if let Some((a, b)) = pair {
                        match f(
                            &ctx,
                            Reference {
                                start: a,
                                end: b,
                                single: false,
                            },
                        ) {
                            Some((q, r)) => emit(&mut out, q, &r),
                            None => out.push_str("#REF!"),
                        }
                        i = k;
                        continue;
                    }
                }
                match parse_cell_word(w) {
                    Some(e) => match f(
                        &ctx,
                        Reference {
                            start: e,
                            end: e,
                            single: true,
                        },
                    ) {
                        Some((q, r)) => emit(&mut out, q, &r),
                        None => out.push_str("#REF!"),
                    },
                    None => out.push_str(w),
                }
                i = j;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn add(v: u32, d: i64, max: u32) -> Option<u32> {
    let n = i64::from(v) + d;
    (0..i64::from(max)).contains(&n).then_some(n as u32)
}

fn shift_end(e: End, rows: i64, cols: i64) -> Option<End> {
    Some(End {
        row: match e.row {
            Some((r, true)) => Some((r, true)),
            Some((r, false)) => Some((add(r, rows, MAX_ROW)?, false)),
            None => None,
        },
        col: match e.col {
            Some((c, true)) => Some((c, true)),
            Some((c, false)) => Some((add(c, cols, MAX_COL)?, false)),
            None => None,
        },
    })
}

/// The formula as it reads when copied `rows` down and `cols` right:
/// relative references move, absolute ones stay.
pub fn shift(formula: &str, rows: i64, cols: i64) -> String {
    map_refs(formula, |_, r| {
        Some(Reference {
            start: shift_end(r.start, rows, cols)?,
            end: shift_end(r.end, rows, cols)?,
            single: r.single,
        })
    })
}

/// A structural change of a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `n` rows inserted before row `at` (zero-based).
    InsertRows {
        /// The first new row.
        at: u32,
        /// How many.
        n: u32,
    },
    /// Rows `at..at + n` deleted.
    DeleteRows {
        /// The first deleted row.
        at: u32,
        /// How many.
        n: u32,
    },
    /// `n` columns inserted before column `at`.
    InsertCols {
        /// The first new column.
        at: u32,
        /// How many.
        n: u32,
    },
    /// Columns `at..at + n` deleted.
    DeleteCols {
        /// The first deleted column.
        at: u32,
        /// How many.
        n: u32,
    },
}

impl Op {
    fn rows(self) -> bool {
        matches!(self, Op::InsertRows { .. } | Op::DeleteRows { .. })
    }

    /// One coordinate moved: `None` when it was deleted.
    pub fn coord(self, v: u32) -> Option<u32> {
        match self {
            Op::InsertRows { at, n } | Op::InsertCols { at, n } => {
                Some(if v >= at { v + n } else { v })
            }
            Op::DeleteRows { at, n } | Op::DeleteCols { at, n } => {
                if v < at {
                    Some(v)
                } else if v >= at + n {
                    Some(v - n)
                } else {
                    None
                }
            }
        }
    }

    /// A span `a..=b` of the moved coordinate: an insertion strictly inside
    /// grows it, a deletion shrinks it, a span wholly deleted is `None`.
    pub fn span(self, a: u32, b: u32) -> Option<(u32, u32)> {
        match self {
            Op::InsertRows { at, n } | Op::InsertCols { at, n } => {
                let max = if self.rows() { MAX_ROW } else { MAX_COL };
                let a2 = if a >= at { a + n } else { a };
                let b2 = if b >= at { b + n } else { b };
                (b2 < max)
                    .then_some((a2, b2))
                    .or(if a2 < max { Some((a2, max - 1)) } else { None })
            }
            Op::DeleteRows { at, n } | Op::DeleteCols { at, n } => {
                let last = at + n - 1;
                if a >= at && b <= last {
                    return None;
                }
                let a2 = if a < at {
                    a
                } else if a > last {
                    a - n
                } else {
                    at
                };
                let b2 = if b < at {
                    b
                } else if b > last {
                    b - n
                } else {
                    at - 1
                };
                Some((a2, b2))
            }
        }
    }

    /// A cell's new place, `None` when it was deleted.
    pub fn cell(self, c: CellRef) -> Option<CellRef> {
        Some(if self.rows() {
            CellRef::new(self.coord(c.row)?, c.col)
        } else {
            CellRef::new(c.row, self.coord(c.col)?)
        })
    }

    /// A range's new extent, `None` when it was deleted.
    pub fn range(self, r: Range) -> Option<Range> {
        Some(if self.rows() {
            let (a, b) = self.span(r.start.row, r.end.row)?;
            Range {
                start: CellRef::new(a, r.start.col),
                end: CellRef::new(b, r.end.col),
            }
        } else {
            let (a, b) = self.span(r.start.col, r.end.col)?;
            Range {
                start: CellRef::new(r.start.row, a),
                end: CellRef::new(r.end.row, b),
            }
        })
    }

    /// A reference adjusted, absolute parts too.
    pub fn reference(self, r: Reference) -> Option<Reference> {
        let get = |e: End| if self.rows() { e.row } else { e.col };
        let (Some((a, abs_a)), Some((b, abs_b))) = (get(r.start), get(r.end)) else {
            // `A:C` under a row change, `1:3` under a column change.
            return Some(r);
        };
        let (a2, b2) = if r.single {
            let v = self.coord(a)?;
            (v, v)
        } else {
            let (lo, hi) = (a.min(b), a.max(b));
            self.span(lo, hi)?
        };
        let set = |e: End, v: u32, abs: bool| -> End {
            if self.rows() {
                End {
                    row: Some((v, abs)),
                    ..e
                }
            } else {
                End {
                    col: Some((v, abs)),
                    ..e
                }
            }
        };
        Some(Reference {
            start: set(r.start, a2, abs_a),
            end: set(r.end, b2, abs_b),
            single: r.single,
        })
    }
}

/// A formula of `formula_sheet` after `op` on `op_sheet`: references to that
/// sheet move; references to other sheets, to other workbooks and over
/// several sheets stay. `formula_sheet` is `None` where every reference is
/// qualified (defined names, charts).
pub fn adjust(formula: &str, op: Op, op_sheet: &str, formula_sheet: Option<&str>) -> String {
    map_refs(formula, |ctx, r| {
        if ctx.elsewhere {
            return Some(r);
        }
        let target = ctx.sheet.or(formula_sheet);
        match target {
            Some(s) if s.eq_ignore_ascii_case(op_sheet) => op.reference(r),
            _ => Some(r),
        }
    })
}

/// A formula of `formula_sheet` after the cells of `range` on `op_sheet`
/// moved by `rows` and `cols`, as Excel's Cut and Paste: references wholly
/// inside the range follow the cells, absolute ones too; others stay.
pub fn move_refs(
    formula: &str,
    op_sheet: &str,
    formula_sheet: Option<&str>,
    range: Range,
    rows: i64,
    cols: i64,
) -> String {
    let inside = |e: &End| match (e.row, e.col) {
        (Some((r, _)), Some((c, _))) => range.contains(CellRef::new(r, c)),
        _ => false,
    };
    let moved = |e: End| -> Option<End> {
        let (r, ar) = e.row?;
        let (c, ac) = e.col?;
        Some(End {
            row: Some((add(r, rows, MAX_ROW)?, ar)),
            col: Some((add(c, cols, MAX_COL)?, ac)),
        })
    };
    map_refs(formula, |ctx, r| {
        if ctx.elsewhere {
            return Some(r);
        }
        match ctx.sheet.or(formula_sheet) {
            Some(s) if s.eq_ignore_ascii_case(op_sheet) && inside(&r.start) && inside(&r.end) => {
                Some(Reference {
                    start: moved(r.start)?,
                    end: moved(r.end)?,
                    single: r.single,
                })
            }
            _ => Some(r),
        }
    })
}

/// A formula of `formula_sheet` after the cells of `range` on `from` moved
/// to `to`, `rows` and `cols` away: references wholly inside the range now
/// name `to`'s cells, qualified with its name.
pub fn move_refs_between(
    formula: &str,
    from: &str,
    to: &str,
    formula_sheet: Option<&str>,
    range: Range,
    rows: i64,
    cols: i64,
) -> String {
    let inside = |e: &End| match (e.row, e.col) {
        (Some((r, _)), Some((c, _))) => range.contains(CellRef::new(r, c)),
        _ => false,
    };
    let moved = |e: End| -> Option<End> {
        let (r, ar) = e.row?;
        let (c, ac) = e.col?;
        Some(End {
            row: Some((add(r, rows, MAX_ROW)?, ar)),
            col: Some((add(c, cols, MAX_COL)?, ac)),
        })
    };
    map_refs_ex(formula, |ctx, r| {
        if ctx.elsewhere {
            return Some((None, r));
        }
        match ctx.sheet.or(formula_sheet) {
            Some(s) if s.eq_ignore_ascii_case(from) && inside(&r.start) && inside(&r.end) => {
                Some((
                    Some(to.to_owned()),
                    Reference {
                        start: moved(r.start)?,
                        end: moved(r.end)?,
                        single: r.single,
                    },
                ))
            }
            _ => Some((None, r)),
        }
    })
}

/// A formula of `sheet` with its unqualified references qualified with
/// `sheet`: what it means when it moves to another sheet.
pub fn qualify_refs(formula: &str, sheet: &str) -> String {
    map_refs_ex(formula, |ctx, r| {
        Some((
            if ctx.sheet.is_none() && !ctx.elsewhere {
                Some(sheet.to_owned())
            } else {
                None
            },
            r,
        ))
    })
}

/// A space-separated list of ranges (`sqref`) after `op`; ranges deleted
/// whole drop out.
pub fn adjust_sqref(sqref: &str, op: Op) -> String {
    sqref
        .split_whitespace()
        .filter_map(|p| Range::parse(p).and_then(|r| op.range(r)))
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_references_move() {
        assert_eq!(shift("A1*2", 1, 0), "A2*2");
        assert_eq!(
            shift("SUM(A1:B2)+$C$3+C$3+$C3", 2, 1),
            "SUM(B3:C4)+$C$3+D$3+$C5"
        );
        assert_eq!(shift("SUM(A:A)+SUM(1:1)", 1, 1), "SUM(B:B)+SUM(2:2)");
    }

    #[test]
    fn names_strings_and_sheets_stay() {
        assert_eq!(shift("LOG10(A1)&\"A1\"", 1, 0), "LOG10(A2)&\"A1\"");
        assert_eq!(shift("'My A1'!B2+Sheet2!C3", 1, 1), "'My A1'!C3+Sheet2!D4");
        assert_eq!(
            shift("Table1[[#This Row],[A1]]*B1", 1, 0),
            "Table1[[#This Row],[A1]]*B2"
        );
        assert_eq!(shift("_xlfn.CONCAT(A1)", 1, 0), "_xlfn.CONCAT(A2)");
    }

    #[test]
    fn off_the_sheet_is_ref_error() {
        assert_eq!(shift("A1+B2", -1, 0), "#REF!+B1");
    }

    #[test]
    fn moved_to_another_sheet() {
        let r = Range::parse("B3:C3").unwrap();
        assert_eq!(
            move_refs_between(
                "B3+C3+D3+'Sheet 1'!B3",
                "Sheet 1",
                "Data",
                Some("Sheet 1"),
                r,
                7,
                6
            ),
            "Data!H10+Data!I10+D3+Data!H10"
        );
        assert_eq!(
            move_refs_between("Sheet1!B3", "Sheet1", "My Data", None, r, 0, 0),
            "'My Data'!B3"
        );
        assert_eq!(
            qualify_refs("A1*2+Other!B2+SUM(A1:A3)", "Sheet 1"),
            "'Sheet 1'!A1*2+Other!B2+SUM('Sheet 1'!A1:A3)"
        );
        assert_eq!(quote_sheet("AB1"), "'AB1'");
        assert_eq!(quote_sheet("Data_2"), "Data_2");
    }

    #[test]
    fn moved_cells_are_followed() {
        let r = Range::parse("B2:C3").unwrap();
        // Wholly inside: follows; partly or outside: stays.
        assert_eq!(
            move_refs("B2+$C$3+SUM(B2:C3)+SUM(A1:C3)+D4", "S", Some("S"), r, 5, 1),
            "C7+$D$8+SUM(C7:D8)+SUM(A1:C3)+D4"
        );
        assert_eq!(move_refs("S!B2+T!B2", "S", Some("T"), r, 1, 0), "S!B3+T!B2");
    }

    #[test]
    fn inserting_rows() {
        let op = Op::InsertRows { at: 2, n: 2 };
        assert_eq!(adjust("A1+A3+$A$5", op, "S", Some("S")), "A1+A5+$A$7");
        // Inside a range it grows; at its first row it moves.
        assert_eq!(
            adjust("SUM(A2:A4)+SUM(A3:A4)+SUM(A1:A2)", op, "S", Some("S")),
            "SUM(A2:A6)+SUM(A5:A6)+SUM(A1:A2)"
        );
        assert_eq!(
            adjust("SUM(B:B)+SUM(3:4)", op, "S", Some("S")),
            "SUM(B:B)+SUM(5:6)"
        );
        // Only the changed sheet's references move.
        assert_eq!(
            adjust("A3+T!A3+'S'!A3", op, "S", Some("T")),
            "A3+T!A3+'S'!A5"
        );
        assert_eq!(adjust("Data!A3", op, "data", None), "Data!A5");
        assert_eq!(
            adjust("[1]S!A3+S:T!A3", op, "S", Some("S")),
            "[1]S!A3+S:T!A3"
        );
    }

    #[test]
    fn deleting_rows_and_columns() {
        let op = Op::DeleteRows { at: 1, n: 2 };
        assert_eq!(adjust("A1+A2+A4", op, "S", Some("S")), "A1+#REF!+A2");
        assert_eq!(
            adjust("SUM(A1:A5)+SUM(A2:A3)+SUM(A3:A6)", op, "S", Some("S")),
            "SUM(A1:A3)+SUM(#REF!)+SUM(A2:A4)"
        );
        assert_eq!(adjust("S!B2", op, "S", None), "S!#REF!");
        let op = Op::DeleteCols { at: 0, n: 1 };
        assert_eq!(
            adjust("A1+B1+SUM(A1:C1)+SUM(A:B)", op, "S", Some("S")),
            "#REF!+A1+SUM(A1:B1)+SUM(A:A)"
        );
        assert_eq!(
            adjust_sqref("A1:A3 B5 C1", Op::DeleteCols { at: 1, n: 1 }),
            "A1:A3 B1"
        );
    }
}
