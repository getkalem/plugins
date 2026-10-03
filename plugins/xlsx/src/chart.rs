//! Charts (ECMA-376 part 1, 21.2 and 20.5): where a sheet's drawing puts
//! them, what their parts say, and new ones written as Excel writes them.

use std::ops::Range as Span;

use kalem_viewer::ChartKind;

use crate::styles::Rgb;
use crate::xml::{self, Reader, Token};

/// A chart's place in a drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    /// The first cell it covers: row, column.
    pub from: (u32, u32),
    /// The last cell it covers.
    pub to: (u32, u32),
    /// The drawing's relationship to the chart part.
    pub rid: String,
    /// The anchor element's bytes.
    pub span: Span<usize>,
}

/// EMUs of a default column (64 pixels) and row (20 pixels).
const COL_EMU: i64 = 609_600;
const ROW_EMU: i64 = 190_500;

/// The charts of a drawing part, in order.
pub fn parse_drawing(text: &str) -> Vec<Anchor> {
    let mut out = Vec::new();
    let mut r = Reader::new(text);
    let mut cur: Option<(usize, String)> = None;
    let (mut from, mut to, mut ext) = ((0u32, 0u32), None, None);
    let mut rid: Option<String> = None;
    let mut which = "";
    let mut to_off = (0i64, 0i64);
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => match tag.name {
                "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor" if !tag.empty => {
                    cur = Some((tag.span.start, tag.name.to_owned()));
                    (from, to, ext, rid) = ((0, 0), None, None, None);
                    to_off = (0, 0);
                }
                "from" | "to" => which = tag.name,
                "colOff" | "rowOff" if !tag.empty && cur.is_some() && which == "to" => {
                    let name = tag.name;
                    let v: i64 = r.text_until_end(name).0.trim().parse().unwrap_or(0);
                    if name == "rowOff" {
                        to_off.0 = v;
                    } else {
                        to_off.1 = v;
                    }
                }
                "col" | "row" if !tag.empty && cur.is_some() => {
                    let name = tag.name;
                    let v: u32 = r.text_until_end(name).0.trim().parse().unwrap_or(0);
                    let target = if which == "to" {
                        to.get_or_insert((0, 0))
                    } else {
                        &mut from
                    };
                    if name == "row" {
                        target.0 = v;
                    } else {
                        target.1 = v;
                    }
                }
                "ext" if cur.is_some() && tag.attr("cx").is_some() && ext.is_none() => {
                    let n = |k: &str| tag.attr(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                    ext = Some((n("cx"), n("cy")));
                }
                "chart" if cur.is_some() => rid = tag.attr("id").map(|v| v.into_owned()),
                _ => {}
            },
            Token::End { name, span } => {
                if cur.as_ref().is_some_and(|c| c.1 == name) {
                    let (start, _) = cur.take().expect("an anchor");
                    if let Some(rid) = rid.take() {
                        // A `to` at a cell's very edge does not cover it.
                        let to = to.map(|(r, c)| {
                            (
                                if to_off.0 == 0 && r > from.0 {
                                    r - 1
                                } else {
                                    r
                                },
                                if to_off.1 == 0 && c > from.1 {
                                    c - 1
                                } else {
                                    c
                                },
                            )
                        });
                        let to = to.unwrap_or_else(|| {
                            let (cx, cy) = ext.unwrap_or((COL_EMU * 8, ROW_EMU * 15));
                            (
                                from.0 + (cy / ROW_EMU).max(1) as u32 - 1,
                                from.1 + (cx / COL_EMU).max(1) as u32 - 1,
                            )
                        });
                        out.push(Anchor {
                            from,
                            to,
                            rid,
                            span: start..span.end,
                        });
                    }
                }
            }
            Token::Text { .. } => {}
        }
    }
    out
}

/// A series as its part says it: the cells it names and what was cached.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeriesDef {
    /// The name's cell, and its cached text.
    pub name: (Option<String>, String),
    /// The categories' (or x values') cells, and their cached texts.
    pub cat: (Option<String>, Vec<String>),
    /// The values' cells, and their cached numbers.
    pub val: (Option<String>, Vec<Option<f64>>),
    /// Its fill or line color.
    pub color: Option<Rgb>,
}

