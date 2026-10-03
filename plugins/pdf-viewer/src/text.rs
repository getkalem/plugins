//! A page's text, for search, copy and the terminal: the glyphs the page
//! draws, in the order its content stream draws them, with the Unicode
//! hayro finds for each (`ToUnicode`, then the glyph's name). Lines break
//! where the baseline moves or the text goes back; a space goes where two
//! glyphs stand further apart than a space would be.

use std::ops::Range;

use hayro::hayro_interpret::font::Glyph;
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::hayro_syntax::page::Page;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache,
    InterpreterSettings, Paint, PathDrawMode, SoftMask, interpret_page,
};
use kurbo::{Affine, BezPath, Point, Rect, Vec2};

/// The glyph space of hayro's glyph transforms: 1,000 units an em.
const EM: f64 = 1000.0;

/// A glyph placed on the page.
struct Placed {
    text: String,
    /// The origin on the baseline, in the page's pixels at scale 1.
    origin: Point,
    /// Where the next glyph would start.
    end: Point,
    /// The unit vector along the baseline.
    dir: Vec2,
    /// The em's height.
    size: f64,
}

struct Collector {
    glyphs: Vec<Placed>,
    /// The page as rendered: a glyph outside it is not shown, so it is
    /// not text of the page (printers' marks, a cropped-off header).
    page: Rect,
}

