//! Formulas as text: references found and moved (ECMA-376 part 1, 18.17).
//!
//! A shared formula (`<f t="shared">`) is written once on its first cell;
//! every other cell of the group holds the formula moved by its offset from
//! that cell, relative references shifted and absolute ones kept. When the
//! first cell is edited the group needs a new first cell holding the moved
//! text, which is what [`shift`] computes.

use crate::cellref::{CellRef, MAX_COL, MAX_ROW, column_index, column_name};

/// One reference found in a formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ref {
    /// `$A$1`: the flags say which parts are absolute.
    Cell {
        cell: CellRef,
        abs_col: bool,
        abs_row: bool,
    },
    /// `A:C`, columns only.
    Cols { col: u32, abs: bool },
    /// `1:3`, rows only.
    Rows { row: u32, abs: bool },
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '\\') || !c.is_ascii()
}

fn parse_cell_word(w: &str) -> Option<Ref> {
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
    Some(Ref::Cell {
        cell,
        abs_col,
        abs_row,
    })
}

fn parse_col_word(w: &str) -> Option<Ref> {
    let abs = w.starts_with('$');
    let col = column_index(&w[usize::from(abs)..])?;
    Some(Ref::Cols { col, abs })
}

fn parse_row_word(w: &str) -> Option<Ref> {
    let abs = w.starts_with('$');
    let d = &w[usize::from(abs)..];
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let row: u32 = d.parse().ok()?;
    (1..=MAX_ROW)
        .contains(&row)
        .then(|| Ref::Rows { row: row - 1, abs })
}

fn write_ref(out: &mut String, r: Ref) {
    match r {
        Ref::Cell {
            cell,
            abs_col,
            abs_row,
        } => {
            if abs_col {
                out.push('$');
            }
            out.push_str(&column_name(cell.col));
            if abs_row {
                out.push('$');
            }
            out.push_str(&(cell.row + 1).to_string());
        }
        Ref::Cols { col, abs } => {
            if abs {
                out.push('$');
            }
            out.push_str(&column_name(col));
        }
        Ref::Rows { row, abs } => {
            if abs {
                out.push('$');
            }
            out.push_str(&(row + 1).to_string());
        }
    }
}

/// Rewrites every reference of a formula through `f`; `None` from `f` turns
/// the reference into `#REF!`, as Excel writes a reference that fell off the
/// sheet. String literals, quoted sheet names, structured references and
/// function names are left alone.
fn map_refs(formula: &str, mut f: impl FnMut(Ref) -> Option<Ref>) -> String {
    let chars: Vec<(usize, char)> = formula.char_indices().collect();
    let mut out = String::with_capacity(formula.len() + 8);
    let mut i = 0;
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
    while i < chars.len() {
        let c = chars[i].1;
        match c {
            '"' | '\'' => {
                // A literal or a quoted sheet name; a doubled quote escapes.
                let mut j = i + 1;
                while j < chars.len() {
                    if chars[j].1 == c {
                        if chars.get(j + 1).is_some_and(|n| n.1 == c) {
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    j += 1;
                }
                let end = (j + 1).min(chars.len());
                out.push_str(slice(i, end));
                i = end;
            }
            '[' => {
                // Structured references and external book indexes, nested.
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
            }
            c if is_word(c) => {
                let j = word_at(i);
                let w = slice(i, j);
                let next = chars.get(j).map(|c| c.1);
                if next == Some('(') || next == Some('!') {
                    out.push_str(w);
                    i = j;
                    continue;
                }
                // `A:C` and `1:3` need both sides; `A1:B2` is two cells.
                if next == Some(':') && j + 1 < chars.len() && is_word(chars[j + 1].1) {
                    let k = word_at(j + 1);
                    let w2 = slice(j + 1, k);
                    let pair = match (parse_col_word(w), parse_col_word(w2)) {
                        (Some(a), Some(b)) if parse_cell_word(w).is_none() => Some((a, b)),
                        _ => match (parse_row_word(w), parse_row_word(w2)) {
                            (Some(a), Some(b)) => Some((a, b)),
                            _ => None,
                        },
                    };
                    if let Some((a, b)) = pair {
                        match (f(a), f(b)) {
                            (Some(a), Some(b)) => {
                                write_ref(&mut out, a);
                                out.push(':');
                                write_ref(&mut out, b);
                            }
                            _ => out.push_str("#REF!"),
                        }
                        i = k;
                        continue;
                    }
                }
                match parse_cell_word(w) {
                    Some(r) => match f(r) {
                        Some(r) => write_ref(&mut out, r),
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

/// The formula as it reads when copied `rows` down and `cols` right:
/// relative references move, absolute ones stay.
pub fn shift(formula: &str, rows: i64, cols: i64) -> String {
    map_refs(formula, |r| {
        Some(match r {
            Ref::Cell {
                cell,
                abs_col,
                abs_row,
            } => Ref::Cell {
                cell: CellRef::new(
                    if abs_row {
                        cell.row
                    } else {
                        add(cell.row, rows, MAX_ROW)?
                    },
                    if abs_col {
                        cell.col
                    } else {
                        add(cell.col, cols, MAX_COL)?
                    },
                ),
                abs_col,
                abs_row,
            },
            Ref::Cols { col, abs } => Ref::Cols {
                col: if abs { col } else { add(col, cols, MAX_COL)? },
                abs,
            },
            Ref::Rows { row, abs } => Ref::Rows {
                row: if abs { row } else { add(row, rows, MAX_ROW)? },
                abs,
            },
        })
    })
}

#[cfg(test)]
mod tests {
    use super::shift;

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
}
