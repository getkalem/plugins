//! Protection, as Excel writes it: cells locked or not (`<protection
//! locked>` of their style), a sheet's `<sheetProtection>` with what it
//! still allows, the workbook's `<workbookProtection lockStructure>`;
//! passwords hashed as Excel hashes them (SHA-512 with a salt and 100,000
//! rounds), the 16-bit hashes of older files checked too; and the edits a
//! protection forbids refused.

use super::*;
use kalem_viewer::SheetProtection;
use sha2::{Digest, Sha512};

const SPIN: u32 = 100_000;

/// A `<workbookProtection>`: its attributes, where it lies, whether it
/// locks the structure.
type WorkbookProtection = (Vec<(String, String)>, std::ops::Range<usize>, bool);

/// The worksheet's children after `<sheetProtection>` (CT_Worksheet).
const AFTER_PROTECTION: [&str; 31] = [
    "protectedRanges",
    "scenarios",
    "autoFilter",
    "sortState",
    "dataConsolidate",
    "customSheetViews",
    "mergeCells",
    "phoneticPr",
    "conditionalFormatting",
    "dataValidations",
    "hyperlinks",
    "printOptions",
    "pageMargins",
    "pageSetup",
    "headerFooter",
    "rowBreaks",
    "colBreaks",
    "customProperties",
    "cellWatches",
    "ignoredErrors",
    "smartTags",
    "drawing",
    "legacyDrawing",
    "legacyDrawingHF",
    "drawingHF",
    "picture",
    "oleObjects",
    "controls",
    "webPublishItems",
    "tableParts",
    "extLst",
];

/// Excel's old 16-bit password hash, as four hexadecimal digits.
pub(crate) fn legacy_hash(password: &str) -> String {
    let mut hash: u32 = 0;
    for (i, c) in password.chars().enumerate() {
        let value = (c as u32 & 0x7FFF) << (i + 1);
        let rotated = value >> 15;
        hash ^= (value & 0x7FFF) | rotated;
    }
    hash ^= password.chars().count() as u32;
    hash ^= 0xCE4B;
    format!("{:04X}", hash & 0xFFFF)
}

/// Excel's SHA-512 password hash: the salt and the password (UTF-16),
/// hashed, then the hash and the round's number hashed `spin` times.
pub(crate) fn sha512_hash(password: &str, salt: &[u8], spin: u32) -> Vec<u8> {
    let mut h = Sha512::new();
    h.update(salt);
    for u in password.encode_utf16() {
        h.update(u.to_le_bytes());
    }
    let mut out = h.finalize().to_vec();
    for i in 0..spin {
        let mut h = Sha512::new();
        h.update(&out);
        h.update(i.to_le_bytes());
        out = h.finalize().to_vec();
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn unbase64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes().filter(|c| *c != b'=') {
        let v = B64.iter().position(|x| *x == c)? as u32;
        buf = buf << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A salt: sixteen bytes, of the time and what is hashed.
fn salt(seed: &str) -> Vec<u8> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut h = Sha512::new();
    h.update(nanos.to_le_bytes());
    h.update(seed.as_bytes());
    h.finalize()[..16].to_vec()
}

/// The attributes of a password for an element: SHA-512 hash, salt and
/// rounds, under `prefix` (`workbook` for the workbook's).
fn password_attrs(password: &str, prefix: &str) -> String {
    let s = salt(password);
    let hash = sha512_hash(password, &s, SPIN);
    let name = |k: &str| {
        if prefix.is_empty() {
            let mut c = k.chars();
            c.next().map_or(String::new(), |f| {
                f.to_lowercase().collect::<String>() + c.as_str()
            })
        } else {
            format!("{prefix}{k}")
        }
    };
    format!(
        " {}=\"SHA-512\" {}=\"{}\" {}=\"{}\" {}=\"{SPIN}\"",
        name("AlgorithmName"),
        name("HashValue"),
        base64(&hash),
        name("SaltValue"),
        base64(&s),
        name("SpinCount"),
    )
}

/// Whether `password` opens a protection with these attributes (none
/// set: no password, any opens it).
fn password_fits(attrs: &[(String, String)], prefix: &str, password: Option<&str>) -> bool {
    let get = |k: &str| {
        attrs
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&format!("{prefix}{k}")))
            .map(|(_, v)| v.as_str())
    };
    let pw = password.unwrap_or("");
    if let (Some(hash), Some(salt)) = (get("HashValue"), get("SaltValue")) {
        let spin = get("SpinCount")
            .and_then(|v| v.parse().ok())
            .unwrap_or(SPIN);
        let Some(salt) = unbase64(salt) else {
            return false;
        };
        return base64(&sha512_hash(pw, &salt, spin)) == hash;
    }
    let legacy = if prefix.is_empty() {
        get("password")
    } else {
        get("Password")
    };
    match legacy {
        Some(h) => h.eq_ignore_ascii_case(&legacy_hash(pw)),
        None => true,
    }
}

