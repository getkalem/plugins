//! Pivot tables (ECMA-376 part 1, 18.10): computed from a source range and
//! written as Excel writes them — a cache definition, its records, the
//! table's definition, and its cells. Fields grouped (dates by month,
//! quarter, year or day, numbers in steps), calculated fields and items,
//! filters (items, labels, values, top 10), sorts by label or value,
//! values shown as percents, running totals or differences, the compact,
//! outline and tabular layouts with or without subtotals and grand
//! totals; all kept in the parts as Excel keeps them. The cache asks to be
//! refreshed on load, so Excel and LibreOffice compute it again themselves.

use std::collections::{HashMap, HashSet};

use kalem_viewer::{
    Aggregate, CalculatedField, CalculatedItem, GroupBy, PivotFilter, PivotFilterKind, PivotGroup,
    PivotSort, ReportForm, ShowAs,
};

use crate::cellref::{CellRef, Range};
use crate::sheet::Value;
use crate::xml;

/// The source: field names, and each record's values and their text.
#[derive(Debug, Clone, Default)]
pub struct Source {
    /// The fields' names (the first row).
    pub names: Vec<String>,
    /// The records' values.
    pub rows: Vec<Vec<Value>>,
    /// The records' values as shown, for item labels.
    pub shown: Vec<Vec<String>>,
}

/// A value field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Val {
    /// The field (a calculated one after the source's).
    pub field: usize,
    /// How it is summarized.
    pub agg: Aggregate,
    /// How the summaries show.
    pub show_as: ShowAs,
    /// The base field of a running total or a difference.
    pub base_field: usize,
    /// The base item of a difference; empty for the previous one.
    pub base_item: String,
}

/// What a pivot table shows, by source field index.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Row fields, outermost first.
    pub rows: Vec<usize>,
    /// Column fields: none or one.
    pub cols: Vec<usize>,
    /// Value fields.
    pub values: Vec<Val>,
    /// Fields grouped.
    pub groups: Vec<PivotGroup>,
    /// Calculated fields, counted after the source's.
    pub calculated: Vec<CalculatedField>,
    /// Calculated items.
    pub calculated_items: Vec<CalculatedItem>,
    /// Sorts.
    pub sorts: Vec<PivotSort>,
    /// Filters.
    pub filters: Vec<PivotFilter>,
    /// The report layout.
    pub form: ReportForm,
    /// Subtotals shown.
    pub subtotals: bool,
    /// Grand totals shown.
    pub grand_totals: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            rows: Vec::new(),
            cols: Vec::new(),
            values: Vec::new(),
            groups: Vec::new(),
            calculated: Vec::new(),
            calculated_items: Vec::new(),
            sorts: Vec::new(),
            filters: Vec::new(),
            form: ReportForm::Compact,
            subtotals: true,
            grand_totals: true,
        }
    }
}

impl Layout {
    /// Row, column and value fields, the rest as a new table has it.
    pub fn simple(rows: Vec<usize>, cols: Vec<usize>, values: Vec<(usize, Aggregate)>) -> Self {
        Layout {
            rows,
            cols,
            values: values
                .into_iter()
                .map(|(field, agg)| Val {
                    field,
                    agg,
                    ..Val::default()
                })
                .collect(),
            ..Layout::default()
        }
    }

    fn group_of(&self, f: usize) -> Option<&PivotGroup> {
        self.groups.iter().find(|g| g.field as usize == f)
    }

    fn sort_of(&self, f: usize) -> Option<&PivotSort> {
        self.sorts.iter().find(|s| s.field as usize == f)
    }
}

/// A value an item stands for: text without regard to case, as Excel
/// groups it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Number(u64),
    Text(String),
    Bool(bool),
    Error(String),
    Blank,
}

fn key(v: &Value) -> Key {
    match v {
        Value::Number(n) => Key::Number((n + 0.0).to_bits()),
        Value::Text(t) => Key::Text(t.to_lowercase()),
        Value::Bool(b) => Key::Bool(*b),
        Value::Error(e) => Key::Error(e.clone()),
        _ => Key::Blank,
    }
}

/// A field's distinct values: in the order first met (the cache's shared
/// items), and their sorted order (the table's items).
#[derive(Debug, Clone, Default)]
pub struct Items {
    /// Each value and its label.
    pub shared: Vec<(Value, String)>,
    /// Shared indexes, ascending: numbers, text, logical values, errors,
    /// then the blank.
    pub order: Vec<usize>,
    /// Each shared index's place in `order`.
    pub place: Vec<usize>,
    /// Each record's shared index.
    pub of_record: Vec<usize>,
    /// A grouped field's grouping: its `rangePr` attributes.
    pub group: Option<String>,
}

impl Items {
    fn label(&self, place: usize) -> &str {
        &self.shared[self.order[place]].1
    }

    /// The place of the item labeled `label`.
    pub fn place_of(&self, label: &str) -> Option<usize> {
        (0..self.order.len()).find(|&p| self.label(p).eq_ignore_ascii_case(label.trim()))
    }
}

fn rank(v: &Value) -> u8 {
    match v {
        Value::Number(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        Value::Error(_) => 3,
        _ => 4,
    }
}

fn items(src: &Source, field: usize) -> Items {
    let mut map: HashMap<Key, usize> = HashMap::new();
    let mut out = Items::default();
    for (i, row) in src.rows.iter().enumerate() {
        let v = row.get(field).cloned().unwrap_or(Value::Empty);
        let k = key(&v);
        let idx = *map.entry(k).or_insert_with(|| {
            let label = src.shown[i].get(field).cloned().unwrap_or_default();
            out.shared.push((v.clone(), label));
            out.shared.len() - 1
        });
        out.of_record.push(idx);
    }
    let mut order: Vec<usize> = (0..out.shared.len()).collect();
    order.sort_by(|&a, &b| {
        let (va, vb) = (&out.shared[a].0, &out.shared[b].0);
        rank(va).cmp(&rank(vb)).then_with(|| match (va, vb) {
            (Value::Number(x), Value::Number(y)) => {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Value::Text(x), Value::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        })
    });
    out.place = vec![0; order.len()];
    for (p, &i) in order.iter().enumerate() {
        out.place[i] = p;
    }
    out.order = order;
    out
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const LEAP_MONTH_DAYS: [u32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn iso(serial: f64) -> String {
    let (y, m, d, _) = crate::numfmt::serial_to_date(serial.floor() as i64, false);
    format!("{y:04}-{m:02}-{d:02}T00:00:00")
}

fn short_date(serial: f64) -> String {
    let (y, m, d, _) = crate::numfmt::serial_to_date(serial.floor() as i64, false);
    format!("{m}/{d}/{y}")
}

fn num(n: f64) -> String {
    format!("{n}")
}

/// A grouped field's items, as Excel lists them in its `groupItems`: a
/// first item for the values before the start and a last for those after
/// the end, the groups between; each record's group.
fn group_items(src: &Source, field: usize, g: &PivotGroup) -> Items {
    let values: Vec<Option<f64>> = src
        .rows
        .iter()
        .map(|r| match r.get(field) {
            Some(Value::Number(n)) => Some(*n),
            _ => None,
        })
        .collect();
    let lo = values
        .iter()
        .flatten()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let hi = values
        .iter()
        .flatten()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let (lo, hi) = if lo.is_finite() { (lo, hi) } else { (0.0, 0.0) };
    let mut labels: Vec<String> = Vec::new();
    let of: Box<dyn Fn(f64) -> usize>;
    let range_pr: String;
    match g.by {
        GroupBy::Step => {
            let step = if g.step > 0.0 { g.step } else { 1.0 };
            let start = g.start.unwrap_or(lo);
            let end = g.end.unwrap_or(hi);
            let n = (((end - start) / step).floor() as usize + 1).min(10_000);
            let whole = start.fract() == 0.0 && step.fract() == 0.0;
            labels.push(format!("<{}", num(start)));
            for k in 0..n {
                let a = start + step * k as f64;
                let b = if whole { a + step - 1.0 } else { a + step };
                labels.push(format!("{}-{}", num(a), num(b)));
            }
            labels.push(format!(">{}", num(start + step * n as f64)));
            of = Box::new(move |v| {
                if v < start {
                    0
                } else if v >= start + step * n as f64 {
                    n + 1
                } else {
                    ((v - start) / step).floor() as usize + 1
                }
            });
            range_pr = format!(
                "autoStart=\"{}\" autoEnd=\"{}\" startNum=\"{}\" endNum=\"{}\" groupInterval=\"{}\"",
                u8::from(g.start.is_none()),
                u8::from(g.end.is_none()),
                num(start),
                num(end),
                num(step)
            );
        }
        by => {
            let start = g.start.unwrap_or(lo).floor();
            let end = g.end.unwrap_or(hi).floor() + 1.0;
            labels.push(format!("<{}", short_date(start)));
            let date = |v: f64| crate::numfmt::serial_to_date(v.floor() as i64, false);
            let (y0, ..) = date(start);
            let (y1, ..) = date(end - 1.0);
            let what = match by {
                GroupBy::Months => {
                    labels.extend(MONTHS.iter().map(|m| (*m).to_owned()));
                    "months"
                }
                GroupBy::Quarters => {
                    labels.extend((1..=4).map(|q| format!("Qtr{q}")));
                    "quarters"
                }
                GroupBy::Years => {
                    labels.extend((y0..=y1).map(|y| y.to_string()));
                    "years"
                }
                _ => {
                    // Every day of a leap year, as Excel lists them.
                    for (m, days) in LEAP_MONTH_DAYS.iter().enumerate() {
                        for d in 1..=*days {
                            labels.push(format!("{d}-{}", MONTHS[m]));
                        }
                    }
                    "days"
                }
            };
            let last = labels.len();
            labels.push(format!(">{}", short_date(end)));
            of = Box::new(move |v| {
                if v < start {
                    return 0;
                }
                if v >= end {
                    return last;
                }
                let (y, m, d, _) = date(v);
                match by {
                    GroupBy::Months => m as usize,
                    GroupBy::Quarters => (m as usize - 1) / 3 + 1,
                    GroupBy::Years => (y - y0) as usize + 1,
                    _ => {
                        let before: u32 = LEAP_MONTH_DAYS.iter().take(m as usize - 1).sum();
                        (before + d) as usize
                    }
                }
            });
            range_pr = format!(
                "groupBy=\"{what}\" startDate=\"{}\" endDate=\"{}\"",
                iso(start),
                iso(end)
            );
        }
    }
    let mut out = Items {
        shared: labels
            .iter()
            .map(|l| (Value::Text(l.clone()), l.clone()))
            .collect(),
        group: Some(range_pr),
        ..Items::default()
    };
    out.order = (0..out.shared.len()).collect();
    out.place = out.order.clone();
    // A record without a number falls before the start.
    out.of_record = values.iter().map(|v| v.map_or(0, &of)).collect();
    out
}

/// A formula of names (`Sales * 0.1`, `'Unit Price' - Cost`) evaluated
/// with `name` giving each name's number.
pub fn eval(formula: &str, name: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
    struct P<'a> {
        s: Vec<char>,
        i: usize,
        name: &'a dyn Fn(&str) -> Option<f64>,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.s.get(self.i).is_some_and(|c| c.is_whitespace()) {
                self.i += 1;
            }
        }
        fn expr(&mut self) -> Result<f64, String> {
            let mut v = self.term()?;
            loop {
                self.ws();
                match self.s.get(self.i) {
                    Some('+') => {
                        self.i += 1;
                        v += self.term()?;
                    }
                    Some('-') => {
                        self.i += 1;
                        v -= self.term()?;
                    }
                    _ => return Ok(v),
                }
            }
        }
        fn term(&mut self) -> Result<f64, String> {
            let mut v = self.power()?;
            loop {
                self.ws();
                match self.s.get(self.i) {
                    Some('*') => {
                        self.i += 1;
                        v *= self.power()?;
                    }
                    Some('/') => {
                        self.i += 1;
                        let d = self.power()?;
                        if d == 0.0 {
                            return Err("#DIV/0!".into());
                        }
                        v /= d;
                    }
                    _ => return Ok(v),
                }
            }
        }
        fn power(&mut self) -> Result<f64, String> {
            let b = self.unary()?;
            self.ws();
            if self.s.get(self.i) == Some(&'^') {
                self.i += 1;
                return Ok(b.powf(self.power()?));
            }
            Ok(b)
        }
        fn unary(&mut self) -> Result<f64, String> {
            self.ws();
            match self.s.get(self.i) {
                Some('-') => {
                    self.i += 1;
                    Ok(-self.unary()?)
                }
                Some('+') => {
                    self.i += 1;
                    self.unary()
                }
                _ => self.atom(),
            }
        }
        fn atom(&mut self) -> Result<f64, String> {
            self.ws();
            match self.s.get(self.i).copied() {
                Some('(') => {
                    self.i += 1;
                    let v = self.expr()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&')') {
                        return Err("#NAME?".into());
                    }
                    self.i += 1;
                    Ok(v)
                }
                Some(c) if c.is_ascii_digit() || c == '.' => {
                    let start = self.i;
                    while self
                        .s
                        .get(self.i)
                        .is_some_and(|c| c.is_ascii_digit() || *c == '.')
                    {
                        self.i += 1;
                    }
                    let t: String = self.s[start..self.i].iter().collect();
                    t.parse().map_err(|_| "#NAME?".to_string())
                }
                Some('\'') => {
                    self.i += 1;
                    let start = self.i;
                    while self.s.get(self.i).is_some_and(|c| *c != '\'') {
                        self.i += 1;
                    }
                    let n: String = self.s[start..self.i].iter().collect();
                    self.i += 1;
                    (self.name)(&n).ok_or_else(|| "#NAME?".to_string())
                }
                Some(c) if c.is_alphanumeric() || c == '_' => {
                    let start = self.i;
                    while self
                        .s
                        .get(self.i)
                        .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ' '))
                    {
                        self.i += 1;
                    }
                    let n: String = self.s[start..self.i].iter().collect();
                    (self.name)(n.trim()).ok_or_else(|| "#NAME?".to_string())
                }
                _ => Err("#NAME?".into()),
            }
        }
    }
    let f = formula.trim().trim_start_matches('=');
    let mut p = P {
        s: f.chars().collect(),
        i: 0,
        name,
    };
    let v = p.expr()?;
    p.ws();
    if p.i != p.s.len() {
        return Err("#NAME?".into());
    }
    Ok(v)
}