/// A chart part as Kalem reads it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartDef {
    /// What kind.
    pub kind: ChartKind,
    /// Its title, when it has one of its own.
    pub title: Option<String>,
    /// The automatic title (a single series' name) is turned off.
    pub title_deleted: bool,
    /// Stacked bars or areas.
    pub stacked: bool,
    /// The horizontal axis's title.
    pub horizontal_title: Option<String>,
    /// The vertical axis's title.
    pub vertical_title: Option<String>,
    /// Its series, of its first plot.
    pub series: Vec<SeriesDef>,
}

fn cache_points(r: &mut Reader<'_>, end: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut idx = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "pt" => {
                idx = tag
                    .attr("idx")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(out.len());
            }
            Token::Start(tag) if tag.name == "v" && !tag.empty => {
                out.push((idx, r.text_until_end("v").0));
            }
            Token::End { name, .. } if name == end => break,
            _ => {}
        }
    }
    out
}

fn dense(points: Vec<(usize, String)>) -> Vec<String> {
    let n = points.iter().map(|p| p.0 + 1).max().unwrap_or(0);
    let mut out = vec![String::new(); n.min(100_000)];
    for (i, v) in points {
        if let Some(slot) = out.get_mut(i) {
            *slot = v;
        }
    }
    out
}

/// A theme color by its scheme name, Excel's defaults when the file has
/// no theme.
fn scheme(name: &str, theme: &[Rgb]) -> Option<Rgb> {
    const ACCENTS: [Rgb; 6] = [0x4472C4, 0xED7D31, 0xA5A5A5, 0xFFC000, 0x5B9BD5, 0x70AD47];
    let i = match name {
        "dk1" | "tx1" => 0,
        "lt1" | "bg1" => 1,
        "dk2" | "tx2" => 2,
        "lt2" | "bg2" => 3,
        a => 3 + a.strip_prefix("accent")?.parse::<usize>().ok()?,
    };
    theme
        .get(i)
        .copied()
        .or_else(|| (i >= 4).then(|| ACCENTS[(i - 4) % 6]))
}

