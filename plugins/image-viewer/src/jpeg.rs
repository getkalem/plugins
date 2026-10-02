//! The EXIF orientation tag of a JPEG file written without touching
//! anything else (CIPA DC-008, the Exif standard; TIFF 6.0 for the IFD):
//! a turn or a flip of a photograph is a lossless edit, the compressed
//! picture never decoded and encoded again.

/// The orientation tag (`Orientation`, 0x0112) in IFD0.
const ORIENTATION: u16 = 0x0112;
/// The TIFF type SHORT.
const SHORT: u16 = 3;
/// The TIFF type LONG.
const LONG: u16 = 4;

#[derive(Clone, Copy)]
struct Order {
    big: bool,
}

impl Order {
    fn u16(self, b: &[u8], at: usize) -> Option<u16> {
        let s: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
        Some(if self.big {
            u16::from_be_bytes(s)
        } else {
            u16::from_le_bytes(s)
        })
    }

    fn u32(self, b: &[u8], at: usize) -> Option<u32> {
        let s: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        Some(if self.big {
            u32::from_be_bytes(s)
        } else {
            u32::from_le_bytes(s)
        })
    }

    fn put_u16(self, v: u16) -> [u8; 2] {
        if self.big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }

    fn put_u32(self, v: u32) -> [u8; 4] {
        if self.big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }
}

/// A segment of a JPEG file before its picture data: where its marker
/// starts, where it ends, its marker.
struct Segment {
    start: usize,
    end: usize,
    marker: u8,
}

/// The segments of `jpeg` before the first scan.
fn segments(jpeg: &[u8]) -> Result<Vec<Segment>, String> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG file".into());
    }
    let mut out = Vec::new();
    let mut pos = 2;
    loop {
        if jpeg.get(pos) != Some(&0xFF) {
            return Err(format!("a broken JPEG segment at byte {pos}"));
        }
        let start = pos;
        // Fill bytes.
        while jpeg.get(pos + 1) == Some(&0xFF) {
            pos += 1;
        }
        let marker = *jpeg.get(pos + 1).ok_or("a JPEG file cut short")?;
        if marker == 0xDA || marker == 0xD9 {
            return Ok(out);
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            pos += 2;
            continue;
        }
        let len = u16::from_be_bytes(
            jpeg.get(pos + 2..pos + 4)
                .ok_or("a JPEG file cut short")?
                .try_into()
                .map_err(|_| "a JPEG file cut short")?,
        ) as usize;
        let end = pos + 2 + len;
        if len < 2 || end > jpeg.len() {
            return Err("a JPEG segment longer than the file".into());
        }
        out.push(Segment { start, end, marker });
        pos = end;
    }
}

const EXIF: &[u8] = b"Exif\0\0";

/// The Exif segment's TIFF data: the segment and the data's offset.
fn exif_segment<'a>(jpeg: &[u8], segs: &'a [Segment]) -> Option<(&'a Segment, usize)> {
    segs.iter().find_map(|s| {
        // The marker, the length, then the identifier.
        let data = s.start + 4;
        (s.marker == 0xE1 && jpeg.get(data..data + EXIF.len()) == Some(EXIF))
            .then_some((s, data + EXIF.len()))
    })
}

/// The orientation a JPEG file's EXIF records (1 to 8), or `None`.
pub fn orientation(jpeg: &[u8]) -> Option<u16> {
    let segs = segments(jpeg).ok()?;
    let (seg, tiff_at) = exif_segment(jpeg, &segs)?;
    let tiff = &jpeg[tiff_at..seg.end];
    let (order, ifd) = header(tiff)?;
    let n = order.u16(tiff, ifd)? as usize;
    (0..n).find_map(|i| {
        let e = ifd + 2 + 12 * i;
        (order.u16(tiff, e)? == ORIENTATION).then(|| match order.u16(tiff, e + 2) {
            Some(LONG) => order.u32(tiff, e + 8).map(|v| v as u16),
            _ => order.u16(tiff, e + 8),
        })?
    })
}