impl<'a> Device<'a> for Collector {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'a>>) {}
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn draw_path(&mut self, _: &BezPath, _: Affine, _: &Paint<'a>, _: &PathDrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: Affine) {}
    fn pop_clip_path(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    fn draw_glyph(
        &mut self,
        glyph: &Glyph<'a>,
        transform: Affine,
        glyph_transform: Affine,
        _: &Paint<'a>,
        _: &GlyphDrawMode,
    ) {
        // Invisible text (OCR layers, render mode 3) is text all the same.
        let Some(text) = glyph.as_unicode().map(|u| match u {
            BfString::Char(c) => c.to_string(),
            BfString::String(s) => s,
        }) else {
            return;
        };
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if text.is_empty() {
            return;
        }
        let m = transform * glyph_transform;
        let origin = m * Point::ORIGIN;
        if !self.page.inflate(1.0, 1.0).contains(origin) {
            return;
        }
        let [a, b, c, d, _, _] = m.as_coeffs();
        let size = (c * c + d * d).sqrt() * EM;
        let along = Vec2::new(a, b);
        let dir = if along.hypot() > 0.0 {
            along.normalize()
        } else {
            Vec2::new(1.0, 0.0)
        };
        let advance = match glyph {
            Glyph::Outline(o) => o.advance_width().map(f64::from),
            Glyph::Type3(_) => None,
        };
        let end = match advance {
            Some(w) => m * Point::new(w, 0.0),
            None => origin + dir * size * 0.5,
        };
        self.glyphs.push(Placed {
            text,
            origin,
            end,
            dir,
            size,
        });
    }
}

/// A page's text and where it stands: each glyph's byte range of the
/// text with its box in the page's pixels at scale 1 (x0, y0, x1, y1).
#[derive(Debug, Clone, Default)]
pub(crate) struct PageText {
    pub(crate) text: String,
    pub(crate) boxes: Vec<(Range<usize>, [f32; 4])>,
}

impl PageText {
    /// The rectangles (x, y, width, height) of `range`: the boxes of the
    /// glyphs in it, a line's run joined into one.
    pub(crate) fn rects(&self, range: Range<usize>) -> Vec<[f32; 4]> {
        let mut out: Vec<[f32; 4]> = Vec::new();
        for (r, b) in &self.boxes {
            if r.end <= range.start || r.start >= range.end {
                continue;
            }
            let h = b[3] - b[1];
            if let Some(last) = out.last_mut() {
                let (lx1, ly0, lh) = (last[0] + last[2], last[1], last[3]);
                // The same line, the next glyph: one rectangle.
                if (b[1] - ly0).abs() < lh.max(h) * 0.5 && b[0] - lx1 < h && b[0] >= last[0] {
                    let x1 = lx1.max(b[2]);
                    let y0 = ly0.min(b[1]);
                    let y1 = (ly0 + lh).max(b[3]);
                    *last = [last[0], y0, x1 - last[0], y1 - y0];
                    continue;
                }
            }
            out.push([b[0], b[1], b[2] - b[0], b[3] - b[1]]);
        }
        out
    }
}

/// A glyph's box: from its origin to where the next glyph starts, an em
/// high from a little below the baseline.
fn glyph_box(g: &Placed) -> [f32; 4] {
    if flat(g) {
        let (x0, x1) = (g.origin.x.min(g.end.x), g.origin.x.max(g.end.x));
        return [
            x0 as f32,
            (g.origin.y - g.size * 0.8) as f32,
            x1 as f32,
            (g.origin.y + g.size * 0.2) as f32,
        ];
    }
    let half = g.size * 0.5;
    [
        (g.origin.x.min(g.end.x) - half) as f32,
        (g.origin.y.min(g.end.y) - half) as f32,
        (g.origin.x.max(g.end.x) + half) as f32,
        (g.origin.y.max(g.end.y) + half) as f32,
    ]
}

/// The text of `page`, with fonts parsed once for the pages that share
/// `cache`.
pub(crate) fn page_text<'a>(
    page: &Page<'a>,
    settings: &InterpreterSettings,
    cache: &InterpreterCache<'a>,
) -> PageText {
    let (w, h) = page.render_dimensions();
    let area = Rect::new(0.0, 0.0, w as f64, h as f64);
    let mut ctx = Context::new(
        page.initial_transform(true).to_kurbo(),
        area,
        cache,
        page.xref(),
        settings.clone(),
    );
    let mut collector = Collector {
        glyphs: Vec::new(),
        page: area,
    };
    interpret_page(page, &mut ctx, &mut collector);
    lay_out(&collector.glyphs)
}

/// Whether glyph `i` is a space the text takes back: the next glyph is
/// drawn over it (some producers draw a space glyph, then move back by its
/// width and kern the word on), so it separates nothing.
fn cancelled_space(glyphs: &[Placed], i: usize) -> bool {
    let g = &glyphs[i];
    if !g.text.chars().all(char::is_whitespace) {
        return false;
    }
    let Some(next) = glyphs.get(i + 1) else {
        return false;
    };
    let width = g.dir.dot(g.end - g.origin);
    let at = g.dir.dot(next.origin - g.origin);
    g.dir.cross(next.origin - g.origin).abs() < g.size * 0.6 && at < width * 0.5
}

/// Whether a glyph runs left to right, as almost all text does; other
/// text (turned, vertical) is kept in the order it is drawn.
fn flat(g: &Placed) -> bool {
    g.dir.x > 0.99
}

/// Glyphs on one baseline.
struct Line {
    glyphs: Vec<usize>,
    flat: bool,
    x0: f64,
    x1: f64,
    base: f64,
    size: f64,
}

impl Line {
    fn new(glyphs: &[Placed], run: Vec<usize>) -> Line {
        let first = &glyphs[run[0]];
        let mut line = Line {
            flat: flat(first),
            x0: f64::INFINITY,
            x1: f64::NEG_INFINITY,
            base: first.origin.y,
            size: 0.0,
            glyphs: Vec::new(),
        };
        for &i in &run {
            let g = &glyphs[i];
            line.x0 = line.x0.min(g.origin.x).min(g.end.x);
            line.x1 = line.x1.max(g.origin.x).max(g.end.x);
            line.size = line.size.max(g.size);
        }
        line.glyphs = run;
        line
    }

    fn top(&self) -> f64 {
        self.base - self.size * 0.8
    }

    fn bottom(&self) -> f64 {
        self.base + self.size * 0.25
    }

    /// Whether `other` continues this line: on its baseline (a
    /// superscript is), touching or inside it.
    fn joins(&self, other: &Line) -> bool {
        let size = self.size.max(other.size);
        self.flat
            && other.flat
            && (self.base - other.base).abs() < size * 0.5
            && other.x0 <= self.x1 + size
            && other.x1 >= self.x0 - size
    }