/// Reads a chart part.
pub fn parse_chart(text: &str, theme: &[Rgb]) -> ChartDef {
    let mut def = ChartDef {
        kind: ChartKind::Other,
        ..ChartDef::default()
    };
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    let mut plot_seen = false;
    let mut in_plot = false;
    let mut ser: Option<SeriesDef> = None;
    let mut title = String::new();
    let mut title_ref: Option<String> = None;
    let (mut axis_title, mut axis_pos) = (String::new(), String::new());
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                let name = tag.name;
                let parent = stack.last().map(String::as_str).unwrap_or("");
                match name {
                    "barChart" | "bar3DChart" | "lineChart" | "line3DChart" | "areaChart"
                    | "area3DChart" | "pieChart" | "pie3DChart" | "ofPieChart"
                    | "doughnutChart" | "scatterChart" | "radarChart" | "stockChart"
                    | "surfaceChart" | "surface3DChart" | "bubbleChart"
                        if !plot_seen =>
                    {
                        plot_seen = true;
                        in_plot = true;
                        def.kind = match name {
                            "barChart" | "bar3DChart" => ChartKind::Column,
                            "lineChart" | "line3DChart" => ChartKind::Line,
                            "areaChart" | "area3DChart" => ChartKind::Area,
                            "pieChart" | "pie3DChart" | "ofPieChart" => ChartKind::Pie,
                            "doughnutChart" => ChartKind::Doughnut,
                            "scatterChart" => ChartKind::Scatter,
                            _ => ChartKind::Other,
                        };
                    }
                    "autoTitleDeleted" if parent == "chart" => {
                        def.title_deleted =
                            matches!(tag.attr("val").as_deref(), Some("1" | "true") | None);
                    }
                    "barDir" if in_plot && tag.attr("val").as_deref() == Some("bar") => {
                        def.kind = ChartKind::Bar;
                    }
                    "grouping"
                        if in_plot
                            && matches!(
                                tag.attr("val").as_deref(),
                                Some("stacked" | "percentStacked")
                            ) =>
                    {
                        def.stacked = true;
                    }
                    "ser" if in_plot => ser = Some(SeriesDef::default()),
                    "f" if !tag.empty => {
                        let f = r.text_until_end("f").0;
                        let within = |n: &str| stack.iter().any(|s| s == n);
                        if let Some(s) = ser.as_mut() {
                            if within("tx") {
                                s.name.0 = Some(f);
                            } else if within("cat") || within("xVal") {
                                s.cat.0 = Some(f);
                            } else if within("val") || within("yVal") {
                                s.val.0 = Some(f);
                            }
                        } else if within("title") && stack.len() <= 4 {
                            title_ref = Some(f);
                        }
                        continue;
                    }
                    "strCache" | "numCache" | "strLit" | "numLit" if !tag.empty => {
                        let within = |n: &str| stack.iter().any(|s| s == n);
                        let points = cache_points(&mut r, name);
                        if let Some(s) = ser.as_mut() {
                            if within("tx") {
                                s.name.1 = points.into_iter().map(|p| p.1).collect();
                            } else if within("cat") || within("xVal") {
                                s.cat.1 = dense(points);
                            } else if within("val") || within("yVal") {
                                s.val.1 = dense(points)
                                    .into_iter()
                                    .map(|v| v.trim().parse().ok())
                                    .collect();
                            }
                        } else if within("title")
                            && title.is_empty()
                            && !stack.iter().any(|s| s.ends_with("Ax"))
                        {
                            title = points.into_iter().map(|p| p.1).collect();
                        }
                        continue;
                    }
                    "v" if !tag.empty && parent == "tx" => {
                        let v = r.text_until_end("v").0;
                        if let Some(s) = ser.as_mut() {
                            s.name.1 = v;
                        }
                        continue;
                    }
                    "t" if !tag.empty => {
                        let v = r.text_until_end("t").0;
                        let in_axis = stack.iter().any(|s| s.ends_with("Ax"));
                        // The chart's own title, or an axis's.
                        if ser.is_none() && stack.iter().any(|s| s == "title") {
                            if in_axis {
                                axis_title.push_str(&v);
                            } else {
                                title.push_str(&v);
                            }
                        }
                        continue;
                    }
                    "axPos" if parent.ends_with("Ax") => {
                        axis_pos = tag.attr("val").map(|v| v.into_owned()).unwrap_or_default();
                    }
                    "srgbClr" | "schemeClr" => {
                        let in_fill = stack.iter().any(|s| s == "spPr")
                            && !stack.iter().any(|s| s == "dPt" || s == "marker");
                        if let Some(s) = ser.as_mut()
                            && s.color.is_none()
                            && in_fill
                        {
                            let v = tag.attr("val").unwrap_or_default();
                            s.color = if name == "srgbClr" {
                                u32::from_str_radix(&v, 16).ok()
                            } else {
                                scheme(&v, theme)
                            };
                        }
                    }
                    _ => {}
                }
                if !tag.empty {
                    stack.push(name.to_owned());
                }
            }
            Token::End { name, .. } => {
                stack.pop();
                if name.ends_with("Ax") && stack.last().is_some_and(|s| s == "plotArea") {
                    let t = std::mem::take(&mut axis_title);
                    if !t.trim().is_empty() {
                        let slot = if matches!(axis_pos.as_str(), "l" | "r") {
                            &mut def.vertical_title
                        } else {
                            &mut def.horizontal_title
                        };
                        slot.get_or_insert(t);
                    }
                    axis_pos.clear();
                }
                if name == "ser" && in_plot {
                    if let Some(s) = ser.take() {
                        def.series.push(s);
                    }
                } else if in_plot && name.ends_with("Chart") {
                    in_plot = false;
                }
            }
            Token::Text { .. } => {}
        }
    }
    let _ = title_ref;
    if !title.trim().is_empty() {
        def.title = Some(title);
    }
    def
}

