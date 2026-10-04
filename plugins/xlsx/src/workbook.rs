//! A workbook opened as itself: the package's parts read on demand, cells
//! shown through their formats, and edits written into the one `<c>` they
//! touch.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::ops::Range as Span;

use crate::calc::{self, Engine};
use crate::cellref::{CellRef, MAX_COL, MAX_ROW, Range};
use crate::chart;
use crate::conditional;
use crate::formula::{self, Op};
use crate::numfmt;
use crate::package::{Package, PackageError};
use crate::pivot;
use crate::rels::{self, Rel, kind};
use crate::sheet::{self, Cell, FormulaKind, Sheet, Value};
use crate::structure;
use crate::styles::{self, CellStyle, Styles};
use crate::validation;
use crate::xml::{self, Reader, Token};

/// An error opening, editing or saving a workbook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The ZIP container is broken.
    Package(PackageError),
    /// The package is not a SpreadsheetML workbook.
    NotAWorkbook(String),
    /// A sheet index or name that does not exist.
    NoSuchSheet(String),
    /// The sheet is not a worksheet (a chart sheet, say).
    NotAWorksheet(String),
    /// An edit Excel would refuse too, with its reason.
    Refused(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(e) => e.fmt(f),
            Self::NotAWorkbook(m) => write!(f, "not an Excel workbook: {m}"),
            Self::NoSuchSheet(s) => write!(f, "no sheet {s}"),
            Self::NotAWorksheet(s) => write!(f, "{s} is not a worksheet"),
            Self::Refused(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<PackageError> for Error {
    fn from(e: PackageError) -> Self {
        Self::Package(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// What a sheet tab holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetKind {
    /// Cells.
    Worksheet,
    /// One chart filling the tab.
    Chartsheet,
    /// An Excel 5 dialog.
    Dialogsheet,
    /// An Excel 4 macro sheet.
    Macrosheet,
}

/// Whether a tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Shown.
    Visible,
    /// Hidden; the user can unhide it.
    Hidden,
    /// Hidden; only VBA can unhide it.
    VeryHidden,
}

/// A tab of the workbook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetInfo {
    /// The tab's name.
    pub name: String,
    /// What it holds.
    pub kind: SheetKind,
    /// Whether it shows.
    pub visibility: Visibility,
    /// The part holding it.
    pub part: String,
}

/// A defined name (`<definedName>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedName {
    /// The name.
    pub name: String,
    /// What it refers to, as formula text.
    pub refers_to: String,
    /// The sheet it is local to, by index.
    pub local_sheet: Option<usize>,
    /// Hidden from the name manager.
    pub hidden: bool,
}

/// A cell comment (a note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// The cell.
    pub cell: CellRef,
    /// The author.
    pub author: String,
    /// The text.
    pub text: String,
}

/// What a typed entry means, read as Excel reads a cell entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// Clears the value; the style stays.
    Clear,
    /// A formula without `=`.
    Formula(String),
    /// A number, with the built-in format Excel applies for how it was typed
    /// (9 for `50%`, 14 for a date), if any.
    Number(f64, Option<u32>),
    /// Text.
    Text(String),
    /// `TRUE` or `FALSE`.
    Bool(bool),
    /// An error literal.
    Error(String),
}

const ERRORS: [&str; 8] = [
    "#NULL!",
    "#DIV/0!",
    "#VALUE!",
    "#REF!",
    "#NAME?",
    "#NUM!",
    "#N/A",
    "#GETTING_DATA",
];

fn parse_plain_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    // Thousands separators only between groups of three digits.
    let (int, rest) = body.split_at(body.find(['.', 'e', 'E']).unwrap_or(body.len()));
    let int_clean = if int.contains(',') {
        let groups: Vec<&str> = int.split(',').collect();
        let ok = !groups[0].is_empty()
            && groups[0].len() <= 3
            && groups[1..].iter().all(|g| g.len() == 3);
        if !ok {
            return None;
        }
        groups.concat()
    } else {
        int.to_owned()
    };
    let txt = format!("{int_clean}{rest}");
    if !txt
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return None;
    }
    let v: f64 = txt.parse().ok()?;
    v.is_finite().then_some(if neg { -v } else { v })
}

fn parse_date_time(s: &str, date1904: bool) -> Option<(f64, u32)> {
    let s = s.trim();
    let (date, time) = match s.split_once([' ', 'T']) {
        Some((d, t)) => (Some(d), Some(t)),
        None if s.contains(':') => (None, Some(s)),
        None => (Some(s), None),
    };
    let mut serial = 0.0;
    if let Some(d) = date {
        let p: Vec<&str> = d.split('-').collect();
        if p.len() != 3 || p[0].len() != 4 {
            return None;
        }
        let (y, m, dd): (i64, u32, u32) =
            (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
        if !(1..=12).contains(&m) || !(1..=31).contains(&dd) || y < 1900 {
            return None;
        }
        serial = numfmt::date_to_serial(y, m, dd, date1904) as f64;
    }
    if let Some(t) = time {
        let p: Vec<&str> = t.split(':').collect();
        if !(2..=3).contains(&p.len()) {
            return None;
        }
        let h: u32 = p[0].parse().ok()?;
        let mi: u32 = p[1].parse().ok()?;
        let sec: f64 = p.get(2).map_or(Some(0.0), |x| x.parse().ok())?;
        if h > 23 || mi > 59 || !(0.0..60.0).contains(&sec) {
            return None;
        }
        serial += (f64::from(h) * 3600.0 + f64::from(mi) * 60.0 + sec) / 86_400.0;
    }
    let fmt = match (date.is_some(), time.is_some()) {
        (true, true) => 22,
        (true, false) => 14,
        _ => 21,
    };
    Some((serial, fmt))
}

impl Input {
    /// Reads a typed entry: `=` starts a formula, `'` forces text, numbers
    /// take `1,234.5`, `50%` and `1e3`, dates take ISO `2026-10-03` and
    /// times `14:30`.
    pub fn parse(s: &str, date1904: bool) -> Self {
        if s.is_empty() {
            return Self::Clear;
        }
        if let Some(f) = s.strip_prefix('=').filter(|f| !f.trim().is_empty()) {
            return Self::Formula(f.to_owned());
        }
        if let Some(t) = s.strip_prefix('\'') {
            return Self::Text(t.to_owned());
        }
        let up = s.trim().to_ascii_uppercase();
        match up.as_str() {
            "TRUE" => return Self::Bool(true),
            "FALSE" => return Self::Bool(false),
            e if ERRORS.contains(&e) => return Self::Error(e.to_owned()),
            _ => {}
        }
        if let Some(p) = s.trim().strip_suffix('%').and_then(parse_plain_number) {
            let decimals = s
                .split_once('.')
                .map_or(0, |(_, f)| f.trim_end_matches('%').len());
            return Self::Number(p / 100.0, Some(if decimals > 0 { 10 } else { 9 }));
        }
        if let Some(v) = parse_plain_number(s) {
            return Self::Number(
                v,
                s.contains(',')
                    .then_some(3)
                    .filter(|_| v.fract() == 0.0)
                    .or(s.contains(',').then_some(4)),
            );
        }
        if let Some((v, fmt)) = parse_date_time(s, date1904) {
            return Self::Number(v, Some(fmt));
        }
        Self::Text(s.to_owned())
    }
}

/// A workbook read from a package.
pub struct Workbook {
    pkg: Package,
    workbook_part: String,
    /// The workbook part's text, edited in place.
    workbook_xml: String,
    workbook_rels: Vec<Rel>,
    sheets: Vec<SheetInfo>,
    strings: Vec<String>,
    styles: Styles,
    styles_part: Option<String>,
    date1904: bool,
    defined_names: Vec<DefinedName>,
    /// Sheet texts and models, read when first asked for.
    loaded: HashMap<usize, (String, Sheet)>,
    /// Sheets whose text changed.
    dirty_sheets: Vec<usize>,
    /// Number formats already given an `<xf>` copy: (style, numFmtId) → new style.
    derived_styles: HashMap<(u32, u32), u32>,
    styles_xml: Option<String>,
    theme: Vec<styles::Rgb>,
    has_vba: bool,
    /// The formula engine, built when first needed; `Err` once it failed.
    engine: Option<std::result::Result<Engine, String>>,
    /// Formula cells whose stored result the engine reproduced.
    trusted: HashSet<(usize, CellRef)>,
    /// The engine's latest result of every formula cell.
    computed: HashMap<(usize, CellRef), Value>,
    /// States before each edit, for undo, and after each undone one, for redo.
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// A batch of edits (a macro run): the state before it, one undo step.
    batch: Option<Snapshot>,
    /// Cells entered into the engine during a batch, not computed yet.
    batch_edited: Vec<(usize, CellRef)>,
    /// Whether the batch changed anything.
    batch_changed: bool,
    /// Counts the changes to sheet texts, for what is computed from them.
    generation: u64,
    /// Each sheet's data validations, at the generation they were read.
    validations: HashMap<usize, (u64, Vec<validation::DataValidation>)>,
    /// The sheets found to hold no formula without its result, at the
    /// generation they were looked at.
    complete: HashMap<usize, u64>,
    /// Formulas computed as if in an unused cell, by sheet and text, at
    /// the generation they were computed.
    scratch: HashMap<(usize, String), (u64, Option<Value>)>,
}

/// What an edit changes, kept whole for undo: the package's bytes are
/// shared, so a snapshot costs the edited texts.
#[derive(Clone)]
struct Snapshot {
    pkg: Package,
    sheets: Vec<SheetInfo>,
    workbook_xml: String,
    workbook_rels: Vec<Rel>,
    defined_names: Vec<DefinedName>,
    loaded: HashMap<usize, (String, Sheet)>,
    dirty_sheets: Vec<usize>,
    derived_styles: HashMap<(u32, u32), u32>,
    styles_xml: Option<String>,
    styles: Styles,
    trusted: HashSet<(usize, CellRef)>,
    computed: HashMap<(usize, CellRef), Value>,
}

impl fmt::Debug for Workbook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Workbook")
            .field("part", &self.workbook_part)
            .field("sheets", &self.sheets)
            .finish_non_exhaustive()
    }
}

fn text_of(bytes: Vec<u8>, name: &str) -> Result<String> {
    // Parts are UTF-8 in practice; a byte order mark is dropped by the reader
    // only for reading, so the text keeps it for writing.
    String::from_utf8(bytes).map_err(|_| Error::NotAWorkbook(format!("{name} is not UTF-8")))
}

impl Workbook {
    /// Opens a workbook from the bytes of an `.xlsx`, `.xlsm`, `.xltx` or `.xltm` file.
    pub fn open(bytes: Vec<u8>) -> Result<Self> {
        let pkg = Package::read(bytes)?;
        let root = rels::parse(&text_of(pkg.part("_rels/.rels")?, "_rels/.rels")?);
        let workbook_part = root
            .iter()
            .find(|r| r.kind == kind::OFFICE_DOCUMENT && !r.external)
            .map(|r| rels::resolve("", &r.target))
            .ok_or_else(|| Error::NotAWorkbook("no office document relationship".into()))?;
        let workbook_xml = text_of(pkg.part(&workbook_part)?, &workbook_part)?;
        let wb_rels_path = rels::rels_path(&workbook_part);
        let workbook_rels = if pkg.contains(&wb_rels_path) {
            rels::parse(&text_of(pkg.part(&wb_rels_path)?, &wb_rels_path)?)
        } else {
            Vec::new()
        };
        let target = |k: &str| {
            workbook_rels
                .iter()
                .find(|r| r.kind == k && !r.external)
                .map(|r| rels::resolve(&workbook_part, &r.target))
                .filter(|p| pkg.contains(p))
        };
        if !workbook_xml.contains("workbook") || workbook_xml.contains("<document") {
            return Err(Error::NotAWorkbook(format!(
                "{workbook_part} is not a workbook part"
            )));
        }
        let strings = match target(kind::SHARED_STRINGS) {
            Some(p) => sheet::parse_shared_strings(&text_of(pkg.part(&p)?, &p)?),
            None => Vec::new(),
        };
        let theme = match workbook_rels
            .iter()
            .find(|r| r.kind == "theme")
            .map(|r| rels::resolve(&workbook_part, &r.target))
            .filter(|p| pkg.contains(p))
        {
            Some(p) => styles::parse_theme(&text_of(pkg.part(&p)?, &p)?),
            None => Vec::new(),
        };
        let styles_part = target(kind::STYLES);
        let styles = match &styles_part {
            Some(p) => styles::parse(&text_of(pkg.part(p)?, p)?, &theme),
            None => Styles::default(),
        };
        let has_vba = target(kind::VBA_PROJECT).is_some();
        let mut wb = Self {
            pkg,
            workbook_part,
            workbook_xml,
            workbook_rels,
            sheets: Vec::new(),
            strings,
            styles,
            styles_part,
            date1904: false,
            defined_names: Vec::new(),
            loaded: HashMap::new(),
            dirty_sheets: Vec::new(),
            derived_styles: HashMap::new(),
            generation: 0,
            validations: HashMap::new(),
            complete: HashMap::new(),
            scratch: HashMap::new(),
            styles_xml: None,
            theme,
            has_vba,
            engine: None,
            trusted: HashSet::new(),
            computed: HashMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            batch: None,
            batch_edited: Vec::new(),
            batch_changed: false,
        };
        wb.read_workbook_part();
        Ok(wb)
    }

