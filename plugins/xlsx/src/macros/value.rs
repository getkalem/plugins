//! VBA's values and their conversions ([MS-VBAL] 5.5).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

/// A place of the workbook a macro holds: cells of a sheet, zero-based,
/// corners inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeRef {
    /// The sheet's index.
    pub sheet: usize,
    /// First row.
    pub r0: u32,
    /// First column.
    pub c0: u32,
    /// Last row.
    pub r1: u32,
    /// Last column.
    pub c1: u32,
}

impl RangeRef {
    /// Rows in the range.
    pub fn rows(&self) -> u32 {
        self.r1 - self.r0 + 1
    }

    /// Columns in the range.
    pub fn cols(&self) -> u32 {
        self.c1 - self.c0 + 1
    }
}

/// A `Collection` or a `Scripting.Dictionary`: values in order, by key.
#[derive(Debug, Default)]
pub struct Keyed {
    /// Keys (lower case for a collection, as given for a dictionary) and values.
    pub items: Vec<(Option<V>, V)>,
}

/// An object a macro holds.
#[derive(Debug, Clone)]
pub enum Obj {
    /// `Application`.
    Application,
    /// The workbook (`ThisWorkbook`, `ActiveWorkbook`).
    Workbook,
    /// `Workbooks`.
    Workbooks,
    /// `Worksheets` or `Sheets`.
    Sheets,
    /// One sheet.
    Sheet(usize),
    /// Cells.
    Range(RangeRef),
    /// `range.Rows`: the range seen as rows (`Count`, `For Each`, `(n)`).
    RowsOf(RangeRef),
    /// `range.Columns`.
    ColsOf(RangeRef),
    /// `range.Font`.
    Font(RangeRef),
    /// `range.Interior`: the fill.
    Interior(RangeRef),
    /// `range.Borders`, or one side of them (`xlEdgeBottom`).
    Borders(RangeRef, Option<i64>),
    /// `Application.WorksheetFunction`.
    WorksheetFunction,
    /// `Err`.
    ErrObject,
    /// `Debug`.
    Debug,
    /// A `Collection`.
    Collection(Rc<RefCell<Keyed>>),
    /// A `Scripting.Dictionary`.
    Dictionary(Rc<RefCell<Keyed>>),
    /// An array of `Range.Areas`, `Font` and other parts not modelled: the
    /// name, for the message when used.
    Unsupported(String),
}

impl PartialEq for Obj {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Obj::Range(a), Obj::Range(b))
            | (Obj::RowsOf(a), Obj::RowsOf(b))
            | (Obj::ColsOf(a), Obj::ColsOf(b))
            | (Obj::Font(a), Obj::Font(b))
            | (Obj::Interior(a), Obj::Interior(b)) => a == b,
            (Obj::Borders(a, x), Obj::Borders(b, y)) => a == b && x == y,
            (Obj::Sheet(a), Obj::Sheet(b)) => a == b,
            (Obj::Collection(a), Obj::Collection(b)) | (Obj::Dictionary(a), Obj::Dictionary(b)) => {
                Rc::ptr_eq(a, b)
            }
            (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }
}

/// An array: dimensions as (lower bound, length), elements row-major on
/// the first dimension last (VBA's order does not show; indexes do).
#[derive(Debug, Clone, PartialEq)]
pub struct Array {
    /// Lower bound and length of each dimension.
    pub dims: Vec<(i64, usize)>,
    /// The elements.
    pub data: Vec<V>,
}

impl Array {
    /// An array of `Empty` with the given bounds (lower, upper inclusive).
    pub fn new(bounds: &[(i64, i64)]) -> Self {
        let dims: Vec<(i64, usize)> = bounds
            .iter()
            .map(|&(l, u)| (l, (u - l + 1).max(0) as usize))
            .collect();
        let n = dims.iter().map(|d| d.1).product();
        Self {
            dims,
            data: vec![V::Empty; n],
        }
    }

    /// The flat index of `idx`, `None` out of bounds.
    pub fn offset(&self, idx: &[i64]) -> Option<usize> {
        if idx.len() != self.dims.len() {
            return None;
        }
        let mut off = 0usize;
        for (i, (&(l, n), &v)) in self.dims.iter().zip(idx).enumerate() {
            let k = v - l;
            if k < 0 || k as usize >= n {
                return None;
            }
            off = if i == 0 {
                k as usize
            } else {
                off * n + k as usize
            };
        }
        Some(off)
    }

    /// Bounds kept and grown or shrunk for `ReDim Preserve` (the last
    /// dimension may change).
    pub fn resized(&self, bounds: &[(i64, i64)]) -> Self {
        let mut out = Array::new(bounds);
        if self.dims.len() != out.dims.len() {
            return out;
        }
        let mut idx: Vec<i64> = self.dims.iter().map(|d| d.0).collect();
        if self.data.is_empty() {
            return out;
        }
        loop {
            if let (Some(a), Some(b)) = (self.offset(&idx), out.offset(&idx)) {
                out.data[b] = self.data[a].clone();
            }
            // Next index, last dimension fastest.
            let mut d = idx.len();
            loop {
                if d == 0 {
                    return out;
                }
                d -= 1;
                idx[d] += 1;
                if idx[d] < self.dims[d].0 + self.dims[d].1 as i64 {
                    break;
                }
                idx[d] = self.dims[d].0;
            }
        }
    }
}

