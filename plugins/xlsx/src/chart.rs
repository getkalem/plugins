//! Charts (ECMA-376 part 1, 21.2 and 20.5): where a sheet's drawing puts
//! them, what their parts say, and new ones written as Excel writes them.

use std::ops::Range as Span;

use kalem_viewer::ChartKind;

use crate::styles::Rgb;
use crate::xml::{self, Reader, Token};

/// A value axis's scale: min, max, major unit, logarithmic.
pub type Scale = (Option<f64>, Option<f64>, Option<f64>, bool);

/// How the chart area's background or border is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fill {
    /// As the style has it.
    #[default]
    Auto,
    /// Not at all.
    None,
    /// In a color.
    Color(Rgb),
}

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
    /// Points with colors of their own: point, color.
    pub points: Vec<(usize, Rgb)>,
    /// How far every slice stands out, in percent of the radius.
    pub explosion: u32,
    /// Slices standing out on their own: point, percent.
    pub point_explosions: Vec<(usize, u32)>,
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
    /// Its legend's position (`b`, `t`, `l`, `r`, `tr`); `None` for none.
    pub legend: Option<String>,
    /// Its data labels: value, category, series, percent.
    pub labels: (bool, bool, bool, bool),
    /// Its value axis's scale: min, max, major unit, logarithmic.
    pub scale: Scale,
    /// The chart area's background.
    pub background: Fill,
    /// The chart area's border.
    pub border: Fill,
    /// The plot area's background.
    pub plot_background: Fill,
    /// The plot area's border.
    pub plot_border: Fill,
    /// Gridlines: horizontal major and minor, vertical major and minor.
    pub gridlines: (bool, bool, bool, bool),
    /// The value axis's own number format, when not the cells'.
    pub axis_format: Option<String>,
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
    let mut axis_grid = (false, false);
    let mut point: Option<(Option<usize>, Option<Rgb>)> = None;
    let mut point_explosion: Option<u32> = None;
    let mut axis_scale: Scale = (None, None, None, false);
    // Each value axis: its side and scale.
    let mut value_axes: Vec<(String, Scale, Option<String>)> = Vec::new();
    let mut axis_format: Option<String> = None;
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
                    "showVal" | "showCatName" | "showSerName" | "showPercent"
                        if parent == "dLbls" && in_plot =>
                    {
                        if matches!(tag.attr("val").as_deref(), Some("1" | "true") | None) {
                            match name {
                                "showVal" => def.labels.0 = true,
                                "showCatName" => def.labels.1 = true,
                                "showSerName" => def.labels.2 = true,
                                _ => def.labels.3 = true,
                            }
                        }
                    }
                    "legend" if parent == "chart" => def.legend = Some("r".into()),
                    "legendPos" if parent == "legend" => {
                        def.legend = Some(tag.attr("val").map_or("r".into(), |v| v.into_owned()));
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
                    "min" | "max" | "logBase" if parent == "scaling" => {
                        let v = tag.attr("val").and_then(|v| v.trim().parse::<f64>().ok());
                        match name {
                            "min" => axis_scale.0 = v,
                            "max" => axis_scale.1 = v,
                            _ => axis_scale.3 = v.is_some(),
                        }
                    }
                    "numFmt" if parent.ends_with("Ax") => {
                        let linked = tag
                            .attr("sourceLinked")
                            .is_some_and(|v| v == "1" || v == "true");
                        let code = tag
                            .attr("formatCode")
                            .map(|v| v.into_owned())
                            .unwrap_or_default();
                        axis_format =
                            (!linked && !code.is_empty() && code != "General").then_some(code);
                    }
                    "majorUnit" if parent.ends_with("Ax") => {
                        axis_scale.2 = tag.attr("val").and_then(|v| v.trim().parse::<f64>().ok());
                    }
                    "axPos" if parent.ends_with("Ax") => {
                        axis_pos = tag.attr("val").map(|v| v.into_owned()).unwrap_or_default();
                    }
                    "majorGridlines" if parent.ends_with("Ax") => axis_grid.0 = true,
                    "minorGridlines" if parent.ends_with("Ax") => axis_grid.1 = true,
                    // The chart area's own fill and line.
                    "noFill" | "solidFill" | "srgbClr" | "schemeClr"
                        if (stack.len() >= 2 && stack[1] == "spPr" && stack[0] == "chartSpace")
                            || (stack.len() >= 4
                                && stack[2] == "plotArea"
                                && stack[3] == "spPr") =>
                    {
                        let in_line = stack.iter().any(|s| s == "ln");
                        let plot = stack.get(2).is_some_and(|s| s == "plotArea");
                        let slot = match (plot, in_line) {
                            (false, false) => &mut def.background,
                            (false, true) => &mut def.border,
                            (true, false) => &mut def.plot_background,
                            (true, true) => &mut def.plot_border,
                        };
                        match name {
                            "noFill" if parent == "spPr" || parent == "ln" => *slot = Fill::None,
                            "srgbClr" | "schemeClr" if parent == "solidFill" => {
                                let v = tag.attr("val").unwrap_or_default();
                                let c = if name == "srgbClr" {
                                    u32::from_str_radix(&v, 16).ok()
                                } else {
                                    scheme(&v, theme)
                                };
                                if let Some(c) = c {
                                    *slot = Fill::Color(c);
                                }
                            }
                            _ => {}
                        }
                    }
                    "dPt" if ser.is_some() && !tag.empty => point = Some((None, None)),
                    "explosion" if parent == "ser" || parent == "dPt" => {
                        let v = tag
                            .attr("val")
                            .and_then(|v| v.parse::<u32>().ok())
                            .unwrap_or(0);
                        if parent == "ser" {
                            if let Some(s) = ser.as_mut() {
                                s.explosion = v;
                            }
                        } else {
                            point_explosion = Some(v);
                        }
                    }
                    "idx" if parent == "dPt" => {
                        if let Some(pt) = point.as_mut() {
                            pt.0 = tag.attr("val").and_then(|v| v.parse::<usize>().ok());
                        }
                    }
                    "srgbClr" | "schemeClr"
                        if point.as_ref().is_some_and(|p| p.1.is_none())
                            && stack.iter().any(|s| s == "spPr")
                            && !stack.iter().any(|s| s == "ln") =>
                    {
                        let v = tag.attr("val").unwrap_or_default();
                        if let Some(pt) = point.as_mut() {
                            pt.1 = if name == "srgbClr" {
                                u32::from_str_radix(&v, 16).ok()
                            } else {
                                scheme(&v, theme)
                            };
                        }
                    }
                    "srgbClr" | "schemeClr" => {
                        // A scatter chart's points take the marker's fill.
                        let in_fill = if def.kind == ChartKind::Scatter {
                            stack.iter().any(|s| s == "marker") && stack.iter().any(|s| s == "spPr")
                        } else {
                            stack.iter().any(|s| s == "spPr")
                                && !stack.iter().any(|s| s == "dPt" || s == "marker")
                        };
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
                if name == "dPt" {
                    let pt = point.take();
                    let ex = point_explosion.take();
                    if let (Some((Some(i), c)), Some(s)) = (pt, ser.as_mut()) {
                        if let Some(c) = c {
                            s.points.push((i, c));
                        }
                        if let Some(e) = ex.filter(|e| *e > 0) {
                            s.point_explosions.push((i, e));
                        }
                    }
                }
                if name.ends_with("Ax") && stack.last().is_some_and(|s| s == "plotArea") {
                    // An axis's gridlines run across it.
                    let (major, minor) = std::mem::take(&mut axis_grid);
                    if name != "serAx" {
                        if matches!(axis_pos.as_str(), "l" | "r") {
                            def.gridlines.0 |= major;
                            def.gridlines.1 |= minor;
                        } else {
                            def.gridlines.2 |= major;
                            def.gridlines.3 |= minor;
                        }
                    }
                    let sc = std::mem::take(&mut axis_scale);
                    if name == "valAx" {
                        value_axes.push((axis_pos.clone(), sc, axis_format.take()));
                    } else {
                        axis_format = None;
                    }
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
    // The value axis: a scatter chart's vertical one, else the only one.
    let pick = if def.kind == ChartKind::Scatter {
        value_axes
            .iter()
            .find(|a| matches!(a.0.as_str(), "l" | "r"))
    } else {
        value_axes.first()
    };
    if let Some(a) = pick {
        def.scale = a.1;
        def.axis_format = a.2.clone();
    }
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

/// A chart part with its legend at `pos` (`b`, `t`, `l`, `r`, `tr`), or
/// without one: a legend there keeps its entries and formatting and only
/// moves; a new one goes after the plot area, where the schema puts it.
pub fn with_legend(text: &str, pos: Option<&str>) -> String {
    let mut r = Reader::new(text);
    let mut depth = 0;
    let mut in_chart = false;
    let mut prefix = String::new();
    let (mut legend, mut legend_pos, mut legend_inner) = (None, None, None);
    let mut after_plot = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == "chart" {
                    in_chart = true;
                    prefix = xml::prefix(tag.qname).to_owned();
                } else if in_chart && depth == 2 && tag.name == "plotArea" {
                    after_plot = Some(if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    });
                    continue;
                } else if in_chart && depth == 2 && tag.name == "legend" {
                    let start = tag.span.start;
                    if tag.empty {
                        legend = Some(start..tag.span.end);
                        continue;
                    }
                    legend_inner = Some(tag.span.end);
                    // Its legendPos, if any.
                    let mut d = 0;
                    let mut end = text.len();
                    while let Some(t2) = r.next_token() {
                        match t2 {
                            Token::Start(t3) => {
                                if d == 0 && t3.name == "legendPos" {
                                    let e = if t3.empty {
                                        t3.span.end
                                    } else {
                                        r.skip_element()
                                    };
                                    legend_pos = Some(t3.span.start..e);
                                    continue;
                                }
                                if !t3.empty {
                                    d += 1;
                                }
                            }
                            Token::End { span, .. } => {
                                if d == 0 {
                                    end = span.end;
                                    break;
                                }
                                d -= 1;
                            }
                            Token::Text { .. } => {}
                        }
                    }
                    legend = Some(start..end);
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
    let p = &prefix;
    let mut out = text.to_owned();
    match (pos, legend) {
        (None, Some(l)) => out.replace_range(l, ""),
        (None, None) => {}
        (Some(v), Some(l)) => match (legend_pos, legend_inner) {
            (Some(lp), _) => out.replace_range(lp, &format!("<{p}legendPos val=\"{v}\"/>")),
            (None, Some(inner)) => out.insert_str(inner, &format!("<{p}legendPos val=\"{v}\"/>")),
            (None, None) => out.replace_range(
                l,
                &format!(
                    "<{p}legend><{p}legendPos val=\"{v}\"/><{p}overlay val=\"0\"/></{p}legend>"
                ),
            ),
        },
        (Some(v), None) => {
            if let Some(at) = after_plot {
                out.insert_str(
                    at,
                    &format!(
                        "<{p}legend><{p}legendPos val=\"{v}\"/><{p}overlay val=\"0\"/></{p}legend>"
                    ),
                );
            }
        }
    }
    out
}

/// A chart part whose series all show `labels` (value, category, series,
/// percent; the percent only for a pie or doughnut), or no labels when
/// all are off: each series' `<c:dLbls>` written again where the schema
/// puts it, the plot's own one taken away.
pub fn with_labels(text: &str, labels: (bool, bool, bool, bool), pie: bool) -> String {
    const AFTER: [&str; 10] = [
        "trendline",
        "errBars",
        "cat",
        "val",
        "xVal",
        "yVal",
        "smooth",
        "shape",
        "bubbleSize",
        "extLst",
    ];
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    let mut edits: Vec<(Span<usize>, String)> = Vec::new();
    let any = labels.0 || labels.1 || labels.2 || (labels.3 && pie);
    let b = |v: bool| u8::from(v);
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                let parent = stack.last().map(String::as_str).unwrap_or("");
                if tag.name == "dLbls" && parent.ends_with("Chart") {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    edits.push((tag.span.start..end, String::new()));
                    continue;
                }
                if tag.name == "ser" && parent.ends_with("Chart") && !tag.empty {
                    let p = xml::prefix(tag.qname).to_owned();
                    let mut at: Option<usize> = None;
                    let mut close = text.len();
                    while let Some(t2) = r.next_token() {
                        match t2 {
                            Token::Start(c) => {
                                let end = if c.empty {
                                    c.span.end
                                } else {
                                    r.skip_element()
                                };
                                if c.name == "dLbls" {
                                    edits.push((c.span.start..end, String::new()));
                                } else if at.is_none() && AFTER.contains(&c.name) {
                                    at = Some(c.span.start);
                                }
                            }
                            Token::End { span, .. } => {
                                close = span.start;
                                break;
                            }
                            Token::Text { .. } => {}
                        }
                    }
                    if any {
                        let at = at.unwrap_or(close);
                        edits.push((
                            at..at,
                            format!(
                                "<{p}dLbls><{p}showLegendKey val=\"0\"/><{p}showVal val=\"{}\"/><{p}showCatName val=\"{}\"/><{p}showSerName val=\"{}\"/><{p}showPercent val=\"{}\"/><{p}showBubbleSize val=\"0\"/></{p}dLbls>",
                                b(labels.0),
                                b(labels.1),
                                b(labels.2),
                                b(labels.3 && pie)
                            ),
                        ));
                    }
                    continue;
                }
                if !tag.empty {
                    stack.push(tag.name.to_owned());
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    edits.sort_by_key(|e| e.0.start);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (span, new) in edits {
        out.push_str(&text[at..span.start]);
        out.push_str(&new);
        at = span.end;
    }
    out.push_str(&text[at..]);
    out
}

/// A chart part with its value axis's scale set (min, max, major unit,
/// logarithmic; `None` leaves the choice to the spreadsheet): the axis's
/// `<c:scaling>` written again in the schema's order and its
/// `<c:majorUnit>` after `<c:crossBetween>`. `None` when the chart has no
/// value axis (a pie).
pub fn with_scale(text: &str, scale: Scale, scatter: bool) -> Option<String> {
    // The value axes in the plot: their bytes and side.
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    let mut axes: Vec<(Span<usize>, String, String)> = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if tag.name == "valAx"
                    && stack.last().is_some_and(|s| s == "plotArea")
                    && !tag.empty
                {
                    let start = tag.span.start;
                    let prefix = xml::prefix(tag.qname).to_owned();
                    let end = r.skip_element();
                    let el = &text[start..end];
                    let pos = el
                        .split("axPos val=\"")
                        .nth(1)
                        .and_then(|v| v.split('"').next())
                        .unwrap_or("l")
                        .to_owned();
                    axes.push((start..end, pos, prefix));
                    continue;
                }
                if !tag.empty {
                    stack.push(tag.name.to_owned());
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    let (span, _, p) = if scatter {
        axes.into_iter()
            .find(|a| matches!(a.1.as_str(), "l" | "r"))?
    } else {
        axes.into_iter().next()?
    };
    let el = &text[span.clone()];
    // The axis's children, by name and bytes.
    let mut r = Reader::new(el);
    let mut depth = 0;
    let mut kids: Vec<(String, Span<usize>)> = Vec::new();
    let (mut open_end, mut close) = (0, el.len());
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 0 {
                    open_end = tag.span.end;
                    depth += 1;
                    continue;
                }
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                kids.push((tag.name.to_owned(), tag.span.start..end));
            }
            Token::End { span, .. } => {
                close = span.start;
                break;
            }
            Token::Text { .. } => {}
        }
    }
    let num = |v: f64| format!("{v}");
    // The scaling's orientation and extensions kept.
    let old_scaling = kids
        .iter()
        .find(|k| k.0 == "scaling")
        .map(|k| &el[k.1.clone()])
        .unwrap_or("");
    let keep = |name: &str| -> String {
        let mut r = Reader::new(old_scaling);
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == name
            {
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                return old_scaling[tag.span.start..end].to_owned();
            }
        }
        String::new()
    };
    let orientation = Some(keep("orientation"))
        .filter(|o| !o.is_empty())
        .unwrap_or_else(|| format!("<{p}orientation val=\"minMax\"/>"));
    let mut scaling = format!("<{p}scaling>");
    if scale.3 {
        scaling.push_str(&format!("<{p}logBase val=\"10\"/>"));
    }
    scaling.push_str(&orientation);
    if let Some(v) = scale.1 {
        scaling.push_str(&format!("<{p}max val=\"{}\"/>", num(v)));
    }
    if let Some(v) = scale.0 {
        scaling.push_str(&format!("<{p}min val=\"{}\"/>", num(v)));
    }
    scaling.push_str(&keep("extLst"));
    scaling.push_str(&format!("</{p}scaling>"));
    let major = scale
        .2
        .map(|v| format!("<{p}majorUnit val=\"{}\"/>", num(v)));
    let mut out = String::from(&el[..open_end]);
    let mut placed = major.is_none();
    let mut scaled = false;
    for (name, sp) in &kids {
        match name.as_str() {
            "scaling" => {
                out.push_str(&scaling);
                scaled = true;
                continue;
            }
            "majorUnit" => continue,
            "minorUnit" | "dispUnits" | "extLst" if !placed => {
                out.push_str(major.as_deref().unwrap_or(""));
                placed = true;
            }
            _ => {}
        }
        if !scaled && name == "delete" {
            out.push_str(&scaling);
            scaled = true;
        }
        out.push_str(&el[sp.clone()]);
        if !placed && name == "crossBetween" {
            out.push_str(major.as_deref().unwrap_or(""));
            placed = true;
        }
    }
    if !placed {
        out.push_str(major.as_deref().unwrap_or(""));
    }
    out.push_str(&el[close..]);
    let mut whole = text.to_owned();
    whole.replace_range(span, &out);
    Some(whole)
}

/// The fills of DrawingML, any one of which an element has.
const FILLS: [&str; 6] = [
    "noFill",
    "solidFill",
    "gradFill",
    "blipFill",
    "pattFill",
    "grpFill",
];

/// An element's direct children (name and bytes), where its content
/// starts and where its end tag starts; an empty element is opened up.
fn open_up(el: &str) -> (String, usize, usize, Vec<(String, Span<usize>)>) {
    let mut r = Reader::new(el);
    let mut out = el.to_owned();
    let (mut open_end, mut close) = (0, el.len());
    let mut kids = Vec::new();
    let mut depth = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 0 {
                    if tag.empty {
                        // `<x a="1"/>` → `<x a="1"></x>`.
                        let head = el[..tag.span.end].trim_end_matches("/>").trim_end();
                        out = format!("{head}></{}>", tag.qname);
                        let open_end = head.len() + 1;
                        return (out, open_end, open_end, Vec::new());
                    }
                    open_end = tag.span.end;
                    depth = 1;
                    continue;
                }
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                kids.push((tag.name.to_owned(), tag.span.start..end));
            }
            Token::End { span, .. } => {
                close = span.start;
                break;
            }
            Token::Text { .. } => {}
        }
    }
    (out, open_end, close, kids)
}

