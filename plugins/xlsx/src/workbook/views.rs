//! How a sheet is shown, as its first `<sheetView>` keeps it: the zoom
//! (`zoomScale`), gridlines (`showGridLines`), headings
//! (`showRowColHeaders`), Page Break Preview (`view`), and a split into
//! panes (`<pane state="split">`, its sizes in twentieths of a point).

use super::*;

/// A split as the file keeps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitRaw {
    /// The top pane's height in twips, headings included.
    pub y: f64,
    /// The left pane's width in twips, headings included.
    pub x: f64,
    /// The first cell the top left pane shows.
    pub top_left: CellRef,
    /// The first cell the bottom right pane shows.
    pub pane_top_left: CellRef,
}

/// A sheet's view settings as the file keeps them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewRaw {
    /// Percent.
    pub zoom: u16,
    /// Gridlines drawn.
    pub gridlines: bool,
    /// Letters and numbers shown.
    pub headings: bool,
    /// Page Break Preview.
    pub preview: bool,
    /// Split into panes.
    pub split: Option<SplitRaw>,
}

impl Default for ViewRaw {
    fn default() -> Self {
        ViewRaw {
            zoom: 100,
            gridlines: true,
            headings: true,
            preview: false,
            split: None,
        }
    }
}

/// A sheet part's first view.
pub(crate) fn parse(text: &str) -> ViewRaw {
    let mut v = ViewRaw::default();
    let mut r = Reader::new(text);
    let mut in_view = false;
    let mut top_left = CellRef::new(0, 0);
    while let Some(t) = r.next_token() {
        match t {
            Token::Start(tag) if tag.name == "sheetView" && !in_view => {
                let off = |k: &str| matches!(tag.attr(k).as_deref(), Some("0" | "false"));
                v.zoom = tag
                    .attr("zoomScale")
                    .and_then(|z| z.trim().parse::<u16>().ok())
                    .filter(|z| (10..=400).contains(z))
                    .unwrap_or(100);
                v.gridlines = !off("showGridLines");
                v.headings = !off("showRowColHeaders");
                v.preview = tag.attr("view").as_deref() == Some("pageBreakPreview");
                top_left = tag
                    .attr("topLeftCell")
                    .and_then(|c| CellRef::parse(c.trim()))
                    .unwrap_or(top_left);
                if tag.empty {
                    break;
                }
                in_view = true;
            }
            Token::Start(tag) if tag.name == "pane" && in_view => {
                let state = tag.attr("state");
                if matches!(state.as_deref(), None | Some("split")) {
                    let n = |k: &str| {
                        tag.attr(k)
                            .and_then(|x| x.trim().parse::<f64>().ok())
                            .unwrap_or(0.0)
                    };
                    let (x, y) = (n("xSplit"), n("ySplit"));
                    if x > 0.0 || y > 0.0 {
                        v.split = Some(SplitRaw {
                            y,
                            x,
                            top_left,
                            pane_top_left: tag
                                .attr("topLeftCell")
                                .and_then(|c| CellRef::parse(c.trim()))
                                .unwrap_or(top_left),
                        });
                    }
                }
            }
            Token::End {
                name: "sheetView", ..
            } => break,
            Token::Start(tag) if tag.name == "sheetData" => break,
            _ => {}
        }
    }
    v
}