/// The byte order and IFD0's offset of TIFF data.
fn header(tiff: &[u8]) -> Option<(Order, usize)> {
    let order = match tiff.get(..2)? {
        b"II" => Order { big: false },
        b"MM" => Order { big: true },
        _ => return None,
    };
    if order.u16(tiff, 2)? != 42 {
        return None;
    }
    Some((order, order.u32(tiff, 4)? as usize))
}

/// `jpeg` with its EXIF orientation set to `value` (1 to 8).
///
/// When the tag is there, its two bytes are the only ones that change.
/// When IFD0 lacks it, a copy of IFD0 with the tag added is appended to the
/// TIFF data and the header points to it: every other offset stays valid,
/// since nothing moves. When the file has no EXIF, a segment holding only
/// the tag is inserted after the JFIF segment (or after the start of the
/// file).
pub fn set_orientation(jpeg: &[u8], value: u16) -> Result<Vec<u8>, String> {
    if !(1..=8).contains(&value) {
        return Err(format!("no orientation {value}"));
    }
    let segs = segments(jpeg)?;
    let Some((seg, tiff_at)) = exif_segment(jpeg, &segs) else {
        return Ok(insert_exif(jpeg, &segs, value));
    };
    let tiff = &jpeg[tiff_at..seg.end];
    let (order, ifd) = header(tiff).ok_or("a broken EXIF header")?;
    let n = order.u16(tiff, ifd).ok_or("a broken EXIF directory")? as usize;
    if ifd + 2 + 12 * n + 4 > tiff.len() {
        return Err("a broken EXIF directory".into());
    }
    for i in 0..n {
        let e = ifd + 2 + 12 * i;
        if order.u16(tiff, e) != Some(ORIENTATION) {
            continue;
        }
        let mut out = jpeg.to_vec();
        let at = tiff_at + e + 8;
        match order.u16(tiff, e + 2) {
            Some(LONG) => out[at..at + 4].copy_from_slice(&order.put_u32(value.into())),
            _ => out[at..at + 2].copy_from_slice(&order.put_u16(value)),
        }
        return Ok(out);
    }
    // IFD0 copied with the tag added, in tag order.
    let mut entries: Vec<[u8; 12]> = (0..n)
        .map(|i| {
            let e = ifd + 2 + 12 * i;
            tiff[e..e + 12].try_into().expect("12 bytes")
        })
        .collect();
    let mut tag = [0u8; 12];
    tag[..2].copy_from_slice(&order.put_u16(ORIENTATION));
    tag[2..4].copy_from_slice(&order.put_u16(SHORT));
    tag[4..8].copy_from_slice(&order.put_u32(1));
    tag[8..10].copy_from_slice(&order.put_u16(value));
    let at = entries
        .iter()
        .position(|e| order.u16(e, 0).is_some_and(|t| t > ORIENTATION))
        .unwrap_or(entries.len());
    entries.insert(at, tag);
    let next = &tiff[ifd + 2 + 12 * n..ifd + 2 + 12 * n + 4];
    let mut new_tiff = tiff.to_vec();
    if new_tiff.len() % 2 == 1 {
        new_tiff.push(0);
    }
    let new_ifd = u32::try_from(new_tiff.len()).map_err(|_| "EXIF data too large")?;
    new_tiff.extend_from_slice(&order.put_u16(entries.len() as u16));
    for e in &entries {
        new_tiff.extend_from_slice(e);
    }
    new_tiff.extend_from_slice(next);
    new_tiff[4..8].copy_from_slice(&order.put_u32(new_ifd));
    let len = 2 + EXIF.len() + new_tiff.len();
    if len > 0xFFFF {
        return Err("the EXIF segment would grow past its 64 KiB".into());
    }
    let mut out = Vec::with_capacity(jpeg.len() + 2 + 12 * (n + 1) + 6);
    out.extend_from_slice(&jpeg[..seg.start]);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(EXIF);
    out.extend_from_slice(&new_tiff);
    out.extend_from_slice(&jpeg[seg.end..]);
    Ok(out)
}

