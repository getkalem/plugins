//! The OPC package (ECMA-376 part 2) as a ZIP archive read part by part.
//!
//! A package is kept as the bytes it was read from. Reading a part inflates
//! it; replacing or removing parts records the change; writing copies every
//! untouched entry's local header and data verbatim, writes the changed ones
//! anew and rebuilds the central directory from the original records, so a
//! save without changes returns the input unchanged and a save after an edit
//! differs from it in the edited parts and the offsets alone.

use std::collections::HashMap;
use std::fmt;

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;
const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;

/// An error reading or writing a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageError {
    /// The bytes are not a ZIP archive or one of its records is cut short.
    Malformed(String),
    /// A part uses a compression method other than stored or deflate.
    Unsupported(String),
    /// A part named by the caller does not exist.
    Missing(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(m) => write!(f, "malformed package: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::Missing(m) => write!(f, "no part named {m}"),
        }
    }
}

impl std::error::Error for PackageError {}

type Result<T> = std::result::Result<T, PackageError>;

fn malformed(m: impl Into<String>) -> PackageError {
    PackageError::Malformed(m.into())
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    /// The central directory record as read, name, extra and comment included.
    central: Vec<u8>,
    local_offset: u64,
    /// Where the next entry (or the central directory) begins: the local
    /// header, the data and any data descriptor are copied as one span.
    local_end: u64,
    data_offset: u64,
    method: u16,
    compressed_size: u64,
    flags: u16,
}

/// A change to the package not yet written.
#[derive(Debug, Clone)]
enum Change {
    Replace(Vec<u8>),
    Remove,
}

/// A ZIP package read from bytes.
#[derive(Clone)]
pub struct Package {
    /// Shared, so that a snapshot for undo costs nothing.
    bytes: std::sync::Arc<Vec<u8>>,
    /// Entries in central directory order.
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    /// The archive comment, kept on rewrite.
    comment: Vec<u8>,
    central_start: u64,
    changes: HashMap<String, Change>,
    /// Parts added by the caller, in the order added.
    added: Vec<(String, Vec<u8>)>,
}

impl fmt::Debug for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Package")
            .field("len", &self.bytes.len())
            .field("parts", &self.entries.len())
            .field("changes", &self.changes.len())
            .finish()
    }
}

fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| malformed("record cut short"))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| malformed("record cut short"))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64> {
    b.get(at..at + 8)
        .map(|s| u64::from_le_bytes(s.try_into().expect("eight bytes")))
        .ok_or_else(|| malformed("record cut short"))
}

fn to_usize(v: u64) -> Result<usize> {
    usize::try_from(v).map_err(|_| malformed("offset beyond memory"))
}

/// The zip64 extended information extra field's values, in the order the
/// specification gives them, present only where the record holds 0xFFFFFFFF.
struct Zip64Fields {
    uncompressed: Option<u64>,
    compressed: Option<u64>,
    offset: Option<u64>,
    /// Byte position of the offset value inside the extra field, for patching.
    offset_pos: Option<usize>,
}

fn zip64_fields(extra: &[u8], unc: u32, comp: u32, off: u32) -> Zip64Fields {
    let mut out = Zip64Fields {
        uncompressed: None,
        compressed: None,
        offset: None,
        offset_pos: None,
    };
    let mut at = 0;
    while at + 4 <= extra.len() {
        let id = u16::from_le_bytes([extra[at], extra[at + 1]]);
        let len = usize::from(u16::from_le_bytes([extra[at + 2], extra[at + 3]]));
        let body = at + 4;
        if id == 0x0001 {
            let mut p = body;
            let end = (body + len).min(extra.len());
            let take = |p: &mut usize| -> Option<u64> {
                let v = extra.get(*p..*p + 8)?;
                if *p + 8 > end {
                    return None;
                }
                *p += 8;
                Some(u64::from_le_bytes(v.try_into().ok()?))
            };
            if unc == u32::MAX {
                out.uncompressed = take(&mut p);
            }
            if comp == u32::MAX {
                out.compressed = take(&mut p);
            }
            if off == u32::MAX {
                out.offset_pos = Some(p);
                out.offset = take(&mut p);
            }
            return out;
        }
        at = body + len;
    }
    out
}

