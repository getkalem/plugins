//! `image-viewer`, the first plugin of the `document-viewer` contract (D54,
//! T3.7.2): pictures decoded by the `image` crate (PNG, JPEG, GIF, WebP,
//! BMP, TIFF, ICO, QOI, TGA, PNM, DDS, OpenEXR, Radiance HDR) and SVG
//! drawn by `resvg`, the EXIF orientation honored, animations as frames.
//!
//! The only edits are lossless: a JPEG is turned or flipped by rewriting
//! its EXIF orientation tag ([`jpeg::set_orientation`]); nothing is ever
//! encoded again, so nothing else is offered.

pub mod jpeg;

use std::io::Cursor;
use std::sync::Arc;

use image::metadata::Orientation;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, RgbaImage};
use kalem_viewer::{
    Bitmap, Detection, Edit, FileHandle, InfoField, RenderRequest, Rendered, Result, SaveOutput,
    Structure, Unit, UnitKind, Viewer, ViewerDocument, ViewerError,
};

/// The longest side a picture is kept at: larger ones are scaled down on
/// decode (a GPU texture's limit, and the memory budget with
/// [`MAX_PIXELS`]).
pub const MAX_SIDE: u32 = 8192;

/// The most pixels a picture is kept at, over all its frames: 40
/// megapixels, 160 MB.
pub const MAX_PIXELS: u64 = 40_000_000;

/// The extensions the viewer opens.
pub const EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "bmp", "dib", "tif", "tiff", "ico",
    "qoi", "tga", "pbm", "pgm", "ppm", "pnm", "pam", "dds", "exr", "hdr", "svg",
];

/// The image viewer.
#[derive(Debug, Default)]
pub struct ImageViewer;

fn err(e: impl std::fmt::Display) -> ViewerError {
    ViewerError(e.to_string())
}

fn is_svg(name_ext: &str, head: &[u8]) -> bool {
    if name_ext == "svg" {
        return true;
    }
    let head = String::from_utf8_lossy(&head[..head.len().min(1024)]);
    head.trim_start().starts_with("<svg") || (head.contains("<?xml") && head.contains("<svg"))
}

impl Viewer for ImageViewer {
    fn id(&self) -> &str {
        "image-viewer"
    }

    fn name(&self) -> &str {
        "Image"
    }

    fn extensions(&self) -> &[&str] {
        EXTENSIONS
    }

    fn detect(&self, name: &str, head: &[u8]) -> Detection {
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if image::guess_format(head).is_ok_and(format_supported) {
            return Detection::Magic;
        }
        if ext == "svg" && is_svg(&ext, head) {
            return Detection::Magic;
        }
        if EXTENSIONS.contains(&ext.as_str()) {
            return Detection::Extension;
        }
        Detection::No
    }

    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        let bytes = file.read_all()?;
        let ext = file.extension();
        let picture = if is_svg(&ext, &bytes) {
            decode_svg(&bytes, file.path().parent())?
        } else {
            decode(&bytes, &ext)?
        };
        let jpeg = picture.format == "jpg";
        Ok(Box::new(ImageDocument {
            name: file.name().to_string(),
            size: bytes.len() as u64,
            orientation: picture.orientation,
            saved_orientation: picture.orientation,
            bytes: if jpeg { Some(bytes) } else { None },
            picture,
        }))
    }
}

fn format_supported(f: ImageFormat) -> bool {
    use ImageFormat as F;
    matches!(
        f,
        F::Png
            | F::Jpeg
            | F::Gif
            | F::WebP
            | F::Bmp
            | F::Tiff
            | F::Ico
            | F::Qoi
            | F::Tga
            | F::Pnm
            | F::Dds
            | F::OpenExr
            | F::Hdr
    )
}

/// A picture decoded.
#[derive(Debug)]
struct Picture {
    /// The format's name (`png`, `jpg`, `svg`).
    format: String,
    /// The frames as shown (oriented, scaled to the budget), with their
    /// durations.
    frames: Vec<(Bitmap, Option<u32>)>,
    /// The size in the file, before orientation.
    width: u32,
    height: u32,
    /// The color type as stored.
    color: String,
    /// Whether an ICC profile is embedded.
    icc: bool,
    /// The raw EXIF data.
    exif: Option<Vec<u8>>,
    /// The orientation applied (EXIF 1 to 8).
    orientation: u8,
}

