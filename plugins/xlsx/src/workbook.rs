//! A workbook opened as itself: the package's parts read on demand, cells
//! shown through their formats, and edits written into the one `<c>` they
//! touch.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::ops::Range as Span;

use crate::calc::{self, Engine};
use crate::cellref::{CellRef, MAX_COL, MAX_ROW, Range};
use crate::formula::Op;
use crate::numfmt;
use crate::package::{Package, PackageError};
use crate::rels::{self, Rel, kind};
use crate::sheet::{self, Cell, FormulaKind, Sheet, Value};
use crate::structure;
use crate::styles::{self, CellStyle, Styles};
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
}

/// What an edit changes, kept whole for undo: the package's bytes are
/// shared, so a snapshot costs the edited texts.
#[derive(Clone)]
struct Snapshot {
    pkg: Package,
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
        if self.engine.is_some() {
            return Ok(());
        }
        let missing = self.loaded[&idx]
            .1
            .cells
            .values()
            .any(|c| c.formula.is_some() && c.value == Value::Empty);
        if missing {
            self.ensure_engine()?;
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
        self.load(idx)?;
        self.ensure_engine()?;
        self.flush()?;
        Ok(match self.engine.as_mut() {
            Some(Ok(e)) => e.eval_scratch(idx, formula.trim_start_matches('=')),
            _ => None,
        })
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
        self.set_input(idx, at, input)
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
        if let Some(&s) = self.derived_styles.get(&(style, fmt)) {
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
        // The start tag of the copied `<xf>` gets the format.
        let tag_end = src.find('>').map_or(src.len(), |p| p + 1);
        let mut head = xml::set_attr(&src[..tag_end], "numFmtId", &fmt.to_string());
        head = xml::set_attr(&head, "applyNumberFormat", "1");
        let copy = format!("{head}{}", &src[tag_end..]);
        let new_open = xml::set_attr(&xml_text[open.clone()], "count", &(count + 1).to_string());
        let out = splice(
            &xml_text,
            vec![(open, new_open), (close.start..close.start, copy)],
        );
        self.styles_xml = Some(out.clone());
        let fresh = styles::parse(&out, &self.theme);
        self.styles = fresh;
        self.derived_styles.insert((style, fmt), count);
        Ok(count)
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
    let set = |tag: &str| -> String {
        let t = xml::set_attr(tag, "width", &w);
        xml::set_attr(&t, "customWidth", "1")
    };
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
