//! Excel's object model as far as macros reach it in Kalem: the
//! application, the workbook, its sheets and their cells; collections and
//! dictionaries; `Err` and `Debug`. Writes are the plugin's cell edits.
//! Formatting is the plugin's Format Cells: `Font`, `Interior`,
//! `Borders`, `NumberFormat`, alignment, wrapping, widths and heights,
//! merging; what is not modelled (`Characters`, `FormatConditions`,
//! theme colors) is skipped and listed in the run's report.

use super::constants;
use super::interp::Interp;
use super::value::*;
use crate::cellref::{CellRef, MAX_COL, MAX_ROW, column_index, column_name};
use crate::sheet::Value;
use crate::workbook::{Input, SheetKind};

fn arg(a: &[V], i: usize) -> &V {
    a.get(i).unwrap_or(&V::Missing)
}

fn has(a: &[V], i: usize) -> bool {
    !matches!(a.get(i), None | Some(V::Missing))
}

fn unsupported(what: &str) -> RtError {
    RtError::fatal(format!("`{what}` is not available to macros in Kalem yet"))
}

fn wb_err(e: crate::workbook::Error) -> RtError {
    RtError::new(1004, e.to_string())
}

fn bad_argument() -> RtError {
    RtError::new(5, "Invalid procedure call or argument")
}

/// The style of a range's first cell, as `Font`, `Interior` and
/// `NumberFormat` read it.
fn style_of(it: &mut Interp<'_>, r: &RangeRef) -> R<crate::styles::CellStyle> {
    let at = CellRef::new(r.r0, r.c0);
    let own = it
        .wb
        .sheet(r.sheet)
        .map_err(wb_err)?
        .cells
        .get(&at)
        .map(|c| c.style);
    let s = own.unwrap_or_else(|| it.wb.row_or_col_style(r.sheet, at));
    Ok(it.wb.style(s))
}

/// A range's cells given a change of format, as Format Cells gives it.
fn restyle(it: &mut Interp<'_>, r: &RangeRef, change: kalem_viewer::StyleChange) -> R<()> {
    let range = crate::cellref::Range {
        start: CellRef::new(r.r0, r.c0),
        end: CellRef::new(r.r1, r.c1),
    };
    it.wb
        .change_style(r.sheet, range, &change)
        .map_err(wb_err)?;
    it.report.changed = true;
    Ok(())
}

/// A VBA color, a Long with red in its low byte, as RGB.
fn color_arg(v: &V) -> R<[u8; 3]> {
    let n = to_int(v)?;
    if !(0..=0xFF_FFFF).contains(&n) {
        return Err(bad_argument());
    }
    Ok([
        (n & 0xFF) as u8,
        (n >> 8 & 0xFF) as u8,
        (n >> 16 & 0xFF) as u8,
    ])
}

/// An RGB color as VBA's Long.
fn vba_color(rgb: u32) -> V {
    let (r, g, b) = (rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF);
    V::Int(i64::from(r | g << 8 | b << 16))
}

/// A `ColorIndex` of the default palette (1 to 56) as RGB.
fn palette_color(n: i64) -> Option<[u8; 3]> {
    let c = usize::try_from(n)
        .ok()
        .filter(|n| (1..=56).contains(n))
        .and_then(|n| crate::styles::indexed_color(n + 7))?;
    Some([(c >> 16) as u8, (c >> 8) as u8, c as u8])
}

/// The `ColorIndex` of an RGB color in the default palette.
fn palette_index(rgb: u32) -> Option<i64> {
    (1..=56usize)
        .find(|n| crate::styles::indexed_color(n + 7) == Some(rgb))
        .map(|n| n as i64)
}

/// A range's rows (or columns), the used ones of a whole column (or
/// row) and no more.
fn lines_of(it: &mut Interp<'_>, r: &RangeRef, rows: bool) -> R<std::ops::RangeInclusive<u32>> {
    let whole = if rows {
        r.r0 == 0 && r.r1 >= MAX_ROW - 1
    } else {
        r.c0 == 0 && r.c1 >= MAX_COL - 1
    };
    let (from, to) = if rows { (r.r0, r.r1) } else { (r.c0, r.c1) };
    if !whole {
        return Ok(from..=to);
    }
    let used = it.wb.sheet(r.sheet).map_err(wb_err)?.used_range();
    let last = used.map_or(0, |u| if rows { u.end.row } else { u.end.col });
    Ok(from..=last.max(from))
}

/// `Range.Borders(side)` as the sides a change draws.
fn border_set(side: Option<i64>) -> R<kalem_viewer::BorderSet> {
    use kalem_viewer::BorderSet as B;
    Ok(match side {
        None => B::All,
        Some(7) => B::Left,
        Some(8) => B::Top,
        Some(9) => B::Bottom,
        Some(10) => B::Right,
        Some(_) => return Err(unsupported("inside and diagonal borders")),
    })
}

/// `LineStyle` (with `Weight`) as a border's line; `None` for no line.
fn line_style(style: i64, weight: i64) -> Option<kalem_viewer::LineStyle> {
    use kalem_viewer::LineStyle as L;
    Some(match (style, weight) {
        (-4142, _) => return None,
        (-4115 | 4 | 5 | 13, _) => L::Dashed,
        (-4118, _) => L::Dotted,
        (-4119, _) => L::Double,
        (_, 1) => L::Hair,
        (_, -4138) => L::Medium,
        (_, 4) => L::Thick,
        _ => L::Thin,
    })
}

/// Borders drawn on a range's sides (`set`) in a line and a color.
fn draw_borders(
    it: &mut Interp<'_>,
    r: &RangeRef,
    set: kalem_viewer::BorderSet,
    line: Option<kalem_viewer::LineStyle>,
    color: Option<[u8; 3]>,
) -> R<()> {
    let set = if line.is_none() {
        kalem_viewer::BorderSet::None
    } else {
        set
    };
    restyle(
        it,
        r,
        kalem_viewer::StyleChange {
            borders: Some((set, color)),
            border_style: line,
            ..Default::default()
        },
    )
}

/// `Range.Font.m`.
fn font_get(it: &mut Interp<'_>, r: &RangeRef, m: &str) -> R<V> {
    let st = style_of(it, r)?;
    Ok(match m {
        "bold" => V::Bool(st.bold),
        "italic" => V::Bool(st.italic),
        "strikethrough" => V::Bool(st.strike),
        "underline" => V::Int(if st.underline { 2 } else { -4142 }),
        "color" => vba_color(st.color.unwrap_or(0)),
        "colorindex" => V::Int(
            st.color
                .map_or(-4105, |c| palette_index(c).unwrap_or(-4105)),
        ),
        "size" => V::Num(st.size.unwrap_or(11.0)),
        "name" => V::Str(st.font.unwrap_or_else(|| "Calibri".into())),
        _ => return Err(RtError::new(438, format!("Font.{m}"))),
    })
}

/// `Range.Font.m = v`.
fn font_set(it: &mut Interp<'_>, r: &RangeRef, m: &str, v: &V) -> R<()> {
    use kalem_viewer::StyleChange as S;
    let change = match m {
        "bold" => S {
            bold: Some(to_bool(v)?),
            ..S::default()
        },
        "italic" => S {
            italic: Some(to_bool(v)?),
            ..S::default()
        },
        "strikethrough" => S {
            strike: Some(to_bool(v)?),
            ..S::default()
        },
        "underline" => S {
            underline: Some(match v {
                V::Bool(b) => *b,
                v => !matches!(to_int(v)?, 0 | -4142),
            }),
            ..S::default()
        },
        "color" => S {
            color: Some(Some(color_arg(v)?)),
            ..S::default()
        },
        "colorindex" => S {
            color: Some(match to_int(v)? {
                -4105 | 0 => None,
                n => Some(palette_color(n).ok_or_else(bad_argument)?),
            }),
            ..S::default()
        },
        "size" => S {
            size: Some(to_num(v)? as f32),
            ..S::default()
        },
        "name" => S {
            face: Some(to_str(v)?),
            ..S::default()
        },
        _ => {
            it.report.skipped.push(format!(
                "line {}: Font.{m} = … (not applied by macros in Kalem)",
                it.line
            ));
            return Ok(());
        }
    };
    restyle(it, r, change)
}

/// `Range.Interior.m`.
fn interior_get(it: &mut Interp<'_>, r: &RangeRef, m: &str) -> R<V> {
    let st = style_of(it, r)?;
    Ok(match m {
        "color" => vba_color(st.fill.unwrap_or(0xFF_FFFF)),
        "colorindex" => V::Int(st.fill.map_or(-4142, |c| palette_index(c).unwrap_or(-4142))),
        "pattern" => V::Int(if st.fill.is_some() { 1 } else { -4142 }),
        _ => return Err(RtError::new(438, format!("Interior.{m}"))),
    })
}

/// `Range.Interior.m = v`.
fn interior_set(it: &mut Interp<'_>, r: &RangeRef, m: &str, v: &V) -> R<()> {
    let fill = match m {
        "color" => Some(color_arg(v)?),
        "colorindex" => match to_int(v)? {
            -4142 | -4105 | 0 => None,
            n => Some(palette_color(n).ok_or_else(bad_argument)?),
        },
        "pattern" if to_int(v)? == -4142 => None,
        "pattern" => return Ok(()),
        _ => {
            it.report.skipped.push(format!(
                "line {}: Interior.{m} = … (not applied by macros in Kalem)",
                it.line
            ));
            return Ok(());
        }
    };
    restyle(
        it,
        r,
        kalem_viewer::StyleChange {
            fill: Some(fill),
            ..Default::default()
        },
    )
}