impl Workbook {
    /// How sheet `idx` is protected, when it is.
    pub fn sheet_protection(&mut self, idx: usize) -> Option<SheetProtection> {
        self.load(idx).ok()?;
        let attrs = self.loaded[&idx].1.protection.clone()?;
        let allowed = |k: &str| {
            attrs
                .iter()
                .find(|(n, _)| n == k)
                .is_some_and(|(_, v)| v == "0" || v == "false")
        };
        Some(SheetProtection {
            has_password: attrs
                .iter()
                .any(|(n, _)| n == "password" || n == "hashValue"),
            format_cells: allowed("formatCells"),
            format_columns: allowed("formatColumns"),
            format_rows: allowed("formatRows"),
            insert_rows: allowed("insertRows"),
            insert_columns: allowed("insertColumns"),
            delete_rows: allowed("deleteRows"),
            delete_columns: allowed("deleteColumns"),
            sort: allowed("sort"),
            filter: allowed("autoFilter"),
        })
    }

    /// Protect Sheet (`Some`, with a password when given) or Unprotect
    /// Sheet (`None`, the password it has asked). One undo step.
    pub fn protect_sheet(
        &mut self,
        idx: usize,
        protection: Option<&SheetProtection>,
        password: Option<&str>,
    ) -> Result<()> {
        self.load(idx)?;
        if self.sheets[idx].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[idx].name.clone()));
        }
        if protection.is_none()
            && let Some(attrs) = &self.loaded[&idx].1.protection
            && !password_fits(attrs, "", password)
        {
            return Err(Error::Refused("The password is not right".into()));
        }
        let p = self.loaded[&idx].1.prefix.clone();
        let el = protection.map(|s| {
            let flag = |k: &str, allowed: bool| {
                if allowed {
                    format!(" {k}=\"0\"")
                } else {
                    String::new()
                }
            };
            let pw = password
                .filter(|p| !p.is_empty())
                .map_or(String::new(), |p| password_attrs(p, ""));
            format!(
                "<{p}sheetProtection{pw} sheet=\"1\" objects=\"1\" scenarios=\"1\"{}{}{}{}{}{}{}{}{}/>",
                flag("formatCells", s.format_cells),
                flag("formatColumns", s.format_columns),
                flag("formatRows", s.format_rows),
                flag("insertColumns", s.insert_columns),
                flag("insertRows", s.insert_rows),
                flag("deleteColumns", s.delete_columns),
                flag("deleteRows", s.delete_rows),
                flag("sort", s.sort),
                flag("autoFilter", s.filter),
            )
        });
        self.in_one_step(|wb| {
            let text = wb.loaded[&idx].0.clone();
            let mut r = Reader::new(&text);
            let mut old = None;
            while let Some(t) = r.next_token() {
                if let Token::Start(tag) = t
                    && tag.name == "sheetProtection"
                {
                    let end = if tag.empty {
                        tag.span.end
                    } else {
                        r.skip_element()
                    };
                    old = Some(tag.span.start..end);
                    break;
                }
            }
            let new = match (old, &el) {
                (Some(span), Some(e)) => splice(&text, vec![(span, e.clone())]),
                (Some(span), None) => splice(&text, vec![(span, String::new())]),
                (None, Some(e)) => insert_top_level(&text, &AFTER_PROTECTION, e),
                (None, None) => text.clone(),
            };
            wb.replace_sheet_text(idx, new);
            wb.batch_changed = true;
            Ok(())
        })
    }

    /// Whether the workbook's structure is protected.
    pub fn workbook_protected(&self) -> bool {
        self.workbook_protection().is_some_and(|p| p.2)
    }

    /// The `<workbookProtection>`'s attributes, where it lies, and whether
    /// it locks the structure.
    fn workbook_protection(&self) -> Option<WorkbookProtection> {
        let mut r = Reader::new(&self.workbook_xml);
        while let Some(t) = r.next_token() {
            let Token::Start(tag) = t else { continue };
            if tag.name == "workbookProtection" {
                let on = tag
                    .attr("lockStructure")
                    .as_deref()
                    .is_some_and(|v| v == "1" || v == "true");
                let attrs = [
                    "workbookPassword",
                    "workbookAlgorithmName",
                    "workbookHashValue",
                    "workbookSaltValue",
                    "workbookSpinCount",
                ]
                .iter()
                .filter_map(|k| tag.attr(k).map(|v| ((*k).to_owned(), v.into_owned())))
                .collect();
                return Some((attrs, tag.span.clone(), on));
            }
            if tag.name == "sheets" {
                break;
            }
        }
        None
    }

    /// Protect Workbook's structure (`on`, with a password when given), or
    /// its protection taken away with the password it has. One undo step.
    pub fn protect_workbook(&mut self, on: bool, password: Option<&str>) -> Result<()> {
        let old = self.workbook_protection();
        if !on
            && let Some((attrs, _, true)) = &old
            && !password_fits(attrs, "workbook", password)
        {
            return Err(Error::Refused("The password is not right".into()));
        }
        let snapshot = (self.batch.is_none()).then(|| self.snapshot());
        let prefix = {
            let mut r = Reader::new(&self.workbook_xml);
            match r.next_token() {
                Some(Token::Start(tag)) => xml::prefix(tag.qname).to_owned(),
                _ => String::new(),
            }
        };
        let el = on.then(|| {
            let pw = password
                .filter(|p| !p.is_empty())
                .map_or(String::new(), |p| password_attrs(p, "workbook"));
            format!("<{prefix}workbookProtection{pw} lockStructure=\"1\"/>")
        });
        let text = self.workbook_xml.clone();
        self.workbook_xml = match (old, el) {
            (Some((_, span, _)), Some(e)) => splice(&text, vec![(span, e)]),
            (Some((_, span, _)), None) => splice(&text, vec![(span, String::new())]),
            (None, Some(e)) => insert_top_level(&text, &["bookViews", "sheets"], &e),
            (None, None) => text,
        };
        let part = self.workbook_part.clone();
        self.pkg
            .set_part(&part, self.workbook_xml.clone().into_bytes());
        match snapshot {
            Some(s) => {
                self.undo.push(s);
                self.redo.clear();
            }
            None => self.batch_changed = true,
        }
        Ok(())
    }

    /// Refused when sheet `idx` is protected and cell `at` locked.
    pub(crate) fn check_cell_edit(&self, idx: usize, at: CellRef) -> Result<()> {
        let Some((_, model)) = self.loaded.get(&idx) else {
            return Ok(());
        };
        if model.protection.is_none() {
            return Ok(());
        }
        let style = model.cells.get(&at).map_or(0, |c| c.style);
        if self.styles.get(style).unlocked {
            return Ok(());
        }
        Err(Error::Refused(
            "The cell is on a protected sheet: unprotect the sheet to change it".into(),
        ))
    }

    /// Refused when sheet `idx` is protected and does not allow `what`
    /// (its `<sheetProtection>` attribute).
    pub(crate) fn check_allowed(&self, idx: usize, what: &str) -> Result<()> {
        let Some((_, model)) = self.loaded.get(&idx) else {
            return Ok(());
        };
        let Some(attrs) = &model.protection else {
            return Ok(());
        };
        let allowed = attrs
            .iter()
            .find(|(n, _)| n == what)
            .is_some_and(|(_, v)| v == "0" || v == "false");
        if allowed {
            Ok(())
        } else {
            Err(Error::Refused(
                "The sheet is protected: unprotect it to do that".into(),
            ))
        }
    }

    /// Refused when the workbook's structure is protected.
    pub(crate) fn check_structure(&self) -> Result<()> {
        if self.workbook_protected() {
            Err(Error::Refused(
                "The workbook's structure is protected: unprotect it to change its sheets".into(),
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_as_excel_makes_them() {
        // The well-known 16-bit hash of "password".
        assert_eq!(legacy_hash("password"), "83AF");
        assert_eq!(unbase64(&base64(b"Kalem!")).unwrap(), b"Kalem!");
        assert_eq!(base64(b"ab"), "YWI=");
        let s = [1u8; 16];
        assert_eq!(sha512_hash("x", &s, 3), sha512_hash("x", &s, 3));
        assert_ne!(sha512_hash("x", &s, 3), sha512_hash("y", &s, 3));
        // An old file's 16-bit hash opens with its password only.
        let old = vec![("password".to_string(), legacy_hash("abc"))];
        assert!(password_fits(&old, "", Some("abc")));
        assert!(!password_fits(&old, "", Some("abd")));
        let wb = vec![("workbookPassword".to_string(), legacy_hash("abc"))];
        assert!(password_fits(&wb, "workbook", Some("abc")));
    }
}
