//! Sparklines, as Excel 2010 writes them: `<x14:sparklineGroups>` in the
//! sheet's `<extLst>`, a group per insertion with its type, colors and
//! high and low points, each sparkline its data (`<xm:f>`) and cell
//! (`<xm:sqref>`).

use super::*;
use kalem_viewer::SparklineKind;

const EXT_URI: &str = "{05C60535-1F16-4fd2-B633-F4F36F0B64E0}";
const X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const XM: &str = "http://schemas.microsoft.com/office/excel/2006/main";

/// A sparkline as read: its kind, colors and marks, its data (a sheet's
/// name when the formula names one) and its cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Spark {
    /// Line, columns or win/loss.
    pub kind: SparklineKind,
    /// The highest and lowest points marked.
    pub high: bool,
    /// The lowest.
    pub low: bool,
    /// The series' color.
    pub color: [u8; 3],
    /// Negative values', and the marks'.
    pub marker: [u8; 3],
    /// The sheet the data is on, when named.
    pub sheet: Option<String>,
    /// The data.
    pub data: Range,
    /// The cell it is drawn in.
    pub cell: CellRef,
}

fn argb(v: &str) -> Option<[u8; 3]> {
    let h = v.trim();
    let h = if h.len() == 8 { &h[2..] } else { h };
    let n = u32::from_str_radix(h, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// A sheet part's sparklines.
fn parse(text: &str) -> Vec<Spark> {
    let mut r = Reader::new(text);
    let mut out = Vec::new();
    let mut group: Option<Spark> = None;
    let mut formula: Option<String> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => match tag.name {
                "sparklineGroup" => {
                    let flag = |k: &str| {
                        tag.attr(k)
                            .as_deref()
                            .is_some_and(|v| v == "1" || v == "true")
                    };
                    group = Some(Spark {
                        kind: match tag.attr("type").as_deref() {
                            Some("column") => SparklineKind::Column,
                            Some("stacked") => SparklineKind::WinLoss,
                            _ => SparklineKind::Line,
                        },
                        high: flag("high"),
                        low: flag("low"),
                        color: [0x37, 0x60, 0x92],
                        marker: [0xD0, 0, 0],
                        sheet: None,
                        data: Range {
                            start: CellRef::new(0, 0),
                            end: CellRef::new(0, 0),
                        },
                        cell: CellRef::new(0, 0),
                    });
                }
                "colorSeries" => {
                    if let (Some(g), Some(c)) =
                        (group.as_mut(), tag.attr("rgb").and_then(|v| argb(&v)))
                    {
                        g.color = c;
                    }
                }
                "colorNegative" | "colorHigh" => {
                    if let (Some(g), Some(c)) =
                        (group.as_mut(), tag.attr("rgb").and_then(|v| argb(&v)))
                    {
                        g.marker = c;
                    }
                }
                "f" if group.is_some() && !tag.empty => formula = Some(r.text_until_end("f").0),
                "sqref" if group.is_some() && !tag.empty => {
                    let cell = r.text_until_end("sqref").0;
                    if let (Some(g), Some(f)) = (group.as_ref(), formula.take()) {
                        let (sheet, refs) = match f.rsplit_once('!') {
                            Some((s, r)) => {
                                (Some(s.trim_matches('\'').replace("''", "'")), r.to_owned())
                            }
                            None => (None, f.clone()),
                        };
                        let refs = refs.replace('$', "");
                        let data = Range::parse(&refs)
                            .or_else(|| CellRef::parse(&refs).map(|c| Range { start: c, end: c }));
                        if let (Some(data), Some(cell)) = (data, CellRef::parse(cell.trim())) {
                            out.push(Spark {
                                sheet,
                                data,
                                cell,
                                ..g.clone()
                            });
                        }
                    }
                }
                _ => {}
            },
            Token::End { name, .. } => {
                if name == "sparklineGroup" {
                    group = None;
                }
            }
            Token::Text { .. } => {}
        }
    }
    out
}

/// The `<x14:sparklineGroup>` of sparklines `lines` (data, cell).
fn group_xml(sheet: &str, kind: SparklineKind, mark: bool, lines: &[(Range, CellRef)]) -> String {
    let ty = match kind {
        SparklineKind::Line => "",
        SparklineKind::Column => " type=\"column\"",
        SparklineKind::WinLoss => " type=\"stacked\"",
    };
    let marks = if mark { " high=\"1\" low=\"1\"" } else { "" };
    let quoted = super::sheet_ops::qualified(sheet);
    let items: String = lines
        .iter()
        .map(|(d, c)| {
            format!(
                "<x14:sparkline><xm:f>{}!{d}</xm:f><xm:sqref>{c}</xm:sqref></x14:sparkline>",
                xml::escape(&quoted)
            )
        })
        .collect();
    format!(
        "<x14:sparklineGroup displayEmptyCellsAs=\"gap\"{ty}{marks}><x14:colorSeries rgb=\"FF376092\"/><x14:colorNegative rgb=\"FFD00000\"/><x14:colorAxis rgb=\"FF000000\"/><x14:colorMarkers rgb=\"FFD00000\"/><x14:colorFirst rgb=\"FFD00000\"/><x14:colorLast rgb=\"FFD00000\"/><x14:colorHigh rgb=\"FFD00000\"/><x14:colorLow rgb=\"FFD00000\"/><x14:sparklines>{items}</x14:sparklines></x14:sparklineGroup>"
    )
}