/// The names a formula refers to.
fn names_in(formula: &str, known: &[String]) -> Vec<usize> {
    let lower = formula.to_lowercase();
    known
        .iter()
        .enumerate()
        .filter(|(_, n)| lower.contains(&n.to_lowercase()))
        .map(|(i, _)| i)
        .collect()
}

/// A cell of the table.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// A label.
    Text(String),
    /// A summary.
    Number(f64),
    /// A summary shown as a percent.
    Percent(f64),
    /// A summary that cannot be computed (`#DIV/0!`).
    Error(String),
}

fn out_of(r: Result<f64, String>) -> Out {
    match r {
        Ok(x) => Out::Number(x),
        Err(e) => Out::Error(e),
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Acc {
    sum: f64,
    count: usize,
    numbers: usize,
    max: f64,
    min: f64,
}

impl Acc {
    fn add(&mut self, v: &Value) {
        if matches!(v, Value::Empty) {
            return;
        }
        self.count += 1;
        if let Value::Number(n) = v {
            if self.numbers == 0 {
                (self.max, self.min) = (*n, *n);
            } else {
                self.max = self.max.max(*n);
                self.min = self.min.min(*n);
            }
            self.numbers += 1;
            self.sum += n;
        }
    }

    fn get(&self, agg: Aggregate) -> Result<f64, String> {
        match agg {
            Aggregate::Sum => Ok(self.sum),
            Aggregate::Count => Ok(self.count as f64),
            Aggregate::Average if self.numbers == 0 => Err("#DIV/0!".into()),
            Aggregate::Average => Ok(self.sum / self.numbers as f64),
            Aggregate::Max => Ok(if self.numbers == 0 { 0.0 } else { self.max }),
            Aggregate::Min => Ok(if self.numbers == 0 { 0.0 } else { self.min }),
        }
    }
}

/// A value field's caption, as Excel names it.
pub fn caption(name: &str, agg: Aggregate) -> String {
    let what = match agg {
        Aggregate::Sum => "Sum",
        Aggregate::Count => "Count",
        Aggregate::Average => "Average",
        Aggregate::Max => "Max",
        Aggregate::Min => "Min",
    };
    format!("{what} of {name}")
}

/// A row of the table.
#[derive(Debug, Clone, PartialEq)]
pub enum RowLine {
    /// An item: its path of places, outermost first.
    Item(Vec<usize>),
    /// A tabular layout's subtotal of an item (its path).
    Subtotal(Vec<usize>),
    /// A calculated item of the outermost row field.
    Calc(usize),
    /// The grand total.
    Grand,
}

/// A column of the table's values (with a column field).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColLine {
    /// An item's place.
    Item(usize),
    /// A calculated item of the column field.
    Calc(usize),
    /// The grand total.
    Grand,
}

/// A computed pivot table.
#[derive(Debug, Clone)]
pub struct Pivot {
    /// What it shows.
    pub layout: Layout,
    /// The fields' names, the source's then the calculated ones.
    pub names: Vec<String>,
    /// The items of each row and column field.
    pub items: HashMap<usize, Items>,
    /// The rows.
    pub rows: Vec<RowLine>,
    /// The value columns, with a column field.
    pub cols: Vec<ColLine>,
    /// The cells, from the table's first cell.
    pub cells: Vec<Vec<Option<Out>>>,
    /// The rows above the first row of items.
    pub header_rows: usize,
    /// The columns of labels before the values.
    pub label_cols: usize,
}

type Sums = HashMap<(Vec<usize>, Option<usize>), Vec<Acc>>;
type Summary = Option<Result<f64, String>>;

/// The rows, depth first: in the tabular layout only the innermost items
/// and the subtotals after their items.
fn walk(
    prefix: &mut Vec<usize>,
    tree: &HashMap<Vec<usize>, Vec<usize>>,
    depth: usize,
    tabular: bool,
    subtotals: bool,
    out: &mut Vec<RowLine>,
) {
    let kids = tree.get(prefix.as_slice()).cloned().unwrap_or_default();
    for c in kids {
        prefix.push(c);
        let leaf = prefix.len() == depth;
        if !tabular || leaf {
            out.push(RowLine::Item(prefix.clone()));
        }
        if !leaf {
            walk(prefix, tree, depth, tabular, subtotals, out);
            if tabular && subtotals {
                out.push(RowLine::Subtotal(prefix.clone()));
            }
        }
        prefix.pop();
    }
}

/// Computes a pivot table; `Err` with what is wrong with the layout.
pub fn compute(src: &Source, layout: &Layout) -> Result<Pivot, String> {
    let l = layout;
    let n_src = src.names.len();
    let mut names = src.names.clone();
    names.extend(l.calculated.iter().map(|c| c.name.clone()));
    if l.rows.is_empty() {
        return Err("A pivot table needs a row field".into());
    }
    if l.values.is_empty() {
        return Err("A pivot table needs a value field".into());
    }
    if l.cols.len() > 1 || (!l.cols.is_empty() && l.values.len() > 1) {
        return Err("Kalem's pivot tables take one column field, with one value field".into());
    }
    let mut axis: Vec<usize> = l.rows.clone();
    axis.extend(&l.cols);
    let mut seen = HashSet::new();
    if !axis.iter().all(|f| seen.insert(*f)) {
        return Err("A field is used twice".into());
    }
    if axis.iter().any(|&f| f >= n_src) {
        return Err("A calculated field is a value field only".into());
    }
    if l.values.iter().any(|v| v.field >= names.len()) {
        return Err("No such field".into());
    }
    let items: HashMap<usize, Items> = axis
        .iter()
        .map(|&f| {
            (
                f,
                match l.group_of(f) {
                    Some(g) => group_items(src, f, g),
                    None => items(src, f),
                },
            )
        })
        .collect();
    let place = |f: usize, rec: usize| items[&f].place[items[&f].of_record[rec]];
    let paths: Vec<Vec<usize>> = (0..src.rows.len())
        .map(|i| l.rows.iter().map(|&f| place(f, i)).collect())
        .collect();
    let col_of = |i: usize| l.cols.first().map(|&f| place(f, i));
    // The source fields summed: the value fields' and those calculated
    // fields name.
    let mut used: Vec<usize> = Vec::new();
    for v in &l.values {
        if v.field < n_src {
            used.push(v.field);
        } else {
            used.extend(names_in(&l.calculated[v.field - n_src].formula, &src.names));
        }
    }
    used.sort_unstable();
    used.dedup();
    // Records hidden by the item and label filters.
    let mut included = vec![true; src.rows.len()];
    for flt in &l.filters {
        let f = flt.field as usize;
        let Some(it) = items.get(&f) else { continue };
        let text = flt.text.to_lowercase();
        for (i, inc) in included.iter_mut().enumerate() {
            let label = it.shared[it.of_record[i]].1.to_lowercase();
            let keep = match flt.kind {
                PivotFilterKind::Items => !flt.hidden.iter().any(|h| h.to_lowercase() == label),
                PivotFilterKind::LabelEquals => label == text,
                PivotFilterKind::LabelBegins => label.starts_with(&text),
                PivotFilterKind::LabelContains => label.contains(&text),
                PivotFilterKind::LabelNotContains => !label.contains(&text),
                _ => true,
            };
            *inc &= keep;
        }
    }
    let sum_up = |included: &[bool]| -> Sums {
        let mut acc: Sums = HashMap::new();
        for (i, path) in paths.iter().enumerate() {
            if !included[i] {
                continue;
            }
            let cols: Vec<Option<usize>> = match col_of(i) {
                Some(c) => vec![Some(c), None],
                None => vec![None],
            };
            for d in 0..=path.len() {
                for c in &cols {
                    let a = acc
                        .entry((path[..d].to_vec(), *c))
                        .or_insert_with(|| vec![Acc::default(); used.len()]);
                    for (k, f) in used.iter().enumerate() {
                        a[k].add(src.rows[i].get(*f).unwrap_or(&Value::Empty));
                    }
                }
            }
        }
        acc
    };
    // A value field's summary of the records at `prefix` and `col`.
    let raw_with = |acc: &Sums, prefix: &[usize], col: Option<usize>, k: usize| -> Summary {
        let a = acc.get(&(prefix.to_vec(), col))?;
        let v = &l.values[k];
        if v.field < n_src {
            let at = used.iter().position(|f| *f == v.field)?;
            return Some(a[at].get(v.agg));
        }
        let calc = &l.calculated[v.field - n_src];
        Some(eval(&calc.formula, &|name: &str| {
            let f = src
                .names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(name))?;
            let at = used.iter().position(|u| *u == f)?;
            Some(a[at].sum)
        }))
    };
    let mut acc = sum_up(&included);
    // The value and top filters: within each parent, the items kept.
    for flt in &l.filters {
        let f = flt.field as usize;
        let k = (flt.value as usize).min(l.values.len() - 1);
        if matches!(
            flt.kind,
            PivotFilterKind::Items
                | PivotFilterKind::LabelEquals
                | PivotFilterKind::LabelBegins
                | PivotFilterKind::LabelContains
                | PivotFilterKind::LabelNotContains
        ) {
            continue;
        }
        type Cand = (Vec<usize>, Option<usize>, f64);
        let mut groups: HashMap<Vec<usize>, Vec<Cand>> = HashMap::new();
        if let Some(d) = l.rows.iter().position(|r| *r == f) {
            let mut done = HashSet::new();
            for (i, p) in paths.iter().enumerate() {
                if included[i] && done.insert(p[..=d].to_vec()) {
                    let v = raw_with(&acc, &p[..=d], None, k)
                        .and_then(Result::ok)
                        .unwrap_or(0.0);
                    groups
                        .entry(p[..d].to_vec())
                        .or_default()
                        .push((p[..=d].to_vec(), None, v));
                }
            }
        } else if l.cols.first() == Some(&f) {
            let mut done = HashSet::new();
            for (i, inc) in included.iter().enumerate() {
                if let Some(c) = col_of(i).filter(|c| *inc && done.insert(*c)) {
                    let v = raw_with(&acc, &[], Some(c), k)
                        .and_then(Result::ok)
                        .unwrap_or(0.0);
                    groups
                        .entry(Vec::new())
                        .or_default()
                        .push((Vec::new(), Some(c), v));
                }
            }
        }
        let mut drop_rows: HashSet<Vec<usize>> = HashSet::new();
        let mut drop_cols: HashSet<usize> = HashSet::new();
        for (_, mut cands) in groups {
            let keep: Vec<bool> = match flt.kind {
                PivotFilterKind::ValueGreater => cands.iter().map(|c| c.2 > flt.number).collect(),
                PivotFilterKind::ValueLess => cands.iter().map(|c| c.2 < flt.number).collect(),
                PivotFilterKind::ValueEquals => cands
                    .iter()
                    .map(|c| (c.2 - flt.number).abs() < 1e-9)
                    .collect(),
                kind => {
                    let top = matches!(kind, PivotFilterKind::Top | PivotFilterKind::TopPercent);
                    cands.sort_by(|a, b| {
                        let o = a.2.total_cmp(&b.2);
                        if top { o.reverse() } else { o }
                    });
                    if matches!(kind, PivotFilterKind::Top | PivotFilterKind::Bottom) {
                        let n = flt.number.max(0.0) as usize;
                        (0..cands.len()).map(|i| i < n).collect()
                    } else {
                        let total: f64 = cands.iter().map(|c| c.2).sum();
                        let mut run = 0.0;
                        cands
                            .iter()
                            .map(|c| {
                                let keep = run < total * flt.number / 100.0;
                                run += c.2;
                                keep
                            })
                            .collect()
                    }
                }
            };
            for (c, k) in cands.iter().zip(keep) {
                if !k {
                    match c.1 {
                        Some(col) => {
                            drop_cols.insert(col);
                        }
                        None => {
                            drop_rows.insert(c.0.clone());
                        }
                    }
                }
            }
        }
        for (i, p) in paths.iter().enumerate() {
            let row_gone = (1..=p.len()).any(|d| drop_rows.contains(&p[..d]));
            let col_gone = col_of(i).is_some_and(|c| drop_cols.contains(&c));
            if row_gone || col_gone {
                included[i] = false;
            }
        }
        acc = sum_up(&included);
    }
    let raw = |prefix: &[usize], col: Option<usize>, k: usize| raw_with(&acc, prefix, col, k);
    // The children of a row prefix, in their field's sort.
    let mut tree: HashMap<Vec<usize>, Vec<usize>> = HashMap::new();
    for (i, p) in paths.iter().enumerate() {
        if !included[i] {
            continue;
        }
        for d in 0..p.len() {
            let kids = tree.entry(p[..d].to_vec()).or_default();
            if !kids.contains(&p[d]) {
                kids.push(p[d]);
            }
        }
    }
    let sorted =
        |f: usize, mut kids: Vec<usize>, value_of: &dyn Fn(usize, usize) -> f64| -> Vec<usize> {
            kids.sort_unstable();
            if let Some(s) = l.sort_of(f) {
                if let Some(v) = s.by_value {
                    let v = (v as usize).min(l.values.len() - 1);
                    kids.sort_by(|a, b| value_of(*a, v).total_cmp(&value_of(*b, v)));
                }
                if s.descending {
                    kids.reverse();
                }
            }
            kids
        };
    for (prefix, kids) in tree.iter_mut() {
        let f = l.rows[prefix.len()];
        let taken = std::mem::take(kids);
        *kids = sorted(f, taken, &|c, v| {
            let mut p = prefix.clone();
            p.push(c);
            raw(&p, None, v).and_then(Result::ok).unwrap_or(0.0)
        });
    }
    let mut cols: Vec<ColLine> = Vec::new();
    if let Some(&cf) = l.cols.first() {
        let mut present: Vec<usize> = (0..src.rows.len())
            .filter(|i| included[*i])
            .filter_map(col_of)
            .collect();
        present.sort_unstable();
        present.dedup();
        let present = sorted(cf, present, &|c, v| {
            raw(&[], Some(c), v).and_then(Result::ok).unwrap_or(0.0)
        });
        cols.extend(present.into_iter().map(ColLine::Item));
        for (k, ci) in l.calculated_items.iter().enumerate() {
            if ci.field as usize == cf {
                cols.push(ColLine::Calc(k));
            }
        }
        if l.grand_totals {
            cols.push(ColLine::Grand);
        }
    }
    let depth = l.rows.len();
    let tabular = l.form == ReportForm::Tabular;
    let mut lines: Vec<RowLine> = Vec::new();
    walk(
        &mut Vec::new(),
        &tree,
        depth,
        tabular,
        l.subtotals,
        &mut lines,
    );
    let row0 = l.rows[0];
    for (k, ci) in l.calculated_items.iter().enumerate() {
        if ci.field as usize == row0 {
            lines.push(RowLine::Calc(k));
        }
    }
    if l.grand_totals {
        lines.push(RowLine::Grand);
    }
    // A calculated item's summary: its formula of its field's items.
    let calc_value = |ci: &CalculatedItem, f: usize, at: &dyn Fn(usize) -> Summary| -> Summary {
        let it = &items[&f];
        Some(eval(&ci.formula, &|name: &str| {
            let p = it.place_of(name)?;
            Some(at(p).and_then(Result::ok).unwrap_or(0.0))
        }))
    };
    // A summary as Show Values As shows it.
    let shown = |prefix: &[usize], col: Option<usize>, k: usize| -> Option<Out> {
        let v = &l.values[k];
        let get = |p: &[usize], c: Option<usize>| raw(p, c, k).and_then(Result::ok);
        let pct = |a: Option<f64>, b: Option<f64>| -> Option<Out> {
            match (a, b) {
                (Some(a), Some(b)) if b != 0.0 => Some(Out::Percent(a / b)),
                (Some(_), _) => Some(Out::Error("#DIV/0!".into())),
                _ => None,
            }
        };
        let base_row = l.rows.iter().position(|r| *r == v.base_field);
        let base_col = l.cols.first() == Some(&v.base_field);
        match v.show_as {
            ShowAs::Normal => raw(prefix, col, k).map(out_of),
            ShowAs::PercentOfTotal => pct(get(prefix, col), get(&[], None)),
            ShowAs::PercentOfRow => pct(get(prefix, col), get(prefix, None)),
            ShowAs::PercentOfColumn => pct(get(prefix, col), get(&[], col)),
            ShowAs::RunningTotal => {
                if let Some(d) = base_row {
                    if prefix.len() <= d {
                        return None;
                    }
                    let sibs = tree.get(&prefix[..d]).cloned().unwrap_or_default();
                    let mut run = 0.0;
                    for s in sibs {
                        let mut p = prefix.to_vec();
                        p[d] = s;
                        run += get(&p, col).unwrap_or(0.0);
                        if s == prefix[d] {
                            break;
                        }
                    }
                    Some(Out::Number(run))
                } else if base_col {
                    let c = col?;
                    let mut run = 0.0;
                    for cl in &cols {
                        if let ColLine::Item(x) = cl {
                            run += get(prefix, Some(*x)).unwrap_or(0.0);
                            if *x == c {
                                break;
                            }
                        }
                    }
                    Some(Out::Number(run))
                } else {
                    raw(prefix, col, k).map(out_of)
                }
            }
            ShowAs::Difference | ShowAs::PercentDifference => {
                let it = items.get(&v.base_field)?;
                let named = (!v.base_item.is_empty())
                    .then(|| it.place_of(&v.base_item))
                    .flatten();
                let here = get(prefix, col);
                let base = if let Some(d) = base_row {
                    if prefix.len() <= d {
                        return None;
                    }
                    let sibs = tree.get(&prefix[..d]).cloned().unwrap_or_default();
                    let b = match named {
                        Some(b) => b,
                        None => {
                            let at = sibs.iter().position(|s| *s == prefix[d])?;
                            *sibs.get(at.checked_sub(1)?)?
                        }
                    };
                    if b == prefix[d] {
                        return None;
                    }
                    let mut p = prefix.to_vec();
                    p[d] = b;
                    get(&p, col)
                } else if base_col {
                    let c = col?;
                    let present: Vec<usize> = cols
                        .iter()
                        .filter_map(|x| match x {
                            ColLine::Item(p) => Some(*p),
                            _ => None,
                        })
                        .collect();
                    let b = match named {
                        Some(b) => b,
                        None => {
                            let at = present.iter().position(|p| *p == c)?;
                            *present.get(at.checked_sub(1)?)?
                        }
                    };
                    if b == c {
                        return None;
                    }
                    get(prefix, Some(b))
                } else {
                    return None;
                };
                match (here, base, v.show_as) {
                    (Some(h), Some(b), ShowAs::Difference) => Some(Out::Number(h - b)),
                    (Some(h), Some(b), _) if b != 0.0 => Some(Out::Percent((h - b) / b)),
                    (Some(_), Some(_), _) => Some(Out::Error("#NULL!".into())),
                    _ => None,
                }
            }
        }
    };
    let label = |f: usize, p: usize| -> Out {
        let it = &items[&f];
        let (v, text) = &it.shared[it.order[p]];
        match v {
            Value::Empty => Out::Text("(blank)".into()),
            Value::Number(x) if text.parse::<f64>().is_ok() => Out::Number(*x),
            _ => Out::Text(text.clone()),
        }
    };
    let label_cols = if l.form == ReportForm::Compact {
        1
    } else {
        depth
    };
    let n_values = l.values.len();
    let caption_of = |k: usize| {
        let v = &l.values[k];
        let agg = if v.field >= n_src {
            Aggregate::Sum
        } else {
            v.agg
        };
        caption(&names[v.field], agg)
    };
    let mut cells: Vec<Vec<Option<Out>>> = Vec::new();
    let row_headers: Vec<Option<Out>> = if l.form == ReportForm::Compact {
        vec![Some(Out::Text("Row Labels".into()))]
    } else {
        l.rows
            .iter()
            .map(|f| Some(Out::Text(names[*f].clone())))
            .collect()
    };
    let header_rows;
    if let Some(&cf) = l.cols.first() {
        header_rows = 2;
        let mut first = vec![Some(Out::Text(caption_of(0)))];
        first.extend(std::iter::repeat_n(None, label_cols - 1));
        first.push(Some(Out::Text(if l.form == ReportForm::Compact {
            "Column Labels".into()
        } else {
            names[cf].clone()
        })));
        cells.push(first);
        let mut head = row_headers.clone();
        for c in &cols {
            head.push(Some(match c {
                ColLine::Item(p) => label(cf, *p),
                ColLine::Calc(k) => Out::Text(l.calculated_items[*k].name.clone()),
                ColLine::Grand => Out::Text("Grand Total".into()),
            }));
        }
        cells.push(head);
    } else {
        header_rows = 1;
        let mut head = row_headers.clone();
        head.extend((0..n_values).map(|k| Some(Out::Text(caption_of(k)))));
        cells.push(head);
    }
    let value_cells = |prefix: &[usize]| -> Vec<Option<Out>> {
        if l.cols.is_empty() {
            (0..n_values).map(|k| shown(prefix, None, k)).collect()
        } else {
            let cf = l.cols[0];
            cols.iter()
                .map(|c| match c {
                    ColLine::Item(p) => shown(prefix, Some(*p), 0),
                    ColLine::Grand => shown(prefix, None, 0),
                    ColLine::Calc(k) => {
                        calc_value(&l.calculated_items[*k], cf, &|p| raw(prefix, Some(p), 0))
                            .map(out_of)
                    }
                })
                .collect()
        }
    };
    let mut prev: Vec<usize> = Vec::new();
    for line in &lines {
        let mut row: Vec<Option<Out>> = vec![None; label_cols];
        match line {
            RowLine::Item(path) => {
                let d = path.len() - 1;
                if tabular {
                    // The labels that changed from the row before.
                    let same = prev.iter().zip(path).take_while(|(a, b)| a == b).count();
                    for (dd, p) in path.iter().enumerate().skip(same) {
                        row[dd] = Some(label(l.rows[dd], *p));
                    }
                    prev = path.clone();
                } else {
                    let at = if l.form == ReportForm::Compact { 0 } else { d };
                    row[at] = Some(label(l.rows[d], path[d]));
                }
                if path.len() == depth || (l.subtotals && !tabular) {
                    row.extend(value_cells(path));
                }
            }
            RowLine::Subtotal(path) => {
                let d = path.len() - 1;
                let name = match label(l.rows[d], path[d]) {
                    Out::Text(t) => t,
                    Out::Number(n) => num(n),
                    _ => String::new(),
                };
                row[d] = Some(Out::Text(format!("{name} Total")));
                row.extend(value_cells(path));
                prev.clear();
            }
            RowLine::Calc(k) => {
                let ci = &l.calculated_items[*k];
                row[0] = Some(Out::Text(ci.name.clone()));
                if l.cols.is_empty() {
                    for v in 0..n_values {
                        row.push(calc_value(ci, row0, &|p| raw(&[p], None, v)).map(out_of));
                    }
                } else {
                    for c in &cols {
                        let col = match c {
                            ColLine::Item(p) => Some(*p),
                            _ => None,
                        };
                        row.push(calc_value(ci, row0, &|p| raw(&[p], col, 0)).map(out_of));
                    }
                }
            }
            RowLine::Grand => {
                row[0] = Some(Out::Text("Grand Total".into()));
                row.extend(value_cells(&[]));
            }
        }
        cells.push(row);
    }
    Ok(Pivot {
        layout: layout.clone(),
        names,
        items,
        rows: lines,
        cols,
        cells,
        header_rows,
        label_cols,
    })
}