/// `jpeg`, which has no EXIF, with an Exif segment holding the
/// orientation alone.
fn insert_exif(jpeg: &[u8], segs: &[Segment], value: u16) -> Vec<u8> {
    let order = Order { big: true };
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"MM");
    tiff.extend_from_slice(&order.put_u16(42));
    tiff.extend_from_slice(&order.put_u32(8));
    tiff.extend_from_slice(&order.put_u16(1));
    tiff.extend_from_slice(&order.put_u16(ORIENTATION));
    tiff.extend_from_slice(&order.put_u16(SHORT));
    tiff.extend_from_slice(&order.put_u32(1));
    tiff.extend_from_slice(&order.put_u16(value));
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&order.put_u32(0));
    // JFIF wants its APP0 first.
    let at = segs
        .first()
        .filter(|s| s.marker == 0xE0)
        .map_or(2, |s| s.end);
    let len = (2 + EXIF.len() + tiff.len()) as u16;
    let mut out = Vec::with_capacity(jpeg.len() + len as usize + 2);
    out.extend_from_slice(&jpeg[..at]);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(EXIF);
    out.extend_from_slice(&tiff);
    out.extend_from_slice(&jpeg[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn plain_jpeg() -> Vec<u8> {
        let img =
            image::RgbImage::from_fn(8, 4, |x, y| image::Rgb([x as u8 * 30, y as u8 * 60, 0]));
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut out)
            .encode_image(&img)
            .unwrap();
        out
    }

    fn exif_of(jpeg: &[u8]) -> exif::Exif {
        exif::Reader::new()
            .read_from_container(&mut std::io::Cursor::new(jpeg))
            .unwrap()
    }

    #[test]
    fn a_tag_added_to_a_file_without_exif() {
        let plain = plain_jpeg();
        assert_eq!(orientation(&plain), None);
        let turned = set_orientation(&plain, 6).unwrap();
        assert_eq!(orientation(&turned), Some(6));
        let e = exif_of(&turned);
        let f = e
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .unwrap();
        assert_eq!(f.value.get_uint(0), Some(6));
        // The picture decodes as before.
        let a = image::load_from_memory(&plain).unwrap().to_rgb8();
        let b = image::load_from_memory(&turned).unwrap().to_rgb8();
        assert_eq!(a, b);
    }

    #[test]
    fn a_tag_present_is_rewritten_in_place() {
        let once = set_orientation(&plain_jpeg(), 6).unwrap();
        let twice = set_orientation(&once, 3).unwrap();
        assert_eq!(once.len(), twice.len());
        let differ: Vec<usize> = (0..once.len()).filter(|&i| once[i] != twice[i]).collect();
        assert_eq!(differ.len(), 1, "only the tag's value changes");
        assert_eq!(orientation(&twice), Some(3));
    }

    /// A little-endian EXIF segment whose IFD0 holds `Make` only.
    fn with_make(jpeg: &[u8]) -> Vec<u8> {
        let o = Order { big: false };
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&o.put_u16(42));
        tiff.extend_from_slice(&o.put_u32(8));
        tiff.extend_from_slice(&o.put_u16(1));
        tiff.extend_from_slice(&o.put_u16(0x010F));
        tiff.extend_from_slice(&o.put_u16(2));
        tiff.extend_from_slice(&o.put_u32(6));
        // The value lives after the directory.
        tiff.extend_from_slice(&o.put_u32(26));
        tiff.extend_from_slice(&o.put_u32(0));
        tiff.extend_from_slice(b"Kalem\0");
        let len = (2 + EXIF.len() + tiff.len()) as u16;
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(EXIF);
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn a_tag_added_to_existing_exif_keeps_the_other_tags() {
        let jpeg = with_make(&plain_jpeg());
        assert_eq!(orientation(&jpeg), None);
        let turned = set_orientation(&jpeg, 8).unwrap();
        let e = exif_of(&turned);
        let make = e.get_field(exif::Tag::Make, exif::In::PRIMARY).unwrap();
        assert_eq!(make.display_value().to_string(), "\"Kalem\"");
        let f = e
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .unwrap();
        assert_eq!(f.value.get_uint(0), Some(8));
        assert_eq!(orientation(&turned), Some(8));
    }

    #[test]
    fn not_a_jpeg() {
        assert!(set_orientation(b"\x89PNG....", 1).is_err());
        assert!(set_orientation(&plain_jpeg(), 9).is_err());
    }
}