fn color_name(c: image::ExtendedColorType) -> String {
    use image::ExtendedColorType as C;
    let (model, bits) = match c {
        C::A8 => ("alpha", 8),
        C::L1 => ("gray", 1),
        C::La1 => ("gray and alpha", 1),
        C::Rgb1 => ("RGB", 1),
        C::Rgba1 => ("RGBA", 1),
        C::L2 => ("gray", 2),
        C::La2 => ("gray and alpha", 2),
        C::Rgb2 => ("RGB", 2),
        C::Rgba2 => ("RGBA", 2),
        C::L4 => ("gray", 4),
        C::La4 => ("gray and alpha", 4),
        C::Rgb4 => ("RGB", 4),
        C::Rgba4 => ("RGBA", 4),
        C::L8 => ("gray", 8),
        C::La8 => ("gray and alpha", 8),
        C::Rgb8 => ("RGB", 8),
        C::Rgba8 => ("RGBA", 8),
        C::L16 => ("gray", 16),
        C::La16 => ("gray and alpha", 16),
        C::Rgb16 => ("RGB", 16),
        C::Rgba16 => ("RGBA", 16),
        C::Bgr8 => ("BGR", 8),
        C::Bgra8 => ("BGRA", 8),
        C::Rgb32F => ("RGB, floating point", 32),
        C::Rgba32F => ("RGBA, floating point", 32),
        C::Cmyk8 => ("CMYK", 8),
        C::Cmyk16 => ("CMYK", 16),
        C::Unknown(bits) => ("unknown", bits),
        _ => ("unknown", 0),
    };
    if bits == 0 {
        model.to_string()
    } else {
        format!("{model}, {bits} bits per channel")
    }
}

/// The factor a `w` × `h` picture of `frames` frames is scaled by to fit
/// the budget.
fn budget_scale(w: u32, h: u32, frames: usize) -> f64 {
    let side = f64::from(MAX_SIDE) / f64::from(w.max(h).max(1));
    let pixels = (MAX_PIXELS as f64 / (f64::from(w) * f64::from(h) * frames.max(1) as f64)).sqrt();
    side.min(pixels).min(1.0)
}

fn scaled(img: DynamicImage, k: f64) -> DynamicImage {
    if k >= 1.0 {
        return img;
    }
    let w = ((f64::from(img.width()) * k).round() as u32).max(1);
    let h = ((f64::from(img.height()) * k).round() as u32).max(1);
    img.resize_exact(w, h, image::imageops::FilterType::Triangle)
}

/// Linear light of a high dynamic range picture tone-mapped to 8-bit sRGB
/// (Reinhard's operator, then the sRGB curve).
fn tone_mapped(img: &DynamicImage) -> RgbaImage {
    let f = img.to_rgba32f();
    let curve = |c: f32| {
        let c = c.max(0.0);
        let c = c / (1.0 + c);
        let s = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    RgbaImage::from_fn(f.width(), f.height(), |x, y| {
        let p = f.get_pixel(x, y);
        image::Rgba([
            curve(p[0]),
            curve(p[1]),
            curve(p[2]),
            (p[3].clamp(0.0, 1.0) * 255.0).round() as u8,
        ])
    })
}

fn to_rgba(img: DynamicImage) -> RgbaImage {
    match img {
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => tone_mapped(&img),
        DynamicImage::ImageRgba8(i) => i,
        other => other.to_rgba8(),
    }
}

fn bitmap(img: RgbaImage) -> Bitmap {
    let (w, h) = img.dimensions();
    Bitmap::new(w, h, img.into_raw())
}

/// A frame's duration in milliseconds, as browsers read it: a delay under
/// 20 ms is 100 ms.
fn duration(delay: image::Delay) -> u32 {
    let (n, d) = delay.numer_denom_ms();
    let ms = n.checked_div(d).unwrap_or(0);
    if ms < 20 { 100 } else { ms }
}

fn decode(bytes: &[u8], ext: &str) -> Result<Picture> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(err)?;
    if reader.format().is_none() {
        reader.set_format(ImageFormat::from_extension(ext).ok_or_else(|| err("not a picture"))?);
    }
    let format = reader.format().expect("set above");
    reader.limits(limits_big());
    let mut decoder = reader.into_decoder().map_err(err)?;
    let (width, height) = decoder.dimensions();
    let color = color_name(decoder.original_color_type());
    let icc = decoder.icc_profile().ok().flatten().is_some();
    let exif = decoder.exif_metadata().ok().flatten();
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    drop(decoder);
    let frames = match animated_frames(bytes, format) {
        Some(frames) => frames,
        None => {
            let mut reader = image::ImageReader::new(Cursor::new(bytes));
            reader.set_format(format);
            reader.limits(limits_big());
            let img = reader.decode().map_err(err)?;
            vec![(img, None)]
        }
    };
    let k = budget_scale(width, height, frames.len());
    let frames = frames
        .into_iter()
        .map(|(img, ms)| {
            let mut img = scaled(img, k);
            img.apply_orientation(orientation);
            (bitmap(to_rgba(img)), ms)
        })
        .collect();
    Ok(Picture {
        format: format.extensions_str()[0].to_string(),
        frames,
        width,
        height,
        color,
        icc,
        exif,
        orientation: orientation.to_exif(),
    })
}