impl Pivot {
    /// The cells' range from its first cell.
    pub fn range(&self, at: CellRef) -> Range {
        let width = self.cells.iter().map(Vec::len).max().unwrap_or(1) as u32;
        Range {
            start: at,
            end: CellRef::new(at.row + self.cells.len() as u32 - 1, at.col + width - 1),
        }
    }

    /// The cells of its values with their labels and captions, for a
    /// PivotChart, the grand totals left out.
    pub fn data_range(&self, at: CellRef) -> Range {
        let full = self.range(at);
        let grand = u32::from(self.layout.grand_totals);
        let last_col = if self.layout.cols.is_empty() {
            full.end.col
        } else {
            full.end.col.saturating_sub(grand)
        };
        Range {
            start: CellRef::new(
                at.row + self.header_rows as u32 - 1,
                at.col + self.label_cols as u32 - 1,
            ),
            end: CellRef::new(
                full.end.row.saturating_sub(grand).max(at.row),
                last_col.max(at.col),
            ),
        }
    }
}

fn item_xml(v: &Value) -> String {
    match v {
        Value::Number(n) => format!("<n v=\"{}\"/>", num(*n)),
        Value::Text(t) => format!("<s v=\"{}\"/>", xml::escape(t)),
        Value::Bool(b) => format!("<b v=\"{}\"/>", u8::from(*b)),
        Value::Error(e) => format!("<e v=\"{}\"/>", xml::escape(e)),
        _ => "<m/>".into(),
    }
}

