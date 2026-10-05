//! Slicers (Excel 2010's, `x14`, and 2013's for tables, `x15`): a field's
//! items as buttons over the cells filtering a pivot table (its field's
//! hidden items) or a table (its AutoFilter on the column). Written as
//! Excel writes them: a slicer cache part named by the workbook's
//! extension list and a defined name, a slicers part named by the sheet's
//! extension list, and an anchor in the sheet's drawing.

use super::*;

const X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const X15: &str = "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main";
const SLICER_REL: &str = "http://schemas.microsoft.com/office/2007/relationships/slicer";
const CACHE_REL: &str = "http://schemas.microsoft.com/office/2007/relationships/slicerCache";
const WB_PIVOT_EXT: &str = "{BBE1A952-AA13-448e-AADC-164F8A28A991}";
const WB_TABLE_EXT: &str = "{46BE6895-7355-4a93-B00E-2C351335B9C9}";
const SHEET_PIVOT_EXT: &str = "{A8765BA9-456A-4dab-B4F3-ACF838C121DE}";
const SHEET_TABLE_EXT: &str = "{3A4CF648-6AED-40f4-86FF-DC5316D8AED3}";

/// A slicer cache as Kalem reads it.
#[derive(Debug, Clone, Default)]
struct Cache {
    part: String,
    name: String,
    source: String,
    pivot: Option<String>,
    table: Option<(u32, u32)>,
}

/// A slicer of a sheet: its name, caption, cache and anchor.
#[derive(Debug, Clone)]
struct SlicerDef {
    name: String,
    caption: String,
    cache: String,
    part: String,
}

fn parse_cache(text: &str, part: &str) -> Cache {
    let mut c = Cache {
        part: part.to_owned(),
        ..Cache::default()
    };
    let mut r = Reader::new(text);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        match tag.name {
            "slicerCacheDefinition" => {
                c.name = tag.attr("name").unwrap_or_default().into_owned();
                c.source = tag.attr("sourceName").unwrap_or_default().into_owned();
            }
            "pivotTable" => {
                c.pivot
                    .get_or_insert(tag.attr("name").unwrap_or_default().into_owned());
            }
            "tableSlicerCache" => {
                let n = |k: &str| tag.attr(k).and_then(|v| v.parse().ok()).unwrap_or(0);
                c.table = Some((n("tableId"), n("column")));
            }
            _ => {}
        }
    }
    c
}

/// A sheet part's extension list with `ext` (whose `uri` it has) added to
/// the extension of that uri, or made.
fn with_ext_item(
    text: &str,
    p: &str,
    uri: &str,
    ns: (&str, &str),
    list: &str,
    item: &str,
) -> String {
    let (pfx, url) = ns;
    // An extension of the uri there: the item put in its list.
    if let Some(at) = text.find(&format!("uri=\"{uri}\"")) {
        let close = format!("</{pfx}:{list}>");
        if let Some(end) = text[at..].find(&close) {
            let pos = at + end;
            return format!("{}{item}{}", &text[..pos], &text[pos..]);
        }
    }
    let ext = format!(
        "<{p}ext uri=\"{uri}\" xmlns:{pfx}=\"{url}\"><{pfx}:{list}>{item}</{pfx}:{list}></{p}ext>"
    );
    let close_list = format!("</{p}extLst>");
    if let Some(pos) = text.rfind(&close_list) {
        return format!("{}{ext}{}", &text[..pos], &text[pos..]);
    }
    let root_close = text.rfind("</").unwrap_or(text.len());
    format!(
        "{}<{p}extLst>{ext}</{p}extLst>{}",
        &text[..root_close],
        &text[root_close..]
    )
}

impl Workbook {
    fn slicer_caches(&self) -> Vec<Cache> {
        let wb = self.workbook_part.clone();
        self.workbook_rels
            .iter()
            .filter(|r| r.kind == "slicerCache" && !r.external)
            .filter_map(|r| {
                let part = rels::resolve(&wb, &r.target);
                let text = text_of(self.pkg.part(&part).ok()?, &part).ok()?;
                Some(parse_cache(&text, &part))
            })
            .collect()
    }