    fn absorb(&mut self, other: Line) {
        if other.size > self.size {
            self.base = other.base;
            self.size = other.size;
        }
        self.x0 = self.x0.min(other.x0);
        self.x1 = self.x1.max(other.x1);
        self.glyphs.extend(other.glyphs);
    }
}

/// The page's text in reading order. Producers draw text in any order
/// (Word draws a line's code font after the whole page), so the glyphs
/// drawn in a row are gathered into runs, runs on one baseline that touch
/// into lines, and the lines ordered top to bottom, a column at a time
/// where consecutive rows of the page share a gutter.
fn lay_out(glyphs: &[Placed]) -> PageText {
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for i in (0..glyphs.len()).filter(|&i| !cancelled_space(glyphs, i)) {
        let g = &glyphs[i];
        let continues = runs.last().and_then(|r| r.last()).is_some_and(|&j| {
            let p = &glyphs[j];
            let size = p.size.max(g.size).max(0.1);
            let across = p.dir.cross(g.origin - p.origin);
            let along = p.dir.dot(g.origin - p.end);
            flat(p) == flat(g) && across.abs() < size * 0.5 && along > -size * 0.5 && along < size
        });
        match runs.last_mut() {
            Some(run) if continues => run.push(i),
            _ => runs.push(vec![i]),
        }
    }
    let mut lines: Vec<Line> = Vec::new();
    for run in runs {
        let line = Line::new(glyphs, run);
        match lines.iter_mut().rev().find(|l| l.joins(&line)) {
            Some(l) => l.absorb(line),
            None => lines.push(line),
        }
    }
    let rows = order(&lines, (0..lines.len()).collect(), 0);
    let mut out = PageText::default();
    for row in rows {
        let parts: Vec<PageText> = row
            .iter()
            .map(|&l| line_text(glyphs, &lines[l]))
            .filter(|t| !t.text.is_empty())
            .collect();
        if parts.is_empty() {
            continue;
        }
        if !out.text.is_empty() {
            out.text.push('\n');
        }
        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                out.text.push(' ');
            }
            let at = out.text.len();
            out.text.push_str(&part.text);
            out.boxes.extend(
                part.boxes
                    .into_iter()
                    .map(|(r, b)| (r.start + at..r.end + at, b)),
            );
        }
    }
    out
}

/// The rows of output the lines `ids` make, in reading order.
fn order(lines: &[Line], mut ids: Vec<usize>, depth: usize) -> Vec<Vec<usize>> {
    // Bands: lines that overlap vertically, top to bottom.
    ids.sort_by(|&a, &b| lines[a].top().total_cmp(&lines[b].top()));
    let mut bands: Vec<Vec<usize>> = Vec::new();
    let mut bottom = f64::NEG_INFINITY;
    for id in ids {
        let l = &lines[id];
        match bands.last_mut() {
            Some(band) if l.top() < bottom => {
                band.push(id);
                bottom = bottom.max(l.bottom());
            }
            _ => {
                bands.push(vec![id]);
                bottom = l.bottom();
            }
        }
    }
    let gutters: Vec<Vec<(f64, f64)>> = bands.iter().map(|b| band_gutters(lines, b)).collect();
    let mut rows = Vec::new();
    let mut i = 0;
    while i < bands.len() {
        // The bands from i on that share a gutter: columns.
        let mut shared = gutters[i].clone();
        let mut j = i + 1;
        while j < bands.len() {
            let next = intersect(&shared, &gutters[j]);
            if next.is_empty() {
                break;
            }
            shared = next;
            j += 1;
        }
        if j - i >= 2
            && depth < 8
            && let Some(columns) = columns(lines, &bands[i..j].concat(), &shared)
        {
            for column in columns {
                rows.extend(order(lines, column, depth + 1));
            }
            i = j;
            continue;
        }
        rows.extend(band_rows(lines, &bands[i]));
        i += 1;
    }
    rows
}

/// The horizontal gaps between a band's lines at least an em wide.
fn band_gutters(lines: &[Line], band: &[usize]) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64, f64)> = band
        .iter()
        .map(|&l| (lines[l].x0, lines[l].x1, lines[l].size))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut gutters = Vec::new();
    let mut reach = f64::NEG_INFINITY;
    for (x0, x1, size) in spans {
        if reach.is_finite() && x0 - reach >= size {
            gutters.push((reach, x0));
        }
        reach = reach.max(x1);
    }
    gutters
}