/// `Range.Borders(side).m = v`: each statement draws the sides again in
/// what the borders have now and what it changes.
fn borders_set(it: &mut Interp<'_>, r: &RangeRef, side: Option<i64>, m: &str, v: &V) -> R<()> {
    let set = border_set(side)?;
    match m {
        "linestyle" => {
            let line = line_style(to_int(v)?, 2);
            draw_borders(it, r, set, line, None)
        }
        "weight" => {
            let line = line_style(1, to_int(v)?);
            draw_borders(it, r, set, line, None)
        }
        "color" => {
            let c = color_arg(v)?;
            draw_borders(it, r, set, Some(kalem_viewer::LineStyle::Thin), Some(c))
        }
        "colorindex" => {
            let c = palette_color(to_int(v)?);
            draw_borders(it, r, set, Some(kalem_viewer::LineStyle::Thin), c)
        }
        _ => {
            it.report.skipped.push(format!(
                "line {}: Borders.{m} = … (not applied by macros in Kalem)",
                it.line
            ));
            Ok(())
        }
    }
}

/// A Range format property set: `NumberFormat`, the alignments,
/// `WrapText`, `ColumnWidth`, `RowHeight`, `IndentLevel`, `Locked`,
/// `Orientation`, `ShrinkToFit`, `MergeCells`.
fn range_format_set(it: &mut Interp<'_>, r: &RangeRef, m: &str, v: &V) -> R<()> {
    use kalem_viewer::{Align, StyleChange as S, VAlign};
    let change = match m {
        "numberformat" | "numberformatlocal" => S {
            number_format: Some(to_str(v)?),
            ..S::default()
        },
        "horizontalalignment" => match to_int(v)? {
            7 => S {
                center_across: Some(true),
                ..S::default()
            },
            n => S {
                align: Some(match n {
                    -4131 => Align::Left,
                    -4108 => Align::Center,
                    -4152 => Align::Right,
                    1 => Align::General,
                    _ => return Err(unsupported("justified and filled alignment")),
                }),
                ..S::default()
            },
        },
        "verticalalignment" => S {
            valign: Some(match to_int(v)? {
                -4160 => VAlign::Top,
                -4108 | -4130 | -4117 => VAlign::Middle,
                -4107 => VAlign::Bottom,
                _ => return Err(bad_argument()),
            }),
            ..S::default()
        },
        "indentlevel" => S {
            indent: Some(u8::try_from(to_int(v)?.clamp(0, 15)).unwrap_or(0)),
            ..S::default()
        },
        "locked" => S {
            locked: Some(to_bool(v)?),
            ..S::default()
        },
        "shrinktofit" => S {
            shrink: Some(to_bool(v)?),
            ..S::default()
        },
        "orientation" => S {
            rotation: Some(match to_int(v)? {
                -4128 => 0,
                -4166 => 255,
                -4171 => 90,
                -4170 => 180,
                n @ 0..=90 => n as u16,
                n @ -90..=-1 => (90 - n) as u16,
                _ => return Err(bad_argument()),
            }),
            ..S::default()
        },
        "wraptext" => {
            let wrap = to_bool(v)?;
            let used = clip_to_used(it, r)?;
            if count(&used) > MAX_CELLS {
                return Err(RtError::new(
                    7,
                    "Out of memory: WrapText over too many cells",
                ));
            }
            for at in cells_of(&used).collect::<Vec<_>>() {
                it.wb.set_wrap(r.sheet, at, wrap).map_err(wb_err)?;
            }
            it.report.changed = true;
            return Ok(());
        }
        "columnwidth" | "rowheight" => {
            let rows = m == "rowheight";
            let size = to_num(v)?;
            if !(0.0..=if rows { 409.0 } else { 255.0 }).contains(&size) {
                return Err(RtError::new(
                    1004,
                    format!("Unable to set the {m} property"),
                ));
            }
            for i in lines_of(it, r, rows)? {
                if rows {
                    it.wb.set_row_height(r.sheet, i, size).map_err(wb_err)?;
                } else {
                    it.wb.set_col_width(r.sheet, i, size).map_err(wb_err)?;
                }
            }
            it.report.changed = true;
            return Ok(());
        }
        "mergecells" => return merge(it, r, to_bool(v)?, false),
        _ => return Err(RtError::new(438, format!("Range.{m} cannot be set"))),
    };
    restyle(it, r, change)
}

/// `Range.Merge` (each row apart with `Across`) and `UnMerge`: unmerging
/// takes away every merged area the range touches.
fn merge(it: &mut Interp<'_>, r: &RangeRef, on: bool, across: bool) -> R<()> {
    let range = |r0: u32, r1: u32| crate::cellref::Range {
        start: CellRef::new(r0, r.c0),
        end: CellRef::new(r1, r.c1),
    };
    if on {
        if across {
            for row in r.r0..=r.r1 {
                it.wb
                    .merge_cells(r.sheet, range(row, row), false)
                    .map_err(wb_err)?;
            }
        } else {
            it.wb
                .merge_cells(r.sheet, range(r.r0, r.r1), false)
                .map_err(wb_err)?;
        }
    } else {
        let touched: Vec<CellRef> = it
            .wb
            .sheet(r.sheet)
            .map_err(wb_err)?
            .merged
            .iter()
            .filter(|m| {
                m.start.row <= r.r1 && m.end.row >= r.r0 && m.start.col <= r.c1 && m.end.col >= r.c0
            })
            .map(|m| m.start)
            .collect();
        for at in touched {
            it.wb.unmerge_cells(r.sheet, at).map_err(wb_err)?;
        }
    }
    it.report.changed = true;
    Ok(())
}

/// The names of a member's parameters, for named arguments.
pub fn param_names(o: &Obj, m: &str) -> &'static [&'static str] {
    match (o, m) {
        (Obj::Range(_), "address") => &[
            "rowabsolute",
            "columnabsolute",
            "referencestyle",
            "external",
        ],
        (Obj::Range(_), "find") => &[
            "what",
            "after",
            "lookin",
            "lookat",
            "searchorder",
            "searchdirection",
            "matchcase",
        ],
        (Obj::Range(_), "copy") => &["destination"],
        (Obj::Range(_), "offset") => &["rowoffset", "columnoffset"],
        (Obj::Range(_), "resize") => &["rowsize", "columnsize"],
        (Obj::Range(_), "insert" | "delete") => &["shift"],
        (Obj::Range(_), "borderaround") => &["linestyle", "weight", "colorindex", "color"],
        (Obj::Range(_), "merge") => &["across"],
        (Obj::Application, "inputbox") => &[
            "prompt",
            "title",
            "default",
            "left",
            "top",
            "helpfile",
            "helpcontextid",
            "type",
        ],
        (Obj::ErrObject, "raise") => &["number", "source", "description"],
        (Obj::Collection(_), "add") => &["item", "key", "before", "after"],
        _ => &[],
    }
}

/// Whether `o.m = v` sets a property (`Range.Value`), as opposed to setting
/// the default member of what `o.m` returns (`ws.Cells(1, 1) = v`).
pub fn settable(o: &Obj, m: &str) -> bool {
    match o {
        Obj::Range(_) | Obj::RowsOf(_) | Obj::ColsOf(_) => matches!(
            m,
            "value"
                | "value2"
                | "formula"
                | "formular1c1"
                | "formulalocal"
                | "numberformat"
                | "numberformatlocal"
                | "horizontalalignment"
                | "verticalalignment"
                | "wraptext"
                | "columnwidth"
                | "rowheight"
                | "indentlevel"
                | "locked"
                | "orientation"
                | "shrinktofit"
                | "mergecells"
        ),
        Obj::Dictionary(_) => m == "item",
        Obj::Unsupported(_) => true,
        _ => !matches!(
            m,
            "range"
                | "cells"
                | "rows"
                | "columns"
                | "item"
                | "worksheets"
                | "sheets"
                | "offset"
                | "resize"
        ),
    }
}

fn sheet_ok(it: &Interp<'_>, idx: usize) -> R<()> {
    match it.wb.sheets().get(idx) {
        Some(s) if s.kind == SheetKind::Worksheet => Ok(()),
        Some(s) => Err(RtError::new(1004, format!("{} is not a worksheet", s.name))),
        None => Err(RtError::new(9, "Subscript out of range")),
    }
}

/// A cell's value as VBA sees it: dates as dates, errors as error values.
fn cell_v(it: &mut Interp<'_>, sheet: usize, at: CellRef) -> R<V> {
    let v = it.wb.value(sheet, at).map_err(wb_err)?;
    let style = it
        .wb
        .sheet(sheet)
        .map_err(wb_err)?
        .cells
        .get(&at)
        .map(|c| c.style);
    Ok(match v {
        Value::Empty => V::Empty,
        Value::Number(n) => {
            if style.is_some_and(|s| crate::numfmt::is_date_format(&it.wb.style(s).num_fmt)) {
                V::Date(n)
            } else {
                V::Num(n)
            }
        }
        Value::Text(t) => V::Str(t),
        Value::Bool(b) => V::Bool(b),
        Value::Error(e) => V::Error(constants::error_number(&e)),
    })
}