/// A value.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum V {
    /// An uninitialized Variant.
    #[default]
    Empty,
    /// `Null`.
    Null,
    /// A Boolean.
    Bool(bool),
    /// An Integer or Long.
    Int(i64),
    /// A Double (and Single, Currency).
    Num(f64),
    /// A Date, as a serial.
    Date(f64),
    /// A String.
    Str(String),
    /// An array.
    Arr(Box<Array>),
    /// An object.
    Obj(Obj),
    /// `Nothing`.
    Nothing,
    /// An omitted optional argument.
    Missing,
    /// An error value (`CVErr`, a cell's `#N/A`): Excel's error number.
    Error(i64),
}

impl V {
    /// VBA's `TypeName`.
    pub fn type_name(&self) -> &'static str {
        match self {
            V::Empty | V::Missing => "Empty",
            V::Null => "Null",
            V::Bool(_) => "Boolean",
            V::Int(i) if i32::try_from(*i).is_ok() && (-32_768..=32_767).contains(i) => "Integer",
            V::Int(_) => "Long",
            V::Num(_) => "Double",
            V::Date(_) => "Date",
            V::Str(_) => "String",
            V::Arr(_) => "Variant()",
            V::Obj(o) => match o {
                Obj::Application => "Application",
                Obj::Workbook => "Workbook",
                Obj::Workbooks => "Workbooks",
                Obj::Sheets => "Sheets",
                Obj::Sheet(_) => "Worksheet",
                Obj::Range(_) | Obj::RowsOf(_) | Obj::ColsOf(_) => "Range",
                Obj::Font(_) => "Font",
                Obj::Interior(_) => "Interior",
                Obj::Borders(_, None) => "Borders",
                Obj::Borders(_, Some(_)) => "Border",
                Obj::WorksheetFunction => "WorksheetFunction",
                Obj::ErrObject => "ErrObject",
                Obj::Debug => "Debug",
                Obj::Collection(_) => "Collection",
                Obj::Dictionary(_) => "Dictionary",
                Obj::Unsupported(_) => "Object",
            },
            V::Nothing => "Nothing",
            V::Error(_) => "Error",
        }
    }

    /// VBA's `VarType`.
    pub fn var_type(&self) -> i64 {
        match self {
            V::Empty | V::Missing => 0,
            V::Null => 1,
            V::Int(_) if self.type_name() == "Integer" => 2,
            V::Int(_) => 3,
            V::Num(_) => 5,
            V::Date(_) => 7,
            V::Str(_) => 8,
            V::Obj(_) | V::Nothing => 9,
            V::Error(_) => 10,
            V::Bool(_) => 11,
            V::Arr(_) => 8204,
        }
    }

    /// Whether the value is a number, or text that reads as one.
    pub fn is_numeric(&self) -> bool {
        match self {
            V::Int(_) | V::Num(_) | V::Bool(_) | V::Empty | V::Date(_) => true,
            V::Str(s) => parse_number(s).is_some(),
            _ => false,
        }
    }
}

/// Text read as a number as VBA's `Val`-like conversions do: decimal with
/// an optional exponent, thousands separators, `&H` hexadecimal.
pub fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(h) = t.strip_prefix("&H").or_else(|| t.strip_prefix("&h")) {
        return i64::from_str_radix(h, 16).ok().map(|v| v as f64);
    }
    let cleaned: String = t.chars().filter(|&c| c != ',').collect();
    let cleaned = cleaned
        .strip_suffix('%')
        .map_or(cleaned.clone(), |c| c.to_owned());
    let v: f64 = cleaned.parse().ok()?;
    let v = if t.ends_with('%') { v / 100.0 } else { v };
    v.is_finite().then_some(v)
}

/// A runtime error: VBA's number and description, and whether `On Error`
/// can catch it (a refused call or the budget cannot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtError {
    /// VBA's error number (13 type mismatch, 9 subscript out of range, …).
    pub number: i64,
    /// The message.
    pub description: String,
    /// The source line, filled in where the error surfaces.
    pub line: usize,
    /// Not catchable.
    pub fatal: bool,
}

impl RtError {
    /// A catchable error.
    pub fn new(number: i64, description: impl Into<String>) -> Self {
        Self {
            number,
            description: description.into(),
            line: 0,
            fatal: false,
        }
    }

    /// An error that stops the macro whatever `On Error` says.
    pub fn fatal(description: impl Into<String>) -> Self {
        Self {
            number: 0,
            description: description.into(),
            line: 0,
            fatal: true,
        }
    }

    /// Type mismatch (13).
    pub fn mismatch() -> Self {
        Self::new(13, "Type mismatch")
    }
}

