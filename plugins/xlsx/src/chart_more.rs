//! More of a chart part changed in place: a series drawn as another kind
//! against the primary or secondary axis (a combo chart), its trendline,
//! its error bars, its data labels taken from cells; as Excel writes
//! them, the rest of the part kept.

use crate::chart::{open_up, set_child};
use crate::xml::{self, Reader, Token};
use kalem_viewer::{ChartKind, ErrorBars, ErrorKind, TrendKind, Trendline};
use std::ops::Range as Span;

/// The children a series has before its trendline (CT_*Ser's order).
const BEFORE_TRENDLINE: [&str; 10] = [
    "idx",
    "order",
    "tx",
    "spPr",
    "invertIfNegative",
    "pictureOptions",
    "marker",
    "dPt",
    "dLbls",
    "explosion",
];

/// Series `series` of a chart part (counted over every plot): its bytes'
/// place and its prefix (`c:`).
fn series_at(text: &str, series: usize) -> Option<(Span<usize>, String)> {
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

/// A chart part with series `series` changed by `edit` (given its bytes
/// and prefix); `None` when there is no such series.
fn edit_series(
    text: &str,
    series: usize,
    edit: impl FnOnce(&str, &str) -> String,
) -> Option<String> {
    let (span, p) = series_at(text, series)?;
    let new = edit(&text[span.clone()], &p);
    Some(format!("{}{new}{}", &text[..span.start], &text[span.end..]))
}

/// A chart part with series `series` given `trendline`, or none.
pub fn with_trendline(text: &str, series: usize, trendline: Option<Trendline>) -> Option<String> {
    edit_series(text, series, |el, p| {
        let new = trendline.map_or(String::new(), |t| {
            let kind = match t.kind {
                TrendKind::Linear => "linear",
                TrendKind::Exponential => "exp",
                TrendKind::Logarithmic => "log",
                TrendKind::Polynomial => "poly",
                TrendKind::Power => "power",
                TrendKind::MovingAverage => "movingAvg",
            };
            let extra = match t.kind {
                TrendKind::Polynomial => format!("<{p}order val=\"{}\"/>", t.order.clamp(2, 6)),
                TrendKind::MovingAverage => {
                    format!("<{p}period val=\"{}\"/>", t.period.clamp(2, 255))
                }
                _ => String::new(),
            };
            format!(
                "<{p}trendline><{p}trendlineType val=\"{kind}\"/>{extra}<{p}dispRSqr val=\"{}\"/><{p}dispEq val=\"{}\"/></{p}trendline>",
                u8::from(t.r_squared),
                u8::from(t.equation)
            )
        });
        set_child(el, &["trendline"], &new, &BEFORE_TRENDLINE)
    })
}

/// A chart part with series `series` given error bars, or none; a
/// scatter or bubble chart's in the y direction.
pub fn with_error_bars(
    text: &str,
    series: usize,
    bars: Option<ErrorBars>,
    xy: bool,
) -> Option<String> {
    edit_series(text, series, |el, p| {
        let new = bars.map_or(String::new(), |b| {
            let kind = match b.kind {
                ErrorKind::Fixed => "fixedVal",
                ErrorKind::Percent => "percentage",
                ErrorKind::StdDev => "stdDev",
                ErrorKind::StdErr => "stdErr",
            };
            let dir = if xy {
                format!("<{p}errDir val=\"y\"/>")
            } else {
                String::new()
            };
            let val = if b.kind == ErrorKind::StdErr {
                String::new()
            } else {
                format!("<{p}val val=\"{}\"/>", b.value)
            };
            format!(
                "<{p}errBars>{dir}<{p}errBarType val=\"both\"/><{p}errValType val=\"{kind}\"/><{p}noEndCap val=\"0\"/>{val}</{p}errBars>"
            )
        });
        let mut after = BEFORE_TRENDLINE.to_vec();
        after.push("trendline");
        // A scatter series may have one for x too; only the y one is set.
        set_child(el, &["errBars"], &new, &after)
    })
}

const LABELS_RANGE: &str = "{02D57815-91ED-43cb-92C2-25804820EDAC}";
const SHOW_RANGE: &str = "{CE6537A1-D6FC-4f65-9D91-7224C49458BB}";
const C15: &str = "http://schemas.microsoft.com/office/drawing/2012/chart";

/// An element's `extLst` without the extension `uri`, and with `add`
/// (an `ext`) when given; the element itself without an empty list.
fn with_ext(el: &str, p: &str, uri: &str, add: &str) -> String {
    let list = crate::chart::child(el, "extLst").unwrap_or("").to_owned();
    let mut exts = String::new();
    if !list.is_empty() {
        let (inner, _, _, kids) = open_up(&list);
        for (name, span) in kids {
            let x = &inner[span];
            if name == "ext" && x.contains(uri) {
                continue;
            }
            exts.push_str(x);
        }
    }
    exts.push_str(add);
    let new = if exts.is_empty() {
        String::new()
    } else {
        format!("<{p}extLst>{exts}</{p}extLst>")
    };
    // The list is a series' and a dLbls' last child.
    let (open, _, _, kids) = open_up(el);
    let after: Vec<&str> = kids
        .iter()
        .map(|k| k.0.as_str())
        .filter(|n| *n != "extLst")
        .collect();
    set_child(&open, &["extLst"], &new, &after)
}

/// A chart part with series `series`'s data labels the texts of cells
/// `range` (a reference like `Sheet1!$C$2:$C$5`) with their texts cached,
/// or no more.
pub fn with_label_cells(
    text: &str,
    series: usize,
    range: Option<(&str, &[String])>,
) -> Option<String> {
    edit_series(text, series, |el, p| {
        let Some((f, texts)) = range else {
            let el = with_ext(el, p, LABELS_RANGE, "");
            return set_child(&el, &["dLbls"], "", &[]);
        };
        let pts: String = texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "<c15:pt idx=\"{i}\"><c15:v>{}</c15:v></c15:pt>",
                    xml::escape(t)
                )
            })
            .collect();
        let ext = format!(
            "<{p}ext uri=\"{LABELS_RANGE}\" xmlns:c15=\"{C15}\"><c15:datalabelsRange><c15:f>{}</c15:f><c15:dlblRangeCache><c15:ptCount val=\"{}\"/>{pts}</c15:dlblRangeCache></c15:datalabelsRange></{p}ext>",
            xml::escape(f),
            texts.len()
        );
        let el = with_ext(el, p, LABELS_RANGE, &ext);
        let labels = format!(
            "<{p}dLbls><{p}showLegendKey val=\"0\"/><{p}showVal val=\"0\"/><{p}showCatName val=\"0\"/><{p}showSerName val=\"0\"/><{p}showPercent val=\"0\"/><{p}showBubbleSize val=\"0\"/><{p}extLst><{p}ext uri=\"{SHOW_RANGE}\" xmlns:c15=\"{C15}\"><c15:showDataLabelsRange val=\"1\"/></{p}ext></{p}extLst></{p}dLbls>"
        );
        let after = [
            "idx",
            "order",
            "tx",
            "spPr",
            "invertIfNegative",
            "pictureOptions",
            "marker",
            "dPt",
        ];
        set_child(&el, &["dLbls"], &labels, &after)
    })
}

