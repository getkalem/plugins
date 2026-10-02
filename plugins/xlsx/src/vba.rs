//! The VBA project of a macro-enabled workbook ([MS-OVBA]).
//!
//! `xl/vbaProject.bin` is a compound file: the `VBA/dir` stream lists the
//! modules, compressed (2.4.1), and each module's stream holds its source,
//! compressed, after the p-code at the offset `dir` gives. This module reads
//! the source text of every module. The project part itself is never
//! rewritten: saving keeps it byte for byte.

use std::fmt;
use std::io::{Cursor, Read};

/// What a module is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    /// A standard module (`Module1`).
    Standard,
    /// A document module (`ThisWorkbook`, `Sheet1`) or a class or form.
    Class,
}

/// One module's source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The module's name.
    pub name: String,
    /// What it is.
    pub kind: ModuleKind,
    /// The source text, `Attribute` lines included.
    pub source: String,
}

/// The project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The project's name.
    pub name: String,
    /// The modules in `dir` order.
    pub modules: Vec<Module>,
}

/// An error reading a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VbaError(pub String);

impl fmt::Display for VbaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VBA project: {}", self.0)
    }
}

impl std::error::Error for VbaError {}

fn err(m: impl Into<String>) -> VbaError {
    VbaError(m.into())
}

/// Decompresses a compressed container (2.4.1.3.1).
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, VbaError> {
    if data.first() != Some(&1) {
        return Err(err("compressed container without its signature byte"));
    }
    let mut out = Vec::with_capacity(data.len() * 2);
    let mut pos = 1;
    while pos + 2 <= data.len() {
        let header = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let size = usize::from(header & 0x0FFF) + 3;
        let compressed = header & 0x8000 != 0;
        let chunk_end = (pos + size).min(data.len());
        pos += 2;
        let chunk_start = out.len();
        if !compressed {
            let end = (pos + 4096).min(data.len());
            out.extend_from_slice(&data[pos..end]);
            pos = end;
            continue;
        }
        while pos < chunk_end {
            let flags = data[pos];
            pos += 1;
            for bit in 0..8 {
                if pos >= chunk_end {
                    break;
                }
                if flags & (1 << bit) == 0 {
                    out.push(data[pos]);
                    pos += 1;
                } else {
                    if pos + 2 > chunk_end {
                        return Err(err("copy token cut short"));
                    }
                    let token = u16::from_le_bytes([data[pos], data[pos + 1]]);
                    pos += 2;
                    let d = out.len() - chunk_start;
                    let mut bit_count = 4;
                    while (1usize << bit_count) < d {
                        bit_count += 1;
                    }
                    let length_mask = 0xFFFFu16 >> bit_count;
                    let length = usize::from(token & length_mask) + 3;
                    let offset = usize::from(token >> (16 - bit_count)) + 1;
                    if offset > out.len() - chunk_start {
                        return Err(err("copy token reaches before its chunk"));
                    }
                    let from = out.len() - offset;
                    for k in 0..length {
                        let b = out[from + k];
                        out.push(b);
                    }
                }
            }
        }
        pos = chunk_end;
    }
    Ok(out)
}

/// Compresses data into a compressed container (2.4.1.3.6), greedy
/// longest match per position.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![1u8];
    for chunk in data.chunks(4096) {
        let mut body = Vec::new();
        let mut pos = 0;
        while pos < chunk.len() {
            let flag_at = body.len();
            body.push(0u8);
            for bit in 0..8 {
                if pos >= chunk.len() {
                    break;
                }
                let mut bit_count = 4;
                while (1usize << bit_count) < pos {
                    bit_count += 1;
                }
                let max_len = (0xFFFFusize >> bit_count) + 3;
                let max_off = 1usize << (16 - bit_count);
                let (mut best_len, mut best_off) = (0, 0);
                let lowest = pos.saturating_sub(max_off);
                for cand in (lowest..pos).rev() {
                    let mut l = 0;
                    while l < max_len && pos + l < chunk.len() && chunk[cand + l] == chunk[pos + l]
                    {
                        l += 1;
                    }
                    if l > best_len {
                        best_len = l;
                        best_off = pos - cand;
                    }
                }
                if best_len >= 3 {
                    let token =
                        (((best_off - 1) as u16) << (16 - bit_count)) | (best_len - 3) as u16;
                    body.extend_from_slice(&token.to_le_bytes());
                    body[flag_at] |= 1 << bit;
                    pos += best_len;
                } else {
                    body.push(chunk[pos]);
                    pos += 1;
                }
            }
        }
        if body.len() > 4096 {
            // Raw chunk: the data padded to 4096 bytes (2.4.1.3.10).
            out.extend_from_slice(&0x3FFFu16.to_le_bytes());
            out.extend_from_slice(chunk);
            out.resize(out.len() + 4096 - chunk.len(), 0);
        } else {
            let header = 0xB000 | ((body.len() + 2 - 3) as u16 & 0x0FFF);
            out.extend_from_slice(&header.to_le_bytes());
            out.extend_from_slice(&body);
        }
    }
    out
}