/// An element with its direct child `name` replaced by `new` (or taken
/// out when `new` is empty), or `new` put after the last of `after` there
/// is, else first.
fn set_child(el: &str, names: &[&str], new: &str, after: &[&str]) -> String {
    let (el, open_end, close, kids) = open_up(el);
    let mut out = String::from(&el[..open_end]);
    let mut placed = new.is_empty();
    let last_after = kids.iter().rposition(|k| after.contains(&k.0.as_str()));
    if !placed && last_after.is_none() && !kids.iter().any(|k| names.contains(&k.0.as_str())) {
        out.push_str(new);
        placed = true;
    }
    for (i, (name, sp)) in kids.iter().enumerate() {
        if names.contains(&name.as_str()) {
            if !placed {
                out.push_str(new);
                placed = true;
            }
            continue;
        }
        out.push_str(&el[sp.clone()]);
        if !placed && Some(i) == last_after {
            out.push_str(new);
            placed = true;
        }
    }
    if !placed {
        out.push_str(new);
    }
    out.push_str(&el[close..]);
    out
}

/// The direct child `name` of an element, if any.
fn child<'a>(el: &'a str, name: &str) -> Option<&'a str> {
    let (_, _, _, kids) = open_up(el);
    kids.into_iter().find(|k| k.0 == name).map(|k| &el[k.1])
}