impl Workbook {
    /// The sparklines of sheet `idx`.
    pub fn sparklines(&mut self, idx: usize) -> Vec<Spark> {
        if let Some((g, v)) = self.spark_cache.get(&idx)
            && *g == self.generation
        {
            return v.clone();
        }
        if self.load(idx).is_err() {
            return Vec::new();
        }
        // Only a sheet with the extension has any: no scan of the others.
        let text = &self.loaded[&idx].0;
        let v = if text.contains("sparklineGroup") {
            parse(text)
        } else {
            Vec::new()
        };
        self.spark_cache.insert(idx, (self.generation, v.clone()));
        v
    }

    /// Puts sparklines in `location` (a row or a column of cells) of
    /// `data`'s rows or columns, one each. One undo step.
    pub fn add_sparklines(
        &mut self,
        idx: usize,
        data: Range,
        location: Range,
        kind: SparklineKind,
        mark: bool,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let down = location.start.col == location.end.col;
        let across = location.start.row == location.end.row;
        let cells: Vec<CellRef> = if down {
            (location.start.row..=location.end.row)
                .map(|r| CellRef::new(r, location.start.col))
                .collect()
        } else if across {
            (location.start.col..=location.end.col)
                .map(|c| CellRef::new(location.start.row, c))
                .collect()
        } else {
            return Err(Error::Refused(
                "Put sparklines in one row or one column".into(),
            ));
        };
        let (rows, cols) = (
            data.end.row - data.start.row + 1,
            data.end.col - data.start.col + 1,
        );
        // One sparkline a cell: the data's rows down a column, its columns
        // along a row, or a single line for a single cell.
        let lines: Vec<(Range, CellRef)> = if cells.len() == 1 {
            vec![(data, cells[0])]
        } else if down && rows as usize == cells.len() {
            cells
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let r = data.start.row + i as u32;
                    (
                        Range {
                            start: CellRef::new(r, data.start.col),
                            end: CellRef::new(r, data.end.col),
                        },
                        *c,
                    )
                })
                .collect()
        } else if across && cols as usize == cells.len() {
            cells
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let col = data.start.col + i as u32;
                    (
                        Range {
                            start: CellRef::new(data.start.row, col),
                            end: CellRef::new(data.end.row, col),
                        },
                        *c,
                    )
                })
                .collect()
        } else {
            return Err(Error::Refused(
                "The data has to have a row or a column for each sparkline".into(),
            ));
        };
        let sheet = self.sheets[idx].name.clone();
        let group = group_xml(&sheet, kind, mark, &lines);
        self.in_one_step(|wb| {
            let text = wb.loaded[&idx].0.clone();
            let p = wb.loaded[&idx].1.prefix.clone();
            let new = match text.find("<x14:sparklineGroups") {
                Some(at) => {
                    let open_end = text[at..].find('>').map_or(text.len(), |e| at + e + 1);
                    splice(&text, vec![(open_end..open_end, group)])
                }
                None => {
                    let ext = format!(
                        "<{p}ext uri=\"{EXT_URI}\" xmlns:x14=\"{X14}\"><x14:sparklineGroups xmlns:xm=\"{XM}\">{group}</x14:sparklineGroups></{p}ext>"
                    );
                    match text.find(&format!("</{p}extLst>")) {
                        Some(at) => splice(&text, vec![(at..at, ext)]),
                        None => insert_top_level(&text, &[], &format!("<{p}extLst>{ext}</{p}extLst>")),
                    }
                }
            };
            wb.replace_sheet_text(idx, new);
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Takes away the sparklines drawn in `range`'s cells (and groups left
    /// empty). One undo step.
    pub fn clear_sparklines(&mut self, idx: usize, range: Range) -> Result<()> {
        self.load(idx)?;
        let text = self.loaded[&idx].0.clone();
        let mut r = Reader::new(&text);
        let mut splices: Vec<(Span<usize>, String)> = Vec::new();
        // The group's start, its sparklines and those going.
        let mut group: Option<(usize, usize, usize)> = None;
        let mut spark_start = 0;
        let mut goes = false;
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) if tag.name == "sparklineGroup" => {
                    group = Some((tag.span.start, splices.len(), 0));
                }
                Token::Start(tag) if tag.name == "sparkline" => {
                    spark_start = tag.span.start;
                    goes = false;
                }
                Token::Start(tag) if tag.name == "sqref" && !tag.empty => {
                    let cell = r.text_until_end("sqref").0;
                    goes = CellRef::parse(cell.trim()).is_some_and(|c| range.contains(c));
                }
                Token::End {
                    name: "sparkline",
                    span,
                } => {
                    if let Some(g) = group.as_mut() {
                        g.2 += 1;
                        if goes {
                            splices.push((spark_start..span.end, String::new()));
                        }
                    }
                }
                Token::End {
                    name: "sparklineGroup",
                    span,
                } => {
                    if let Some((start, first, all)) = group.take()
                        && all > 0
                        && splices.len() - first == all
                    {
                        // The whole group goes instead of its sparklines.
                        splices.truncate(first);
                        splices.push((start..span.end, String::new()));
                    }
                }
                _ => {}
            }
        }
        if splices.is_empty() {
            return Ok(());
        }
        self.in_one_step(|wb| {
            let mut new = splice(&text, splices);
            // A sparkline extension left empty goes too, and an empty list.
            if parse(&new).is_empty()
                && let Some(u) = new.find(&format!("uri=\"{EXT_URI}\""))
                && let Some(a) = new[..u].rfind('<')
                && let Some(g) = new[u..].find("</x14:sparklineGroups>")
                && let Some(e) = new[u + g..].find("ext>")
            {
                new.replace_range(a..u + g + e + "ext>".len(), "");
            }
            let p = wb.loaded[&idx].1.prefix.clone();
            new = new.replace(&format!("<{p}extLst></{p}extLst>"), "");
            wb.replace_sheet_text(idx, new);
            wb.batch_changed = true;
            Ok(())
        })
    }
}