    fn sheet_slicers(&self, idx: usize) -> Vec<SlicerDef> {
        let mut out = Vec::new();
        let Ok(parts) = self.sheet_parts(idx) else {
            return out;
        };
        for (kind, part) in parts {
            if kind != "slicer" {
                continue;
            }
            let Ok(text) = self
                .pkg
                .part(&part)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
            else {
                continue;
            };
            let mut r = Reader::new(&text);
            while let Some(t) = r.next_token() {
                if let Token::Start(tag) = t
                    && tag.name == "slicer"
                {
                    out.push(SlicerDef {
                        name: tag.attr("name").unwrap_or_default().into_owned(),
                        caption: tag
                            .attr("caption")
                            .or_else(|| tag.attr("name"))
                            .unwrap_or_default()
                            .into_owned(),
                        cache: tag.attr("cache").unwrap_or_default().into_owned(),
                        part: part.clone(),
                    });
                }
            }
        }
        out
    }

    /// Where a slicer stands: the cells its anchor in the sheet's drawing
    /// covers.
    fn slicer_anchor(&self, idx: usize, name: &str) -> Option<([u32; 4], std::ops::Range<usize>)> {
        let drawing = self.sheet_drawing(idx)?;
        let text = text_of(self.pkg.part(&drawing).ok()?, &drawing).ok()?;
        let mark = format!("name=\"{}\"", xml::escape(name));
        let mut r = Reader::new(&text);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            if !matches!(tag.name, "twoCellAnchor" | "oneCellAnchor") || tag.empty {
                continue;
            }
            let start = tag.span.start;
            let end = r.skip_element();
            let el = &text[start..end];
            if !(el.contains("slicer") && el.contains(&mark)) {
                continue;
            }
            let num = |which: &str, what: &str| -> u32 {
                el.split(&format!("{which}>"))
                    .nth(1)
                    .and_then(|s| s.split(&format!("{what}>")).nth(1))
                    .and_then(|s| s.split('<').next())
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0)
            };
            let (r0, c0) = (num("from", "row"), num("from", "col"));
            let (r1, c1) = (num("to", "row"), num("to", "col"));
            return Some((
                [
                    r0,
                    c0,
                    r1.saturating_sub(1).max(r0),
                    c1.saturating_sub(1).max(c0),
                ],
                start..end,
            ));
        }
        None
    }

    /// A slicer's items and selection: a pivot table's field's items (the
    /// hidden ones not selected), or a table column's values (those its
    /// filter lets through selected).
    fn slicer_items(&mut self, cache: &Cache) -> Vec<(String, bool)> {
        if let Some(pivot) = &cache.pivot {
            for (i, part) in self.pivot_tables() {
                let Ok((t, _, _, _, src, layout)) = self.pivot_parts(&part) else {
                    continue;
                };
                if !t.name.eq_ignore_ascii_case(pivot) {
                    continue;
                }
                let Some(f) = src
                    .names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(&cache.source))
                else {
                    return Vec::new();
                };
                let _ = i;
                let mut free = layout.clone();
                free.filters.retain(|x| x.field as usize != f);
                let mut l = free.clone();
                if !l.rows.contains(&f) && !l.cols.contains(&f) {
                    // A field not on the table: its items from the source.
                    l.rows = vec![f];
                    l.cols.clear();
                }
                let hidden: Vec<String> = layout
                    .filters
                    .iter()
                    .filter(|x| {
                        x.field as usize == f && x.kind == kalem_viewer::PivotFilterKind::Items
                    })
                    .flat_map(|x| x.hidden.iter().map(|h| h.to_lowercase()))
                    .collect();
                let Ok(p) = pivot::compute(&src, &l) else {
                    return Vec::new();
                };
                let Some(it) = p.items.get(&f) else {
                    return Vec::new();
                };
                return it
                    .order
                    .iter()
                    .map(|x| {
                        let label = it.shared[*x].1.clone();
                        let on = !hidden.contains(&label.to_lowercase());
                        (
                            if label.is_empty() {
                                "(blank)".into()
                            } else {
                                label
                            },
                            on,
                        )
                    })
                    .collect();
            }
            return Vec::new();
        }
        let Some((table_id, column)) = cache.table else {
            return Vec::new();
        };
        let Some(t) = self
            .tables()
            .unwrap_or_default()
            .into_iter()
            .find(|t| t.id == table_id)
        else {
            return Vec::new();
        };
        let col = t.range.start.col + column.saturating_sub(1);
        let first = t.range.start.row + u32::from(t.header);
        let last = t.range.end.row - u32::from(t.totals);
        let mut values: Vec<String> = Vec::new();
        for row in first..=last {
            let v = self
                .display(t.sheet, CellRef::new(row, col))
                .unwrap_or_default();
            if !values.contains(&v) {
                values.push(v);
            }
        }
        values.sort_by_key(|v| v.to_lowercase());
        let kept = self.table_filter_values(&t.part, column - 1);
        values
            .into_iter()
            .map(|v| {
                let on = kept
                    .as_ref()
                    .is_none_or(|k| k.iter().any(|x| x.eq_ignore_ascii_case(&v)));
                (if v.is_empty() { "(blank)".into() } else { v }, on)
            })
            .collect()
    }

    /// The values a table's AutoFilter lets through in column `col_id`, or
    /// `None` when that column is not filtered.
    fn table_filter_values(&self, part: &str, col_id: u32) -> Option<Vec<String>> {
        let text = text_of(self.pkg.part(part).ok()?, part).ok()?;
        let mut r = Reader::new(&text);
        let mut in_col = false;
        let mut out: Option<Vec<String>> = None;
        while let Some(t) = r.next_token() {
            match t {
                Token::Start(tag) if tag.name == "filterColumn" => {
                    in_col = tag.attr("colId").and_then(|v| v.parse::<u32>().ok()) == Some(col_id);
                    if in_col {
                        out = Some(Vec::new());
                    }
                }
                Token::Start(tag) if in_col && tag.name == "filter" => {
                    if let Some(v) = tag.attr("val") {
                        out.get_or_insert_default().push(v.into_owned());
                    }
                }
                Token::Start(tag) if in_col && tag.name == "filters" => {
                    if tag.attr("blank").is_some_and(|v| v == "1") {
                        out.get_or_insert_default().push(String::new());
                    }
                }
                Token::End {
                    name: "filterColumn",
                    ..
                } => in_col = false,
                _ => {}
            }
        }
        out
    }

    /// A table's column `col_id` filtered to `values` (`None` every row),
    /// its rows hidden as its filters say.
    fn filter_table(
        &mut self,
        table: &super::tables::TableDef,
        col_id: u32,
        values: Option<&[String]>,
    ) -> Result<()> {
        let part = table.part.clone();
        let text = text_of(self.pkg.part(&part)?, &part)?;
        // The table's AutoFilter: each column's values kept, ours changed.
        let mut kept: Vec<(u32, Vec<String>)> = Vec::new();
        for c in 0..table.columns.len() as u32 {
            if c == col_id {
                if let Some(v) = values {
                    kept.push((c, v.to_vec()));
                }
            } else if let Some(v) = self.table_filter_values(&part, c) {
                kept.push((c, v));
            }
        }
        let p = {
            let mut r = Reader::new(&text);
            match r.next_token() {
                Some(Token::Start(tag)) => xml::prefix(tag.qname).to_owned(),
                _ => String::new(),
            }
        };
        let header = table.range.start.row;
        let body_end = table.range.end.row - u32::from(table.totals);
        let af_ref = Range {
            start: CellRef::new(header, table.range.start.col),
            end: CellRef::new(body_end, table.range.end.col),
        };
        let cols: String = kept
            .iter()
            .map(|(c, vals)| {
                let blank = if vals.iter().any(|v| v.is_empty()) {
                    " blank=\"1\""
                } else {
                    ""
                };
                let items: String = vals
                    .iter()
                    .filter(|v| !v.is_empty())
                    .map(|v| format!("<{p}filter val=\"{}\"/>", xml::escape(v)))
                    .collect();
                format!("<{p}filterColumn colId=\"{c}\"><{p}filters{blank}>{items}</{p}filters></{p}filterColumn>")
            })
            .collect();
        let af = if cols.is_empty() {
            format!("<{p}autoFilter ref=\"{af_ref}\"/>")
        } else {
            format!("<{p}autoFilter ref=\"{af_ref}\">{cols}</{p}autoFilter>")
        };
        let new = crate::chart::set_child(&text, &["autoFilter"], &af, &[]);
        self.pkg.set_part(&part, new.into_bytes());
        // The rows hidden: those a filtered column does not keep.
        let mut rows: Vec<(u32, bool)> = Vec::new();
        for row in header + 1..=body_end {
            let mut hide = false;
            for (c, vals) in &kept {
                let v = self.display(table.sheet, CellRef::new(row, table.range.start.col + c))?;
                if !vals.iter().any(|x| x.eq_ignore_ascii_case(&v)) {
                    hide = true;
                }
            }
            rows.push((row, hide));
        }
        self.set_rows_hidden(table.sheet, &rows);
        Ok(())
    }

    /// Sheet `idx`'s slicers.
    pub fn slicers(&mut self, idx: usize) -> Vec<kalem_viewer::Slicer> {
        let caches = self.slicer_caches();
        let mut out = Vec::new();
        for s in self.sheet_slicers(idx) {
            let Some(cache) = caches.iter().find(|c| c.name == s.cache) else {
                continue;
            };
            let anchor = self
                .slicer_anchor(idx, &s.name)
                .map_or([0, 0, 9, 2], |a| a.0);
            let items = self.slicer_items(&cache.clone());
            out.push(kalem_viewer::Slicer {
                name: s.name.clone(),
                caption: s.caption.clone(),
                anchor,
                items,
            });
        }
        out
    }

    /// A slicer of `field` for the pivot table named `pivot` of sheet
    /// `idx` (or for the table named `table`), over `anchor`. One undo
    /// step.
    pub fn insert_slicer(
        &mut self,
        idx: usize,
        pivot: Option<&str>,
        table: Option<&str>,
        field: &str,
        anchor: Range,
    ) -> Result<()> {
        self.load(idx)?;
        let caches = self.slicer_caches();
        let base: String = field
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let cache_name = (0..)
            .map(|n| {
                if n == 0 {
                    format!("Slicer_{base}")
                } else {
                    format!("Slicer_{base}{n}")
                }
            })
            .find(|n| !caches.iter().any(|c| c.name.eq_ignore_ascii_case(n)))
            .expect("a free name");
        let names: Vec<String> = (0..self.sheets.len())
            .flat_map(|i| self.sheet_slicers(i))
            .map(|s| s.name)
            .collect();
        let slicer_name = (0..)
            .map(|n| {
                if n == 0 {
                    field.to_owned()
                } else {
                    format!("{field} {n}")
                }
            })
            .find(|n| !names.iter().any(|x| x.eq_ignore_ascii_case(n)))
            .expect("a free name");
        // What it filters.
        let data = if let Some(pv) = pivot {
            let (sheet_idx, part) = self
                .pivot_tables()
                .into_iter()
                .find(|(_, part)| {
                    self.pkg
                        .part(part)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())
                        .is_some_and(|t| pivot::parse_table(&t).name.eq_ignore_ascii_case(pv))
                })
                .ok_or_else(|| Error::Refused(format!("No pivot table {pv}")))?;
            let (t, _, _, _, src, layout) = self.pivot_parts(&part)?;
            let f = src
                .names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(field))
                .ok_or_else(|| Error::Refused(format!("{} has no field {field}", t.name)))?;
            let mut l = layout.clone();
            if !l.rows.contains(&f) && !l.cols.contains(&f) {
                l.rows = vec![f];
                l.cols.clear();
            }
            let p = pivot::compute(&src, &l).map_err(Error::Refused)?;
            let it = p.items.get(&f).cloned().unwrap_or_default();
            let items: String = it
                .order
                .iter()
                .map(|x| format!("<i x=\"{x}\" s=\"1\"/>"))
                .collect();
            let tab = self.sheet_id(sheet_idx).unwrap_or(1);
            format!(
                "<pivotTables><pivotTable tabId=\"{tab}\" name=\"{}\"/></pivotTables><data><tabular pivotCacheId=\"{}\"><items count=\"{}\">{items}</items></tabular></data>",
                xml::escape(&t.name),
                t.cache_id,
                it.order.len()
            )
        } else if let Some(tn) = table {
            let t = self
                .tables()
                .unwrap_or_default()
                .into_iter()
                .find(|t| t.name.eq_ignore_ascii_case(tn))
                .ok_or_else(|| Error::Refused(format!("No table {tn}")))?;
            let column = t
                .columns
                .iter()
                .position(|c| c.eq_ignore_ascii_case(field))
                .ok_or_else(|| Error::Refused(format!("{tn} has no column {field}")))?;
            format!(
                "<extLst><x:ext xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" uri=\"{{2F2917AC-EB37-4324-AD4E-5DD8C200BD13}}\"><x15:tableSlicerCache xmlns:x15=\"{X15}\" tableId=\"{}\" column=\"{}\"/></x:ext></extLst>",
                t.id,
                column + 1
            )
        } else {
            return Err(Error::Refused(
                "A slicer filters a pivot table or a table".into(),
            ));
        };
        let for_table = pivot.is_none();
        self.in_one_step(|wb| {
            // The cache, named by the workbook.
            let cache_part = wb.free_part("xl/slicerCaches/slicerCache");
            wb.add_part(
                &cache_part,
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<slicerCacheDefinition xmlns=\"{X14}\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" mc:Ignorable=\"x\" xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" name=\"{}\" sourceName=\"{}\">{data}</slicerCacheDefinition>",
                    xml::escape(&cache_name),
                    xml::escape(field)
                ),
                "application/vnd.ms-excel.slicerCache+xml",
            )?;
            let wb_part = wb.workbook_part.clone();
            let rid = wb.add_rel_typed(&wb_part, CACHE_REL, &cache_part)?;
            let p = {
                let mut r = Reader::new(&wb.workbook_xml);
                let mut p = String::new();
                while let Some(t) = r.next_token() {
                    if let Token::Start(tag) = t {
                        p = xml::prefix(tag.qname).to_owned();
                        break;
                    }
                }
                p
            };
            let (rp, decl) = wb.r_prefix();
            let item = if for_table {
                format!("<x15:slicerCache{decl} {rp}:id=\"{rid}\"/>")
            } else {
                format!("<x14:slicerCache{decl} {rp}:id=\"{rid}\"/>")
            };
            wb.workbook_xml = if for_table {
                with_ext_item(&wb.workbook_xml, &p, WB_TABLE_EXT, ("x15", X15), "slicerCaches", &item)
            } else {
                with_ext_item(&wb.workbook_xml, &p, WB_PIVOT_EXT, ("x14", X14), "slicerCaches", &item)
            };
            wb.set_defined_name(&cache_name, Some("#N/A"))?;
            // The slicers part, named by the sheet.
            let sheet_part = wb.sheets[idx].part.clone();
            let existing = wb
                .sheet_parts(idx)?
                .into_iter()
                .find(|(k, _)| k == "slicer")
                .map(|(_, part)| part);
            let row_height = 241_300;
            let slicer = format!(
                "<slicer name=\"{}\" cache=\"{}\" caption=\"{}\" rowHeight=\"{row_height}\"/>",
                xml::escape(&slicer_name),
                xml::escape(&cache_name),
                xml::escape(field)
            );
            match existing {
                Some(part) => {
                    let text = text_of(wb.pkg.part(&part)?, &part)?;
                    let close = text.rfind("</").unwrap_or(text.len());
                    wb.pkg.set_part(
                        &part,
                        format!("{}{slicer}{}", &text[..close], &text[close..]).into_bytes(),
                    );
                }
                None => {
                    let part = wb.free_part("xl/slicers/slicer");
                    wb.add_part(
                        &part,
                        format!(
                            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<slicers xmlns=\"{X14}\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" mc:Ignorable=\"x xr10\" xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:xr10=\"http://schemas.microsoft.com/office/spreadsheetml/2016/revision10\">{slicer}</slicers>"
                        ),
                        "application/vnd.ms-excel.slicer+xml",
                    )?;
                    let srid = wb.add_rel_typed(&sheet_part, SLICER_REL, &part)?;
                    let text = wb.loaded[&idx].0.clone();
                    let sp = wb.loaded[&idx].1.prefix.clone();
                    let (srp, sdecl) = super::links::sheet_r_prefix(&text);
                    let new = if for_table {
                        with_ext_item(
                            &text,
                            &sp,
                            SHEET_TABLE_EXT,
                            ("x14", X14),
                            "slicerList",
                            &format!("<x14:slicer{sdecl} {srp}:id=\"{srid}\"/>"),
                        )
                    } else {
                        with_ext_item(
                            &text,
                            &sp,
                            SHEET_PIVOT_EXT,
                            ("x14", X14),
                            "slicerList",
                            &format!("<x14:slicer{sdecl} {srp}:id=\"{srid}\"/>"),
                        )
                    };
                    wb.replace_sheet_text(idx, new);
                }
            }
            // Its frame in the sheet's drawing.
            let drawing = wb.ensure_drawing(idx)?;
            let text = text_of(wb.pkg.part(&drawing)?, &drawing)?;
            let dp = text
                .find("wsDr")
                .and_then(|i| text[..i].rfind('<').map(|s| text[s + 1..i].to_owned()))
                .unwrap_or_default();
            let id = crate::chart::max_shape_id(&text) + 1;
            let (from, to) = (
                (anchor.start.row, anchor.start.col),
                (anchor.end.row + 1, anchor.end.col + 1),
            );
            let el = format!(
                "<{dp}twoCellAnchor editAs=\"oneCell\"><{dp}from><{dp}col>{}</{dp}col><{dp}colOff>0</{dp}colOff><{dp}row>{}</{dp}row><{dp}rowOff>0</{dp}rowOff></{dp}from><{dp}to><{dp}col>{}</{dp}col><{dp}colOff>0</{dp}colOff><{dp}row>{}</{dp}row><{dp}rowOff>0</{dp}rowOff></{dp}to><mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice xmlns:sle15=\"http://schemas.microsoft.com/office/drawing/2012/slicer\" xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" Requires=\"{}\"><{dp}graphicFrame macro=\"\"><{dp}nvGraphicFramePr><{dp}cNvPr id=\"{id}\" name=\"{}\"/><{dp}cNvGraphicFramePr/></{dp}nvGraphicFramePr><{dp}xfrm><a:off xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" x=\"0\" y=\"0\"/><a:ext xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" cx=\"0\" cy=\"0\"/></{dp}xfrm><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.microsoft.com/office/drawing/2010/slicer\"><sle:slicer xmlns:sle=\"http://schemas.microsoft.com/office/drawing/2010/slicer\" name=\"{}\"/></a:graphicData></a:graphic></{dp}graphicFrame></mc:Choice></mc:AlternateContent><{dp}clientData/></{dp}twoCellAnchor>",
                from.1,
                from.0,
                to.1,
                to.0,
                if for_table { "sle15" } else { "a14" },
                xml::escape(&slicer_name),
                xml::escape(&slicer_name)
            );
            let close = text.rfind("</").unwrap_or(text.len());
            wb.pkg.set_part(
                &drawing,
                format!("{}{el}{}", &text[..close], &text[close..]).into_bytes(),
            );
            wb.generation += 1;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Slicer `index` of sheet `idx` with only `selected` selected (all
    /// when empty): its pivot table's field's other items hidden, or its
    /// table filtered. One undo step.
    pub fn select_slicer(&mut self, idx: usize, index: usize, selected: &[String]) -> Result<()> {
        let s = self
            .sheet_slicers(idx)
            .into_iter()
            .nth(index)
            .ok_or_else(|| Error::Refused("No such slicer".into()))?;
        let cache = self
            .slicer_caches()
            .into_iter()
            .find(|c| c.name == s.cache)
            .ok_or_else(|| Error::Refused("The slicer's cache is missing".into()))?;
        let items = self.slicer_items(&cache);
        let all = selected.is_empty()
            || items
                .iter()
                .all(|(l, _)| selected.iter().any(|x| x.eq_ignore_ascii_case(l)));
        let hidden: Vec<String> = if all {
            Vec::new()
        } else {
            items
                .iter()
                .filter(|(l, _)| !selected.iter().any(|x| x.eq_ignore_ascii_case(l)))
                .map(|(l, _)| {
                    if l == "(blank)" {
                        String::new()
                    } else {
                        l.clone()
                    }
                })
                .collect()
        };
        if let Some(pv) = &cache.pivot {
            let found = self.pivot_tables().into_iter().find_map(|(i, part)| {
                let text = String::from_utf8(self.pkg.part(&part).ok()?).ok()?;
                pivot::parse_table(&text)
                    .name
                    .eq_ignore_ascii_case(pv)
                    .then_some((i, part))
            });
            let (pidx, part) =
                found.ok_or_else(|| Error::Refused(format!("No pivot table {pv}")))?;
            let (_, _, _, _, src, mut layout) = self.pivot_parts(&part)?;
            let f = src
                .names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(&cache.source))
                .ok_or_else(|| Error::Refused("The slicer's field is gone".into()))?;
            layout.filters.retain(|x| {
                !(x.field as usize == f && x.kind == kalem_viewer::PivotFilterKind::Items)
            });
            if !hidden.is_empty() {
                layout.filters.push(kalem_viewer::PivotFilter {
                    field: f as u32,
                    kind: kalem_viewer::PivotFilterKind::Items,
                    hidden: hidden.clone(),
                    ..kalem_viewer::PivotFilter::default()
                });
            }
            let cache_part = cache.part.clone();
            return self.in_one_step(|wb| {
                if !layout.rows.contains(&f) && !layout.cols.contains(&f) {
                    return Err(Error::Refused(format!(
                        "Put {} on the pivot table's rows or columns for its slicer to filter it",
                        cache.source
                    )));
                }
                wb.write_pivot(pidx, &part, Some(layout))?;
                // The cache's items selected as the field's.
                let text = text_of(wb.pkg.part(&cache_part)?, &cache_part)?;
                let mut r = Reader::new(&text);
                let mut splices = Vec::new();
                let labels: Vec<String> = items.iter().map(|(l, _)| l.to_lowercase()).collect();
                let mut k = 0;
                while let Some(t) = r.next_token() {
                    if let Token::Start(tag) = t
                        && tag.name == "i"
                    {
                        let off = labels
                            .get(k)
                            .is_some_and(|l| hidden.iter().any(|h| h.to_lowercase() == *l));
                        splices.push((
                            tag.span.clone(),
                            xml::set_attr(
                                &text[tag.span.clone()],
                                "s",
                                if off { "0" } else { "1" },
                            ),
                        ));
                        k += 1;
                    }
                }
                wb.pkg
                    .set_part(&cache_part, splice(&text, splices).into_bytes());
                wb.generation += 1;
                wb.batch_changed = true;
                Ok(())
            });
        }
        let Some((table_id, column)) = cache.table else {
            return Err(Error::Refused(
                "The slicer filters nothing Kalem knows".into(),
            ));
        };
        let t = self
            .tables()?
            .into_iter()
            .find(|t| t.id == table_id)
            .ok_or_else(|| Error::Refused("The slicer's table is gone".into()))?;
        let keep: Option<Vec<String>> = (!all).then(|| {
            items
                .iter()
                .filter(|(l, _)| selected.iter().any(|x| x.eq_ignore_ascii_case(l)))
                .map(|(l, _)| {
                    if l == "(blank)" {
                        String::new()
                    } else {
                        l.clone()
                    }
                })
                .collect()
        });
        self.load(t.sheet)?;
        self.in_one_step(|wb| {
            wb.filter_table(&t, column.saturating_sub(1), keep.as_deref())?;
            wb.generation += 1;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Removes slicer `index` of sheet `idx`: its filter taken off, its
    /// frame, its entry and (when no other slicer uses it) its cache. One
    /// undo step.
    pub fn delete_slicer(&mut self, idx: usize, index: usize) -> Result<()> {
        let s = self
            .sheet_slicers(idx)
            .into_iter()
            .nth(index)
            .ok_or_else(|| Error::Refused("No such slicer".into()))?;
        self.in_one_step(|wb| {
            // Every item selected again.
            let _ = wb.select_slicer(idx, index, &[]);
            if let Some((_, span)) = wb.slicer_anchor(idx, &s.name)
                && let Some(drawing) = wb.sheet_drawing(idx)
            {
                let text = text_of(wb.pkg.part(&drawing)?, &drawing)?;
                wb.pkg.set_part(
                    &drawing,
                    format!("{}{}", &text[..span.start], &text[span.end..]).into_bytes(),
                );
            }
            let text = text_of(wb.pkg.part(&s.part)?, &s.part)?;
            let mut r = Reader::new(&text);
            let mut cut = None;
            while let Some(t) = r.next_token() {
                if let Token::Start(tag) = t
                    && tag.name == "slicer"
                    && tag.attr("name").as_deref() == Some(s.name.as_str())
                {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    cut = Some(tag.span.start..end);
                }
            }
            if let Some(c) = cut {
                wb.pkg.set_part(
                    &s.part,
                    splice(&text, vec![(c, String::new())]).into_bytes(),
                );
            }
            wb.generation += 1;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// A sheet's `sheetId` in the workbook part.
    pub(crate) fn sheet_id(&self, idx: usize) -> Option<u32> {
        let mut r = Reader::new(&self.workbook_xml);
        let mut n = 0;
        while let Some(t) = r.next_token() {
            if let Token::Start(tag) = t
                && tag.name == "sheet"
            {
                if n == idx {
                    return tag.attr("sheetId").and_then(|v| v.parse().ok());
                }
                n += 1;
            }
        }
        None
    }
}
