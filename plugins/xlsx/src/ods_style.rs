//! An OpenDocument spreadsheet's looks, which calamine does not read: each
//! cell's style (`table:style-name`), each column's default cell style,
//! and the styles' background, font color, bold, italic and underline,
//! a style's parent's where it says nothing.

use std::collections::HashMap;

use crate::xml::{Reader, Token};

/// What a cell style shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Look {
    /// Its background.
    pub fill: Option<[u8; 3]>,
    /// Its text's color.
    pub color: Option<[u8; 3]>,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
}

impl Look {
    /// Whether it shows anything an empty cell shows.
    pub fn paints(&self) -> bool {
        self.fill.is_some()
    }
}

/// A sheet's styles by place.
#[derive(Debug, Clone, Default)]
pub struct SheetLooks {
    /// Columns' default cell styles: first and last column, style.
    pub cols: Vec<(u32, u32, String)>,
    /// Runs of cells: first and last row, first and last column, their
    /// style (`None` for cells without one, which take the column's).
    pub cells: Vec<(u32, u32, u32, u32, Option<String>)>,
}

/// A workbook's looks: its styles and each sheet's.
#[derive(Debug, Clone, Default)]
pub struct Looks {
    styles: HashMap<String, (Option<String>, Look, [bool; 5])>,
    /// Each sheet's, in order.
    pub sheets: Vec<SheetLooks>,
}

fn hex(v: &str) -> Option<[u8; 3]> {
    let v = v.trim().strip_prefix('#')?;
    if v.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(v, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// Reads the cell styles of a part (`content.xml`, `styles.xml`).
fn read_styles(text: &str, out: &mut HashMap<String, (Option<String>, Look, [bool; 5])>) {
    let mut r = Reader::new(text);
    let mut cur: Option<String> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "style" => {
                let cell = tag.attr("family").as_deref() == Some("table-cell");
                let name = tag.attr("name").map(|v| v.into_owned());
                cur = None;
                if let (true, Some(n)) = (cell, name) {
                    let parent = tag.attr("parent-style-name").map(|v| v.into_owned());
                    out.insert(n.clone(), (parent, Look::default(), [false; 5]));
                    if !tag.empty {
                        cur = Some(n);
                    }
                }
            }
            Token::Start(tag) if tag.name == "default-style" => cur = None,
            Token::Start(tag) if cur.is_some() => {
                let Some(e) = cur.as_ref().and_then(|n| out.get_mut(n)) else {
                    continue;
                };
                let (look, set) = (&mut e.1, &mut e.2);
                match tag.name {
                    "table-cell-properties" => {
                        if let Some(v) = tag.attr("background-color") {
                            look.fill = hex(&v);
                            set[0] = true;
                        }
                    }
                    "text-properties" => {
                        if let Some(v) = tag.attr("color") {
                            look.color = hex(&v);
                            set[1] = true;
                        }
                        if let Some(v) = tag.attr("font-weight") {
                            look.bold = v == "bold" || v.parse::<u32>().is_ok_and(|w| w >= 600);
                            set[2] = true;
                        }
                        if let Some(v) = tag.attr("font-style") {
                            look.italic = v == "italic" || v == "oblique";
                            set[3] = true;
                        }
                        if let Some(v) = tag.attr("text-underline-style") {
                            look.underline = v != "none";
                            set[4] = true;
                        }
                    }
                    _ => {}
                }
            }
            Token::End { name: "style", .. } => cur = None,
            _ => {}
        }
    }
}

fn repeat(tag: &crate::xml::Tag<'_>, k: &str) -> u32 {
    tag.attr(k)
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1)
        .max(1)
}