/// Decoding needs the whole picture once; it is scaled down after.
fn limits_big() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(2 << 30);
    limits
}

/// The frames of an animated GIF, WebP or PNG; `None` for a still
/// picture.
fn animated_frames(bytes: &[u8], format: ImageFormat) -> Option<Vec<(DynamicImage, Option<u32>)>> {
    let frames = match format {
        ImageFormat::Gif => image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
            .ok()?
            .into_frames()
            .collect_frames()
            .ok()?,
        ImageFormat::WebP => {
            let d = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
            if !d.has_animation() {
                return None;
            }
            d.into_frames().collect_frames().ok()?
        }
        ImageFormat::Png => {
            let d = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).ok()?;
            if !d.is_apng().ok()? {
                return None;
            }
            d.apng().ok()?.into_frames().collect_frames().ok()?
        }
        _ => return None,
    };
    if frames.len() < 2 {
        return None;
    }
    Some(
        frames
            .into_iter()
            .map(|f| {
                let ms = duration(f.delay());
                (DynamicImage::ImageRgba8(f.into_buffer()), Some(ms))
            })
            .collect(),
    )
}

fn decode_svg(bytes: &[u8], dir: Option<&std::path::Path>) -> Result<Picture> {
    let opts = resvg::usvg::Options {
        resources_dir: dir.map(std::path::Path::to_path_buf),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &opts).map_err(err)?;
    let size = tree.size();
    let (w, h) = (
        size.width().ceil().max(1.0) as u32,
        size.height().ceil().max(1.0) as u32,
    );
    // Drawn at its own size, larger for a small drawing, within the
    // budget.
    let (fw, fh) = (f64::from(w), f64::from(h));
    let grow = (1024.0 / fw.max(fh)).max(1.0);
    let limit = (f64::from(MAX_SIDE) / fw.max(fh)).min((MAX_PIXELS as f64 / (fw * fh)).sqrt());
    let k = grow.min(limit) as f32;
    let (pw, ph) = (
        ((w as f32) * k).round().max(1.0) as u32,
        ((h as f32) * k).round().max(1.0) as u32,
    );
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(pw, ph).ok_or_else(|| err("an empty picture"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(k, k),
        &mut pixmap.as_mut(),
    );
    // tiny-skia keeps premultiplied alpha.
    let mut rgba = pixmap.take();
    for p in rgba.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(Picture {
        format: "svg".into(),
        frames: vec![(Bitmap::new(pw, ph, rgba), None)],
        width: w,
        height: h,
        color: "vector".into(),
        icc: false,
        exif: None,
        orientation: 1,
    })
}

/// The EXIF orientation that shows a picture as `op` applied to it shown
/// with `current`. Found by trying each of the eight on a picture whose
/// pixels all differ.
fn compose(current: u8, op: Orientation) -> u8 {
    let probe = DynamicImage::ImageRgba8(RgbaImage::from_fn(3, 2, |x, y| {
        image::Rgba([(y * 3 + x) as u8, 0, 0, 255])
    }));
    let mut want = probe.clone();
    want.apply_orientation(Orientation::from_exif(current).unwrap_or(Orientation::NoTransforms));
    want.apply_orientation(op);
    (1..=8)
        .find(|&o| {
            let mut p = probe.clone();
            p.apply_orientation(Orientation::from_exif(o).expect("1 to 8"));
            p == want
        })
        .unwrap_or(1)
}

/// The edits of a JPEG, and what each does to the picture shown.
const EDITS: &[(&str, &str, &str, Orientation)] = &[
    (
        "rotate-right",
        "Rotate Right",
        "rotate-left",
        Orientation::Rotate90,
    ),
    (
        "rotate-left",
        "Rotate Left",
        "rotate-right",
        Orientation::Rotate270,
    ),
    (
        "flip-horizontal",
        "Flip Horizontally",
        "flip-horizontal",
        Orientation::FlipHorizontal,
    ),
    (
        "flip-vertical",
        "Flip Vertically",
        "flip-vertical",
        Orientation::FlipVertical,
    ),
];

fn orientation_name(o: u8) -> &'static str {
    match o {
        1 => "normal",
        2 => "flipped horizontally",
        3 => "turned 180°",
        4 => "flipped vertically",
        5 => "transposed",
        6 => "turned 90° clockwise",
        7 => "transversed",
        8 => "turned 90° counter-clockwise",
        _ => "unknown",
    }
}