/// A chart part with its title set to `title`, or taken away: the
/// chart's own `<c:title>` and `<c:autoTitleDeleted>` written again as the
/// first children of `<c:chart>`, everything else kept.
pub fn titled(text: &str, title: Option<&str>) -> String {
    const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let mut r = Reader::new(text);
    let mut depth = 0;
    let mut chart_open: Option<(Span<usize>, String)> = None;
    let mut drop: Vec<Span<usize>> = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == "chart" && chart_open.is_none() {
                    chart_open = Some((tag.span.clone(), xml::prefix(tag.qname).to_owned()));
                } else if depth == 2
                    && chart_open.is_some()
                    && matches!(tag.name, "title" | "autoTitleDeleted")
                {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    drop.push(tag.span.start..end);
                    continue;
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { name, .. } => {
                depth -= 1;
                if depth == 1 && name == "chart" {
                    break;
                }
            }
            Token::Text { .. } => {}
        }
    }
    let Some((open, p)) = chart_open else {
        return text.to_owned();
    };
    // The DrawingML prefix the part declares, or one declared here.
    let a = text[..open.start].split("xmlns:").skip(1).find_map(|d| {
        let (pfx, rest) = d.split_once("=\"")?;
        (rest.split('"').next()? == DRAWING).then(|| pfx.to_owned())
    });
    let (a, decl) = match a {
        Some(a) => (a, String::new()),
        None => ("a".to_owned(), format!(" xmlns:a=\"{DRAWING}\"")),
    };
    let new = match title {
        Some(t) => format!(
            "<{p}title><{p}tx><{p}rich{decl}><{a}:bodyPr/><{a}:lstStyle/><{a}:p><{a}:r><{a}:t>{}</{a}:t></{a}:r></{a}:p></{p}rich></{p}tx><{p}overlay val=\"0\"/></{p}title><{p}autoTitleDeleted val=\"0\"/>",
            xml::escape(t)
        ),
        None => format!("<{p}autoTitleDeleted val=\"1\"/>"),
    };
    let mut out = String::with_capacity(text.len() + new.len());
    out.push_str(&text[..open.end]);
    out.push_str(&new);
    let mut at = open.end;
    for d in drop {
        out.push_str(&text[at..d.start]);
        at = d.end;
    }
    out.push_str(&text[at..]);
    out
}

/// A chart part with the title of its horizontal (`vertical` false) or
/// vertical axis set, or taken away; `None` when the chart has no such
/// axis (a pie). The axis's `<c:title>` goes where the schema puts it,
/// after its position and gridlines.
pub fn axis_titled(text: &str, vertical: bool, title: Option<&str>) -> Option<String> {
    const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    // The axis found: its prefix, its title's bytes, where a title goes.
    let mut found: Option<(String, Option<Span<usize>>, usize)> = None;
    let mut cur: Option<(String, Option<Span<usize>>, usize, bool)> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                let parent = stack.last().map(String::as_str).unwrap_or("");
                if parent == "plotArea" && tag.name.ends_with("Ax") && !tag.empty {
                    cur = Some((xml::prefix(tag.qname).to_owned(), None, tag.span.end, false));
                } else if parent.ends_with("Ax")
                    && let Some(c) = cur.as_mut()
                {
                    match tag.name {
                        "axPos" => {
                            c.3 = matches!(tag.attr("val").as_deref(), Some("l" | "r")) == vertical;
                        }
                        "title" => {
                            let end = if tag.empty {
                                tag.span.end
                            } else {
                                r.skip_element()
                            };
                            c.1 = Some(tag.span.start..end);
                            continue;
                        }
                        _ => {}
                    }
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    if matches!(
                        tag.name,
                        "axId"
                            | "scaling"
                            | "delete"
                            | "axPos"
                            | "majorGridlines"
                            | "minorGridlines"
                    ) {
                        c.2 = end;
                    }
                    continue;
                }
                if !tag.empty {
                    stack.push(tag.name.to_owned());
                }
            }
            Token::End { name, .. } => {
                stack.pop();
                if name.ends_with("Ax")
                    && let Some(c) = cur.take()
                    && c.3
                    && found.is_none()
                {
                    found = Some((c.0, c.1, c.2));
                }
            }
            Token::Text { .. } => {}
        }
    }
    let (p, old, at) = found?;
    let a = text.split("xmlns:").skip(1).find_map(|d| {
        let (pfx, rest) = d.split_once("=\"")?;
        (rest.split('"').next()? == DRAWING).then(|| pfx.to_owned())
    });
    let (a, decl) = match a {
        Some(a) => (a, String::new()),
        None => ("a".to_owned(), format!(" xmlns:a=\"{DRAWING}\"")),
    };
    let rot = if vertical {
        " rot=\"-5400000\" vert=\"horz\""
    } else {
        ""
    };
    let new = title.map_or(String::new(), |t| {
        format!(
            "<{p}title><{p}tx><{p}rich{decl}><{a}:bodyPr{rot}/><{a}:lstStyle/><{a}:p><{a}:r><{a}:t>{}</{a}:t></{a}:r></{a}:p></{p}rich></{p}tx><{p}overlay val=\"0\"/></{p}title>",
            xml::escape(t)
        )
    });
    // The old title sits after `at` when there is one: take it out, then
    // put the new one in its place.
    let mut out = text.to_owned();
    match old {
        Some(o) => out.replace_range(o, &new),
        None => out.insert_str(at, &new),
    }
    Some(out)
}