/// The plot elements of a chart part's plot area and its axes: each with
/// its name, place, and the axis ids it names (an axis its own id and
/// position).
struct PlotArea {
    span: Span<usize>,
    plots: Vec<(String, Span<usize>, Vec<String>)>,
    axes: Vec<(String, Span<usize>, String, String)>,
    prefix: String,
}

fn plot_area(text: &str) -> Option<PlotArea> {
    let mut r = Reader::new(text);
    let mut found = None;
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "plotArea"
            && !tag.empty
        {
            let start = tag.span.start;
            let end = r.skip_element();
            found = Some((start..end, xml::prefix(tag.qname).to_owned()));
            break;
        }
    }
    let (span, prefix) = found?;
    let el = &text[span.clone()];
    let (_, _, _, kids) = open_up(el);
    let mut plots = Vec::new();
    let mut axes = Vec::new();
    for (name, s) in kids {
        let abs = span.start + s.start..span.start + s.end;
        let inner = &el[s];
        let (_, _, _, sub) = open_up(inner);
        let val = |k: &str| -> Vec<String> {
            sub.iter()
                .filter(|x| x.0 == k)
                .filter_map(|x| {
                    inner[x.1.clone()]
                        .split("val=\"")
                        .nth(1)
                        .and_then(|v| v.split('"').next())
                        .map(str::to_owned)
                })
                .collect()
        };
        if name.ends_with("Chart") {
            plots.push((name, abs, val("axId")));
        } else if name.ends_with("Ax") {
            let id = val("axId").into_iter().next().unwrap_or_default();
            let pos = val("axPos").into_iter().next().unwrap_or_default();
            axes.push((name, abs, id, pos));
        }
    }
    Some(PlotArea {
        span,
        plots,
        axes,
        prefix,
    })
}