/// Text in a Windows code page; single-byte Western and Turkish pages are
/// mapped, UTF-8 read as such, anything else as Latin-1.
pub fn decode(bytes: &[u8], codepage: u16) -> String {
    const CP1252_80: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013,
        0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    match codepage {
        65001 => String::from_utf8_lossy(bytes).into_owned(),
        1252 | 1254 => bytes
            .iter()
            .map(|&b| {
                let u = match b {
                    0x80..=0x9F => CP1252_80[usize::from(b - 0x80)],
                    _ => u32::from(b),
                };
                let u = if codepage == 1254 {
                    match b {
                        0x8E | 0x9E => 0xFFFD,
                        0xD0 => 0x011E,
                        0xDD => 0x0130,
                        0xDE => 0x015E,
                        0xF0 => 0x011F,
                        0xFD => 0x0131,
                        0xFE => 0x015F,
                        _ => u,
                    }
                } else {
                    u
                };
                char::from_u32(u).unwrap_or('\u{FFFD}')
            })
            .collect(),
        _ => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// Reads the project's modules from the bytes of `vbaProject.bin`.
pub fn read_project(bin: &[u8]) -> Result<Project, VbaError> {
    let mut cf =
        cfb::CompoundFile::open(Cursor::new(bin.to_vec())).map_err(|e| err(e.to_string()))?;
    // The VBA storage is at the root in `vbaProject.bin`; other hosts nest it.
    let vba_dir = cf
        .walk()
        .find(|e| e.is_storage() && e.name().eq_ignore_ascii_case("VBA"))
        .map(|e| e.path().to_path_buf())
        .ok_or_else(|| err("no VBA storage"))?;
    let read =
        |cf: &mut cfb::CompoundFile<Cursor<Vec<u8>>>, name: &str| -> Result<Vec<u8>, VbaError> {
            let path = vba_dir.join(name);
            let mut s = cf
                .open_stream(&path)
                .map_err(|e| err(format!("{name}: {e}")))?;
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).map_err(|e| err(e.to_string()))?;
            Ok(buf)
        };
    let dir = decompress(&read(&mut cf, "dir")?)?;
    let mut codepage = 1252u16;
    let mut project_name = String::new();
    struct Pending {
        name: String,
        stream: String,
        offset: u32,
        kind: ModuleKind,
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut cur: Option<Pending> = None;
    let mut p = 0;
    while p + 6 <= dir.len() {
        let id = u16::from_le_bytes([dir[p], dir[p + 1]]);
        let size = u32::from_le_bytes([dir[p + 2], dir[p + 3], dir[p + 4], dir[p + 5]]) as usize;
        p += 6;
        // PROJECTVERSION's size field says 4 but six bytes follow (2.3.4.2.1.11).
        let size = if id == 0x0009 { 6 } else { size };
        let data = dir
            .get(p..p + size)
            .ok_or_else(|| err("dir record cut short"))?;
        match id {
            0x0003 if size >= 2 => codepage = u16::from_le_bytes([data[0], data[1]]),
            0x0004 => project_name = decode(data, codepage),
            0x0019 => {
                if let Some(m) = cur.take() {
                    pending.push(m);
                }
                cur = Some(Pending {
                    name: decode(data, codepage),
                    stream: String::new(),
                    offset: 0,
                    kind: ModuleKind::Standard,
                });
            }
            0x0047 => {
                if let Some(m) = cur.as_mut() {
                    let units: Vec<u16> = data
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect();
                    m.name = String::from_utf16_lossy(&units);
                }
            }
            0x001A => {
                if let Some(m) = cur.as_mut() {
                    m.stream = decode(data, codepage);
                }
            }
            0x0032 => {
                if let Some(m) = cur.as_mut() {
                    let units: Vec<u16> = data
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect();
                    m.stream = String::from_utf16_lossy(&units);
                }
            }
            0x0031 if size >= 4 => {
                if let Some(m) = cur.as_mut() {
                    m.offset = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                }
            }
            0x0022 => {
                if let Some(m) = cur.as_mut() {
                    m.kind = ModuleKind::Class;
                }
            }
            0x002B => {
                if let Some(m) = cur.take() {
                    pending.push(m);
                }
            }
            _ => {}
        }
        p += size;
    }
    if let Some(m) = cur.take() {
        pending.push(m);
    }
    let mut modules = Vec::new();
    for m in pending {
        let stream = read(&mut cf, &m.stream)?;
        let at = (m.offset as usize).min(stream.len());
        let source = decode(&decompress(&stream[at..])?, codepage);
        modules.push(Module {
            name: m.name,
            kind: m.kind,
            source,
        });
    }
    Ok(Project {
        name: project_name,
        modules,
    })
}

fn rec(out: &mut Vec<u8>, id: u16, data: &[u8]) {
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

/// A project holding the given modules (name, kind, source in `codepage`),
/// with the records this reader needs: for tests and for trying macros on
/// a workbook. It is not a project Excel would open as it is.
pub fn write_project(modules: &[(&str, ModuleKind, &[u8])], codepage: u16) -> Vec<u8> {
    use std::io::Write;
    let mut dir = Vec::new();
    rec(&mut dir, 0x0001, &1u32.to_le_bytes());
    rec(&mut dir, 0x0002, &0x0409u32.to_le_bytes());
    rec(&mut dir, 0x0003, &codepage.to_le_bytes());
    rec(&mut dir, 0x0004, b"VBAProject");
    // PROJECTVERSION: size 4, six bytes follow.
    dir.extend_from_slice(&0x0009u16.to_le_bytes());
    dir.extend_from_slice(&4u32.to_le_bytes());
    dir.extend_from_slice(&[0; 6]);
    rec(&mut dir, 0x000F, &(modules.len() as u16).to_le_bytes());
    rec(&mut dir, 0x0013, &0xFFFFu16.to_le_bytes());
    for (name, kind, _) in modules {
        rec(&mut dir, 0x0019, name.as_bytes());
        rec(&mut dir, 0x001A, name.as_bytes());
        rec(&mut dir, 0x0031, &4u32.to_le_bytes());
        rec(
            &mut dir,
            if *kind == ModuleKind::Standard {
                0x0021
            } else {
                0x0022
            },
            &[],
        );
        rec(&mut dir, 0x002B, &[]);
    }
    rec(&mut dir, 0x0010, &[]);
    let mut cf =
        cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("an in-memory compound file");
    cf.create_storage("/VBA").expect("storage");
    cf.create_stream("/VBA/dir")
        .and_then(|mut s| s.write_all(&compress(&dir)))
        .expect("dir");
    for (name, _, source) in modules {
        // Four bytes standing for the p-code before the source's offset.
        let mut module = vec![0xAA; 4];
        module.extend(compress(source));
        cf.create_stream(format!("/VBA/{name}"))
            .and_then(|mut s| s.write_all(&module))
            .expect("module");
    }
    let listing: String = modules
        .iter()
        .map(|(n, _, _)| format!("Module={n}\r\n"))
        .collect();
    cf.create_stream("/PROJECT")
        .and_then(|mut s| s.write_all(listing.as_bytes()))
        .expect("project");
    cf.flush().expect("flush");
    cf.into_inner().into_inner()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal project with one standard module.
    pub(crate) fn sample_project(source: &[u8]) -> Vec<u8> {
        write_project(&[("Module1", ModuleKind::Standard, source)], 1254)
    }

    #[test]
    fn compress_round_trip() {
        let text: Vec<u8> = (0..10_000u32).map(|i| (i % 7) as u8).collect();
        let packed = compress(&text);
        assert!(packed.len() < text.len() / 2);
        assert_eq!(decompress(&packed).unwrap(), text);
        let prose = b"Sub A()\r\n    Range(\"A1\").Value = 1\r\nEnd Sub\r\n".repeat(200);
        assert_eq!(decompress(&compress(&prose)).unwrap(), prose);
    }

    #[test]
    fn copy_tokens() {
        // The specification's example (3.2.3): "#aaabcdefaaaaghijaaaabcdefaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        // is long; a short hand-made one: "abcabcabc" = literals a b c, copy offset 3 length 6.
        let d = 3usize; // decompressed so far in chunk
        let bit_count = 4;
        assert!(d <= 1 << bit_count);
        let token: u16 = (((3 - 1) as u16) << (16 - bit_count)) | (6 - 3);
        let mut body = vec![0b0000_1000, b'a', b'b', b'c'];
        body.extend_from_slice(&token.to_le_bytes());
        let mut c = vec![1u8];
        c.extend_from_slice(&(0xB000u16 | (body.len() as u16 + 2 - 3)).to_le_bytes());
        c.extend(body);
        assert_eq!(decompress(&c).unwrap(), b"abcabcabc");
    }

    #[test]
    fn modules_are_read() {
        let src = "Attribute VB_Name = \"Module1\"\r\nSub Merhaba()\r\n    MsgBox \"\u{131}\"\r\nEnd Sub\r\n";
        // Code page 1254: ı is 0xFD.
        let bytes: Vec<u8> = src
            .chars()
            .map(|c| if c == '\u{131}' { 0xFD } else { c as u8 })
            .collect();
        let project = read_project(&sample_project(&bytes)).unwrap();
        assert_eq!(project.name, "VBAProject");
        assert_eq!(project.modules.len(), 1);
        assert_eq!(project.modules[0].name, "Module1");
        assert_eq!(project.modules[0].source, src);
        assert_eq!(project.modules[0].kind, ModuleKind::Standard);
    }
}