/// A value written into a cell as Excel's `Range.Value = v` writes it:
/// text is read as typed, dates get a date format.
fn write_v(it: &mut Interp<'_>, sheet: usize, at: CellRef, v: &V) -> R<()> {
    let date1904 = it.wb.date1904();
    let input = match v {
        V::Empty | V::Missing | V::Null => Input::Clear,
        V::Bool(b) => Input::Bool(*b),
        V::Int(i) => Input::Number(*i as f64, None),
        V::Num(n) => Input::Number(*n, None),
        V::Date(d) => Input::Number(
            *d,
            Some(if d.fract() == 0.0 {
                14
            } else if *d < 1.0 {
                21
            } else {
                22
            }),
        ),
        V::Str(s) => Input::parse(s, date1904),
        V::Error(e) => Input::Error(constants::error_text(*e).into()),
        V::Arr(_) | V::Obj(_) | V::Nothing => return Err(RtError::mismatch()),
    };
    it.wb.set_input(sheet, at, input).map_err(wb_err)?;
    it.report.changed = true;
    Ok(())
}

fn cells_of(r: &RangeRef) -> impl Iterator<Item = CellRef> + '_ {
    (r.r0..=r.r1).flat_map(move |row| (r.c0..=r.c1).map(move |col| CellRef::new(row, col)))
}

const MAX_CELLS: u64 = 2_000_000;

fn count(r: &RangeRef) -> u64 {
    u64::from(r.rows()) * u64::from(r.cols())
}

/// Parses `A1`, `A1:B2`, `$A$1`, `A:C`, `1:3`, `Sheet1!A1`, `'My Sheet'!A1`
/// or a defined name, on `default_sheet` when unqualified.
pub fn parse_address(it: &mut Interp<'_>, default_sheet: usize, text: &str) -> Option<RangeRef> {
    let text = text.trim();
    if text.contains(',') {
        return None;
    }
    let (sheet, rest) = match text.rsplit_once('!') {
        Some((s, r)) => {
            let name = s.trim_matches('\'').replace("''", "'");
            (it.wb.sheet_index(&name)?, r)
        }
        None => (default_sheet, text),
    };
    let plain = rest.replace('$', "");
    let (a, b) = plain.split_once(':').unwrap_or((&plain, &plain));
    if let (Some(x), Some(y)) = (CellRef::parse(a), CellRef::parse(b)) {
        return Some(RangeRef {
            sheet,
            r0: x.row.min(y.row),
            c0: x.col.min(y.col),
            r1: x.row.max(y.row),
            c1: x.col.max(y.col),
        });
    }
    if plain.contains(':') {
        if let (Some(x), Some(y)) = (column_index(a), column_index(b)) {
            return Some(RangeRef {
                sheet,
                r0: 0,
                c0: x.min(y),
                r1: MAX_ROW - 1,
                c1: x.max(y),
            });
        }
        if let (Ok(x), Ok(y)) = (a.parse::<u32>(), b.parse::<u32>())
            && x >= 1
            && y >= 1
        {
            return Some(RangeRef {
                sheet,
                r0: x.min(y) - 1,
                c0: 0,
                r1: x.max(y) - 1,
                c1: MAX_COL - 1,
            });
        }
    }
    // A defined name: its sheet-local one first.
    let names = it.wb.defined_names().to_vec();
    let found = names
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case(text) && n.local_sheet == Some(default_sheet))
        .or_else(|| {
            names
                .iter()
                .find(|n| n.name.eq_ignore_ascii_case(text) && n.local_sheet.is_none())
        })?;
    let refers = found.refers_to.trim_start_matches('=').to_owned();
    if refers.eq_ignore_ascii_case(text) {
        return None;
    }
    parse_address(it, default_sheet, &refers)
}

fn address_text(r: &RangeRef, row_abs: bool, col_abs: bool) -> String {
    let one = |row: u32, col: u32| {
        format!(
            "{}{}{}{}",
            if col_abs { "$" } else { "" },
            column_name(col),
            if row_abs { "$" } else { "" },
            row + 1
        )
    };
    if r.c0 == 0 && r.c1 == MAX_COL - 1 {
        let d = if row_abs { "$" } else { "" };
        return format!("{d}{}:{d}{}", r.r0 + 1, r.r1 + 1);
    }
    if r.r0 == 0 && r.r1 == MAX_ROW - 1 {
        let d = if col_abs { "$" } else { "" };
        return format!("{d}{}:{d}{}", column_name(r.c0), column_name(r.c1));
    }
    if r.r0 == r.r1 && r.c0 == r.c1 {
        one(r.r0, r.c0)
    } else {
        format!("{}:{}", one(r.r0, r.c0), one(r.r1, r.c1))
    }
}

/// A reference as formula text, its sheet named.
fn formula_ref(it: &Interp<'_>, r: &RangeRef) -> String {
    let name = &it.wb.sheets()[r.sheet].name;
    let quoted = if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        name.clone()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    };
    format!("{quoted}!{}", address_text(r, true, true))
}

/// `Range.End(direction)`: Ctrl+arrow from the range's first cell.
fn end(it: &mut Interp<'_>, r: &RangeRef, dir: i64) -> R<RangeRef> {
    let (dr, dc): (i64, i64) = match dir {
        -4162 => (-1, 0),
        -4121 => (1, 0),
        -4159 => (0, -1),
        -4161 => (0, 1),
        _ => return Err(RtError::new(5, "Invalid procedure call or argument")),
    };
    let sheet = r.sheet;
    // The cells that hold something on the line walked, sorted.
    let line: Vec<u32> = {
        let model = it.wb.sheet(sheet).map_err(wb_err)?;
        let mut v: Vec<u32> = model
            .cells
            .iter()
            .filter(|(p, c)| {
                (if dr != 0 {
                    p.col == r.c0
                } else {
                    p.row == r.r0
                }) && (c.value != Value::Empty || c.formula.is_some())
            })
            .map(|(p, _)| if dr != 0 { p.row } else { p.col })
            .collect();
        v.sort_unstable();
        v
    };
    let start = if dr != 0 { r.r0 } else { r.c0 };
    let max = if dr != 0 { MAX_ROW - 1 } else { MAX_COL - 1 };
    let filled = |x: u32| line.binary_search(&x).is_ok();
    let step = dr + dc;
    let next = |x: u32| -> Option<u32> {
        let n = i64::from(x) + step;
        (0..=i64::from(max)).contains(&n).then_some(n as u32)
    };
    let mut pos = start;
    match next(pos) {
        None => {}
        Some(n) if filled(pos) && filled(n) => {
            // Run to the last filled cell of the block.
            pos = n;
            while let Some(m) = next(pos) {
                if !filled(m) {
                    break;
                }
                pos = m;
            }
        }
        Some(_) => {
            // Jump to the next filled cell, or the edge.
            pos = if step > 0 {
                line.iter().copied().find(|&x| x > start).unwrap_or(max)
            } else {
                line.iter().rev().copied().find(|&x| x < start).unwrap_or(0)
            };
        }
    }
    Ok(if dr != 0 {
        RangeRef {
            sheet,
            r0: pos,
            r1: pos,
            c0: r.c0,
            c1: r.c0,
        }
    } else {
        RangeRef {
            sheet,
            r0: r.r0,
            r1: r.r0,
            c0: pos,
            c1: pos,
        }
    })
}

/// `Range.CurrentRegion`: the block of filled cells around the range,
/// bounded by empty rows and columns.
fn current_region(it: &mut Interp<'_>, r: &RangeRef) -> R<RangeRef> {
    let filled: std::collections::HashSet<(u32, u32)> = it
        .wb
        .sheet(r.sheet)
        .map_err(wb_err)?
        .cells
        .iter()
        .filter(|(_, c)| c.value != Value::Empty || c.formula.is_some())
        .map(|(p, _)| (p.row, p.col))
        .collect();
    let mut b = *r;
    loop {
        let mut grown = b;
        let row_has = |row: i64, c0: u32, c1: u32| {
            row >= 0 && (c0.saturating_sub(1)..=c1 + 1).any(|c| filled.contains(&(row as u32, c)))
        };
        let col_has = |col: i64, r0: u32, r1: u32| {
            col >= 0 && (r0.saturating_sub(1)..=r1 + 1).any(|rr| filled.contains(&(rr, col as u32)))
        };
        if row_has(i64::from(b.r0) - 1, b.c0, b.c1) {
            grown.r0 = b.r0 - 1;
        }
        if row_has(i64::from(b.r1) + 1, b.c0, b.c1) {
            grown.r1 = b.r1 + 1;
        }
        if col_has(i64::from(b.c0) - 1, b.r0, b.r1) {
            grown.c0 = b.c0 - 1;
        }
        if col_has(i64::from(b.c1) + 1, b.r0, b.r1) {
            grown.c1 = b.c1 + 1;
        }
        if grown == b {
            return Ok(b);
        }
        b = grown;
    }
}

