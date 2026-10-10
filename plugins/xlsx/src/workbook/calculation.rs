//! When formulas are computed (`<calcPr calcMode iterate iterateCount
//! iterateDelta>`): Automatic, Automatic except Data Tables, Manual; and
//! circular references, found, or computed over and over.

use super::*;
use kalem_viewer::{CalcMode, CalcOptions};

impl Workbook {
    /// The workbook's calculation settings, as its `<calcPr>` says.
    pub fn calc_options(&self) -> CalcOptions {
        let mut o = CalcOptions::default();
        let mut r = Reader::new(&self.workbook_xml);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            if tag.name != "calcPr" {
                continue;
            }
            o.mode = match tag.attr("calcMode").as_deref() {
                Some("manual") => CalcMode::Manual,
                Some("autoNoTable") => CalcMode::AutomaticExceptTables,
                _ => CalcMode::Automatic,
            };
            o.iterate = matches!(tag.attr("iterate").as_deref(), Some("1" | "true"));
            if let Some(n) = tag.attr("iterateCount").and_then(|v| v.parse().ok()) {
                o.max_iterations = n;
            }
            if let Some(d) = tag.attr("iterateDelta").and_then(|v| v.parse().ok()) {
                o.max_change = d;
            }
            break;
        }
        o
    }

    /// Sets the calculation settings, as one undo step.
    pub fn set_calc_options(&mut self, o: CalcOptions) -> Result<()> {
        if !(1..=32767).contains(&o.max_iterations) || o.max_change.is_nan() || o.max_change < 0.0 {
            return Err(Error::Refused(
                "Iterations are 1 to 32,767, the change at least 0".into(),
            ));
        }
        let snapshot = self.snapshot();
        // A `<calcPr>` there to write into.
        self.set_full_calc_on_load();
        let text = self.workbook_xml.clone();
        let mut r = Reader::new(&text);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            if tag.name != "calcPr" {
                continue;
            }
            let mut el = text[tag.span.clone()].to_owned();
            el = match o.mode {
                CalcMode::Automatic => xml::remove_attr(&el, "calcMode"),
                CalcMode::AutomaticExceptTables => xml::set_attr(&el, "calcMode", "autoNoTable"),
                CalcMode::Manual => xml::set_attr(&el, "calcMode", "manual"),
            };
            el = if o.iterate {
                let el = xml::set_attr(&el, "iterate", "1");
                let el = xml::set_attr(&el, "iterateCount", &o.max_iterations.to_string());
                xml::set_attr(&el, "iterateDelta", &o.max_change.to_string())
            } else {
                let el = xml::remove_attr(&el, "iterate");
                let el = xml::remove_attr(&el, "iterateCount");
                xml::remove_attr(&el, "iterateDelta")
            };
            self.workbook_xml = splice(&text, vec![(tag.span.clone(), el)]);
            break;
        }
        // Saved with the rest, though only the workbook part changed.
        let part = self.workbook_part.clone();
        self.pkg
            .set_part(&part, self.workbook_xml.clone().into_bytes());
        self.push_undo(snapshot);
        self.redo.clear();
        // Computed again under the new settings.
        self.recalculate()
    }

    /// The engine's results for `cells`: formulas held as values by the
    /// last iterative calculation given back first; circular references
    /// computed over and over when the workbook asks it. `None` without
    /// an engine.
    pub(crate) fn evaluate_all(
        &mut self,
        cells: &[(usize, CellRef)],
    ) -> Option<HashMap<(usize, CellRef), Value>> {
        let o = self.calc_options();
        let back = std::mem::take(&mut self.circ_overrides);
        let Some(Ok(engine)) = self.engine.as_mut() else {
            return None;
        };
        for (s, at, f) in &back {
            let _ = engine.set(*s, *at, Some(f), &Value::Empty);
        }
        let mut after = engine.evaluate(cells).clone();
        if !o.iterate {
            return Some(after);
        }
        let circ: Vec<(usize, CellRef, String, f64)> = after
            .iter()
            .filter(|(_, v)| matches!(v, Value::Error(e) if e == "#CIRC!"))
            .filter_map(|(&(s, at), _)| {
                let c = self.loaded.get(&s)?.1.cells.get(&at)?;
                let f = c.formula.as_ref()?.text.clone();
                let start = match c.value {
                    Value::Number(n) => n,
                    _ => 0.0,
                };
                Some((s, at, f, start))
            })
            .collect();
        if circ.is_empty() {
            return Some(after);
        }
        let start: Vec<f64> = circ.iter().map(|c| c.3).collect();
        let circ: Vec<(usize, CellRef, String)> =
            circ.into_iter().map(|(s, a, f, _)| (s, a, f)).collect();
        let Some(Ok(engine)) = self.engine.as_mut() else {
            return Some(after);
        };
        let vals = engine.iterate(&circ, start, o.max_iterations, o.max_change);
        after = engine.evaluate(cells).clone();
        for ((s, at, _), v) in circ.iter().zip(vals) {
            after.insert((*s, *at), Value::Number(v));
        }
        self.circ_overrides = circ;
        Some(after)
    }

    /// The cells whose formulas refer to themselves round a circle, as
    /// the engine last computed them (none when they are iterated).
    pub fn circular_references(&mut self) -> Vec<(usize, CellRef)> {
        if self.ensure_engine().is_err() {
            return Vec::new();
        }
        let _ = self.flush();
        let mut out: Vec<(usize, CellRef)> = self
            .computed
            .iter()
            .filter(|(_, v)| matches!(v, Value::Error(e) if e == "#CIRC!"))
            .map(|(k, _)| *k)
            .collect();
        out.sort();
        out
    }

    /// After a change has been computed: data tables computed again when
    /// the workbook computes them automatically.
    pub(crate) fn after_change(&mut self) -> Result<()> {
        if self.calc_options().mode == CalcMode::Automatic
            && self.loaded.values().any(|e| !e.1.data_tables.is_empty())
        {
            self.refill_data_tables()?;
        }
        Ok(())
    }
}
