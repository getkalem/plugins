//! The pictures of a document drawn for Kalem (the flow's
//! `render-picture`, the docx list's WP7f): an image part of the package
//! decoded and scaled down to the size Kalem asks for. The raster formats
//! Word inserts are decoded (PNG, JPEG, GIF, BMP, TIFF, WebP); an SVG
//! picture comes with a PNG of it, which is the one the document names
//! first and the one drawn. A picture in a vector format of Windows (EMF,
//! WMF) is not decoded, and Kalem shows its alternative text instead.

use image::imageops::FilterType;
use kalem_viewer::Bitmap;

use crate::document::Document;

/// Image part `part` of `doc`, at most `max` pixels on its longer side.
pub fn render(doc: &Document, part: &str, max: u32) -> Result<Bitmap, String> {
    if part.is_empty() || part.contains("://") {
        return Err("This picture is linked, not in the document".into());
    }
    let bytes = doc
        .part_bytes(part)
        .map_err(|e| format!("The picture {part} is not in the document: {e}"))?;
    let img = image::load_from_memory(&bytes).map_err(|e| match vector(&bytes) {
        Some(kind) => format!("This picture is {kind}, which Kalem does not draw yet"),
        None => format!("The picture {part} is not read: {e}"),
    })?;
    let max = max.max(1);
    let (w, h) = (img.width(), img.height());
    let img = if w.max(h) > max {
        let k = max as f64 / w.max(h) as f64;
        let (nw, nh) = (
            ((w as f64 * k).round() as u32).max(1),
            ((h as f64 * k).round() as u32).max(1),
        );
        img.resize_exact(nw, nh, FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(Bitmap::new(w, h, rgba.into_raw()))
}

/// The vector format of Windows a picture is in, by its first bytes.
fn vector(bytes: &[u8]) -> Option<&'static str> {
    // An EMF record of type 1 (the header), with " EMF" at byte 40.
    if bytes.len() > 44 && bytes[..4] == [1, 0, 0, 0] && &bytes[40..44] == b" EMF" {
        return Some("an EMF drawing");
    }
    // A placeable WMF's key, or a WMF header.
    if bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) || bytes.starts_with(&[1, 0, 9, 0]) {
        return Some("a WMF drawing");
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_drawings_named() {
        let mut emf = vec![1, 0, 0, 0];
        emf.resize(40, 0);
        emf.extend_from_slice(b" EMF");
        emf.resize(80, 0);
        assert_eq!(super::vector(&emf), Some("an EMF drawing"));
        assert_eq!(
            super::vector(&[0xD7, 0xCD, 0xC6, 0x9A, 0, 0]),
            Some("a WMF drawing")
        );
        assert_eq!(super::vector(b"\x89PNG\r\n\x1a\n"), None);
    }
}