fn used_range(it: &mut Interp<'_>, sheet: usize) -> R<RangeRef> {
    let u = it.wb.sheet(sheet).map_err(wb_err)?.used_range();
    Ok(match u {
        Some(u) => RangeRef {
            sheet,
            r0: u.start.row,
            c0: u.start.col,
            r1: u.end.row,
            c1: u.end.col,
        },
        None => RangeRef {
            sheet,
            r0: 0,
            c0: 0,
            r1: 0,
            c1: 0,
        },
    })
}

fn rel(r: &RangeRef, row: i64, col: i64) -> R<RangeRef> {
    let rr = i64::from(r.r0) + row;
    let cc = i64::from(r.c0) + col;
    if rr < 0 || cc < 0 || rr >= i64::from(MAX_ROW) || cc >= i64::from(MAX_COL) {
        return Err(RtError::new(
            1004,
            "Application-defined or object-defined error",
        ));
    }
    Ok(RangeRef {
        sheet: r.sheet,
        r0: rr as u32,
        c0: cc as u32,
        r1: rr as u32,
        c1: cc as u32,
    })
}

fn range_value(it: &mut Interp<'_>, r: &RangeRef) -> R<V> {
    if r.r0 == r.r1 && r.c0 == r.c1 {
        return cell_v(it, r.sheet, CellRef::new(r.r0, r.c0));
    }
    if count(r) > MAX_CELLS {
        return Err(RtError::new(
            7,
            "Out of memory: the range is too large to read at once",
        ));
    }
    let mut a = Array::new(&[(1, i64::from(r.rows())), (1, i64::from(r.cols()))]);
    for (k, at) in cells_of(r).enumerate() {
        a.data[k] = cell_v(it, r.sheet, at)?;
    }
    Ok(V::Arr(Box::new(a)))
}

/// `Value = v` and `Formula = v` alike: text starting with `=` enters a formula.
fn set_range_value(it: &mut Interp<'_>, r: &RangeRef, v: V) -> R<()> {
    match &v {
        V::Arr(a) => {
            let (rows, cols) = match a.dims.len() {
                1 => (1usize, a.dims[0].1),
                2 => (a.dims[0].1, a.dims[1].1),
                _ => return Err(RtError::mismatch()),
            };
            for (k, at) in cells_of(r).enumerate() {
                let (i, j) = (k / r.cols() as usize, k % r.cols() as usize);
                let x = if i < rows && j < cols {
                    a.data[i * cols + j].clone()
                } else {
                    // Excel fills what the array does not reach with #N/A.
                    V::Error(2042)
                };
                write_v(it, r.sheet, at, &x)?;
            }
        }
        V::Empty if count(r) > 1 => clear(it, r)?,
        _ => {
            if count(r) > MAX_CELLS {
                return Err(RtError::new(
                    7,
                    "Out of memory: the range is too large to fill",
                ));
            }
            for at in cells_of(r).collect::<Vec<_>>() {
                write_v(it, r.sheet, at, &v)?;
            }
        }
    }
    Ok(())
}

/// Clears the values of the cells that exist in the range.
fn clear(it: &mut Interp<'_>, r: &RangeRef) -> R<()> {
    let cells: Vec<CellRef> = it
        .wb
        .sheet(r.sheet)
        .map_err(wb_err)?
        .cells
        .keys()
        .filter(|p| (r.r0..=r.r1).contains(&p.row) && (r.c0..=r.c1).contains(&p.col))
        .copied()
        .collect();
    for at in cells {
        it.wb.set_input(r.sheet, at, Input::Clear).map_err(wb_err)?;
        it.report.changed = true;
    }
    Ok(())
}

fn whole_rows(r: &RangeRef) -> bool {
    r.c0 == 0 && r.c1 == MAX_COL - 1
}

fn whole_cols(r: &RangeRef) -> bool {
    r.r0 == 0 && r.r1 == MAX_ROW - 1
}

fn insert_delete(it: &mut Interp<'_>, r: &RangeRef, insert: bool) -> R<()> {
    let res = if whole_rows(r) {
        if insert {
            it.wb.insert_rows(r.sheet, r.r0, r.rows())
        } else {
            it.wb.delete_rows(r.sheet, r.r0, r.rows())
        }
    } else if whole_cols(r) {
        if insert {
            it.wb.insert_cols(r.sheet, r.c0, r.cols())
        } else {
            it.wb.delete_cols(r.sheet, r.c0, r.cols())
        }
    } else {
        return Err(unsupported(
            "Insert and Delete of cells that are not whole rows or columns (use EntireRow or EntireColumn)",
        ));
    };
    res.map_err(wb_err)?;
    it.report.changed = true;
    Ok(())
}

fn copy_to(it: &mut Interp<'_>, from: &RangeRef, dest: &RangeRef) -> R<()> {
    // Formulas move as a copy does; values copy.
    let mut writes = Vec::new();
    for at in cells_of(from) {
        let text = it.wb.edit_text(from.sheet, at).map_err(wb_err)?;
        let (dr, dc) = (
            i64::from(dest.r0) - i64::from(from.r0),
            i64::from(dest.c0) - i64::from(from.c0),
        );
        let to = CellRef::new(
            (i64::from(at.row) + dr) as u32,
            (i64::from(at.col) + dc) as u32,
        );
        let text = match text.strip_prefix('=') {
            Some(f) => format!("={}", crate::formula::shift(f, dr, dc)),
            None => text,
        };
        writes.push((to, text));
    }
    for (to, text) in writes {
        it.wb.set_cell(dest.sheet, to, &text).map_err(wb_err)?;
    }
    it.report.changed = true;
    Ok(())
}

fn find(it: &mut Interp<'_>, r: &RangeRef, a: &[V]) -> R<V> {
    let what = to_str(arg(a, 0))?;
    let whole = has(a, 3) && to_int(arg(a, 3))? == 1;
    let case = has(a, 6) && to_bool(arg(a, 6))?;
    let cells: Vec<CellRef> = it
        .wb
        .sheet(r.sheet)
        .map_err(wb_err)?
        .cells
        .keys()
        .filter(|p| (r.r0..=r.r1).contains(&p.row) && (r.c0..=r.c1).contains(&p.col))
        .copied()
        .collect();
    let norm = |s: &str| if case { s.to_owned() } else { s.to_lowercase() };
    let w = norm(&what);
    for at in cells {
        let t = norm(&it.wb.display(r.sheet, at).map_err(wb_err)?);
        if (whole && t == w) || (!whole && t.contains(&w)) {
            return Ok(V::Obj(Obj::Range(RangeRef {
                sheet: r.sheet,
                r0: at.row,
                c0: at.col,
                r1: at.row,
                c1: at.col,
            })));
        }
    }
    Ok(V::Nothing)
}

/// `Range("A1")`, `Range("A1", "B2")`, `Range(cell1, cell2)` on a sheet.
fn range_of(it: &mut Interp<'_>, sheet: usize, a: &[V]) -> R<RangeRef> {
    let one = |it: &mut Interp<'_>, v: &V| -> R<RangeRef> {
        match v {
            V::Obj(Obj::Range(r)) => Ok(*r),
            v => {
                let t = to_str(v)?;
                parse_address(it, sheet, &t)
                    .ok_or_else(|| RtError::new(1004, format!("Method 'Range' failed: {t}")))
            }
        }
    };
    let a0 = one(it, arg(a, 0))?;
    if !has(a, 1) {
        return Ok(a0);
    }
    let b = one(it, arg(a, 1))?;
    Ok(RangeRef {
        sheet: a0.sheet,
        r0: a0.r0.min(b.r0),
        c0: a0.c0.min(b.c0),
        r1: a0.r1.max(b.r1),
        c1: a0.c1.max(b.c1),
    })
}

/// `Cells(row, col)` relative to a range's corner, one-based; a column may be a letter.
fn cells_at(r: &RangeRef, a: &[V]) -> R<RangeRef> {
    if !has(a, 0) {
        return Ok(*r);
    }
    if !has(a, 1) {
        // `Cells(n)`: the n-th cell, row by row.
        let n = to_int(arg(a, 0))? - 1;
        let w = i64::from(r.cols());
        return rel(r, n.div_euclid(w), n.rem_euclid(w));
    }
    let row = to_int(arg(a, 0))?;
    let col = match arg(a, 1) {
        V::Str(s) if !arg(a, 1).is_numeric() => {
            i64::from(column_index(s).ok_or_else(|| RtError::new(1004, format!("no column {s}")))?)
                + 1
        }
        v => to_int(v)?,
    };
    rel(r, row - 1, col - 1)
}

pub(crate) fn sheet_range(sheet: usize) -> RangeRef {
    RangeRef {
        sheet,
        r0: 0,
        c0: 0,
        r1: MAX_ROW - 1,
        c1: MAX_COL - 1,
    }
}

fn rows_of(r: &RangeRef, a: &[V]) -> R<V> {
    if !has(a, 0) {
        return Ok(V::Obj(Obj::RowsOf(*r)));
    }
    let n = match arg(a, 0) {
        V::Str(s) => {
            // `Rows("2:4")`.
            let (x, y) = s.split_once(':').unwrap_or((s, s));
            let (x, y): (u32, u32) = (
                x.trim().parse().map_err(|_| RtError::mismatch())?,
                y.trim().parse().map_err(|_| RtError::mismatch())?,
            );
            return Ok(V::Obj(Obj::Range(RangeRef {
                sheet: r.sheet,
                r0: r.r0 + x - 1,
                r1: r.r0 + y - 1,
                c0: r.c0,
                c1: r.c1,
            })));
        }
        v => to_int(v)?,
    };
    let row = rel(r, n - 1, 0)?;
    Ok(V::Obj(Obj::Range(RangeRef {
        r1: row.r0,
        c1: r.c1,
        ..row
    })))
}