    fn read_workbook_part(&mut self) {
        let xml_text = self.workbook_xml.clone();
        let mut r = Reader::new(&xml_text);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            match tag.name {
                "workbookPr" => {
                    self.date1904 = tag
                        .attr("date1904")
                        .as_deref()
                        .is_some_and(|v| v == "1" || v == "true");
                }
                "sheet" => {
                    let rid = tag.attr("id").unwrap_or_default();
                    let rel = self.workbook_rels.iter().find(|r| r.id == rid);
                    let kind = match rel.map(|r| r.kind.as_str()) {
                        Some(kind::CHARTSHEET) => SheetKind::Chartsheet,
                        Some(kind::DIALOGSHEET) => SheetKind::Dialogsheet,
                        Some(kind::MACROSHEET | "xlMacrosheet" | "xlIntlMacrosheet") => {
                            SheetKind::Macrosheet
                        }
                        _ => SheetKind::Worksheet,
                    };
                    self.sheets.push(SheetInfo {
                        name: tag.attr("name").unwrap_or_default().into_owned(),
                        kind,
                        visibility: match tag.attr("state").as_deref() {
                            Some("hidden") => Visibility::Hidden,
                            Some("veryHidden") => Visibility::VeryHidden,
                            _ => Visibility::Visible,
                        },
                        part: rel
                            .map(|r| rels::resolve(&self.workbook_part, &r.target))
                            .unwrap_or_default(),
                    });
                }
                "definedName" if !tag.empty => {
                    let name = tag.attr("name").unwrap_or_default().into_owned();
                    let local_sheet = tag.attr("localSheetId").and_then(|v| v.parse().ok());
                    let hidden = tag
                        .attr("hidden")
                        .as_deref()
                        .is_some_and(|v| v == "1" || v == "true");
                    let (refers_to, _) = r.text_until_end("definedName");
                    self.defined_names.push(DefinedName {
                        name,
                        refers_to,
                        local_sheet,
                        hidden,
                    });
                }
                _ => {}
            }
        }
    }

    /// The tabs in order.
    pub fn sheets(&self) -> &[SheetInfo] {
        &self.sheets
    }

    /// The index of a tab by name, ignoring case as Excel does.
    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets
            .iter()
            .position(|s| s.name.to_lowercase() == name.to_lowercase())
    }

    /// The defined names.
    pub fn defined_names(&self) -> &[DefinedName] {
        &self.defined_names
    }

    /// Whether dates count from 1904 (old Mac workbooks).
    pub fn date1904(&self) -> bool {
        self.date1904
    }

    /// Whether the workbook carries a VBA project (an `.xlsm`).
    pub fn has_vba(&self) -> bool {
        self.has_vba
    }

    /// The bytes of the VBA project part, if any.
    pub fn vba_project_bytes(&self) -> Result<Option<Vec<u8>>> {
        let part = self
            .workbook_rels
            .iter()
            .find(|r| r.kind == kind::VBA_PROJECT && !r.external)
            .map(|r| rels::resolve(&self.workbook_part, &r.target));
        match part {
            Some(p) if self.pkg.contains(&p) => Ok(Some(self.pkg.part(&p)?)),
            _ => Ok(None),
        }
    }

    /// The VBA project's modules, read from its part; `None` without one.
    pub fn vba_project(&self) -> Result<Option<crate::vba::Project>> {
        match self.vba_project_bytes()? {
            Some(bin) => crate::vba::read_project(&bin)
                .map(Some)
                .map_err(|e| Error::NotAWorkbook(e.to_string())),
            None => Ok(None),
        }
    }

    /// The names of the package's parts.
    pub fn parts(&self) -> Vec<String> {
        self.pkg.names()
    }

    /// The style of a cell format index.
    pub fn style(&self, s: u32) -> CellStyle {
        self.styles.get(s)
    }

    fn check_index(&self, idx: usize) -> Result<&SheetInfo> {
        self.sheets
            .get(idx)
            .ok_or_else(|| Error::NoSuchSheet(idx.to_string()))
    }

    fn load(&mut self, idx: usize) -> Result<()> {
        if self.loaded.contains_key(&idx) {
            return Ok(());
        }
        let info = self.check_index(idx)?.clone();
        if info.kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(info.name));
        }
        let text = text_of(self.pkg.part(&info.part)?, &info.part)?;
        let model = sheet::parse(&text, &self.strings, self.date1904);
        self.loaded.insert(idx, (text, model));
        Ok(())
    }

    /// A worksheet's model.
    pub fn sheet(&mut self, idx: usize) -> Result<&Sheet> {
        self.load(idx)?;
        Ok(&self.loaded[&idx].1)
    }

    /// A cell as the grid shows it: the value through its number format.
    pub fn display(&mut self, idx: usize, at: CellRef) -> Result<String> {
        self.load(idx)?;
        self.flush()?;
        self.compute_missing(idx)?;
        let Some(c) = self.loaded[&idx].1.cells.get(&at) else {
            return Ok(String::new());
        };
        Ok(self.format_value(c, &self.value_of(idx, at, c)))
    }

    /// A cell's value: the stored one, or for a formula stored without its
    /// result, the engine's.
    pub fn value(&mut self, idx: usize, at: CellRef) -> Result<Value> {
        self.load(idx)?;
        self.flush()?;
        self.compute_missing(idx)?;
        Ok(self.loaded[&idx]
            .1
            .cells
            .get(&at)
            .map_or(Value::Empty, |c| self.value_of(idx, at, c)))
    }

    /// The numbers among a range's values, and how many of its cells hold
    /// a value: a status bar's Sum, Average and Count.
    pub fn range_summary(&mut self, idx: usize, range: Range) -> Result<(Vec<f64>, usize)> {
        self.load(idx)?;
        self.flush()?;
        self.compute_missing(idx)?;
        let cells = &self.loaded[&idx].1.cells;
        let (mut numbers, mut count) = (Vec::new(), 0);
        let from = CellRef::new(range.start.row, 0);
        let to = CellRef::new(range.end.row, u32::MAX);
        for (at, c) in cells.range(from..=to) {
            if !(range.start.col..=range.end.col).contains(&at.col) {
                continue;
            }
            match self.value_of(idx, *at, c) {
                Value::Empty => {}
                Value::Number(v) => {
                    numbers.push(v);
                    count += 1;
                }
                _ => count += 1,
            }
        }
        Ok((numbers, count))
    }

    fn value_of(&self, idx: usize, at: CellRef, c: &Cell) -> Value {
        if c.formula.is_some()
            && c.value == Value::Empty
            && let Some(v) = self.computed.get(&(idx, at))
        {
            return v.clone();
        }
        c.value.clone()
    }

    /// Builds the engine when a sheet holds formulas stored without results
    /// (openpyxl writes them so), so that the grid shows their values.
    fn compute_missing(&mut self, idx: usize) -> Result<()> {
        if self.engine.is_some() || self.complete.get(&idx) == Some(&self.generation) {
            return Ok(());
        }
        let missing = self.loaded[&idx]
            .1
            .cells
            .values()
            .any(|c| c.formula.is_some() && c.value == Value::Empty);
        if missing {
            self.ensure_engine()?;
        } else {
            // Looked at once until the sheet changes: the grid asks for
            // every cell it shows.
            self.complete.insert(idx, self.generation);
        }
        Ok(())
    }

    /// Every formula cell of the workbook.
    fn formula_cells(&self) -> Vec<(usize, CellRef)> {
        let mut out = Vec::new();
        for (i, (_, model)) in &self.loaded {
            out.extend(
                model
                    .cells
                    .iter()
                    .filter(|(_, c)| c.formula.is_some())
                    .map(|(p, _)| (*i, *p)),
            );
        }
        out
    }

    /// Loads the workbook into the engine, computes it, and trusts the cells
    /// whose stored results it reproduces.
    fn ensure_engine(&mut self) -> Result<()> {
        if self.engine.is_some() {
            return Ok(());
        }
        if self.date1904 {
            self.engine = Some(Err("the 1904 date system".into()));
            return Ok(());
        }
        for i in 0..self.sheets.len() {
            if self.sheets[i].kind == SheetKind::Worksheet {
                self.load(i)?;
            }
        }
        let sheets: Vec<(String, Option<&Sheet>)> = (0..self.sheets.len())
            .map(|i| {
                (
                    self.sheets[i].name.clone(),
                    self.loaded.get(&i).map(|(_, m)| m),
                )
            })
            .collect();
        let names: Vec<(String, Option<usize>, String)> = self
            .defined_names
            .iter()
            .filter(|n| !n.name.starts_with("_xlnm."))
            .map(|n| (n.name.clone(), n.local_sheet, n.refers_to.clone()))
            .collect();
        let cells = self.formula_cells();
        let mut engine = match Engine::load(&sheets, &names) {
            Ok(e) => e,
            Err(e) => {
                self.engine = Some(Err(e));
                return Ok(());
            }
        };
        self.computed = engine.evaluate(&cells).clone();
        self.trusted = cells
            .into_iter()
            .filter(|(i, p)| {
                let stored = &self.loaded[i].1.cells[p].value;
                *stored != Value::Empty
                    && self
                        .computed
                        .get(&(*i, *p))
                        .is_some_and(|v| calc::same(v, stored))
            })
            .collect();
        self.engine = Some(Ok(engine));
        Ok(())
    }

    /// After an edit: the engine computes again; formula cells whose results
    /// changed get their new result when trusted and lose their stored one
    /// otherwise. The edited cell, when a formula, gets its result unless the
    /// engine does not know a function in it.
    fn recompute_after(&mut self, idx: usize, at: CellRef) -> Result<()> {
        let (formula, value) = match self.loaded[&idx].1.cells.get(&at) {
            Some(c) => (c.formula.as_ref().map(|f| f.text.clone()), c.value.clone()),
            None => (None, Value::Empty),
        };
        let cells = self.formula_cells();
        let batch = self.batch.is_some();
        let Some(Ok(engine)) = self.engine.as_mut() else {
            return Ok(());
        };
        engine
            .set(idx, at, formula.as_deref(), &value)
            .map_err(|e| Error::Refused(format!("the formula engine: {e}")))?;
        if batch {
            // Computed when a value is read or the batch ends.
            self.batch_edited.push((idx, at));
            return Ok(());
        }
        let after = engine.evaluate(&cells).clone();
        let before = std::mem::take(&mut self.computed);
        self.write_results(&before, after, &cells, &[(idx, at)]);
        Ok(())
    }

    /// Computes the cells entered during a batch and writes their results.
    fn flush(&mut self) -> Result<()> {
        if self.batch_edited.is_empty() {
            return Ok(());
        }
        let edited = std::mem::take(&mut self.batch_edited);
        let cells = self.formula_cells();
        let Some(Ok(engine)) = self.engine.as_mut() else {
            return Ok(());
        };
        let after = engine.evaluate(&cells).clone();
        let before = std::mem::take(&mut self.computed);
        self.write_results(&before, after, &cells, &edited);
        Ok(())
    }

    /// Starts a batch of edits that undo as one step and compute when read:
    /// a macro's run.
    pub fn begin_batch(&mut self) -> Result<()> {
        for i in 0..self.sheets.len() {
            if self.sheets[i].kind == SheetKind::Worksheet {
                self.load(i)?;
            }
        }
        self.ensure_engine()?;
        self.batch = Some(self.snapshot());
        self.batch_changed = false;
        Ok(())
    }

    /// Ends a batch: everything is computed, and the batch is one undo step
    /// when it changed anything. Returns whether it did.
    pub fn end_batch(&mut self) -> Result<bool> {
        let flushed = self.flush();
        let Some(before) = self.batch.take() else {
            return flushed.map(|()| false);
        };
        if self.batch_changed {
            self.undo.push(before);
            self.redo.clear();
        }
        flushed.map(|()| self.batch_changed)
    }

    /// A formula computed as if it were in an unused cell of the sheet
    /// (`Evaluate`, `WorksheetFunction`); `None` when the engine cannot.
    pub fn evaluate_formula(&mut self, idx: usize, formula: &str) -> Result<Option<Value>> {
        Ok(self
            .evaluate_formulas(idx, &[formula.to_owned()])?
            .pop()
            .flatten())
    }

    /// Formulas computed together, as [`Workbook::evaluate_formula`] one:
    /// every engine computation recalculates the workbook, so the grid asks
    /// for what a screenful needs at once. Kept until the workbook changes.
    pub fn evaluate_formulas(
        &mut self,
        idx: usize,
        formulas: &[String],
    ) -> Result<Vec<Option<Value>>> {
        self.load(idx)?;
        self.ensure_engine()?;
        self.flush()?;
        let generation = self.generation;
        let key = |f: &String| (idx, f.trim_start_matches('=').to_owned());
        let mut missing: Vec<String> = formulas
            .iter()
            .filter(|f| {
                self.scratch
                    .get(&key(f))
                    .is_none_or(|(g, _)| *g != generation)
            })
            .map(|f| f.trim_start_matches('=').to_owned())
            .collect();
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            let values = match self.engine.as_mut() {
                Some(Ok(e)) => e.eval_scratch_many(idx, &missing),
                _ => vec![None; missing.len()],
            };
            if self.scratch.len() > 200_000 {
                self.scratch.clear();
            }
            for (f, v) in missing.into_iter().zip(values) {
                self.scratch.insert((idx, f), (generation, v));
            }
        }
        Ok(formulas
            .iter()
            .map(|f| self.scratch.get(&key(f)).and_then(|(_, v)| v.clone()))
            .collect())
    }

    /// Writes the results that changed between two computations: trusted
    /// cells get the new result, untrusted ones lose theirs; the `edited`
    /// cell gets its result unless the engine does not know a function in it.
    fn write_results(
        &mut self,
        before: &HashMap<(usize, CellRef), Value>,
        after: HashMap<(usize, CellRef), Value>,
        cells: &[(usize, CellRef)],
        edited: &[(usize, CellRef)],
    ) {
        let mut writes: BTreeMap<usize, Vec<(CellRef, Option<Value>)>> = BTreeMap::new();
        for key in cells {
            let new = after.get(key);
            let changed = match (before.get(key), new) {
                (Some(a), Some(b)) => !calc::same(a, b),
                (None, None) => false,
                _ => true,
            };
            if edited.contains(key) {
                let usable = new.filter(|v| !matches!(v, Value::Error(e) if e == "#NAME?"));
                writes
                    .entry(key.0)
                    .or_default()
                    .push((key.1, usable.cloned()));
                if usable.is_some() {
                    self.trusted.insert(*key);
                }
                continue;
            }
            if !changed {
                continue;
            }
            if self.trusted.contains(key) {
                writes.entry(key.0).or_default().push((key.1, new.cloned()));
            } else if self.loaded[&key.0].1.cells[&key.1].value != Value::Empty {
                writes.entry(key.0).or_default().push((key.1, None));
            }
        }
        self.computed = after;
        for (sheet_idx, cells) in writes {
            let (text, model) = &self.loaded[&sheet_idx];
            let prefix = model.prefix.clone();
            let splices: Vec<(Span<usize>, String)> = cells
                .iter()
                .filter_map(|(p, v)| {
                    let c = model.cells.get(p)?;
                    Some((
                        c.span.clone(),
                        with_cached(&text[c.span.clone()], &prefix, v.as_ref()),
                    ))
                })
                .filter(|(span, new)| text[span.clone()] != **new)
                .collect();
            if splices.is_empty() {
                continue;
            }
            let new_text = splice(text, splices);
            let model = sheet::parse(&new_text, &self.strings, self.date1904);
            self.loaded.insert(sheet_idx, (new_text, model));
            self.generation += 1;
            if !self.dirty_sheets.contains(&sheet_idx) {
                self.dirty_sheets.push(sheet_idx);
            }
        }
    }

    /// A cell's value through its number format.
    pub fn format_cell(&self, c: &Cell) -> String {
        self.format_value(c, &c.value)
    }

    fn format_value(&self, c: &Cell, value: &Value) -> String {
        let code = self.styles.get(c.style).num_fmt;
        match value {
            Value::Empty => String::new(),
            Value::Number(v) => numfmt::format_number(*v, &code, self.date1904),
            Value::Text(t) => numfmt::format_text(t, &code),
            Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            Value::Error(e) => e.clone(),
        }
    }

    /// A cell as the formula bar shows it: `=` and the formula, a date in
    /// ISO form, a number in full, text as typed.
    pub fn edit_text(&mut self, idx: usize, at: CellRef) -> Result<String> {
        self.load(idx)?;
        let Some(c) = self.loaded[&idx].1.cells.get(&at) else {
            return Ok(String::new());
        };
        if let Some(f) = &c.formula {
            return Ok(format!("={}", f.text));
        }
        Ok(match &c.value {
            Value::Number(v) if self.styles.is_date(c.style) => {
                let code = if v.fract() == 0.0 {
                    "yyyy-mm-dd"
                } else if *v < 1.0 {
                    "hh:mm:ss"
                } else {
                    "yyyy-mm-dd hh:mm:ss"
                };
                numfmt::format_number(*v, code, self.date1904)
            }
            Value::Number(v) => {
                let s = format!("{v}");
                if self.styles.get(c.style).num_fmt.contains('%') {
                    format!("{}%", numfmt::format_general(v * 100.0))
                } else {
                    s
                }
            }
            Value::Text(t) => {
                let looks_other = !matches!(Input::parse(t, self.date1904), Input::Text(_));
                if looks_other {
                    format!("'{t}")
                } else {
                    t.clone()
                }
            }
            Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            Value::Error(e) => e.clone(),
            Value::Empty => String::new(),
        })
    }

    /// The comments (notes) of a worksheet.
    pub fn comments(&self, idx: usize) -> Result<Vec<Comment>> {
        let info = self.check_index(idx)?;
        let rels_path = rels::rels_path(&info.part);
        if !self.pkg.contains(&rels_path) {
            return Ok(Vec::new());
        }
        let srels = rels::parse(&text_of(self.pkg.part(&rels_path)?, &rels_path)?);
        let mut out = Vec::new();
        for rel in srels
            .iter()
            .filter(|r| r.kind == kind::COMMENTS && !r.external)
        {
            let p = rels::resolve(&info.part, &rel.target);
            if !self.pkg.contains(&p) {
                continue;
            }
            let text = text_of(self.pkg.part(&p)?, &p)?;
            let mut r = Reader::new(&text);
            let mut authors = Vec::new();
            while let Some(t) = r.next_token() {
                let Token::Start(tag) = t else { continue };
                match tag.name {
                    "author" if !tag.empty => authors.push(r.text_until_end("author").0),
                    "comment" if !tag.empty => {
                        let cell = tag.attr("ref").and_then(|v| CellRef::parse(&v));
                        let author = tag
                            .attr("authorId")
                            .and_then(|v| v.parse::<usize>().ok())
                            .and_then(|i| authors.get(i).cloned())
                            .unwrap_or_default();
                        let (body, _) = r.text_until_end("comment");
                        if let Some(cell) = cell {
                            out.push(Comment {
                                cell,
                                author,
                                text: xml::unescape_st_xstring(&body).into_owned(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(out)
    }

    /// Types an entry into a cell, as typing it in Excel and pressing Enter.
    pub fn set_cell(&mut self, idx: usize, at: CellRef, entry: &str) -> Result<()> {
        let input = Input::parse(entry, self.date1904);
        if !(matches!(&input, Input::Text(t) if t.contains('\n'))) {
            return self.set_input(idx, at, input);
        }
        // Text with a line break (Alt+Enter): wrapped, as Excel does, in
        // the same undo step.
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = self
            .set_input(idx, at, input)
            .and_then(|()| self.set_wrap(idx, at, true));
        self.batch_changed = true;
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Enters `entry` into every cell of `range`, as Excel's Ctrl+Enter: a
    /// formula's references moved for each cell as from `at`, where it was
    /// typed. One undo step.
    pub fn enter_in_range(
        &mut self,
        idx: usize,
        range: Range,
        at: CellRef,
        entry: &str,
    ) -> Result<()> {
        self.load(idx)?;
        let area = u64::from(range.end.row - range.start.row + 1)
            * u64::from(range.end.col - range.start.col + 1);
        if area > 200_000 {
            return Err(Error::Refused("Select fewer cells: 200,000 at most".into()));
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = (|| -> Result<()> {
            for r in range.start.row..=range.end.row {
                for c in range.start.col..=range.end.col {
                    let input = match Input::parse(entry, self.date1904) {
                        Input::Formula(f) => Input::Formula(formula::shift(
                            &f,
                            i64::from(r) - i64::from(at.row),
                            i64::from(c) - i64::from(at.col),
                        )),
                        other => other,
                    };
                    self.set_input(idx, CellRef::new(r, c), input)?;
                }
            }
            Ok(())
        })();
        self.batch_changed = true;
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Writes a parsed entry into a cell. Only the cell's `<c>` changes,
    /// plus, where the specification needs it: a shared formula's next cell
    /// when the group's first cell is overwritten, the `<dimension>`, the
    /// `<row>` holding a new cell, a copy of the cell's `<xf>` when the entry
    /// brings a number format (a date typed into a General cell), the
    /// workbook's `calcPr` so Excel recalculates on open, and the
    /// calculation chain removed when a formula is removed (Excel rebuilds it).
    pub fn set_input(&mut self, idx: usize, at: CellRef, input: Input) -> Result<()> {
        self.load(idx)?;
        self.ensure_engine()?;
        if self.batch.is_some() {
            self.set_input_inner(idx, at, input)?;
            self.batch_changed = true;
            return Ok(());
        }
        let snapshot = self.snapshot();
        let result = self.set_input_inner(idx, at, input);
        match &result {
            Ok(()) => {
                self.undo.push(snapshot);
                self.redo.clear();
            }
            Err(_) => self.restore(snapshot),
        }
        result
    }

    fn set_input_inner(&mut self, idx: usize, at: CellRef, input: Input) -> Result<()> {
        let model = &self.loaded[&idx].1;
        let old = model.cells.get(&at).cloned();
        if let Some(m) = model.merge_at(at).filter(|m| m.start != at) {
            return Err(Error::Refused(format!(
                "{at} is inside the merged cell {m}; edit {}",
                m.start
            )));
        }
        if let Some(c) = &old {
            if let Some(anchor) = c.in_array_of {
                return Err(Error::Refused(format!(
                    "{at} is part of the array formula at {anchor}; an array is changed as a whole"
                )));
            }
            if let Some(f) = &c.formula {
                match f.kind {
                    FormulaKind::Array { range } if range.start != range.end => {
                        return Err(Error::Refused(format!(
                            "{at} holds an array formula over {range}; an array is changed as a whole"
                        )));
                    }
                    FormulaKind::DataTable => {
                        return Err(Error::Refused(format!("{at} is part of a data table")));
                    }
                    _ => {}
                }
            }
        }
        let mut style = old
            .as_ref()
            .map_or_else(|| self.row_or_col_style(idx, at), |c| c.style);
        if let Input::Number(_, Some(fmt)) = input {
            // Excel applies a format only when the cell's is General.
            if self.styles.get(style).num_fmt == "General" {
                style = self.style_with_numfmt(style, fmt)?;
            }
        }
        let (text, model) = &self.loaded[&idx];
        let prefix = model.prefix.clone();
        let is_formula = matches!(input, Input::Formula(_));
        let removes_formula = old.as_ref().is_some_and(|c| c.formula.is_some()) && !is_formula;
        let mut splices: Vec<(Span<usize>, String)> = Vec::new();
        let new_c = cell_xml(&prefix, at, style, &input);
        // A shared group's first cell overwritten: its next cell takes the text.
        if let Some(Cell {
            formula: Some(f), ..
        }) = &old
            && let FormulaKind::Shared { si, master: true } = f.kind
        {
            let members: Vec<(CellRef, Cell)> = model
                .cells
                .iter()
                .filter(|(p, c)| {
                    **p != at && matches!(&c.formula, Some(g) if g.kind == FormulaKind::Shared { si, master: false })
                })
                .map(|(p, c)| (*p, c.clone()))
                .collect();
            if let Some((first, fc)) = members.first() {
                let (mut r0, mut r1, mut c0, mut c1) = (first.row, first.row, first.col, first.col);
                for (p, _) in &members {
                    r0 = r0.min(p.row);
                    r1 = r1.max(p.row);
                    c0 = c0.min(p.col);
                    c1 = c1.max(p.col);
                }
                let range = Range {
                    start: CellRef::new(r0, c0),
                    end: CellRef::new(r1, c1),
                };
                let ftext = &fc.formula.as_ref().expect("member").text;
                let replaced =
                    rewrite_shared_master(&text[fc.span.clone()], &prefix, si, range, ftext);
                splices.push((fc.span.clone(), replaced));
            }
        }
        match &old {
            Some(c) => {
                if matches!(input, Input::Clear) && style == 0 {
                    splices.push((c.span.clone(), String::new()));
                } else {
                    splices.push((c.span.clone(), new_c));
                }
            }
            None if matches!(input, Input::Clear) && style == 0 => return Ok(()),
            None => splices.extend(insert_cell(text, model, at, new_c)?),
        }
        if let Some((span, dim)) = &model.dimension {
            let grown = match Range::parse(dim) {
                Some(r) if r.contains(at) => None,
                Some(r) => Some(Range {
                    start: CellRef::new(r.start.row.min(at.row), r.start.col.min(at.col)),
                    end: CellRef::new(r.end.row.max(at.row), r.end.col.max(at.col)),
                }),
                None => Some(Range { start: at, end: at }),
            };
            if let Some(g) = grown.filter(|_| !matches!(input, Input::Clear)) {
                splices.push((
                    span.clone(),
                    xml::set_attr(&text[span.clone()], "ref", &g.to_string()),
                ));
            }
        }
        let new_text = splice(text, splices);
        let model = sheet::parse(&new_text, &self.strings, self.date1904);
        self.loaded.insert(idx, (new_text, model));
        self.generation += 1;
        if !self.dirty_sheets.contains(&idx) {
            self.dirty_sheets.push(idx);
        }
        self.recompute_after(idx, at)?;
        if is_formula || self.any_formula()? {
            self.set_full_calc_on_load();
        }
        if removes_formula {
            self.drop_calc_chain()?;
        }
        Ok(())
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            pkg: self.pkg.clone(),
            sheets: self.sheets.clone(),
            workbook_xml: self.workbook_xml.clone(),
            workbook_rels: self.workbook_rels.clone(),
            defined_names: self.defined_names.clone(),
            loaded: self.loaded.clone(),
            dirty_sheets: self.dirty_sheets.clone(),
            derived_styles: self.derived_styles.clone(),
            styles_xml: self.styles_xml.clone(),
            styles: self.styles.clone(),
            trusted: self.trusted.clone(),
            computed: self.computed.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.pkg = s.pkg;
        self.sheets = s.sheets;
        self.workbook_xml = s.workbook_xml;
        self.workbook_rels = s.workbook_rels;
        self.defined_names = s.defined_names;
        self.loaded = s.loaded;
        self.dirty_sheets = s.dirty_sheets;
        self.derived_styles = s.derived_styles;
        self.styles_xml = s.styles_xml;
        self.styles = s.styles;
        self.trusted = s.trusted;
        self.computed = s.computed;
        self.generation += 1;
        // The engine holds the cells as they were; it is built again when needed.
        self.engine = None;
    }

    /// Undoes the last edit; `false` when there is none.
    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else {
            return false;
        };
        let now = self.snapshot();
        self.restore(s);
        self.redo.push(now);
        true
    }

    /// Undoes the last edit and forgets it, with no redo: a macro's run
    /// stopped to ask the user is taken back this way.
    pub fn rollback(&mut self) -> bool {
        let Some(s) = self.undo.pop() else {
            return false;
        };
        self.restore(s);
        true
    }

    /// Redoes the last undone edit; `false` when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else {
            return false;
        };
        let now = self.snapshot();
        self.restore(s);
        self.undo.push(now);
        true
    }

    /// How many edits there are to undo: a host compares it with the count
    /// at its last save to know whether the file is modified.
    pub fn history_len(&self) -> usize {
        self.undo.len()
    }

    /// Whether there is an edit to undo, and one to redo.
    pub fn can_undo_redo(&self) -> (bool, bool) {
        (!self.undo.is_empty(), !self.redo.is_empty())
    }

    /// Sets a row's height in points, as a drag of its edge in Excel: the
    /// `<row>` gets `ht` and `customHeight`, made when the row holds nothing.
    pub fn set_row_height(&mut self, idx: usize, row: u32, height: f64) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if !(0.0..=409.0).contains(&height) || row >= MAX_ROW {
            return Err(Error::Refused("a row is 0 to 409 points high".into()));
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let (text, model) = &self.loaded[&idx];
        let ht = format!("{}", (height * 100.0).round() / 100.0);
        let p = model.prefix.clone();
        let splices = match model.rows.get(&row) {
            Some(r) => {
                let tag = xml::set_attr(&text[r.start.clone()], "ht", &ht);
                vec![(r.start.clone(), xml::set_attr(&tag, "customHeight", "1"))]
            }
            None => {
                let new_row = format!("<{p}row r=\"{}\" ht=\"{ht}\" customHeight=\"1\"/>", row + 1);
                let Some((sd_start, sd_end)) = &model.sheet_data else {
                    return Err(Error::Refused("the sheet part has no sheetData".into()));
                };
                match sd_end {
                    None => {
                        let tag = &text[sd_start.clone()];
                        let open = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
                        vec![(sd_start.clone(), format!("{open}>{new_row}</{p}sheetData>"))]
                    }
                    Some(_) => {
                        let at = model
                            .rows
                            .range(..row)
                            .next_back()
                            .map_or(sd_start.end, |(_, r)| {
                                r.end.as_ref().map_or(r.start.end, |e| e.end)
                            });
                        vec![(at..at, new_row)]
                    }
                }
            }
        };
        let new = splice(text, splices);
        let model = sheet::parse(&new, &self.strings, self.date1904);
        self.loaded.insert(idx, (new, model));
        self.generation += 1;
        if !self.dirty_sheets.contains(&idx) {
            self.dirty_sheets.push(idx);
        }
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets a column's width in characters of the default font's digit,
    /// as Excel's autofit and drag do: the `<col>` covering it is split so
    /// that only this column changes, and gets `customWidth`.
    pub fn set_col_width(&mut self, idx: usize, col: u32, width: f64) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if !(0.0..=255.0).contains(&width) || col >= MAX_COL {
            return Err(Error::Refused(
                "a column is 0 to 255 characters wide".into(),
            ));
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let text = self.loaded[&idx].0.clone();
        let new = col_width_text(&text, col + 1, width);
        if new != text {
            let model = sheet::parse(&new, &self.strings, self.date1904);
            self.loaded.insert(idx, (new, model));
            self.generation += 1;
            if !self.dirty_sheets.contains(&idx) {
                self.dirty_sheets.push(idx);
            }
            match snapshot {
                Some(s) => {
                    self.undo.push(s);
                    self.redo.clear();
                }
                None => self.batch_changed = true,
            }
        }
        Ok(())
    }

    /// Freezes the first `rows` rows and `cols` columns, as Excel's Freeze
    /// Panes (none of either unfreezes): the first sheet view's `<pane>`.
    pub fn set_frozen(&mut self, idx: usize, rows: u32, cols: u32) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if rows >= MAX_ROW || cols >= MAX_COL {
            return Err(Error::Refused("No such row or column".into()));
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let text = self.loaded[&idx].0.clone();
        let new = frozen_text(&text, &self.loaded[&idx].1.prefix, rows, cols);
        if new != text {
            let model = sheet::parse(&new, &self.strings, self.date1904);
            self.loaded.insert(idx, (new, model));
            self.generation += 1;
            if !self.dirty_sheets.contains(&idx) {
                self.dirty_sheets.push(idx);
            }
            match snapshot {
                Some(s) => {
                    self.undo.push(s);
                    self.redo.clear();
                }
                None => self.batch_changed = true,
            }
        }
        Ok(())
    }

    /// Hides rows (`rows`) or columns `from..=to`, or shows them again, as
    /// Excel's Hide and Unhide: `hidden` on their `<row>`s or `<col>`s.
    pub fn set_hidden(
        &mut self,
        idx: usize,
        rows: bool,
        from: u32,
        to: u32,
        hidden: bool,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let (from, to) = (from.min(to), from.max(to));
        let limit = if rows { MAX_ROW } else { MAX_COL };
        if to >= limit {
            return Err(Error::Refused("No such row or column".into()));
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let text = self.loaded[&idx].0.clone();
        let new = if rows {
            if hidden && to - from >= 100_000 {
                return Err(Error::Refused("Hide 100,000 rows at most at once".into()));
            }
            rows_hidden_text(&text, &self.loaded[&idx].1, from, to, hidden)?
        } else {
            let mut t = text.clone();
            // Only columns a `<col>` has can be hidden already.
            let has = |c: u32| {
                self.loaded[&idx]
                    .1
                    .cols
                    .iter()
                    .any(|k| (k.min..=k.max).contains(&c))
            };
            for col in from..=to {
                if !hidden && !has(col) {
                    continue;
                }
                t = col_attrs_text(&t, col + 1, &|tag: &str| {
                    if hidden {
                        xml::set_attr(tag, "hidden", "1")
                    } else {
                        xml::remove_attr(tag, "hidden")
                    }
                });
            }
            t
        };
        if new != text {
            let model = sheet::parse(&new, &self.strings, self.date1904);
            self.loaded.insert(idx, (new, model));
            self.generation += 1;
            if !self.dirty_sheets.contains(&idx) {
                self.dirty_sheets.push(idx);
            }
            match snapshot {
                Some(s) => {
                    self.undo.push(s);
                    self.redo.clear();
                }
                None => self.batch_changed = true,
            }
        }
        Ok(())
    }

    /// Inserts `n` empty rows before row `at` (zero-based).
    pub fn insert_rows(&mut self, idx: usize, at: u32, n: u32) -> Result<()> {
        self.structural(idx, Op::InsertRows { at, n })
    }

    /// Deletes rows `at..at + n`.
    pub fn delete_rows(&mut self, idx: usize, at: u32, n: u32) -> Result<()> {
        self.structural(idx, Op::DeleteRows { at, n })
    }

    /// Inserts `n` empty columns before column `at` (zero-based).
    pub fn insert_cols(&mut self, idx: usize, at: u32, n: u32) -> Result<()> {
        self.structural(idx, Op::InsertCols { at, n })
    }

    /// Deletes columns `at..at + n`.
    pub fn delete_cols(&mut self, idx: usize, at: u32, n: u32) -> Result<()> {
        self.structural(idx, Op::DeleteCols { at, n })
    }

    /// The parts a sheet's relationships name, by relationship type.
    fn sheet_parts(&self, idx: usize) -> Result<Vec<(String, String)>> {
        let part = &self.sheets[idx].part;
        let rels_path = rels::rels_path(part);
        if !self.pkg.contains(&rels_path) {
            return Ok(Vec::new());
        }
        let rels = rels::parse(&text_of(self.pkg.part(&rels_path)?, &rels_path)?);
        Ok(rels
            .into_iter()
            .filter(|r| !r.external)
            .map(|r| (r.kind, rels::resolve(part, &r.target)))
            .filter(|(_, p)| self.pkg.contains(p))
            .collect())
    }

    fn rewrite_part(&mut self, part: &str, f: impl Fn(&str) -> String) -> Result<()> {
        let text = text_of(self.pkg.part(part)?, part)?;
        let new = f(&text);
        if new != text {
            self.pkg.set_part(part, new.into_bytes());
        }
        Ok(())
    }

    fn structural(&mut self, idx: usize, op: Op) -> Result<()> {
        let (Op::InsertRows { n, .. }
        | Op::DeleteRows { n, .. }
        | Op::InsertCols { n, .. }
        | Op::DeleteCols { n, .. }) = op;
        if n == 0 {
            return Ok(());
        }
        self.load(idx)?;
        self.ensure_engine()?;
        for i in 0..self.sheets.len() {
            if self.sheets[i].kind == SheetKind::Worksheet {
                self.load(i)?;
            }
        }
        self.check_structural(idx, op)?;
        if self.batch.is_some() {
            self.flush()?;
            self.structural_inner(idx, op)?;
            self.batch_changed = true;
            return Ok(());
        }
        let snapshot = self.snapshot();
        match self.structural_inner(idx, op) {
            Ok(()) => {
                self.undo.push(snapshot);
                self.redo.clear();
                Ok(())
            }
            Err(e) => {
                self.restore(snapshot);
                Err(e)
            }
        }
    }

    /// What Excel refuses: data pushed off the sheet, part of an array
    /// formula, columns through a table.
    fn check_structural(&self, idx: usize, op: Op) -> Result<()> {
        let model = &self.loaded[&idx].1;
        let rows_op = matches!(op, Op::InsertRows { .. } | Op::DeleteRows { .. });
        if let Op::InsertRows { n, .. } | Op::InsertCols { n, .. } = op {
            let max = if rows_op { MAX_ROW } else { MAX_COL };
            let last = model
                .cells
                .keys()
                .map(|c| if rows_op { c.row } else { c.col })
                .max()
                .unwrap_or(0);
            if !model.cells.is_empty() && last + n >= max {
                return Err(Error::Refused(
                    "cells would be pushed off the end of the sheet".into(),
                ));
            }
        }
        for c in model.cells.values() {
            if let Some(f) = &c.formula
                && let FormulaKind::Array { range } = f.kind
                && range.start != range.end
            {
                let (a, b) = if rows_op {
                    (range.start.row, range.end.row)
                } else {
                    (range.start.col, range.end.col)
                };
                let touched = match op {
                    Op::InsertRows { at, .. } | Op::InsertCols { at, .. } => a < at && at <= b,
                    Op::DeleteRows { at, n } | Op::DeleteCols { at, n } => {
                        let (lo, hi) = (at, at + n - 1);
                        hi >= a && lo <= b && !(lo <= a && hi >= b)
                    }
                };
                if touched {
                    return Err(Error::Refused(format!(
                        "this would change part of the array formula over {range}"
                    )));
                }
            }
        }
        if !rows_op {
            for (kind, part) in self.sheet_parts(idx)? {
                if kind != "table" {
                    continue;
                }
                let text = text_of(self.pkg.part(&part)?, &part)?;
                if let Some(t) = structure::table_range(&text) {
                    let (a, b) = (t.start.col, t.end.col);
                    let inside = match op {
                        Op::InsertCols { at, .. } => a < at && at <= b,
                        Op::DeleteCols { at, n } => at + n > a && at <= b,
                        _ => false,
                    };
                    if inside {
                        return Err(Error::Refused(format!(
                            "columns of the table at {t} are changed in the table, not the sheet"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn structural_inner(&mut self, idx: usize, op: Op) -> Result<()> {
        let name = self.sheets[idx].name.clone();
        let names: Vec<String> = self.sheets.iter().map(|s| s.name.clone()).collect();
        // The sheets' parts.
        let indices: Vec<usize> = self.loaded.keys().copied().collect();
        for i in indices {
            let (text, model) = &self.loaded[&i];
            let new = if i == idx {
                structure::rewrite_sheet(text, op, &name, model)
            } else {
                structure::rewrite_formulas(text, op, &name, &names[i])
            };
            if new != *text {
                let model = sheet::parse(&new, &self.strings, self.date1904);
                self.loaded.insert(i, (new, model));
                self.generation += 1;
                if !self.dirty_sheets.contains(&i) {
                    self.dirty_sheets.push(i);
                }
            }
        }
        // Defined names.
        let wb = structure::rewrite_defined_names(&self.workbook_xml, op, &name, &names);
        if wb != self.workbook_xml {
            self.workbook_xml = wb;
            self.reread_defined_names();
        }
        // What hangs on the sheet: comments, note shapes, drawings, tables.
        for (kind, part) in self.sheet_parts(idx)? {
            match kind.as_str() {
                "comments" => self.rewrite_part(&part, |t| structure::rewrite_comments(t, op))?,
                "vmlDrawing" => self.rewrite_part(&part, |t| structure::rewrite_vml(t, op))?,
                "drawing" => self.rewrite_part(&part, |t| structure::rewrite_drawing(t, op))?,
                "table" => self.rewrite_part(&part, |t| structure::rewrite_table(t, op))?,
                _ => {}
            }
        }
        // Charts and pivot caches anywhere in the package.
        for part in self.pkg.names() {
            let file = part.rsplit('/').next().unwrap_or_default();
            if part.starts_with("xl/charts/") && file.starts_with("chart") && file.ends_with(".xml")
            {
                self.rewrite_part(&part, |t| structure::rewrite_chart(t, op, &name))?;
            } else if file.starts_with("pivotCacheDefinition") && file.ends_with(".xml") {
                self.rewrite_part(&part, |t| structure::rewrite_pivot_cache(t, op, &name))?;
            }
        }
        self.drop_calc_chain()?;
        self.set_full_calc_on_load();
        // Results: the cells moved, so do their last results and trust.
        let moved = |(s, p): (usize, CellRef)| -> Option<(usize, CellRef)> {
            if s == idx {
                op.cell(p).map(|q| (s, q))
            } else {
                Some((s, p))
            }
        };
        let before: HashMap<(usize, CellRef), Value> = std::mem::take(&mut self.computed)
            .into_iter()
            .filter_map(|(k, v)| moved(k).map(|k| (k, v)))
            .collect();
        self.trusted = std::mem::take(&mut self.trusted)
            .into_iter()
            .filter_map(moved)
            .collect();
        let trusted = self.trusted.clone();
        self.engine = None;
        self.ensure_engine()?;
        // Building the engine trusted cells against the file; keep the
        // trust carried over from before the change.
        self.trusted.extend(trusted);
        let after = std::mem::take(&mut self.computed);
        let cells = self.formula_cells();
        self.write_results(&before, after, &cells, &[]);
        Ok(())
    }

    fn reread_defined_names(&mut self) {
        self.defined_names.clear();
        let text = self.workbook_xml.clone();
        let mut r = Reader::new(&text);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            if tag.name == "definedName" && !tag.empty {
                let name = tag.attr("name").unwrap_or_default().into_owned();
                let local_sheet = tag.attr("localSheetId").and_then(|v| v.parse().ok());
                let hidden = tag
                    .attr("hidden")
                    .as_deref()
                    .is_some_and(|v| v == "1" || v == "true");
                let (refers_to, _) = r.text_until_end("definedName");
                self.defined_names.push(DefinedName {
                    name,
                    refers_to,
                    local_sheet,
                    hidden,
                });
            }
        }
    }

    fn row_or_col_style(&self, _idx: usize, _at: CellRef) -> u32 {
        // A row's or column's own style (`<row s customFormat>`, `<col style>`)
        // is what Excel gives a new cell; reading it is a later step.
        0
    }

    fn any_formula(&mut self) -> Result<bool> {
        for i in 0..self.sheets.len() {
            if self.sheets[i].kind != SheetKind::Worksheet {
                continue;
            }
            self.load(i)?;
            if self.loaded[&i]
                .1
                .cells
                .values()
                .any(|c| c.formula.is_some())
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A copy of the `<xf>` at `style` with `numFmtId` set, appended to
    /// `cellXfs`; reused when asked again.
    fn style_with_numfmt(&mut self, style: u32, fmt: u32) -> Result<u32> {
        self.derive_style(style, fmt, |src| {
            // The start tag of the copied `<xf>` gets the format.
            let tag_end = src.find('>').map_or(src.len(), |p| p + 1);
            let mut head = xml::set_attr(&src[..tag_end], "numFmtId", &fmt.to_string());
            head = xml::set_attr(&head, "applyNumberFormat", "1");
            format!("{head}{}", &src[tag_end..])
        })
    }

    /// A copy of the `<xf>` at `style` changed by `edit`, appended to
    /// `cellXfs`; reused when asked again with the same `key`.
    fn derive_style(&mut self, style: u32, key: u32, edit: impl Fn(&str) -> String) -> Result<u32> {
        if let Some(&s) = self.derived_styles.get(&(style, key)) {
            return Ok(s);
        }
        let Some(part) = self.styles_part.clone() else {
            return Ok(style);
        };
        let xml_text = match &self.styles_xml {
            Some(t) => t.clone(),
            None => text_of(self.pkg.part(&part)?, &part)?,
        };
        let mut r = Reader::new(&xml_text);
        let mut in_cell_xfs = false;
        let mut count = 0u32;
        let mut cell_xfs_tag: Option<Span<usize>> = None;
        let mut source: Option<Span<usize>> = None;
        let mut end_tag: Option<Span<usize>> = None;
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) if tag.name == "cellXfs" && !tag.empty => {
                    in_cell_xfs = true;
                    cell_xfs_tag = Some(tag.span.clone());
                }
                Token::Start(tag) if in_cell_xfs && tag.name == "xf" => {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    if count == style {
                        source = Some(tag.span.start..end);
                    }
                    count += 1;
                }
                Token::End {
                    name: "cellXfs",
                    span,
                } if in_cell_xfs => {
                    end_tag = Some(span);
                    break;
                }
                _ => {}
            }
        }
        let (Some(open), Some(close)) = (cell_xfs_tag, end_tag) else {
            return Ok(style);
        };
        let src = source.map_or_else(
            || {
                "<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>"
                    .to_owned()
            },
            |s| xml_text[s].to_owned(),
        );
        let copy = edit(&src);
        let new_open = xml::set_attr(&xml_text[open.clone()], "count", &(count + 1).to_string());
        let out = splice(
            &xml_text,
            vec![(open, new_open), (close.start..close.start, copy)],
        );
        self.styles_xml = Some(out.clone());
        self.styles = styles::parse(&out, &self.theme);
        self.derived_styles.insert((style, key), count);
        Ok(count)
    }

    /// `styles.xml` as it stands now: the part's name and its text.
    fn styles_now(&self) -> Result<(String, String)> {
        let part = self
            .styles_part
            .clone()
            .ok_or_else(|| Error::Refused("The workbook has no styles part".into()))?;
        let text = match &self.styles_xml {
            Some(t) => t.clone(),
            None => text_of(self.pkg.part(&part)?, &part)?,
        };
        Ok((part, text))
    }

    /// A copy of the style at index `style` with `change` made: a new
    /// `<font>` and `<fill>` where the change touches them, and a new
    /// `<xf>`; each the same one again when `styles.xml` has it already.
    fn restyle(
        &mut self,
        style: u32,
        change: &kalem_viewer::StyleChange,
        sides: [Option<bool>; 4],
    ) -> Result<u32> {
        let (_, mut text) = self.styles_now()?;
        let xfs = style_children(&text, "cellXfs")
            .ok_or_else(|| Error::Refused("styles.xml has no cell formats".into()))?;
        let mut xf = xfs
            .get(style as usize)
            .map(|s| text[s.clone()].to_owned())
            .unwrap_or_else(|| {
                "<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>".into()
            });
        let id = |xf: &str, k: &str| -> usize {
            xf.split(&format!(" {k}=\""))
                .nth(1)
                .and_then(|v| v.split('"').next()?.parse().ok())
                .unwrap_or(0)
        };
        let font_touched = change.bold.is_some()
            || change.italic.is_some()
            || change.underline.is_some()
            || change.strike.is_some()
            || change.color.is_some()
            || change.size.is_some()
            || change.face.is_some();
        if font_touched {
            let fonts = style_children(&text, "fonts")
                .ok_or_else(|| Error::Refused("styles.xml has no fonts".into()))?;
            let src = fonts
                .get(id(&xf, "fontId"))
                .map(|s| text[s.clone()].to_owned())
                .unwrap_or_else(|| "<font/>".into());
            let p = xml::prefix(
                src.trim_start_matches('<')
                    .split([' ', '/', '>'])
                    .next()
                    .unwrap_or(""),
            )
            .to_owned();
            let mut f = src;
            for (name, on) in [
                ("b", change.bold),
                ("i", change.italic),
                ("u", change.underline),
                ("strike", change.strike),
            ] {
                if let Some(on) = on {
                    let el = if on {
                        format!("<{p}{name}/>")
                    } else {
                        String::new()
                    };
                    f = crate::chart::set_child(&f, &[name], &el, &[]);
                }
            }
            if let Some(sz) = change.size {
                f = crate::chart::set_child(
                    &f,
                    &["sz"],
                    &format!("<{p}sz val=\"{sz}\"/>"),
                    &["b", "i", "strike", "u"],
                );
            }
            if let Some(c) = change.color {
                let el = c.map_or(String::new(), |[r, g, b]| {
                    format!("<{p}color rgb=\"FF{r:02X}{g:02X}{b:02X}\"/>")
                });
                f = crate::chart::set_child(&f, &["color"], &el, &["b", "i", "strike", "u", "sz"]);
            }
            if let Some(face) = &change.face {
                // A theme's font (`scheme`) would win over the name.
                f = crate::chart::set_child(&f, &["scheme"], "", &[]);
                f = crate::chart::set_child(
                    &f,
                    &["name"],
                    &format!("<{p}name val=\"{}\"/>", xml::escape(face)),
                    &["b", "i", "strike", "u", "sz", "color"],
                );
            }
            let (t, fid) = add_style_child(&text, "fonts", &f);
            text = t;
            xf = xml::set_attr(&xf, "fontId", &fid.to_string());
            xf = xml::set_attr(&xf, "applyFont", "1");
        }
        if let Some(fill) = change.fill {
            let fid = match fill {
                None => 0,
                Some([r, g, b]) => {
                    let el = format!(
                        "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{r:02X}{g:02X}{b:02X}\"/><bgColor indexed=\"64\"/></patternFill></fill>"
                    );
                    let (t, fid) = add_style_child(&text, "fills", &el);
                    text = t;
                    fid
                }
            };
            xf = xml::set_attr(&xf, "fillId", &fid.to_string());
            xf = xml::set_attr(&xf, "applyFill", "1");
        }
        if sides.iter().any(Option::is_some) {
            let borders = style_children(&text, "borders")
                .ok_or_else(|| Error::Refused("styles.xml has no borders".into()))?;
            let src = borders
                .get(id(&xf, "borderId"))
                .map(|s| text[s.clone()].to_owned())
                .unwrap_or_else(|| "<border/>".into());
            let p = xml::prefix(
                src.trim_start_matches('<')
                    .split([' ', '/', '>'])
                    .next()
                    .unwrap_or(""),
            )
            .to_owned();
            let (set, color) = change
                .borders
                .unwrap_or((kalem_viewer::BorderSet::None, None));
            let weight = if set == kalem_viewer::BorderSet::ThickOutside {
                "medium"
            } else {
                "thin"
            };
            let color = color.map_or(format!("<{p}color auto=\"1\"/>"), |[r, g, b]| {
                format!("<{p}color rgb=\"FF{r:02X}{g:02X}{b:02X}\"/>")
            });
            let mut f = src;
            // CT_Border's order: left (start), right (end), top, bottom.
            let order: [(&[&str], &str, &[&str]); 4] = [
                (&["top"], "top", &["left", "start", "right", "end"]),
                (&["right", "end"], "right", &["left", "start"]),
                (
                    &["bottom"],
                    "bottom",
                    &["left", "start", "right", "end", "top"],
                ),
                (&["left", "start"], "left", &[]),
            ];
            for (draw, (names, name, after)) in sides.iter().zip(order) {
                let Some(draw) = draw else { continue };
                let el = if *draw {
                    format!("<{p}{name} style=\"{weight}\">{color}</{p}{name}>")
                } else {
                    format!("<{p}{name}/>")
                };
                f = crate::chart::set_child(&f, names, &el, after);
            }
            let (t, bid) = add_style_child(&text, "borders", &f);
            text = t;
            xf = xml::set_attr(&xf, "borderId", &bid.to_string());
            xf = xml::set_attr(&xf, "applyBorder", "1");
        }
        if let Some(code) = &change.number_format {
            let (t, id) = with_num_fmt(&text, code);
            text = t;
            xf = xml::set_attr(&xf, "numFmtId", &id.to_string());
            xf = xml::set_attr(&xf, "applyNumberFormat", "1");
        }
        if let Some(a) = change.align {
            let v = match a {
                kalem_viewer::Align::Left => "left",
                kalem_viewer::Align::Center => "center",
                kalem_viewer::Align::Right => "right",
                _ => "general",
            };
            xf = with_alignment(&xf, "horizontal", v);
        }
        if let Some(a) = change.valign {
            let v = match a {
                kalem_viewer::VAlign::Top => "top",
                kalem_viewer::VAlign::Middle => "center",
                _ => "bottom",
            };
            xf = with_alignment(&xf, "vertical", v);
        }
        let (t, xid) = add_style_child(&text, "cellXfs", &xf);
        text = t;
        self.styles = styles::parse(&text, &self.theme);
        self.styles_xml = Some(text);
        Ok(xid as u32)
    }

    /// Changes the format of a range's cells, as Format Cells: each cell's
    /// style copied with the change made (the same new style for the same
    /// old one), empty cells given one too. One undo step.
    pub fn change_style(
        &mut self,
        idx: usize,
        range: Range,
        change: &kalem_viewer::StyleChange,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let area = u64::from(range.end.row - range.start.row + 1)
            * u64::from(range.end.col - range.start.col + 1);
        if area > 200_000 {
            return Err(Error::Refused("Select fewer cells: 200,000 at most".into()));
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = (|| -> Result<()> {
            let mut made: HashMap<(u32, [Option<bool>; 4]), u32> = HashMap::new();
            let others = kalem_viewer::StyleChange {
                borders: None,
                ..change.clone()
            } != kalem_viewer::StyleChange::default();
            for row in range.start.row..=range.end.row {
                for col in range.start.col..=range.end.col {
                    let at = CellRef::new(row, col);
                    let sides = border_sides(change, range, at);
                    // A cell inside an outside border has nothing to change.
                    if !others && sides.iter().all(Option::is_none) {
                        continue;
                    }
                    let old = self.loaded[&idx]
                        .1
                        .cells
                        .get(&at)
                        .map_or_else(|| self.row_or_col_style(idx, at), |c| c.style);
                    let new = match made.get(&(old, sides)) {
                        Some(n) => *n,
                        None => {
                            let n = self.restyle(old, change, sides)?;
                            made.insert((old, sides), n);
                            n
                        }
                    };
                    if new != old || !self.loaded[&idx].1.cells.contains_key(&at) {
                        self.apply_style(idx, at, new)?;
                    }
                }
            }
            Ok(())
        })();
        self.batch_changed = true;
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Pastes cells as Excel's Paste Special does: all of them, their
    /// values, their formats or their formulas (moved as copied formulas
    /// move), rows turned into columns when `transpose`. One undo step.
    pub fn paste_cells(
        &mut self,
        from: (usize, Range),
        to: (usize, CellRef),
        kind: kalem_viewer::PasteKind,
        transpose: bool,
    ) -> Result<()> {
        use kalem_viewer::PasteKind;
        let ((si, src), (di, at)) = (from, to);
        for i in [si, di] {
            self.load(i)?;
            if self.sheets[i].kind != SheetKind::Worksheet {
                return Err(Error::NotAWorksheet(self.sheets[i].name.clone()));
            }
        }
        let (h, w) = (
            src.end.row - src.start.row + 1,
            src.end.col - src.start.col + 1,
        );
        let (dh, dw) = if transpose { (w, h) } else { (h, w) };
        if u64::from(h) * u64::from(w) > 200_000 {
            return Err(Error::Refused("Paste 200,000 cells at most".into()));
        }
        if at.row + dh > MAX_ROW || at.col + dw > MAX_COL {
            return Err(Error::Refused(
                "The cells would go past the sheet's edge".into(),
            ));
        }
        // What the source has, read whole before anything is written: the
        // source and the cells pasted to may overlap.
        let mut items = Vec::new();
        for r in src.start.row..=src.end.row {
            for c in src.start.col..=src.end.col {
                let pos = CellRef::new(r, c);
                let cell = self.loaded[&si].1.cells.get(&pos).cloned();
                let value = self.value(si, pos)?;
                let style = cell
                    .as_ref()
                    .map_or_else(|| self.row_or_col_style(si, pos), |c| c.style);
                let formula = cell.and_then(|c| c.formula.map(|f| f.text));
                let (dr, dc) = (r - src.start.row, c - src.start.col);
                let dest = if transpose {
                    CellRef::new(at.row + dc, at.col + dr)
                } else {
                    CellRef::new(at.row + dr, at.col + dc)
                };
                items.push((pos, dest, value, formula, style));
            }
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = (|| -> Result<()> {
            for (pos, dest, value, formula, style) in items {
                let constant = match value {
                    Value::Number(v) => Input::Number(v, None),
                    Value::Text(t) => Input::Text(t),
                    Value::Bool(b) => Input::Bool(b),
                    Value::Error(e) => Input::Error(e),
                    Value::Empty => Input::Clear,
                };
                let input = match (kind, formula) {
                    (PasteKind::Formats, _) => None,
                    (PasteKind::Values, _) => Some(constant),
                    (_, Some(f)) => Some(Input::Formula(formula::shift(
                        &f,
                        i64::from(dest.row) - i64::from(pos.row),
                        i64::from(dest.col) - i64::from(pos.col),
                    ))),
                    (_, None) => Some(constant),
                };
                if let Some(input) = input {
                    self.set_input(di, dest, input)?;
                }
                if matches!(kind, PasteKind::All | PasteKind::Formats) {
                    let now = self.loaded[&di]
                        .1
                        .cells
                        .get(&dest)
                        .map_or_else(|| self.row_or_col_style(di, dest), |c| c.style);
                    if now != style {
                        self.apply_style(di, dest, style)?;
                    }
                }
            }
            Ok(())
        })();
        self.batch_changed = true;
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Turns wrapping of a cell's text on or off, as Excel's Wrap Text: the
    /// cell gets a copy of its `<xf>` with `<alignment wrapText>`.
    pub fn set_wrap(&mut self, idx: usize, at: CellRef, wrap: bool) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let old = self.loaded[&idx].1.cells.get(&at).cloned();
        let style = old.as_ref().map_or(0, |c| c.style);
        if self.styles.get(style).wrap == wrap {
            return Ok(());
        }
        // Keys past any number format id: one for on, one for off.
        let key = if wrap { u32::MAX } else { u32::MAX - 1 };
        let new_style = self.derive_style(style, key, |src| with_wrap(src, wrap))?;
        self.apply_style(idx, at, new_style)?;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Gives a cell the style `new_style`: its `s`, or a `<c>` made to
    /// carry it. No undo step of its own.
    fn apply_style(&mut self, idx: usize, at: CellRef, new_style: u32) -> Result<()> {
        let (text, model) = &self.loaded[&idx];
        let splices = match model.cells.get(&at) {
            Some(c) => {
                let el = &text[c.span.clone()];
                let tag_end = el.find('>').map_or(el.len(), |p| p + 1);
                let head = if new_style == 0 {
                    xml::remove_attr(&el[..tag_end], "s")
                } else {
                    xml::set_attr(&el[..tag_end], "s", &new_style.to_string())
                };
                vec![(c.span.start..c.span.start + tag_end, head)]
            }
            None => {
                let p = &model.prefix;
                insert_cell(
                    text,
                    model,
                    at,
                    format!("<{p}c r=\"{at}\" s=\"{new_style}\"/>"),
                )?
            }
        };
        let new = splice(text, splices);
        self.replace_sheet_text(idx, new);
        Ok(())
    }

    /// Counts the changes to sheet texts: what was computed from a sheet
    /// at one count holds until it moves.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The differential formats of `styles.xml`, which conditional formats name.
    pub fn dxfs(&self) -> &[styles::Dxf] {
        &self.styles.dxfs
    }

    /// A worksheet's conditional formats.
    pub fn conditional_formats(&mut self, idx: usize) -> Result<Vec<conditional::CondFormat>> {
        self.load(idx)?;
        Ok(conditional::parse(&self.loaded[&idx].0, &self.theme))
    }

    /// Adds a conditional format to `range`, as Excel's Conditional
    /// Formatting menu: the new rule comes first, before the sheet's others.
    pub fn add_conditional_format(
        &mut self,
        idx: usize,
        range: Range,
        rule: &kalem_viewer::CondRule,
        style: &kalem_viewer::CondStyle,
    ) -> Result<()> {
        use kalem_viewer::CondRule;
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let highlights = !matches!(
            rule,
            CondRule::ColorScale(_) | CondRule::DataBar(_) | CondRule::IconSet(_)
        );
        let dxf = if highlights {
            Some(self.add_dxf(&conditional::dxf_xml(style))?)
        } else {
            None
        };
        let (text, model) = &self.loaded[&idx];
        let p = model.prefix.clone();
        // Every rule already there moves one place down.
        let mut r = Reader::new(text);
        let mut splices = Vec::new();
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "cfRule"
                && let Some(n) = tag.attr("priority").and_then(|v| v.parse::<i64>().ok())
            {
                let head = xml::set_attr(&text[tag.span.clone()], "priority", &(n + 1).to_string());
                splices.push((tag.span.clone(), head));
            }
        }
        let shifted = splice(text, splices);
        let block = format!(
            "<{p}conditionalFormatting sqref=\"{range}\">{}</{p}conditionalFormatting>",
            conditional::rule_xml(&p, rule, dxf, range.start)
        );
        let new = insert_top_level(&shifted, &AFTER_MERGE_CELLS[2..], &block);
        self.replace_sheet_text(idx, new);
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Takes away the conditional formats of the ranges that meet `range`,
    /// or all of the sheet's; `false` when there were none.
    pub fn clear_conditional_formats(&mut self, idx: usize, range: Option<Range>) -> Result<bool> {
        let formats = self.conditional_formats(idx)?;
        let meets = |a: &Range| {
            range.is_none_or(|b| {
                a.start.row <= b.end.row
                    && b.start.row <= a.end.row
                    && a.start.col <= b.end.col
                    && b.start.col <= a.end.col
            })
        };
        let text = &self.loaded[&idx].0;
        let mut splices = Vec::new();
        for f in &formats {
            if !f.ranges.iter().any(&meets) {
                continue;
            }
            let kept: Vec<String> = f
                .ranges
                .iter()
                .filter(|r| !meets(r))
                .map(ToString::to_string)
                .collect();
            if kept.is_empty() {
                splices.push((f.span.clone(), String::new()));
            } else {
                let el = &text[f.span.clone()];
                let tag_end = el.find('>').map_or(el.len(), |p| p + 1);
                let head = xml::set_attr(&el[..tag_end], "sqref", &kept.join(" "));
                splices.push((f.span.start..f.span.start + tag_end, head));
            }
        }
        if splices.is_empty() {
            return Ok(false);
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let new = splice(text, splices);
        self.replace_sheet_text(idx, new);
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(true)
    }

    /// A worksheet's data validations.
    pub fn validations(&mut self, idx: usize) -> Result<Vec<validation::DataValidation>> {
        self.load(idx)?;
        if let Some((g, v)) = self.validations.get(&idx)
            && *g == self.generation
        {
            return Ok(v.clone());
        }
        let v = validation::parse(&self.loaded[&idx].0);
        self.validations.insert(idx, (self.generation, v.clone()));
        Ok(v)
    }

    /// The data validation of a cell, if any.
    pub fn validation_at(
        &mut self,
        idx: usize,
        at: CellRef,
    ) -> Result<Option<validation::DataValidation>> {
        Ok(self.validations(idx)?.into_iter().find(|v| v.covers(at)))
    }

    /// A list validation's values as they read now: its own, or the cells
    /// of the range (or the defined name) it names, each once.
    pub fn list_values(
        &mut self,
        idx: usize,
        dv: &validation::DataValidation,
        at: CellRef,
    ) -> Vec<String> {
        let Some(f) = &dv.formula1 else {
            return Vec::new();
        };
        if let Some(l) = validation::literal_list(f) {
            return l;
        }
        let mut f = f.trim().trim_start_matches('=').to_owned();
        if let Some(d) = self
            .defined_names
            .iter()
            .filter(|d| d.name.eq_ignore_ascii_case(&f))
            .min_by_key(|d| d.local_sheet != Some(idx))
        {
            f = d.refers_to.trim_start_matches('=').to_owned();
        }
        let moved = conditional::moved(&f, dv.first(), at);
        let (sheet, reference) = match moved.rsplit_once('!') {
            Some((s, r)) => (
                self.sheet_index(&s.trim_matches('\'').replace("''", "'")),
                r,
            ),
            None => (Some(idx), moved.as_str()),
        };
        let (Some(si), Some(range)) = (sheet, Range::parse(&reference.replace('$', ""))) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for row in range.start.row..=range.end.row.min(range.start.row + 999) {
            for col in range.start.col..=range.end.col.min(range.start.col + 99) {
                let t = self.display(si, CellRef::new(row, col)).unwrap_or_default();
                if !t.is_empty() && !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        out
    }

    /// A validation's bound for a cell: a number, or a formula's value.
    fn validation_bound(
        &mut self,
        idx: usize,
        f: &str,
        first: CellRef,
        at: CellRef,
    ) -> Option<f64> {
        let t = f.trim().trim_start_matches('=');
        if let Ok(n) = t.parse::<f64>() {
            return Some(n);
        }
        match self
            .evaluate_formula(idx, &conditional::moved(t, first, at))
            .ok()
            .flatten()?
        {
            Value::Number(n) => Some(n),
            _ => None,
        }
    }

    /// Whether a cell's value is one its validation accepts; blank cells
    /// are, as Excel neither checks a cleared cell nor circles a blank one.
    pub fn accepts(
        &mut self,
        idx: usize,
        at: CellRef,
        dv: &validation::DataValidation,
    ) -> Result<bool> {
        let value = self.value(idx, at)?;
        let shown = self.display(idx, at)?;
        if value == Value::Empty || shown.is_empty() {
            return Ok(true);
        }
        let first = dv.first();
        let number = match dv.kind.as_str() {
            "none" => return Ok(true),
            "list" => {
                let items = self.list_values(idx, dv, at);
                let n = match value {
                    Value::Number(n) => Some(n),
                    _ => None,
                };
                return Ok(items.iter().any(|i| {
                    i.trim().eq_ignore_ascii_case(shown.trim())
                        || i.to_lowercase() == shown.to_lowercase()
                        || n.is_some_and(|n| i.trim().parse::<f64>() == Ok(n))
                }));
            }
            "custom" => {
                let f = dv.formula1.clone().unwrap_or_default();
                let v = self.evaluate_formula(
                    idx,
                    &conditional::moved(f.trim_start_matches('='), first, at),
                )?;
                return Ok(match v {
                    Some(Value::Bool(b)) => b,
                    Some(Value::Number(n)) => n != 0.0,
                    _ => false,
                });
            }
            "textLength" => shown.chars().count() as f64,
            kind => match value {
                Value::Number(n) if kind != "whole" || n.fract() == 0.0 => n,
                _ => return Ok(false),
            },
        };
        let a = dv
            .formula1
            .clone()
            .and_then(|f| self.validation_bound(idx, &f, first, at));
        let b = dv
            .formula2
            .clone()
            .and_then(|f| self.validation_bound(idx, &f, first, at));
        let Some(a) = a else {
            return Ok(true);
        };
        Ok(match dv.operator.as_str() {
            "greaterThan" => number > a,
            "lessThan" => number < a,
            "greaterThanOrEqual" => number >= a,
            "lessThanOrEqual" => number <= a,
            "equal" => number == a,
            "notEqual" => number != a,
            op => {
                let b = b.unwrap_or(a);
                let inside = number >= a.min(b) && number <= a.max(b);
                inside == (op != "notBetween")
            }
        })
    }

    /// Computes together the formulas the validations of `cells` need
    /// (custom rules, bounds that are formulas), as the grid circles a
    /// screenful of invalid data: one recalculation, not one a cell.
    pub fn prefetch_validations(
        &mut self,
        idx: usize,
        cells: &[(CellRef, &validation::DataValidation)],
    ) {
        let mut formulas = Vec::new();
        for (at, dv) in cells {
            let first = dv.first();
            let custom = dv.kind == "custom";
            for f in [&dv.formula1, &dv.formula2].into_iter().flatten() {
                let t = f.trim().trim_start_matches('=');
                if custom || (dv.kind != "list" && t.parse::<f64>().is_err()) {
                    formulas.push(conditional::moved(t, first, *at));
                }
            }
        }
        if !formulas.is_empty() {
            let _ = self.evaluate_formulas(idx, &formulas);
        }
    }

    /// The validation an entry breaks if typed into a cell, as Excel checks
    /// it on Enter: tried, read, and taken back, the history kept.
    pub fn check_entry(
        &mut self,
        idx: usize,
        at: CellRef,
        entry: &str,
    ) -> Result<Option<validation::DataValidation>> {
        let Some(dv) = self.validation_at(idx, at)? else {
            return Ok(None);
        };
        if dv.kind == "none" || !dv.show_error || entry.is_empty() || self.batch.is_some() {
            return Ok(None);
        }
        let redo = std::mem::take(&mut self.redo);
        let tried = self.set_cell(idx, at, entry);
        let accepted = match tried {
            Ok(()) => {
                let ok = self.accepts(idx, at, &dv);
                self.rollback();
                ok
            }
            Err(e) => Err(e),
        };
        self.redo = redo;
        Ok((!accepted?).then_some(dv))
    }

    /// Sets the data validation of a range, or removes it: what its cells
    /// had goes, the rest of each validation's ranges kept. One undo step.
    pub fn set_validation(
        &mut self,
        idx: usize,
        range: Range,
        v: Option<&kalem_viewer::Validation>,
    ) -> Result<()> {
        use kalem_viewer::{CompareOp, ValidationKind};
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        // The formulas first: a value that is not one refuses the edit.
        let number = |s: &str, date1904: bool| -> Result<String> {
            let t = s.trim();
            if let Some(f) = t.strip_prefix('=') {
                return Ok(f.to_owned());
            }
            match Input::parse(t, date1904) {
                Input::Number(n, _) => Ok(format!("{n}")),
                _ => Err(Error::Refused(format!("Not a number, date or time: {t}"))),
            }
        };
        let formulas = match v {
            None => (None, None),
            Some(v) => match v.kind {
                ValidationKind::Any => (None, None),
                ValidationKind::Custom => (
                    Some(v.value.trim().trim_start_matches('=').to_owned()),
                    None,
                ),
                ValidationKind::List => {
                    let t = v.value.trim();
                    if let Some(f) = t.strip_prefix('=') {
                        (Some(f.to_owned()), None)
                    } else {
                        let items: Vec<&str> = t
                            .split(',')
                            .map(str::trim)
                            .filter(|i| !i.is_empty())
                            .collect();
                        let joined = items.join(",");
                        if items.is_empty() || joined.chars().count() > 255 {
                            return Err(Error::Refused(
                                "A list takes values separated by commas, 255 characters at most"
                                    .into(),
                            ));
                        }
                        (Some(format!("\"{}\"", joined.replace('"', "\"\""))), None)
                    }
                }
                _ => {
                    let two = matches!(v.op, CompareOp::Between | CompareOp::NotBetween);
                    let second = match (&v.value2, two) {
                        (Some(s), true) => Some(number(s, self.date1904)?),
                        (None, true) => {
                            return Err(Error::Refused("Between takes two values".into()));
                        }
                        _ => None,
                    };
                    (Some(number(&v.value, self.date1904)?), second)
                }
            },
        };
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let (text, model) = &self.loaded[&idx];
        let p = model.prefix.clone();
        let mut splices = Vec::new();
        for dv in validation::parse(text) {
            if !dv
                .ranges
                .iter()
                .any(|r| validation::subtract(*r, range) != vec![*r])
            {
                continue;
            }
            let kept: Vec<String> = dv
                .ranges
                .iter()
                .flat_map(|r| validation::subtract(*r, range))
                .map(|r| validation::range_text(&r))
                .collect();
            if kept.is_empty() {
                splices.push((dv.span.clone(), String::new()));
                continue;
            }
            let Some(sq) = dv.sqref_span.clone() else {
                continue;
            };
            let el = &text[sq.clone()];
            let new = if el.contains("sqref=") {
                xml::set_attr(el, "sqref", &kept.join(" "))
            } else {
                // The x14 form's `<xm:sqref>` element.
                let open = el.find('>').map_or(0, |i| i + 1);
                let close = el.rfind("</").unwrap_or(el.len());
                format!("{}{}{}", &el[..open], kept.join(" "), &el[close..])
            };
            splices.push((sq, new));
        }
        let mut new = splice(text, splices);
        if let Some(v) = v {
            let el = validation::element(
                &p,
                v,
                formulas.0.as_deref(),
                formulas.1.as_deref(),
                &validation::range_text(&range),
            );
            new = add_validation(&new, &p, &el);
        }
        let new = tidy_validations(&new);
        if new == self.loaded[&idx].0 {
            return Ok(());
        }
        self.replace_sheet_text(idx, new);
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// A sheet's drawing part, if it has one.
    fn sheet_drawing(&self, idx: usize) -> Option<String> {
        let part = &self.sheets.get(idx)?.part;
        let text = self.pkg.part(&rels::rels_path(part)).ok()?;
        rels::parse(&String::from_utf8_lossy(&text))
            .into_iter()
            .find(|r| r.kind == "drawing" && !r.external)
            .map(|r| rels::resolve(part, &r.target))
    }

    /// The part a drawing's relationship names.
    fn rel_target(&self, source: &str, rid: &str) -> Option<String> {
        let text = self.pkg.part(&rels::rels_path(source)).ok()?;
        rels::parse(&String::from_utf8_lossy(&text))
            .into_iter()
            .find(|r| r.id == rid && !r.external)
            .map(|r| rels::resolve(source, &r.target))
    }

    /// The sheet and range a chart's reference names (`Sheet1!$B$2:$B$5`).
    fn chart_range(&self, f: &str) -> Option<(usize, Range)> {
        let f = f.trim().trim_start_matches('(').trim_end_matches(')');
        if f.contains(',') {
            return None;
        }
        let (sheet, r) = f.rsplit_once('!')?;
        let sheet = sheet.trim_matches('\'').replace("''", "'");
        Some((
            self.sheet_index(&sheet)?,
            Range::parse(&r.replace('$', ""))?,
        ))
    }

    fn range_texts(&mut self, idx: usize, r: Range) -> Vec<String> {
        let mut out = Vec::new();
        for row in r.start.row..=r.end.row.min(r.start.row + 9999) {
            for col in r.start.col..=r.end.col.min(r.start.col + 999) {
                out.push(
                    self.display(idx, CellRef::new(row, col))
                        .unwrap_or_default(),
                );
            }
        }
        out
    }

    fn range_numbers(&mut self, idx: usize, r: Range) -> Vec<Option<f64>> {
        let mut out = Vec::new();
        for row in r.start.row..=r.end.row.min(r.start.row + 9999) {
            for col in r.start.col..=r.end.col.min(r.start.col + 999) {
                out.push(match self.value(idx, CellRef::new(row, col)) {
                    Ok(Value::Number(n)) => Some(n),
                    _ => None,
                });
            }
        }
        out
    }

    /// A sheet's charts as they read now: their values from the cells they
    /// name, else from what the file cached.
    pub fn charts(&mut self, idx: usize) -> Result<Vec<kalem_viewer::Chart>> {
        let axis_font = |f: &chart::Font| kalem_viewer::AxisFont {
            size: f.size,
            bold: f.bold,
            italic: f.italic,
            color: f.color.map(|c| [(c >> 16) as u8, (c >> 8) as u8, c as u8]),
            face: f.face.clone(),
        };
        let paint = |f: chart::Fill| match f {
            chart::Fill::Auto => kalem_viewer::Paint::Automatic,
            chart::Fill::None => kalem_viewer::Paint::None,
            chart::Fill::Color(c) => {
                kalem_viewer::Paint::Color([(c >> 16) as u8, (c >> 8) as u8, c as u8])
            }
        };
        let Some(drawing) = self.sheet_drawing(idx) else {
            return Ok(Vec::new());
        };
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let mut out = Vec::new();
        for a in chart::parse_drawing(&text) {
            let Some(part) = self.rel_target(&drawing, &a.rid) else {
                continue;
            };
            let Ok(bytes) = self.pkg.part(&part) else {
                continue;
            };
            let def = chart::parse_chart(&String::from_utf8_lossy(&bytes), &self.theme);
            let mut categories = Vec::new();
            let mut series = Vec::new();
            for s in &def.series {
                let (name_r, cat_r, val_r) = (
                    s.name.0.as_deref().and_then(|f| self.chart_range(f)),
                    s.cat.0.as_deref().and_then(|f| self.chart_range(f)),
                    s.val.0.as_deref().and_then(|f| self.chart_range(f)),
                );
                let name = match name_r {
                    Some((i, r)) => self
                        .range_texts(
                            i,
                            Range {
                                start: r.start,
                                end: r.start,
                            },
                        )
                        .join(""),
                    None => s.name.1.clone(),
                };
                let cats = match cat_r {
                    Some((i, r)) => self.range_texts(i, r),
                    None => s.cat.1.clone(),
                };
                let x: Vec<Option<f64>> = if def.kind == kalem_viewer::ChartKind::Scatter {
                    match cat_r {
                        Some((i, r)) => self.range_numbers(i, r),
                        None => s.cat.1.iter().map(|v| v.trim().parse().ok()).collect(),
                    }
                } else {
                    Vec::new()
                };
                let values = match val_r {
                    Some((i, r)) => self.range_numbers(i, r),
                    None => s.val.1.clone(),
                };
                if categories.is_empty() {
                    categories = cats;
                }
                // Fields the contract gains later start empty.
                #[allow(clippy::needless_update)]
                series.push(kalem_viewer::ChartSeries {
                    name,
                    values,
                    x,
                    color: s.color.map(|c| [(c >> 16) as u8, (c >> 8) as u8, c as u8]),
                    point_colors: s
                        .points
                        .iter()
                        .map(|(i, c)| (*i, [(c >> 16) as u8, (c >> 8) as u8, *c as u8]))
                        .collect(),
                    explosion: s.explosion,
                    point_explosions: s.point_explosions.clone(),
                    ..kalem_viewer::ChartSeries::default()
                });
            }
            // Fields the contract gains later start empty instead of
            // breaking the build of Kalem's pinned revision.
            #[allow(clippy::needless_update)]
            out.push(kalem_viewer::Chart {
                kind: def.kind,
                title: def.title.clone().or_else(|| {
                    if def.title_deleted {
                        return None;
                    }
                    // One series and no title: Excel shows the series' name.
                    (series.len() == 1 && !series[0].name.is_empty())
                        .then(|| series[0].name.clone())
                }),
                categories,
                series,
                anchor: [a.from.0, a.from.1, a.to.0, a.to.1],
                stacked: def.stacked,
                horizontal_title: def.horizontal_title.clone(),
                vertical_title: def.vertical_title.clone(),
                legend: def.legend.as_deref().map(|v| match v {
                    "b" => kalem_viewer::LegendPosition::Bottom,
                    "t" => kalem_viewer::LegendPosition::Top,
                    "l" => kalem_viewer::LegendPosition::Left,
                    "tr" => kalem_viewer::LegendPosition::TopRight,
                    _ => kalem_viewer::LegendPosition::Right,
                }),
                labels: kalem_viewer::DataLabels {
                    value: def.labels.0,
                    category: def.labels.1,
                    series: def.labels.2,
                    percent: def.labels.3,
                },
                scale: kalem_viewer::AxisScale {
                    min: def.scale.0,
                    max: def.scale.1,
                    major: def.scale.2,
                    log: def.scale.3,
                },
                background: paint(def.background),
                border: paint(def.border),
                plot_background: paint(def.plot_background),
                plot_border: paint(def.plot_border),
                axis_format: def.axis_format.clone(),
                horizontal_font: axis_font(&def.horizontal_font),
                vertical_font: axis_font(&def.vertical_font),
                title_font: axis_font(&def.title_font),
                legend_font: axis_font(&def.legend_font),
                gridlines: kalem_viewer::Gridlines {
                    horizontal_major: def.gridlines.0,
                    horizontal_minor: def.gridlines.1,
                    vertical_major: def.gridlines.2,
                    vertical_minor: def.gridlines.3,
                },
                ..kalem_viewer::Chart::default()
            });
        }
        Ok(out)
    }

    /// Inserts a chart of a range beside it, as Excel's Insert Chart: the
    /// series in its columns, or its rows when it is wider than tall; a
    /// first row or column of text names them and the categories. One
    /// undo step.
    pub fn insert_chart(
        &mut self,
        idx: usize,
        range: Range,
        kind: kalem_viewer::ChartKind,
        title: Option<&str>,
    ) -> Result<()> {
        use kalem_viewer::ChartKind;
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let sheet = self.sheets[idx].name.clone();
        let is_text = |wb: &mut Self, r: u32, c: u32| {
            matches!(wb.value(idx, CellRef::new(r, c)), Ok(Value::Text(_)))
        };
        let is_empty = |wb: &mut Self, r: u32, c: u32| {
            matches!(wb.value(idx, CellRef::new(r, c)), Ok(Value::Empty))
        };
        let (r0, c0, r1, c1) = (
            range.start.row,
            range.start.col,
            range.end.row,
            range.end.col,
        );
        let corner_empty = is_empty(self, r0, c0);
        let header_row =
            r1 > r0 && (corner_empty || ((c0 + 1).max(c0)..=c1).any(|c| is_text(self, r0, c)));
        let label_col = c1 > c0
            && kind != ChartKind::Scatter
            && (corner_empty || ((r0 + 1)..=r1).any(|r| is_text(self, r, c0)));
        let data_r0 = r0 + u32::from(header_row);
        let data_c0 = c0 + u32::from(label_col || (kind == ChartKind::Scatter && c1 > c0));
        if data_r0 > r1 || data_c0 > c1 {
            return Err(Error::Refused(
                "A chart needs numbers under or beside its labels".into(),
            ));
        }
        let rect = |a: (u32, u32), b: (u32, u32)| Range {
            start: CellRef::new(a.0, a.1),
            end: CellRef::new(b.0, b.1),
        };
        let in_rows = kind != ChartKind::Scatter && (c1 - data_c0) > (r1 - data_r0);
        let mut series = Vec::new();
        let lines: Vec<u32> = if in_rows {
            (data_r0..=r1).collect()
        } else {
            (data_c0..=c1).collect()
        };
        for line in lines {
            let (val_r, name_at, cat_r) = if in_rows {
                (
                    rect((line, data_c0), (line, c1)),
                    label_col.then_some((line, c0)),
                    header_row.then(|| rect((r0, data_c0), (r0, c1))),
                )
            } else {
                (
                    rect((data_r0, line), (r1, line)),
                    header_row.then_some((r0, line)),
                    (label_col || (kind == ChartKind::Scatter && c1 > c0))
                        .then(|| rect((data_r0, c0), (r1, c0))),
                )
            };
            let name = match name_at {
                Some((r, c)) => {
                    let text = self.display(idx, CellRef::new(r, c))?;
                    Some((chart::reference(&sheet, rect((r, c), (r, c))), text))
                }
                None => None,
            };
            let cat = match cat_r {
                Some(cr) => {
                    let texts = self.range_texts(idx, cr);
                    Some((
                        chart::reference(&sheet, cr),
                        texts,
                        kind == ChartKind::Scatter,
                    ))
                }
                None => None,
            };
            let values = self.range_numbers(idx, val_r);
            series.push(chart::NewSeries {
                name,
                cat,
                val: (chart::reference(&sheet, val_r), values),
                color: None,
            });
        }
        if series.iter().all(|s| s.val.1.iter().all(Option::is_none)) {
            return Err(Error::Refused("A chart needs numbers to draw".into()));
        }
        if matches!(kind, ChartKind::Pie | ChartKind::Doughnut) {
            series.truncate(1);
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let chart_part = self.free_part("xl/charts/chart");
        self.add_part(
            &chart_part,
            chart::chart_xml(kind, title, &series),
            "application/vnd.openxmlformats-officedocument.drawingml.chart+xml",
        )?;
        let from = (r0, c1 + 2);
        let to = (r0 + 14, c1 + 9);
        let sheet_part = self.sheets[idx].part.clone();
        match self.sheet_drawing(idx) {
            Some(drawing) => {
                let rid = self.add_rel(&drawing, "chart", &chart_part)?;
                let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
                let prefix = text
                    .find("wsDr")
                    .and_then(|i| text[..i].rfind('<').map(|s| text[s + 1..i].to_owned()))
                    .unwrap_or_default();
                let anchor =
                    chart::anchor_xml(&prefix, from, to, chart::max_shape_id(&text) + 1, &rid);
                let close = text.rfind("</").unwrap_or(text.len());
                self.pkg.set_part(
                    &drawing,
                    format!("{}{anchor}{}", &text[..close], &text[close..]).into_bytes(),
                );
            }
            None => {
                let drawing = self.free_part("xl/drawings/drawing");
                // The relationship first, so the drawing can name the chart.
                self.pkg.set_part(&drawing, Vec::new());
                let rid = self.add_rel(&drawing, "chart", &chart_part)?;
                let anchor = chart::anchor_xml("xdr:", from, to, 2, &rid);
                self.pkg.remove_part(&drawing);
                self.add_part(
                    &drawing,
                    chart::drawing_xml(&anchor),
                    "application/vnd.openxmlformats-officedocument.drawing+xml",
                )?;
                let sheet_rid = self.add_rel(&sheet_part, "drawing", &drawing)?;
                let (text, model) = &self.loaded[&idx];
                let p = model.prefix.clone();
                let head = &text[..text
                    .find("<sheetData")
                    .or_else(|| text.find("sheetData"))
                    .unwrap_or(text.len())];
                let rp = head.split("xmlns:").skip(1).find_map(|d| {
                    let (pfx, rest) = d.split_once("=\"")?;
                    rest.split('"')
                        .next()
                        .filter(|u| u.ends_with("officeDocument/2006/relationships"))
                        .map(|_| pfx.to_owned())
                });
                let el = match rp {
                    Some(rp) => format!("<{p}drawing {rp}:id=\"{sheet_rid}\"/>"),
                    None => format!(
                        "<{p}drawing xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:id=\"{sheet_rid}\"/>"
                    ),
                };
                let new = insert_top_level(
                    text,
                    &[
                        "legacyDrawing",
                        "legacyDrawingHF",
                        "drawingHF",
                        "picture",
                        "oleObjects",
                        "controls",
                        "webPublishItems",
                        "tableParts",
                        "extLst",
                    ],
                    &el,
                );
                self.replace_sheet_text(idx, new);
            }
        }
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Moves or resizes a sheet's chart (by its place among
    /// [`Workbook::charts`]) to cover `to`'s cells. One undo step.
    pub fn move_chart(&mut self, idx: usize, index: usize, to: Range) -> Result<()> {
        if to.end.row >= MAX_ROW || to.end.col >= MAX_COL {
            return Err(Error::Refused("A chart stays inside the sheet".into()));
        }
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let a = chart::parse_drawing(&text)
            .into_iter()
            .filter(|a| self.rel_target(&drawing, &a.rid).is_some())
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let new = chart::moved_anchor(
            &text[a.span.clone()],
            (to.start.row, to.start.col),
            (to.end.row, to.end.col),
        );
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(
            &drawing,
            format!("{}{new}{}", &text[..a.span.start], &text[a.span.end..]).into_bytes(),
        );
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets or removes the title of a sheet's chart (by its place among
    /// [`Workbook::charts`]). One undo step.
    pub fn set_chart_title(&mut self, idx: usize, index: usize, title: Option<&str>) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        // The title keeps its font when its words change.
        let font = chart::parse_chart(&old, &self.theme).title_font;
        let new = chart::titled(&old, title.map(str::trim).filter(|t| !t.is_empty()), &font);
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets or removes the title of an axis of a sheet's chart (by its
    /// place among [`Workbook::charts`]). One undo step.
    pub fn set_axis_title(
        &mut self,
        idx: usize,
        index: usize,
        vertical: bool,
        title: Option<&str>,
    ) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let new = chart::axis_titled(
            &old,
            vertical,
            title.map(str::trim).filter(|t| !t.is_empty()),
        )
        .ok_or_else(|| Error::Refused("This chart has no such axis".into()))?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Puts the legend of a sheet's chart (by its place among
    /// [`Workbook::charts`]) at `position`, or takes it away. One undo step.
    pub fn set_legend(
        &mut self,
        idx: usize,
        index: usize,
        position: Option<kalem_viewer::LegendPosition>,
    ) -> Result<()> {
        use kalem_viewer::LegendPosition as L;
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let pos = position.map(|p| match p {
            L::Bottom => "b",
            L::Top => "t",
            L::Left => "l",
            L::Right => "r",
            L::TopRight => "tr",
        });
        let new = chart::with_legend(&old, pos);
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets what the data labels of a sheet's chart (by its place among
    /// [`Workbook::charts`]) show, every series alike. One undo step.
    pub fn set_data_labels(
        &mut self,
        idx: usize,
        index: usize,
        labels: kalem_viewer::DataLabels,
    ) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let kind = chart::parse_chart(&old, &self.theme).kind;
        let pie = matches!(
            kind,
            kalem_viewer::ChartKind::Pie | kalem_viewer::ChartKind::Doughnut
        );
        let new = chart::with_labels(
            &old,
            (labels.value, labels.category, labels.series, labels.percent),
            pie,
        );
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets the value axis's scale of a sheet's chart (by its place among
    /// [`Workbook::charts`]). One undo step.
    pub fn set_axis_scale(
        &mut self,
        idx: usize,
        index: usize,
        scale: kalem_viewer::AxisScale,
    ) -> Result<()> {
        if let (Some(a), Some(b)) = (scale.min, scale.max)
            && a >= b
        {
            return Err(Error::Refused(
                "The minimum must be below the maximum".into(),
            ));
        }
        if scale.major.is_some_and(|m| m <= 0.0) {
            return Err(Error::Refused("The major unit must be above zero".into()));
        }
        if scale.log && (scale.min.is_some_and(|m| m <= 0.0) || scale.max.is_some_and(|m| m <= 0.0))
        {
            return Err(Error::Refused(
                "A logarithmic scale shows values above zero".into(),
            ));
        }
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let scatter =
            chart::parse_chart(&old, &self.theme).kind == kalem_viewer::ChartKind::Scatter;
        let new = chart::with_scale(
            &old,
            (scale.min, scale.max, scale.major, scale.log),
            scatter,
        )
        .ok_or_else(|| Error::Refused("This chart has no value axis".into()))?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Changes the kind of a sheet's chart (by its place among
    /// [`Workbook::charts`]), as Excel's Change Chart Type: the part
    /// written again for the new kind from the cells its series name, with
    /// their colors, the titles (an axis's going with its role, so a
    /// column chart's category title stays the categories' as a bar
    /// chart's), the legend, the data labels and the value axis's scale.
    /// One undo step.
    pub fn set_chart_kind(
        &mut self,
        idx: usize,
        index: usize,
        kind: kalem_viewer::ChartKind,
    ) -> Result<()> {
        use kalem_viewer::ChartKind as K;
        if kind == K::Other {
            return Err(Error::Refused(
                "Kalem does not write that kind of chart".into(),
            ));
        }
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let def = chart::parse_chart(&old, &self.theme);
        if def.kind == kind {
            return Ok(());
        }
        let mut series = Vec::new();
        for s in &def.series {
            let Some(val_f) = s.val.0.clone() else {
                return Err(Error::Refused(
                    "The chart's values are not in cells: Kalem cannot redraw it".into(),
                ));
            };
            let values = match self.chart_range(&val_f) {
                Some((i, r)) => self.range_numbers(i, r),
                None => s.val.1.clone(),
            };
            let name = s.name.0.clone().map(|f| {
                let text = match self.chart_range(&f) {
                    Some((i, r)) => self
                        .range_texts(
                            i,
                            Range {
                                start: r.start,
                                end: r.start,
                            },
                        )
                        .join(""),
                    None => s.name.1.clone(),
                };
                (f, text)
            });
            let cat = s.cat.0.clone().map(|f| {
                let texts = match self.chart_range(&f) {
                    Some((i, r)) => self.range_texts(i, r),
                    None => s.cat.1.clone(),
                };
                (f, texts, kind == K::Scatter)
            });
            series.push(chart::NewSeries {
                name,
                cat,
                val: (val_f, values),
                color: s.color,
            });
        }
        if series.is_empty() {
            return Err(Error::Refused("The chart has no series".into()));
        }
        let pie = |k: K| matches!(k, K::Pie | K::Doughnut);
        let mut new = chart::chart_xml(kind, def.title.as_deref(), &series);
        // Points with colors of their own keep them; a pie's slices pulled
        // out stay so in a doughnut, and the other way round.
        for (si, s) in def.series.iter().enumerate() {
            for (pt, c) in &s.points {
                if let Some(x) = chart::with_point_color(&new, si, *pt, Some(*c)) {
                    new = x;
                }
            }
            if matches!(kind, K::Pie | K::Doughnut) {
                if s.explosion > 0
                    && let Some(x) = chart::with_explosion(&new, si, None, s.explosion)
                {
                    new = x;
                }
                for (pt, e) in &s.point_explosions {
                    if let Some(x) = chart::with_explosion(&new, si, Some(*pt), *e) {
                        new = x;
                    }
                }
            }
        }
        if def.title.is_some() && !def.title_font.is_default() {
            new = chart::titled(&new, def.title.as_deref(), &def.title_font);
        }
        if def.title.is_none() && def.title_deleted {
            new = chart::titled(&new, None, &chart::Font::default());
        }
        // The legend as it was; a pie made from a chart without one gets
        // the legend its slices need.
        let legend = match (&def.legend, pie(kind) && !pie(def.kind)) {
            (None, true) => Some("r".to_owned()),
            (l, _) => l.clone(),
        };
        new = chart::with_legend(&new, legend.as_deref());
        if !def.legend_font.is_default()
            && let Some(x) = chart::with_legend_font(&new, &def.legend_font)
        {
            new = x;
        }
        new = chart::with_labels(&new, def.labels, pie(kind));
        // An axis's title keeps its role: categories (or x) and values.
        let (cat_title, val_title) = if def.kind == K::Bar {
            (def.vertical_title.clone(), def.horizontal_title.clone())
        } else {
            (def.horizontal_title.clone(), def.vertical_title.clone())
        };
        let (horizontal, vertical) = if kind == K::Bar {
            (val_title, cat_title)
        } else {
            (cat_title, val_title)
        };
        if let Some(t) = horizontal
            && let Some(x) = chart::axis_titled(&new, false, Some(&t))
        {
            new = x;
        }
        if let Some(t) = vertical
            && let Some(x) = chart::axis_titled(&new, true, Some(&t))
        {
            new = x;
        }
        if def.scale != (None, None, None, false)
            && let Some(x) = chart::with_scale(&new, def.scale, kind == K::Scatter)
        {
            new = x;
        }
        if let Some(f) = &def.axis_format
            && let Some(x) = chart::with_axis_format(&new, Some(f), kind == K::Scatter)
        {
            new = x;
        }
        // Axis fonts go with their axis's role, as the titles do.
        let (cat_font, val_font) = if def.kind == K::Bar {
            (&def.vertical_font, &def.horizontal_font)
        } else {
            (&def.horizontal_font, &def.vertical_font)
        };
        let (h_font, v_font) = if kind == K::Bar {
            (val_font, cat_font)
        } else {
            (cat_font, val_font)
        };
        for (vertical, f) in [(false, h_font), (true, v_font)] {
            if !f.is_default()
                && let Some(x) = chart::with_axis_font(&new, vertical, f)
            {
                new = x;
            }
        }
        if (def.background, def.border) != (chart::Fill::Auto, chart::Fill::Auto) {
            new = chart::with_chart_area(&new, def.background, def.border);
        }
        if (def.plot_background, def.plot_border) != (chart::Fill::Auto, chart::Fill::Auto) {
            new = chart::with_plot_area(&new, def.plot_background, def.plot_border);
        }
        // Gridlines go with their axis's role: a column chart's value lines
        // (horizontal) are a bar chart's vertical ones.
        let g = def.gridlines;
        let (cat_lines, val_lines) = if def.kind == K::Bar {
            ((g.0, g.1), (g.2, g.3))
        } else {
            ((g.2, g.3), (g.0, g.1))
        };
        let lines = if kind == K::Bar {
            (cat_lines.0, cat_lines.1, val_lines.0, val_lines.1)
        } else {
            (val_lines.0, val_lines.1, cat_lines.0, cat_lines.1)
        };
        if !matches!(def.kind, K::Pie | K::Doughnut)
            && let Some(x) = chart::with_gridlines(&new, lines)
        {
            new = x;
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Gives a series of a sheet's chart (by its place among
    /// [`Workbook::charts`]) a color, or the theme's again. One undo step.
    pub fn set_series_color(
        &mut self,
        idx: usize,
        index: usize,
        series: usize,
        color: Option<[u8; 3]>,
    ) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let kind = chart::parse_chart(&old, &self.theme).kind;
        if matches!(
            kind,
            kalem_viewer::ChartKind::Pie | kalem_viewer::ChartKind::Doughnut
        ) {
            return Err(Error::Refused(
                "A pie's slices each take a color of their own, not the series'".into(),
            ));
        }
        let rgb = color.map(|[r, g, b]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
        let new = chart::with_series_color(&old, series, rgb, kind)
            .ok_or_else(|| Error::Refused("No such series".into()))?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Gives a point (a pie's slice) of a sheet's chart (by its place
    /// among [`Workbook::charts`]) a color of its own, or its series'
    /// again. One undo step.
    pub fn set_point_color(
        &mut self,
        idx: usize,
        index: usize,
        series: usize,
        point: usize,
        color: Option<[u8; 3]>,
    ) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let points = self
            .charts(idx)?
            .get(index)
            .and_then(|c| c.series.get(series))
            .map_or(0, |s| s.values.len());
        if point >= points {
            return Err(Error::Refused("No such point".into()));
        }
        let rgb = color.map(|[r, g, b]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
        let new = chart::with_point_color(&old, series, point, rgb)
            .ok_or_else(|| Error::Refused("No such series".into()))?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Pulls a pie's slice (or every slice) of a sheet's chart (by its
    /// place among [`Workbook::charts`]) out by `percent` of the radius.
    /// One undo step.
    pub fn set_explosion(
        &mut self,
        idx: usize,
        index: usize,
        series: usize,
        point: Option<usize>,
        percent: u32,
    ) -> Result<()> {
        if percent > 400 {
            return Err(Error::Refused(
                "A slice stands out 400 percent at most".into(),
            ));
        }
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        if !matches!(
            chart::parse_chart(&old, &self.theme).kind,
            kalem_viewer::ChartKind::Pie | kalem_viewer::ChartKind::Doughnut
        ) {
            return Err(Error::Refused(
                "Only a pie's or a doughnut's slices stand out".into(),
            ));
        }
        let points = self
            .charts(idx)?
            .get(index)
            .and_then(|c| c.series.get(series))
            .map_or(0, |s| s.values.len());
        if point.is_some_and(|p| p >= points) {
            return Err(Error::Refused("No such slice".into()));
        }
        let new = chart::with_explosion(&old, series, point, percent)
            .ok_or_else(|| Error::Refused("No such series".into()))?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Paints the chart area (background and border) of a sheet's chart
    /// (by its place among [`Workbook::charts`]). One undo step.
    pub fn set_chart_area(
        &mut self,
        idx: usize,
        index: usize,
        background: kalem_viewer::Paint,
        border: kalem_viewer::Paint,
    ) -> Result<()> {
        let fill = |p: kalem_viewer::Paint| match p {
            kalem_viewer::Paint::Automatic => chart::Fill::Auto,
            kalem_viewer::Paint::None => chart::Fill::None,
            kalem_viewer::Paint::Color([r, g, b]) => {
                chart::Fill::Color((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
            }
        };
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let new = chart::with_chart_area(&old, fill(background), fill(border));
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Paints the plot area (background and border, inside the axes) of a
    /// sheet's chart
    /// (by its place among [`Workbook::charts`]). One undo step.
    pub fn set_plot_area(
        &mut self,
        idx: usize,
        index: usize,
        background: kalem_viewer::Paint,
        border: kalem_viewer::Paint,
    ) -> Result<()> {
        let fill = |p: kalem_viewer::Paint| match p {
            kalem_viewer::Paint::Automatic => chart::Fill::Auto,
            kalem_viewer::Paint::None => chart::Fill::None,
            kalem_viewer::Paint::Color([r, g, b]) => {
                chart::Fill::Color((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
            }
        };
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let new = chart::with_plot_area(&old, fill(background), fill(border));
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Shows or hides the gridlines of a sheet's chart (by its place among
    /// [`Workbook::charts`]). One undo step.
    pub fn set_gridlines(
        &mut self,
        idx: usize,
        index: usize,
        lines: kalem_viewer::Gridlines,
    ) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let new = chart::with_gridlines(
            &old,
            (
                lines.horizontal_major,
                lines.horizontal_minor,
                lines.vertical_major,
                lines.vertical_minor,
            ),
        )
        .ok_or_else(|| Error::Refused("This chart has no axes".into()))?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets the number format of the value axis's labels of a sheet's
    /// chart (by its place among [`Workbook::charts`]), or the cells' own.
    /// One undo step.
    pub fn set_axis_format(
        &mut self,
        idx: usize,
        index: usize,
        format: Option<&str>,
    ) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let scatter =
            chart::parse_chart(&old, &self.theme).kind == kalem_viewer::ChartKind::Scatter;
        let format = format
            .map(str::trim)
            .filter(|f| !f.is_empty() && *f != "General");
        let new = chart::with_axis_format(&old, format, scatter)
            .ok_or_else(|| Error::Refused("This chart has no value axis".into()))?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets the font of an axis's labels of a sheet's chart (by its place
    /// among [`Workbook::charts`]). One undo step.
    pub fn set_axis_font(
        &mut self,
        idx: usize,
        index: usize,
        vertical: bool,
        font: &kalem_viewer::AxisFont,
    ) -> Result<()> {
        if font.size.is_some_and(|s| !(1.0..=400.0).contains(&s)) {
            return Err(Error::Refused("A font is 1 to 400 points".into()));
        }
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let f = chart::Font {
            size: font.size,
            bold: font.bold,
            italic: font.italic,
            color: font
                .color
                .map(|[r, g, b]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)),
            face: font.face.clone().filter(|f| !f.trim().is_empty()),
        };
        let new = chart::with_axis_font(&old, vertical, &f)
            .ok_or_else(|| Error::Refused("This chart has no such axis".into()))?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets the font of a sheet's chart's title (by its place among
    /// [`Workbook::charts`]). The chart needs a title of its own. One undo
    /// step.
    pub fn set_title_font(
        &mut self,
        idx: usize,
        index: usize,
        font: &kalem_viewer::AxisFont,
    ) -> Result<()> {
        if font.size.is_some_and(|s| !(1.0..=400.0).contains(&s)) {
            return Err(Error::Refused("A font is 1 to 400 points".into()));
        }
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        let def = chart::parse_chart(&old, &self.theme);
        let Some(title) = def.title else {
            return Err(Error::Refused("Give the chart a title first (h t)".into()));
        };
        let f = chart::Font {
            size: font.size,
            bold: font.bold,
            italic: font.italic,
            color: font
                .color
                .map(|[r, g, b]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)),
            face: font.face.clone().filter(|f| !f.trim().is_empty()),
        };
        let new = chart::titled(&old, Some(&title), &f);
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Sets the font of a sheet's chart's legend (by its place among
    /// [`Workbook::charts`]). The chart needs a legend. One undo step.
    pub fn set_legend_font(
        &mut self,
        idx: usize,
        index: usize,
        font: &kalem_viewer::AxisFont,
    ) -> Result<()> {
        if font.size.is_some_and(|s| !(1.0..=400.0).contains(&s)) {
            return Err(Error::Refused("A font is 1 to 400 points".into()));
        }
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let part = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        let old = text_of(self.pkg.part(&part)?, &part)?;

        let f = chart::Font {
            size: font.size,
            bold: font.bold,
            italic: font.italic,
            color: font
                .color
                .map(|[r, g, b]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)),
            face: font.face.clone().filter(|f| !f.trim().is_empty()),
        };
        let new = chart::with_legend_font(&old, &f).ok_or_else(|| {
            Error::Refused("The chart has no legend: place one first (h l)".into())
        })?;
        if new == old {
            return Ok(());
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(&part, new.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Fills `target` from `source` (which it holds, going past it one
    /// way), as Excel's fill handle: with `series`, numbers, dates,
    /// numbered text and month or day names go on; else the source is
    /// copied over again (Fill Down, Fill Right). Formulas move their
    /// relative references, and filled cells take the source's style. One
    /// undo step; a cell Excel would refuse (in a merged cell, part of an
    /// array) refuses the whole fill.
    pub fn fill(
        &mut self,
        idx: usize,
        source: Range,
        target: Range,
        series: bool,
        lists: &[Vec<String>],
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let contains = target.start.row <= source.start.row
            && target.start.col <= source.start.col
            && target.end.row >= source.end.row
            && target.end.col >= source.end.col;
        let same_cols = (target.start.col, target.end.col) == (source.start.col, source.end.col);
        let same_rows = (target.start.row, target.end.row) == (source.start.row, source.end.row);
        if !contains || (!same_cols && !same_rows) || target == source {
            return Err(Error::Refused(
                "A fill goes one way from its cells: down, up, right or left".into(),
            ));
        }
        if target.end.row >= MAX_ROW || target.end.col >= MAX_COL {
            return Err(Error::Refused(
                "The fill goes past the end of the sheet".into(),
            ));
        }
        let down = same_cols;
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = self.fill_inner(idx, source, target, series, down, lists);
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    fn fill_inner(
        &mut self,
        idx: usize,
        source: Range,
        target: Range,
        series: bool,
        down: bool,
        lists: &[Vec<String>],
    ) -> Result<()> {
        // Each column of a fill down (each row of a fill right) on its own.
        let (lines, s0, s1, t0, t1) = if down {
            (
                source.start.col..=source.end.col,
                source.start.row,
                source.end.row,
                target.start.row,
                target.end.row,
            )
        } else {
            (
                source.start.row..=source.end.row,
                source.start.col,
                source.end.col,
                target.start.col,
                target.end.col,
            )
        };
        let at = |line: u32, k: u32| {
            if down {
                CellRef::new(k, line)
            } else {
                CellRef::new(line, k)
            }
        };
        for line in lines {
            let mut items = Vec::new();
            let mut cells = Vec::new();
            for k in s0..=s1 {
                let c = at(line, k);
                let value = self.value(idx, c)?;
                let cell = self.loaded[&idx].1.cells.get(&c).cloned();
                let style = cell.as_ref().map_or(0, |c| c.style);
                items.push(crate::fill::Item {
                    value,
                    formula: cell.as_ref().is_some_and(|c| c.formula.is_some()),
                    date: self.styles.is_date(style),
                });
                cells.push((cell, style));
            }
            let pattern = crate::fill::analyze_with(&items, series, lists);
            for k in t0..=t1 {
                if (s0..=s1).contains(&k) {
                    continue;
                }
                let p = i64::from(k) - i64::from(s0);
                let n = items.len() as i64;
                let from = p.rem_euclid(n) as usize;
                let dest = at(line, k);
                let input = match pattern.at(p) {
                    crate::fill::Out::Number(v) => Input::Number(v, None),
                    crate::fill::Out::Text(t) => Input::Text(t),
                    crate::fill::Out::Copy(i) => {
                        let offset = i64::from(k) - i64::from(s0) - i as i64;
                        match (&cells[i].0, &items[i].value) {
                            (Some(c), _) if c.formula.is_some() => {
                                let f = &c.formula.as_ref().expect("checked").text;
                                let (dr, dc) = if down { (offset, 0) } else { (0, offset) };
                                Input::Formula(formula::shift(f, dr, dc))
                            }
                            (_, Value::Number(v)) => Input::Number(*v, None),
                            (_, Value::Text(t)) => Input::Text(t.clone()),
                            (_, Value::Bool(b)) => Input::Bool(*b),
                            (_, Value::Error(e)) => Input::Error(e.clone()),
                            _ => Input::Clear,
                        }
                    }
                };
                self.set_input(idx, dest, input)?;
                // The source's format, the number format with it.
                let style = cells[from].1;
                let now = self.loaded[&idx].1.cells.get(&dest).map_or(0, |c| c.style);
                if now != style {
                    self.apply_style(idx, dest, style)?;
                }
            }
        }
        Ok(())
    }

    /// Removes a sheet's chart (by its place among [`Workbook::charts`]):
    /// its anchor, its part and what hangs on it. One undo step.
    pub fn delete_chart(&mut self, idx: usize, index: usize) -> Result<()> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let anchors: Vec<chart::Anchor> = chart::parse_drawing(&text)
            .into_iter()
            .filter(|a| self.rel_target(&drawing, &a.rid).is_some())
            .collect();
        let a = anchors
            .get(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?
            .clone();
        let chart_part = self.rel_target(&drawing, &a.rid).expect("checked above");
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        self.pkg.set_part(
            &drawing,
            format!("{}{}", &text[..a.span.start], &text[a.span.end..]).into_bytes(),
        );
        let drawing_rels = rels::rels_path(&drawing);
        let rels_text = text_of(self.pkg.part(&drawing_rels)?, &drawing_rels)?;
        self.pkg
            .set_part(&drawing_rels, rels::remove(&rels_text, &a.rid).into_bytes());
        // The chart and the parts only it uses (its style and colors).
        let mut gone = vec![chart_part.clone()];
        let chart_rels = rels::rels_path(&chart_part);
        if let Ok(b) = self.pkg.part(&chart_rels) {
            for r in rels::parse(&String::from_utf8_lossy(&b)) {
                if !r.external && matches!(r.kind.as_str(), "chartStyle" | "chartColorStyle") {
                    gone.push(rels::resolve(&chart_part, &r.target));
                }
            }
            self.pkg.remove_part(&chart_rels);
        }
        let ct = "[Content_Types].xml";
        let mut types = text_of(self.pkg.part(ct)?, ct)?;
        for part in gone {
            self.pkg.remove_part(&part);
            types = rels::remove_override(&types, &part);
        }
        self.pkg.set_part(ct, types.into_bytes());
        self.generation += 1;
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// The relationship type URL of `kind`, in the namespace the
    /// workbook's own relationships use (transitional or strict).
    fn rel_type(&self, kind: &str) -> String {
        let rels_path = rels::rels_path(&self.workbook_part);
        let text = self
            .pkg
            .part(&rels_path)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let base = text
            .split("Type=\"")
            .skip(1)
            .filter_map(|t| t.split('"').next())
            .find_map(|t| t.strip_suffix("/worksheet"))
            .unwrap_or("http://schemas.openxmlformats.org/officeDocument/2006/relationships");
        format!("{base}/{kind}")
    }

    /// Adds a relationship from `source` to `target` (a part name); its id.
    fn add_rel(&mut self, source: &str, kind: &str, target: &str) -> Result<String> {
        let rels_path = rels::rels_path(source);
        let text = if self.pkg.contains(&rels_path) {
            text_of(self.pkg.part(&rels_path)?, &rels_path)?
        } else {
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_string()
        };
        let used: Vec<String> = rels::parse(&text).into_iter().map(|r| r.id).collect();
        let id = (1..)
            .map(|n| format!("rId{n}"))
            .find(|i| !used.contains(i))
            .expect("a free id");
        // The target relative to the source's folder.
        let dir = source.rsplit_once('/').map_or("", |(d, _)| d);
        let from: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        let to: Vec<&str> = target.split('/').collect();
        let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
        let mut rel: Vec<&str> = vec![".."; from.len() - common];
        rel.extend(&to[common..]);
        let item = format!(
            "<Relationship Id=\"{id}\" Type=\"{}\" Target=\"{}\"/>",
            self.rel_type(kind),
            rel.join("/")
        );
        let close = text.rfind("</").unwrap_or(text.len());
        let new = format!("{}{item}{}", &text[..close], &text[close..]);
        self.pkg.set_part(&rels_path, new.into_bytes());
        if source == self.workbook_part {
            self.workbook_rels.push(Rel {
                id: id.clone(),
                kind: kind.to_owned(),
                target: rel.join("/"),
                external: false,
            });
        }
        Ok(id)
    }

    /// Puts a new part into the package with its content type.
    fn add_part(&mut self, name: &str, text: String, content_type: &str) -> Result<()> {
        self.pkg.set_part(name, text.into_bytes());
        let ct = "[Content_Types].xml";
        let types = text_of(self.pkg.part(ct)?, ct)?;
        let item = format!("<Override PartName=\"/{name}\" ContentType=\"{content_type}\"/>");
        let close = types.rfind("</").unwrap_or(types.len());
        self.pkg.set_part(
            ct,
            format!("{}{item}{}", &types[..close], &types[close..]).into_bytes(),
        );
        Ok(())
    }

    /// A part name not in the package: `{stem}{n}.xml`.
    fn free_part(&self, stem: &str) -> String {
        (1..)
            .map(|n| format!("{stem}{n}.xml"))
            .find(|p| !self.pkg.contains(p))
            .expect("a free name")
    }

    /// The prefix the workbook part gives the relationships namespace, or
    /// the declaration to put on an element that needs it.
    fn r_prefix(&self) -> (String, String) {
        const NS: &str = "officeDocument/2006/relationships";
        let head_end = self
            .workbook_xml
            .find("<sheets")
            .unwrap_or(self.workbook_xml.len());
        let head = &self.workbook_xml[..head_end];
        for part in head.split("xmlns:").skip(1) {
            if let Some((p, rest)) = part.split_once("=\"")
                && rest.split('"').next().is_some_and(|u| u.ends_with(NS))
            {
                return (p.to_owned(), String::new());
            }
        }
        (
            "r".into(),
            " xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\""
                .into(),
        )
    }

    /// Adds an empty worksheet after the others, as Excel's New Sheet; its
    /// index. Its name is `stem` and the first free number.
    pub fn add_sheet(&mut self, stem: &str) -> Result<usize> {
        let name = (1..)
            .map(|n| format!("{stem}{n}"))
            .find(|n| self.sheet_index(n).is_none())
            .expect("a free name");
        let part = self.free_part("xl/worksheets/sheet");
        let main = self
            .workbook_xml
            .split("xmlns=\"")
            .nth(1)
            .and_then(|t| t.split('"').next())
            .unwrap_or("http://schemas.openxmlformats.org/spreadsheetml/2006/main")
            .to_owned();
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"{main}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><dimension ref=\"A1\"/><sheetViews><sheetView workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/><sheetData/><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>"
        );
        self.add_part(
            &part,
            text,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
        )?;
        let wb_part = self.workbook_part.clone();
        let rid = self.add_rel(&wb_part, kind::WORKSHEET, &part)?;
        // Its `<sheet>`, with the next sheetId.
        let mut r = Reader::new(&self.workbook_xml);
        let (mut max_id, mut close, mut prefix) = (0u32, None, String::new());
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) if tag.name == "sheet" => {
                    prefix = xml::prefix(tag.qname).to_owned();
                    max_id = max_id.max(
                        tag.attr("sheetId")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0),
                    );
                }
                Token::End {
                    name: "sheets",
                    span,
                } => {
                    close = Some(span.start);
                    break;
                }
                _ => {}
            }
        }
        let close =
            close.ok_or_else(|| Error::Refused("the workbook part has no <sheets>".into()))?;
        let (rp, decl) = self.r_prefix();
        let item = format!(
            "<{prefix}sheet name=\"{}\" sheetId=\"{}\"{decl} {rp}:id=\"{rid}\"/>",
            xml::escape(&name),
            max_id + 1
        );
        self.workbook_xml = splice(&self.workbook_xml, vec![(close..close, item)]);
        self.sheets.push(SheetInfo {
            name,
            kind: SheetKind::Worksheet,
            visibility: Visibility::Visible,
            part,
        });
        // The engine is built again with the new sheet.
        self.engine = None;
        self.generation += 1;
        Ok(self.sheets.len() - 1)
    }

    /// A range's records for a pivot table: its first row names the fields.
    fn pivot_source(&mut self, idx: usize, range: Range) -> Result<pivot::Source> {
        if range.start.row >= range.end.row {
            return Err(Error::Refused(
                "A pivot table needs a header row and rows under it".into(),
            ));
        }
        let mut src = pivot::Source::default();
        for col in range.start.col..=range.end.col {
            let name = self.display(idx, CellRef::new(range.start.row, col))?;
            if name.trim().is_empty() {
                return Err(Error::Refused(format!(
                    "The field in column {} has no name: a pivot table needs a name for each",
                    crate::cellref::column_name(col)
                )));
            }
            src.names.push(name);
        }
        for row in range.start.row + 1..=range.end.row {
            let mut values = Vec::new();
            let mut shown = Vec::new();
            for col in range.start.col..=range.end.col {
                let at = CellRef::new(row, col);
                values.push(self.value(idx, at)?);
                shown.push(self.display(idx, at)?);
            }
            src.rows.push(values);
            src.shown.push(shown);
        }
        Ok(src)
    }

    /// Writes a computed pivot table's cells from `at` on.
    fn write_pivot_cells(&mut self, idx: usize, p: &pivot::Pivot, at: CellRef) -> Result<()> {
        for (i, line) in p.cells.iter().enumerate() {
            for (j, cell) in line.iter().enumerate() {
                let input = match cell {
                    Some(pivot::Out::Text(t)) => Input::Text(t.clone()),
                    Some(pivot::Out::Number(n)) => Input::Number(*n, None),
                    Some(pivot::Out::Error(e)) => Input::Error(e.clone()),
                    None => continue,
                };
                self.set_input(
                    idx,
                    CellRef::new(at.row + i as u32, at.col + j as u32),
                    input,
                )?;
            }
        }
        Ok(())
    }

    /// Inserts a pivot table of `range` on a new sheet, at A3 as Excel puts
    /// it: the cache, its records and the table written as Excel writes
    /// them. One undo step; the new sheet's index.
    pub fn insert_pivot(
        &mut self,
        idx: usize,
        range: Range,
        layout: &pivot::Layout,
    ) -> Result<usize> {
        self.load(idx)?;
        let src = self.pivot_source(idx, range)?;
        let p = pivot::compute(&src, layout).map_err(Error::Refused)?;
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = self.insert_pivot_inner(idx, range, &src, &p);
        if own {
            match &result {
                Ok(_) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    fn insert_pivot_inner(
        &mut self,
        idx: usize,
        range: Range,
        src: &pivot::Source,
        p: &pivot::Pivot,
    ) -> Result<usize> {
        let sheet_name = self.sheets[idx].name.clone();
        let new = self.add_sheet("Pivot")?;
        self.batch_changed = true;
        let cache_id = self.next_cache_id();
        let at = CellRef::new(2, 0);
        let def_part = self.free_part("xl/pivotCache/pivotCacheDefinition");
        let rec_part = self.free_part("xl/pivotCache/pivotCacheRecords");
        let table_part = self.free_part("xl/pivotTables/pivotTable");
        let rec_rid = "rId1";
        self.add_part(
            &def_part,
            pivot::cache_definition(p, src, &sheet_name, range, rec_rid),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml",
        )?;
        self.add_rel(&def_part, "pivotCacheRecords", &rec_part)?;
        self.add_part(
            &rec_part,
            pivot::cache_records(p, src),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml",
        )?;
        let name = (1..)
            .map(|n| format!("PivotTable{n}"))
            .find(|n| !self.pivot_names().contains(n))
            .expect("a free name");
        self.add_part(
            &table_part,
            pivot::table_definition(p, src, &name, cache_id, at, "PivotStyleLight16"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml",
        )?;
        self.add_rel(&table_part, "pivotCacheDefinition", &def_part)?;
        let sheet_part = self.sheets[new].part.clone();
        self.add_rel(&sheet_part, "pivotTable", &table_part)?;
        // The cache in the workbook part.
        let wb_part = self.workbook_part.clone();
        let rid = self.add_rel(&wb_part, "pivotCacheDefinition", &def_part)?;
        let (rp, decl) = self.r_prefix();
        let mut r = Reader::new(&self.workbook_xml);
        let (mut depth, mut prefix, mut list_end, mut before, mut end) =
            (0, String::new(), None, None, None);
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) => {
                    if depth == 0 {
                        prefix = xml::prefix(tag.qname).to_owned();
                    }
                    if depth == 1 && tag.name == "pivotCaches" && !tag.empty {
                        let e = r.skip_element();
                        list_end = Some(self.workbook_xml[..e].rfind('<').unwrap_or(e));
                        continue;
                    }
                    if depth == 1
                        && before.is_none()
                        && matches!(
                            tag.name,
                            "smartTagPr"
                                | "smartTagTypes"
                                | "webPublishing"
                                | "fileRecoveryPr"
                                | "webPublishObjects"
                                | "extLst"
                        )
                    {
                        before = Some(tag.span.start);
                    }
                    if !tag.empty {
                        depth += 1;
                    }
                }
                Token::End { span, .. } => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(span.start);
                    }
                }
                Token::Text { .. } => {}
            }
        }
        let item = format!("<{prefix}pivotCache cacheId=\"{cache_id}\"{decl} {rp}:id=\"{rid}\"/>");
        let edit = match list_end {
            Some(e) => (e..e, item),
            None => {
                let at = before.or(end).unwrap_or(self.workbook_xml.len());
                (
                    at..at,
                    format!("<{prefix}pivotCaches>{item}</{prefix}pivotCaches>"),
                )
            }
        };
        self.workbook_xml = splice(&self.workbook_xml, vec![edit]);
        self.write_pivot_cells(new, p, at)?;
        // Columns as wide as their labels, as Excel fits a new table.
        let width = p.cells.iter().map(Vec::len).max().unwrap_or(0);
        for j in 0..width {
            let chars = p
                .cells
                .iter()
                .filter_map(|line| line.get(j).cloned().flatten())
                .map(|c| match c {
                    pivot::Out::Text(t) | pivot::Out::Error(t) => t.chars().count(),
                    pivot::Out::Number(n) => format!("{n}").len(),
                })
                .max()
                .unwrap_or(0);
            let w = (chars as f64 + 2.0).clamp(8.43, 60.0);
            self.set_col_width(new, at.col + j as u32, w)?;
        }
        Ok(new)
    }

    fn next_cache_id(&self) -> u32 {
        let mut r = Reader::new(&self.workbook_xml);
        let mut max = 0;
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "pivotCache"
            {
                max = max.max(
                    tag.attr("cacheId")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                );
            }
        }
        max + 1
    }

    fn pivot_names(&self) -> Vec<String> {
        self.pkg
            .names()
            .into_iter()
            .filter(|n| n.contains("pivotTables/") && n.ends_with(".xml"))
            .filter_map(|n| self.pkg.part(&n).ok())
            .filter_map(|b| String::from_utf8(b).ok())
            .map(|t| pivot::parse_table(&t).name)
            .collect()
    }

    /// Each worksheet's pivot tables: the sheet and the table's part.
    pub fn pivot_tables(&self) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        for (i, s) in self.sheets.iter().enumerate() {
            let rels_path = rels::rels_path(&s.part);
            let Ok(text) = self.pkg.part(&rels_path) else {
                continue;
            };
            for r in rels::parse(&String::from_utf8_lossy(&text)) {
                if r.kind == "pivotTable" && !r.external {
                    out.push((i, rels::resolve(&s.part, &r.target)));
                }
            }
        }
        out
    }

    /// Computes every pivot table again from its source, as Excel's
    /// Refresh All: its cache, records, table and cells rewritten, in
    /// compact form. One undo step; the sheets that changed. A table Kalem
    /// cannot compute (report filters, grouped or calculated fields, a
    /// source outside the workbook) refuses the whole refresh.
    pub fn refresh_pivots(&mut self) -> Result<Vec<usize>> {
        let tables = self.pivot_tables();
        if tables.is_empty() {
            return Err(Error::Refused("This workbook has no pivot tables".into()));
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let mut changed = Vec::new();
        let mut result = Ok(());
        for (idx, part) in tables {
            result = self.refresh_pivot(idx, &part);
            if result.is_err() {
                break;
            }
            changed.push(idx);
        }
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result.map(|()| changed)
    }

    fn refresh_pivot(&mut self, idx: usize, table_part: &str) -> Result<()> {
        let text = text_of(self.pkg.part(table_part)?, table_part)?;
        let t = pivot::parse_table(&text);
        let refuse = |why: &str| Error::Refused(format!("{}: {why}", t.name));
        if t.pages > 0 {
            return Err(refuse(
                "Kalem does not refresh a pivot table with report filters yet",
            ));
        }
        let table_rels = rels::rels_path(table_part);
        let def_part = rels::parse(&text_of(self.pkg.part(&table_rels)?, &table_rels)?)
            .into_iter()
            .find(|r| r.kind == "pivotCacheDefinition")
            .map(|r| rels::resolve(table_part, &r.target))
            .ok_or_else(|| refuse("its cache is missing"))?;
        let def = text_of(self.pkg.part(&def_part)?, &def_part)?;
        let (sheet, range, names, rec_rid) = pivot::parse_cache(&def)
            .ok_or_else(|| refuse("its source or its fields are not ones Kalem computes"))?;
        let src_idx = self
            .sheet_index(&sheet)
            .ok_or_else(|| refuse("its source sheet is gone"))?;
        let src = self.pivot_source(src_idx, range)?;
        // The table's fields by name, in the source as it is now.
        let by_name = |f: usize| -> Result<usize> {
            let name = names.get(f).ok_or_else(|| refuse("a field is missing"))?;
            src.names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(name))
                .ok_or_else(|| refuse(&format!("the field {name} is no longer in the source")))
        };
        let layout = pivot::Layout {
            rows: t.rows.iter().map(|f| by_name(*f)).collect::<Result<_>>()?,
            cols: t.cols.iter().map(|f| by_name(*f)).collect::<Result<_>>()?,
            values: t
                .values
                .iter()
                .map(|(f, a)| by_name(*f).map(|x| (x, *a)))
                .collect::<Result<_>>()?,
        };
        let p = pivot::compute(&src, &layout).map_err(|e| refuse(&e))?;
        let at = t.location.map_or(CellRef::new(2, 0), |l| l.start);
        let style = if t.style.is_empty() {
            "PivotStyleLight16"
        } else {
            t.style.as_str()
        };
        // The records part: the one the cache names, or a new one.
        let def_rels = rels::rels_path(&def_part);
        let rec_part = rec_rid.as_ref().and_then(|rid| {
            let rels_text = self.pkg.part(&def_rels).ok()?;
            rels::parse(&String::from_utf8_lossy(&rels_text))
                .into_iter()
                .find(|r| &r.id == rid)
                .map(|r| rels::resolve(&def_part, &r.target))
        });
        let (rec_part, rec_rid) = match (rec_part, rec_rid) {
            (Some(p), Some(rid)) => (p, rid),
            _ => {
                let part = self.free_part("xl/pivotCache/pivotCacheRecords");
                self.add_part(
                    &part,
                    String::new(),
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml",
                )?;
                let rid = self.add_rel(&def_part, "pivotCacheRecords", &part)?;
                (part, rid)
            }
        };
        self.pkg.set_part(
            &def_part,
            pivot::cache_definition(&p, &src, &sheet, range, &rec_rid).into_bytes(),
        );
        self.pkg
            .set_part(&rec_part, pivot::cache_records(&p, &src).into_bytes());
        self.pkg.set_part(
            table_part,
            pivot::table_definition(&p, &src, &t.name, t.cache_id, at, style).into_bytes(),
        );
        if let Some(old) = t.location {
            self.clear_range(idx, old)?;
        }
        self.write_pivot_cells(idx, &p, at)?;
        self.batch_changed = true;
        Ok(())
    }

    /// Appends a `<dxf>` to `styles.xml`, making `<dxfs>` where the schema
    /// puts it; its index.
    fn add_dxf(&mut self, dxf: &str) -> Result<u32> {
        let Some(part) = self.styles_part.clone() else {
            return Err(Error::Refused("the workbook has no styles part".into()));
        };
        let text = match &self.styles_xml {
            Some(t) => t.clone(),
            None => text_of(self.pkg.part(&part)?, &part)?,
        };
        let mut r = Reader::new(&text);
        let mut depth = 0;
        let mut prefix = String::new();
        let mut before: Option<usize> = None;
        let mut end: Option<usize> = None;
        let mut out: Option<String> = None;
        let count = self.styles.dxfs.len() as u32;
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) => {
                    if depth == 0 {
                        prefix = xml::prefix(tag.qname).to_owned();
                    }
                    if depth == 1 && tag.name == "dxfs" {
                        let open = xml::set_attr(
                            &text[tag.span.clone()],
                            "count",
                            &(count + 1).to_string(),
                        );
                        let item = with_prefix(dxf, &prefix);
                        out = Some(if tag.empty {
                            let o = open.trim_end_matches("/>").trim_end();
                            splice(
                                &text,
                                vec![(tag.span.clone(), format!("{o}>{item}</{prefix}dxfs>"))],
                            )
                        } else {
                            let e = r.skip_element();
                            let close = text[..e].rfind('<').unwrap_or(e);
                            splice(&text, vec![(tag.span.clone(), open), (close..close, item)])
                        });
                        break;
                    }
                    if depth == 1
                        && before.is_none()
                        && matches!(tag.name, "tableStyles" | "colors" | "extLst")
                    {
                        before = Some(tag.span.start);
                    }
                    if !tag.empty {
                        depth += 1;
                    }
                }
                Token::End { span, .. } => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(span.start);
                    }
                }
                Token::Text { .. } => {}
            }
        }
        let out = match out {
            Some(o) => o,
            None => {
                let at = before
                    .or(end)
                    .ok_or_else(|| Error::Refused("styles.xml has no styleSheet".into()))?;
                let block = format!(
                    "<{prefix}dxfs count=\"1\">{}</{prefix}dxfs>",
                    with_prefix(dxf, &prefix)
                );
                splice(&text, vec![(at..at, block)])
            }
        };
        self.styles = styles::parse(&out, &self.theme);
        self.styles_xml = Some(out);
        Ok(count)
    }

    fn replace_sheet_text(&mut self, idx: usize, new: String) {
        let model = sheet::parse(&new, &self.strings, self.date1904);
        self.loaded.insert(idx, (new, model));
        self.generation += 1;
        if !self.dirty_sheets.contains(&idx) {
            self.dirty_sheets.push(idx);
        }
    }

    /// Enters texts into scattered cells, each as typed, in one undo step
    /// (Flash Fill's results); a cell Excel would refuse refuses them all.
    pub fn set_cell_list(&mut self, idx: usize, cells: &[(CellRef, String)]) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let mut result = Ok(());
        for (at, v) in cells {
            result = self.set_cell(idx, *at, v);
            if result.is_err() {
                break;
            }
        }
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Enters rows of texts from `at` on, each as typed, as Excel's Paste of
    /// text: one undo step; a cell Excel would refuse (inside a merged
    /// cell, part of an array) refuses the whole paste.
    pub fn set_cells(&mut self, idx: usize, at: CellRef, values: &[Vec<String>]) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let rows = values.len() as u32;
        let cols = values.iter().map(Vec::len).max().unwrap_or(0) as u32;
        if at.row + rows > MAX_ROW || at.col + cols > MAX_COL {
            return Err(Error::Refused(
                "the pasted cells go past the end of the sheet".into(),
            ));
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let mut result = Ok(());
        'rows: for (i, line) in values.iter().enumerate() {
            for (j, v) in line.iter().enumerate() {
                result = self.set_cell(idx, CellRef::new(at.row + i as u32, at.col + j as u32), v);
                if result.is_err() {
                    break 'rows;
                }
            }
        }
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Moves a range's cells to start at `to`, as Excel's Cut and Paste:
    /// values, formulas (their text as it was) and formats go, the cells
    /// left are empty, and every formula that referred to the moved cells,
    /// on any sheet and in defined names, points at their new place. One
    /// undo step. Merged cells and array formulas in the way are refused.
    pub fn move_range(&mut self, idx: usize, src: Range, to: CellRef) -> Result<()> {
        self.move_range_to(idx, src, idx, to)
    }

    /// Moves a range's cells from sheet `from` to start at `to` on sheet
    /// `into`, as Excel's Cut on one sheet and Paste on another: as
    /// [`Workbook::move_range`], the formulas that read the cells now
    /// naming `into`, and the moved formulas' own references qualified
    /// with `from`, the sheet they meant.
    pub fn move_range_to(
        &mut self,
        from: usize,
        src: Range,
        into: usize,
        to: CellRef,
    ) -> Result<()> {
        for i in [from, into] {
            self.load(i)?;
            if self.sheets[i].kind != SheetKind::Worksheet {
                return Err(Error::NotAWorksheet(self.sheets[i].name.clone()));
            }
        }
        let (rows, cols) = (src.end.row - src.start.row, src.end.col - src.start.col);
        if to.row + rows >= MAX_ROW || to.col + cols >= MAX_COL {
            return Err(Error::Refused(
                "the cells would go past the end of the sheet".into(),
            ));
        }
        let dest = Range {
            start: to,
            end: CellRef::new(to.row + rows, to.col + cols),
        };
        if from == into && dest == src {
            return Ok(());
        }
        let touches = |m: &Range, r: &Range| {
            m.start.row <= r.end.row
                && r.start.row <= m.end.row
                && m.start.col <= r.end.col
                && r.start.col <= m.end.col
        };
        for (i, area) in [(from, src), (into, dest)] {
            let model = &self.loaded[&i].1;
            if let Some(m) = model.merged.iter().find(|m| touches(m, &area)) {
                return Err(Error::Refused(format!(
                    "the merged cell {m} is in the way; unmerge it first"
                )));
            }
            for c in model.cells.values() {
                if let Some(f) = &c.formula
                    && let FormulaKind::Array { range: a } = f.kind
                    && touches(&a, &area)
                {
                    return Err(Error::Refused(format!(
                        "the array formula over {a} is in the way"
                    )));
                }
            }
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = self.move_inner(from, src, into, dest);
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                    return result;
                }
            }
        }
        result?;
        // Formulas that referred to the moved cells follow them.
        let (src_name, dest_name) = (
            self.sheets[from].name.clone(),
            self.sheets[into].name.clone(),
        );
        let (dr, dc) = (
            i64::from(dest.start.row) - i64::from(src.start.row),
            i64::from(dest.start.col) - i64::from(src.start.col),
        );
        let follow = |f: &str, sheet: Option<&str>| {
            if from == into {
                formula::move_refs(f, &src_name, sheet, src, dr, dc)
            } else {
                formula::move_refs_between(f, &src_name, &dest_name, sheet, src, dr, dc)
            }
        };
        let indices: Vec<usize> = self.loaded.keys().copied().collect();
        for i in indices {
            let own_name = self.sheets[i].name.clone();
            let text = &self.loaded[&i].0;
            let new = structure::map_formula_texts(text, |f| follow(f, Some(&own_name)));
            if new != *text {
                self.replace_sheet_text(i, new);
            }
        }
        let wb = structure::map_formula_texts(&self.workbook_xml, |f| follow(f, None));
        if wb != self.workbook_xml {
            self.workbook_xml = wb;
            self.reread_defined_names();
        }
        // The engine reads the new texts; the cells it was trusted for keep
        // its trust where they went, and get its results: a formula that
        // read the moved cells had been computed while they were away.
        let (sr, sc) = (src.start.row, src.start.col);
        let trusted: HashSet<(usize, CellRef)> = std::mem::take(&mut self.trusted)
            .into_iter()
            .map(|(s, p)| {
                if s == from && src.contains(p) {
                    (
                        into,
                        CellRef::new(dest.start.row + p.row - sr, dest.start.col + p.col - sc),
                    )
                } else {
                    (s, p)
                }
            })
            .collect();
        self.engine = None;
        self.computed.clear();
        self.ensure_engine()?;
        self.trusted.extend(trusted);
        let after = std::mem::take(&mut self.computed);
        let cells = self.formula_cells();
        let before: HashMap<(usize, CellRef), Value> = cells
            .iter()
            .map(|&(s, p)| ((s, p), self.loaded[&s].1.cells[&p].value.clone()))
            .collect();
        self.write_results(&before, after, &cells, &[]);
        Ok(())
    }

    fn move_inner(&mut self, from: usize, src: Range, into: usize, dest: Range) -> Result<()> {
        let from_name = self.sheets[from].name.clone();
        // Every source cell, as entered, with its style; a formula going to
        // another sheet keeps meaning the cells of the sheet it was on.
        let mut moved = Vec::new();
        for (p, c) in self.loaded[&from].1.cells.clone() {
            if src.contains(p) {
                let mut text = if c.value != Value::Empty || c.formula.is_some() {
                    self.edit_text(from, p)?
                } else {
                    String::new()
                };
                if from != into && c.formula.is_some() {
                    text = format!("={}", formula::qualify_refs(&text[1..], &from_name));
                }
                moved.push((p.row - src.start.row, p.col - src.start.col, text, c.style));
            }
        }
        // The source and the destination emptied, styles too.
        for (i, area) in [(from, src), (into, dest)] {
            let cleared: Vec<(CellRef, bool, u32)> = self.loaded[&i]
                .1
                .cells
                .iter()
                .filter(|(p, _)| area.contains(**p))
                .map(|(p, c)| (*p, c.value != Value::Empty || c.formula.is_some(), c.style))
                .collect();
            for (p, filled, style) in cleared {
                if filled {
                    self.set_input(i, p, Input::Clear)?;
                }
                if style != 0 {
                    self.apply_style(i, p, 0)?;
                }
            }
        }
        let date1904 = self.date1904;
        for (dr, dc, text, style) in moved {
            let at = CellRef::new(dest.start.row + dr, dest.start.col + dc);
            if !text.is_empty() {
                self.set_input(into, at, Input::parse(&text, date1904))?;
            }
            if style != 0 {
                self.apply_style(into, at, style)?;
            }
        }
        self.batch_changed = true;
        Ok(())
    }

    /// Runs `f` as one undo step (the batch the macros use), taken back
    /// whole when it fails.
    fn in_one_step(&mut self, f: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let r = f(self);
        if own {
            match &r {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        r
    }

    /// Sorts the rows of a range by column `key`, as Excel's Sort: numbers
    /// first, then text (ignoring case), logical values and errors, empty
    /// cells last either way; the first row stays when `header`. A row's
    /// values, formats and formulas move together, a formula's relative
    /// references shifted as a copy shifts them. One undo step.
    pub fn sort_range(
        &mut self,
        idx: usize,
        range: Range,
        key: u32,
        descending: bool,
        header: bool,
    ) -> Result<()> {
        let key = kalem_viewer::SortKey {
            col: key,
            descending,
            list: None,
        };
        self.sort_range_keys(idx, range, &[key], header)
    }

    /// Sorts the rows of a range by several columns in turn, as Excel's
    /// Custom Sort: each level ascending or descending, or in the order of
    /// a custom list (its values first, the others after as usual); ties
    /// keep their order. As [`Workbook::sort_range`] otherwise.
    pub fn sort_range_keys(
        &mut self,
        idx: usize,
        range: Range,
        keys: &[kalem_viewer::SortKey],
        header: bool,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if keys.is_empty() {
            return Err(Error::Refused("Sort by which column?".into()));
        }
        if keys
            .iter()
            .any(|k| !(range.start.col..=range.end.col).contains(&k.col))
        {
            return Err(Error::Refused(
                "the sort column is outside the range".into(),
            ));
        }
        let first = range.start.row + u32::from(header);
        if first >= range.end.row {
            return Ok(());
        }
        let body = Range {
            start: CellRef::new(first, range.start.col),
            end: range.end,
        };
        let model = &self.loaded[&idx].1;
        let touches = |m: &Range| {
            m.start.row <= body.end.row
                && body.start.row <= m.end.row
                && m.start.col <= body.end.col
                && body.start.col <= m.end.col
        };
        if let Some(m) = model.merged.iter().find(|m| touches(m)) {
            return Err(Error::Refused(format!(
                "the merged cell {m} is in the range; unmerge it first"
            )));
        }
        if model.cells.values().any(|c| matches!(&c.formula, Some(f) if matches!(f.kind, FormulaKind::Array { range: a } if touches(&a)))) {
            return Err(Error::Refused("an array formula is in the range".into()));
        }
        // Each row: its key and its cells as entered, with their styles.
        let mut rows = Vec::new();
        for r in first..=range.end.row {
            let mut k = Vec::new();
            for key in keys {
                let at = CellRef::new(r, key.col);
                k.push((self.value(idx, at)?, self.display(idx, at)?.to_lowercase()));
            }
            let mut cells = Vec::new();
            for c in range.start.col..=range.end.col {
                let at = CellRef::new(r, c);
                let Some(cell) = self.loaded[&idx].1.cells.get(&at).cloned() else {
                    continue;
                };
                let text = if cell.value != Value::Empty || cell.formula.is_some() {
                    self.edit_text(idx, at)?
                } else {
                    String::new()
                };
                cells.push((c, text, cell.style));
            }
            rows.push((r, k, cells));
        }
        let rank = |v: &Value| match v {
            Value::Number(_) => 0,
            Value::Text(_) => 1,
            Value::Bool(_) => 2,
            Value::Error(_) => 3,
            Value::Empty => 4,
        };
        let lists: Vec<Option<Vec<String>>> = keys
            .iter()
            .map(|k| {
                k.list
                    .as_ref()
                    .map(|l| l.iter().map(|x| x.to_lowercase()).collect())
            })
            .collect();
        let one = |i: usize, a: &(Value, String), b: &(Value, String)| {
            let (ra, rb) = (rank(&a.0), rank(&b.0));
            // Empty cells last, ascending or descending.
            if ra == 4 || rb == 4 {
                return ra.cmp(&rb);
            }
            // A custom list's values in its order, before the others.
            let place = |x: &(Value, String)| {
                lists[i]
                    .as_ref()
                    .and_then(|l| l.iter().position(|v| *v == x.1))
            };
            let o = match (place(a), place(b)) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => ra.cmp(&rb).then_with(|| match (&a.0, &b.0) {
                    (Value::Number(x), Value::Number(y)) => {
                        x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    (Value::Text(x), Value::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
                    (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
                    (Value::Error(x), Value::Error(y)) => x.cmp(y),
                    _ => std::cmp::Ordering::Equal,
                }),
            };
            if keys[i].descending { o.reverse() } else { o }
        };
        rows.sort_by(|a, b| {
            (0..keys.len())
                .map(|i| one(i, &a.1[i], &b.1[i]))
                .find(|o| o.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if rows
            .iter()
            .enumerate()
            .all(|(i, row)| row.0 == first + i as u32)
        {
            return Ok(());
        }
        let date1904 = self.date1904;
        self.in_one_step(|wb| {
            let cleared: Vec<(CellRef, bool, u32)> = wb.loaded[&idx]
                .1
                .cells
                .iter()
                .filter(|(p, _)| body.contains(**p))
                .map(|(p, c)| (*p, c.value != Value::Empty || c.formula.is_some(), c.style))
                .collect();
            for (p, filled, style) in cleared {
                if filled {
                    wb.set_input(idx, p, Input::Clear)?;
                }
                if style != 0 {
                    wb.apply_style(idx, p, 0)?;
                }
            }
            for (i, (old, _, cells)) in rows.into_iter().enumerate() {
                let new = first + i as u32;
                for (c, text, style) in cells {
                    let at = CellRef::new(new, c);
                    if let Some(f) = text.strip_prefix('=') {
                        let moved = formula::shift(f, i64::from(new) - i64::from(old), 0);
                        wb.set_input(idx, at, Input::Formula(moved))?;
                    } else if !text.is_empty() {
                        wb.set_input(idx, at, Input::parse(&text, date1904))?;
                    }
                    if style != 0 {
                        wb.apply_style(idx, at, style)?;
                    }
                }
            }
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Puts an AutoFilter on a range (its first row the headers), as
    /// Excel's Filter; `None` takes it off and shows the rows it hid.
    pub fn set_filter(&mut self, idx: usize, range: Option<Range>) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        self.in_one_step(|wb| {
            if let Some(old) = wb.loaded[&idx].1.auto_filter.clone() {
                let text = wb.loaded[&idx].0.clone();
                wb.replace_sheet_text(idx, splice(&text, vec![(old.span.clone(), String::new())]));
                let shown: Vec<(u32, bool)> = (old.range.start.row + 1..=old.range.end.row)
                    .map(|r| (r, false))
                    .collect();
                wb.set_rows_hidden(idx, &shown);
            }
            if let Some(range) = range {
                let p = wb.loaded[&idx].1.prefix.clone();
                let text = wb.loaded[&idx].0.clone();
                let xml = format!("<{p}autoFilter ref=\"{range}\"/>");
                wb.replace_sheet_text(idx, insert_top_level(&text, &AFTER_AUTO_FILTER, &xml));
            }
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Filters column `col` of the AutoFilter to the rows whose cell shows
    /// one of `values` (an empty text for empty cells), or clears its filter
    /// with `None`; the rows the filters hide get `hidden`, as Excel writes.
    pub fn filter_column(
        &mut self,
        idx: usize,
        col: u32,
        values: Option<Vec<String>>,
    ) -> Result<()> {
        self.filter_column_rule(idx, col, values.map(kalem_viewer::FilterRule::Values))
    }

    /// Filters column `col` of the AutoFilter by a rule, as Excel's Text,
    /// Number and Color Filters write it (`<filters>`, `<customFilters>`,
    /// `<top10>`, `<dynamicFilter>`, `<colorFilter>` with its `<dxf>`), or
    /// clears its filter with `None`; then hides the rows the filters
    /// hide. One undo step.
    pub fn filter_column_rule(
        &mut self,
        idx: usize,
        col: u32,
        rule: Option<kalem_viewer::FilterRule>,
    ) -> Result<()> {
        self.load(idx)?;
        let Some(af) = self.loaded[&idx].1.auto_filter.clone() else {
            return Err(Error::Refused(
                "the sheet has no filter; turn it on first".into(),
            ));
        };
        if !(af.range.start.col..=af.range.end.col).contains(&col) {
            return Err(Error::Refused("the column is outside the filter".into()));
        }
        let col_id = col - af.range.start.col;
        let p = self.loaded[&idx].1.prefix.clone();
        let mut columns: Vec<crate::sheet::FilterColumn> = af
            .columns
            .iter()
            .filter(|c| c.col_id != col_id)
            .cloned()
            .collect();
        self.in_one_step(|wb| {
            if let Some(rule) = &rule {
                let fc = wb.filter_column_xml(&p, col_id, rule)?;
                columns.push(fc);
                columns.sort_by_key(|c| c.col_id);
            }
            let inner: String = columns.iter().map(|c| c.raw.as_str()).collect();
            let xml = if inner.is_empty() {
                format!("<{p}autoFilter ref=\"{}\"/>", af.range)
            } else {
                format!(
                    "<{p}autoFilter ref=\"{}\">{inner}</{p}autoFilter>",
                    af.range
                )
            };
            let text = wb.loaded[&idx].0.clone();
            wb.replace_sheet_text(idx, splice(&text, vec![(af.span.clone(), xml)]));
            let rows = wb.filtered_rows(idx, af.range, &columns)?;
            wb.set_rows_hidden(idx, &rows);
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// A filter column's element for a rule, and what it is.
    fn filter_column_xml(
        &mut self,
        p: &str,
        col_id: u32,
        rule: &kalem_viewer::FilterRule,
    ) -> Result<crate::sheet::FilterColumn> {
        use crate::sheet::{FilterColumn, FilterKind};
        use kalem_viewer::{FilterOp, FilterRule};
        let open = format!("<{p}filterColumn colId=\"{col_id}\">");
        let close = format!("</{p}filterColumn>");
        let mut fc = FilterColumn {
            col_id,
            values: None,
            blank: false,
            raw: String::new(),
            kind: None,
        };
        match rule {
            FilterRule::Values(vs) => {
                let blank = vs.iter().any(String::is_empty);
                let shown: Vec<String> = vs.iter().filter(|v| !v.is_empty()).cloned().collect();
                let mut raw = format!(
                    "{open}<{p}filters{}>",
                    if blank { " blank=\"1\"" } else { "" }
                );
                for v in &shown {
                    raw.push_str(&format!("<{p}filter val=\"{}\"/>", xml::escape(v)));
                }
                raw.push_str(&format!("</{p}filters>{close}"));
                fc.values = Some(shown);
                fc.blank = blank;
                fc.raw = raw;
            }
            FilterRule::Custom { first, second } => {
                let cond = |op: FilterOp, v: &str| -> (String, String) {
                    let (o, v) = match op {
                        FilterOp::Equal => ("equal", v.to_owned()),
                        FilterOp::NotEqual => ("notEqual", v.to_owned()),
                        FilterOp::Greater => ("greaterThan", v.to_owned()),
                        FilterOp::GreaterOrEqual => ("greaterThanOrEqual", v.to_owned()),
                        FilterOp::Less => ("lessThan", v.to_owned()),
                        FilterOp::LessOrEqual => ("lessThanOrEqual", v.to_owned()),
                        FilterOp::BeginsWith => ("equal", format!("{v}*")),
                        FilterOp::EndsWith => ("equal", format!("*{v}")),
                        FilterOp::Contains => ("equal", format!("*{v}*")),
                        FilterOp::NotContains => ("notEqual", format!("*{v}*")),
                    };
                    (o.to_owned(), v)
                };
                let mut conditions = vec![cond(first.0, &first.1)];
                let and = second.as_ref().is_some_and(|s| s.0);
                if let Some((_, op, v)) = second {
                    conditions.push(cond(*op, v));
                }
                let mut raw = format!(
                    "{open}<{p}customFilters{}>",
                    if and { " and=\"1\"" } else { "" }
                );
                for (o, v) in &conditions {
                    let op = if o == "equal" {
                        String::new()
                    } else {
                        format!(" operator=\"{o}\"")
                    };
                    raw.push_str(&format!(
                        "<{p}customFilter{op} val=\"{}\"/>",
                        xml::escape(v)
                    ));
                }
                raw.push_str(&format!("</{p}customFilters>{close}"));
                fc.raw = raw;
                fc.kind = Some(FilterKind::Custom { and, conditions });
            }
            FilterRule::Top {
                count,
                percent,
                bottom,
            } => {
                fc.raw = format!(
                    "{open}<{p}top10{}{} val=\"{count}\"/>{close}",
                    if *bottom { " top=\"0\"" } else { "" },
                    if *percent { " percent=\"1\"" } else { "" }
                );
                fc.kind = Some(FilterKind::Top {
                    top: !bottom,
                    percent: *percent,
                    count: f64::from(*count),
                });
            }
            FilterRule::Average { above } => {
                let t = if *above {
                    "aboveAverage"
                } else {
                    "belowAverage"
                };
                fc.raw = format!("{open}<{p}dynamicFilter type=\"{t}\"/>{close}");
                fc.kind = Some(FilterKind::Dynamic(t.into()));
            }
            FilterRule::Fill([r, g, b]) => {
                let dxf = format!(
                    "<dxf><fill><patternFill patternType=\"solid\"><bgColor rgb=\"FF{r:02X}{g:02X}{b:02X}\"/></patternFill></fill></dxf>"
                );
                let id = self.add_dxf(&dxf)?;
                fc.raw = format!("{open}<{p}colorFilter dxfId=\"{id}\"/>{close}");
                fc.kind = Some(FilterKind::Color(id));
            }
        }
        Ok(fc)
    }

    /// Which rows of a filter's range its columns hide: every column of a
    /// kind read here must let a row show.
    fn filtered_rows(
        &mut self,
        idx: usize,
        range: Range,
        columns: &[crate::sheet::FilterColumn],
    ) -> Result<Vec<(u32, bool)>> {
        use crate::sheet::FilterKind;
        let body = range.start.row + 1..=range.end.row;
        // What the top ten and the average compare with, by column.
        let mut limits: HashMap<u32, f64> = HashMap::new();
        for c in columns {
            let col = range.start.col + c.col_id;
            let numbers = |wb: &mut Self| -> Result<Vec<f64>> {
                let mut v = Vec::new();
                for r in body.clone() {
                    if let Value::Number(n) = wb.value(idx, CellRef::new(r, col))? {
                        v.push(n);
                    }
                }
                Ok(v)
            };
            match &c.kind {
                Some(FilterKind::Top {
                    top,
                    percent,
                    count,
                }) => {
                    let mut v = numbers(self)?;
                    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    if *top {
                        v.reverse();
                    }
                    let n = if *percent {
                        ((v.len() as f64) * count / 100.0).ceil() as usize
                    } else {
                        *count as usize
                    };
                    if let Some(x) = v.get(n.clamp(1, v.len().max(1)) - 1) {
                        limits.insert(c.col_id, *x);
                    }
                }
                Some(FilterKind::Dynamic(_)) => {
                    let v = numbers(self)?;
                    if !v.is_empty() {
                        limits.insert(c.col_id, v.iter().sum::<f64>() / v.len() as f64);
                    }
                }
                _ => {}
            }
        }
        let mut rows = Vec::new();
        for r in body {
            let mut show = true;
            for c in columns {
                let at = CellRef::new(r, range.start.col + c.col_id);
                let ok = match (&c.values, &c.kind) {
                    (Some(values), _) => {
                        let shown = self.display(idx, at)?;
                        if shown.is_empty() {
                            c.blank
                        } else {
                            values.contains(&shown)
                        }
                    }
                    (_, Some(FilterKind::Custom { and, conditions })) => {
                        let value = self.value(idx, at)?;
                        let shown = self.display(idx, at)?;
                        let mut results = conditions
                            .iter()
                            .map(|(op, v)| filter_condition(op, v, &value, &shown));
                        if *and {
                            results.all(|x| x)
                        } else {
                            results.any(|x| x)
                        }
                    }
                    (_, Some(FilterKind::Top { top, .. })) => {
                        match (self.value(idx, at)?, limits.get(&c.col_id)) {
                            (Value::Number(n), Some(l)) => {
                                if *top {
                                    n >= *l
                                } else {
                                    n <= *l
                                }
                            }
                            _ => false,
                        }
                    }
                    (_, Some(FilterKind::Dynamic(t))) => {
                        match (self.value(idx, at)?, limits.get(&c.col_id)) {
                            (Value::Number(n), Some(l)) if t == "aboveAverage" => n > *l,
                            (Value::Number(n), Some(l)) if t == "belowAverage" => n < *l,
                            (_, _) if t == "aboveAverage" || t == "belowAverage" => false,
                            // Dates and the rest are not computed here.
                            _ => true,
                        }
                    }
                    (_, Some(FilterKind::Color(dxf))) => {
                        let want = self.styles.dxfs.get(*dxf as usize).and_then(|d| d.fill);
                        let style = self.loaded[&idx].1.cells.get(&at).map_or(0, |c| c.style);
                        want.is_some() && self.styles.get(style).fill == want
                    }
                    _ => true,
                };
                if !ok {
                    show = false;
                    break;
                }
            }
            rows.push((r, !show));
        }
        Ok(rows)
    }

    /// Reapply: the filter's rules applied again to the rows as they are
    /// now. One undo step.
    pub fn reapply_filter(&mut self, idx: usize) -> Result<()> {
        self.load(idx)?;
        let Some(af) = self.loaded[&idx].1.auto_filter.clone() else {
            return Err(Error::Refused("the sheet has no filter".into()));
        };
        self.in_one_step(|wb| {
            let rows = wb.filtered_rows(idx, af.range, &af.columns)?;
            wb.set_rows_hidden(idx, &rows);
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// The rule of filter column `col`, when it is one the rules tell.
    pub fn column_filter(&mut self, idx: usize, col: u32) -> Option<kalem_viewer::FilterRule> {
        use crate::sheet::FilterKind;
        use kalem_viewer::{FilterOp, FilterRule};
        self.load(idx).ok()?;
        let af = self.loaded[&idx].1.auto_filter.clone()?;
        let col_id = col.checked_sub(af.range.start.col)?;
        let c = af.columns.into_iter().find(|c| c.col_id == col_id)?;
        if let Some(mut v) = c.values {
            if c.blank {
                v.push(String::new());
            }
            return Some(FilterRule::Values(v));
        }
        Some(match c.kind? {
            FilterKind::Custom { and, conditions } => {
                let cond = |(o, v): &(String, String)| -> (FilterOp, String) {
                    let star_first = v.starts_with('*');
                    let star_last = v.ends_with('*') && v.len() > 1;
                    let inner = v.trim_matches('*').to_owned();
                    match o.as_str() {
                        "notEqual" if star_first && star_last => (FilterOp::NotContains, inner),
                        "notEqual" => (FilterOp::NotEqual, v.clone()),
                        "greaterThan" => (FilterOp::Greater, v.clone()),
                        "greaterThanOrEqual" => (FilterOp::GreaterOrEqual, v.clone()),
                        "lessThan" => (FilterOp::Less, v.clone()),
                        "lessThanOrEqual" => (FilterOp::LessOrEqual, v.clone()),
                        _ if star_first && star_last => (FilterOp::Contains, inner),
                        _ if star_last => (FilterOp::BeginsWith, inner),
                        _ if star_first => (FilterOp::EndsWith, inner),
                        _ => (FilterOp::Equal, v.clone()),
                    }
                };
                let first = cond(conditions.first()?);
                let second = conditions.get(1).map(|c| {
                    let (op, v) = cond(c);
                    (and, op, v)
                });
                FilterRule::Custom { first, second }
            }
            FilterKind::Top {
                top,
                percent,
                count,
            } => FilterRule::Top {
                count: count as u32,
                percent,
                bottom: !top,
            },
            FilterKind::Dynamic(t) if t == "aboveAverage" => FilterRule::Average { above: true },
            FilterKind::Dynamic(t) if t == "belowAverage" => FilterRule::Average { above: false },
            FilterKind::Dynamic(_) => return None,
            FilterKind::Color(dxf) => {
                let fill = self.styles.dxfs.get(dxf as usize)?.fill?;
                FilterRule::Fill([(fill >> 16) as u8, (fill >> 8) as u8, fill as u8])
            }
        })
    }

    /// Hides or shows rows (`<row hidden>`), making a `<row>` to hide one
    /// that holds nothing.
    fn set_rows_hidden(&mut self, idx: usize, rows: &[(u32, bool)]) {
        let (text, model) = &self.loaded[&idx];
        let mut splices = Vec::new();
        let mut missing = Vec::new();
        for &(r, hide) in rows {
            match model.rows.get(&r) {
                Some(row) if row.hidden != hide => {
                    let tag = &text[row.start.clone()];
                    let new = if hide {
                        xml::set_attr(tag, "hidden", "1")
                    } else {
                        xml::remove_attr(tag, "hidden")
                    };
                    splices.push((row.start.clone(), new));
                }
                Some(_) => {}
                None if hide => missing.push(r),
                None => {}
            }
        }
        if !splices.is_empty() {
            let new = splice(text, splices);
            self.replace_sheet_text(idx, new);
        }
        for r in missing {
            let (text, model) = &self.loaded[&idx];
            let p = &model.prefix;
            let new_row = format!("<{p}row r=\"{}\" hidden=\"1\"/>", r + 1);
            let Some((sd_start, sd_end)) = &model.sheet_data else {
                return;
            };
            let splices = match sd_end {
                None => {
                    let tag = &text[sd_start.clone()];
                    let open = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
                    vec![(sd_start.clone(), format!("{open}>{new_row}</{p}sheetData>"))]
                }
                Some(_) => {
                    let at = model
                        .rows
                        .range(..r)
                        .next_back()
                        .map_or(sd_start.end, |(_, row)| {
                            row.end.as_ref().map_or(row.start.end, |e| e.end)
                        });
                    vec![(at..at, new_row)]
                }
            };
            let new = splice(text, splices);
            self.replace_sheet_text(idx, new);
        }
    }
    /// Clears a range as Excel's Clear: its contents, its formats (the
    /// cells' style the workbook's default), or both with its notes (Clear
    /// All). One undo step.
    pub fn clear_parts(
        &mut self,
        idx: usize,
        range: Range,
        contents: bool,
        formats: bool,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = (|| -> Result<()> {
            if contents {
                self.clear_range(idx, range)?;
            }
            if formats {
                let styled: Vec<CellRef> = self.loaded[&idx]
                    .1
                    .cells
                    .iter()
                    .filter(|(p, c)| range.contains(**p) && c.style != 0)
                    .map(|(p, _)| *p)
                    .collect();
                for at in styled {
                    self.apply_style(idx, at, 0)?;
                }
            }
            if contents && formats {
                let notes: Vec<CellRef> = self
                    .comments(idx)?
                    .into_iter()
                    .map(|c| c.cell)
                    .filter(|p| range.contains(*p))
                    .collect();
                for at in notes {
                    self.set_comment(idx, at, None)?;
                }
            }
            Ok(())
        })();
        self.batch_changed = true;
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Removes the rows of `range` that repeat an earlier one in `columns`
    /// (what the cells show, small and capital letters alike), as Excel's
    /// Remove Duplicates: the rows kept move up within the range, formulas
    /// and formats with them. One undo step; how many rows went.
    pub fn remove_duplicates(
        &mut self,
        idx: usize,
        range: Range,
        columns: &[u32],
        header: bool,
    ) -> Result<usize> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let first = range.start.row + u32::from(header);
        if first > range.end.row {
            return Ok(0);
        }
        let cols: Vec<u32> = if columns.is_empty() {
            (range.start.col..=range.end.col).collect()
        } else {
            columns
                .iter()
                .copied()
                .filter(|c| (range.start.col..=range.end.col).contains(c))
                .collect()
        };
        let mut seen = HashSet::new();
        let mut kept = Vec::new();
        for r in first..=range.end.row {
            let mut key = Vec::new();
            for &c in &cols {
                key.push(self.display(idx, CellRef::new(r, c))?.to_lowercase());
            }
            if seen.insert(key) {
                kept.push(r);
            }
        }
        let removed = (range.end.row - first + 1) as usize - kept.len();
        if removed == 0 {
            return Ok(0);
        }
        // Every row's cells, read before any is written.
        let mut rows: HashMap<u32, Vec<(Option<String>, Value, u32)>> = HashMap::new();
        for r in first..=range.end.row {
            let mut line = Vec::new();
            for c in range.start.col..=range.end.col {
                let at = CellRef::new(r, c);
                let cell = self.loaded[&idx].1.cells.get(&at).cloned();
                let value = self.value(idx, at)?;
                let style = cell.as_ref().map_or(0, |c| c.style);
                line.push((cell.and_then(|c| c.formula.map(|f| f.text)), value, style));
            }
            rows.insert(r, line);
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = (|| -> Result<()> {
            for (t, row) in (first..=range.end.row).enumerate() {
                let source = kept.get(t).copied();
                if source == Some(row) {
                    continue;
                }
                for (k, c) in (range.start.col..=range.end.col).enumerate() {
                    let at = CellRef::new(row, c);
                    let (input, style) = match source {
                        Some(src) => {
                            let (formula, value, style) = rows[&src][k].clone();
                            let input = match (formula, value) {
                                (Some(f), _) => Input::Formula(formula::shift(
                                    &f,
                                    i64::from(row) - i64::from(src),
                                    0,
                                )),
                                (_, Value::Number(v)) => Input::Number(v, None),
                                (_, Value::Text(t)) => Input::Text(t),
                                (_, Value::Bool(b)) => Input::Bool(b),
                                (_, Value::Error(e)) => Input::Error(e),
                                (_, Value::Empty) => Input::Clear,
                            };
                            (input, style)
                        }
                        None => (Input::Clear, 0),
                    };
                    let had = self.loaded[&idx].1.cells.get(&at).map(|c| c.style);
                    if had.is_some() || !matches!(input, Input::Clear) {
                        self.set_input(idx, at, input)?;
                    }
                    let now = self.loaded[&idx].1.cells.get(&at).map(|c| c.style);
                    if now.is_some_and(|s| s != style) || (now.is_none() && style != 0) {
                        self.apply_style(idx, at, style)?;
                    }
                }
            }
            Ok(())
        })();
        self.batch_changed = true;
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result.map(|()| removed)
    }

    /// Gives the cells of `to` the formats of `from`, repeated over it as
    /// Excel's Format Painter paints a larger selection. One undo step.
    pub fn fill_formats(&mut self, from: (usize, Range), to: (usize, Range)) -> Result<()> {
        let ((si, src), (di, dst)) = (from, to);
        for i in [si, di] {
            self.load(i)?;
            if self.sheets[i].kind != SheetKind::Worksheet {
                return Err(Error::NotAWorksheet(self.sheets[i].name.clone()));
            }
        }
        let area =
            u64::from(dst.end.row - dst.start.row + 1) * u64::from(dst.end.col - dst.start.col + 1);
        if area > 200_000 {
            return Err(Error::Refused("Select fewer cells: 200,000 at most".into()));
        }
        let (h, w) = (
            src.end.row - src.start.row + 1,
            src.end.col - src.start.col + 1,
        );
        let mut styles = Vec::new();
        for r in 0..h {
            for c in 0..w {
                let p = CellRef::new(src.start.row + r, src.start.col + c);
                let style = self.loaded[&si]
                    .1
                    .cells
                    .get(&p)
                    .map_or_else(|| self.row_or_col_style(si, p), |c| c.style);
                styles.push(style);
            }
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = (|| -> Result<()> {
            for r in dst.start.row..=dst.end.row {
                for c in dst.start.col..=dst.end.col {
                    let at = CellRef::new(r, c);
                    let i = ((r - dst.start.row) % h) * w + (c - dst.start.col) % w;
                    let style = styles[i as usize];
                    let now = self.loaded[&di].1.cells.get(&at).map(|c| c.style);
                    if now.unwrap_or_else(|| self.row_or_col_style(di, at)) != style
                        || (now.is_none() && style != 0)
                    {
                        self.apply_style(di, at, style)?;
                    }
                }
            }
            Ok(())
        })();
        self.batch_changed = true;
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Clears the values and formulas of a range, formats kept, as Excel's
    /// Delete on a selection: one undo step.
    pub fn clear_range(&mut self, idx: usize, range: Range) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        let filled: Vec<CellRef> = self.loaded[&idx]
            .1
            .cells
            .iter()
            .filter(|(p, c)| {
                range.contains(**p) && (c.value != Value::Empty || c.formula.is_some())
            })
            .map(|(p, _)| *p)
            .collect();
        if filled.is_empty() {
            return Ok(());
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let mut result = Ok(());
        for at in filled {
            result = self.set_input(idx, at, Input::Clear);
            if result.is_err() {
                break;
            }
        }
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    /// Merges a range into one cell, as Excel's Merge Cells (and Merge &
    /// Center with `center`): the other cells' values are cleared, as Excel
    /// clears them, and a `<mergeCell>` is written. One undo step.
    pub fn merge_cells(&mut self, idx: usize, range: Range, center: bool) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if range.start == range.end {
            return Err(Error::Refused("select more than one cell to merge".into()));
        }
        let model = &self.loaded[&idx].1;
        let overlaps = |m: &Range| {
            m.start.row <= range.end.row
                && range.start.row <= m.end.row
                && m.start.col <= range.end.col
                && range.start.col <= m.end.col
        };
        if let Some(m) = model.merged.iter().find(|m| overlaps(m)) {
            return Err(Error::Refused(format!(
                "{range} overlaps the merged cell {m}; unmerge it first"
            )));
        }
        for c in model.cells.values() {
            if let Some(f) = &c.formula
                && let FormulaKind::Array { range: a } = f.kind
                && overlaps(&a)
            {
                return Err(Error::Refused(format!(
                    "{range} holds part of the array formula over {a}"
                )));
            }
        }
        let own = self.batch.is_none();
        if own {
            self.begin_batch()?;
        }
        let result = self.merge_inner(idx, range, center);
        if own {
            match &result {
                Ok(()) => {
                    self.end_batch()?;
                }
                Err(_) => {
                    if let Some(s) = self.batch.take() {
                        self.restore(s);
                    }
                }
            }
        }
        result
    }

    fn merge_inner(&mut self, idx: usize, range: Range, center: bool) -> Result<()> {
        // Only the first cell's value stays.
        let others: Vec<CellRef> = self.loaded[&idx]
            .1
            .cells
            .iter()
            .filter(|(p, c)| {
                range.contains(**p)
                    && **p != range.start
                    && (c.value != Value::Empty || c.formula.is_some())
            })
            .map(|(p, _)| *p)
            .collect();
        for at in others {
            self.set_input(idx, at, Input::Clear)?;
        }
        if center {
            let style = self.loaded[&idx]
                .1
                .cells
                .get(&range.start)
                .map_or(0, |c| c.style);
            let new_style = self.derive_style(style, u32::MAX - 2, |src| {
                with_alignment(src, "horizontal", "center")
            })?;
            self.apply_style(idx, range.start, new_style)?;
        }
        let new = add_merge(&self.loaded[&idx].0, &range.to_string());
        self.replace_sheet_text(idx, new);
        self.batch_changed = true;
        Ok(())
    }

    /// Splits the merged range holding `at` back into cells, as Excel's
    /// Unmerge Cells; the values stay where they are.
    pub fn unmerge_cells(&mut self, idx: usize, at: CellRef) -> Result<()> {
        self.load(idx)?;
        let Some(m) = self.loaded[&idx].1.merge_at(at) else {
            return Err(Error::Refused(format!("{at} is not in a merged cell")));
        };
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let new = remove_merge(&self.loaded[&idx].0, m);
        self.replace_sheet_text(idx, new);
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    fn set_full_calc_on_load(&mut self) {
        let text = self.workbook_xml.clone();
        let mut r = Reader::new(&text);
        let mut insert_before: Option<usize> = None;
        let mut close: Option<usize> = None;
        let mut prefix = String::new();
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) if tag.name == "workbook" => {
                    prefix = xml::prefix(tag.qname).to_owned()
                }
                Token::Start(tag) if tag.name == "calcPr" => {
                    if tag
                        .attr("fullCalcOnLoad")
                        .as_deref()
                        .is_some_and(|v| v == "1" || v == "true")
                    {
                        return;
                    }
                    let new_tag = xml::set_attr(&text[tag.span.clone()], "fullCalcOnLoad", "1");
                    self.workbook_xml = splice(&text, vec![(tag.span.clone(), new_tag)]);
                    return;
                }
                Token::Start(tag)
                    if insert_before.is_none()
                        && matches!(
                            tag.name,
                            "oleSize"
                                | "customWorkbookViews"
                                | "pivotCaches"
                                | "smartTagPr"
                                | "smartTagTypes"
                                | "webPublishing"
                                | "fileRecoveryPr"
                                | "webPublishObjects"
                                | "extLst"
                        ) =>
                {
                    insert_before = Some(tag.span.start);
                    if !tag.empty {
                        r.skip_element();
                    }
                }
                Token::End {
                    name: "workbook",
                    span,
                } => close = Some(span.start),
                _ => {}
            }
        }
        if let Some(at) = insert_before.or(close) {
            let tag = format!("<{prefix}calcPr fullCalcOnLoad=\"1\"/>");
            self.workbook_xml = splice(&text, vec![(at..at, tag)]);
        }
    }

    fn drop_calc_chain(&mut self) -> Result<()> {
        let Some(rel) = self
            .workbook_rels
            .iter()
            .find(|r| r.kind == kind::CALC_CHAIN)
            .cloned()
        else {
            return Ok(());
        };
        let part = rels::resolve(&self.workbook_part, &rel.target);
        self.pkg.remove_part(&part);
        let rels_path = rels::rels_path(&self.workbook_part);
        let rels_text = text_of(self.pkg.part(&rels_path)?, &rels_path)?;
        self.pkg
            .set_part(&rels_path, rels::remove(&rels_text, &rel.id).into_bytes());
        let ct = "[Content_Types].xml";
        let ct_text = text_of(self.pkg.part(ct)?, ct)?;
        self.pkg
            .set_part(ct, rels::remove_override(&ct_text, &part).into_bytes());
        self.workbook_rels.retain(|r| r.id != rel.id);
        Ok(())
    }

    /// Whether anything was edited since the workbook was opened.
    pub fn is_dirty(&self) -> bool {
        !self.dirty_sheets.is_empty() || self.pkg.is_dirty() || self.styles_xml.is_some()
    }

    fn staged(&self) -> Package {
        let mut pkg = self.pkg.clone();
        for &i in &self.dirty_sheets {
            pkg.set_part(&self.sheets[i].part, self.loaded[&i].0.clone().into_bytes());
        }
        if let (Some(part), Some(text)) = (&self.styles_part, &self.styles_xml) {
            pkg.set_part(part, text.clone().into_bytes());
        }
        if self.is_dirty() {
            let current = self.pkg.part(&self.workbook_part).ok();
            if current.as_deref() != Some(self.workbook_xml.as_bytes()) {
                pkg.set_part(&self.workbook_part, self.workbook_xml.clone().into_bytes());
            }
        }
        pkg
    }

    /// The parts a save would write anew, add or remove.
    pub fn changed_parts(&self) -> Vec<String> {
        self.staged().changed_parts()
    }

    /// The workbook as file bytes: the input itself when nothing changed.
    pub fn save(&self) -> Result<Vec<u8>> {
        Ok(self.staged().write()?)
    }

    /// The grid of a sheet's used range as display strings, row by row.
    pub fn grid(&mut self, idx: usize) -> Result<Vec<Vec<String>>> {
        self.load(idx)?;
        self.flush()?;
        self.compute_missing(idx)?;
        let model = &self.loaded[&idx].1;
        let Some(used) = model.used_range() else {
            return Ok(Vec::new());
        };
        let width = (used.end.col + 1) as usize;
        let mut rows: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        for (pos, c) in &model.cells {
            let row = rows
                .entry(pos.row)
                .or_insert_with(|| vec![String::new(); width]);
            row[pos.col as usize] = self.format_value(c, &self.value_of(idx, *pos, c));
        }
        Ok((0..=used.end.row)
            .map(|r| {
                rows.remove(&r)
                    .unwrap_or_else(|| vec![String::new(); width])
            })
            .collect())
    }
}

/// A sheet part with column `col` (one-based) `width` wide: its `<col>`
/// split around it, or a new one, `<cols>` made where the schema puts it.
fn col_width_text(text: &str, col: u32, width: f64) -> String {
    let w = format!("{}", (width * 100.0).round() / 100.0);
    col_attrs_text(text, col, &|tag: &str| {
        let t = xml::set_attr(tag, "width", &w);
        xml::set_attr(&t, "customWidth", "1")
    })
}

/// A sheet part with column `col` (one-based) given its own `<col>`, made
/// by `set` from the one it was in (split around it) or a new one.
fn col_attrs_text(text: &str, col: u32, set: &dyn Fn(&str) -> String) -> String {
    let mut r = Reader::new(text);
    let mut cols_start: Option<(Span<usize>, bool, String)> = None;
    let mut cols_end: Option<usize> = None;
    let mut entries: Vec<(Span<usize>, u32, u32)> = Vec::new();
    // Where `<cols>` goes when there is none: before `<sheetData>`.
    let mut sheet_data: Option<(usize, String)> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "cols" => {
                cols_start = Some((
                    tag.span.clone(),
                    tag.empty,
                    xml::prefix(tag.qname).to_owned(),
                ));
            }
            Token::Start(tag)
                if tag.name == "col" && cols_start.is_some() && cols_end.is_none() =>
            {
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                let get = |k| tag.attr(k).and_then(|v| v.parse::<u32>().ok());
                if let (Some(a), Some(b)) = (get("min"), get("max")) {
                    entries.push((tag.span.start..end, a, b));
                }
            }
            Token::End { name: "cols", span } => cols_end = Some(span.start),
            Token::Start(tag) if tag.name == "sheetData" => {
                sheet_data = Some((tag.span.start, xml::prefix(tag.qname).to_owned()));
                break;
            }
            _ => {}
        }
    }
    match (cols_start, cols_end) {
        (Some((open, false, prefix)), Some(close)) => {
            if let Some((span, a, b)) = entries
                .iter()
                .find(|(_, a, b)| (*a..=*b).contains(&col))
                .cloned()
            {
                // Split `a..=b` into before, the column, after, each a copy.
                let el = &text[span.clone()];
                let tag_end = el.find('>').map_or(el.len(), |p| p + 1);
                let (head, rest) = el.split_at(tag_end);
                let piece = |lo: u32, hi: u32, width: bool| -> String {
                    let h = xml::set_attr(head, "min", &lo.to_string());
                    let h = xml::set_attr(&h, "max", &hi.to_string());
                    let h = if width { set(&h) } else { h };
                    format!("{h}{rest}")
                };
                let mut out = String::new();
                if a < col {
                    out.push_str(&piece(a, col - 1, false));
                }
                out.push_str(&piece(col, col, true));
                if col < b {
                    out.push_str(&piece(col + 1, b, false));
                }
                return splice(text, vec![(span, out)]);
            }
            let new = set(&format!("<{prefix}col min=\"{col}\" max=\"{col}\"/>"));
            let at = entries
                .iter()
                .find(|(_, a, _)| *a > col)
                .map_or(close, |(s, _, _)| s.start);
            let _ = open;
            splice(text, vec![(at..at, new)])
        }
        (Some((open, true, prefix)), _) => {
            let new = set(&format!("<{prefix}col min=\"{col}\" max=\"{col}\"/>"));
            splice(
                text,
                vec![(open, format!("<{prefix}cols>{new}</{prefix}cols>"))],
            )
        }
        _ => match sheet_data {
            Some((at, prefix)) => {
                let new = set(&format!("<{prefix}col min=\"{col}\" max=\"{col}\"/>"));
                splice(
                    text,
                    vec![(at..at, format!("<{prefix}cols>{new}</{prefix}cols>"))],
                )
            }
            None => text.to_owned(),
        },
    }
}

/// A sheet part with its first view's panes frozen at `rows` and `cols`
/// (none: not frozen): its `<pane>` and the selections of panes replaced.
fn frozen_text(text: &str, p: &str, rows: u32, cols: u32) -> String {
    let pane = if rows == 0 && cols == 0 {
        String::new()
    } else {
        let active = match (rows > 0, cols > 0) {
            (true, true) => "bottomRight",
            (true, false) => "bottomLeft",
            _ => "topRight",
        };
        let mut a = String::new();
        if cols > 0 {
            a.push_str(&format!(" xSplit=\"{cols}\""));
        }
        if rows > 0 {
            a.push_str(&format!(" ySplit=\"{rows}\""));
        }
        format!(
            "<{p}pane{a} topLeftCell=\"{}\" activePane=\"{active}\" state=\"frozen\"/>",
            CellRef::new(rows, cols)
        )
    };
    let mut r = Reader::new(text);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        match tag.name {
            "sheetView" => {
                if tag.empty {
                    if pane.is_empty() {
                        return text.to_owned();
                    }
                    let el = &text[tag.span.clone()];
                    let open = el.trim_end_matches("/>").trim_end();
                    return splice(
                        text,
                        vec![(tag.span.clone(), format!("{open}>{pane}</{p}sheetView>"))],
                    );
                }
                // The old pane and its selections out, the new pane first.
                let open_end = tag.span.end;
                let mut splices = vec![(open_end..open_end, pane)];
                while let Some(t) = r.next_token() {
                    match t {
                        Token::Start(k) => {
                            // Children of the view only: each skipped whole.
                            let gone = k.name == "pane"
                                || (k.name == "selection" && k.attr("pane").is_some());
                            let end = if k.empty {
                                k.span.end
                            } else {
                                r.skip_element()
                            };
                            if gone {
                                splices.push((k.span.start..end, String::new()));
                            }
                        }
                        Token::End {
                            name: "sheetView", ..
                        } => break,
                        _ => {}
                    }
                }
                return splice(text, splices);
            }
            "sheetData" => break,
            _ => {}
        }
    }
    if pane.is_empty() {
        return text.to_owned();
    }
    insert_top_level(
        text,
        &["sheetFormatPr", "cols", "sheetData"],
        &format!(
            "<{p}sheetViews><{p}sheetView workbookViewId=\"0\">{pane}</{p}sheetView></{p}sheetViews>"
        ),
    )
}

/// A sheet part with rows `from..=to` (zero-based) hidden or shown: the
/// `hidden` of their `<row>`s, rows made for the ones the part lacks.
fn rows_hidden_text(text: &str, model: &Sheet, from: u32, to: u32, hidden: bool) -> Result<String> {
    let p = &model.prefix;
    let mut splices: Vec<(Span<usize>, String)> = Vec::new();
    let mut inserts: BTreeMap<usize, String> = BTreeMap::new();
    let Some((sd_start, sd_end)) = &model.sheet_data else {
        return Err(Error::Refused("the sheet part has no sheetData".into()));
    };
    let mut fresh = String::new();
    for row in from..=to {
        match model.rows.get(&row) {
            Some(r) => {
                let tag = &text[r.start.clone()];
                let new = if hidden {
                    xml::set_attr(tag, "hidden", "1")
                } else {
                    xml::remove_attr(tag, "hidden")
                };
                if new != tag {
                    splices.push((r.start.clone(), new));
                }
            }
            None if hidden => {
                let el = format!("<{p}row r=\"{}\" hidden=\"1\"/>", row + 1);
                if sd_end.is_none() {
                    fresh.push_str(&el);
                    continue;
                }
                let at = model
                    .rows
                    .range(..row)
                    .next_back()
                    .map_or(sd_start.end, |(_, r)| {
                        r.end.as_ref().map_or(r.start.end, |e| e.end)
                    });
                inserts.entry(at).or_default().push_str(&el);
            }
            None => {}
        }
    }
    if !fresh.is_empty() {
        let tag = &text[sd_start.clone()];
        let open = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
        splices.push((sd_start.clone(), format!("{open}>{fresh}</{p}sheetData>")));
    }
    splices.extend(inserts.into_iter().map(|(at, s)| (at..at, s)));
    splices.sort_by_key(|s| (s.0.start, s.0.end));
    Ok(splice(text, splices))
}

/// An `<xf>` element with its text wrapping set: `wrapText` on its
/// `<alignment>`, which is made, first of its children, when there is none.
fn with_wrap(src: &str, wrap: bool) -> String {
    with_alignment(src, "wrapText", if wrap { "1" } else { "0" })
}

/// An `<xf>` element with one attribute of its `<alignment>` set, the
/// `<alignment>` made, first of its children, when there is none.
fn with_alignment(src: &str, attr: &str, value: &str) -> String {
    let tag_end = src.find('>').map_or(src.len(), |p| p + 1);
    let head = xml::set_attr(&src[..tag_end], "applyAlignment", "1");
    let name_end = head[1..]
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .map_or(head.len(), |p| p + 1);
    let p = xml::prefix(&head[1..name_end]).to_owned();
    let rest = &src[tag_end..];
    if let Some(a) = rest.find(&format!("<{p}alignment")) {
        let a_end = rest[a..].find('>').map_or(rest.len(), |e| a + e + 1);
        let tag = xml::set_attr(&rest[a..a_end], attr, value);
        return format!("{head}{}{tag}{}", &rest[..a], &rest[a_end..]);
    }
    let alignment = format!("<{p}alignment {attr}=\"{}\"/>", xml::escape(value));
    if head.ends_with("/>") {
        let open = head[..head.len() - 2].trim_end();
        format!("{open}>{alignment}</{p}xf>")
    } else {
        format!("{head}{alignment}{rest}")
    }
}

/// The worksheet's children that come after `<mergeCells>` in the schema
/// (CT_Worksheet): a new `<mergeCells>` goes before the first of them.
const AFTER_MERGE_CELLS: [&str; 24] = [
    "phoneticPr",
    "conditionalFormatting",
    "dataValidations",
    "hyperlinks",
    "printOptions",
    "pageMargins",
    "pageSetup",
    "headerFooter",
    "rowBreaks",
    "colBreaks",
    "customProperties",
    "cellWatches",
    "ignoredErrors",
    "smartTags",
    "drawing",
    "legacyDrawing",
    "legacyDrawingHF",
    "drawingHF",
    "picture",
    "oleObjects",
    "controls",
    "webPublishItems",
    "tableParts",
    "extLst",
];

/// The worksheet's children that come after `<autoFilter>` in the schema.
const AFTER_AUTO_FILTER: [&str; 28] = [
    "sortState",
    "dataConsolidate",
    "customSheetViews",
    "mergeCells",
    "phoneticPr",
    "conditionalFormatting",
    "dataValidations",
    "hyperlinks",
    "printOptions",
    "pageMargins",
    "pageSetup",
    "headerFooter",
    "rowBreaks",
    "colBreaks",
    "customProperties",
    "cellWatches",
    "ignoredErrors",
    "smartTags",
    "drawing",
    "legacyDrawing",
    "legacyDrawingHF",
    "drawingHF",
    "picture",
    "oleObjects",
    "controls",
    "webPublishItems",
    "tableParts",
    "extLst",
];

/// The children of a top-level list of `styles.xml` (`fonts`, `fills`,
/// `cellXfs`), as byte ranges.
fn style_children(text: &str, list: &str) -> Option<Vec<Span<usize>>> {
    let mut r = Reader::new(text);
    let mut depth = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == list {
                    if tag.empty {
                        return Some(Vec::new());
                    }
                    let mut out = Vec::new();
                    while let Some(t2) = r.next_token() {
                        match t2 {
                            Token::Start(c) => {
                                let end = if c.empty {
                                    c.span.end
                                } else {
                                    r.skip_element()
                                };
                                out.push(c.span.start..end);
                            }
                            Token::End { .. } => return Some(out),
                            Token::Text { .. } => {}
                        }
                    }
                    return Some(out);
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => depth -= 1,
            Token::Text { .. } => {}
        }
    }
    None
}

/// `styles.xml` with `el` the last child of its list `list` (`count` kept
/// right), and its index; the one already there when the list has it.
/// The id of number format `code`: a built-in one's, one the style sheet
/// has, or one added to its `<numFmts>` (made when there is none).
fn with_num_fmt(text: &str, code: &str) -> (String, u32) {
    if let Some(id) = (0..=49).find(|&i| crate::numfmt::builtin(i) == Some(code)) {
        return (text.to_owned(), id);
    }
    let mut r = Reader::new(text);
    let mut root: Option<std::ops::Range<usize>> = None;
    let mut list: Option<String> = None;
    let mut top = 163;
    let mut depth = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 0 {
                    root = Some(tag.span.clone());
                } else if depth == 1 && tag.name == "numFmts" {
                    list = Some(xml::prefix(tag.qname).to_owned());
                } else if tag.name == "numFmt" && list.is_some() {
                    let id: u32 = tag
                        .attr("numFmtId")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    if tag.attr("formatCode").as_deref() == Some(code) {
                        return (text.to_owned(), id);
                    }
                    top = top.max(id);
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { name, .. } => {
                depth -= 1;
                if name == "numFmts" {
                    break;
                }
            }
            Token::Text { .. } => {}
        }
    }
    let id = top + 1;
    match list {
        Some(p) => {
            let el = format!(
                "<{p}numFmt numFmtId=\"{id}\" formatCode=\"{}\"/>",
                xml::escape(code)
            );
            (add_style_child(text, "numFmts", &el).0, id)
        }
        None => {
            let Some(root) = root else {
                return (text.to_owned(), 0);
            };
            let p = xml::prefix(
                text[root.clone()]
                    .trim_start_matches('<')
                    .split([' ', '/', '>'])
                    .next()
                    .unwrap_or(""),
            )
            .to_owned();
            let el = format!(
                "<{p}numFmts count=\"1\"><{p}numFmt numFmtId=\"{id}\" formatCode=\"{}\"/></{p}numFmts>",
                xml::escape(code)
            );
            (splice(text, vec![(root.end..root.end, el)]), id)
        }
    }
}

/// Whether a cell passes a filter's custom condition (Excel's operators;
/// `*` and `?` wildcards in text, compared in either case; numbers by
/// value).
fn filter_condition(op: &str, want: &str, value: &Value, shown: &str) -> bool {
    use std::cmp::Ordering;
    let order = match (value, want.trim().parse::<f64>()) {
        (Value::Number(n), Ok(w)) => n.partial_cmp(&w),
        _ => Some(shown.to_lowercase().cmp(&want.to_lowercase())),
    };
    let equal = match (value, want.trim().parse::<f64>()) {
        (Value::Number(n), Ok(w)) => *n == w,
        _ => wildcard(&want.to_lowercase(), &shown.to_lowercase()),
    };
    match op {
        "notEqual" => !equal,
        "greaterThan" => order == Some(Ordering::Greater),
        "greaterThanOrEqual" => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
        "lessThan" => order == Some(Ordering::Less),
        "lessThanOrEqual" => matches!(order, Some(Ordering::Less | Ordering::Equal)),
        _ => equal,
    }
}

/// Whether `text` matches `pattern`, `*` any run and `?` any character.
fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut i, mut j) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while j < t.len() {
        if i < p.len() && (p[i] == '?' || p[i] == t[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            mark = j;
            i += 1;
        } else if let Some(s) = star {
            i = s + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    p[i..].iter().all(|c| *c == '*')
}

/// Which of a cell's sides a change's borders draw (`Some(true)`), take
/// away (`Some(false)`) or leave: top, right, bottom, left.
fn border_sides(
    change: &kalem_viewer::StyleChange,
    range: Range,
    at: CellRef,
) -> [Option<bool>; 4] {
    use kalem_viewer::BorderSet;
    let Some((set, _)) = change.borders else {
        return [None; 4];
    };
    let edge = [
        at.row == range.start.row,
        at.col == range.end.col,
        at.row == range.end.row,
        at.col == range.start.col,
    ];
    let on = |i: usize| edge[i].then_some(true);
    match set {
        BorderSet::All => [Some(true); 4],
        BorderSet::None => [Some(false); 4],
        BorderSet::Outside | BorderSet::ThickOutside => [on(0), on(1), on(2), on(3)],
        BorderSet::Top => [on(0), None, None, None],
        BorderSet::Right => [None, on(1), None, None],
        BorderSet::Bottom => [None, None, on(2), None],
        BorderSet::Left => [None, None, None, on(3)],
    }
}

fn add_style_child(text: &str, list: &str, el: &str) -> (String, usize) {
    let kids = style_children(text, list).unwrap_or_default();
    if let Some(i) = kids.iter().position(|k| text[k.clone()] == *el) {
        return (text.to_owned(), i);
    }
    let mut r = Reader::new(text);
    let mut depth = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == list {
                    let p = xml::prefix(tag.qname).to_owned();
                    // Markup made from the sheet's own elements has the
                    // prefix already.
                    let el = if p.is_empty() || el.starts_with(&format!("<{p}")) {
                        el.to_owned()
                    } else {
                        with_prefix(el, &p)
                    };
                    let open = xml::set_attr(
                        &text[tag.span.clone()],
                        "count",
                        &(kids.len() + 1).to_string(),
                    );
                    if tag.empty {
                        let o = open.trim_end_matches("/>").trim_end().to_owned();
                        return (
                            splice(
                                text,
                                vec![(tag.span.clone(), format!("{o}>{el}</{p}{list}>"))],
                            ),
                            0,
                        );
                    }
                    let end = r.skip_element();
                    let close = text[..end].rfind("</").unwrap_or(end);
                    return (
                        splice(text, vec![(tag.span.clone(), open), (close..close, el)]),
                        kids.len(),
                    );
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => depth -= 1,
            Token::Text { .. } => {}
        }
    }
    (text.to_owned(), 0)
}

/// A sheet part with a `<dataValidation>` added to the sheet's
/// `<dataValidations>`, made where the schema puts it when missing.
fn add_validation(text: &str, p: &str, el: &str) -> String {
    let mut r = Reader::new(text);
    let mut depth = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && tag.name == "dataValidations" {
                    if tag.empty {
                        let o = text[tag.span.clone()]
                            .trim_end_matches("/>")
                            .trim_end()
                            .to_owned();
                        return splice(
                            text,
                            vec![(tag.span.clone(), format!("{o}>{el}</{p}dataValidations>"))],
                        );
                    }
                    let end = r.skip_element();
                    let close = text[..end].rfind('<').unwrap_or(end);
                    return splice(text, vec![(close..close, el.to_owned())]);
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { .. } => depth -= 1,
            Token::Text { .. } => {}
        }
    }
    let block = format!("<{p}dataValidations count=\"1\">{el}</{p}dataValidations>");
    insert_top_level(text, &AFTER_MERGE_CELLS[3..], &block)
}

/// A sheet part with each `<dataValidations>` counting its children, and
/// gone when it has none: in the x14 form its `<ext>` goes too, and the
/// `<extLst>` when that was its only extension.
fn tidy_validations(text: &str) -> String {
    let mut r = Reader::new(text);
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut block: Option<(Span<usize>, usize)> = None;
    let mut ext_empty = false;
    let (mut exts, mut emptied) = (0, Vec::new());
    let mut edits: Vec<(Span<usize>, String)> = Vec::new();
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if tag.name == "dataValidation"
                    && let Some(b) = block.as_mut()
                {
                    b.1 += 1;
                }
                if tag.empty {
                    if tag.name == "dataValidations" {
                        edits.push((tag.span.clone(), String::new()));
                    }
                    continue;
                }
                if tag.name == "dataValidations" {
                    block = Some((tag.span.clone(), 0));
                }
                stack.push((tag.name.to_owned(), tag.span.start));
            }
            Token::End { name, span } => {
                let Some((_, start)) = stack.pop() else {
                    continue;
                };
                let parent = stack.last().map(|(p, _)| p.as_str());
                match name {
                    "dataValidations" => {
                        if let Some((open, n)) = block.take() {
                            if n > 0 {
                                let head =
                                    xml::set_attr(&text[open.clone()], "count", &n.to_string());
                                edits.push((open, head));
                            } else if parent == Some("ext") {
                                ext_empty = true;
                            } else {
                                edits.push((start..span.end, String::new()));
                            }
                        }
                    }
                    "ext" if parent == Some("extLst") => {
                        exts += 1;
                        if std::mem::take(&mut ext_empty) {
                            emptied.push(start..span.end);
                        }
                    }
                    "extLst" => {
                        if exts > 0 && emptied.len() == exts {
                            edits.push((start..span.end, String::new()));
                            emptied.clear();
                        } else {
                            edits.extend(emptied.drain(..).map(|s| (s, String::new())));
                        }
                        exts = 0;
                    }
                    _ => {}
                }
            }
            Token::Text { .. } => {}
        }
    }
    splice(text, edits)
}

/// Unprefixed markup with every element name given `prefix`.
fn with_prefix(markup: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return markup.to_owned();
    }
    markup
        .replace("</", "\u{0}")
        .replace('<', &format!("<{prefix}"))
        .replace('\u{0}', &format!("</{prefix}"))
}

/// A sheet part with `xml` put among the worksheet's children before the
/// first of `before`, or at its end.
fn insert_top_level(text: &str, before: &[&str], xml: &str) -> String {
    let mut r = Reader::new(text);
    let mut depth = 0;
    let mut at: Option<usize> = None;
    let mut end: Option<usize> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 1 && at.is_none() && before.contains(&tag.name) {
                    at = Some(tag.span.start);
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { span, .. } => {
                depth -= 1;
                if depth == 0 {
                    end = Some(span.start);
                }
            }
            Token::Text { .. } => {}
        }
    }
    match at.or(end) {
        Some(p) => splice(text, vec![(p..p, xml.to_owned())]),
        None => text.to_owned(),
    }
}

/// A sheet part with a `<mergeCell ref>` added, `<mergeCells count>` kept
/// right or made where the schema puts it.
fn add_merge(text: &str, range: &str) -> String {
    let mut r = Reader::new(text);
    let mut depth = 0;
    let mut prefix = String::new();
    let mut before: Option<usize> = None;
    let mut end_of_sheet: Option<usize> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) => {
                if depth == 0 {
                    prefix = xml::prefix(tag.qname).to_owned();
                }
                if depth == 1 && tag.name == "mergeCells" {
                    let count = tag
                        .attr("count")
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(0)
                        + 1;
                    let open = xml::set_attr(&text[tag.span.clone()], "count", &count.to_string());
                    let item = format!("<{prefix}mergeCell ref=\"{range}\"/>");
                    if tag.empty {
                        let o = open.trim_end_matches("/>").trim_end();
                        return splice(
                            text,
                            vec![(tag.span.clone(), format!("{o}>{item}</{prefix}mergeCells>"))],
                        );
                    }
                    let end = r.skip_element();
                    let close = text[..end].rfind('<').unwrap_or(end);
                    return splice(text, vec![(tag.span.clone(), open), (close..close, item)]);
                }
                if depth == 1 && before.is_none() && AFTER_MERGE_CELLS.contains(&tag.name) {
                    before = Some(tag.span.start);
                }
                if !tag.empty {
                    depth += 1;
                }
            }
            Token::End { span, .. } => {
                depth -= 1;
                if depth == 0 {
                    end_of_sheet = Some(span.start);
                }
            }
            Token::Text { .. } => {}
        }
    }
    let Some(at) = before.or(end_of_sheet) else {
        return text.to_owned();
    };
    let block = format!(
        "<{prefix}mergeCells count=\"1\"><{prefix}mergeCell ref=\"{range}\"/></{prefix}mergeCells>"
    );
    splice(text, vec![(at..at, block)])
}

/// A sheet part without the `<mergeCell>` of `range`; `<mergeCells>` goes
/// when it was the last one, since the schema wants at least one.
fn remove_merge(text: &str, range: Range) -> String {
    let mut r = Reader::new(text);
    let mut block: Option<(Span<usize>, Span<usize>, u32)> = None;
    let mut item: Option<Span<usize>> = None;
    let mut items = 0;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "mergeCells" && !tag.empty => {
                block = Some((tag.span.clone(), 0..0, 0));
            }
            Token::Start(tag) if tag.name == "mergeCell" && block.is_some() => {
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                items += 1;
                if tag.attr("ref").and_then(|v| Range::parse(&v)) == Some(range) {
                    item = Some(tag.span.start..end);
                }
            }
            Token::End {
                name: "mergeCells",
                span,
            } => {
                if let Some(b) = block.as_mut() {
                    b.1 = span;
                    b.2 = items;
                }
                break;
            }
            _ => {}
        }
    }
    let (Some((open, close, n)), Some(item)) = (block, item) else {
        return text.to_owned();
    };
    if n <= 1 {
        return splice(text, vec![(open.start..close.end, String::new())]);
    }
    let new_open = xml::set_attr(&text[open.clone()], "count", &(n - 1).to_string());
    splice(text, vec![(open, new_open), (item, String::new())])
}

/// Applies non-overlapping replacements to a text.
fn splice(text: &str, mut edits: Vec<(Span<usize>, String)>) -> String {
    edits.sort_by_key(|(s, _)| std::cmp::Reverse((s.start, s.end)));
    let mut out = text.to_owned();
    for (span, with) in edits {
        out.replace_range(span, &with);
    }
    out
}

/// The `<c>` element for an entry.
fn cell_xml(prefix: &str, at: CellRef, style: u32, input: &Input) -> String {
    let s = if style == 0 {
        String::new()
    } else {
        format!(" s=\"{style}\"")
    };
    let p = prefix;
    match input {
        Input::Clear => format!("<{p}c r=\"{at}\"{s}/>"),
        Input::Number(v, _) => format!(
            "<{p}c r=\"{at}\"{s}><{p}v>{}</{p}v></{p}c>",
            number_text(*v)
        ),
        Input::Bool(b) => format!(
            "<{p}c r=\"{at}\"{s} t=\"b\"><{p}v>{}</{p}v></{p}c>",
            u8::from(*b)
        ),
        Input::Error(e) => format!(
            "<{p}c r=\"{at}\"{s} t=\"e\"><{p}v>{}</{p}v></{p}c>",
            xml::escape(e)
        ),
        Input::Text(t) => {
            let space = if t.starts_with(char::is_whitespace) || t.ends_with(char::is_whitespace) {
                " xml:space=\"preserve\""
            } else {
                ""
            };
            format!(
                "<{p}c r=\"{at}\"{s} t=\"inlineStr\"><{p}is><{p}t{space}>{}</{p}t></{p}is></{p}c>",
                xml::escape(t)
            )
        }
        // No cached value: Excel computes it on open (fullCalcOnLoad).
        Input::Formula(f) => format!("<{p}c r=\"{at}\"{s}><{p}f>{}</{p}f></{p}c>", xml::escape(f)),
    }
}

/// A formula cell's `<c>` with its stored result replaced (or removed when
/// `value` is `None`), the `<f>` and every attribute but `t` kept.
fn with_cached(c_xml: &str, prefix: &str, value: Option<&Value>) -> String {
    let mut r = Reader::new(c_xml);
    let Some(Token::Start(open)) = r.next_token() else {
        return c_xml.to_owned();
    };
    if open.empty {
        return c_xml.to_owned();
    }
    let mut v_span: Option<Span<usize>> = None;
    let mut f_end: Option<usize> = None;
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "v" => {
                let end = if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                };
                v_span = Some(tag.span.start..end);
            }
            Token::Start(tag) if tag.name == "f" => {
                f_end = Some(if tag.empty {
                    tag.span.end
                } else {
                    r.skip_element()
                });
            }
            Token::Start(tag) if !tag.empty => {
                r.skip_element();
            }
            _ => {}
        }
    }
    let (ty, body) = match value {
        None => (None, None),
        Some(Value::Empty) => (Some("str"), Some(String::new())),
        Some(Value::Number(n)) => (None, Some(number_text(*n))),
        Some(Value::Text(t)) => (Some("str"), Some(xml::escape(t))),
        Some(Value::Bool(b)) => (Some("b"), Some(u8::from(*b).to_string())),
        Some(Value::Error(e)) => (Some("e"), Some(xml::escape(e))),
    };
    let open_text = &c_xml[open.span.clone()];
    let new_open = match ty {
        Some(t) => xml::set_attr(open_text, "t", t),
        None => xml::remove_attr(open_text, "t"),
    };
    let v = body.map_or(String::new(), |b| format!("<{prefix}v>{b}</{prefix}v>"));
    let mut out = String::with_capacity(c_xml.len() + 16);
    out.push_str(&new_open);
    match (v_span, f_end) {
        (Some(vs), _) => {
            out.push_str(&c_xml[open.span.end..vs.start]);
            out.push_str(&v);
            out.push_str(&c_xml[vs.end..]);
        }
        (None, Some(fe)) => {
            out.push_str(&c_xml[open.span.end..fe]);
            out.push_str(&v);
            out.push_str(&c_xml[fe..]);
        }
        (None, None) => out.push_str(&c_xml[open.span.end..]),
    }
    out
}

/// A number as Excel writes it: the shortest text that reads back the same.
fn number_text(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('e') || s.len() > 17 {
        format!("{v:E}")
    } else {
        s
    }
}

/// A shared group's member turned into the group's first cell.
fn rewrite_shared_master(c_xml: &str, prefix: &str, si: u32, range: Range, text: &str) -> String {
    let mut r = Reader::new(c_xml);
    while let Some(t) = r.next_token() {
        if let Token::Start(tag) = t
            && tag.name == "f"
        {
            let end = if tag.empty {
                tag.span.end
            } else {
                r.skip_element()
            };
            let f = format!(
                "<{prefix}f t=\"shared\" ref=\"{range}\" si=\"{si}\">{}</{prefix}f>",
                xml::escape(text)
            );
            return format!("{}{}{}", &c_xml[..tag.span.start], f, &c_xml[end..]);
        }
    }
    c_xml.to_owned()
}

/// The splices that put a new `<c>` in its row, making the row if needed.
fn insert_cell(
    text: &str,
    model: &Sheet,
    at: CellRef,
    c: String,
) -> Result<Vec<(Span<usize>, String)>> {
    let p = &model.prefix;
    if let Some(row) = model.rows.get(&at.row) {
        let start_tag = &text[row.start.clone()];
        let widened = widen_spans(start_tag, at.col);
        let Some(_) = &row.end else {
            // `<row …/>` opens up to hold the cell.
            let open = widened
                .trim_end_matches('>')
                .trim_end_matches('/')
                .trim_end();
            return Ok(vec![(row.start.clone(), format!("{open}>{c}</{p}row>"))]);
        };
        let before = model
            .cells
            .range(CellRef::new(at.row, 0)..at)
            .next_back()
            .map(|(_, cell)| cell.span.end);
        let mut out = vec![(
            before.unwrap_or(row.start.end)..before.unwrap_or(row.start.end),
            c,
        )];
        if widened != start_tag {
            out.push((row.start.clone(), widened));
        }
        return Ok(out);
    }
    let new_row = format!("<{p}row r=\"{}\">{c}</{p}row>", at.row + 1);
    let Some((sd_start, sd_end)) = &model.sheet_data else {
        return Err(Error::Refused("the sheet part has no sheetData".into()));
    };
    let Some(_) = sd_end else {
        // `<sheetData/>` opens up.
        let tag = &text[sd_start.clone()];
        let open = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
        return Ok(vec![(
            sd_start.clone(),
            format!("{open}>{new_row}</{p}sheetData>"),
        )]);
    };
    let after = model
        .rows
        .range(..at.row)
        .next_back()
        .map(|(_, r)| r.end.as_ref().map_or(r.start.end, |e| e.end));
    let at_pos = after.unwrap_or(sd_start.end);
    Ok(vec![(at_pos..at_pos, new_row)])
}

/// A row's `spans` hint widened to a column, when the row has one.
fn widen_spans(row_tag: &str, col: u32) -> String {
    let tag = Reader::new(row_tag).next_token();
    let Some(Token::Start(tag)) = tag else {
        return row_tag.to_owned();
    };
    let Some(spans) = tag.attr("spans") else {
        return row_tag.to_owned();
    };
    let Some((a, b)) = spans.split_once(':') else {
        return row_tag.to_owned();
    };
    let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) else {
        return row_tag.to_owned();
    };
    let c = (col + 1).min(MAX_COL);
    if (a..=b).contains(&c) {
        return row_tag.to_owned();
    }
    xml::set_attr(row_tag, "spans", &format!("{}:{}", a.min(c), b.max(c)))
}

mod links;
mod names;
mod notes;
mod sheet_ops;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formats_found_or_added() {
        // A built-in code is its id; a new one goes past the sheet's own.
        let x = r#"<styleSheet><numFmts count="1"><numFmt numFmtId="170" formatCode="0.0"/></numFmts><cellXfs/></styleSheet>"#;
        assert_eq!(with_num_fmt(x, "0.00E+00").1, 11);
        assert_eq!(with_num_fmt(x, "0.0").1, 170);
        let (t, id) = with_num_fmt(x, "#,##0.00 \"₺\"");
        assert_eq!(id, 171);
        assert!(t.contains(r#"<numFmts count="2">"#), "{t}");
        assert!(
            t.contains(r##"formatCode="#,##0.00 &quot;₺&quot;""##),
            "{t}"
        );
        // None yet: the list made first in the sheet, prefixed as it is.
        let x = r#"<x:styleSheet xmlns:x="s"><x:fonts/></x:styleSheet>"#;
        let (t, id) = with_num_fmt(x, "0.000");
        assert_eq!(id, 164);
        assert!(
            t.starts_with(r#"<x:styleSheet xmlns:x="s"><x:numFmts count="1"><x:numFmt numFmtId="164" formatCode="0.000"/></x:numFmts><x:fonts/>"#),
            "{t}"
        );
    }

    #[test]
    fn entries_read_as_excel_reads_them() {
        assert_eq!(
            Input::parse("=SUM(A1:A3)", false),
            Input::Formula("SUM(A1:A3)".into())
        );
        assert_eq!(Input::parse("12.5", false), Input::Number(12.5, None));
        assert_eq!(
            Input::parse("-1,234", false),
            Input::Number(-1234.0, Some(3))
        );
        assert_eq!(
            Input::parse("1,234.5", false),
            Input::Number(1234.5, Some(4))
        );
        assert_eq!(Input::parse("12,34", false), Input::Text("12,34".into()));
        assert_eq!(Input::parse("50%", false), Input::Number(0.5, Some(9)));
        assert_eq!(Input::parse("'007", false), Input::Text("007".into()));
        assert_eq!(Input::parse("true", false), Input::Bool(true));
        assert_eq!(Input::parse("#N/A", false), Input::Error("#N/A".into()));
        assert_eq!(
            Input::parse("2023-03-15", false),
            Input::Number(45_000.0, Some(14))
        );
        assert_eq!(Input::parse("12:00", false), Input::Number(0.5, Some(21)));
        assert_eq!(Input::parse("=", false), Input::Text("=".into()));
        assert_eq!(Input::parse("", false), Input::Clear);
        assert_eq!(Input::parse("1e3", false), Input::Number(1000.0, None));
        assert_eq!(Input::parse("abc", false), Input::Text("abc".into()));
    }

    #[test]
    fn cell_xml_forms() {
        let at = CellRef::new(1, 1);
        assert_eq!(
            cell_xml("", at, 0, &Input::Number(2.0, None)),
            "<c r=\"B2\"><v>2</v></c>"
        );
        assert_eq!(
            cell_xml("x:", at, 3, &Input::Text(" a<b".into())),
            "<x:c r=\"B2\" s=\"3\" t=\"inlineStr\"><x:is><x:t xml:space=\"preserve\"> a&lt;b</x:t></x:is></x:c>"
        );
        assert_eq!(
            cell_xml("", at, 0, &Input::Formula("A1&\"x\"".into())),
            "<c r=\"B2\"><f>A1&amp;&quot;x&quot;</f></c>"
        );
    }

    #[test]
    fn cached_values_replaced() {
        let c = r#"<c r="D2" s="3" t="n"><f aca="false">B2+C2</f><v>2400</v></c>"#;
        assert_eq!(
            with_cached(c, "", Some(&Value::Number(2500.0))),
            r#"<c r="D2" s="3"><f aca="false">B2+C2</f><v>2500</v></c>"#
        );
        assert_eq!(
            with_cached(c, "", None),
            r#"<c r="D2" s="3"><f aca="false">B2+C2</f></c>"#
        );
        assert_eq!(
            with_cached(
                "<c r=\"A1\"><f>\"a\"</f></c>",
                "",
                Some(&Value::Text("a&b".into()))
            ),
            "<c r=\"A1\" t=\"str\"><f>\"a\"</f><v>a&amp;b</v></c>"
        );
    }

    #[test]
    fn merges_written_and_removed() {
        let sheet = r#"<worksheet><sheetData/><pageMargins left="0.7"/></worksheet>"#;
        let one = add_merge(sheet, "A1:B2");
        assert_eq!(
            one,
            r#"<worksheet><sheetData/><mergeCells count="1"><mergeCell ref="A1:B2"/></mergeCells><pageMargins left="0.7"/></worksheet>"#
        );
        let two = add_merge(&one, "D4:E4");
        assert!(two.contains(r#"<mergeCells count="2"><mergeCell ref="A1:B2"/><mergeCell ref="D4:E4"/></mergeCells>"#), "{two}");
        let back = remove_merge(&two, Range::parse("D4:E4").unwrap());
        assert_eq!(back, one);
        assert_eq!(remove_merge(&one, Range::parse("A1:B2").unwrap()), sheet);
        let bare = r#"<x:worksheet><x:sheetData/></x:worksheet>"#;
        assert_eq!(
            add_merge(bare, "C1:C3"),
            r#"<x:worksheet><x:sheetData/><x:mergeCells count="1"><x:mergeCell ref="C1:C3"/></x:mergeCells></x:worksheet>"#
        );
        assert_eq!(
            with_alignment(r#"<xf numFmtId="0"/>"#, "horizontal", "center"),
            r#"<xf numFmtId="0" applyAlignment="1"><alignment horizontal="center"/></xf>"#
        );
    }

    #[test]
    fn wrap_in_xf() {
        assert_eq!(
            with_wrap(r#"<xf numFmtId="0" fontId="1"/>"#, true),
            r#"<xf numFmtId="0" fontId="1" applyAlignment="1"><alignment wrapText="1"/></xf>"#
        );
        assert_eq!(
            with_wrap(
                r#"<x:xf numFmtId="0"><x:alignment horizontal="center"/><x:protection/></x:xf>"#,
                true
            ),
            r#"<x:xf numFmtId="0" applyAlignment="1"><x:alignment horizontal="center" wrapText="1"/><x:protection/></x:xf>"#
        );
        assert_eq!(
            with_wrap(r#"<xf><protection locked="0"/></xf>"#, false),
            r#"<xf applyAlignment="1"><alignment wrapText="0"/><protection locked="0"/></xf>"#
        );
    }

    #[test]
    fn column_widths() {
        let none = "<worksheet><sheetFormatPr/><sheetData/></worksheet>";
        assert_eq!(
            col_width_text(none, 2, 20.5),
            r#"<worksheet><sheetFormatPr/><cols><col min="2" max="2" width="20.5" customWidth="1"/></cols><sheetData/></worksheet>"#
        );
        let some = r#"<x:worksheet><x:cols><x:col min="1" max="4" width="9" style="3"/><x:col min="8" max="8" width="5"/></x:cols><x:sheetData/></x:worksheet>"#;
        assert_eq!(
            col_width_text(some, 2, 12.0),
            r#"<x:worksheet><x:cols><x:col min="1" max="1" width="9" style="3"/><x:col min="2" max="2" width="12" style="3" customWidth="1"/><x:col min="3" max="4" width="9" style="3"/><x:col min="8" max="8" width="5"/></x:cols><x:sheetData/></x:worksheet>"#
        );
        assert_eq!(
            col_width_text(some, 6, 7.0),
            r#"<x:worksheet><x:cols><x:col min="1" max="4" width="9" style="3"/><x:col min="6" max="6" width="7" customWidth="1"/><x:col min="8" max="8" width="5"/></x:cols><x:sheetData/></x:worksheet>"#
        );
    }

    #[test]
    fn spans_widen() {
        assert_eq!(
            widen_spans("<row r=\"1\" spans=\"1:3\">", 5),
            "<row r=\"1\" spans=\"1:6\">"
        );
        assert_eq!(
            widen_spans("<row r=\"1\" spans=\"1:3\">", 1),
            "<row r=\"1\" spans=\"1:3\">"
        );
        assert_eq!(widen_spans("<row r=\"1\">", 9), "<row r=\"1\">");
    }

    #[test]
    fn validation_blocks_tidied() {
        let x14 = r#"<worksheet><sheetData/><dataValidations count="5"><dataValidation sqref="A1"/></dataValidations><pageMargins/><extLst><ext uri="a"><x14:dataValidations count="1"></x14:dataValidations></ext></extLst></worksheet>"#;
        assert_eq!(
            tidy_validations(x14),
            r#"<worksheet><sheetData/><dataValidations count="1"><dataValidation sqref="A1"/></dataValidations><pageMargins/></worksheet>"#
        );
        let two = r#"<worksheet><dataValidations count="1"></dataValidations><extLst><ext uri="a"><x14:dataValidations/></ext><ext uri="b"><y/></ext></extLst></worksheet>"#;
        assert_eq!(
            tidy_validations(two),
            r#"<worksheet><extLst><ext uri="a"></ext><ext uri="b"><y/></ext></extLst></worksheet>"#
        );
        let added = add_validation(
            "<worksheet><sheetData/><hyperlinks/></worksheet>",
            "",
            "<dataValidation/>",
        );
        assert_eq!(
            added,
            r#"<worksheet><sheetData/><dataValidations count="1"><dataValidation/></dataValidations><hyperlinks/></worksheet>"#
        );
    }
}