impl Package {
    /// Reads the directory of a package; parts are inflated when asked for.
    pub fn read(bytes: Vec<u8>) -> Result<Self> {
        let len = bytes.len();
        if len < 22 {
            return Err(malformed("shorter than an end of central directory record"));
        }
        // The end record is the last 22 bytes plus a comment of up to 64 KiB.
        let lowest = len.saturating_sub(22 + 0xFFFF);
        let mut eocd = None;
        let mut at = len - 22;
        loop {
            if u32_at(&bytes, at)? == EOCD_SIG {
                let comment_len = usize::from(u16_at(&bytes, at + 20)?);
                if at + 22 + comment_len == len {
                    eocd = Some(at);
                    break;
                }
            }
            if at == lowest {
                break;
            }
            at -= 1;
        }
        let eocd = eocd.ok_or_else(|| malformed("no end of central directory record"))?;
        let comment_len = usize::from(u16_at(&bytes, eocd + 20)?);
        let comment = bytes[eocd + 22..eocd + 22 + comment_len].to_vec();
        let mut count = u64::from(u16_at(&bytes, eocd + 10)?);
        let mut cd_size = u64::from(u32_at(&bytes, eocd + 12)?);
        let mut cd_start = u64::from(u32_at(&bytes, eocd + 16)?);
        if eocd >= 20 && u32_at(&bytes, eocd - 20)? == ZIP64_LOCATOR_SIG {
            let z = to_usize(u64_at(&bytes, eocd - 20 + 8)?)?;
            if u32_at(&bytes, z)? != ZIP64_EOCD_SIG {
                return Err(malformed("zip64 locator points at no zip64 end record"));
            }
            count = u64_at(&bytes, z + 32)?;
            cd_size = u64_at(&bytes, z + 40)?;
            cd_start = u64_at(&bytes, z + 48)?;
        }
        let cd_end = cd_start
            .checked_add(cd_size)
            .filter(|&e| e <= len as u64)
            .ok_or_else(|| malformed("central directory beyond the end"))?;
        let mut entries = Vec::new();
        let mut p = to_usize(cd_start)?;
        for _ in 0..count {
            if u32_at(&bytes, p)? != CENTRAL_SIG {
                return Err(malformed("central directory record expected"));
            }
            let flags = u16_at(&bytes, p + 8)?;
            let method = u16_at(&bytes, p + 10)?;
            let comp32 = u32_at(&bytes, p + 20)?;
            let unc32 = u32_at(&bytes, p + 24)?;
            let name_len = usize::from(u16_at(&bytes, p + 28)?);
            let extra_len = usize::from(u16_at(&bytes, p + 30)?);
            let comment_len = usize::from(u16_at(&bytes, p + 32)?);
            let off32 = u32_at(&bytes, p + 42)?;
            let rec_len = 46 + name_len + extra_len + comment_len;
            let central = bytes
                .get(p..p + rec_len)
                .ok_or_else(|| malformed("central record cut short"))?
                .to_vec();
            let name = String::from_utf8_lossy(&central[46..46 + name_len]).into_owned();
            let z64 = zip64_fields(
                &central[46 + name_len..46 + name_len + extra_len],
                unc32,
                comp32,
                off32,
            );
            let local_offset = z64.offset.unwrap_or(u64::from(off32));
            let compressed_size = z64.compressed.unwrap_or(u64::from(comp32));
            let lo = to_usize(local_offset)?;
            if u32_at(&bytes, lo)? != LOCAL_SIG {
                return Err(malformed(format!("no local header for {name}")));
            }
            let lname = usize::from(u16_at(&bytes, lo + 26)?);
            let lextra = usize::from(u16_at(&bytes, lo + 28)?);
            let data_offset = local_offset + 30 + (lname + lextra) as u64;
            entries.push(Entry {
                name,
                central,
                local_offset,
                local_end: 0,
                data_offset,
                method,
                compressed_size,
                flags,
            });
            p += rec_len;
        }
        if p as u64 > cd_end {
            return Err(malformed("central directory longer than declared"));
        }
        // Each local span runs to the next local header or the directory.
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by_key(|&i| entries[i].local_offset);
        for w in 0..order.len() {
            let end = order
                .get(w + 1)
                .map_or(cd_start, |&n| entries[n].local_offset);
            let e = &mut entries[order[w]];
            if end < e.data_offset + e.compressed_size {
                return Err(malformed(format!("{} overlaps the next entry", e.name)));
            }
            e.local_end = end;
        }
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.name.clone(), i))
            .collect();
        Ok(Self {
            bytes: std::sync::Arc::new(bytes),
            entries,
            index,
            comment,
            central_start: cd_start,
            changes: HashMap::new(),
            added: Vec::new(),
        })
    }

    /// The names of the parts as the package lists them, changes applied.
    pub fn names(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .entries
            .iter()
            .filter(|e| !matches!(self.changes.get(&e.name), Some(Change::Remove)))
            .map(|e| e.name.clone())
            .collect();
        out.extend(self.added.iter().map(|(n, _)| n.clone()));
        out
    }

    /// Whether a part exists, changes applied.
    pub fn contains(&self, name: &str) -> bool {
        match self.changes.get(name) {
            Some(Change::Remove) => false,
            Some(Change::Replace(_)) => true,
            None => self.index.contains_key(name) || self.added.iter().any(|(n, _)| n == name),
        }
    }

    /// A part's bytes, inflated, with changes applied.
    pub fn part(&self, name: &str) -> Result<Vec<u8>> {
        match self.changes.get(name) {
            Some(Change::Replace(b)) => return Ok(b.clone()),
            Some(Change::Remove) => return Err(PackageError::Missing(name.into())),
            None => {}
        }
        if let Some((_, b)) = self.added.iter().find(|(n, _)| n == name) {
            return Ok(b.clone());
        }
        let e = &self.entries[*self
            .index
            .get(name)
            .ok_or_else(|| PackageError::Missing(name.into()))?];
        let start = to_usize(e.data_offset)?;
        let data = self
            .bytes
            .get(start..start + to_usize(e.compressed_size)?)
            .ok_or_else(|| malformed(format!("{name} cut short")))?;
        match e.method {
            0 => Ok(data.to_vec()),
            8 => miniz_oxide::inflate::decompress_to_vec(data)
                .map_err(|err| malformed(format!("{name}: {err:?}"))),
            m => Err(PackageError::Unsupported(format!(
                "{name} is compressed with method {m}"
            ))),
        }
    }

    /// Replaces a part's bytes (or adds the part) for the next write.
    pub fn set_part(&mut self, name: &str, bytes: Vec<u8>) {
        if self.index.contains_key(name) {
            self.changes.insert(name.into(), Change::Replace(bytes));
        } else if let Some(slot) = self.added.iter_mut().find(|(n, _)| n == name) {
            slot.1 = bytes;
        } else {
            self.added.push((name.into(), bytes));
        }
    }

    /// Removes a part for the next write.
    pub fn remove_part(&mut self, name: &str) {
        if self.index.contains_key(name) {
            self.changes.insert(name.into(), Change::Remove);
        } else {
            self.added.retain(|(n, _)| n != name);
        }
    }

    /// Forgets any change to a part (replaced, removed or added), so that
    /// the next write copies it as it was read: an edit undone leaves the
    /// package as it was, byte for byte.
    pub fn restore_part(&mut self, name: &str) {
        self.changes.remove(name);
        self.added.retain(|(n, _)| n != name);
    }

    /// Whether anything changed since the package was read.
    pub fn is_dirty(&self) -> bool {
        !self.changes.is_empty() || !self.added.is_empty()
    }

    /// The names of the parts that a write would change, add or remove.
    pub fn changed_parts(&self) -> Vec<String> {
        let mut out: Vec<String> = self.changes.keys().cloned().collect();
        out.extend(self.added.iter().map(|(n, _)| n.clone()));
        out.sort();
        out
    }

    /// The package as bytes: the input itself when nothing changed.
    pub fn write(&self) -> Result<Vec<u8>> {
        if !self.is_dirty() {
            return Ok(self.bytes.to_vec());
        }
        let mut out = Vec::with_capacity(self.bytes.len());
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by_key(|&i| self.entries[i].local_offset);
        // Whatever precedes the first entry (a stub, say) stays.
        let first = order
            .first()
            .map_or(self.central_start, |&i| self.entries[i].local_offset);
        out.extend_from_slice(&self.bytes[..to_usize(first)?]);
        let mut central: Vec<Option<Vec<u8>>> = vec![None; self.entries.len()];
        for &i in &order {
            let e = &self.entries[i];
            let offset = out.len() as u64;
            match self.changes.get(&e.name) {
                Some(Change::Remove) => {}
                Some(Change::Replace(data)) => {
                    central[i] = Some(write_entry(&mut out, Some(e), &e.name, data, offset)?);
                }
                None => {
                    let s = to_usize(e.local_offset)?;
                    out.extend_from_slice(&self.bytes[s..to_usize(e.local_end)?]);
                    central[i] = Some(relocated_central(e, offset)?);
                }
            }
        }
        let mut added_central = Vec::new();
        for (name, data) in &self.added {
            let offset = out.len() as u64;
            added_central.push(write_entry(&mut out, None, name, data, offset)?);
        }
        let cd_start = out.len() as u64;
        let mut count: u64 = 0;
        for rec in central.into_iter().flatten().chain(added_central) {
            out.extend_from_slice(&rec);
            count += 1;
        }
        let cd_size = out.len() as u64 - cd_start;
        let fits =
            count < 0xFFFF && cd_size < u64::from(u32::MAX) && cd_start < u64::from(u32::MAX);
        if !fits {
            let z = out.len() as u64;
            put32(&mut out, ZIP64_EOCD_SIG);
            out.extend_from_slice(&44u64.to_le_bytes());
            put16(&mut out, 45);
            put16(&mut out, 45);
            put32(&mut out, 0);
            put32(&mut out, 0);
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&cd_size.to_le_bytes());
            out.extend_from_slice(&cd_start.to_le_bytes());
            put32(&mut out, ZIP64_LOCATOR_SIG);
            put32(&mut out, 0);
            out.extend_from_slice(&z.to_le_bytes());
            put32(&mut out, 1);
        }
        put32(&mut out, EOCD_SIG);
        put16(&mut out, 0);
        put16(&mut out, 0);
        let c16 = u16::try_from(count).unwrap_or(u16::MAX);
        put16(&mut out, c16);
        put16(&mut out, c16);
        put32(&mut out, u32::try_from(cd_size).unwrap_or(u32::MAX));
        put32(&mut out, u32::try_from(cd_start).unwrap_or(u32::MAX));
        put16(&mut out, self.comment.len() as u16);
        out.extend_from_slice(&self.comment);
        Ok(out)
    }
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// The original central record with its local header offset moved.
fn relocated_central(e: &Entry, offset: u64) -> Result<Vec<u8>> {
    let mut rec = e.central.clone();
    let off32 = u32_at(&rec, 42)?;
    if off32 == u32::MAX {
        let name_len = usize::from(u16_at(&rec, 28)?);
        let extra_len = usize::from(u16_at(&rec, 30)?);
        let extra_start = 46 + name_len;
        let z = zip64_fields(
            &rec[extra_start..extra_start + extra_len],
            u32_at(&rec, 24)?,
            u32_at(&rec, 20)?,
            off32,
        );
        let pos = extra_start
            + z.offset_pos
                .ok_or_else(|| malformed(format!("{}: zip64 offset missing", e.name)))?;
        rec[pos..pos + 8].copy_from_slice(&offset.to_le_bytes());
    } else {
        let o = u32::try_from(offset)
            .map_err(|_| PackageError::Unsupported(format!("{} moved beyond 4 GiB", e.name)))?;
        rec[42..46].copy_from_slice(&o.to_le_bytes());
    }
    Ok(rec)
}

