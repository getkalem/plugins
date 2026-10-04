//! A chart's series drawn as another kind on the secondary axis, its
//! trendlines, error bars and labels from cells; a chart moved to a chart
//! sheet of its own and back; a chart saved as a chart template (`.crtx`)
//! and a template's look given to a chart. Each one undo step.

use super::*;
use kalem_viewer::{ChartKind, ErrorBars, Trendline};

const CHART_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const DRAWING_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";
const CHARTSHEET_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml";

fn rgb3(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

impl Workbook {
    /// Sheet `idx`'s chart `index`: its drawing, its anchor there, and its
    /// part.
    fn chart_at(&self, idx: usize, index: usize) -> Result<(String, chart::Anchor, String)> {
        let drawing = self
            .sheet_drawing(idx)
            .ok_or_else(|| Error::Refused("This sheet has no charts".into()))?;
        let text = text_of(self.pkg.part(&drawing)?, &drawing)?;
        let (a, part) = chart::parse_drawing(&text)
            .into_iter()
            .filter_map(|a| self.rel_target(&drawing, &a.rid).map(|p| (a, p)))
            .nth(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?;
        Ok((drawing, a, part))
    }

    /// A chart part changed by `edit`, as one undo step.
    fn edit_chart_part(
        &mut self,
        idx: usize,
        index: usize,
        edit: impl FnOnce(&Self, &str) -> Result<String>,
    ) -> Result<()> {
        let (_, _, part) = self.chart_at(idx, index)?;
        let old = text_of(self.pkg.part(&part)?, &part)?;
        if old.contains("/drawing/2014/chartex") {
            return Err(Error::Refused(
                "Kalem draws this kind of chart but does not change it".into(),
            ));
        }
        let new = edit(self, &old)?;
        if new == old {
            return Ok(());
        }
        self.in_one_step(|wb| {
            wb.pkg.set_part(&part, new.into_bytes());
            wb.generation += 1;
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Series `series` of a chart drawn as `kind` (`None` the chart's),
    /// against the secondary value axis or the primary.
    pub fn set_series_kind(
        &mut self,
        idx: usize,
        index: usize,
        series: usize,
        kind: Option<ChartKind>,
        secondary: bool,
    ) -> Result<()> {
        self.edit_chart_part(idx, index, |_, old| {
            crate::chart_more::with_series_kind(old, series, kind, secondary)
                .map_err(Error::Refused)
        })
    }

    /// Series `series` of a chart given a trendline, or none.
    pub fn set_trendline(
        &mut self,
        idx: usize,
        index: usize,
        series: usize,
        trendline: Option<Trendline>,
    ) -> Result<()> {
        self.edit_chart_part(idx, index, |_, old| {
            crate::chart_more::with_trendline(old, series, trendline)
                .ok_or_else(|| Error::Refused("No such series".into()))
        })
    }

    /// Series `series` of a chart given error bars, or none.
    pub fn set_error_bars(
        &mut self,
        idx: usize,
        index: usize,
        series: usize,
        bars: Option<ErrorBars>,
    ) -> Result<()> {
        self.edit_chart_part(idx, index, |wb, old| {
            let kind = chart::parse_chart(old, &wb.theme).kind;
            let xy = matches!(kind, ChartKind::Scatter | ChartKind::Bubble);
            crate::chart_more::with_error_bars(old, series, bars, xy)
                .ok_or_else(|| Error::Refused("No such series".into()))
        })
    }

    /// Series `series` of a chart labeled with the texts of `range` on the
    /// sheet, or no more.
    pub fn set_label_cells(
        &mut self,
        idx: usize,
        index: usize,
        series: usize,
        range: Option<Range>,
    ) -> Result<()> {
        let sheet = self.sheets[idx].name.clone();
        let cells = match range {
            Some(r) => {
                self.load(idx)?;
                Some((chart::reference(&sheet, r), self.range_texts(idx, r)))
            }
            None => None,
        };
        self.edit_chart_part(idx, index, |_, old| {
            crate::chart_more::with_label_cells(
                old,
                series,
                cells.as_ref().map(|(f, t)| (f.as_str(), t.as_slice())),
            )
            .ok_or_else(|| Error::Refused("No such series".into()))
        })
    }

    /// A drawing's anchor taken out, with its relationship.
    fn detach_anchor(&mut self, drawing: &str, a: &chart::Anchor) -> Result<()> {
        let text = text_of(self.pkg.part(drawing)?, drawing)?;
        self.pkg.set_part(
            drawing,
            format!("{}{}", &text[..a.span.start], &text[a.span.end..]).into_bytes(),
        );
        let rels_path = rels::rels_path(drawing);
        let rels_text = text_of(self.pkg.part(&rels_path)?, &rels_path)?;
        self.pkg
            .set_part(&rels_path, rels::remove(&rels_text, &a.rid).into_bytes());
        Ok(())
    }

    /// Move Chart to a new sheet: chart `index` of sheet `idx` taken off
    /// it and put on a chart sheet named `name`, last; its index.
    pub fn move_chart_to_sheet(&mut self, idx: usize, index: usize, name: &str) -> Result<usize> {
        self.check_sheet_name(usize::MAX, name)?;
        let (drawing, a, part) = self.chart_at(idx, index)?;
        let name = name.to_owned();
        let mut new_idx = 0;
        self.in_one_step(|wb| {
            wb.detach_anchor(&drawing, &a)?;
            // The chart sheet's drawing: the chart filling it.
            let new_drawing = wb.free_part("xl/drawings/drawing");
            wb.pkg.set_part(&new_drawing, Vec::new());
            let rid = wb.add_rel(&new_drawing, "chart", &part)?;
            wb.pkg.remove_part(&new_drawing);
            wb.add_part(
                &new_drawing,
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><xdr:absoluteAnchor><xdr:pos x=\"0\" y=\"0\"/><xdr:ext cx=\"8661400\" cy=\"6286500\"/><xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"2\" name=\"Chart 1\"/><xdr:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></xdr:cNvGraphicFramePr></xdr:nvGraphicFramePr><xdr:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/></xdr:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:absoluteAnchor></xdr:wsDr>"
                ),
                DRAWING_TYPE,
            )?;
            let sheet_part = wb.free_part("xl/chartsheets/sheet");
            wb.pkg.set_part(&sheet_part, Vec::new());
            let drid = wb.add_rel(&sheet_part, "drawing", &new_drawing)?;
            wb.pkg.remove_part(&sheet_part);
            let main = wb
                .workbook_xml
                .split("xmlns=\"")
                .nth(1)
                .and_then(|t| t.split('"').next())
                .unwrap_or("http://schemas.openxmlformats.org/spreadsheetml/2006/main")
                .to_owned();
            wb.add_part(
                &sheet_part,
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<chartsheet xmlns=\"{main}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheetPr/><sheetViews><sheetView zoomToFit=\"1\" workbookViewId=\"0\"/></sheetViews><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/><drawing r:id=\"{drid}\"/></chartsheet>"
                ),
                CHARTSHEET_TYPE,
            )?;
            new_idx = wb.add_sheet_entry(name, sheet_part, kind::CHARTSHEET, SheetKind::Chartsheet)?;
            wb.generation += 1;
            wb.batch_changed = true;
            Ok(())
        })?;
        Ok(new_idx)
    }

    /// Move Chart back as an object: chart sheet `idx`'s chart put on
    /// sheet `target` over `anchor`, the chart sheet removed; the target's
    /// index then.
    pub fn move_chart_to_grid(
        &mut self,
        idx: usize,
        target: usize,
        anchor: Range,
    ) -> Result<usize> {
        if self.sheets.get(idx).map(|s| s.kind) != Some(SheetKind::Chartsheet) {
            return Err(Error::Refused("This is not a chart sheet".into()));
        }
        if self.sheets.get(target).map(|s| s.kind) != Some(SheetKind::Worksheet) {
            return Err(Error::NotAWorksheet(
                self.sheets
                    .get(target)
                    .map_or(String::new(), |s| s.name.clone()),
            ));
        }
        let (drawing, a, part) = self.chart_at(idx, 0)?;
        self.in_one_step(|wb| {
            wb.detach_anchor(&drawing, &a)?;
            wb.attach_chart(
                target,
                &part,
                (anchor.start.row, anchor.start.col),
                (anchor.end.row, anchor.end.col),
            )?;
            // The chart sheet's drawing goes with it.
            wb.remove_part_and_type(&drawing)?;
            let own = rels::rels_path(&drawing);
            if wb.pkg.contains(&own) {
                wb.pkg.remove_part(&own);
            }
            wb.edit_sheets_now(&kalem_viewer::SheetEdit::Delete(idx))?;
            wb.generation += 1;
            wb.batch_changed = true;
            Ok(())
        })?;
        Ok(if target > idx { target - 1 } else { target })
    }

    /// Save as Template: chart `index` of sheet `idx` as a chart template
    /// package (`.crtx`), its data's cells left as they are.
    pub fn chart_template(&mut self, idx: usize, index: usize) -> Result<Vec<u8>> {
        let (_, _, part) = self.chart_at(idx, index)?;
        let chart = self.pkg.part(&part)?;
        // An empty ZIP archive, then its parts.
        let mut empty = b"PK\x05\x06".to_vec();
        empty.extend_from_slice(&[0u8; 18]);
        let mut pkg = Package::read(empty)?;
        pkg.set_part(
            "[Content_Types].xml",
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/chart/chart.xml\" ContentType=\"{CHART_TYPE}\"/></Types>"
            )
            .into_bytes(),
        );
        pkg.set_part(
            "_rels/.rels",
            b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"chart/chart.xml\"/></Relationships>".to_vec(),
        );
        pkg.set_part("chart/chart.xml", chart);
        Ok(pkg.write()?)
    }

    /// A chart template's look given to chart `index` of sheet `idx`: its
    /// kind, legend, data labels, gridlines, chart and plot areas, axis
    /// format, fonts and series colors; the data kept.
    pub fn apply_chart_template(
        &mut self,
        idx: usize,
        index: usize,
        template: &[u8],
    ) -> Result<()> {
        let pkg = Package::read(template.to_vec())
            .map_err(|_| Error::Refused("Not a chart template".into()))?;
        let text = pkg
            .names()
            .into_iter()
            .filter(|n| n.ends_with(".xml"))
            .filter_map(|n| pkg.part(&n).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .find(|t| t.contains("chartSpace") && t.contains("plotArea"))
            .ok_or_else(|| Error::Refused("Not a chart template".into()))?;
        let t = chart::parse_chart(&text, &self.theme);
        let paint = |f: chart::Fill| match f {
            chart::Fill::Auto => kalem_viewer::Paint::Automatic,
            chart::Fill::None => kalem_viewer::Paint::None,
            chart::Fill::Color(c) => kalem_viewer::Paint::Color(rgb3(c)),
        };
        let font = |f: &chart::Font| kalem_viewer::AxisFont {
            size: f.size,
            bold: f.bold,
            italic: f.italic,
            color: f.color.map(rgb3),
            face: f.face.clone(),
        };
        let legend = t.legend.as_deref().map(|v| match v {
            "b" => kalem_viewer::LegendPosition::Bottom,
            "t" => kalem_viewer::LegendPosition::Top,
            "l" => kalem_viewer::LegendPosition::Left,
            "tr" => kalem_viewer::LegendPosition::TopRight,
            _ => kalem_viewer::LegendPosition::Right,
        });
        let now = self.charts(idx)?;
        let current = now
            .get(index)
            .ok_or_else(|| Error::Refused("No such chart".into()))?
            .clone();
        // Its own title, not the one a lone series' name makes.
        let has_title = {
            let (_, _, part) = self.chart_at(idx, index)?;
            let text = text_of(self.pkg.part(&part)?, &part)?;
            chart::parse_chart(&text, &self.theme).title.is_some()
        };
        self.in_one_step(|wb| {
            if t.kind != current.kind
                && !matches!(
                    t.kind,
                    ChartKind::Other | ChartKind::Histogram | ChartKind::Waterfall
                )
            {
                wb.set_chart_kind(idx, index, t.kind)?;
            }
            wb.set_legend(idx, index, legend)?;
            wb.set_data_labels(
                idx,
                index,
                kalem_viewer::DataLabels {
                    value: t.labels.0,
                    category: t.labels.1,
                    series: t.labels.2,
                    percent: t.labels.3,
                },
            )?;
            let axes = !matches!(t.kind, ChartKind::Pie | ChartKind::Doughnut);
            if axes {
                wb.set_gridlines(
                    idx,
                    index,
                    kalem_viewer::Gridlines {
                        horizontal_major: t.gridlines.0,
                        horizontal_minor: t.gridlines.1,
                        vertical_major: t.gridlines.2,
                        vertical_minor: t.gridlines.3,
                    },
                )?;
                wb.set_axis_format(idx, index, t.axis_format.as_deref())?;
                wb.set_axis_font(idx, index, false, &font(&t.horizontal_font))?;
                wb.set_axis_font(idx, index, true, &font(&t.vertical_font))?;
            }
            wb.set_chart_area(idx, index, paint(t.background), paint(t.border))?;
            wb.set_plot_area(idx, index, paint(t.plot_background), paint(t.plot_border))?;
            if has_title {
                wb.set_title_font(idx, index, &font(&t.title_font))?;
            }
            if legend.is_some() {
                wb.set_legend_font(idx, index, &font(&t.legend_font))?;
            }
            if axes {
                for (i, s) in t.series.iter().enumerate().take(current.series.len()) {
                    if let Some(c) = s.color {
                        wb.set_series_color(idx, index, i, Some(rgb3(c)))?;
                    }
                }
            }
            Ok(())
        })
    }
}