/// The `<sharedItems>` of a field: what kinds of value it holds, and its
/// items when it is a row or column field (and its calculated items).
fn shared_items(src: &Source, field: usize, items: Option<&Items>, extra: &[String]) -> String {
    let (mut text, mut number, mut int, mut blank, mut boolean, mut error, mut long) =
        (false, false, true, false, false, false, false);
    let (mut min, mut max) = (f64::MAX, f64::MIN);
    for row in &src.rows {
        match row.get(field).unwrap_or(&Value::Empty) {
            Value::Number(n) => {
                number = true;
                int &= n.fract() == 0.0;
                min = min.min(*n);
                max = max.max(*n);
            }
            Value::Text(t) => {
                text = true;
                long |= t.chars().count() > 255;
            }
            Value::Bool(_) => boolean = true,
            Value::Error(_) => error = true,
            _ => blank = true,
        }
    }
    let mut a = String::new();
    if !text {
        a.push_str(" containsSemiMixedTypes=\"0\" containsString=\"0\"");
    }
    if [text, number, boolean, error]
        .iter()
        .filter(|x| **x)
        .count()
        > 1
    {
        a.push_str(" containsMixedTypes=\"1\"");
    }
    if number {
        a.push_str(" containsNumber=\"1\"");
        if int {
            a.push_str(" containsInteger=\"1\"");
        }
        a.push_str(&format!(
            " minValue=\"{}\" maxValue=\"{}\"",
            num(min),
            num(max)
        ));
    }
    if blank {
        a.push_str(" containsBlank=\"1\"");
    }
    if long {
        a.push_str(" longText=\"1\"");
    }
    match items.filter(|it| it.group.is_none()) {
        Some(it) => {
            let mut body: String = it.shared.iter().map(|(v, _)| item_xml(v)).collect();
            body.extend(
                extra
                    .iter()
                    .map(|e| format!("<s v=\"{}\"/>", xml::escape(e))),
            );
            format!(
                "<sharedItems{a} count=\"{}\">{body}</sharedItems>",
                it.shared.len() + extra.len()
            )
        }
        None => format!("<sharedItems{a}/>"),
    }
}

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// A field's calculated items' names.
fn calc_names(l: &Layout, f: usize) -> Vec<String> {
    l.calculated_items
        .iter()
        .filter(|c| c.field as usize == f)
        .map(|c| c.name.clone())
        .collect()
}