fn cols_of(r: &RangeRef, a: &[V]) -> R<V> {
    if !has(a, 0) {
        return Ok(V::Obj(Obj::ColsOf(*r)));
    }
    let (x, y) = match arg(a, 0) {
        V::Str(s) if !arg(a, 0).is_numeric() => {
            let (x, y) = s.split_once(':').unwrap_or((s, s));
            let x = column_index(x.trim()).ok_or_else(RtError::mismatch)?;
            let y = column_index(y.trim()).ok_or_else(RtError::mismatch)?;
            (r.c0 + x, r.c0 + y)
        }
        v => {
            let n = to_int(v)? - 1;
            let c = rel(r, 0, n)?.c0;
            (c, c)
        }
    };
    Ok(V::Obj(Obj::Range(RangeRef {
        sheet: r.sheet,
        r0: r.r0,
        r1: r.r1,
        c0: x,
        c1: y,
    })))
}

/// The members of a range.
fn range_get(it: &mut Interp<'_>, r: &RangeRef, m: &str, a: Vec<V>) -> R<V> {
    let range = |x: RangeRef| V::Obj(Obj::Range(x));
    Ok(match m {
        "value" | "value2" => range_value(it, r)?,
        "formula" | "formulalocal" | "formular1c1" => {
            if m == "formular1c1" {
                return Err(unsupported("FormulaR1C1"));
            }
            if count(r) == 1 {
                V::Str(
                    it.wb
                        .edit_text(r.sheet, CellRef::new(r.r0, r.c0))
                        .map_err(wb_err)?,
                )
            } else {
                let mut arr = Array::new(&[(1, i64::from(r.rows())), (1, i64::from(r.cols()))]);
                for (k, at) in cells_of(r).enumerate() {
                    arr.data[k] = V::Str(it.wb.edit_text(r.sheet, at).map_err(wb_err)?);
                }
                V::Arr(Box::new(arr))
            }
        }
        "text" => V::Str(
            it.wb
                .display(r.sheet, CellRef::new(r.r0, r.c0))
                .map_err(wb_err)?,
        ),
        "hasformula" => {
            let model = it.wb.sheet(r.sheet).map_err(wb_err)?;
            V::Bool(cells_of(r).all(|at| model.cells.get(&at).is_some_and(|c| c.formula.is_some())))
        }
        "address" => {
            let row_abs = !has(&a, 0) || to_bool(arg(&a, 0))?;
            let col_abs = !has(&a, 1) || to_bool(arg(&a, 1))?;
            let mut s = address_text(r, row_abs, col_abs);
            if has(&a, 3) && to_bool(arg(&a, 3))? {
                s = format!("[Book]{}", formula_ref(it, r));
            }
            V::Str(s)
        }
        "row" => V::Int(i64::from(r.r0) + 1),
        "column" => V::Int(i64::from(r.c0) + 1),
        "count" | "countlarge" => V::Int(count(r) as i64),
        "cells" | "item" => {
            let x = cells_at(r, &a)?;
            range(x)
        }
        "range" => {
            // Relative to the range's corner.
            let inner = range_of(it, r.sheet, &a)?;
            range(RangeRef {
                sheet: r.sheet,
                r0: r.r0 + inner.r0,
                c0: r.c0 + inner.c0,
                r1: r.r0 + inner.r1,
                c1: r.c0 + inner.c1,
            })
        }
        "rows" => rows_of(r, &a)?,
        "columns" => cols_of(r, &a)?,
        "entirerow" => range(RangeRef {
            c0: 0,
            c1: MAX_COL - 1,
            ..*r
        }),
        "entirecolumn" => range(RangeRef {
            r0: 0,
            r1: MAX_ROW - 1,
            ..*r
        }),
        "offset" => {
            let dr = if has(&a, 0) { to_int(arg(&a, 0))? } else { 0 };
            let dc = if has(&a, 1) { to_int(arg(&a, 1))? } else { 0 };
            let first = rel(r, dr, dc)?;
            let last = rel(
                r,
                dr + i64::from(r.rows()) - 1,
                dc + i64::from(r.cols()) - 1,
            )?;
            range(RangeRef {
                r1: last.r0,
                c1: last.c0,
                ..first
            })
        }
        "resize" => {
            let rows = if has(&a, 0) {
                to_int(arg(&a, 0))?
            } else {
                i64::from(r.rows())
            };
            let cols = if has(&a, 1) {
                to_int(arg(&a, 1))?
            } else {
                i64::from(r.cols())
            };
            if rows < 1 || cols < 1 {
                return Err(RtError::new(
                    1004,
                    "Application-defined or object-defined error",
                ));
            }
            let last = rel(r, rows - 1, cols - 1)?;
            range(RangeRef {
                r1: last.r0,
                c1: last.c0,
                ..*r
            })
        }
        "end" => range(end(it, r, to_int(arg(&a, 0))?)?),
        "currentregion" => range(current_region(it, r)?),
        "worksheet" | "parent" => V::Obj(Obj::Sheet(r.sheet)),
        "find" => find(it, r, &a)?,
        "clearcontents" | "clear" => {
            clear(it, r)?;
            if m == "clear" {
                it.report.skipped.push(format!(
                    "line {}: Clear removed the values; formats are kept",
                    it.line
                ));
            }
            V::Empty
        }
        "delete" | "insert" => {
            insert_delete(it, r, m == "insert")?;
            V::Empty
        }
        "copy" => {
            if has(&a, 0) {
                let V::Obj(Obj::Range(dest)) = arg(&a, 0) else {
                    return Err(RtError::new(424, "Object required"));
                };
                copy_to(it, r, dest)?;
            } else {
                return Err(unsupported("Copy without Destination (the clipboard)"));
            }
            V::Empty
        }
        "select" | "activate" => {
            it.active_sheet = r.sheet;
            it.active_cell = (r.r0, r.c0);
            V::Empty
        }
        "calculate" => V::Empty,
        "font" => V::Obj(Obj::Font(*r)),
        "interior" => V::Obj(Obj::Interior(*r)),
        "borders" => V::Obj(Obj::Borders(
            *r,
            if has(&a, 0) {
                Some(to_int(arg(&a, 0))?)
            } else {
                None
            },
        )),
        "borderaround" => {
            let style = if has(&a, 0) { to_int(arg(&a, 0))? } else { 1 };
            let weight = if has(&a, 1) { to_int(arg(&a, 1))? } else { 2 };
            let color = if has(&a, 3) {
                Some(color_arg(arg(&a, 3))?)
            } else if has(&a, 2) {
                palette_color(to_int(arg(&a, 2))?)
            } else {
                None
            };
            let line = line_style(style, weight);
            draw_borders(it, r, kalem_viewer::BorderSet::Outside, line, color)?;
            V::Bool(true)
        }
        "merge" => {
            let across = has(&a, 0) && to_bool(arg(&a, 0))?;
            merge(it, r, true, across)?;
            V::Empty
        }
        "unmerge" => {
            merge(it, r, false, false)?;
            V::Empty
        }
        "numberformat" | "numberformatlocal" => V::Str(style_of(it, r)?.num_fmt),
        "horizontalalignment" => V::Int(match style_of(it, r)?.align.as_deref() {
            Some("left") => -4131,
            Some("center") => -4108,
            Some("right") => -4152,
            Some("centerContinuous") => 7,
            Some("justify") => -4130,
            Some("distributed") => -4117,
            Some("fill") => 5,
            _ => 1,
        }),
        "verticalalignment" => V::Int(match style_of(it, r)?.valign.as_deref() {
            Some("top") => -4160,
            Some("center") => -4108,
            Some("justify") => -4130,
            Some("distributed") => -4117,
            _ => -4107,
        }),
        "wraptext" => V::Bool(style_of(it, r)?.wrap),
        "indentlevel" => V::Int(i64::from(style_of(it, r)?.indent)),
        "locked" => V::Bool(!style_of(it, r)?.unlocked),
        "shrinktofit" => V::Bool(style_of(it, r)?.shrink),
        "orientation" => V::Int(match style_of(it, r)?.rotation {
            0 => -4128,
            255 => -4166,
            n @ 1..=90 => i64::from(n),
            n => 90 - i64::from(n),
        }),
        "mergecells" => {
            let (row, col) = (r.r0, r.c0);
            V::Bool(
                it.wb
                    .sheet(r.sheet)
                    .map_err(wb_err)?
                    .merged
                    .iter()
                    .any(|m| {
                        (m.start.row..=m.end.row).contains(&row)
                            && (m.start.col..=m.end.col).contains(&col)
                    }),
            )
        }
        "columnwidth" => {
            let sheet = it.wb.sheet(r.sheet).map_err(wb_err)?;
            let w = sheet
                .cols
                .iter()
                .find(|c| (c.min..=c.max).contains(&r.c0))
                .and_then(|c| c.width)
                .or(sheet.default_col_width)
                .unwrap_or(8.43);
            V::Num(w)
        }
        "rowheight" => {
            let sheet = it.wb.sheet(r.sheet).map_err(wb_err)?;
            let h = sheet
                .rows
                .get(&r.r0)
                .and_then(|x| x.height)
                .or(sheet.default_row_height)
                .unwrap_or(15.0);
            V::Num(h)
        }
        "characters" | "autofit" | "style" | "comment" | "addcomment" | "hyperlinks"
        | "validation" | "formatconditions" => {
            it.report.skipped.push(format!(
                "line {}: Range.{m} (not applied by macros in Kalem)",
                it.line
            ));
            V::Obj(Obj::Unsupported(m.to_owned()))
        }
        "sort" | "autofilter" | "pastespecial" | "texttocolumns" | "removeduplicates"
        | "specialcells" | "areas" | "cut" => {
            return Err(unsupported(&format!("Range.{m}")));
        }
        "filldown" => {
            // The first row copied into the others.
            let first = RangeRef { r1: r.r0, ..*r };
            for row in r.r0 + 1..=r.r1 {
                copy_to(
                    it,
                    &first,
                    &RangeRef {
                        r0: row,
                        r1: row,
                        ..first
                    },
                )?;
            }
            V::Empty
        }
        _ => {
            return Err(RtError::new(
                438,
                format!("Object doesn't support this property or method: Range.{m}"),
            ));
        }
    })
}

