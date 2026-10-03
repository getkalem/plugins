//! Text strings (ISO 32000-2, 7.9.2.2): the titles of the outline, the
//! document information and the page labels' prefixes.

/// A text string decoded: UTF-16BE or UTF-8 after their byte order marks,
/// PDFDocEncoding otherwise. The language escapes of UTF-16 strings
/// (U+001B, a language code, U+001B) are dropped.
pub(crate) fn decode(bytes: &[u8]) -> String {
    let s = if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        without_language_escapes(&String::from_utf16_lossy(&units))
    } else if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else {
        bytes.iter().map(|&b| pdf_doc(b)).collect()
    };
    s.replace('\0', "")
}

fn without_language_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\u{1B}' {
            in_escape = !in_escape;
        } else if !in_escape {
            out.push(c);
        }
    }
    out
}

/// A byte of PDFDocEncoding (Annex D.3): Latin-1 but for 0x18 to 0x1F
/// and 0x80 to 0xA0.
fn pdf_doc(b: u8) -> char {
    const LOW: [char; 8] = [
        '\u{02D8}', '\u{02C7}', '\u{02C6}', '\u{02D9}', '\u{02DD}', '\u{02DB}', '\u{02DA}',
        '\u{02DC}',
    ];
    const HIGH: [char; 33] = [
        '\u{2022}', '\u{2020}', '\u{2021}', '\u{2026}', '\u{2014}', '\u{2013}', '\u{0192}',
        '\u{2044}', '\u{2039}', '\u{203A}', '\u{2212}', '\u{2030}', '\u{201E}', '\u{201C}',
        '\u{201D}', '\u{2018}', '\u{2019}', '\u{201A}', '\u{2122}', '\u{FB01}', '\u{FB02}',
        '\u{0141}', '\u{0152}', '\u{0160}', '\u{0178}', '\u{017D}', '\u{0131}', '\u{0142}',
        '\u{0153}', '\u{0161}', '\u{017E}', '\u{FFFD}', '\u{20AC}',
    ];
    match b {
        0x18..=0x1F => LOW[(b - 0x18) as usize],
        0x80..=0xA0 => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn encodings() {
        assert_eq!(decode(b"Chapter 1"), "Chapter 1");
        assert_eq!(
            decode(&[0x84, b'x', 0x93, 0xA0]),
            "\u{2014}x\u{FB01}\u{20AC}"
        );
        assert_eq!(decode(&[0xE7]), "ç");
        let utf16: Vec<u8> = [0xFEFF_u16, 0x015F, 0x0130]
            .iter()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        assert_eq!(decode(&utf16), "şİ");
        assert_eq!(decode("\u{FEFF}Ünite".as_bytes()), "Ünite");
        let tagged: Vec<u8> = "\u{FEFF}\u{1B}tr\u{1B}Giriş"
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        assert_eq!(decode(&tagged), "Giriş");
    }
}