/// An image file opened.
#[derive(Debug)]
pub struct ImageDocument {
    name: String,
    size: u64,
    picture: Picture,
    orientation: u8,
    saved_orientation: u8,
    /// The file's bytes, for a JPEG, which the edits rewrite.
    bytes: Option<Vec<u8>>,
}

impl ImageDocument {
    /// The orientation the picture is shown with (EXIF 1 to 8).
    pub fn orientation(&self) -> u8 {
        self.orientation
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["bytes", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1000.0 && u + 1 < UNITS.len() {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

impl ViewerDocument for ImageDocument {
    fn structure(&self) -> Structure {
        let animated = self.picture.frames.len() > 1;
        Structure {
            units: self
                .picture
                .frames
                .iter()
                .enumerate()
                .map(|(i, (_, ms))| Unit {
                    kind: if animated {
                        UnitKind::Frame
                    } else {
                        UnitKind::Image
                    },
                    label: if animated {
                        format!("{}", i + 1)
                    } else {
                        self.name.clone()
                    },
                    duration_ms: *ms,
                })
                .collect(),
            outline: Vec::new(),
        }
    }

    fn render(&mut self, unit: usize, request: RenderRequest) -> Result<Rendered> {
        let (b, _) = self
            .picture
            .frames
            .get(unit)
            .ok_or_else(|| err(format!("no frame {unit}")))?;
        if request.scale >= 0.999 {
            return Ok(Rendered::Bitmap(b.clone()));
        }
        let img = RgbaImage::from_raw(b.width, b.height, b.rgba.to_vec())
            .ok_or_else(|| err("a broken bitmap"))?;
        let img = scaled(
            DynamicImage::ImageRgba8(img),
            f64::from(request.scale.max(0.001)),
        );
        Ok(Rendered::Bitmap(bitmap(img.to_rgba8())))
    }

    fn text(&self, _unit: usize) -> String {
        let (w, h) = self.shown_size();
        format!(
            "{}: {} picture, {w} × {h} pixels",
            self.name,
            self.picture.format.to_uppercase()
        )
    }

    fn info(&self) -> Vec<InfoField> {
        let p = &self.picture;
        let (w, h) = self.shown_size();
        let mut out = vec![
            InfoField::new("Format", p.format.to_uppercase()),
            InfoField::new("Size", format!("{w} × {h} pixels")),
            InfoField::new("File size", human_size(self.size)),
            InfoField::new("Color", p.color.clone()),
            InfoField::new("ICC profile", if p.icc { "embedded" } else { "none" }),
        ];
        let (bw, bh) = p
            .frames
            .first()
            .map_or((0, 0), |(b, _)| (b.width, b.height));
        if (bw, bh) != (w, h) && p.format != "svg" {
            out.push(InfoField::new("Shown at", format!("{bw} × {bh} pixels")));
        }
        if p.frames.len() > 1 {
            let total: u32 = p.frames.iter().filter_map(|(_, ms)| *ms).sum();
            out.push(InfoField::new(
                "Frames",
                format!("{}, {:.1} s", p.frames.len(), f64::from(total) / 1000.0),
            ));
        }
        if self.orientation != 1 || p.exif.is_some() {
            out.push(InfoField::new(
                "Orientation",
                orientation_name(self.orientation),
            ));
        }
        if let Some(raw) = &p.exif
            && let Ok(e) = exif::Reader::new().read_raw(raw.clone())
        {
            use exif::Tag as T;
            for (tag, label) in [
                (T::Make, "Camera make"),
                (T::Model, "Camera model"),
                (T::LensModel, "Lens"),
                (T::DateTimeOriginal, "Taken"),
                (T::ExposureTime, "Exposure"),
                (T::FNumber, "Aperture"),
                (T::PhotographicSensitivity, "ISO"),
                (T::FocalLength, "Focal length"),
                (T::ColorSpace, "Color space"),
                (T::GPSLatitude, "Latitude"),
                (T::GPSLongitude, "Longitude"),
                (T::Software, "Software"),
                (T::Artist, "Artist"),
                (T::Copyright, "Copyright"),
            ] {
                let field = e
                    .get_field(tag, exif::In::PRIMARY)
                    .or_else(|| e.fields().find(|f| f.tag == tag));
                if let Some(f) = field {
                    let v = f.display_value().with_unit(&e).to_string();
                    out.push(InfoField::new(label, v.trim_matches('"').to_string()));
                }
            }
        }
        out
    }

    fn edits(&self, _unit: usize) -> Vec<Edit> {
        if self.bytes.is_none() {
            return Vec::new();
        }
        EDITS
            .iter()
            .map(|(id, title, inverse, _)| Edit {
                id: (*id).into(),
                title: (*title).into(),
                inverse: Some((*inverse).into()),
            })
            .collect()
    }

    fn apply(&mut self, edit: &str) -> Result<Vec<usize>> {
        if self.bytes.is_none() {
            return Err(err("Only a JPEG is turned without encoding it again"));
        }
        let (_, _, _, op) = EDITS
            .iter()
            .find(|(id, ..)| *id == edit)
            .ok_or_else(|| err(format!("No edit {edit}")))?;
        self.orientation = compose(self.orientation, *op);
        for (b, _) in &mut self.picture.frames {
            let img = RgbaImage::from_raw(b.width, b.height, b.rgba.to_vec())
                .ok_or_else(|| err("a broken bitmap"))?;
            let mut img = DynamicImage::ImageRgba8(img);
            img.apply_orientation(*op);
            *b = bitmap(img.to_rgba8());
        }
        Ok((0..self.picture.frames.len()).collect())
    }

    fn modified(&self) -> bool {
        self.orientation != self.saved_orientation
    }

    fn save(&mut self) -> Result<SaveOutput> {
        let bytes = self
            .bytes
            .as_ref()
            .ok_or_else(|| err("Only a JPEG's orientation is saved"))?;
        let out = jpeg::set_orientation(bytes, u16::from(self.orientation)).map_err(err)?;
        self.bytes = Some(out.clone());
        self.saved_orientation = self.orientation;
        self.size = out.len() as u64;
        Ok(SaveOutput {
            bytes: out,
            losses: Vec::new(),
        })
    }
}

impl ImageDocument {
    /// The picture's size as shown: its stored size, turned by its
    /// orientation.
    fn shown_size(&self) -> (u32, u32) {
        let (w, h) = (self.picture.width, self.picture.height);
        if (5..=8).contains(&self.orientation) {
            (h, w)
        } else {
            (w, h)
        }
    }
}

/// The viewer as the host registers it.
pub fn viewer() -> Arc<dyn Viewer> {
    Arc::new(ImageViewer)
}