/// The gaps free in both lists, three points wide at least.
fn intersect(a: &[(f64, f64)], b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for &(a0, a1) in a {
        for &(b0, b1) in b {
            let (x0, x1) = (a0.max(b0), a1.min(b1));
            if x1 - x0 >= 3.0 {
                out.push((x0, x1));
            }
        }
    }
    out
}

/// The lines split into columns at the gutters, left to right; `None`
/// when a column would be narrower than text columns are (eight ems),
/// which makes the lines a table, read a row at a time.
fn columns(lines: &[Line], ids: &[usize], gutters: &[(f64, f64)]) -> Option<Vec<Vec<usize>>> {
    let mut cuts: Vec<f64> = gutters.iter().map(|(a, b)| (a + b) / 2.0).collect();
    cuts.sort_by(f64::total_cmp);
    let mut columns = vec![Vec::new(); cuts.len() + 1];
    for &id in ids {
        let center = (lines[id].x0 + lines[id].x1) / 2.0;
        columns[cuts.iter().filter(|&&c| c < center).count()].push(id);
    }
    let mut sizes: Vec<f64> = ids.iter().map(|&l| lines[l].size).collect();
    sizes.sort_by(f64::total_cmp);
    let em = sizes[sizes.len() / 2];
    let wide = columns.iter().all(|c| {
        let x0 = c.iter().map(|&l| lines[l].x0).fold(f64::INFINITY, f64::min);
        let x1 = c
            .iter()
            .map(|&l| lines[l].x1)
            .fold(f64::NEG_INFINITY, f64::max);
        !c.is_empty() && x1 - x0 >= em * 8.0
    });
    wide.then_some(columns)
}

/// A band's rows: its lines on one baseline make a row, left to right.
fn band_rows(lines: &[Line], band: &[usize]) -> Vec<Vec<usize>> {
    let mut ids = band.to_vec();
    ids.sort_by(|&a, &b| lines[a].base.total_cmp(&lines[b].base));
    let mut rows: Vec<Vec<usize>> = Vec::new();
    for id in ids {
        let l = &lines[id];
        let same = rows.last().is_some_and(|row| {
            let r = &lines[row[0]];
            l.flat && r.flat && (l.base - r.base).abs() < l.size.max(r.size) * 0.5
        });
        match rows.last_mut() {
            Some(row) if same => row.push(id),
            _ => rows.push(vec![id]),
        }
    }
    for row in &mut rows {
        row.sort_by(|&a, &b| lines[a].x0.total_cmp(&lines[b].x0));
    }
    rows
}

/// A line's text: its glyphs left to right (in drawing order for text
/// that is not flat), a glyph drawn twice over itself (fake bold) once,
/// spaces where the glyphs stand apart.
fn line_text(glyphs: &[Placed], line: &Line) -> PageText {
    let mut ids = line.glyphs.clone();
    if line.flat {
        ids.sort_by(|&a, &b| glyphs[a].origin.x.total_cmp(&glyphs[b].origin.x));
    }
    let mut out = String::new();
    let mut boxes = Vec::new();
    let mut prev: Option<&Placed> = None;
    for i in ids {
        let g = &glyphs[i];
        if let Some(p) = prev {
            let size = p.size.max(g.size).max(0.1);
            let offset = g.origin - p.origin;
            if g.text == p.text && offset.hypot() < size * 0.1 {
                continue;
            }
            if p.dir.dot(g.origin - p.end) > size * 0.15
                && !out.ends_with(' ')
                && !g.text.starts_with(char::is_whitespace)
            {
                out.push(' ');
            }
        }
        if g.text.chars().all(char::is_whitespace) {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            let at = out.len();
            push_unligated(&mut out, &g.text);
            boxes.push((at..out.len(), glyph_box(g)));
        }
        prev = Some(g);
    }
    // Spaces are pushed only after text, so trimming the end keeps every
    // range.
    let len = out.trim_end().len();
    out.truncate(len);
    PageText { text: out, boxes }
}