impl fmt::Display for RtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.number != 0 {
            write!(f, "run-time error {}: {}", self.number, self.description)?;
        } else {
            f.write_str(&self.description)?;
        }
        if self.line > 0 {
            write!(f, " (line {})", self.line)?;
        }
        Ok(())
    }
}

/// The result of evaluation.
pub type R<T> = Result<T, RtError>;

/// A number from a value: `True` is −1, `Empty` 0, numeric text its number.
pub fn to_num(v: &V) -> R<f64> {
    match v {
        V::Empty | V::Missing => Ok(0.0),
        V::Bool(b) => Ok(if *b { -1.0 } else { 0.0 }),
        V::Int(i) => Ok(*i as f64),
        V::Num(n) | V::Date(n) => Ok(*n),
        V::Str(s) => parse_number(s)
            .or_else(|| crate::macros::builtins::parse_date(s))
            .ok_or_else(RtError::mismatch),
        V::Null => Err(RtError::new(94, "Invalid use of Null")),
        _ => Err(RtError::mismatch()),
    }
}

/// VBA's banker's rounding to a whole number (`CInt`, `CLng`, array indexes).
pub fn round_even(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    }
}

/// A whole number from a value, rounded half to even.
pub fn to_int(v: &V) -> R<i64> {
    let n = round_even(to_num(v)?);
    if !n.is_finite() || n.abs() > 9.2e18 {
        return Err(RtError::new(6, "Overflow"));
    }
    Ok(n as i64)
}

/// A Boolean from a value.
pub fn to_bool(v: &V) -> R<bool> {
    match v {
        V::Bool(b) => Ok(*b),
        V::Str(s) if s.eq_ignore_ascii_case("true") => Ok(true),
        V::Str(s) if s.eq_ignore_ascii_case("false") => Ok(false),
        _ => Ok(to_num(v)? != 0.0),
    }
}

/// Text from a value, as `CStr` writes it.
pub fn to_str(v: &V) -> R<String> {
    Ok(match v {
        V::Empty | V::Missing => String::new(),
        V::Null => return Err(RtError::new(94, "Invalid use of Null")),
        V::Bool(b) => if *b { "True" } else { "False" }.into(),
        V::Int(i) => i.to_string(),
        V::Num(n) => num_text(*n),
        V::Date(d) => crate::macros::builtins::date_text(*d),
        V::Str(s) => s.clone(),
        V::Error(e) => format!("Error {e}"),
        V::Arr(_) | V::Obj(_) | V::Nothing => return Err(RtError::mismatch()),
    })
}

/// A Double as VBA prints it: up to fifteen significant digits, scientific
/// past them.
pub fn num_text(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{n:.0}");
    }
    let a = n.abs();
    if !(1e-5..1e15).contains(&a) {
        let s = format!("{n:.14E}");
        let (m, e) = s.split_once('E').unwrap_or((&s, "0"));
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{m}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let digits = 15 - (a.log10().floor() as i32 + 1);
    let s = format!("{n:.*}", digits.max(0) as usize);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A dictionary's key compared as VBA compares keys: numbers by value,
/// text exactly (a collection's keys ignore case: they are lowered first).
pub fn same_key(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Str(x), V::Str(y)) => x == y,
        (V::Str(_), _) | (_, V::Str(_)) => false,
        _ => matches!((to_num(a), to_num(b)), (Ok(x), Ok(y)) if x == y),
    }
}

/// Variables of a procedure or module, shared by reference for `ByRef`.
pub type Slot = Rc<RefCell<V>>;

/// A fresh variable.
pub fn slot(v: V) -> Slot {
    Rc::new(RefCell::new(v))
}

/// A scope's variables by lower-case name.
pub type Scope = HashMap<String, Slot>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(to_num(&V::Str("1,234.5".into())).unwrap(), 1234.5);
        assert_eq!(to_num(&V::Bool(true)).unwrap(), -1.0);
        assert!(to_num(&V::Str("abc".into())).is_err());
        assert_eq!(to_int(&V::Num(2.5)).unwrap(), 2);
        assert_eq!(to_int(&V::Num(3.5)).unwrap(), 4);
        assert_eq!(to_int(&V::Num(-2.5)).unwrap(), -2);
        assert_eq!(num_text(0.1 + 0.2), "0.3");
        assert_eq!(num_text(1.0 / 3.0), "0.333333333333333");
        assert_eq!(num_text(1e20), "1E+20");
        assert_eq!(to_str(&V::Num(2.0)).unwrap(), "2");
    }

    #[test]
    fn arrays() {
        let mut a = Array::new(&[(1, 2), (0, 2)]);
        let o = a.offset(&[2, 1]).unwrap();
        a.data[o] = V::Int(7);
        assert_eq!(a.offset(&[3, 0]), None);
        let b = a.resized(&[(1, 2), (0, 4)]);
        assert_eq!(b.data[b.offset(&[2, 1]).unwrap()], V::Int(7));
        assert_eq!(b.dims[1], (0, 5));
    }
}