/// An absolute reference to a range of a sheet, as charts write them.
pub fn reference(sheet: &str, r: crate::cellref::Range) -> String {
    let abs = |c: crate::cellref::CellRef| {
        format!("${}${}", crate::cellref::column_name(c.col), c.row + 1)
    };
    let q = crate::formula::quote_sheet(sheet);
    if r.start == r.end {
        format!("{q}!{}", abs(r.start))
    } else {
        format!("{q}!{}:{}", abs(r.start), abs(r.end))
    }
}

/// A new series: its name's cell and text, its categories' or x values'
/// cells and texts, its values' cells and numbers.
#[derive(Debug, Clone, Default)]
pub struct NewSeries {
    /// The name's cell and text.
    pub name: Option<(String, String)>,
    /// The categories' cells and texts, or x values' as numbers.
    pub cat: Option<(String, Vec<String>, bool)>,
    /// The values' cells and numbers.
    pub val: (String, Vec<Option<f64>>),
}

fn str_cache(f: &str, items: &[String]) -> String {
    let pts: String = items
        .iter()
        .enumerate()
        .map(|(i, v)| format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", xml::escape(v)))
        .collect();
    format!(
        "<c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val=\"{}\"/>{pts}</c:strCache></c:strRef>",
        xml::escape(f),
        items.len()
    )
}

fn num_cache(f: &str, items: &[Option<f64>]) -> String {
    let pts: String = items
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| format!("<c:pt idx=\"{i}\"><c:v>{v}</c:v></c:pt>")))
        .collect();
    format!(
        "<c:numRef><c:f>{}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>{pts}</c:numCache></c:numRef>",
        xml::escape(f),
        items.len()
    )
}