/// Which of a plot's ids is its value axis.
fn value_axis_of<'a>(area: &'a PlotArea, ids: &[String]) -> Option<&'a String> {
    area.axes
        .iter()
        .find(|a| a.0 == "valAx" && ids.contains(&a.2))
        .map(|a| &a.2)
}

/// A series' children a plot of `kind` does not take, left out.
fn cleaned_for(el: &str, kind: ChartKind) -> String {
    let (open, _, _, kids) = open_up(el);
    let drop: &[&str] = match kind {
        ChartKind::Line => &["invertIfNegative", "pictureOptions", "explosion", "shape"],
        ChartKind::Area => &[
            "invertIfNegative",
            "pictureOptions",
            "marker",
            "explosion",
            "shape",
            "smooth",
        ],
        _ => &["marker", "explosion", "smooth"],
    };
    let mut out = open.clone();
    for name in drop {
        if kids.iter().any(|k| k.0 == *name) {
            out = set_child(&out, &[name], "", &[]);
        }
    }
    // A line not smoothed, as Excel writes a new one.
    if kind == ChartKind::Line && !kids.iter().any(|k| k.0 == "smooth") {
        let p = xml::prefix(
            open.trim_start_matches('<')
                .split([' ', '>', '/'])
                .next()
                .unwrap_or(""),
        )
        .to_owned();
        let after: Vec<&str> = kids
            .iter()
            .map(|k| k.0.as_str())
            .filter(|n| *n != "extLst")
            .collect();
        out = set_child(
            &out,
            &["smooth"],
            &format!("<{p}smooth val=\"0\"/>"),
            &after,
        );
    }
    out
}