/// The cache definition: the source, its fields (a grouped one with its
/// groups), the calculated fields and items.
pub fn cache_definition(
    p: &Pivot,
    src: &Source,
    sheet: &str,
    range: Range,
    records_rid: &str,
    cache_id: u32,
) -> String {
    let l = &p.layout;
    let mut fields: String = src
        .names
        .iter()
        .enumerate()
        .map(|(f, name)| {
            let it = p.items.get(&f);
            let group = match it.and_then(|it| it.group.as_ref().map(|g| (it, g))) {
                Some((it, g)) => {
                    let labels: String = it
                        .shared
                        .iter()
                        .map(|(_, t)| format!("<s v=\"{}\"/>", xml::escape(t)))
                        .collect();
                    format!(
                        "<fieldGroup base=\"{f}\"><rangePr {g}/><groupItems count=\"{}\">{labels}</groupItems></fieldGroup>",
                        it.shared.len()
                    )
                }
                None => String::new(),
            };
            format!(
                "<cacheField name=\"{}\" numFmtId=\"0\">{}{group}</cacheField>",
                xml::escape(name),
                shared_items(src, f, it, &calc_names(l, f))
            )
        })
        .collect();
    for c in &l.calculated {
        fields.push_str(&format!(
            "<cacheField name=\"{}\" numFmtId=\"0\" formula=\"{}\" databaseField=\"0\"/>",
            xml::escape(&c.name),
            xml::escape(c.formula.trim().trim_start_matches('=').trim())
        ));
    }
    let calc_items: String = l
        .calculated_items
        .iter()
        .filter_map(|ci| {
            let f = ci.field as usize;
            let it = p.items.get(&f)?;
            let k = calc_names(l, f).iter().position(|n| *n == ci.name)?;
            Some(format!(
                "<calculatedItem formula=\"{}\"><pivotArea cacheIndex=\"1\" outline=\"0\" fieldPosition=\"0\"><references count=\"1\"><reference field=\"{f}\" count=\"1\"><x v=\"{}\"/></reference></references></pivotArea></calculatedItem>",
                xml::escape(ci.formula.trim().trim_start_matches('=').trim()),
                it.shared.len() + k
            ))
        })
        .collect();
    let calc_items = if calc_items.is_empty() {
        String::new()
    } else {
        format!(
            "<calculatedItems count=\"{}\">{calc_items}</calculatedItems>",
            l.calculated_items.len()
        )
    };
    format!(
        "{DECL}<pivotCacheDefinition xmlns=\"{MAIN}\" xmlns:r=\"{RELS}\" r:id=\"{records_rid}\" refreshOnLoad=\"1\" refreshedBy=\"Kalem\" createdVersion=\"8\" refreshedVersion=\"8\" minRefreshableVersion=\"3\" recordCount=\"{}\"><cacheSource type=\"worksheet\"><worksheetSource ref=\"{range}\" sheet=\"{}\"/></cacheSource><cacheFields count=\"{}\">{fields}</cacheFields>{calc_items}<extLst><ext uri=\"{{725AE2AE-9491-48be-B2B4-4EB974FC3084}}\" xmlns:x14=\"http://schemas.microsoft.com/office/spreadsheetml/2009/9/main\"><x14:pivotCacheDefinition pivotCacheId=\"{cache_id}\"/></ext></extLst></pivotCacheDefinition>",
        src.rows.len(),
        xml::escape(sheet),
        src.names.len() + l.calculated.len()
    )
}

/// The cache's records: row and column fields by item, the rest (and a
/// grouped field) as values.
pub fn cache_records(p: &Pivot, src: &Source) -> String {
    let mut body = String::new();
    for (i, row) in src.rows.iter().enumerate() {
        body.push_str("<r>");
        for f in 0..src.names.len() {
            match p.items.get(&f).filter(|it| it.group.is_none()) {
                Some(it) => body.push_str(&format!("<x v=\"{}\"/>", it.of_record[i])),
                None => body.push_str(&item_xml(row.get(f).unwrap_or(&Value::Empty))),
            }
        }
        body.push_str("</r>");
    }
    format!(
        "{DECL}<pivotCacheRecords xmlns=\"{MAIN}\" xmlns:r=\"{RELS}\" count=\"{}\">{body}</pivotCacheRecords>",
        src.rows.len()
    )
}

/// `showDataAs`'s value of a way to show values.
fn show_as_name(s: ShowAs) -> Option<&'static str> {
    Some(match s {
        ShowAs::Normal => return None,
        ShowAs::PercentOfTotal => "percentOfTotal",
        ShowAs::PercentOfRow => "percentOfRow",
        ShowAs::PercentOfColumn => "percentOfCol",
        ShowAs::RunningTotal => "runTotal",
        ShowAs::Difference => "difference",
        ShowAs::PercentDifference => "percentDiff",
    })
}

fn is_label_filter(k: PivotFilterKind) -> bool {
    matches!(
        k,
        PivotFilterKind::LabelEquals
            | PivotFilterKind::LabelBegins
            | PivotFilterKind::LabelContains
            | PivotFilterKind::LabelNotContains
    )
}

/// A filter's `<filter>`.
fn filter_xml(id: usize, f: &PivotFilter) -> String {
    let (ty, body) = match f.kind {
        PivotFilterKind::Top
        | PivotFilterKind::Bottom
        | PivotFilterKind::TopPercent
        | PivotFilterKind::BottomPercent => {
            let pct = matches!(
                f.kind,
                PivotFilterKind::TopPercent | PivotFilterKind::BottomPercent
            );
            let top = matches!(f.kind, PivotFilterKind::Top | PivotFilterKind::TopPercent);
            (
                if pct { "percent" } else { "count" },
                format!(
                    "<top10{}{} val=\"{}\" filterVal=\"{}\"/>",
                    if top { "" } else { " top=\"0\"" },
                    if pct { " percent=\"1\"" } else { "" },
                    num(f.number),
                    num(f.number)
                ),
            )
        }
        kind => {
            let t = xml::escape(&f.text);
            let (ty, op, val) = match kind {
                PivotFilterKind::LabelEquals => ("captionEqual", "", t.clone()),
                PivotFilterKind::LabelBegins => ("captionBeginsWith", "", format!("{t}*")),
                PivotFilterKind::LabelContains => ("captionContains", "", format!("*{t}*")),
                PivotFilterKind::LabelNotContains => (
                    "captionNotContains",
                    " operator=\"notEqual\"",
                    format!("*{t}*"),
                ),
                PivotFilterKind::ValueGreater => (
                    "valueGreaterThan",
                    " operator=\"greaterThan\"",
                    num(f.number),
                ),
                PivotFilterKind::ValueLess => {
                    ("valueLessThan", " operator=\"lessThan\"", num(f.number))
                }
                _ => ("valueEqual", "", num(f.number)),
            };
            (
                ty,
                format!("<customFilters><customFilter{op} val=\"{val}\"/></customFilters>"),
            )
        }
    };
    let measure = if is_label_filter(f.kind) {
        format!(" stringValue1=\"{}\"", xml::escape(&f.text))
    } else {
        format!(" iMeasureFld=\"{}\"", f.value)
    };
    format!(
        "<filter fld=\"{}\" type=\"{ty}\" evalOrder=\"-1\" id=\"{}\"{measure}><autoFilter ref=\"A1\"><filterColumn colId=\"0\">{body}</filterColumn></autoFilter></filter>",
        f.field,
        id + 1
    )
}

/// The table's definition.
pub fn table_definition(
    p: &Pivot,
    src: &Source,
    name: &str,
    cache_id: u32,
    at: CellRef,
    style: &str,
) -> String {
    let l = &p.layout;
    let n_fields = p.names.len();
    let form_attrs = match l.form {
        ReportForm::Compact => "",
        ReportForm::Outline => " compact=\"0\"",
        ReportForm::Tabular => " compact=\"0\" outline=\"0\"",
    };
    let fields: String = (0..n_fields)
        .map(|f| {
            let axis = if l.rows.contains(&f) {
                Some("axisRow")
            } else if l.cols.contains(&f) {
                Some("axisCol")
            } else {
                None
            };
            let data = if l.values.iter().any(|v| v.field == f) {
                " dataField=\"1\""
            } else {
                ""
            };
            if f >= src.names.len() {
                return format!(
                    "<pivotField{data} dragToRow=\"0\" dragToCol=\"0\" dragToPage=\"0\" showAll=\"0\" defaultSubtotal=\"0\"/>"
                );
            }
            let Some(a) = axis else {
                return format!("<pivotField{data}{form_attrs} showAll=\"0\"/>");
            };
            let it = &p.items[&f];
            let hidden: Vec<String> = l
                .filters
                .iter()
                .filter(|x| x.field as usize == f && x.kind == PivotFilterKind::Items)
                .flat_map(|x| x.hidden.iter().map(|h| h.to_lowercase()))
                .collect();
            let mut list: String = it
                .order
                .iter()
                .map(|x| {
                    let h = if hidden.contains(&it.shared[*x].1.to_lowercase()) {
                        " h=\"1\""
                    } else {
                        ""
                    };
                    format!("<item x=\"{x}\"{h}/>")
                })
                .collect();
            let calcs = calc_names(l, f);
            for k in 0..calcs.len() {
                list.push_str(&format!("<item x=\"{}\" f=\"1\"/>", it.shared.len() + k));
            }
            let sub = if l.subtotals {
                list.push_str("<item t=\"default\"/>");
                ""
            } else {
                " defaultSubtotal=\"0\""
            };
            let count = list.matches("<item").count();
            let sort = l.sort_of(f);
            let sort_attr = match sort {
                Some(s) if s.descending => " sortType=\"descending\"",
                Some(_) => " sortType=\"ascending\"",
                None => "",
            };
            let scope = sort.and_then(|s| s.by_value).map_or(String::new(), |v| {
                format!(
                    "<autoSortScope><pivotArea dataOnly=\"0\" outline=\"0\" fieldPosition=\"0\"><references count=\"1\"><reference field=\"4294967294\" count=\"1\" selected=\"0\"><x v=\"{v}\"/></reference></references></pivotArea></autoSortScope>"
                )
            });
            format!(
                "<pivotField axis=\"{a}\"{data}{form_attrs} showAll=\"0\"{sort_attr}{sub}><items count=\"{count}\">{list}</items>{scope}</pivotField>"
            )
        })
        .collect();
    let x = |v: usize| {
        if v == 0 {
            "<x/>".to_owned()
        } else {
            format!("<x v=\"{v}\"/>")
        }
    };
    let row_fields: String = l
        .rows
        .iter()
        .map(|f| format!("<field x=\"{f}\"/>"))
        .collect();
    let calc_x = |f: usize, k: usize| -> usize {
        let before = l.calculated_items[..k]
            .iter()
            .filter(|c| c.field as usize == f)
            .count();
        p.items[&f].order.len() + before
    };
    let r_attr = |d: usize| {
        if d == 0 {
            String::new()
        } else {
            format!(" r=\"{d}\"")
        }
    };
    let mut prev: Vec<usize> = Vec::new();
    let row_items: String = p
        .rows
        .iter()
        .map(|line| match line {
            RowLine::Item(path) if l.form == ReportForm::Tabular => {
                let same = prev.iter().zip(path).take_while(|(a, b)| a == b).count();
                prev = path.clone();
                let xs: String = path[same..].iter().map(|v| x(*v)).collect();
                format!("<i{}>{xs}</i>", r_attr(same))
            }
            RowLine::Item(path) => {
                let d = path.len() - 1;
                format!("<i{}>{}</i>", r_attr(d), x(path[d]))
            }
            RowLine::Subtotal(path) => {
                prev.clear();
                let d = path.len() - 1;
                format!("<i t=\"default\"{}>{}</i>", r_attr(d), x(path[d]))
            }
            RowLine::Calc(k) => format!("<i>{}</i>", x(calc_x(l.rows[0], *k))),
            RowLine::Grand => "<i t=\"grand\"><x/></i>".into(),
        })
        .collect();
    let (col_fields, col_items) = if let Some(&cf) = l.cols.first() {
        let items: String = p
            .cols
            .iter()
            .map(|c| match c {
                ColLine::Item(v) => format!("<i>{}</i>", x(*v)),
                ColLine::Calc(k) => format!("<i>{}</i>", x(calc_x(cf, *k))),
                ColLine::Grand => "<i t=\"grand\"><x/></i>".into(),
            })
            .collect();
        (
            format!("<colFields count=\"1\"><field x=\"{cf}\"/></colFields>"),
            format!("<colItems count=\"{}\">{items}</colItems>", p.cols.len()),
        )
    } else if l.values.len() > 1 {
        let items: String = (0..l.values.len())
            .map(|k| {
                if k == 0 {
                    "<i><x/></i>".to_owned()
                } else {
                    format!("<i i=\"{k}\"><x v=\"{k}\"/></i>")
                }
            })
            .collect();
        (
            "<colFields count=\"1\"><field x=\"-2\"/></colFields>".to_owned(),
            format!("<colItems count=\"{}\">{items}</colItems>", l.values.len()),
        )
    } else {
        (
            String::new(),
            "<colItems count=\"1\"><i/></colItems>".to_owned(),
        )
    };
    let data_fields: String = l
        .values
        .iter()
        .map(|v| {
            let agg = if v.field >= src.names.len() {
                Aggregate::Sum
            } else {
                v.agg
            };
            let sub = match agg {
                Aggregate::Sum => "",
                Aggregate::Count => " subtotal=\"count\"",
                Aggregate::Average => " subtotal=\"average\"",
                Aggregate::Max => " subtotal=\"max\"",
                Aggregate::Min => " subtotal=\"min\"",
            };
            let mut show = String::new();
            let mut base_field = 0;
            let mut base_item = 0u32;
            if let Some(s) = show_as_name(v.show_as) {
                show = format!(" showDataAs=\"{s}\"");
                base_field = v.base_field;
                base_item = p
                    .items
                    .get(&v.base_field)
                    .and_then(|it| it.place_of(&v.base_item))
                    .map_or(1_048_828, |pl| pl as u32);
                if matches!(
                    v.show_as,
                    ShowAs::PercentOfTotal
                        | ShowAs::PercentOfRow
                        | ShowAs::PercentOfColumn
                        | ShowAs::PercentDifference
                ) {
                    show.push_str(" numFmtId=\"10\"");
                }
            }
            format!(
                "<dataField name=\"{}\" fld=\"{}\"{sub}{show} baseField=\"{base_field}\" baseItem=\"{base_item}\"/>",
                xml::escape(&caption(&p.names[v.field], agg)),
                v.field
            )
        })
        .collect();
    let filters: Vec<String> = l
        .filters
        .iter()
        .filter(|f| f.kind != PivotFilterKind::Items)
        .enumerate()
        .map(|(id, f)| filter_xml(id, f))
        .collect();
    let filters = if filters.is_empty() {
        String::new()
    } else {
        format!(
            "<filters count=\"{}\">{}</filters>",
            filters.len(),
            filters.concat()
        )
    };
    let first_header = if l.cols.is_empty() && l.values.len() > 1 {
        0
    } else {
        1
    };
    let (compact, outline) = match l.form {
        ReportForm::Compact => ("", " outline=\"1\" outlineData=\"1\""),
        ReportForm::Outline => (
            " compact=\"0\" compactData=\"0\"",
            " outline=\"1\" outlineData=\"1\"",
        ),
        ReportForm::Tabular => (" compact=\"0\" compactData=\"0\"", ""),
    };
    let totals = if l.grand_totals {
        ""
    } else {
        " rowGrandTotals=\"0\" colGrandTotals=\"0\""
    };
    format!(
        "{DECL}<pivotTableDefinition xmlns=\"{MAIN}\" name=\"{}\" cacheId=\"{cache_id}\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" applyWidthHeightFormats=\"1\" dataCaption=\"Values\"{totals} updatedVersion=\"8\" minRefreshableVersion=\"3\" useAutoFormatting=\"1\" itemPrintTitles=\"1\" createdVersion=\"8\" indent=\"0\"{compact}{outline} multipleFieldFilters=\"0\"><location ref=\"{}\" firstHeaderRow=\"{first_header}\" firstDataRow=\"{}\" firstDataCol=\"{}\"/><pivotFields count=\"{n_fields}\">{fields}</pivotFields><rowFields count=\"{}\">{row_fields}</rowFields><rowItems count=\"{}\">{row_items}</rowItems>{col_fields}{col_items}<dataFields count=\"{}\">{data_fields}</dataFields><pivotTableStyleInfo name=\"{}\" showRowHeaders=\"1\" showColHeaders=\"1\" showRowStripes=\"0\" showColStripes=\"0\" showLastColumn=\"1\"/>{filters}</pivotTableDefinition>",
        xml::escape(name),
        p.range(at),
        p.header_rows,
        p.label_cols,
        l.rows.len(),
        p.rows.len(),
        l.values.len(),
        xml::escape(style)
    )
}