/// A chart part, as Excel writes a new chart of its kind.
pub fn chart_xml(kind: ChartKind, title: Option<&str>, series: &[NewSeries]) -> String {
    let (cat_ax, val_ax) = (500_000_001u32, 500_000_002u32);
    let title_xml = match title {
        Some(t) => format!(
            "<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>",
            xml::escape(t)
        ),
        None if series.len() == 1 => "<c:autoTitleDeleted val=\"0\"/>".to_owned(),
        None => "<c:autoTitleDeleted val=\"1\"/>".to_owned(),
    };
    let sers: String = series
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut x = format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>");
            if let Some((f, text)) = &s.name {
                x.push_str(&format!("<c:tx>{}</c:tx>", str_cache(f, std::slice::from_ref(text))));
            }
            match kind {
                ChartKind::Column | ChartKind::Bar => x.push_str("<c:invertIfNegative val=\"0\"/>"),
                ChartKind::Line => x.push_str("<c:marker><c:symbol val=\"none\"/></c:marker>"),
                ChartKind::Scatter => x.push_str(
                    "<c:spPr><a:ln w=\"19050\" cap=\"rnd\"><a:noFill/></a:ln></c:spPr><c:marker><c:symbol val=\"circle\"/><c:size val=\"5\"/></c:marker>",
                ),
                _ => {}
            }
            if matches!(kind, ChartKind::Pie | ChartKind::Doughnut) {
                // Each slice its own color, as varyColors asks.
            }
            let (cat_tag, val_tag) = if kind == ChartKind::Scatter {
                ("xVal", "yVal")
            } else {
                ("cat", "val")
            };
            if let Some((f, items, numeric)) = &s.cat {
                let body = if *numeric {
                    let nums: Vec<Option<f64>> = items.iter().map(|v| v.parse().ok()).collect();
                    num_cache(f, &nums)
                } else {
                    str_cache(f, items)
                };
                x.push_str(&format!("<c:{cat_tag}>{body}</c:{cat_tag}>"));
            }
            x.push_str(&format!("<c:{val_tag}>{}</c:{val_tag}>", num_cache(&s.val.0, &s.val.1)));
            match kind {
                ChartKind::Line | ChartKind::Scatter => x.push_str("<c:smooth val=\"0\"/>"),
                _ => {}
            }
            x.push_str("</c:ser>");
            x
        })
        .collect();
    let axes_ids = format!("<c:axId val=\"{cat_ax}\"/><c:axId val=\"{val_ax}\"/>");
    let plot = match kind {
        ChartKind::Bar | ChartKind::Column => format!(
            "<c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>{sers}<c:gapWidth val=\"182\"/>{axes_ids}</c:barChart>",
            if kind == ChartKind::Bar { "bar" } else { "col" }
        ),
        ChartKind::Line | ChartKind::Other => format!(
            "<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>{sers}<c:marker val=\"1\"/>{axes_ids}</c:lineChart>"
        ),
        ChartKind::Area => format!(
            "<c:areaChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>{sers}{axes_ids}</c:areaChart>"
        ),
        ChartKind::Pie => format!(
            "<c:pieChart><c:varyColors val=\"1\"/>{sers}<c:firstSliceAng val=\"0\"/></c:pieChart>"
        ),
        ChartKind::Doughnut => format!(
            "<c:doughnutChart><c:varyColors val=\"1\"/>{sers}<c:firstSliceAng val=\"0\"/><c:holeSize val=\"50\"/></c:doughnutChart>"
        ),
        ChartKind::Scatter => format!(
            "<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{sers}{axes_ids}</c:scatterChart>"
        ),
    };
    let ax_common = "<c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/>";
    let ticks = "<c:majorTickMark val=\"none\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/>";
    let (cat_pos, val_pos) = if kind == ChartKind::Bar {
        ("l", "b")
    } else {
        ("b", "l")
    };
    let axes = match kind {
        ChartKind::Pie | ChartKind::Doughnut => String::new(),
        ChartKind::Scatter => format!(
            "<c:valAx><c:axId val=\"{cat_ax}\"/>{ax_common}<c:axPos val=\"b\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>{ticks}<c:crossAx val=\"{val_ax}\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"midCat\"/></c:valAx><c:valAx><c:axId val=\"{val_ax}\"/>{ax_common}<c:axPos val=\"l\"/><c:majorGridlines/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>{ticks}<c:crossAx val=\"{cat_ax}\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"midCat\"/></c:valAx>"
        ),
        _ => format!(
            "<c:catAx><c:axId val=\"{cat_ax}\"/>{ax_common}<c:axPos val=\"{cat_pos}\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>{ticks}<c:crossAx val=\"{val_ax}\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx><c:valAx><c:axId val=\"{val_ax}\"/>{ax_common}<c:axPos val=\"{val_pos}\"/><c:majorGridlines/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>{ticks}<c:crossAx val=\"{cat_ax}\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"between\"/></c:valAx>"
        ),
    };
    let legend = if series.len() > 1 || matches!(kind, ChartKind::Pie | ChartKind::Doughnut) {
        "<c:legend><c:legendPos val=\"b\"/><c:overlay val=\"0\"/></c:legend>"
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:roundedCorners val=\"0\"/><c:chart>{title_xml}<c:plotArea><c:layout/>{plot}{axes}</c:plotArea>{legend}<c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart></c:chartSpace>"
    )
}

/// A drawing's anchor for a chart over cells `from` to `to` (inclusive).
pub fn anchor_xml(p: &str, from: (u32, u32), to: (u32, u32), id: u32, rid: &str) -> String {
    format!(
        "<{p}twoCellAnchor><{p}from><{p}col>{}</{p}col><{p}colOff>0</{p}colOff><{p}row>{}</{p}row><{p}rowOff>0</{p}rowOff></{p}from><{p}to><{p}col>{}</{p}col><{p}colOff>0</{p}colOff><{p}row>{}</{p}row><{p}rowOff>0</{p}rowOff></{p}to><{p}graphicFrame macro=\"\"><{p}nvGraphicFramePr><{p}cNvPr id=\"{id}\" name=\"Chart {}\"/><{p}cNvGraphicFramePr/></{p}nvGraphicFramePr><{p}xfrm><a:off xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" x=\"0\" y=\"0\"/><a:ext xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" cx=\"0\" cy=\"0\"/></{p}xfrm><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:id=\"{rid}\"/></a:graphicData></a:graphic></{p}graphicFrame><{p}clientData/></{p}twoCellAnchor>",
        from.1,
        from.0,
        to.1 + 1,
        to.0 + 1,
        id.saturating_sub(1)
    )
}