/// A chart part with series `series` drawn as `kind` (a column, line or
/// area; `None` the chart's own) against the secondary value axis or the
/// primary: the series moved into a plot of that kind on those axes,
/// made when there is none, a plot left empty taken away, axes no plot
/// names taken away.
pub fn with_series_kind(
    text: &str,
    series: usize,
    kind: Option<ChartKind>,
    secondary: bool,
) -> Result<String, String> {
    let area = plot_area(text).ok_or("The chart has no plot area")?;
    let p = area.prefix.clone();
    let first = area.plots.first().ok_or("The chart has no plot")?.clone();
    let plot_kind = |name: &str, span: &Span<usize>| -> ChartKind {
        match name {
            "barChart" | "bar3DChart" => {
                if text[span.clone()].contains("barDir val=\"bar\"") {
                    ChartKind::Bar
                } else {
                    ChartKind::Column
                }
            }
            "lineChart" | "line3DChart" => ChartKind::Line,
            "areaChart" | "area3DChart" => ChartKind::Area,
            _ => ChartKind::Other,
        }
    };
    let main = plot_kind(&first.0, &first.1);
    if !matches!(main, ChartKind::Column | ChartKind::Line | ChartKind::Area) {
        return Err("A combo chart mixes columns, lines and areas".into());
    }
    let kind = kind.unwrap_or(main);
    if !matches!(kind, ChartKind::Column | ChartKind::Line | ChartKind::Area) {
        return Err("A series of a combo chart is a column, a line or an area".into());
    }
    let (ser_span, _) = series_at(text, series).ok_or("No such series")?;
    let from = area
        .plots
        .iter()
        .position(|pl| pl.1.start <= ser_span.start && ser_span.end <= pl.1.end)
        .ok_or("No such series")?;
    let ser_count = |pl: &Span<usize>| text[pl.clone()].matches(&format!("<{p}ser>")).count();
    if from == 0 && ser_count(&area.plots[0].1) <= 1 {
        return Err("The chart's first series keeps its kind; change another".into());
    }
    let primary = value_axis_of(&area, &first.2).cloned().unwrap_or_default();
    // The axes the target plot names: the primary ones, else a secondary
    // pair there is or is made.
    let mut new_axes = String::new();
    let ids: Vec<String> = if secondary {
        match area
            .plots
            .iter()
            .find(|pl| value_axis_of(&area, &pl.2).is_some_and(|v| *v != primary))
        {
            Some(pl) => pl.2.clone(),
            None => {
                let used: Vec<u64> = area.axes.iter().filter_map(|a| a.2.parse().ok()).collect();
                let next = used.iter().copied().max().unwrap_or(500_000_000) + 1;
                let (cat, val) = (next.to_string(), (next + 1).to_string());
                let common = format!("<{p}scaling><{p}orientation val=\"minMax\"/></{p}scaling>");
                new_axes = format!(
                    "<{p}catAx><{p}axId val=\"{cat}\"/>{common}<{p}delete val=\"1\"/><{p}axPos val=\"b\"/><{p}majorTickMark val=\"out\"/><{p}minorTickMark val=\"none\"/><{p}tickLblPos val=\"nextTo\"/><{p}crossAx val=\"{val}\"/><{p}crosses val=\"autoZero\"/><{p}auto val=\"1\"/><{p}lblAlgn val=\"ctr\"/><{p}lblOffset val=\"100\"/><{p}noMultiLvlLbl val=\"0\"/></{p}catAx><{p}valAx><{p}axId val=\"{val}\"/>{common}<{p}delete val=\"0\"/><{p}axPos val=\"r\"/><{p}numFmt formatCode=\"General\" sourceLinked=\"1\"/><{p}majorTickMark val=\"out\"/><{p}minorTickMark val=\"none\"/><{p}tickLblPos val=\"nextTo\"/><{p}crossAx val=\"{cat}\"/><{p}crosses val=\"max\"/><{p}crossBetween val=\"between\"/></{p}valAx>"
                );
                vec![cat, val]
            }
        }
    } else {
        first.2.clone()
    };
    let ser_el = cleaned_for(&text[ser_span.clone()], kind);
    // A plot of the kind on those axes, other than the one it is in.
    let target = area
        .plots
        .iter()
        .position(|pl| plot_kind(&pl.0, &pl.1) == kind && pl.2 == ids);
    if target == Some(from) {
        return Ok(text.to_owned());
    }
    // The plot area rebuilt: each plot with the series taken out or put
    // in, a new plot after the last, the new axes after the last axis.
    let pa = &text[area.span.clone()];
    let (open, _, _, kids) = open_up(pa);
    let mut out = String::from(&open[..open.find('>').map_or(0, |i| i + 1)]);
    let last_plot = kids.iter().rposition(|k| k.0.ends_with("Chart"));
    let last_axis = kids.iter().rposition(|k| k.0.ends_with("Ax"));
    let mut plot_n = 0;
    for (i, (name, span)) in kids.iter().enumerate() {
        let el = &open[span.clone()];
        if name.ends_with("Chart") {
            let mut el = el.to_owned();
            if plot_n == from {
                let rel = ser_span.start - area.span.start - span.start;
                let len = ser_span.end - ser_span.start;
                el = format!("{}{}", &el[..rel], &el[rel + len..]);
            }
            if Some(plot_n) == target {
                // After its last series, or its leading settings.
                let (inner, _, _, sub) = open_up(&el);
                let at = sub
                    .iter()
                    .rposition(|k| {
                        matches!(
                            k.0.as_str(),
                            "ser" | "varyColors" | "grouping" | "barDir" | "scatterStyle"
                        )
                    })
                    .map_or(inner.find('>').map_or(0, |x| x + 1), |k| sub[k].1.end);
                el = format!("{}{ser_el}{}", &inner[..at], &inner[at..]);
            }
            let empty = !el.contains(&format!("<{p}ser>")) && !el.contains(&format!("<{p}ser "));
            if !(empty && plot_n != 0) {
                out.push_str(&el);
            }
            if Some(i) == last_plot && target.is_none() {
                let axes: String = ids
                    .iter()
                    .map(|id| format!("<{p}axId val=\"{id}\"/>"))
                    .collect();
                out.push_str(&match kind {
                    ChartKind::Column => format!(
                        "<{p}barChart><{p}barDir val=\"col\"/><{p}grouping val=\"clustered\"/><{p}varyColors val=\"0\"/>{ser_el}<{p}gapWidth val=\"150\"/>{axes}</{p}barChart>"
                    ),
                    ChartKind::Area => format!(
                        "<{p}areaChart><{p}grouping val=\"standard\"/><{p}varyColors val=\"0\"/>{ser_el}{axes}</{p}areaChart>"
                    ),
                    _ => format!(
                        "<{p}lineChart><{p}grouping val=\"standard\"/><{p}varyColors val=\"0\"/>{ser_el}<{p}marker val=\"1\"/>{axes}</{p}lineChart>"
                    ),
                });
            }
            plot_n += 1;
            continue;
        }
        out.push_str(el);
        if Some(i) == last_axis {
            out.push_str(&new_axes);
        }
    }
    out.push_str(&open[open.rfind("</").unwrap_or(open.len())..]);
    // Axes no plot names any more taken away.
    let mut new = format!(
        "{}{out}{}",
        &text[..area.span.start],
        &text[area.span.end..]
    );
    if let Some(a2) = plot_area(&new) {
        let named: Vec<&String> = a2.plots.iter().flat_map(|pl| pl.2.iter()).collect();
        let mut gone: Vec<Span<usize>> = a2
            .axes
            .iter()
            .filter(|a| !named.contains(&&a.2))
            .map(|a| a.1.clone())
            .collect();
        gone.sort_by_key(|s| std::cmp::Reverse(s.start));
        for s in gone {
            new.replace_range(s, "");
        }
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{NewSeries, chart_xml, parse_chart};

    fn two_series() -> String {
        let s = NewSeries {
            name: Some(("Sheet1!$B$1".into(), "Sales".into())),
            cat: Some((
                "Sheet1!$A$2:$A$4".into(),
                vec!["a".into(), "b".into(), "c".into()],
                false,
            )),
            val: (
                "Sheet1!$B$2:$B$4".into(),
                vec![Some(1.0), Some(2.0), Some(3.0)],
            ),
            color: None,
        };
        chart_xml(ChartKind::Column, None, &[s.clone(), s])
    }

    #[test]
    fn combo_on_a_secondary_axis_and_back() {
        let x = two_series();
        let combo = with_series_kind(&x, 1, Some(ChartKind::Line), true).unwrap();
        let def = parse_chart(&combo, &[]);
        assert_eq!(def.plots, vec![ChartKind::Column, ChartKind::Line]);
        assert_eq!(def.secondary, vec![1]);
        assert_eq!(def.series.len(), 2);
        assert_eq!(combo.matches("<c:valAx>").count(), 2);
        // Back as a column on the primary axis: one plot, two axes.
        let back = with_series_kind(&combo, 1, None, false).unwrap();
        let def = parse_chart(&back, &[]);
        assert_eq!(def.plots, vec![ChartKind::Column]);
        assert_eq!(back.matches("Ax>").count() / 2, 2);
        assert!(with_series_kind(&x, 0, Some(ChartKind::Line), false).is_ok());
    }

    #[test]
    fn trendlines_error_bars_and_cell_labels() {
        let x = two_series();
        let t = Trendline {
            kind: TrendKind::Polynomial,
            order: 3,
            period: 0,
            equation: true,
            r_squared: true,
        };
        let y = with_trendline(&x, 0, Some(t)).unwrap();
        let b = ErrorBars {
            kind: ErrorKind::Percent,
            value: 5.0,
        };
        let y = with_error_bars(&y, 0, Some(b), false).unwrap();
        let texts = ["x".to_string(), "y".into(), "z".into()];
        let y = with_label_cells(&y, 1, Some(("Sheet1!$C$2:$C$4", &texts))).unwrap();
        let def = parse_chart(&y, &[]);
        assert_eq!(def.series[0].trendline, Some(t));
        assert_eq!(def.series[0].error_bars, Some(b));
        assert_eq!(
            def.series[1].label_cells.0.as_deref(),
            Some("Sheet1!$C$2:$C$4")
        );
        assert_eq!(def.series[1].label_cells.1, texts);
        // The trendline before the error bars, both before the categories.
        let s0 = &y[y.find("<c:ser>").unwrap()..];
        let (tl, eb, cat) = (
            s0.find("<c:trendline>").unwrap(),
            s0.find("<c:errBars>").unwrap(),
            s0.find("<c:cat>").unwrap(),
        );
        assert!(tl < eb && eb < cat);
        let none = with_label_cells(&y, 1, None).unwrap();
        let none = with_trendline(&none, 0, None).unwrap();
        let def = parse_chart(&none, &[]);
        assert_eq!(def.series[1].label_cells.0, None);
        assert_eq!(def.series[0].trendline, None);
    }
}