fn sheet_get(it: &mut Interp<'_>, idx: usize, m: &str, a: Vec<V>) -> R<V> {
    let all = sheet_range(idx);
    Ok(match m {
        "name" | "codename" => {
            if m == "codename" {
                return Ok(V::Str(
                    it.wb
                        .sheet(idx)
                        .map_err(wb_err)?
                        .code_name
                        .clone()
                        .unwrap_or_default(),
                ));
            }
            V::Str(it.wb.sheets()[idx].name.clone())
        }
        "index" => V::Int(idx as i64 + 1),
        "range" => {
            sheet_ok(it, idx)?;
            V::Obj(Obj::Range(range_of(it, idx, &a)?))
        }
        "cells" => {
            sheet_ok(it, idx)?;
            if has(&a, 0) {
                V::Obj(Obj::Range(cells_at(&all, &a)?))
            } else {
                V::Obj(Obj::Range(all))
            }
        }
        "rows" => rows_of(&all, &a)?,
        "columns" => cols_of(&all, &a)?,
        "usedrange" => V::Obj(Obj::Range(used_range(it, idx)?)),
        "activate" | "select" => {
            sheet_ok(it, idx)?;
            it.active_sheet = idx;
            it.active_cell = (0, 0);
            V::Empty
        }
        "visible" => V::Int(match it.wb.sheets()[idx].visibility {
            crate::workbook::Visibility::Visible => -1,
            crate::workbook::Visibility::Hidden => 0,
            crate::workbook::Visibility::VeryHidden => 2,
        }),
        "parent" => V::Obj(Obj::Workbook),
        "calculate" => V::Empty,
        "evaluate" => evaluate(it, idx, &to_str(arg(&a, 0))?)?,
        "protect" | "unprotect" | "tab" | "pagesetup" | "shapes" | "chartobjects"
        | "listobjects" | "pivottables" => {
            it.report.skipped.push(format!(
                "line {}: Worksheet.{m} is not available to macros in Kalem yet",
                it.line
            ));
            V::Obj(Obj::Unsupported(m.to_owned()))
        }
        "delete" | "copy" | "move" | "saveas" | "printout" => {
            return Err(unsupported(&format!("Worksheet.{m}")));
        }
        _ => {
            return Err(RtError::new(
                438,
                format!("Object doesn't support this property or method: Worksheet.{m}"),
            ));
        }
    })
}

fn sheet_by(it: &Interp<'_>, key: &V) -> R<usize> {
    match key {
        V::Str(s) => it
            .wb
            .sheet_index(s)
            .ok_or_else(|| RtError::new(9, format!("Subscript out of range: no sheet {s}"))),
        v => {
            let n = to_int(v)?;
            if n >= 1 && (n as usize) <= it.wb.sheets().len() {
                Ok(n as usize - 1)
            } else {
                Err(RtError::new(9, "Subscript out of range"))
            }
        }
    }
}

/// `WorksheetFunction.Name(args)` (raises errors) and `Application.Name(args)`
/// (returns them): computed by the formula engine as if typed in a cell.
fn worksheet_function(it: &mut Interp<'_>, name: &str, a: &[V], raise: bool) -> R<V> {
    let mut parts = Vec::new();
    for v in a {
        parts.push(match v {
            V::Missing => String::new(),
            V::Obj(Obj::Range(r)) => formula_ref(it, r),
            V::Str(s) => format!("\"{}\"", s.replace('"', "\"\"")),
            V::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            V::Arr(arr) => {
                let cols = if arr.dims.len() == 2 {
                    arr.dims[1].1
                } else {
                    arr.data.len()
                };
                let items: R<Vec<String>> = arr
                    .data
                    .iter()
                    .map(|x| match x {
                        V::Str(s) => Ok(format!("\"{}\"", s.replace('"', "\"\""))),
                        V::Empty => Ok("0".into()),
                        x => Ok(num_text(to_num(x)?)),
                    })
                    .collect();
                let items = items?;
                let rows: Vec<String> = items.chunks(cols.max(1)).map(|c| c.join(",")).collect();
                format!("{{{}}}", rows.join(";"))
            }
            V::Error(e) => constants::error_text(*e).into(),
            V::Empty => "0".into(),
            x => num_text(to_num(x)?),
        });
    }
    let formula = format!("{}({})", name.to_uppercase(), parts.join(","));
    let sheet = it.active_sheet;
    let v = it.wb.evaluate_formula(sheet, &formula).map_err(wb_err)?;
    match v {
        Some(Value::Error(e)) if raise => Err(RtError::new(
            1004,
            format!("Unable to get the {name} property of the WorksheetFunction class ({e})"),
        )),
        Some(Value::Error(e)) => Ok(V::Error(constants::error_number(&e))),
        Some(Value::Number(n)) => Ok(V::Num(n)),
        Some(Value::Text(t)) => Ok(V::Str(t)),
        Some(Value::Bool(b)) => Ok(V::Bool(b)),
        Some(Value::Empty) => Ok(V::Empty),
        None => Err(RtError::fatal(format!(
            "WorksheetFunction.{name} could not be computed"
        ))),
    }
}

/// `Evaluate("…")` and `[…]`: a reference is a range, anything else a formula.
pub fn evaluate(it: &mut Interp<'_>, sheet: usize, text: &str) -> R<V> {
    if let Some(r) = parse_address(it, sheet, text) {
        return Ok(V::Obj(Obj::Range(r)));
    }
    match it.wb.evaluate_formula(sheet, text).map_err(wb_err)? {
        Some(Value::Number(n)) => Ok(V::Num(n)),
        Some(Value::Text(t)) => Ok(V::Str(t)),
        Some(Value::Bool(b)) => Ok(V::Bool(b)),
        Some(Value::Error(e)) => Ok(V::Error(constants::error_number(&e))),
        Some(Value::Empty) => Ok(V::Empty),
        None => Err(RtError::new(1004, format!("cannot evaluate {text}"))),
    }
}

fn keyed_get(k: &Keyed, key: &V, collection: bool) -> R<Option<usize>> {
    if collection && !matches!(key, V::Str(_)) {
        let n = to_int(key)?;
        return Ok((n >= 1 && (n as usize) <= k.items.len()).then(|| n as usize - 1));
    }
    let key = if collection {
        V::Str(to_str(key)?.to_lowercase())
    } else {
        key.clone()
    };
    Ok(k.items
        .iter()
        .position(|(kk, _)| kk.as_ref().is_some_and(|kk| same_key(kk, &key))))
}