/// A chart part with series `series` of its plot given `color`, or the
/// theme's again (`None`): a column, bar or area series' fill, a line's
/// stroke, a scatter chart's points; the rest of its formatting kept.
/// `None` when there is no such series.
pub fn with_series_color(
    text: &str,
    series: usize,
    color: Option<Rgb>,
    kind: ChartKind,
) -> Option<String> {
    const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    let mut found = None;
    let mut n = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                let parent = stack.last().map(String::as_str).unwrap_or("");
                if tag.name == "ser" && parent.ends_with("Chart") {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    if n == series {
                        found = Some((tag.span.start..end, xml::prefix(tag.qname).to_owned()));
                        break;
                    }
                    n += 1;
                    continue;
                }
                if !tag.empty {
                    stack.push(tag.name.to_owned());
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    let (span, p) = found?;
    let a = text.split("xmlns:").skip(1).find_map(|d| {
        let (pfx, rest) = d.split_once("=\"")?;
        (rest.split('"').next()? == DRAWING).then(|| pfx.to_owned())
    });
    let (a, decl) = match a {
        Some(a) => (a, String::new()),
        None => ("a".to_owned(), format!(" xmlns:a=\"{DRAWING}\"")),
    };
    let fill = color.map_or(String::new(), |c| {
        format!("<{a}:solidFill{decl}><{a}:srgbClr val=\"{c:06X}\"/></{a}:solidFill>")
    });
    let geometry = ["xfrm", "custGeom", "prstGeom"];
    let ser = &text[span.clone()];
    let head = ["idx", "order", "tx"];
    let new_ser = match kind {
        ChartKind::Line => match child(ser, "spPr") {
            Some(sp) => {
                let ln = match child(sp, "ln") {
                    Some(ln) => set_child(ln, &FILLS, &fill, &[]),
                    None if fill.is_empty() => String::new(),
                    None => format!("<{a}:ln{decl} w=\"28575\" cap=\"rnd\">{fill}</{a}:ln>"),
                };
                let mut after = geometry.to_vec();
                after.extend(FILLS);
                let sp = if ln.is_empty() {
                    sp.to_owned()
                } else {
                    set_child(sp, &["ln"], &ln, &after)
                };
                set_child(ser, &["spPr"], &sp, &head)
            }
            None if fill.is_empty() => ser.to_owned(),
            None => set_child(
                ser,
                &["spPr"],
                &format!(
                    "<{p}spPr><{a}:ln{decl} w=\"28575\" cap=\"rnd\">{fill}</{a}:ln></{p}spPr>"
                ),
                &head,
            ),
        },
        ChartKind::Scatter => {
            let marker = match child(ser, "marker") {
                Some(m) => {
                    let sp = match child(m, "spPr") {
                        Some(sp) => set_child(sp, &FILLS, &fill, &geometry),
                        None => format!("<{p}spPr>{fill}</{p}spPr>"),
                    };
                    set_child(m, &["spPr"], &sp, &["symbol", "size"])
                }
                None if fill.is_empty() => String::new(),
                None => format!(
                    "<{p}marker><{p}symbol val=\"circle\"/><{p}size val=\"5\"/><{p}spPr>{fill}</{p}spPr></{p}marker>"
                ),
            };
            if marker.is_empty() {
                ser.to_owned()
            } else {
                set_child(ser, &["marker"], &marker, &["idx", "order", "tx", "spPr"])
            }
        }
        _ => match child(ser, "spPr") {
            Some(sp) => set_child(
                ser,
                &["spPr"],
                &set_child(sp, &FILLS, &fill, &geometry),
                &head,
            ),
            None if fill.is_empty() => ser.to_owned(),
            None => set_child(ser, &["spPr"], &format!("<{p}spPr>{fill}</{p}spPr>"), &head),
        },
    };
    let mut out = text.to_owned();
    out.replace_range(span, &new_ser);
    Some(out)
}

/// Series `series` of a chart part's plot: its bytes and prefix.
fn series_span(text: &str, series: usize) -> Option<(Span<usize>, String)> {
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    let mut n = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                let parent = stack.last().map(String::as_str).unwrap_or("");
                if tag.name == "ser" && parent.ends_with("Chart") {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    if n == series {
                        return Some((tag.span.start..end, xml::prefix(tag.qname).to_owned()));
                    }
                    n += 1;
                    continue;
                }
                if !tag.empty {
                    stack.push(tag.name.to_owned());
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    None
}

/// The DrawingML prefix a part declares, or `a` and its declaration.
fn drawing_prefix(text: &str) -> (String, String) {
    const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let a = text.split("xmlns:").skip(1).find_map(|d| {
        let (pfx, rest) = d.split_once("=\"")?;
        (rest.split('"').next()? == DRAWING).then(|| pfx.to_owned())
    });
    match a {
        Some(a) => (a, String::new()),
        None => ("a".to_owned(), format!(" xmlns:a=\"{DRAWING}\"")),
    }
}

/// Whether a `<c:dPt>` says nothing beyond which point it is: no
/// explosion, marker or formatting.
fn trivial_point(el: &str) -> bool {
    let (_, _, _, kids) = open_up(el);
    kids.iter().all(|(name, sp)| match name.as_str() {
        "idx" | "bubble3D" | "invertIfNegative" => true,
        "spPr" => open_up(&el[sp.clone()]).3.is_empty(),
        _ => false,
    })
}

/// A series with the `<c:dPt>` of `point` made by `f` from the one there
/// (or none), taken away when `f` gives `None`; a new one goes in the
/// order of the points, after the series' own properties.
fn edit_point(
    ser: &str,
    p: &str,
    point: usize,
    f: impl Fn(Option<&str>) -> Option<String>,
) -> String {
    let (ser_text, open_end, close, kids) = open_up(ser);
    let idx_of = |el: &str| -> Option<usize> {
        el.split("idx val=\"")
            .nth(1)?
            .split('"')
            .next()?
            .parse()
            .ok()
    };
    let before_points = [
        "idx",
        "order",
        "tx",
        "spPr",
        "invertIfNegative",
        "pictureOptions",
        "marker",
        "explosion",
    ];
    let mut out = String::from(&ser_text[..open_end]);
    let mut placed = false;
    let put_new = |out: &mut String| {
        if let Some(el) = f(None) {
            out.push_str(&el);
        }
    };
    let _ = p;
    for (name, sp) in &kids {
        let el = &ser_text[sp.clone()];
        if !placed {
            let here = match name.as_str() {
                "dPt" => idx_of(el).is_some_and(|k| k >= point),
                n => !before_points.contains(&n),
            };
            if here {
                placed = true;
                if name == "dPt" && idx_of(el) == Some(point) {
                    if let Some(new) = f(Some(el)) {
                        out.push_str(&new);
                    }
                    continue;
                }
                put_new(&mut out);
            }
        }
        out.push_str(el);
    }
    if !placed {
        put_new(&mut out);
    }
    out.push_str(&ser_text[close..]);
    out
}

/// A chart part with point `point` of series `series` (a pie's slice)
/// given `color`, or its series' again (`None`): the point's `<c:dPt>`
/// written, in the order of the points, or taken away when nothing else
/// is said of the point. `None` when there is no such series.
pub fn with_point_color(
    text: &str,
    series: usize,
    point: usize,
    color: Option<Rgb>,
) -> Option<String> {
    let (span, p) = series_span(text, series)?;
    let (a, decl) = drawing_prefix(text);
    let fill = color.map_or(String::new(), |c| {
        format!("<{a}:solidFill{decl}><{a}:srgbClr val=\"{c:06X}\"/></{a}:solidFill>")
    });
    let after = ["idx", "invertIfNegative", "marker", "bubble3D", "explosion"];
    let new_ser = edit_point(&text[span.clone()], &p, point, |old| {
        let base = old.map_or_else(
            || format!("<{p}dPt><{p}idx val=\"{point}\"/><{p}bubble3D val=\"0\"/></{p}dPt>"),
            str::to_owned,
        );
        let spr = match child(&base, "spPr") {
            Some(spr) => set_child(spr, &FILLS, &fill, &["xfrm", "custGeom", "prstGeom"]),
            None if fill.is_empty() => String::new(),
            None => format!("<{p}spPr>{fill}</{p}spPr>"),
        };
        let el = if spr.is_empty() {
            base
        } else {
            set_child(&base, &["spPr"], &spr, &after)
        };
        (!trivial_point(&el)).then_some(el)
    });
    let mut out = text.to_owned();
    out.replace_range(span, &new_ser);
    Some(out)
}

/// A chart part with a pie's slice (point `point` of series `series`)
/// pulled `percent` of the radius out of it, or every slice (`None`: the
/// series' own explosion, the slices' own ones taken away); 0 puts it
/// back. `None` when there is no such series.
pub fn with_explosion(
    text: &str,
    series: usize,
    point: Option<usize>,
    percent: u32,
) -> Option<String> {
    let (span, p) = series_span(text, series)?;
    let value = if percent > 0 {
        format!("<{p}explosion val=\"{percent}\"/>")
    } else {
        String::new()
    };
    let ser = &text[span.clone()];
    let new_ser = match point {
        Some(point) => edit_point(ser, &p, point, |old| {
            let base = old.map_or_else(
                || format!("<{p}dPt><{p}idx val=\"{point}\"/><{p}bubble3D val=\"0\"/></{p}dPt>"),
                str::to_owned,
            );
            let el = set_child(
                &base,
                &["explosion"],
                &value,
                &["idx", "invertIfNegative", "marker", "bubble3D"],
            );
            (!trivial_point(&el)).then_some(el)
        }),
        None => {
            let with_own = set_child(ser, &["explosion"], &value, &["idx", "order", "tx", "spPr"]);
            // The slices' own explosions give way to the series'.
            let (t, open_end, close, kids) = open_up(&with_own);
            let mut out = String::from(&t[..open_end]);
            for (name, sp) in &kids {
                let el = &t[sp.clone()];
                if name == "dPt" {
                    let el = set_child(el, &["explosion"], "", &[]);
                    if !trivial_point(&el) {
                        out.push_str(&el);
                    }
                    continue;
                }
                out.push_str(el);
            }
            out.push_str(&t[close..]);
            out
        }
    };
    let mut out = text.to_owned();
    out.replace_range(span, &new_ser);
    Some(out)
}

/// An `<c:spPr>` (or none) given a fill and a line's fill, the rest of
/// it kept; empty when nothing is left in it.
fn painted(
    sp: Option<&str>,
    p: &str,
    (a, decl): (&str, &str),
    background: Fill,
    border: Fill,
) -> String {
    let paint = |f: Fill| match f {
        Fill::Auto => String::new(),
        Fill::None => format!("<{a}:noFill{decl}/>"),
        Fill::Color(c) => {
            format!("<{a}:solidFill{decl}><{a}:srgbClr val=\"{c:06X}\"/></{a}:solidFill>")
        }
    };
    let geometry = ["xfrm", "custGeom", "prstGeom"];
    let sp = sp.map_or_else(|| format!("<{p}spPr></{p}spPr>"), str::to_owned);
    let sp = set_child(&sp, &FILLS, &paint(background), &geometry);
    let line = paint(border);
    let ln = match child(&sp, "ln") {
        Some(ln) => set_child(ln, &FILLS, &line, &[]),
        None if line.is_empty() => String::new(),
        None => format!("<{a}:ln{decl}>{line}</{a}:ln>"),
    };
    // A line left with nothing to say goes.
    let ln = if !ln.is_empty() && open_up(&ln).3.is_empty() && !ln.contains(" w=") {
        String::new()
    } else {
        ln
    };
    let mut after = geometry.to_vec();
    after.extend(FILLS);
    let sp = set_child(&sp, &["ln"], &ln, &after);
    if open_up(&sp).3.is_empty() {
        String::new()
    } else {
        sp
    }
}

/// A chart part with its chart area's background and border painted so:
/// the chart space's `<c:spPr>` (after `<c:chart>`, as the schema puts
/// it) given the fill and the line's fill, the rest of it kept.
pub fn with_chart_area(text: &str, background: Fill, border: Fill) -> String {
    // The root element's bytes.
    let mut r = Reader::new(text);
    let mut root = None;
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t {
            let end = if tag.empty {
                tag.span.end
            } else {
                r.skip_element()
            };
            root = Some((tag.span.start..end, xml::prefix(tag.qname).to_owned()));
            break;
        }
    }
    let Some((span, p)) = root else {
        return text.to_owned();
    };
    let el = &text[span.clone()];
    let (a, decl) = drawing_prefix(text);
    let sp = painted(child(el, "spPr"), &p, (&a, &decl), background, border);
    let new_root = set_child(
        el,
        &["spPr"],
        &sp,
        &[
            "date1904",
            "lang",
            "roundedCorners",
            "AlternateContent",
            "style",
            "clrMapOvr",
            "pivotSource",
            "protection",
            "chart",
        ],
    );
    let mut out = text.to_owned();
    out.replace_range(span, &new_root);
    out
}

/// A chart part with its plot area's background and border painted so:
/// the plot area's `<c:spPr>`, after its plots and axes as the schema
/// puts it, the rest of it kept.
pub fn with_plot_area(text: &str, background: Fill, border: Fill) -> String {
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    let mut found = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if tag.name == "plotArea" && stack.last().is_some_and(|s| s == "chart") {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    found = Some((tag.span.start..end, xml::prefix(tag.qname).to_owned()));
                    break;
                }
                if !tag.empty {
                    stack.push(tag.name.to_owned());
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    let Some((span, p)) = found else {
        return text.to_owned();
    };
    let el = &text[span.clone()];
    let (a, decl) = drawing_prefix(text);
    let sp = painted(child(el, "spPr"), &p, (&a, &decl), background, border);
    // After every child but its own spPr and extensions.
    let names: Vec<String> = open_up(el)
        .3
        .into_iter()
        .map(|k| k.0)
        .filter(|n| n != "spPr" && n != "extLst")
        .collect();
    let after: Vec<&str> = names.iter().map(String::as_str).collect();
    let new_plot = set_child(el, &["spPr"], &sp, &after);
    let mut out = text.to_owned();
    out.replace_range(span, &new_plot);
    out
}

/// A chart part with its gridlines shown or hidden (horizontal major and
/// minor, from the vertical axis; vertical major and minor, from the
/// horizontal axis): a line kept as it is when it stays, written after
/// the axis's position when it comes. `None` when the chart has no axes.
pub fn with_gridlines(text: &str, lines: (bool, bool, bool, bool)) -> Option<String> {
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    let mut axes: Vec<(Span<usize>, String)> = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if tag.name.ends_with("Ax")
                    && tag.name != "serAx"
                    && stack.last().is_some_and(|s| s == "plotArea")
                    && !tag.empty
                {
                    let end = r.skip_element();
                    axes.push((tag.span.start..end, xml::prefix(tag.qname).to_owned()));
                    continue;
                }
                if !tag.empty {
                    stack.push(tag.name.to_owned());
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    if axes.is_empty() {
        return None;
    }
    let mut out = text.to_owned();
    // From the last, so the earlier spans hold.
    for (span, p) in axes.into_iter().rev() {
        let el = &text[span.clone()];
        let vertical = matches!(
            child(el, "axPos").and_then(|a| a.split("val=\"").nth(1)?.split('"').next()),
            Some("l" | "r")
        );
        let (major, minor) = if vertical {
            (lines.0, lines.1)
        } else {
            (lines.2, lines.3)
        };
        let mut new = el.to_owned();
        for (name, want, after) in [
            (
                "majorGridlines",
                major,
                &["axId", "scaling", "delete", "axPos"][..],
            ),
            (
                "minorGridlines",
                minor,
                &["axId", "scaling", "delete", "axPos", "majorGridlines"][..],
            ),
        ] {
            match (child(&new, name).is_some(), want) {
                (true, false) => new = set_child(&new, &[name], "", &[]),
                (false, true) => new = set_child(&new, &[name], &format!("<{p}{name}/>"), after),
                _ => {}
            }
        }
        out.replace_range(span, &new);
    }
    Some(out)
}

/// The value axis of a chart part (a scatter chart's vertical one): its
/// bytes and prefix.
fn value_axis(text: &str, scatter: bool) -> Option<(Span<usize>, String)> {
    let mut r = Reader::new(text);
    let mut stack: Vec<String> = Vec::new();
    let mut axes: Vec<(Span<usize>, String, bool)> = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if tag.name == "valAx"
                    && stack.last().is_some_and(|s| s == "plotArea")
                    && !tag.empty
                {
                    let start = tag.span.start;
                    let p = xml::prefix(tag.qname).to_owned();
                    let end = r.skip_element();
                    let vertical = matches!(
                        child(&text[start..end], "axPos").and_then(|a| a
                            .split("val=\"")
                            .nth(1)?
                            .split('"')
                            .next()),
                        Some("l" | "r")
                    );
                    axes.push((start..end, p, vertical));
                    continue;
                }
                if !tag.empty {
                    stack.push(tag.name.to_owned());
                }
            }
            Token::End { .. } => {
                stack.pop();
            }
            Token::Text { .. } => {}
        }
    }
    let pick = if scatter {
        axes.into_iter().find(|a| a.2)
    } else {
        axes.into_iter().next()
    };
    pick.map(|a| (a.0, a.1))
}