/// Writes a new local entry, deflated, and returns its central record. The
/// time, the attributes and the comment of the entry it replaces are kept.
fn write_entry(
    out: &mut Vec<u8>,
    old: Option<&Entry>,
    name: &str,
    data: &[u8],
    offset: u64,
) -> Result<Vec<u8>> {
    let compressed = miniz_oxide::deflate::compress_to_vec(data, 6);
    let crc = crc32fast::hash(data);
    let too_big = |what: &str| PackageError::Unsupported(format!("{name}: {what} beyond 4 GiB"));
    let comp = u32::try_from(compressed.len()).map_err(|_| too_big("part"))?;
    let unc = u32::try_from(data.len()).map_err(|_| too_big("part"))?;
    let off = u32::try_from(offset).map_err(|_| too_big("offset"))?;
    // Bit 3 (data descriptor) is dropped: sizes are known; bit 11 (UTF-8 names) kept.
    let (flags, time, date, made_by, internal, external, comment) = match old {
        Some(e) => {
            let c = &e.central;
            let name_len = usize::from(u16_at(c, 28)?);
            let extra_len = usize::from(u16_at(c, 30)?);
            (
                e.flags & 0x0800,
                u16_at(c, 12)?,
                u16_at(c, 14)?,
                u16_at(c, 4)?,
                u16_at(c, 36)?,
                u32_at(c, 38)?,
                c[46 + name_len + extra_len..].to_vec(),
            )
        }
        None => (
            0x0800 * u16::from(!name.is_ascii()),
            0,
            0x21,
            20,
            0,
            0,
            Vec::new(),
        ),
    };
    put32(out, LOCAL_SIG);
    put16(out, 20);
    put16(out, flags);
    put16(out, 8);
    put16(out, time);
    put16(out, date);
    put32(out, crc);
    put32(out, comp);
    put32(out, unc);
    put16(out, name.len() as u16);
    put16(out, 0);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&compressed);
    let mut c = Vec::with_capacity(46 + name.len() + comment.len());
    put32(&mut c, CENTRAL_SIG);
    put16(&mut c, made_by);
    put16(&mut c, 20);
    put16(&mut c, flags);
    put16(&mut c, 8);
    put16(&mut c, time);
    put16(&mut c, date);
    put32(&mut c, crc);
    put32(&mut c, comp);
    put32(&mut c, unc);
    put16(&mut c, name.len() as u16);
    put16(&mut c, 0);
    put16(&mut c, comment.len() as u16);
    put16(&mut c, 0);
    put16(&mut c, internal);
    put32(&mut c, external);
    put32(&mut c, off);
    c.extend_from_slice(name.as_bytes());
    c.extend_from_slice(&comment);
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-part archive written by this module from an empty one.
    fn sample() -> Vec<u8> {
        let empty = {
            let mut b = Vec::new();
            put32(&mut b, EOCD_SIG);
            b.extend_from_slice(&[0; 18]);
            b
        };
        let mut p = Package::read(empty).unwrap();
        p.set_part("a.xml", b"<a/>".to_vec());
        p.set_part("b/c.xml", b"<c>hello</c>".to_vec());
        p.write().unwrap()
    }

    #[test]
    fn round_trip_without_changes_is_identical() {
        let bytes = sample();
        let p = Package::read(bytes.clone()).unwrap();
        assert_eq!(p.write().unwrap(), bytes);
        assert_eq!(p.part("b/c.xml").unwrap(), b"<c>hello</c>");
    }

    #[test]
    fn replace_keeps_other_entries_verbatim() {
        let bytes = sample();
        let mut p = Package::read(bytes).unwrap();
        p.set_part("a.xml", b"<a x=\"1\"/>".to_vec());
        let out = p.write().unwrap();
        let q = Package::read(out).unwrap();
        assert_eq!(q.part("a.xml").unwrap(), b"<a x=\"1\"/>");
        assert_eq!(q.part("b/c.xml").unwrap(), b"<c>hello</c>");
        assert_eq!(q.names(), ["a.xml", "b/c.xml"]);
    }

    #[test]
    fn remove_drops_the_entry() {
        let mut p = Package::read(sample()).unwrap();
        p.remove_part("a.xml");
        let q = Package::read(p.write().unwrap()).unwrap();
        assert_eq!(q.names(), ["b/c.xml"]);
        assert!(!q.contains("a.xml"));
    }

    #[test]
    fn a_restored_part_is_copied_as_read() {
        let bytes = sample();
        let mut p = Package::read(bytes.clone()).unwrap();
        p.set_part("a.xml", b"<changed/>".to_vec());
        p.set_part("new.xml", b"<new/>".to_vec());
        p.restore_part("a.xml");
        p.restore_part("new.xml");
        assert!(!p.is_dirty());
        assert_eq!(p.write().unwrap(), bytes);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(Package::read(b"not a zip at all, not at all".to_vec()).is_err());
    }
}