/// Pushes `text` with the Latin ligatures spelled out, so that a search
/// for "find" finds "ﬁnd".
fn push_unligated(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            '\u{00A0}' => out.push(' '),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(text: &str, x: f64, y: f64, width: f64) -> Placed {
        Placed {
            text: text.into(),
            origin: Point::new(x, y),
            end: Point::new(x + width, y),
            dir: Vec2::new(1.0, 0.0),
            size: 10.0,
        }
    }

    #[test]
    fn words_and_lines() {
        let glyphs = [
            glyph("a", 0.0, 10.0, 5.0),
            glyph("b", 5.2, 10.0, 5.0),
            glyph("c", 14.0, 10.0, 5.0),
            glyph("\u{FB01}", 0.0, 24.0, 5.0),
            glyph(" ", 5.0, 24.0, 2.5),
            glyph("x", 7.5, 24.0, 5.0),
        ];
        assert_eq!(lay_out(&glyphs).text, "ab c\nfi x");
    }

    #[test]
    fn a_range_has_its_rectangles() {
        let glyphs = [
            glyph("ab", 0.0, 10.0, 10.0),
            glyph("c", 14.0, 10.0, 5.0),
            glyph("de", 0.0, 24.0, 10.0),
        ];
        let t = lay_out(&glyphs);
        assert_eq!(t.text, "ab c\nde");
        // "b c" on the first line is one rectangle from b's glyph to c's
        // (a glyph's box is whole: "ab" is one glyph); "c\nd" two.
        assert_eq!(t.rects(1..4), [[0.0, 2.0, 19.0, 10.0]]);
        assert_eq!(t.rects(3..6).len(), 2);
        assert!(t.rects(4..5).is_empty(), "the line break has no box");
    }

    #[test]
    fn a_space_drawn_over_is_no_space() {
        let glyphs = [
            glyph("h", 0.0, 10.0, 5.0),
            glyph(" ", 5.0, 10.0, 2.5),
            glyph("a", 5.0, 10.0, 5.0),
        ];
        assert_eq!(lay_out(&glyphs).text, "ha");
    }

    #[test]
    fn a_word_drawn_later_goes_where_it_stands() {
        let glyphs = [
            glyph("The", 0.0, 10.0, 15.0),
            glyph("is", 50.0, 10.0, 10.0),
            glyph("next", 0.0, 24.0, 20.0),
            glyph("_date", 18.0, 10.0, 29.0),
        ];
        assert_eq!(lay_out(&glyphs).text, "The _date is\nnext");
    }

    #[test]
    fn columns_are_read_one_after_the_other() {
        let mut glyphs = vec![glyph("Title", 0.0, 0.0, 220.0)];
        for row in 1..=3 {
            let y = 10.0 + 12.0 * row as f64;
            glyphs.push(glyph(&format!("L{row}"), 0.0, y, 100.0));
            glyphs.push(glyph(&format!("R{row}"), 120.0, y, 100.0));
        }
        assert_eq!(lay_out(&glyphs).text, "Title\nL1\nL2\nL3\nR1\nR2\nR3");
    }

    #[test]
    fn a_table_is_read_a_row_at_a_time() {
        let glyphs = [
            glyph("Name", 0.0, 10.0, 30.0),
            glyph("Bob", 0.0, 22.0, 20.0),
            glyph("Age", 60.0, 10.0, 20.0),
            glyph("3", 60.0, 22.0, 5.0),
        ];
        assert_eq!(lay_out(&glyphs).text, "Name Age\nBob 3");
    }

    #[test]
    fn fake_bold_and_superscripts() {
        let mut sup = glyph("2", 5.2, 6.0, 3.5);
        sup.size = 7.0;
        let glyphs = [
            glyph("x", 0.0, 10.0, 5.0),
            sup,
            glyph("y", 12.0, 10.0, 5.0),
            glyph("H", 0.0, 30.0, 7.0),
            glyph("H", 0.3, 30.0, 7.0),
        ];
        assert_eq!(lay_out(&glyphs).text, "x2 y\nH");
    }
}