impl Looks {
    /// An OpenDocument spreadsheet's looks, from its bytes.
    pub fn read(bytes: &[u8]) -> Option<Looks> {
        let pkg = crate::package::Package::read(bytes.to_vec()).ok()?;
        let text = |name: &str| {
            pkg.part(name)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        let content = text("content.xml")?;
        let mut looks = Looks::default();
        if let Some(s) = text("styles.xml") {
            read_styles(&s, &mut looks.styles);
        }
        read_styles(&content, &mut looks.styles);
        let mut r = Reader::new(&content);
        let mut sheet: Option<SheetLooks> = None;
        let (mut row, mut col, mut rows_here) = (0u32, 0u32, 1u32);
        let mut row_default: Option<String> = None;
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) => match tag.name {
                    "table" if sheet.is_none() => {
                        sheet = Some(SheetLooks::default());
                        (row, col) = (0, 0);
                    }
                    "table-column" => {
                        if let Some(s) = sheet.as_mut() {
                            let n = repeat(&tag, "number-columns-repeated");
                            if let Some(style) = tag.attr("default-cell-style-name") {
                                s.cols
                                    .push((col, col.saturating_add(n - 1), style.into_owned()));
                            }
                            col = col.saturating_add(n);
                        }
                    }
                    "table-row" => {
                        col = 0;
                        rows_here = repeat(&tag, "number-rows-repeated");
                        row_default = tag.attr("default-cell-style-name").map(|v| v.into_owned());
                        if tag.empty {
                            row = row.saturating_add(rows_here);
                        }
                    }
                    "table-cell" | "covered-table-cell" => {
                        if let Some(s) = sheet.as_mut() {
                            let n = repeat(&tag, "number-columns-repeated");
                            let style = tag
                                .attr("style-name")
                                .map(|v| v.into_owned())
                                .or_else(|| row_default.clone());
                            // A style a cell says, or none (the column's).
                            let last = s.cells.last_mut();
                            match last {
                                Some(l)
                                    if l.0 == row
                                        && l.1 == row + rows_here - 1
                                        && l.3 + 1 == col
                                        && l.4 == style =>
                                {
                                    l.3 = col.saturating_add(n - 1);
                                }
                                _ => s.cells.push((
                                    row,
                                    row.saturating_add(rows_here - 1),
                                    col,
                                    col.saturating_add(n - 1),
                                    style,
                                )),
                            }
                            col = col.saturating_add(n);
                        }
                        if !tag.empty {
                            r.skip_element();
                        }
                    }
                    _ => {}
                },
                Token::End { name, .. } => match name {
                    "table-row" => row = row.saturating_add(rows_here),
                    "table" => {
                        if let Some(s) = sheet.take() {
                            looks.sheets.push(s);
                        }
                    }
                    _ => {}
                },
                Token::Text { .. } => {}
            }
        }
        Some(looks)
    }

    /// A style's look, its parents' where it says nothing.
    pub fn style(&self, name: &str) -> Look {
        let mut out = Look::default();
        let mut done = [false; 5];
        let mut at = Some(name.to_owned());
        let mut hops = 0;
        while let Some(n) = at.take() {
            let Some((parent, look, set)) = self.styles.get(&n) else {
                break;
            };
            if set[0] && !done[0] {
                out.fill = look.fill;
                done[0] = true;
            }
            if set[1] && !done[1] {
                out.color = look.color;
                done[1] = true;
            }
            if set[2] && !done[2] {
                out.bold = look.bold;
                done[2] = true;
            }
            if set[3] && !done[3] {
                out.italic = look.italic;
                done[3] = true;
            }
            if set[4] && !done[4] {
                out.underline = look.underline;
                done[4] = true;
            }
            hops += 1;
            if hops < 16 {
                at = parent.clone();
            }
        }
        out
    }

    /// Cell `(row, col)` of sheet `sheet`'s look: its own style's, else
    /// its column's default.
    pub fn at(&self, sheet: usize, row: u32, col: u32) -> Look {
        let Some(s) = self.sheets.get(sheet) else {
            return Look::default();
        };
        // The runs are in row order, a row block's together.
        let end = s.cells.partition_point(|c| c.0 <= row);
        let block = end.checked_sub(1).map(|i| s.cells[i].0);
        let own = s.cells[..end]
            .iter()
            .rev()
            .take_while(|c| Some(c.0) == block)
            .find(|c| c.1 >= row && (c.2..=c.3).contains(&col))
            .and_then(|c| c.4.clone());
        let name = own.or_else(|| {
            s.cols
                .iter()
                .find(|c| (c.0..=c.1).contains(&col))
                .map(|c| c.2.clone())
        });
        name.map_or_else(Look::default, |n| self.style(&n))
    }

    /// The last row and column a sheet's styles paint, within `limit`.
    pub fn extent(&self, sheet: usize, limit: (u32, u32)) -> (u32, u32) {
        let Some(s) = self.sheets.get(sheet) else {
            return (0, 0);
        };
        let mut rows = 0;
        let mut cols = 0;
        for c in &s.cells {
            if c.4.as_ref().is_some_and(|n| self.style(n).paints()) && c.1 < limit.0 {
                rows = rows.max(c.1 + 1);
                cols = cols.max((c.3 + 1).min(limit.1));
            }
        }
        for c in &s.cols {
            if self.style(&c.2).paints() && c.0 < limit.1 {
                cols = cols.max((c.1 + 1).min(limit.1));
            }
        }
        (rows, cols)
    }
}
