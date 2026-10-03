//! Conditional formats (ECMA-376 part 1, 18.3.1.10 and 18.3.1.18): read
//! from a sheet, evaluated for its cells, and written as Excel writes them.
//!
//! A rule's formulas are written for the first cell of its range and read,
//! for another cell, moved by its offset, as a shared formula is; constant
//! ones (`100`, `"text"`) are read without the formula engine.

use std::collections::HashMap;

use crate::cellref::{CellRef, Range};
use crate::sheet::Value;
use crate::styles::{Dxf, Rgb};
use crate::workbook::Workbook;
use crate::xml::{self, Reader, Token};

/// A threshold of a color scale, data bar or icon set (`<cfvo>`).
#[derive(Debug, Clone, PartialEq)]
pub enum Cfvo {
    /// The lowest value.
    Min,
    /// The highest value.
    Max,
    /// A number.
    Num(f64),
    /// A percent of the way from the lowest to the highest.
    Percent(f64),
    /// A percentile of the values.
    Percentile(f64),
    /// A formula's value.
    Formula(String),
}

/// One rule (`<cfRule>`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CfRule {
    /// `cellIs`, `expression`, `colorScale`, …
    pub kind: String,
    /// 1 is applied first.
    pub priority: i64,
    /// The differential format it applies.
    pub dxf: Option<usize>,
    /// Lower rules stop applying where this one does.
    pub stop: bool,
    /// `greaterThan`, `between`, `containsText`, …
    pub operator: Option<String>,
    /// Its formulas, for the range's first cell.
    pub formulas: Vec<String>,
    /// The text of text rules.
    pub text: Option<String>,
    /// `top10`: how many, percent, bottom.
    pub rank: u32,
    /// See [`CfRule::rank`].
    pub percent: bool,
    /// See [`CfRule::rank`].
    pub bottom: bool,
    /// `aboveAverage`: above (true) or below.
    pub above: bool,
    /// `aboveAverage`: the average itself counts.
    pub equal_average: bool,
    /// `aboveAverage`: so many standard deviations away.
    pub std_dev: Option<f64>,
    /// A color scale's thresholds and colors.
    pub scale: Vec<(Cfvo, Rgb)>,
    /// A data bar's thresholds and color.
    pub bar: Option<(Cfvo, Cfvo, Rgb)>,
    /// An icon set's name, thresholds, and whether its order is reversed.
    pub icons: Option<(String, Vec<Cfvo>, bool)>,
}

/// A `<conditionalFormatting>`: its ranges and rules.
#[derive(Debug, Clone, PartialEq)]
pub struct CondFormat {
    /// The ranges (`sqref`).
    pub ranges: Vec<Range>,
    /// The rules.
    pub rules: Vec<CfRule>,
    /// The element's bytes.
    pub span: std::ops::Range<usize>,
}

fn cfvo(tag: &xml::Tag<'_>) -> Cfvo {
    let val = tag.attr("val").map(|v| v.into_owned()).unwrap_or_default();
    let num = val.trim().parse::<f64>().ok();
    match tag.attr("type").as_deref() {
        Some("min") => Cfvo::Min,
        Some("max") => Cfvo::Max,
        Some("num") => num.map_or(Cfvo::Formula(val), Cfvo::Num),
        Some("percent") => Cfvo::Percent(num.unwrap_or(0.0)),
        Some("percentile") => Cfvo::Percentile(num.unwrap_or(50.0)),
        _ => Cfvo::Formula(val),
    }
}