/// An anchor element moved to cover cells `from` to `to` (inclusive): a
/// two-cell anchor, whatever it was, its frame and the rest kept.
pub fn moved_anchor(el: &str, from: (u32, u32), to: (u32, u32)) -> String {
    let mut r = Reader::new(el);
    let mut depth = 0;
    let (mut head, mut prefix, mut attrs) = (0..0, String::new(), String::new());
    let mut drop: Vec<Span<usize>> = Vec::new();
    let mut close = el.len();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 0 {
                    head = tag.span.clone();
                    prefix = xml::prefix(tag.qname).to_owned();
                    if tag.name == "twoCellAnchor" {
                        // `editAs` and the like stay.
                        let open = &el[tag.span.clone()];
                        let name_end = open.find(char::is_whitespace).unwrap_or(open.len());
                        attrs = open[name_end..]
                            .trim_end_matches('>')
                            .trim_end_matches('/')
                            .to_owned();
                    }
                } else if depth == 1 && matches!(tag.name, "from" | "to" | "ext" | "pos") {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    drop.push(tag.span.start..end);
                    continue;
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { span, .. } => {
                depth -= 1;
                if depth == 0 {
                    close = span.start;
                }
            }
            Token::Text { .. } => {}
        }
    }
    let mut body = String::new();
    let mut at = head.end;
    for d in drop {
        body.push_str(&el[at..d.start]);
        at = d.end;
    }
    body.push_str(&el[at..close]);
    let p = &prefix;
    let cell = |name: &str, (row, col): (u32, u32)| {
        format!(
            "<{p}{name}><{p}col>{col}</{p}col><{p}colOff>0</{p}colOff><{p}row>{row}</{p}row><{p}rowOff>0</{p}rowOff></{p}{name}>"
        )
    };
    format!(
        "<{p}twoCellAnchor{attrs}>{}{}{body}</{p}twoCellAnchor>",
        cell("from", from),
        cell("to", (to.0 + 1, to.1 + 1))
    )
}

/// A new drawing part holding one anchor.
pub fn drawing_xml(anchor: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">{anchor}</xdr:wsDr>"
    )
}

