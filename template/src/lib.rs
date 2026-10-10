//! Template: a Kalem plugin that opens `.template` files.
//!
//! A viewer written against Kalem's Rust contract (`kalem_viewer`): the
//! viewer says which files are its own and opens one; the document it
//! returns says what units the file has (a picture, pages, frames, sheets)
//! and draws each when asked, as pixels Kalem shows. The crate runs
//! natively in its tests, and as a WebAssembly component in Kalem (`kalem
//! plugin build`, `kalem plugin dev`), where it sees nothing but the file
//! Kalem hands it.
//!
//! A viewer's format is one that is not text: Kalem opens a text file in
//! a mode, as text to edit, never in a viewer. As it starts, a file is a
//! picture whose first bytes are `DOTS` and a NUL byte (`MAGIC`, which
//! `plugin.json` declares in `applies`), then its rows, `#` a dark cell
//! and `.` a light one, a line a row, as
//! `printf 'DOTS\0.##.\n#..#\n.##.\n' > a.template` writes one:
//!
//! ```text
//! .##.
//! #..#
//! .##.
//! ```
//!
//! Replace `MAGIC`, `Doc`, `parse` and the drawing with your format, and
//! the first bytes `plugin.json` declares with its own.

use kalem_viewer::{
    Bitmap, Detection, FileHandle, InfoField, RenderRequest, Rendered, Result, Structure, Unit,
    UnitKind, Viewer, ViewerDocument, ViewerError,
};

/// The viewer: one for the whole plugin, with no state of its own.
#[derive(Debug)]
pub struct TemplateViewer;

/// An open file: its rows of cells.
struct Doc {
    rows: Vec<Vec<bool>>,
    name: String,
}

/// A cell's side in pixels at scale 1.
const CELL: f32 = 8.0;

/// The format's first bytes: a NUL, which no text has, makes Kalem take
/// the file for one that is not text. `plugin.json` declares them as
/// pairs of hexadecimal digits (`"applies": {"magic": ["44 4F 54 53 00"]}`),
/// so that a file of the format opens whatever its name.
const MAGIC: &[u8] = b"DOTS\0";

impl Viewer for TemplateViewer {
    fn id(&self) -> &str {
        "template"
    }

    fn name(&self) -> &str {
        "Template"
    }

    fn extensions(&self) -> &[&str] {
        &["template"]
    }

    /// Whether `name`, starting with `head` (its first bytes), is a file of
    /// this viewer: by its content when the format has a signature, else
    /// by its extension. Kalem does not ask it: `plugin.json` declares the
    /// same (`opens`, `applies`).
    fn detect(&self, name: &str, head: &[u8]) -> Detection {
        if head.starts_with(MAGIC) {
            return Detection::Magic;
        }
        let ext = name.rsplit('.').next().unwrap_or_default();
        if ext.eq_ignore_ascii_case("template") {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    /// Reads the file (`read_all`, or `read_at` for a piece of a large
    /// one) and makes the document.
    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        Ok(Box::new(Doc {
            rows: parse(&file.read_all()?)?,
            name: file.name().to_owned(),
        }))
    }
}

/// The rows of a picture after its first bytes, `#` dark and `.` light,
/// each as wide as the widest.
fn parse(bytes: &[u8]) -> Result<Vec<Vec<bool>>> {
    let Some(body) = bytes.strip_prefix(MAGIC) else {
        return Err(ViewerError("Not a picture: no DOTS at its start".into()));
    };
    let text = String::from_utf8_lossy(body);
    if let Some(c) = text.chars().find(|c| !matches!(c, '#' | '.' | '\n' | '\r')) {
        return Err(ViewerError(format!("Not a picture: {c:?} in it")));
    }
    let rows: Vec<Vec<bool>> = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.chars().map(|c| c == '#').collect())
        .collect();
    if rows.is_empty() {
        return Err(ViewerError("An empty picture".into()));
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    Ok(rows
        .into_iter()
        .map(|mut r| {
            r.resize(width, false);
            r
        })
        .collect())
}

impl ViewerDocument for Doc {
    /// The units: here one picture; pages, frames or sheets for other
    /// formats.
    fn structure(&self) -> Structure {
        Structure {
            units: vec![Unit {
                kind: UnitKind::Image,
                label: "1".into(),
                duration_ms: None,
            }],
            outline: Vec::new(),
        }
    }

    /// A unit's size at scale 1, in pixels.
    fn size(&self, _unit: usize) -> Option<(f32, f32)> {
        Some((
            self.rows[0].len() as f32 * CELL,
            self.rows.len() as f32 * CELL,
        ))
    }

    /// The unit as pixels at the scale asked, in the theme's colors.
    fn render(&mut self, _unit: usize, request: RenderRequest) -> Result<Rendered> {
        let cell = (CELL * request.scale).max(1.0) as u32;
        let (w, h) = (
            self.rows[0].len() as u32 * cell,
            self.rows.len() as u32 * cell,
        );
        let (dark, light) = (request.theme.foreground, request.theme.background);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let on = self.rows[(y / cell) as usize][(x / cell) as usize];
                let [r, g, b] = if on { dark } else { light };
                rgba.extend([r, g, b, 255]);
            }
        }
        Ok(Rendered::Bitmap(Bitmap::new(w, h, rgba)))
    }

    /// The unit's text, for search, copying and the terminal: the rows as
    /// written.
    fn text(&self, _unit: usize) -> String {
        self.rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|&c| if c { '#' } else { '.' })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The information panel.
    fn info(&self) -> Vec<InfoField> {
        vec![
            InfoField::new("Name", self.name.clone()),
            InfoField::new(
                "Size",
                format!("{} × {} cells", self.rows[0].len(), self.rows.len()),
            ),
        ]
    }
}

// The component's exports, written by `kalem-plugin`'s adapter over the
// contract: nothing to add here.
#[cfg(target_arch = "wasm32")]
kalem_plugin::export_viewer_of!(TemplateViewer);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_read_and_drawn() {
        let file = b"DOTS\0.#\n#.\n";
        assert_eq!(TemplateViewer.detect("a", file), Detection::Magic);
        let rows = parse(file).unwrap();
        assert_eq!(rows, vec![vec![false, true], vec![true, false]]);
        let mut doc = Doc {
            rows,
            name: "a.template".into(),
        };
        let request = RenderRequest {
            scale: 0.125,
            ..RenderRequest::default()
        };
        let Rendered::Bitmap(b) = doc.render(0, request).unwrap();
        assert_eq!((b.width, b.height), (2, 2));
        assert_eq!(doc.text(0), ".#\n#.");
        assert!(parse(b"DOTS\0x").is_err());
        assert!(parse(b".#\n#.\n").is_err(), "text, without the first bytes");
    }

    /// Kalem knows a file by what `plugin.json` declares, not by `detect`.
    #[test]
    fn the_manifest_declares_the_first_bytes() {
        let hex: Vec<String> = MAGIC.iter().map(|b| format!("{b:02X}")).collect();
        let manifest = include_str!("../plugin.json");
        assert!(manifest.contains(&hex.join(" ")), "applies.magic: {hex:?}");
    }
}