/// The conditional formats of a sheet part.
pub fn parse(text: &str, theme: &[Rgb]) -> Vec<CondFormat> {
    let mut out = Vec::new();
    let mut r = Reader::new(text);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        if tag.name != "conditionalFormatting" || tag.empty {
            continue;
        }
        let start = tag.span.start;
        let ranges: Vec<Range> = tag
            .attr("sqref")
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(Range::parse)
            .collect();
        let mut rules = Vec::new();
        let mut cur: Option<CfRule> = None;
        let mut cfvos: Vec<Cfvo> = Vec::new();
        let mut colors: Vec<Rgb> = Vec::new();
        let mut end = text.len();
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) => match tag.name {
                    "cfRule" => {
                        let flag =
                            |k: &str, d: bool| tag.attr(k).map_or(d, |v| v == "1" || v == "true");
                        let rule = CfRule {
                            kind: tag.attr("type").unwrap_or_default().into_owned(),
                            priority: tag
                                .attr("priority")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(i64::MAX),
                            dxf: tag.attr("dxfId").and_then(|v| v.parse().ok()),
                            stop: flag("stopIfTrue", false),
                            operator: tag.attr("operator").map(|v| v.into_owned()),
                            text: tag.attr("text").map(|v| v.into_owned()),
                            rank: tag.attr("rank").and_then(|v| v.parse().ok()).unwrap_or(10),
                            percent: flag("percent", false),
                            bottom: flag("bottom", false),
                            above: flag("aboveAverage", true),
                            equal_average: flag("equalAverage", false),
                            std_dev: tag.attr("stdDev").and_then(|v| v.parse().ok()),
                            ..CfRule::default()
                        };
                        if tag.empty {
                            rules.push(rule);
                        } else {
                            cur = Some(rule);
                            cfvos.clear();
                            colors.clear();
                        }
                    }
                    "formula" if !tag.empty => {
                        let (f, _) = r.text_until_end("formula");
                        if let Some(c) = cur.as_mut() {
                            c.formulas.push(f);
                        }
                    }
                    "cfvo" => cfvos.push(cfvo(&tag)),
                    "color" => colors.push(crate::styles::color(&tag, theme).unwrap_or(0)),
                    "iconSet" => {
                        if let Some(c) = cur.as_mut() {
                            let name = tag
                                .attr("iconSet")
                                .map_or("3TrafficLights1".into(), |v| v.into_owned());
                            let reverse = tag
                                .attr("reverse")
                                .as_deref()
                                .is_some_and(|v| v == "1" || v == "true");
                            c.icons = Some((name, Vec::new(), reverse));
                        }
                    }
                    _ => {}
                },
                Token::End { name: "cfRule", .. } => {
                    if let Some(mut c) = cur.take() {
                        match c.kind.as_str() {
                            "colorScale" => {
                                c.scale = cfvos.drain(..).zip(colors.drain(..)).collect()
                            }
                            "dataBar" if cfvos.len() >= 2 => {
                                c.bar = Some((
                                    cfvos[0].clone(),
                                    cfvos[1].clone(),
                                    colors.first().copied().unwrap_or(0x638EC6),
                                ));
                            }
                            "iconSet" => {
                                if let Some(i) = c.icons.as_mut() {
                                    i.1 = std::mem::take(&mut cfvos);
                                }
                            }
                            _ => {}
                        }
                        rules.push(c);
                    }
                }
                Token::End {
                    name: "conditionalFormatting",
                    span,
                } => {
                    end = span.end;
                    break;
                }
                _ => {}
            }
        }
        out.push(CondFormat {
            ranges,
            rules,
            span: start..end,
        });
    }
    out
}

/// What the conditional formats make of a cell.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CfResult {
    /// Bold.
    pub bold: Option<bool>,
    /// Italic.
    pub italic: Option<bool>,
    /// Underlined.
    pub underline: Option<bool>,
    /// Struck through.
    pub strike: Option<bool>,
    /// Text color.
    pub color: Option<Rgb>,
    /// Fill.
    pub fill: Option<Rgb>,
    /// A data bar: thousandths of the width, color.
    pub bar: Option<(u16, Rgb)>,
    /// An icon: glyph, color.
    pub icon: Option<(String, Rgb)>,
}

impl CfResult {
    fn take(&mut self, d: &Dxf) {
        self.bold = self.bold.or(d.bold);
        self.italic = self.italic.or(d.italic);
        self.underline = self.underline.or(d.underline);
        self.strike = self.strike.or(d.strike);
        self.color = self.color.or(d.color);
        self.fill = self.fill.or(d.fill);
    }
}

/// The values of a rule's range, gathered once.
#[derive(Debug, Clone, Default)]
struct Stats {
    numbers: Vec<f64>,
    counts: HashMap<String, usize>,
    mean: f64,
    sd: f64,
}