/// The largest `cNvPr` id of a drawing.
pub fn max_shape_id(text: &str) -> u32 {
    let mut r = Reader::new(text);
    let mut max = 1;
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "cNvPr"
        {
            max = max.max(tag.attr("id").and_then(|v| v.parse().ok()).unwrap_or(0));
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chart_written_reads_back() {
        let s = NewSeries {
            name: Some(("Budget!$B$1".into(), "Q1".into())),
            cat: Some((
                "Budget!$A$2:$A$3".into(),
                vec!["Rent".into(), "Food".into()],
                false,
            )),
            val: ("Budget!$B$2:$B$3".into(), vec![Some(1200.0), Some(431.5)]),
        };
        for kind in [
            ChartKind::Column,
            ChartKind::Bar,
            ChartKind::Line,
            ChartKind::Area,
            ChartKind::Pie,
            ChartKind::Doughnut,
            ChartKind::Scatter,
        ] {
            let x = chart_xml(kind, Some("Spending"), std::slice::from_ref(&s));
            let d = parse_chart(&x, &[]);
            assert_eq!(d.kind, kind, "{x}");
            assert_eq!(d.title.as_deref(), Some("Spending"));
            assert_eq!(d.series.len(), 1);
            assert_eq!(d.series[0].name, (Some("Budget!$B$1".into()), "Q1".into()));
            assert_eq!(d.series[0].val.1, vec![Some(1200.0), Some(431.5)]);
            assert_eq!(d.series[0].cat.0.as_deref(), Some("Budget!$A$2:$A$3"));
        }
        let anchor = anchor_xml("xdr:", (1, 5), (15, 12), 2, "rId1");
        let a = parse_drawing(&drawing_xml(&anchor));
        assert_eq!(a.len(), 1);
        assert_eq!(
            (a[0].from, a[0].to, a[0].rid.as_str()),
            ((1, 5), (15, 12), "rId1")
        );
    }

    #[test]
    fn titles_set_and_removed() {
        let s = NewSeries {
            name: Some(("S!$B$1".into(), "Q1".into())),
            cat: None,
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
        };
        let x = chart_xml(ChartKind::Line, Some("Old"), std::slice::from_ref(&s));
        let y = titled(&x, Some("New & better"));
        let d = parse_chart(&y, &[]);
        assert_eq!(d.title.as_deref(), Some("New & better"));
        assert!(!d.title_deleted);
        assert_eq!(y.matches("autoTitleDeleted").count(), 1);
        let z = titled(&y, None);
        let d = parse_chart(&z, &[]);
        assert_eq!((d.title, d.title_deleted), (None, true));
        assert_eq!(d.series.len(), 1, "the rest kept");
        // A part without the DrawingML prefix declares it.
        let bare = r#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea/></c:chart></c:chartSpace>"#;
        assert!(titled(bare, Some("T")).contains("<c:rich xmlns:a="));
    }

    #[test]
    fn axis_titles_set_and_removed() {
        let s = NewSeries {
            name: None,
            cat: Some(("S!$A$2:$A$3".into(), vec!["a".into(), "b".into()], false)),
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
        };
        for kind in [ChartKind::Column, ChartKind::Bar, ChartKind::Scatter] {
            let x = chart_xml(kind, None, std::slice::from_ref(&s));
            let y = axis_titled(&x, false, Some("Month")).unwrap();
            let y = axis_titled(&y, true, Some("Sales")).unwrap();
            let d = parse_chart(&y, &[]);
            assert_eq!(d.horizontal_title.as_deref(), Some("Month"), "{kind:?}");
            assert_eq!(d.vertical_title.as_deref(), Some("Sales"), "{kind:?}");
            assert_eq!(d.title, None, "not the chart's own title");
            // Set again: replaced, not added.
            let z = axis_titled(&y, true, Some("Revenue")).unwrap();
            assert_eq!(z.matches("<c:title>").count(), 2);
            assert_eq!(
                parse_chart(&z, &[]).vertical_title.as_deref(),
                Some("Revenue")
            );
            let z = axis_titled(&z, false, None).unwrap();
            let d = parse_chart(&z, &[]);
            assert_eq!(
                (d.horizontal_title, d.vertical_title.as_deref()),
                (None, Some("Revenue"))
            );
            // The title after the axis's position and gridlines.
            let ax = &y[y.find("<c:valAx>").unwrap()..];
            let ax = &ax[..ax.find("</c:valAx>").unwrap()];
            if ax.contains("<c:title>") {
                assert!(ax.find("<c:title>") > ax.find("<c:axPos"), "{ax}");
                assert!(ax.find("<c:title>") < ax.find("<c:numFmt"), "{ax}");
            }
        }
        let pie = chart_xml(ChartKind::Pie, None, std::slice::from_ref(&s));
        assert!(axis_titled(&pie, false, Some("x")).is_none());
    }

    #[test]
    fn colors_and_titles_of_excel_charts() {
        let x = r#"<c:chartSpace xmlns:c="c" xmlns:a="a"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Sales </a:t></a:r><a:r><a:t>2026</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:barDir val="col"/><c:grouping val="stacked"/><c:ser><c:idx val="0"/><c:tx><c:v>Direct</c:v></c:tx><c:spPr><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></c:spPr><c:val><c:numLit><c:ptCount val="2"/><c:pt idx="1"><c:v>4</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx><c:title><c:tx><c:rich><a:p><a:r><a:t>Month</a:t></a:r></a:p></c:rich></c:tx></c:title></c:catAx></c:plotArea></c:chart></c:chartSpace>"#;
        let d = parse_chart(x, &[]);
        assert_eq!(d.title.as_deref(), Some("Sales 2026"));
        assert!(d.stacked);
        assert_eq!(d.series[0].name.1, "Direct");
        assert_eq!(d.series[0].color, Some(0xED7D31));
        assert_eq!(d.series[0].val.1, vec![None, Some(4.0)]);
    }
}