/// A filter as a table definition lists it: field, type, measure, number
/// and text.
pub type FilterDef = (usize, String, u32, f64, String);

/// What Kalem reads of an existing table: its name, cache, first cell,
/// style and layout by cache field.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableDef {
    /// The table's name.
    pub name: String,
    /// The cache it reads.
    pub cache_id: u32,
    /// Where it stands.
    pub location: Option<Range>,
    /// Its style's name.
    pub style: String,
    /// Row fields, by cache field.
    pub rows: Vec<usize>,
    /// Column fields, by cache field (the values' `-2` left out).
    pub cols: Vec<usize>,
    /// Value fields, by cache field: the field, summary, show-as, base
    /// field and base item (a place in the base field's items).
    pub values: Vec<(usize, Aggregate, ShowAs, usize, u32)>,
    /// Page (filter) fields: not refreshed.
    pub pages: usize,
    /// The report layout.
    pub form: ReportForm,
    /// Subtotals shown.
    pub subtotals: bool,
    /// Grand totals shown.
    pub grand_totals: bool,
    /// Each axis field's items (shared indexes) and whether hidden.
    pub items: HashMap<usize, Vec<(usize, bool)>>,
    /// Sorts: field, descending, by a value field.
    pub sorts: Vec<(usize, bool, Option<u32>)>,
    /// Filters.
    pub filters: Vec<FilterDef>,
}