/// A sheet's conditional formats ready to evaluate, with what was computed.
#[derive(Debug, Default)]
pub struct Evaluator {
    rules: Vec<(Vec<Range>, CfRule)>,
    stats: HashMap<usize, Stats>,
    cells: HashMap<CellRef, CfResult>,
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        _ => None,
    }
}

/// A formula for the range's first cell, as read from another cell: moved
/// by the offset, and `ROW()` and `COLUMN()` made that cell's, since the
/// engine computes it elsewhere.
fn moved(formula: &str, first: CellRef, at: CellRef) -> String {
    let f = crate::formula::shift(
        formula,
        i64::from(at.row) - i64::from(first.row),
        i64::from(at.col) - i64::from(first.col),
    );
    let mut out = String::with_capacity(f.len());
    let mut quoted = false;
    let mut rest = f.as_str();
    while let Some(c) = rest.chars().next() {
        let after_name = out
            .chars()
            .last()
            .is_some_and(|p| p.is_alphanumeric() || p == '_' || p == '.');
        if !quoted && !after_name {
            let upper = rest
                .chars()
                .take(8)
                .collect::<String>()
                .to_ascii_uppercase();
            if upper.starts_with("ROW()") {
                out.push_str(&(at.row + 1).to_string());
                rest = &rest[5..];
                continue;
            }
            if upper.starts_with("COLUMN()") {
                out.push_str(&(at.col + 1).to_string());
                rest = &rest[8..];
                continue;
            }
        }
        if c == '"' {
            quoted = !quoted;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// A constant formula's value, without the engine.
fn constant(f: &str) -> Option<Value> {
    let t = f.trim();
    if let Ok(n) = t.parse::<f64>() {
        return Some(Value::Number(n));
    }
    let inner = t.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains('"') && !inner.contains("\"\"") {
        return None;
    }
    Some(Value::Text(inner.replace("\"\"", "\"")))
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let k = (p / 100.0).clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (lo, hi) = (k.floor() as usize, k.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (k - lo as f64)
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let ch = |s: u32| -> u32 {
        let (x, y) = (f64::from((a >> s) & 0xFF), f64::from((b >> s) & 0xFF));
        ((x + (y - x) * t.clamp(0.0, 1.0)).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// An icon set's glyphs and colors, lowest first.
fn icon_glyphs(name: &str) -> Vec<(&'static str, Rgb)> {
    let (red, yellow, green, gray, black) =
        (0xE0_3C_31, 0xF2_C0_00, 0x2E_A0_43, 0x80_80_80, 0x30_30_30);
    match name {
        "3Arrows" => vec![("▼", red), ("►", yellow), ("▲", green)],
        "3ArrowsGray" => vec![("▼", gray), ("►", gray), ("▲", gray)],
        "3Flags" => vec![("⚑", red), ("⚑", yellow), ("⚑", green)],
        "3Symbols" | "3Symbols2" => vec![("✖", red), ("!", yellow), ("✔", green)],
        "3Stars" => vec![("☆", yellow), ("⯪", yellow), ("★", yellow)],
        "3Triangles" => vec![("▼", red), ("▬", yellow), ("▲", green)],
        "4Arrows" => vec![("▼", red), ("↘", yellow), ("↗", yellow), ("▲", green)],
        "4ArrowsGray" => vec![("▼", gray), ("↘", gray), ("↗", gray), ("▲", gray)],
        "4RedToBlack" => vec![("●", black), ("●", gray), ("●", 0xF0_80_80), ("●", red)],
        "4Rating" => vec![("▁", gray), ("▃", gray), ("▅", gray), ("▇", gray)],
        "4TrafficLights" => vec![("●", black), ("●", red), ("●", yellow), ("●", green)],
        "5Arrows" => vec![
            ("▼", red),
            ("↘", yellow),
            ("►", yellow),
            ("↗", yellow),
            ("▲", green),
        ],
        "5ArrowsGray" => vec![
            ("▼", gray),
            ("↘", gray),
            ("►", gray),
            ("↗", gray),
            ("▲", gray),
        ],
        "5Rating" => vec![
            ("▁", gray),
            ("▂", gray),
            ("▄", gray),
            ("▆", gray),
            ("█", gray),
        ],
        "5Quarters" => vec![
            ("○", black),
            ("◔", black),
            ("◑", black),
            ("◕", black),
            ("●", black),
        ],
        "5Boxes" => vec![
            ("□", gray),
            ("▤", gray),
            ("▥", gray),
            ("▦", gray),
            ("■", gray),
        ],
        _ => vec![("●", red), ("●", yellow), ("●", green)],
    }
}

impl Evaluator {
    /// The formats of a sheet part, highest priority first.
    pub fn new(formats: Vec<CondFormat>) -> Self {
        let mut rules: Vec<(Vec<Range>, CfRule)> = formats
            .into_iter()
            .flat_map(|f| f.rules.into_iter().map(move |r| (f.ranges.clone(), r)))
            .collect();
        rules.sort_by_key(|(_, r)| r.priority);
        Self {
            rules,
            ..Self::default()
        }
    }

    /// Whether there are no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether a rule may color a cell that holds nothing.
    pub fn colors_empty(&self) -> bool {
        self.rules.iter().any(|(_, r)| {
            matches!(
                r.kind.as_str(),
                "expression"
                    | "containsBlanks"
                    | "notContainsErrors"
                    | "notContainsText"
                    | "cellIs"
            )
        })
    }

    /// Whether any rule could apply to a cell.
    pub fn touches(&self, at: CellRef) -> bool {
        self.rules
            .iter()
            .any(|(rs, _)| rs.iter().any(|r| r.contains(at)))
    }

    fn stats(&mut self, i: usize, wb: &mut Workbook, idx: usize) -> Stats {
        if let Some(s) = self.stats.get(&i) {
            return s.clone();
        }
        let ranges = self.rules[i].0.clone();
        let mut cells: Vec<CellRef> = Vec::new();
        if let Ok(sheet) = wb.sheet(idx) {
            cells = sheet
                .cells
                .keys()
                .copied()
                .filter(|p| ranges.iter().any(|r| r.contains(*p)))
                .collect();
        }
        let mut st = Stats::default();
        for at in cells {
            let v = wb.value(idx, at).unwrap_or(Value::Empty);
            if let Some(n) = number(&v) {
                st.numbers.push(n);
            }
            if v != Value::Empty {
                let shown = wb.display(idx, at).unwrap_or_default().to_lowercase();
                *st.counts.entry(shown).or_default() += 1;
            }
        }
        st.numbers
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = st.numbers.len() as f64;
        if n > 0.0 {
            st.mean = st.numbers.iter().sum::<f64>() / n;
            st.sd = (st
                .numbers
                .iter()
                .map(|x| (x - st.mean).powi(2))
                .sum::<f64>()
                / n)
                .sqrt();
        }
        self.stats.insert(i, st.clone());
        st
    }

    fn threshold(c: &Cfvo, st: &Stats, wb: &mut Workbook, idx: usize) -> f64 {
        let (min, max) = (
            st.numbers.first().copied().unwrap_or(0.0),
            st.numbers.last().copied().unwrap_or(0.0),
        );
        match c {
            Cfvo::Min => min,
            Cfvo::Max => max,
            Cfvo::Num(n) => *n,
            Cfvo::Percent(p) => min + (max - min) * p / 100.0,
            Cfvo::Percentile(p) => percentile(&st.numbers, *p),
            Cfvo::Formula(f) => match constant(f) {
                Some(Value::Number(n)) => n,
                _ => wb
                    .evaluate_formula(idx, f)
                    .ok()
                    .flatten()
                    .as_ref()
                    .and_then(number)
                    .unwrap_or(0.0),
            },
        }
    }

    /// A formula of rule `i` for cell `at`.
    fn formula_value(
        &self,
        i: usize,
        k: usize,
        at: CellRef,
        wb: &mut Workbook,
        idx: usize,
    ) -> Value {
        let (ranges, rule) = &self.rules[i];
        let Some(f) = rule.formulas.get(k) else {
            return Value::Empty;
        };
        if let Some(v) = constant(f) {
            return v;
        }
        let first = ranges.first().map_or(at, |r| r.start);
        wb.evaluate_formula(idx, &moved(f, first, at))
            .ok()
            .flatten()
            .unwrap_or(Value::Empty)
    }

    fn matches(&mut self, i: usize, at: CellRef, wb: &mut Workbook, idx: usize) -> bool {
        let value = wb.value(idx, at).unwrap_or(Value::Empty);
        let rule = self.rules[i].1.clone();
        match rule.kind.as_str() {
            "cellIs" => {
                let a = self.formula_value(i, 0, at, wb, idx);
                let b = self.formula_value(i, 1, at, wb, idx);
                compare(&value, rule.operator.as_deref().unwrap_or("equal"), &a, &b)
            }
            "expression" => truthy(&self.formula_value(i, 0, at, wb, idx)),
            "containsText" | "notContainsText" | "beginsWith" | "endsWith" => {
                let t = rule.text.clone().unwrap_or_default().to_lowercase();
                let s = wb_display(wb, idx, at).to_lowercase();
                match rule.kind.as_str() {
                    "containsText" => s.contains(&t),
                    "notContainsText" => !s.contains(&t),
                    "beginsWith" => s.starts_with(&t),
                    _ => s.ends_with(&t),
                }
            }
            "containsBlanks" => wb_display(wb, idx, at).trim().is_empty(),
            "notContainsBlanks" => !wb_display(wb, idx, at).trim().is_empty(),
            "containsErrors" => matches!(value, Value::Error(_)),
            "notContainsErrors" => !matches!(value, Value::Error(_)),
            "duplicateValues" | "uniqueValues" => {
                if value == Value::Empty {
                    return false;
                }
                let n = self
                    .stats(i, wb, idx)
                    .counts
                    .get(&wb_display(wb, idx, at).to_lowercase())
                    .copied()
                    .unwrap_or(0);
                if rule.kind == "duplicateValues" {
                    n > 1
                } else {
                    n == 1
                }
            }
            "top10" => {
                let Some(v) = number(&value) else {
                    return false;
                };
                let st = self.stats(i, wb, idx);
                let len = st.numbers.len();
                if len == 0 {
                    return false;
                }
                let n = if rule.percent {
                    ((len as f64 * f64::from(rule.rank) / 100.0).floor() as usize).max(1)
                } else {
                    rule.rank as usize
                }
                .min(len);
                if rule.bottom {
                    v <= st.numbers[n - 1]
                } else {
                    v >= st.numbers[len - n]
                }
            }
            "aboveAverage" => {
                let Some(v) = number(&value) else {
                    return false;
                };
                let st = self.stats(i, wb, idx);
                let line = st.mean
                    + rule.std_dev.unwrap_or(0.0) * st.sd * if rule.above { 1.0 } else { -1.0 };
                match (rule.above, rule.equal_average) {
                    (true, false) => v > line,
                    (true, true) => v >= line,
                    (false, false) => v < line,
                    (false, true) => v <= line,
                }
            }
            _ => false,
        }
    }

    /// What the conditional formats make of cell `at`.
    pub fn result(&mut self, at: CellRef, wb: &mut Workbook, idx: usize, dxfs: &[Dxf]) -> CfResult {
        if let Some(r) = self.cells.get(&at) {
            return r.clone();
        }
        let mut out = CfResult::default();
        for i in 0..self.rules.len() {
            if !self.rules[i].0.iter().any(|r| r.contains(at)) {
                continue;
            }
            let rule = self.rules[i].1.clone();
            match rule.kind.as_str() {
                "colorScale" => {
                    let Some(v) = number(&wb.value(idx, at).unwrap_or(Value::Empty)) else {
                        continue;
                    };
                    let st = self.stats(i, wb, idx);
                    let points: Vec<(f64, Rgb)> = rule
                        .scale
                        .iter()
                        .map(|(c, col)| (Self::threshold(c, &st, wb, idx), *col))
                        .collect();
                    if out.fill.is_none() && points.len() >= 2 {
                        let color = if v <= points[0].0 {
                            points[0].1
                        } else if v >= points[points.len() - 1].0 {
                            points[points.len() - 1].1
                        } else {
                            let k = points.windows(2).position(|w| v <= w[1].0).unwrap_or(0);
                            let (a, b) = (points[k], points[k + 1]);
                            mix(
                                a.1,
                                b.1,
                                if b.0 > a.0 {
                                    (v - a.0) / (b.0 - a.0)
                                } else {
                                    0.0
                                },
                            )
                        };
                        out.fill = Some(color);
                    }
                }
                "dataBar" => {
                    let Some(v) = number(&wb.value(idx, at).unwrap_or(Value::Empty)) else {
                        continue;
                    };
                    let Some((lo, hi, color)) = rule.bar.clone() else {
                        continue;
                    };
                    let st = self.stats(i, wb, idx);
                    let (a, b) = (
                        Self::threshold(&lo, &st, wb, idx),
                        Self::threshold(&hi, &st, wb, idx),
                    );
                    let t = if b > a {
                        ((v - a) / (b - a)).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    if out.bar.is_none() {
                        // Excel's bars start at a tenth of the width.
                        out.bar = Some(((100.0 + 900.0 * t).round() as u16, color));
                    }
                }
                "iconSet" => {
                    let Some(v) = number(&wb.value(idx, at).unwrap_or(Value::Empty)) else {
                        continue;
                    };
                    let Some((name, cfvos, reverse)) = rule.icons.clone() else {
                        continue;
                    };
                    let st = self.stats(i, wb, idx);
                    let mut k = 0;
                    for (j, c) in cfvos.iter().enumerate().skip(1) {
                        if v >= Self::threshold(c, &st, wb, idx) {
                            k = j;
                        }
                    }
                    let glyphs = icon_glyphs(&name);
                    let k = if reverse {
                        glyphs.len().saturating_sub(1 + k)
                    } else {
                        k.min(glyphs.len() - 1)
                    };
                    if out.icon.is_none() {
                        out.icon = Some((glyphs[k].0.to_owned(), glyphs[k].1));
                    }
                }
                _ => {
                    if self.matches(i, at, wb, idx) {
                        if let Some(d) = rule.dxf.and_then(|d| dxfs.get(d)) {
                            out.take(d);
                        }
                        if rule.stop {
                            break;
                        }
                    }
                }
            }
        }
        self.cells.insert(at, out.clone());
        out
    }
}

fn wb_display(wb: &mut Workbook, idx: usize, at: CellRef) -> String {
    wb.display(idx, at).unwrap_or_default()
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0,
        _ => false,
    }
}

/// A cell's value against a rule's values: numbers as numbers (an empty
/// cell is 0), text without regard to case.
fn compare(v: &Value, op: &str, a: &Value, b: &Value) -> bool {
    use std::cmp::Ordering;
    let ord = |x: &Value, y: &Value| -> Option<Ordering> {
        match (x, y) {
            (Value::Number(p), Value::Number(q)) => p.partial_cmp(q),
            (Value::Empty, Value::Number(q)) => 0f64.partial_cmp(q),
            (Value::Text(p), Value::Text(q)) => Some(p.to_lowercase().cmp(&q.to_lowercase())),
            (Value::Empty, Value::Text(q)) => Some(String::new().cmp(&q.to_lowercase())),
            (Value::Bool(p), Value::Bool(q)) => Some(p.cmp(q)),
            // Text is greater than any number, as Excel compares them.
            (Value::Text(_), Value::Number(_)) => Some(Ordering::Greater),
            (Value::Number(_), Value::Text(_)) => Some(Ordering::Less),
            _ => None,
        }
    };
    let Some(o) = ord(v, a) else { return false };
    match op {
        "greaterThan" => o.is_gt(),
        "lessThan" => o.is_lt(),
        "greaterThanOrEqual" => o.is_ge(),
        "lessThanOrEqual" => o.is_le(),
        "notEqual" => o.is_ne(),
        "between" | "notBetween" => {
            let Some(o2) = ord(v, b) else { return false };
            // Either order of the two bounds.
            let inside = (o.is_ge() && o2.is_le()) || (o.is_le() && o2.is_ge());
            inside == (op == "between")
        }
        _ => o.is_eq(),
    }
}

/// A rule as `<cfRule>`, for a range whose first cell is `first`.
pub fn rule_xml(
    p: &str,
    rule: &kalem_viewer::CondRule,
    dxf: Option<u32>,
    first: CellRef,
) -> String {
    use kalem_viewer::{CompareOp, CondRule};
    let dxf = dxf.map_or(String::new(), |d| format!(" dxfId=\"{d}\""));
    let rgb = |c: &[u8; 3]| format!("FF{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    // A value as typed: a formula after `=`, a number, else text.
    let value = |v: &str| -> String {
        let t = v.trim();
        if let Some(f) = t.strip_prefix('=') {
            f.to_owned()
        } else if t.parse::<f64>().is_ok() {
            t.to_owned()
        } else {
            format!("\"{}\"", t.replace('"', "\"\""))
        }
    };
    let f = |s: &str| format!("<{p}formula>{}</{p}formula>", xml::escape(s));
    match rule {
        CondRule::Compare {
            op,
            value: v1,
            value2,
        } => {
            let name = match op {
                CompareOp::Greater => "greaterThan",
                CompareOp::Less => "lessThan",
                CompareOp::GreaterOrEqual => "greaterThanOrEqual",
                CompareOp::LessOrEqual => "lessThanOrEqual",
                CompareOp::Equal => "equal",
                CompareOp::NotEqual => "notEqual",
                CompareOp::Between => "between",
                CompareOp::NotBetween => "notBetween",
            };
            let mut body = f(&value(v1));
            if let Some(v2) = value2 {
                body.push_str(&f(&value(v2)));
            }
            format!(
                "<{p}cfRule type=\"cellIs\"{dxf} priority=\"1\" operator=\"{name}\">{body}</{p}cfRule>"
            )
        }
        CondRule::TextContains(t) => {
            let quoted = t.replace('"', "\"\"");
            format!(
                "<{p}cfRule type=\"containsText\"{dxf} priority=\"1\" operator=\"containsText\" text=\"{}\">{}</{p}cfRule>",
                xml::escape(t),
                f(&format!("NOT(ISERROR(SEARCH(\"{quoted}\",{first})))"))
            )
        }
        CondRule::Duplicates => {
            format!("<{p}cfRule type=\"duplicateValues\"{dxf} priority=\"1\"/>")
        }
        CondRule::Unique => format!("<{p}cfRule type=\"uniqueValues\"{dxf} priority=\"1\"/>"),
        CondRule::Top {
            count,
            bottom,
            percent,
        } => format!(
            "<{p}cfRule type=\"top10\"{dxf} priority=\"1\" rank=\"{count}\"{}{}/>",
            if *percent { " percent=\"1\"" } else { "" },
            if *bottom { " bottom=\"1\"" } else { "" }
        ),
        CondRule::Average { below } => format!(
            "<{p}cfRule type=\"aboveAverage\"{dxf} priority=\"1\"{}/>",
            if *below { " aboveAverage=\"0\"" } else { "" }
        ),
        CondRule::Formula(text) => format!(
            "<{p}cfRule type=\"expression\"{dxf} priority=\"1\">{}</{p}cfRule>",
            f(text.trim_start_matches('='))
        ),
        CondRule::ColorScale(colors) => {
            let mid = if colors.len() >= 3 {
                format!("<{p}cfvo type=\"percentile\" val=\"50\"/>")
            } else {
                String::new()
            };
            let cs: String = colors
                .iter()
                .take(3)
                .map(|c| format!("<{p}color rgb=\"{}\"/>", rgb(c)))
                .collect();
            format!(
                "<{p}cfRule type=\"colorScale\" priority=\"1\"><{p}colorScale><{p}cfvo type=\"min\"/>{mid}<{p}cfvo type=\"max\"/>{cs}</{p}colorScale></{p}cfRule>"
            )
        }
        CondRule::DataBar(c) => format!(
            "<{p}cfRule type=\"dataBar\" priority=\"1\"><{p}dataBar><{p}cfvo type=\"min\"/><{p}cfvo type=\"max\"/><{p}color rgb=\"{}\"/></{p}dataBar></{p}cfRule>",
            rgb(c)
        ),
        CondRule::IconSet(name) => {
            let n = name
                .chars()
                .next()
                .and_then(|c| c.to_digit(10))
                .unwrap_or(3)
                .clamp(3, 5);
            let cfvos: String = (0..n)
                .map(|k| {
                    format!(
                        "<{p}cfvo type=\"percent\" val=\"{}\"/>",
                        (k * 100).div_ceil(n)
                    )
                })
                .collect();
            format!(
                "<{p}cfRule type=\"iconSet\" priority=\"1\"><{p}iconSet iconSet=\"{}\">{cfvos}</{p}iconSet></{p}cfRule>",
                xml::escape(name)
            )
        }
    }
}

/// A `<dxf>` for a highlighting style.
pub fn dxf_xml(style: &kalem_viewer::CondStyle) -> String {
    let rgb = |c: &[u8; 3]| format!("FF{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    let mut font = String::new();
    if style.bold {
        font.push_str("<b/>");
    }
    if let Some(c) = &style.color {
        font.push_str(&format!("<color rgb=\"{}\"/>", rgb(c)));
    }
    let font = if font.is_empty() {
        String::new()
    } else {
        format!("<font>{font}</font>")
    };
    let fill = style.fill.map_or(String::new(), |c| {
        format!(
            "<fill><patternFill><bgColor rgb=\"{}\"/></patternFill></fill>",
            rgb(&c)
        )
    });
    format!("<dxf>{font}{fill}</dxf>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_read() {
        let sheet = r#"<worksheet><sheetData/><conditionalFormatting sqref="B2:B5 D2"><cfRule type="cellIs" dxfId="0" priority="2" operator="greaterThan"><formula>100</formula></cfRule><cfRule type="colorScale" priority="1"><colorScale><cfvo type="min"/><cfvo type="max"/><color rgb="FFF8696B"/><color rgb="FF63BE7B"/></colorScale></cfRule></conditionalFormatting><conditionalFormatting sqref="C2:C5"><cfRule type="iconSet" priority="3"><iconSet iconSet="3Arrows"><cfvo type="percent" val="0"/><cfvo type="percent" val="33"/><cfvo type="percent" val="67"/></iconSet></cfRule></conditionalFormatting></worksheet>"#;
        let f = parse(sheet, &[]);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].ranges.len(), 2);
        assert_eq!(f[0].rules[0].formulas, ["100"]);
        assert_eq!(
            f[0].rules[1].scale,
            vec![(Cfvo::Min, 0xF8696B), (Cfvo::Max, 0x63BE7B)]
        );
        let icons = f[1].rules[0].icons.clone().unwrap();
        assert_eq!((icons.0.as_str(), icons.1.len()), ("3Arrows", 3));
    }

    #[test]
    fn comparisons_and_colors() {
        let n = Value::Number;
        assert!(compare(&n(150.0), "greaterThan", &n(100.0), &Value::Empty));
        assert!(compare(&n(5.0), "between", &n(10.0), &n(1.0)));
        assert!(compare(
            &Value::Text("Food".into()),
            "equal",
            &Value::Text("food".into()),
            &Value::Empty
        ));
        assert!(compare(&Value::Empty, "lessThan", &n(5.0), &Value::Empty));
        assert_eq!(mix(0x000000, 0xFFFFFF, 0.5), 0x808080);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 50.0), 2.5);
        assert_eq!(constant("\"a\"\"b\""), Some(Value::Text("a\"b".into())));
        assert_eq!(constant("A1"), None);
        let (a1, c4) = (CellRef::new(0, 0), CellRef::new(3, 2));
        assert_eq!(moved("MOD(ROW(),2)=0", a1, c4), "MOD(4,2)=0");
        assert_eq!(moved("A1>\"ROW()\"", a1, c4), "C4>\"ROW()\"");
    }
}
