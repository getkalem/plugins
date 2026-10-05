//! One file per format, made here so no picture's license needs
//! recording, opened through the contract and compared with the pixels it
//! was made from.

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use kalem_plugin_image_viewer::ImageViewer;
use kalem_viewer::{
    Detection, FileHandle, RenderRequest, Rendered, UnitKind, Viewer, ViewerDocument,
};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("image-viewer-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A 6 × 4 picture whose pixels differ.
fn source() -> RgbaImage {
    RgbaImage::from_fn(6, 4, |x, y| Rgba([x as u8 * 40, y as u8 * 60, 200, 255]))
}

fn open(path: &Path) -> Box<dyn ViewerDocument> {
    let head = std::fs::read(path).unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    assert_ne!(
        ImageViewer.detect(name, &head[..head.len().min(4096)]),
        Detection::No,
        "{name}"
    );
    ImageViewer.open(FileHandle::new(path)).unwrap()
}

fn pixels(doc: &mut Box<dyn ViewerDocument>, unit: usize) -> RgbaImage {
    let Rendered::Bitmap(b) = doc.render(unit, RenderRequest::default()).unwrap();
    RgbaImage::from_raw(b.width, b.height, b.rgba.to_vec()).unwrap()
}

/// Whether two pictures agree within `tolerance` per channel.
fn close(a: &RgbaImage, b: &RgbaImage, tolerance: u8) -> bool {
    a.dimensions() == b.dimensions()
        && a.pixels()
            .zip(b.pixels())
            .all(|(p, q)| (0..4).all(|i| p[i].abs_diff(q[i]) <= tolerance))
}

#[test]
fn every_lossless_format_decodes_to_its_pixels() {
    let d = dir("lossless");
    let src = source();
    let rgb = DynamicImage::ImageRgba8(src.clone()).to_rgb8();
    for (name, format, alpha) in [
        ("a.png", ImageFormat::Png, true),
        ("a.bmp", ImageFormat::Bmp, true),
        ("a.tiff", ImageFormat::Tiff, true),
        ("a.ico", ImageFormat::Ico, true),
        ("a.qoi", ImageFormat::Qoi, true),
        ("a.tga", ImageFormat::Tga, true),
        ("a.ppm", ImageFormat::Pnm, false),
        ("a.webp", ImageFormat::WebP, true),
    ] {
        let p = d.join(name);
        if alpha {
            src.save_with_format(&p, format).unwrap();
        } else {
            rgb.save_with_format(&p, format).unwrap();
        }
        let mut doc = open(&p);
        let s = doc.structure();
        assert_eq!(s.units.len(), 1, "{name}");
        assert_eq!(s.units[0].kind, UnitKind::Image);
        assert!(close(&pixels(&mut doc, 0), &src, 0), "{name}");
        assert!(doc.edits(0).is_empty(), "{name}: only a JPEG is edited");
    }
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn lossy_and_high_dynamic_range_formats() {
    let d = dir("lossy");
    let src = source();
    let rgb = DynamicImage::ImageRgba8(src.clone()).to_rgb8();
    let p = d.join("a.jpg");
    rgb.save_with_format(&p, ImageFormat::Jpeg).unwrap();
    let mut doc = open(&p);
    let got = pixels(&mut doc, 0);
    assert_eq!(got.dimensions(), (6, 4));
    assert_eq!(doc.edits(0).len(), 4);
    // Linear light, tone-mapped: the picture keeps its size and order of
    // brightness.
    let f = DynamicImage::ImageRgba8(src.clone()).to_rgb32f();
    for (name, format) in [("a.exr", ImageFormat::OpenExr), ("a.hdr", ImageFormat::Hdr)] {
        let p = d.join(name);
        DynamicImage::ImageRgb32F(f.clone())
            .save_with_format(&p, format)
            .unwrap();
        let mut doc = open(&p);
        let got = pixels(&mut doc, 0);
        assert_eq!(got.dimensions(), (6, 4), "{name}");
        assert!(got.get_pixel(5, 0)[0] > got.get_pixel(0, 0)[0], "{name}");
        assert!(got.get_pixel(0, 3)[1] > got.get_pixel(0, 0)[1], "{name}");
    }
    std::fs::remove_dir_all(d).ok();
}

/// A DXT1 DDS file of one 4 × 4 block in one color.
fn dxt1(rgb565: u16) -> Vec<u8> {
    let mut f = b"DDS ".to_vec();
    let u32le = |f: &mut Vec<u8>, v: u32| f.extend_from_slice(&v.to_le_bytes());
    u32le(&mut f, 124);
    // Caps, height, width, pixel format; linear size.
    u32le(&mut f, 0x1 | 0x2 | 0x4 | 0x1000 | 0x80000);
    u32le(&mut f, 4);
    u32le(&mut f, 4);
    u32le(&mut f, 8);
    u32le(&mut f, 0);
    u32le(&mut f, 0);
    for _ in 0..11 {
        u32le(&mut f, 0);
    }
    // The pixel format: a FourCC.
    u32le(&mut f, 32);
    u32le(&mut f, 0x4);
    f.extend_from_slice(b"DXT1");
    for _ in 0..5 {
        u32le(&mut f, 0);
    }
    u32le(&mut f, 0x1000);
    for _ in 0..4 {
        u32le(&mut f, 0);
    }
    assert_eq!(f.len(), 128);
    // Both end colors the same, every index 0.
    f.extend_from_slice(&rgb565.to_le_bytes());
    f.extend_from_slice(&rgb565.to_le_bytes());
    f.extend_from_slice(&[0; 4]);
    f
}

#[test]
fn a_dds_texture() {
    let d = dir("dds");
    let p = d.join("a.dds");
    // Pure red in 5:6:5.
    std::fs::write(&p, dxt1(0xF800)).unwrap();
    let mut doc = open(&p);
    let got = pixels(&mut doc, 0);
    assert_eq!(got.dimensions(), (4, 4));
    assert_eq!(got.get_pixel(2, 2).0, [255, 0, 0, 255]);
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn an_animated_gif_is_frames() {
    use image::codecs::gif::{GifEncoder, Repeat};
    let d = dir("gif");
    let p = d.join("a.gif");
    {
        let file = std::fs::File::create(&p).unwrap();
        let mut enc = GifEncoder::new(file);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for c in [[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]] {
            let frame = image::Frame::from_parts(
                RgbaImage::from_pixel(4, 4, Rgba(c)),
                0,
                0,
                image::Delay::from_numer_denom_ms(200, 1),
            );
            enc.encode_frame(frame).unwrap();
        }
    }
    let mut doc = open(&p);
    let s = doc.structure();
    assert!(s.animated());
    assert_eq!(s.units.len(), 3);
    assert_eq!(s.units[1].duration_ms, Some(200));
    assert_eq!(pixels(&mut doc, 1).get_pixel(0, 0).0, [0, 0, 255, 255]);
    let info = doc.info();
    assert!(
        info.iter()
            .any(|f| f.label == "Frames" && f.value.starts_with("3,"))
    );
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn an_animation_over_the_budget_keeps_every_frame_smaller() {
    use image::codecs::gif::GifEncoder;
    use kalem_plugin_image_viewer::MAX_PIXELS;
    let d = dir("gif-budget");
    let p = d.join("a.gif");
    // Twelve frames of 2000 × 2000 are 48 million pixels, over the budget;
    // each after the first is one pixel, so they are quick to make.
    {
        let mut enc = GifEncoder::new(std::fs::File::create(&p).unwrap());
        for i in 0..12 {
            let size = if i == 0 { 2000 } else { 1 };
            let frame = image::Frame::from_parts(
                RgbaImage::from_pixel(size, size, Rgba([255, 0, 0, 255])),
                0,
                0,
                image::Delay::from_numer_denom_ms(100, 1),
            );
            enc.encode_frame(frame).unwrap();
        }
    }
    let mut doc = open(&p);
    assert_eq!(doc.structure().units.len(), 12);
    let last = pixels(&mut doc, 11);
    assert!(last.width() < 2000 && last.width() == last.height());
    assert!(u64::from(last.width()) * u64::from(last.height()) * 12 <= MAX_PIXELS);
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn svg() {
    let d = dir("svg");
    let p = d.join("a.svg");
    std::fs::write(
        &p,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="20" height="20" fill="red"/></svg>"#,
    )
    .unwrap();
    let mut doc = open(&p);
    let got = pixels(&mut doc, 0);
    // A small drawing is drawn larger, keeping its shape.
    assert_eq!(got.width(), 2 * got.height());
    assert!(got.width() >= 1024);
    assert_eq!(got.get_pixel(1, 1).0, [255, 0, 0, 255]);
    assert_eq!(got.get_pixel(got.width() - 2, 1)[3], 0);
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn the_exif_orientation_is_honored_and_a_turn_rewrites_the_tag_only() {
    let d = dir("orientation");
    let rgb = DynamicImage::ImageRgba8(source()).to_rgb8();
    let mut plain = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 100)
        .encode_image(&rgb)
        .unwrap();
    let p = d.join("photo.jpg");
    // Stored 6 × 4, to be shown turned clockwise.
    let original = kalem_plugin_image_viewer::jpeg::set_orientation(&plain, 6).unwrap();
    std::fs::write(&p, &original).unwrap();
    let mut doc = open(&p);
    assert_eq!(pixels(&mut doc, 0).dimensions(), (4, 6));
    assert!(doc.info().iter().any(|f| f.value == "turned 90° clockwise"));

    // Turned back: shown as stored.
    assert!(!doc.modified());
    doc.apply("rotate-left").unwrap();
    assert!(doc.modified());
    assert_eq!(pixels(&mut doc, 0).dimensions(), (6, 4));
    // The inverse undoes it.
    let edit = doc
        .edits(0)
        .into_iter()
        .find(|e| e.id == "rotate-left")
        .unwrap();
    doc.apply(edit.inverse.as_deref().unwrap()).unwrap();
    assert!(!doc.modified());
    doc.apply("rotate-left").unwrap();
    let saved = doc.save().unwrap();
    assert!(saved.losses.is_empty());
    assert!(!doc.modified());
    assert_eq!(saved.bytes.len(), original.len());
    let differ = (0..original.len())
        .filter(|&i| original[i] != saved.bytes[i])
        .count();
    assert_eq!(differ, 1, "the orientation tag only");
    assert_eq!(
        kalem_plugin_image_viewer::jpeg::orientation(&saved.bytes),
        Some(1)
    );

    // The eight orientations, each shown upright.
    for o in 1..=8u16 {
        let bytes = kalem_plugin_image_viewer::jpeg::set_orientation(&plain, o).unwrap();
        let p = d.join(format!("o{o}.jpg"));
        std::fs::write(&p, &bytes).unwrap();
        let mut doc = open(&p);
        let mut want = DynamicImage::ImageRgb8(image::load_from_memory(&plain).unwrap().to_rgb8());
        want.apply_orientation(image::metadata::Orientation::from_exif(o as u8).unwrap());
        assert!(
            close(&pixels(&mut doc, 0), &want.to_rgba8(), 0),
            "orientation {o}"
        );
    }
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn a_picture_past_the_budget_is_scaled_down_on_decode() {
    let d = dir("large");
    let p = d.join("large.png");
    // 50 megapixels, 10,000 pixels wide.
    image::GrayImage::from_pixel(10_000, 5_000, image::Luma([128]))
        .save(&p)
        .unwrap();
    let mut doc = open(&p);
    let got = pixels(&mut doc, 0);
    assert!(got.width() <= kalem_plugin_image_viewer::MAX_SIDE);
    assert!(
        u64::from(got.width()) * u64::from(got.height()) <= kalem_plugin_image_viewer::MAX_PIXELS
    );
    assert_eq!(got.width(), 2 * got.height());
    let info = doc.info();
    assert!(info.iter().any(|f| f.value == "10000 × 5000 pixels"));
    assert!(info.iter().any(|f| f.label == "Shown at"));
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn detection() {
    let png = {
        let mut b = Vec::new();
        source()
            .write_to(&mut std::io::Cursor::new(&mut b), ImageFormat::Png)
            .unwrap();
        b
    };
    assert_eq!(ImageViewer.detect("x.bin", &png), Detection::Magic);
    assert_eq!(
        ImageViewer.detect("x.tga", b"\0\0\x02"),
        Detection::Extension
    );
    assert_eq!(ImageViewer.detect("x.txt", b"hello"), Detection::No);
    assert_eq!(
        ImageViewer.detect("x.svg", b"<?xml version=\"1.0\"?><svg>"),
        Detection::Magic
    );
}

#[test]
fn a_broken_file_is_an_error() {
    let d = dir("broken");
    let p = d.join("a.png");
    std::fs::write(&p, b"\x89PNG\r\n\x1a\nnot really").unwrap();
    assert!(ImageViewer.open(FileHandle::new(&p)).is_err());
    std::fs::remove_dir_all(d).ok();
}