/// A member of an object, read or called.
pub fn get(it: &mut Interp<'_>, o: &Obj, m: &str, a: Vec<V>) -> R<V> {
    match o {
        Obj::Range(r) => range_get(it, r, m, a),
        Obj::Font(r) => font_get(it, r, m),
        Obj::Interior(r) => interior_get(it, r, m),
        Obj::Borders(r, side) => match m {
            // `Borders(xlEdgeBottom)` again, `Borders.Item(xlEdgeTop)`.
            "item" => Ok(V::Obj(Obj::Borders(*r, Some(to_int(arg(&a, 0))?)))),
            "linestyle" | "weight" | "color" | "colorindex" => {
                let st = style_of(it, r)?;
                let i = match side {
                    Some(8) => 0,
                    Some(10) => 1,
                    Some(9) => 2,
                    Some(7) => 3,
                    _ => 2,
                };
                Ok(match (m, st.sides[i]) {
                    ("linestyle", Some(_)) => V::Int(1),
                    ("linestyle", None) => V::Int(-4142),
                    ("weight", Some((_, true))) => V::Int(-4138),
                    ("weight", _) => V::Int(2),
                    ("color", s) => vba_color(s.map_or(0, |(c, _)| c)),
                    (_, s) => V::Int(s.and_then(|(c, _)| palette_index(c)).unwrap_or(-4105)),
                })
            }
            _ => Err(RtError::new(438, format!("Borders.{m}"))),
        },
        Obj::RowsOf(r) | Obj::ColsOf(r) => {
            let rows = matches!(o, Obj::RowsOf(_));
            match m {
                "count" => Ok(V::Int(i64::from(if rows { r.rows() } else { r.cols() }))),
                "item" => {
                    if rows {
                        rows_of(r, &a)
                    } else {
                        cols_of(r, &a)
                    }
                }
                "insert" | "delete" => {
                    let full = if rows {
                        RangeRef {
                            c0: 0,
                            c1: MAX_COL - 1,
                            ..*r
                        }
                    } else {
                        RangeRef {
                            r0: 0,
                            r1: MAX_ROW - 1,
                            ..*r
                        }
                    };
                    insert_delete(it, &full, m == "insert")?;
                    Ok(V::Empty)
                }
                _ => range_get(it, r, m, a),
            }
        }
        Obj::Sheet(i) => sheet_get(it, *i, m, a),
        Obj::Sheets => match m {
            "count" => Ok(V::Int(it.wb.sheets().len() as i64)),
            "item" => Ok(V::Obj(Obj::Sheet(sheet_by(it, arg(&a, 0))?))),
            "add" | "copy" | "move" | "delete" => Err(unsupported(&format!("Worksheets.{m}"))),
            _ => Err(RtError::new(
                438,
                format!("Object doesn't support this property or method: Worksheets.{m}"),
            )),
        },
        Obj::Workbook => match m {
            "worksheets" | "sheets" => {
                if has(&a, 0) {
                    Ok(V::Obj(Obj::Sheet(sheet_by(it, arg(&a, 0))?)))
                } else {
                    Ok(V::Obj(Obj::Sheets))
                }
            }
            "activesheet" => Ok(V::Obj(Obj::Sheet(it.active_sheet))),
            "name" | "fullname" => Ok(V::Str(it.host.workbook_name())),
            "path" => Ok(V::Str(String::new())),
            "save" => {
                it.report.save_requested = true;
                Ok(V::Empty)
            }
            "names" => Err(unsupported("Workbook.Names")),
            "saveas" | "close" | "savecopyas" | "printout" | "protect" | "unprotect" => {
                Err(unsupported(&format!("Workbook.{m}")))
            }
            _ => Err(RtError::new(
                438,
                format!("Object doesn't support this property or method: Workbook.{m}"),
            )),
        },
        Obj::Workbooks => match m {
            "count" => Ok(V::Int(1)),
            "item" => Ok(V::Obj(Obj::Workbook)),
            _ => Err(unsupported(&format!("Workbooks.{m} (other workbooks)"))),
        },
        Obj::Application => match m {
            "activesheet" => Ok(V::Obj(Obj::Sheet(it.active_sheet))),
            "activeworkbook" | "thisworkbook" => Ok(V::Obj(Obj::Workbook)),
            "activecell" | "selection" => {
                let (r, c) = it.active_cell;
                Ok(V::Obj(Obj::Range(RangeRef {
                    sheet: it.active_sheet,
                    r0: r,
                    c0: c,
                    r1: r,
                    c1: c,
                })))
            }
            "workbooks" => Ok(V::Obj(Obj::Workbooks)),
            "worksheets" | "sheets" => get(it, &Obj::Workbook, "worksheets", a),
            "range" | "cells" | "rows" | "columns" | "usedrange" => {
                sheet_get(it, it.active_sheet, m, a)
            }
            "worksheetfunction" => Ok(V::Obj(Obj::WorksheetFunction)),
            "evaluate" => evaluate(it, it.active_sheet, &to_str(arg(&a, 0))?),
            "inputbox" => {
                let s = |i: usize| {
                    if has(&a, i) {
                        to_str(arg(&a, i))
                    } else {
                        Ok(String::new())
                    }
                };
                let answer = it.host.input_box(&s(0)?, &s(1)?, &s(2)?);
                it.check_stopped()?;
                match answer {
                    // Cancel returns False from Application.InputBox.
                    None => Ok(V::Bool(false)),
                    Some(t) => Ok(if has(&a, 7) && to_int(arg(&a, 7))? == 1 {
                        V::Num(parse_number(&t).unwrap_or(0.0))
                    } else {
                        V::Str(t)
                    }),
                }
            }
            "run" => {
                let name = to_str(arg(&a, 0))?;
                it.run(&name, a.into_iter().skip(1).collect())
            }
            "intersect" => {
                let mut acc: Option<RangeRef> = None;
                for v in a.iter().filter(|v| !matches!(v, V::Missing)) {
                    let V::Obj(Obj::Range(r)) = v else {
                        return Err(RtError::mismatch());
                    };
                    acc = Some(match acc {
                        None => *r,
                        Some(x) if x.sheet == r.sheet => {
                            let (r0, r1, c0, c1) = (
                                x.r0.max(r.r0),
                                x.r1.min(r.r1),
                                x.c0.max(r.c0),
                                x.c1.min(r.c1),
                            );
                            if r0 > r1 || c0 > c1 {
                                return Ok(V::Nothing);
                            }
                            RangeRef {
                                sheet: x.sheet,
                                r0,
                                r1,
                                c0,
                                c1,
                            }
                        }
                        Some(_) => return Ok(V::Nothing),
                    });
                }
                Ok(acc.map_or(V::Nothing, |r| V::Obj(Obj::Range(r))))
            }
            "screenupdating" | "displayalerts" | "enableevents" | "interactive" => {
                Ok(V::Bool(true))
            }
            "calculation" => Ok(V::Int(-4105)),
            "version" => Ok(V::Str("16.0".into())),
            "username" => Ok(V::Str("Kalem".into())),
            "name" => Ok(V::Str("Microsoft Excel".into())),
            "calculate" | "calculatefull" | "volatile" | "wait" => Ok(V::Empty),
            "statusbar" => Ok(V::Bool(false)),
            "ontime" | "onkey" | "quit" | "sendkeys" | "dialogs" | "getopenfilename"
            | "getsaveasfilename" | "filedialog" => Err(unsupported(&format!("Application.{m}"))),
            // `Application.Sum(…)`: errors come back as values.
            name => worksheet_function(it, name, &a, false),
        },
        Obj::WorksheetFunction => worksheet_function(it, m, &a, true),
        Obj::ErrObject => match m {
            "number" => Ok(V::Int(it.err.0)),
            "description" => Ok(V::Str(it.err.1.clone())),
            "source" => Ok(V::Str(it.err.2.clone())),
            "clear" => {
                it.err = (0, String::new(), String::new());
                Ok(V::Empty)
            }
            "raise" => {
                let n = to_int(arg(&a, 0))?;
                let d = if has(&a, 2) {
                    to_str(arg(&a, 2))?
                } else {
                    format!("Application-defined or object-defined error {n}")
                };
                Err(RtError::new(n, d))
            }
            _ => Err(RtError::new(
                438,
                format!("Object doesn't support this property or method: Err.{m}"),
            )),
        },
        Obj::Debug => match m {
            "print" => {
                let parts: R<Vec<String>> = a
                    .iter()
                    .map(|v| {
                        if matches!(v, V::Null) {
                            Ok("Null".into())
                        } else {
                            to_str(v)
                        }
                    })
                    .collect();
                let line = parts?.join(" ");
                it.host.print(&line);
                it.report.output.push(line);
                Ok(V::Empty)
            }
            "assert" => Ok(V::Empty),
            _ => Err(RtError::new(438, format!("Debug.{m}"))),
        },
        Obj::Collection(c) => {
            let mut k = c.borrow_mut();
            match m {
                "count" => Ok(V::Int(k.items.len() as i64)),
                "add" => {
                    let key = if has(&a, 1) {
                        Some(V::Str(to_str(arg(&a, 1))?.to_lowercase()))
                    } else {
                        None
                    };
                    if let Some(key) = &key
                        && k.items.iter().any(|(kk, _)| kk.as_ref() == Some(key))
                    {
                        return Err(RtError::new(
                            457,
                            "This key is already associated with an element of this collection",
                        ));
                    }
                    k.items.push((key, arg(&a, 0).clone()));
                    Ok(V::Empty)
                }
                "item" => {
                    let i = keyed_get(&k, arg(&a, 0), true)?
                        .ok_or_else(|| RtError::new(5, "Invalid procedure call or argument"))?;
                    Ok(k.items[i].1.clone())
                }
                "remove" => {
                    let i = keyed_get(&k, arg(&a, 0), true)?
                        .ok_or_else(|| RtError::new(5, "Invalid procedure call or argument"))?;
                    k.items.remove(i);
                    Ok(V::Empty)
                }
                _ => Err(RtError::new(438, format!("Collection.{m}"))),
            }
        }
        Obj::Dictionary(d) => {
            let mut k = d.borrow_mut();
            let list = |vals: Vec<V>| {
                let mut arr = Array::new(&[(0, vals.len() as i64 - 1)]);
                arr.data = vals;
                V::Arr(Box::new(arr))
            };
            match m {
                "count" => Ok(V::Int(k.items.len() as i64)),
                "add" => {
                    if keyed_get(&k, arg(&a, 0), false)?.is_some() {
                        return Err(RtError::new(
                            457,
                            "This key is already associated with an element of this collection",
                        ));
                    }
                    k.items.push((Some(arg(&a, 0).clone()), arg(&a, 1).clone()));
                    Ok(V::Empty)
                }
                "exists" => Ok(V::Bool(keyed_get(&k, arg(&a, 0), false)?.is_some())),
                "item" => match keyed_get(&k, arg(&a, 0), false)? {
                    Some(i) => Ok(k.items[i].1.clone()),
                    None => {
                        // Reading a missing key adds it, as Scripting.Dictionary does.
                        k.items.push((Some(arg(&a, 0).clone()), V::Empty));
                        Ok(V::Empty)
                    }
                },
                "keys" => Ok(list(
                    k.items
                        .iter()
                        .map(|(kk, _)| kk.clone().unwrap_or_default())
                        .collect(),
                )),
                "items" => Ok(list(k.items.iter().map(|(_, v)| v.clone()).collect())),
                "remove" => {
                    let i = keyed_get(&k, arg(&a, 0), false)?
                        .ok_or_else(|| RtError::new(32_811, "Element not found"))?;
                    k.items.remove(i);
                    Ok(V::Empty)
                }
                "removeall" => {
                    k.items.clear();
                    Ok(V::Empty)
                }
                "comparemode" => Ok(V::Int(0)),
                _ => Err(RtError::new(438, format!("Dictionary.{m}"))),
            }
        }
        Obj::Unsupported(what) => {
            it.report
                .skipped
                .push(format!("line {}: {what}.{m}", it.line));
            Ok(V::Obj(Obj::Unsupported(format!("{what}.{m}"))))
        }
    }
}