/// A chart part with its value axis's labels in `format`, or in the
/// cells' own (`None`): the axis's `<c:numFmt>` written where the schema
/// puts it. `None` when the chart has no value axis (a pie).
pub fn with_axis_format(text: &str, format: Option<&str>, scatter: bool) -> Option<String> {
    let (span, p) = value_axis(text, scatter)?;
    let el = &text[span.clone()];
    let fmt = match format {
        Some(code) => format!(
            "<{p}numFmt formatCode=\"{}\" sourceLinked=\"0\"/>",
            xml::escape(code)
        ),
        None => format!("<{p}numFmt formatCode=\"General\" sourceLinked=\"1\"/>"),
    };
    let new = set_child(
        el,
        &["numFmt"],
        &fmt,
        &[
            "axId",
            "scaling",
            "delete",
            "axPos",
            "majorGridlines",
            "minorGridlines",
            "title",
        ],
    );
    let mut out = text.to_owned();
    out.replace_range(span, &new);
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
    /// Its color, when it keeps one.
    pub color: Option<Rgb>,
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
            // Its own color, kept from the chart it was (not a pie's slices).
            if let Some(c) = s.color {
                match kind {
                    ChartKind::Column | ChartKind::Bar | ChartKind::Area => x.push_str(&format!(
                        "<c:spPr><a:solidFill><a:srgbClr val=\"{c:06X}\"/></a:solidFill></c:spPr>"
                    )),
                    ChartKind::Line => x.push_str(&format!(
                        "<c:spPr><a:ln w=\"28575\" cap=\"rnd\"><a:solidFill><a:srgbClr val=\"{c:06X}\"/></a:solidFill></a:ln></c:spPr>"
                    )),
                    _ => {}
                }
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
            color: None,
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
            color: None,
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
            color: None,
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
    fn legends_moved_added_and_removed() {
        let s = NewSeries {
            name: None,
            cat: None,
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
            color: None,
        };
        // One series: no legend to start with.
        let x = chart_xml(ChartKind::Column, None, std::slice::from_ref(&s));
        assert_eq!(parse_chart(&x, &[]).legend, None);
        let y = with_legend(&x, Some("r"));
        assert_eq!(parse_chart(&y, &[]).legend.as_deref(), Some("r"));
        assert!(y.find("<c:legend>") > y.find("</c:plotArea>"));
        assert!(y.find("<c:legend>") < y.find("<c:plotVisOnly"));
        let z = with_legend(&y, Some("t"));
        assert_eq!(z.matches("<c:legend>").count(), 1);
        assert_eq!(parse_chart(&z, &[]).legend.as_deref(), Some("t"));
        assert_eq!(parse_chart(&with_legend(&z, None), &[]).legend, None);
        // Excel's legend, formatted and without a position: moved, kept.
        let excel = r#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea/><c:legend><c:layout/><c:txPr>font</c:txPr></c:legend></c:chart></c:chartSpace>"#;
        assert_eq!(parse_chart(excel, &[]).legend.as_deref(), Some("r"));
        let m = with_legend(excel, Some("b"));
        assert!(
            m.contains(
                "<c:legend><c:legendPos val=\"b\"/><c:layout/><c:txPr>font</c:txPr></c:legend>"
            ),
            "{m}"
        );
    }

    #[test]
    fn data_labels_on_and_off() {
        let s = NewSeries {
            name: Some(("S!$B$1".into(), "Q1".into())),
            cat: Some(("S!$A$2:$A$3".into(), vec!["a".into(), "b".into()], false)),
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
            color: None,
        };
        for kind in [
            ChartKind::Column,
            ChartKind::Line,
            ChartKind::Pie,
            ChartKind::Scatter,
        ] {
            let x = chart_xml(kind, None, &[s.clone(), s.clone()]);
            let pie = kind == ChartKind::Pie;
            let y = with_labels(&x, (true, false, false, true), pie);
            let d = parse_chart(&y, &[]);
            assert_eq!(d.labels, (true, false, false, pie), "{kind:?}");
            assert_eq!(y.matches("<c:dLbls>").count(), 2, "one a series");
            // Before the categories and values, as the schema asks.
            let ser = &y[y.find("<c:ser>").unwrap()..y.find("</c:ser>").unwrap()];
            let v = ser
                .find("<c:cat>")
                .or_else(|| ser.find("<c:xVal>"))
                .unwrap();
            assert!(ser.find("<c:dLbls>").unwrap() < v, "{ser}");
            // Again: replaced; off: gone.
            let z = with_labels(&y, (false, true, true, false), pie);
            assert_eq!(parse_chart(&z, &[]).labels, (false, true, true, false));
            assert_eq!(
                z.matches("<c:dLbls>").count(),
                y.matches("<c:dLbls>").count()
            );
            let off = with_labels(&z, (false, false, false, false), pie);
            assert!(!off.contains("dLbls"));
            assert_eq!(parse_chart(&off, &[]).series.len(), 2);
        }
        // The plot's own labels count, and go when set per series.
        let plot = r#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea><c:barChart><c:ser><c:val/></c:ser><c:dLbls><c:showVal val="1"/></c:dLbls></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        assert!(parse_chart(plot, &[]).labels.0);
        let set = with_labels(plot, (false, true, false, false), false);
        assert_eq!(set.matches("<c:dLbls>").count(), 1);
        assert_eq!(parse_chart(&set, &[]).labels, (false, true, false, false));
    }

    #[test]
    fn value_axis_scales() {
        let s = NewSeries {
            name: None,
            cat: Some(("S!$A$2:$A$3".into(), vec!["1".into(), "2".into()], false)),
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(200.0)]),
            color: None,
        };
        for kind in [
            ChartKind::Column,
            ChartKind::Bar,
            ChartKind::Line,
            ChartKind::Scatter,
        ] {
            let x = chart_xml(kind, None, std::slice::from_ref(&s));
            let scatter = kind == ChartKind::Scatter;
            assert_eq!(parse_chart(&x, &[]).scale, (None, None, None, false));
            let y = with_scale(&x, (Some(0.0), Some(250.0), Some(50.0), false), scatter).unwrap();
            assert_eq!(
                parse_chart(&y, &[]).scale,
                (Some(0.0), Some(250.0), Some(50.0), false),
                "{kind:?}"
            );
            // The schema's order: orientation, max, min; majorUnit after crossBetween.
            let ax = &y[y.rfind("<c:valAx>").unwrap()..];
            let ax = &ax[..ax.find("</c:valAx>").unwrap()];
            if !scatter || ax.contains("<c:max") {
                let o = ax.find("<c:orientation").unwrap();
                assert!(
                    o < ax.find("<c:max").unwrap() && ax.find("<c:max") < ax.find("<c:min"),
                    "{ax}"
                );
                assert!(ax.find("<c:crossBetween") < ax.find("<c:majorUnit"), "{ax}");
            }
            // Logarithmic, the bounds left to the spreadsheet.
            let z = with_scale(&y, (None, None, None, true), scatter).unwrap();
            assert_eq!(parse_chart(&z, &[]).scale, (None, None, None, true));
            assert!(z.contains("<c:logBase val=\"10\"/><c:orientation"));
            assert!(!z.contains("majorUnit"));
            let back = with_scale(&z, (None, None, None, false), scatter).unwrap();
            assert_eq!(parse_chart(&back, &[]).scale, (None, None, None, false));
        }
        let pie = chart_xml(ChartKind::Pie, None, std::slice::from_ref(&s));
        assert!(with_scale(&pie, (Some(1.0), None, None, false), false).is_none());
    }

    #[test]
    fn series_colors_set_and_cleared() {
        let s = NewSeries {
            name: None,
            cat: Some(("S!$A$2:$A$3".into(), vec!["1".into(), "2".into()], false)),
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
            color: None,
        };
        for kind in [
            ChartKind::Column,
            ChartKind::Line,
            ChartKind::Area,
            ChartKind::Scatter,
        ] {
            let x = chart_xml(kind, None, &[s.clone(), s.clone()]);
            let y = with_series_color(&x, 1, Some(0x123456), kind).unwrap();
            let d = parse_chart(&y, &[]);
            assert_eq!(
                (d.series[0].color, d.series[1].color),
                (None, Some(0x123456)),
                "{kind:?}"
            );
            // Again: replaced, not added.
            let z = with_series_color(&y, 1, Some(0xABCDEF), kind).unwrap();
            assert_eq!(parse_chart(&z, &[]).series[1].color, Some(0xABCDEF));
            assert_eq!(z.matches("srgbClr").count(), 1, "{z}");
            // Cleared: the theme's.
            let back = with_series_color(&z, 1, None, kind).unwrap();
            assert_eq!(parse_chart(&back, &[]).series[1].color, None, "{back}");
            if kind == ChartKind::Scatter {
                assert!(back.contains("<a:noFill/>"), "the line stays off");
            }
        }
        // Excel's series: a theme fill and a border; the fill changes only.
        let excel = r#"<c:chartSpace xmlns:c="c" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:barChart><c:ser><c:idx val="0"/><c:order val="0"/><c:spPr><a:solidFill><a:schemeClr val="accent1"/></a:solidFill><a:ln><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></c:spPr><c:val/></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let y = with_series_color(excel, 0, Some(0xFF0000), ChartKind::Column).unwrap();
        assert!(y.contains(r#"<c:spPr><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:ln><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></c:spPr>"#), "{y}");
        assert!(with_series_color(excel, 3, Some(1), ChartKind::Column).is_none());
    }

    #[test]
    fn slice_colors_set_ordered_and_cleared() {
        let s = NewSeries {
            name: None,
            cat: Some((
                "S!$A$2:$A$4".into(),
                vec!["a".into(), "b".into(), "c".into()],
                false,
            )),
            val: ("S!$B$2:$B$4".into(), vec![Some(1.0), Some(2.0), Some(3.0)]),
            color: None,
        };
        let x = chart_xml(ChartKind::Pie, None, std::slice::from_ref(&s));
        let y = with_point_color(&x, 0, 2, Some(0xFF0000)).unwrap();
        let y = with_point_color(&y, 0, 0, Some(0x00FF00)).unwrap();
        let d = parse_chart(&y, &[]);
        assert_eq!(d.series[0].points, vec![(0, 0x00FF00), (2, 0xFF0000)]);
        // In the order of the points, before the categories.
        let ser = &y[y.find("<c:ser>").unwrap()..];
        assert!(ser.find("idx val=\"0\"/><c:bubble3D") < ser.find("idx val=\"2\"/><c:bubble3D"));
        assert!(ser.rfind("<c:dPt>") < ser.find("<c:cat>"));
        assert!(ser.find("<c:dPt>") > ser.find("<c:order"));
        // Recolored in place; cleared.
        let z = with_point_color(&y, 0, 2, Some(0x0000FF)).unwrap();
        assert_eq!(
            parse_chart(&z, &[]).series[0].points,
            vec![(0, 0x00FF00), (2, 0x0000FF)]
        );
        assert_eq!(z.matches("<c:dPt>").count(), 2);
        let z = with_point_color(&z, 0, 0, None).unwrap();
        assert_eq!(parse_chart(&z, &[]).series[0].points, vec![(2, 0x0000FF)]);
        // The middle one between them.
        let m = with_point_color(&y, 0, 1, Some(0x111111)).unwrap();
        let pts: Vec<usize> = parse_chart(&m, &[]).series[0]
            .points
            .iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(pts, [0, 1, 2]);
        assert!(
            m.find("idx val=\"1\"/><c:bubble3D").unwrap()
                < m.find("idx val=\"2\"/><c:bubble3D").unwrap()
        );
        assert!(with_point_color(&x, 4, 0, Some(1)).is_none());
    }

    #[test]
    fn slices_pulled_out() {
        let s = NewSeries {
            name: None,
            cat: Some((
                "S!$A$2:$A$4".into(),
                vec!["a".into(), "b".into(), "c".into()],
                false,
            )),
            val: ("S!$B$2:$B$4".into(), vec![Some(1.0), Some(2.0), Some(3.0)]),
            color: None,
        };
        let x = chart_xml(ChartKind::Pie, None, std::slice::from_ref(&s));
        // One slice out, colored too; its color and explosion share a dPt.
        let y = with_explosion(&x, 0, Some(1), 25).unwrap();
        let y = with_point_color(&y, 0, 1, Some(0xFF0000)).unwrap();
        let d = parse_chart(&y, &[]);
        assert_eq!(d.series[0].point_explosions, vec![(1, 25)]);
        assert_eq!(d.series[0].points, vec![(1, 0xFF0000)]);
        assert_eq!(y.matches("<c:dPt>").count(), 1);
        let pt = &y[y.find("<c:dPt>").unwrap()..y.find("</c:dPt>").unwrap()];
        assert!(pt.find("<c:explosion") < pt.find("<c:spPr"), "{pt}");
        // Its color taken away: the explosion stays; back in: nothing left.
        let z = with_point_color(&y, 0, 1, None).unwrap();
        assert_eq!(
            parse_chart(&z, &[]).series[0].point_explosions,
            vec![(1, 25)]
        );
        let z = with_explosion(&z, 0, Some(1), 0).unwrap();
        assert!(!z.contains("dPt"), "{z}");
        // Every slice: the series' explosion, after its properties, the
        // slices' own given way.
        let all = with_explosion(&y, 0, None, 10).unwrap();
        let d = parse_chart(&all, &[]);
        assert_eq!(
            (d.series[0].explosion, d.series[0].point_explosions.clone()),
            (10, vec![])
        );
        assert_eq!(d.series[0].points, vec![(1, 0xFF0000)], "the color stays");
        let ser = &all[all.find("<c:ser>").unwrap()..];
        assert!(ser.find("<c:explosion") < ser.find("<c:dPt>"));
        assert!(ser.find("<c:explosion") > ser.find("<c:order"));
        let none = with_explosion(&all, 0, None, 0).unwrap();
        assert_eq!(parse_chart(&none, &[]).series[0].explosion, 0);
        assert!(with_explosion(&x, 2, None, 5).is_none());
    }

    #[test]
    fn chart_area_painted() {
        let s = NewSeries {
            name: None,
            cat: None,
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
            color: None,
        };
        let x = chart_xml(ChartKind::Column, None, std::slice::from_ref(&s));
        let d = parse_chart(&x, &[]);
        assert_eq!((d.background, d.border), (Fill::Auto, Fill::Auto));
        let y = with_chart_area(&x, Fill::Color(0xFFF2CC), Fill::Color(0x404040));
        let d = parse_chart(&y, &[]);
        assert_eq!(
            (d.background, d.border),
            (Fill::Color(0xFFF2CC), Fill::Color(0x404040))
        );
        // After the chart, as the schema puts it; a series' fill not taken.
        assert!(y.find("<c:spPr>") > y.find("</c:chart>"), "{y}");
        assert!(parse_chart(&y, &[]).series[0].color.is_none());
        // No border, the background kept; then all automatic: no spPr.
        let z = with_chart_area(&y, Fill::Color(0xFFF2CC), Fill::None);
        assert_eq!(parse_chart(&z, &[]).border, Fill::None);
        assert_eq!(parse_chart(&z, &[]).background, Fill::Color(0xFFF2CC));
        let z = with_chart_area(&z, Fill::None, Fill::None);
        assert_eq!(parse_chart(&z, &[]).background, Fill::None);
        let back = with_chart_area(&z, Fill::Auto, Fill::Auto);
        assert!(
            !back[back.find("</c:chart>").unwrap()..].contains("spPr"),
            "{back}"
        );
        // Excel's area: a width on its line stays.
        let excel = r#"<c:chartSpace xmlns:c="c" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart/><c:spPr><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:ln w="9525" cap="flat"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill></a:ln></c:spPr><c:txPr/></c:chartSpace>"#;
        let e = with_chart_area(excel, Fill::Auto, Fill::Color(0xFF0000));
        assert!(e.contains(r#"<c:spPr><a:ln w="9525" cap="flat"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:ln></c:spPr><c:txPr/>"#), "{e}");
    }

    #[test]
    fn plot_area_painted() {
        let s = NewSeries {
            name: None,
            cat: None,
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
            color: None,
        };
        for kind in [ChartKind::Column, ChartKind::Pie, ChartKind::Scatter] {
            let x = chart_xml(kind, None, std::slice::from_ref(&s));
            let y = with_plot_area(&x, Fill::Color(0xF2F2F2), Fill::Color(0x7F7F7F));
            let d = parse_chart(&y, &[]);
            assert_eq!(
                (d.plot_background, d.plot_border),
                (Fill::Color(0xF2F2F2), Fill::Color(0x7F7F7F)),
                "{kind:?}"
            );
            assert_eq!(
                (d.background, d.border),
                (Fill::Auto, Fill::Auto),
                "not the chart area's"
            );
            assert!(d.series[0].color.is_none(), "not the series'");
            // After the plots and axes, inside the plot area.
            let pa = &y[y.find("<c:plotArea>").unwrap()..y.find("</c:plotArea>").unwrap()];
            assert!(
                pa.rfind("<c:spPr>").unwrap() > pa.rfind("Chart>").unwrap(),
                "{pa}"
            );
            if pa.contains("Ax>") {
                assert!(
                    pa.rfind("<c:spPr>").unwrap() > pa.rfind("Ax>").unwrap(),
                    "{pa}"
                );
            }
            let back = with_plot_area(&y, Fill::Auto, Fill::Auto);
            assert_eq!(parse_chart(&back, &[]).plot_background, Fill::Auto);
            assert_eq!(
                back.matches("<c:spPr>").count(),
                x.matches("<c:spPr>").count()
            );
        }
    }

    #[test]
    fn gridlines_shown_and_hidden() {
        let s = NewSeries {
            name: None,
            cat: Some(("S!$A$2:$A$3".into(), vec!["1".into(), "2".into()], false)),
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
            color: None,
        };
        // A column chart's value axis is vertical: its lines horizontal; a
        // bar chart's the other way round.
        let col = chart_xml(ChartKind::Column, None, std::slice::from_ref(&s));
        assert_eq!(
            parse_chart(&col, &[]).gridlines,
            (true, false, false, false)
        );
        let bar = chart_xml(ChartKind::Bar, None, std::slice::from_ref(&s));
        assert_eq!(
            parse_chart(&bar, &[]).gridlines,
            (false, false, true, false)
        );
        for x in [
            col,
            bar,
            chart_xml(ChartKind::Scatter, None, std::slice::from_ref(&s)),
        ] {
            let all = with_gridlines(&x, (true, true, true, true)).unwrap();
            assert_eq!(parse_chart(&all, &[]).gridlines, (true, true, true, true));
            // After the position, major before minor.
            for ax in all.split("Ax>").filter(|a| a.contains("<c:axPos")) {
                let pos = ax.find("<c:axPos").unwrap();
                let major = ax.find("<c:majorGridlines").unwrap();
                assert!(
                    pos < major && major < ax.find("<c:minorGridlines").unwrap(),
                    "{ax}"
                );
            }
            let none = with_gridlines(&all, (false, false, false, false)).unwrap();
            assert!(!none.contains("Gridlines"));
        }
        // Excel's formatted gridlines stay as they are when kept.
        let excel = r#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea><c:valAx><c:axId val="1"/><c:axPos val="l"/><c:majorGridlines><c:spPr>gray</c:spPr></c:majorGridlines><c:numFmt/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let kept = with_gridlines(excel, (true, true, false, false)).unwrap();
        assert!(kept.contains("<c:majorGridlines><c:spPr>gray</c:spPr></c:majorGridlines><c:minorGridlines/><c:numFmt/>"), "{kept}");
        let pie = chart_xml(ChartKind::Pie, None, std::slice::from_ref(&s));
        assert!(with_gridlines(&pie, (true, false, false, false)).is_none());
    }

    #[test]
    fn axis_number_formats() {
        let s = NewSeries {
            name: None,
            cat: Some(("S!$A$2:$A$3".into(), vec!["1".into(), "2".into()], false)),
            val: ("S!$B$2:$B$3".into(), vec![Some(1.0), Some(2.0)]),
            color: None,
        };
        for kind in [ChartKind::Column, ChartKind::Bar, ChartKind::Scatter] {
            let x = chart_xml(kind, None, std::slice::from_ref(&s));
            let scatter = kind == ChartKind::Scatter;
            assert_eq!(
                parse_chart(&x, &[]).axis_format,
                None,
                "linked to the cells"
            );
            let y = with_axis_format(&x, Some("#,##0 \"TL\""), scatter).unwrap();
            assert_eq!(
                parse_chart(&y, &[]).axis_format.as_deref(),
                Some("#,##0 \"TL\""),
                "{kind:?}"
            );
            assert_eq!(
                y.matches("<c:numFmt").count(),
                x.matches("<c:numFmt").count(),
                "replaced"
            );
            let back = with_axis_format(&y, None, scatter).unwrap();
            assert_eq!(parse_chart(&back, &[]).axis_format, None);
        }
        // After the title, before the tick marks.
        let x = chart_xml(ChartKind::Column, None, std::slice::from_ref(&s));
        let x = axis_titled(&x, true, Some("TRY")).unwrap();
        let y = with_axis_format(&x, Some("0%"), false).unwrap();
        let ax = &y[y.find("<c:valAx>").unwrap()..];
        assert!(ax.find("</c:title>") < ax.find("formatCode=\"0%\""));
        assert!(ax.find("formatCode=\"0%\"") < ax.find("<c:majorTickMark"));
        let pie = chart_xml(ChartKind::Pie, None, std::slice::from_ref(&s));
        assert!(with_axis_format(&pie, Some("0"), false).is_none());
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