/// A sheet part with its first view as `v` says; the frozen pane kept
/// when there is no split.
pub(crate) fn apply(text: &str, p: &str, v: &ViewRaw) -> String {
    let now = parse(text);
    let mut text = text.to_owned();
    match v.split {
        Some(s) => {
            let active = match (s.y > 0.0, s.x > 0.0) {
                (true, true) => "bottomRight",
                (true, false) => "bottomLeft",
                _ => "topRight",
            };
            let mut a = String::new();
            if s.x > 0.0 {
                a.push_str(&format!(" xSplit=\"{}\"", s.x.round()));
            }
            if s.y > 0.0 {
                a.push_str(&format!(" ySplit=\"{}\"", s.y.round()));
            }
            text = with_pane(
                &text,
                p,
                format!(
                    "<{p}pane{a} topLeftCell=\"{}\" activePane=\"{active}\" state=\"split\"/>",
                    s.pane_top_left
                ),
            );
        }
        None if now.split.is_some() => text = with_pane(&text, p, String::new()),
        None => {}
    }
    let default = ViewRaw::default();
    let attrs: [(&str, Option<String>); 5] = [
        ("zoomScale", (v.zoom != 100).then(|| v.zoom.to_string())),
        ("showGridLines", (!v.gridlines).then(|| "0".into())),
        ("showRowColHeaders", (!v.headings).then(|| "0".into())),
        ("view", v.preview.then(|| "pageBreakPreview".into())),
        ("topLeftCell", v.split.map(|s| s.top_left.to_string())),
    ];
    let mut r = Reader::new(&text);
    while let Some(t) = r.next_token() {
        let Token::Start(tag) = t else { continue };
        match tag.name {
            "sheetView" => {
                let mut el = text[tag.span.clone()].to_owned();
                for (k, val) in &attrs {
                    el = match val {
                        Some(val) => xml::set_attr(&el, k, val),
                        // The top left cell stays without a split.
                        None if *k == "topLeftCell" => el,
                        None => xml::remove_attr(&el, k),
                    };
                }
                return splice(&text, vec![(tag.span.clone(), el)]);
            }
            "sheetData" => break,
            _ => {}
        }
    }
    if *v == default {
        return text;
    }
    // No view yet: one with the settings.
    let mut a = String::new();
    for (k, val) in &attrs {
        if let Some(val) = val {
            a.push_str(&format!(" {k}=\"{}\"", xml::escape(val)));
        }
    }
    insert_top_level(
        &text,
        &["sheetFormatPr", "cols", "sheetData"],
        &format!("<{p}sheetViews><{p}sheetView{a} workbookViewId=\"0\"/></{p}sheetViews>"),
    )
}

impl Workbook {
    /// How sheet `idx` is shown.
    pub fn view_raw(&mut self, idx: usize) -> ViewRaw {
        if let Some(v) = self.views.get(&idx) {
            return *v;
        }
        if self.load(idx).is_err() {
            return ViewRaw::default();
        }
        parse(&self.loaded[&idx].0)
    }

    /// Sets how sheet `idx` is shown, written when the workbook is saved;
    /// whether it changed.
    pub fn set_view_raw(&mut self, idx: usize, v: ViewRaw) -> Result<bool> {
        self.load(idx)?;
        if self.view_raw(idx) == v {
            return Ok(false);
        }
        self.views.insert(idx, v);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_read_and_written() {
        let t = r#"<worksheet><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" state="frozen"/></sheetView></sheetViews><sheetData/></worksheet>"#;
        assert_eq!(parse(t), ViewRaw::default());
        let v = ViewRaw {
            zoom: 125,
            gridlines: false,
            headings: true,
            preview: true,
            split: None,
        };
        let out = apply(t, "", &v);
        assert!(out.contains(r#"zoomScale="125""#) && out.contains(r#"showGridLines="0""#));
        assert!(out.contains(r#"view="pageBreakPreview""#) && out.contains("frozen"));
        assert_eq!(parse(&out), v);
        let split = ViewRaw {
            split: Some(SplitRaw {
                y: 1500.0,
                x: 0.0,
                top_left: CellRef::new(4, 0),
                pane_top_left: CellRef::new(10, 0),
            }),
            ..v
        };
        let out = apply(&out, "", &split);
        assert!(
            out.contains(r#"state="split""#) && !out.contains("frozen"),
            "{out}"
        );
        assert_eq!(parse(&out), split);
        // Back to the default: nothing of it left.
        let out = apply(&out, "", &ViewRaw::default());
        assert!(
            !out.contains("pane") && !out.contains("zoomScale") && !out.contains("view="),
            "{out}"
        );
        // A sheet without a view gets one.
        let bare = "<worksheet><sheetData/></worksheet>";
        assert_eq!(parse(&apply(bare, "", &v)), v);
    }
}