/// `o.m = v` (or `o.m(args) = v`).
pub fn set(it: &mut Interp<'_>, o: &Obj, m: &str, a: Vec<V>, v: V) -> R<()> {
    match o {
        Obj::Range(r) => match m {
            "value" | "value2" | "formula" | "formulalocal" => set_range_value(it, r, v),
            "formular1c1" => Err(unsupported("FormulaR1C1")),
            _ => range_format_set(it, r, m, &v),
        },
        // `Rows(2).RowHeight = 20`, `Columns("A:C").ColumnWidth = 12`.
        Obj::RowsOf(r) | Obj::ColsOf(r) if settable(o, m) => set(it, &Obj::Range(*r), m, a, v),
        Obj::Font(r) => font_set(it, r, m, &v),
        Obj::Interior(r) => interior_set(it, r, m, &v),
        Obj::Borders(r, side) => borders_set(it, r, *side, m, &v),
        Obj::Application => match m {
            "screenupdating" | "displayalerts" | "enableevents" | "calculation" | "cursor"
            | "interactive" | "cutcopymode" | "displaystatusbar" => Ok(()),
            "statusbar" => {
                if !matches!(v, V::Bool(false)) {
                    it.host.status(&to_str(&v)?);
                }
                Ok(())
            }
            _ => Err(unsupported(&format!("Application.{m} = …"))),
        },
        Obj::Sheet(i) => match m {
            // `False`, `xlSheetHidden` and `xlSheetVeryHidden` hide it.
            "visible" => {
                let hidden = match v {
                    V::Bool(b) => !b,
                    ref v => to_int(v)? != -1,
                };
                it.wb
                    .edit_sheets(&kalem_viewer::SheetEdit::Hide(*i, hidden))
                    .map_err(wb_err)?;
                it.report.changed = true;
                Ok(())
            }
            "name" => {
                let name = to_str(&v)?;
                it.wb
                    .edit_sheets(&kalem_viewer::SheetEdit::Rename(*i, name))
                    .map_err(wb_err)?;
                it.report.changed = true;
                Ok(())
            }
            _ => Err(RtError::new(438, format!("Worksheet.{m} cannot be set"))),
        },
        Obj::ErrObject => {
            match m {
                "number" => it.err.0 = to_int(&v)?,
                "description" => it.err.1 = to_str(&v)?,
                "source" => it.err.2 = to_str(&v)?,
                _ => return Err(RtError::new(438, format!("Err.{m}"))),
            }
            Ok(())
        }
        Obj::Dictionary(d) if m == "item" => {
            let mut k = d.borrow_mut();
            match keyed_get(&k, arg(&a, 0), false)? {
                Some(i) => k.items[i].1 = v,
                None => k.items.push((Some(arg(&a, 0).clone()), v)),
            }
            Ok(())
        }
        Obj::Dictionary(_) if m == "comparemode" => Ok(()),
        Obj::Unsupported(what) => {
            it.report
                .skipped
                .push(format!("line {}: {what}.{m} = …", it.line));
            Ok(())
        }
        _ => Err(RtError::new(
            438,
            format!("Object doesn't support this property: {m}"),
        )),
    }
}

/// `o(args)`: the default member.
pub fn get_default(it: &mut Interp<'_>, o: &Obj, a: Vec<V>) -> R<V> {
    match o {
        Obj::Range(_)
        | Obj::RowsOf(_)
        | Obj::ColsOf(_)
        | Obj::Collection(_)
        | Obj::Dictionary(_)
        | Obj::Sheets
        | Obj::Workbooks => get(it, o, "item", a),
        // `Range("A1")` bare, `Cells(1, 1)` bare.
        Obj::Sheet(_) if a.len() == 2 || matches!(a.first(), Some(V::Int(_) | V::Num(_))) => {
            get(it, o, "cells", a)
        }
        Obj::Sheet(_) => get(it, o, "range", a),
        _ => Err(RtError::new(
            438,
            "Object doesn't support this property or method",
        )),
    }
}

/// `o(args) = v`.
pub fn set_default(it: &mut Interp<'_>, o: &Obj, a: Vec<V>, v: V) -> R<()> {
    match o {
        Obj::Range(_) if a.is_empty() => set(it, o, "value", a, v),
        Obj::Range(_) | Obj::Sheet(_) => {
            let target = get_default(it, o, a)?;
            match target {
                V::Obj(t) => set(it, &t, "value", Vec::new(), v),
                _ => Err(RtError::new(424, "Object required")),
            }
        }
        Obj::Dictionary(_) => set(it, o, "item", a, v),
        Obj::Collection(_) => Err(RtError::new(438, "a Collection's items cannot be replaced")),
        Obj::Unsupported(_) => set(it, o, "value", a, v),
        _ => Err(RtError::new(
            438,
            "Object doesn't support this property or method",
        )),
    }
}

/// The value an object stands for in an expression.
pub fn default_value(it: &mut Interp<'_>, o: &Obj) -> R<V> {
    match o {
        Obj::Range(r) | Obj::RowsOf(r) | Obj::ColsOf(r) => range_value(it, r),
        Obj::ErrObject => Ok(V::Int(it.err.0)),
        Obj::Application => Ok(V::Str("Microsoft Excel".into())),
        Obj::Unsupported(_) => Ok(V::Empty),
        _ => Err(RtError::new(
            438,
            "Object doesn't support this property or method",
        )),
    }
}

/// What `For Each` walks over an object.
pub fn elements(it: &mut Interp<'_>, o: &Obj) -> R<Vec<V>> {
    Ok(match o {
        Obj::Range(r) => {
            // Only the used part of whole rows and columns.
            let r = clip_to_used(it, r)?;
            if count(&r) > MAX_CELLS {
                return Err(RtError::new(
                    7,
                    "Out of memory: For Each over too many cells",
                ));
            }
            cells_of(&r)
                .map(|c| {
                    V::Obj(Obj::Range(RangeRef {
                        sheet: r.sheet,
                        r0: c.row,
                        c0: c.col,
                        r1: c.row,
                        c1: c.col,
                    }))
                })
                .collect()
        }
        Obj::RowsOf(r) => {
            let r = clip_to_used(it, r)?;
            (r.r0..=r.r1)
                .map(|row| {
                    V::Obj(Obj::Range(RangeRef {
                        r0: row,
                        r1: row,
                        ..r
                    }))
                })
                .collect()
        }
        Obj::ColsOf(r) => {
            let r = clip_to_used(it, r)?;
            (r.c0..=r.c1)
                .map(|col| {
                    V::Obj(Obj::Range(RangeRef {
                        c0: col,
                        c1: col,
                        ..r
                    }))
                })
                .collect()
        }
        Obj::Sheets => (0..it.wb.sheets().len())
            .map(|i| V::Obj(Obj::Sheet(i)))
            .collect(),
        Obj::Workbooks => vec![V::Obj(Obj::Workbook)],
        Obj::Collection(c) => c.borrow().items.iter().map(|(_, v)| v.clone()).collect(),
        Obj::Dictionary(d) => d
            .borrow()
            .items
            .iter()
            .map(|(k, _)| k.clone().unwrap_or_default())
            .collect(),
        _ => {
            return Err(RtError::new(
                438,
                "Object doesn't support this property or method",
            ));
        }
    })
}

fn clip_to_used(it: &mut Interp<'_>, r: &RangeRef) -> R<RangeRef> {
    if !whole_rows(r) && !whole_cols(r) {
        return Ok(*r);
    }
    let u = used_range(it, r.sheet)?;
    Ok(RangeRef {
        sheet: r.sheet,
        r0: r.r0,
        c0: r.c0,
        r1: if whole_cols(r) {
            r.r1.min(u.r1).max(r.r0)
        } else {
            r.r1
        },
        c1: if whole_rows(r) {
            r.c1.min(u.c1).max(r.c0)
        } else {
            r.c1
        },
    })
}