/// Reads a table definition.
pub fn parse_table(text: &str) -> TableDef {
    let mut t = TableDef {
        subtotals: true,
        grand_totals: true,
        ..TableDef::default()
    };
    let mut r = xml::Reader::new(text);
    let mut list = "";
    let mut field = 0usize;
    let mut compact = true;
    let mut outline = true;
    let mut sort: Option<(usize, bool)> = None;
    let mut in_scope = false;
    let mut filter: Option<FilterDef> = None;
    while let Some(tok) = r.next_token() {
        let tag = match tok {
            xml::Token::End { name, .. } => {
                match name {
                    "pivotField" => {
                        if let Some((f, d)) = sort.take() {
                            t.sorts.push((f, d, None));
                        }
                        field += 1;
                    }
                    "autoSortScope" => in_scope = false,
                    "filter" => {
                        if let Some(f) = filter.take() {
                            t.filters.push(f);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            xml::Token::Text { .. } => continue,
            xml::Token::Start(tag) => tag,
        };
        let on = |k: &str| tag.attr(k).map(|v| v == "1" || v == "true");
        match tag.name {
            "pivotTableDefinition" => {
                t.name = tag.attr("name").unwrap_or_default().into_owned();
                t.cache_id = tag
                    .attr("cacheId")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                t.grand_totals =
                    on("rowGrandTotals").unwrap_or(true) && on("colGrandTotals").unwrap_or(true);
                compact = on("compact").unwrap_or(true);
                outline = on("outline").unwrap_or(true);
            }
            "location" => t.location = tag.attr("ref").and_then(|v| Range::parse(&v)),
            "pivotTableStyleInfo" => t.style = tag.attr("name").unwrap_or_default().into_owned(),
            "pivotField" => {
                if tag.attr("axis").is_some() {
                    if on("defaultSubtotal") == Some(false) {
                        t.subtotals = false;
                    }
                    let c = on("compact").unwrap_or(compact);
                    let o = on("outline").unwrap_or(outline);
                    match (c, o) {
                        (true, _) => {}
                        (false, true) => t.form = ReportForm::Outline,
                        (false, false) => t.form = ReportForm::Tabular,
                    }
                    if let Some(s) = tag.attr("sortType").filter(|s| s != "manual") {
                        sort = Some((field, s == "descending"));
                    }
                }
                if tag.empty {
                    field += 1;
                }
            }
            "item" if tag.attr("t").is_none() && tag.attr("f").is_none() => {
                if let Some(x) = tag.attr("x").and_then(|v| v.parse().ok()) {
                    t.items
                        .entry(field)
                        .or_default()
                        .push((x, on("h") == Some(true)));
                }
            }
            "autoSortScope" => in_scope = true,
            "x" if in_scope => {
                let v = tag.attr("v").and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some((f, d)) = sort.take() {
                    t.sorts.push((f, d, Some(v)));
                }
            }
            "rowFields" | "colFields" => {
                list = if tag.name == "rowFields" {
                    "rows"
                } else {
                    "cols"
                }
            }
            "field" => {
                if let Some(x) = tag
                    .attr("x")
                    .and_then(|v| v.parse::<i64>().ok())
                    .filter(|x| *x >= 0)
                {
                    if list == "rows" {
                        t.rows.push(x as usize);
                    } else {
                        t.cols.push(x as usize);
                    }
                }
            }
            "pageField" => t.pages += 1,
            "dataField" => {
                let agg = match tag.attr("subtotal").as_deref() {
                    Some("count" | "countNums") => Aggregate::Count,
                    Some("average") => Aggregate::Average,
                    Some("max") => Aggregate::Max,
                    Some("min") => Aggregate::Min,
                    _ => Aggregate::Sum,
                };
                let show = match tag.attr("showDataAs").as_deref() {
                    Some("percentOfTotal") => ShowAs::PercentOfTotal,
                    Some("percentOfRow") => ShowAs::PercentOfRow,
                    Some("percentOfCol") => ShowAs::PercentOfColumn,
                    Some("runTotal") => ShowAs::RunningTotal,
                    Some("difference") => ShowAs::Difference,
                    Some("percentDiff") => ShowAs::PercentDifference,
                    _ => ShowAs::Normal,
                };
                let bf = tag
                    .attr("baseField")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let bi = tag
                    .attr("baseItem")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1_048_828);
                if let Some(f) = tag.attr("fld").and_then(|v| v.parse().ok()) {
                    t.values.push((f, agg, show, bf, bi));
                }
            }
            "filter" => {
                filter = Some((
                    tag.attr("fld").and_then(|v| v.parse().ok()).unwrap_or(0),
                    tag.attr("type").unwrap_or_default().into_owned(),
                    tag.attr("iMeasureFld")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                    0.0,
                    tag.attr("stringValue1").unwrap_or_default().into_owned(),
                ));
            }
            "top10" => {
                if let Some(f) = filter.as_mut() {
                    f.3 = tag.attr("val").and_then(|v| v.parse().ok()).unwrap_or(10.0);
                    if tag.attr("top").as_deref() == Some("0") {
                        f.1.push_str("-bottom");
                    }
                }
            }
            "customFilter" => {
                if let Some(f) = filter.as_mut() {
                    f.3 = tag.attr("val").and_then(|v| v.parse().ok()).unwrap_or(0.0);
                }
            }
            _ => {}
        }
    }
    t
}

/// What Kalem reads of a cache definition.
#[derive(Debug, Clone, Default)]
pub struct CacheDef {
    /// The source's sheet.
    pub sheet: String,
    /// The source's range.
    pub range: Option<Range>,
    /// The fields' names, the calculated ones last.
    pub names: Vec<String>,
    /// Each field's shared items' labels (a grouped field's groups').
    pub labels: Vec<Vec<String>>,
    /// Calculated fields.
    pub calculated: Vec<CalculatedField>,
    /// Grouped fields.
    pub groups: Vec<PivotGroup>,
    /// Calculated items: field, the shared index, formula.
    pub calculated_items: Vec<(usize, usize, String)>,
    /// The records' relationship.
    pub records: Option<String>,
}

/// Reads a cache definition; `None` for a source Kalem does not refresh
/// (a named range, another file, a consolidation, a group of a group).
pub fn parse_cache(text: &str) -> Option<CacheDef> {
    let mut c = CacheDef::default();
    let mut r = xml::Reader::new(text);
    let mut sheet = None;
    let mut in_group = false;
    let mut calc_formula: Option<String> = None;
    let mut calc_field: Option<(usize, usize)> = None;
    while let Some(tok) = r.next_token() {
        if let xml::Token::End { name, .. } = tok {
            match name {
                "groupItems" | "fieldGroup" => in_group = false,
                "calculatedItem" => {
                    if let (Some(f), Some((fld, x))) = (calc_formula.take(), calc_field.take()) {
                        c.calculated_items.push((fld, x, f));
                    }
                }
                _ => {}
            }
            continue;
        }
        let xml::Token::Start(tag) = tok else {
            continue;
        };
        let field = c.names.len().saturating_sub(1);
        match tag.name {
            "pivotCacheDefinition" => c.records = tag.attr("id").map(|v| v.into_owned()),
            "worksheetSource" => {
                if tag.attr("name").is_some() && tag.attr("ref").is_none() {
                    return None;
                }
                sheet = tag.attr("sheet").map(|v| v.into_owned());
                c.range = tag
                    .attr("ref")
                    .and_then(|v| Range::parse(&v.replace('$', "")));
            }
            "cacheField" => {
                let name = tag.attr("name").unwrap_or_default().into_owned();
                if let Some(f) = tag.attr("formula") {
                    c.calculated.push(CalculatedField {
                        name: name.clone(),
                        formula: f.into_owned(),
                    });
                }
                c.names.push(name);
                c.labels.push(Vec::new());
            }
            "s" | "n" | "d" | "b" | "e" if tag.attr("v").is_some() && calc_formula.is_none() => {
                if let Some(l) = c.labels.last_mut() {
                    l.push(tag.attr("v").unwrap_or_default().into_owned());
                }
            }
            "m" if !in_group => {
                if let Some(l) = c.labels.last_mut() {
                    l.push("(blank)".into());
                }
            }
            "fieldGroup" => {
                if tag.attr("par").is_some()
                    || tag.attr("base").and_then(|b| b.parse::<usize>().ok()) != Some(field)
                {
                    return None;
                }
            }
            "rangePr" => {
                let num_attr = |k: &str| tag.attr(k).and_then(|v| v.parse::<f64>().ok());
                let by = match tag.attr("groupBy").as_deref() {
                    Some("months") => GroupBy::Months,
                    Some("quarters") => GroupBy::Quarters,
                    Some("years") => GroupBy::Years,
                    Some("days") => GroupBy::Days,
                    None | Some("range") => GroupBy::Step,
                    _ => return None,
                };
                let auto = |k: &str| tag.attr(k).is_none_or(|v| v == "1" || v == "true");
                c.groups.push(if by == GroupBy::Step {
                    PivotGroup {
                        field: field as u32,
                        by,
                        start: (!auto("autoStart")).then(|| num_attr("startNum")).flatten(),
                        end: (!auto("autoEnd")).then(|| num_attr("endNum")).flatten(),
                        step: num_attr("groupInterval").unwrap_or(1.0),
                    }
                } else {
                    PivotGroup {
                        field: field as u32,
                        by,
                        start: None,
                        end: None,
                        step: 0.0,
                    }
                });
            }
            "groupItems" => {
                in_group = true;
                if let Some(l) = c.labels.last_mut() {
                    l.clear();
                }
            }
            "calculatedItem" => calc_formula = tag.attr("formula").map(|v| v.into_owned()),
            "reference" if calc_formula.is_some() => {
                let fld = tag.attr("field").and_then(|v| v.parse().ok()).unwrap_or(0);
                calc_field = Some((fld, 0));
            }
            "x" if calc_formula.is_some() => {
                let x = tag.attr("v").and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(cf) = calc_field.as_mut() {
                    cf.1 = x;
                }
            }
            "consolidation" => return None,
            _ => {}
        }
    }
    c.sheet = sheet?;
    c.range?;
    Some(c)
}

/// A table's layout from its definition and cache, by cache field: the
/// hidden items, base items, filters and calculated items by their labels
/// in the cache.
pub fn layout_of(t: &TableDef, c: &CacheDef) -> Layout {
    let label = |f: usize, x: usize| c.labels.get(f).and_then(|l| l.get(x)).cloned();
    let place_label = |f: usize, place: u32| -> String {
        t.items
            .get(&f)
            .and_then(|list| list.get(place as usize))
            .and_then(|(x, _)| label(f, *x))
            .unwrap_or_default()
    };
    let mut filters: Vec<PivotFilter> = t
        .items
        .iter()
        .filter_map(|(f, list)| {
            let hidden: Vec<String> = list
                .iter()
                .filter(|(_, h)| *h)
                .filter_map(|(x, _)| label(*f, *x))
                .collect();
            (!hidden.is_empty()).then(|| PivotFilter {
                field: *f as u32,
                kind: PivotFilterKind::Items,
                hidden,
                ..PivotFilter::default()
            })
        })
        .collect();
    filters.sort_by_key(|f| f.field);
    for (f, ty, measure, n, text) in &t.filters {
        let kind = match ty.as_str() {
            "count" => PivotFilterKind::Top,
            "count-bottom" => PivotFilterKind::Bottom,
            "percent" => PivotFilterKind::TopPercent,
            "percent-bottom" => PivotFilterKind::BottomPercent,
            "captionEqual" => PivotFilterKind::LabelEquals,
            "captionBeginsWith" => PivotFilterKind::LabelBegins,
            "captionContains" => PivotFilterKind::LabelContains,
            "captionNotContains" => PivotFilterKind::LabelNotContains,
            "valueGreaterThan" => PivotFilterKind::ValueGreater,
            "valueLessThan" => PivotFilterKind::ValueLess,
            "valueEqual" => PivotFilterKind::ValueEquals,
            _ => continue,
        };
        let label_filter = is_label_filter(kind);
        filters.push(PivotFilter {
            field: *f as u32,
            kind,
            value: if label_filter { 0 } else { *measure },
            number: if label_filter { 0.0 } else { *n },
            text: text.clone(),
            hidden: Vec::new(),
        });
    }
    Layout {
        rows: t.rows.clone(),
        cols: t.cols.clone(),
        values: t
            .values
            .iter()
            .map(|(f, agg, show, bf, bi)| Val {
                field: *f,
                agg: *agg,
                show_as: *show,
                base_field: if *show == ShowAs::Normal { 0 } else { *bf },
                base_item: if *bi >= 1_048_828 || *show == ShowAs::Normal {
                    String::new()
                } else {
                    place_label(*bf, *bi)
                },
            })
            .collect(),
        groups: c.groups.clone(),
        calculated: c.calculated.clone(),
        calculated_items: c
            .calculated_items
            .iter()
            .map(|(f, x, formula)| CalculatedItem {
                field: *f as u32,
                name: label(*f, *x).unwrap_or_default(),
                formula: formula.clone(),
            })
            .collect(),
        sorts: t
            .sorts
            .iter()
            .map(|(f, d, v)| PivotSort {
                field: *f as u32,
                descending: *d,
                by_value: *v,
            })
            .collect(),
        filters,
        form: t.form,
        subtotals: t.subtotals,
        grand_totals: t.grand_totals,
    }
}

/// A layout as the grid contract's spec says it.
pub fn layout_from_spec(s: &kalem_viewer::PivotSpec) -> Layout {
    Layout {
        rows: s.rows.iter().map(|f| *f as usize).collect(),
        cols: s.cols.iter().map(|f| *f as usize).collect(),
        values: s
            .values
            .iter()
            .map(|v| Val {
                field: v.field as usize,
                agg: v.aggregate,
                show_as: v.show_as,
                base_field: v.base_field as usize,
                base_item: v.base_item.clone(),
            })
            .collect(),
        groups: s.groups.clone(),
        calculated: s.calculated.clone(),
        calculated_items: s.calculated_items.clone(),
        sorts: s.sorts.clone(),
        filters: s.filters.clone(),
        form: s.form,
        subtotals: s.subtotals,
        grand_totals: s.grand_totals,
    }
}

/// The grid contract's spec of a layout over `range`.
pub fn spec_of(l: &Layout, range: Range) -> kalem_viewer::PivotSpec {
    kalem_viewer::PivotSpec {
        range: [
            range.start.row,
            range.start.col,
            range.end.row,
            range.end.col,
        ],
        rows: l.rows.iter().map(|f| *f as u32).collect(),
        cols: l.cols.iter().map(|f| *f as u32).collect(),
        values: l
            .values
            .iter()
            .map(|v| kalem_viewer::PivotValue {
                field: v.field as u32,
                aggregate: v.agg,
                show_as: v.show_as,
                base_field: v.base_field as u32,
                base_item: v.base_item.clone(),
            })
            .collect(),
        groups: l.groups.clone(),
        calculated: l.calculated.clone(),
        calculated_items: l.calculated_items.clone(),
        sorts: l.sorts.clone(),
        filters: l.filters.clone(),
        form: l.form,
        subtotals: l.subtotals,
        grand_totals: l.grand_totals,
    }
}

/// A layout with every field index mapped by `map` (a cache's fields to
/// the source's as it is now).
pub fn remap(
    mut l: Layout,
    map: &dyn Fn(usize) -> Result<usize, String>,
) -> Result<Layout, String> {
    let m32 = |f: u32| map(f as usize).map(|x| x as u32);
    l.rows = l.rows.iter().map(|f| map(*f)).collect::<Result<_, _>>()?;
    l.cols = l.cols.iter().map(|f| map(*f)).collect::<Result<_, _>>()?;
    for v in &mut l.values {
        v.field = map(v.field)?;
        if v.show_as != ShowAs::Normal {
            v.base_field = map(v.base_field)?;
        }
    }
    for g in &mut l.groups {
        g.field = m32(g.field)?;
    }
    for c in &mut l.calculated_items {
        c.field = m32(c.field)?;
    }
    for s in &mut l.sorts {
        s.field = m32(s.field)?;
    }
    for f in &mut l.filters {
        f.field = m32(f.field)?;
    }
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Source {
        let t = |s: &str| Value::Text(s.into());
        let n = Value::Number;
        let rows = vec![
            vec![t("West"), t("Pens"), n(10.0)],
            vec![t("East"), t("Pens"), n(5.0)],
            vec![t("west"), t("Ink"), n(7.0)],
            vec![t("East"), t("Ink"), Value::Empty],
        ];
        let shown = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|v| match v {
                        Value::Text(s) => s.clone(),
                        Value::Number(x) => format!("{x}"),
                        _ => String::new(),
                    })
                    .collect()
            })
            .collect();
        Source {
            names: vec!["Region".into(), "Item".into(), "Units".into()],
            rows,
            shown,
        }
    }

    fn grid(p: &Pivot) -> Vec<Vec<String>> {
        let text = |o: &Option<Out>| match o {
            Some(Out::Text(t)) => t.clone(),
            Some(Out::Number(n)) => format!("{n}"),
            Some(Out::Percent(n)) => format!("{:.1}%", n * 100.0),
            Some(Out::Error(e)) => e.clone(),
            None => String::new(),
        };
        p.cells
            .iter()
            .map(|r| r.iter().map(text).collect())
            .collect()
    }

    fn data() -> Range {
        Range::parse("A1:C5").unwrap()
    }

    #[test]
    fn rows_and_a_sum() {
        let src = source();
        let p = compute(
            &src,
            &Layout::simple(vec![0], vec![], vec![(2, Aggregate::Sum)]),
        )
        .unwrap();
        assert_eq!(
            grid(&p),
            [
                ["Row Labels", "Sum of Units"],
                ["East", "5"],
                ["West", "17"],
                ["Grand Total", "22"]
            ]
        );
        assert_eq!(p.range(CellRef::new(2, 0)).to_string(), "A3:B6");
        let def = table_definition(
            &p,
            &src,
            "PivotTable1",
            1,
            CellRef::new(2, 0),
            "PivotStyleLight16",
        );
        assert!(def.contains("<rowItems count=\"3\"><i><x/></i><i><x v=\"1\"/></i><i t=\"grand\"><x/></i></rowItems>"), "{def}");
        assert!(
            def.contains(
                "<items count=\"3\"><item x=\"1\"/><item x=\"0\"/><item t=\"default\"/></items>"
            ),
            "{def}"
        );
        let t = parse_table(&def);
        assert_eq!((t.rows.clone(), t.values.len()), (vec![0], 1));
        let cache = cache_definition(&p, &src, "Data", data(), "rId1", 1);
        assert!(
            cache.contains("<sharedItems count=\"2\"><s v=\"West\"/><s v=\"East\"/></sharedItems>"),
            "{cache}"
        );
        let c = parse_cache(&cache).unwrap();
        assert_eq!(
            (
                c.sheet.as_str(),
                c.range.unwrap().to_string(),
                c.names.len()
            ),
            ("Data", "A1:C5".into(), 3)
        );
        assert!(
            cache_records(&p, &src).contains("<r><x v=\"0\"/><s v=\"Pens\"/><n v=\"10\"/></r>")
        );
        assert_eq!(layout_of(&t, &c), p.layout);
    }

    #[test]
    fn nested_rows_and_columns() {
        let src = source();
        let p = compute(
            &src,
            &Layout::simple(vec![0, 1], vec![], vec![(2, Aggregate::Count)]),
        )
        .unwrap();
        assert_eq!(p.rows.len(), 7);
        assert_eq!(
            p.cells[1][1],
            Some(Out::Number(1.0)),
            "East counts one value"
        );
        let p = compute(
            &src,
            &Layout::simple(vec![0], vec![1], vec![(2, Aggregate::Average)]),
        )
        .unwrap();
        assert_eq!(p.cells[0][1], Some(Out::Text("Column Labels".into())));
        assert_eq!(p.cells[2][1], Some(Out::Error("#DIV/0!".into())));
        assert_eq!(p.cells[3][1], Some(Out::Number(7.0)));
    }

    #[test]
    fn show_values_as_sorts_and_filters() {
        let src = source();
        let mut l = Layout::simple(vec![0], vec![], vec![(2, Aggregate::Sum)]);
        l.values[0].show_as = ShowAs::PercentOfTotal;
        let p = compute(&src, &l).unwrap();
        assert_eq!(grid(&p)[1], ["East", "22.7%"]);
        // Largest first by the sum.
        l.values[0].show_as = ShowAs::Normal;
        l.sorts = vec![PivotSort {
            field: 0,
            descending: true,
            by_value: Some(0),
        }];
        let p = compute(&src, &l).unwrap();
        assert_eq!(grid(&p)[1], ["West", "17"]);
        // Running total down the regions, then the top 1.
        l.values[0].show_as = ShowAs::RunningTotal;
        l.values[0].base_field = 0;
        let p = compute(&src, &l).unwrap();
        assert_eq!(grid(&p)[2], ["East", "22"]);
        l.values[0].show_as = ShowAs::Normal;
        l.values[0].base_field = 0;
        l.filters = vec![PivotFilter {
            field: 0,
            kind: PivotFilterKind::Top,
            number: 1.0,
            ..PivotFilter::default()
        }];
        let p = compute(&src, &l).unwrap();
        assert_eq!(
            grid(&p),
            [
                ["Row Labels", "Sum of Units"],
                ["West", "17"],
                ["Grand Total", "17"]
            ]
        );
        // East hidden by hand.
        l.filters = vec![PivotFilter {
            field: 0,
            kind: PivotFilterKind::Items,
            hidden: vec!["East".into()],
            ..PivotFilter::default()
        }];
        let p = compute(&src, &l).unwrap();
        assert_eq!(p.cells.len(), 3);
        let def = table_definition(&p, &src, "P", 1, CellRef::new(2, 0), "S");
        assert!(def.contains("h=\"1\""), "{def}");
        assert!(def.contains("sortType=\"descending\""), "{def}");
        let c = parse_cache(&cache_definition(&p, &src, "Data", data(), "rId1", 1)).unwrap();
        assert_eq!(layout_of(&parse_table(&def), &c), l);
    }

    #[test]
    fn layouts_calculated_fields_and_items() {
        let src = source();
        let mut l = Layout::simple(vec![0, 1], vec![], vec![(2, Aggregate::Sum)]);
        l.form = ReportForm::Tabular;
        let p = compute(&src, &l).unwrap();
        assert_eq!(
            grid(&p),
            [
                ["Region", "Item", "Sum of Units"],
                ["East", "Ink", "0"],
                ["", "Pens", "5"],
                ["East Total", "", "5"],
                ["West", "Ink", "7"],
                ["", "Pens", "10"],
                ["West Total", "", "17"],
                ["Grand Total", "", "22"]
            ]
        );
        let def = table_definition(&p, &src, "P", 1, CellRef::new(2, 0), "S");
        let c = parse_cache(&cache_definition(&p, &src, "Data", data(), "rId1", 1)).unwrap();
        assert_eq!(layout_of(&parse_table(&def), &c).form, ReportForm::Tabular);
        l.subtotals = false;
        l.grand_totals = false;
        assert_eq!(compute(&src, &l).unwrap().cells.len(), 5);
        // A calculated field of the sums; a calculated item of the items.
        let mut l = Layout::simple(vec![0], vec![], vec![(3, Aggregate::Sum)]);
        l.calculated = vec![CalculatedField {
            name: "Double".into(),
            formula: "Units * 2".into(),
        }];
        l.calculated_items = vec![CalculatedItem {
            field: 0,
            name: "Both".into(),
            formula: "East + West".into(),
        }];
        let p = compute(&src, &l).unwrap();
        assert_eq!(
            grid(&p),
            [
                ["Row Labels", "Sum of Double"],
                ["East", "10"],
                ["West", "34"],
                ["Both", "44"],
                ["Grand Total", "44"]
            ]
        );
        let def = table_definition(&p, &src, "P", 1, CellRef::new(2, 0), "S");
        let cache = cache_definition(&p, &src, "Data", data(), "rId1", 1);
        assert!(
            cache.contains("formula=\"Units * 2\" databaseField=\"0\""),
            "{cache}"
        );
        assert!(
            cache.contains("<calculatedItem formula=\"East + West\">"),
            "{cache}"
        );
        let back = layout_of(&parse_table(&def), &parse_cache(&cache).unwrap());
        assert_eq!(back.calculated, l.calculated);
        assert_eq!(back.calculated_items, l.calculated_items);
        assert_eq!(eval("2 * (3 + 4) ^ 2", &|_| None), Ok(98.0));
    }

    #[test]
    fn dates_and_numbers_grouped() {
        // 2024-01-15, 2024-02-20, 2024-02-01, 2025-01-03 as serials.
        let n = Value::Number;
        let rows = vec![
            vec![n(45306.0), n(1.0)],
            vec![n(45342.0), n(2.0)],
            vec![n(45323.0), n(4.0)],
            vec![n(45660.0), n(8.0)],
        ];
        let shown = rows
            .iter()
            .map(|r| r.iter().map(|_| String::new()).collect())
            .collect();
        let src = Source {
            names: vec!["Date".into(), "Amount".into()],
            rows,
            shown,
        };
        let mut l = Layout::simple(vec![0], vec![], vec![(1, Aggregate::Sum)]);
        l.groups = vec![PivotGroup {
            field: 0,
            by: GroupBy::Months,
            ..PivotGroup::default()
        }];
        let p = compute(&src, &l).unwrap();
        assert_eq!(grid(&p)[1..3], [["Jan", "9"], ["Feb", "6"]]);
        l.groups[0].by = GroupBy::Years;
        assert_eq!(
            grid(&compute(&src, &l).unwrap())[1..3],
            [["2024", "7"], ["2025", "8"]]
        );
        l.groups[0].by = GroupBy::Quarters;
        let p = compute(&src, &l).unwrap();
        assert_eq!(grid(&p)[1], ["Qtr1", "15"]);
        let cache = cache_definition(&p, &src, "Data", Range::parse("A1:B5").unwrap(), "rId1", 1);
        assert!(cache.contains("<rangePr groupBy=\"quarters\""), "{cache}");
        let c = parse_cache(&cache).unwrap();
        assert_eq!(c.groups, l.groups);
        // Amounts in steps of 5.
        let mut l = Layout::simple(vec![1], vec![], vec![(1, Aggregate::Count)]);
        l.groups = vec![PivotGroup {
            field: 1,
            by: GroupBy::Step,
            start: Some(0.0),
            end: Some(9.0),
            step: 5.0,
        }];
        let p = compute(&src, &l).unwrap();
        assert_eq!(grid(&p)[1..3], [["0-4", "3"], ["5-9", "1"]]);
    }
}
