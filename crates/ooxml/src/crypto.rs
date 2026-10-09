//! Office files with a password to open (MS-OFFCRYPTO 2.3.4): an OLE
//! compound file whose `EncryptionInfo` stream says how the file's
//! package was encrypted and whose `EncryptedPackage` stream holds it.
//! Agile encryption (Excel 2010 and later, LibreOffice: AES with SHA-512
//! by default) is read and written; Standard encryption (Excel 2007:
//! AES-128 with SHA-1) is read.

use std::io::{Cursor, Read, Write};

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use sha2::Digest;

/// Why an encrypted file was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    /// The password does not open it.
    WrongPassword,
    /// Encrypted another way (RC4, an algorithm Kalem lacks), or damaged.
    Unsupported(String),
}

impl CryptoError {
    /// The message for the user, the file named as `what` is (`workbook`,
    /// `document`).
    pub fn describe(&self, what: &str) -> String {
        match self {
            CryptoError::WrongPassword => format!("The password does not open this {what}"),
            CryptoError::Unsupported(why) => {
                format!("This {what} is encrypted in a way Kalem does not read ({why})")
            }
        }
    }
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe("file"))
    }
}

fn unsupported(why: impl Into<String>) -> CryptoError {
    CryptoError::Unsupported(why.into())
}

/// The hash functions agile encryption names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    fn named(name: &str) -> Option<Hash> {
        Some(match name.to_ascii_uppercase().replace('-', "").as_str() {
            "SHA1" => Hash::Sha1,
            "SHA256" => Hash::Sha256,
            "SHA384" => Hash::Sha384,
            "SHA512" => Hash::Sha512,
            _ => return None,
        })
    }

    fn of(self, parts: &[&[u8]]) -> Vec<u8> {
        fn run<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
            let mut d = D::new();
            for p in parts {
                d.update(p);
            }
            d.finalize().to_vec()
        }
        match self {
            Hash::Sha1 => run::<sha1::Sha1>(parts),
            Hash::Sha256 => run::<sha2::Sha256>(parts),
            Hash::Sha384 => run::<sha2::Sha384>(parts),
            Hash::Sha512 => run::<sha2::Sha512>(parts),
        }
    }

    /// The size of a block the function reads, for HMAC.
    fn block(self) -> usize {
        match self {
            Hash::Sha1 | Hash::Sha256 => 64,
            Hash::Sha384 | Hash::Sha512 => 128,
        }
    }
}

/// HMAC with `hash` (RFC 2104).
fn hmac(hash: Hash, key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut k = if key.len() > hash.block() {
        hash.of(&[key])
    } else {
        key.to_vec()
    };
    k.resize(hash.block(), 0);
    let inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let h = hash.of(&[&inner, data]);
    hash.of(&[&outer, &h])
}

/// AES with a key of 128, 192 or 256 bits.
enum Cipher {
    A128(aes::Aes128),
    A192(aes::Aes192),
    A256(aes::Aes256),
}

impl Cipher {
    fn new(key: &[u8]) -> Result<Cipher, CryptoError> {
        let bad = |_| unsupported("a key of an unknown size");
        Ok(match key.len() {
            16 => Cipher::A128(aes::Aes128::new_from_slice(key).map_err(bad)?),
            24 => Cipher::A192(aes::Aes192::new_from_slice(key).map_err(bad)?),
            32 => Cipher::A256(aes::Aes256::new_from_slice(key).map_err(bad)?),
            _ => return Err(unsupported("a key of an unknown size")),
        })
    }

    fn encrypt(&self, block: &mut [u8; BLOCK]) {
        let b = GenericArray::from_mut_slice(block);
        match self {
            Cipher::A128(c) => c.encrypt_block(b),
            Cipher::A192(c) => c.encrypt_block(b),
            Cipher::A256(c) => c.encrypt_block(b),
        }
    }

    fn decrypt(&self, block: &mut [u8; BLOCK]) {
        let b = GenericArray::from_mut_slice(block);
        match self {
            Cipher::A128(c) => c.decrypt_block(b),
            Cipher::A192(c) => c.decrypt_block(b),
            Cipher::A256(c) => c.decrypt_block(b),
        }
    }
}

const BLOCK: usize = 16;

/// `data` decrypted in CBC mode from `iv` (a partial last block left out).
fn cbc_decrypt(c: &Cipher, iv: &[u8], data: &[u8]) -> Vec<u8> {
    let mut prev = iv_block(iv);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<BLOCK>().0 {
        let mut b = *chunk;
        c.decrypt(&mut b);
        for (x, p) in b.iter_mut().zip(&prev) {
            *x ^= p;
        }
        out.extend_from_slice(&b);
        prev = *chunk;
    }
    out
}

/// An IV of one block, cut or padded with 0x36.
fn iv_block(iv: &[u8]) -> [u8; BLOCK] {
    let mut b = [0x36; BLOCK];
    let n = iv.len().min(BLOCK);
    b[..n].copy_from_slice(&iv[..n]);
    b
}

/// `data`, padded with zeros to whole blocks, encrypted in CBC mode from
/// `iv`.
fn cbc_encrypt(c: &Cipher, iv: &[u8], data: &[u8]) -> Vec<u8> {
    let mut prev = iv_block(iv);
    let mut out = Vec::with_capacity(data.len().div_ceil(BLOCK) * BLOCK);
    for chunk in data.chunks(BLOCK) {
        let mut b = [0u8; BLOCK];
        b[..chunk.len()].copy_from_slice(chunk);
        for (x, p) in b.iter_mut().zip(&prev) {
            *x ^= p;
        }
        c.encrypt(&mut b);
        out.extend_from_slice(&b);
        prev = b;
    }
    out
}

/// `data` decrypted in ECB mode (Standard encryption).
fn ecb_decrypt(c: &Cipher, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<BLOCK>().0 {
        let mut b = *chunk;
        c.decrypt(&mut b);
        out.extend_from_slice(&b);
    }
    out
}

/// `bytes` made `len` long: cut, or padded with `pad` (MS-OFFCRYPTO
/// 2.3.4.11 pads keys and IVs with 0x36).
fn fit(bytes: &[u8], len: usize, pad: u8) -> Vec<u8> {
    let mut v = bytes[..bytes.len().min(len)].to_vec();
    v.resize(len, pad);
    v
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            c => B64.iter().position(|&b| b == c)? as u32,
        };
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The attributes of the first element whose local name is `name` in
/// `xml` (the agile `EncryptionInfo` is one line of known elements).
fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let mut at = 0;
    while let Some(i) = xml[at..].find(name) {
        let start = at + i;
        let before = xml[..start].chars().next_back();
        let after = xml[start + name.len()..].chars().next();
        if matches!(before, Some('<' | ':')) && matches!(after, Some(' ' | '/' | '>')) {
            let end = xml[start..].find('>')? + start;
            return Some(&xml[start + name.len()..end]);
        }
        at = start + name.len();
    }
    None
}

fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let i = attrs.find(&key)? + key.len();
    let j = attrs[i..].find('"')? + i;
    Some(&attrs[i..j])
}

/// What a file's `EncryptionInfo` and `EncryptedPackage` streams
/// read from its compound file.
fn streams(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(bytes))
        .map_err(|e| unsupported(format!("not a compound file: {e}")))?;
    let mut read = |path: &str| -> Result<Vec<u8>, CryptoError> {
        let mut s = cf
            .open_stream(path)
            .map_err(|_| unsupported(format!("no {} stream", path.trim_start_matches('/'))))?;
        let mut v = Vec::new();
        s.read_to_end(&mut v)
            .map_err(|e| unsupported(e.to_string()))?;
        Ok(v)
    };
    Ok((read("/EncryptionInfo")?, read("/EncryptedPackage")?))
}

/// The package (the `.xlsx` or `.docx` zip) of an encrypted file, with
/// `password`.
pub fn decrypt(bytes: &[u8], password: &str) -> Result<Vec<u8>, CryptoError> {
    let (info, package) = streams(bytes)?;
    if info.len() < 8 || package.len() < 8 {
        return Err(unsupported("damaged"));
    }
    let (major, minor) = (
        u16::from_le_bytes([info[0], info[1]]),
        u16::from_le_bytes([info[2], info[3]]),
    );
    match (major, minor) {
        (4, 4) => agile_decrypt(&info[8..], &package, password),
        (2..=4, 2) => standard_decrypt(&info, &package, password),
        _ => Err(unsupported(format!("version {major}.{minor}"))),
    }
}

/// The block keys of agile encryption (MS-OFFCRYPTO 2.3.4.13 and 2.3.4.14).
const VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const VERIFIER_HASH: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
const HMAC_KEY: [u8; 8] = [0x5f, 0xb2, 0xad, 0x01, 0x0c, 0xb9, 0xe1, 0xf6];
const HMAC_VALUE: [u8; 8] = [0xa0, 0x67, 0x7f, 0x02, 0xb2, 0x2c, 0x84, 0x33];

/// The password's hash after `spin` rounds (MS-OFFCRYPTO 2.3.4.11).
fn spun(hash: Hash, salt: &[u8], password: &str, spin: u32) -> Vec<u8> {
    let mut h = hash.of(&[salt, &utf16(password)]);
    for i in 0..spin {
        h = hash.of(&[&i.to_le_bytes(), &h]);
    }
    h
}

/// The key the spun hash gives for `block` (one of the block keys).
fn derived(hash: Hash, spun: &[u8], block: &[u8], bits: usize) -> Vec<u8> {
    fit(&hash.of(&[spun, block]), bits / 8, 0x36)
}

const SEGMENT: usize = 4096;

fn agile_decrypt(xml: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, CryptoError> {
    let xml = std::str::from_utf8(xml).map_err(|_| unsupported("damaged"))?;
    let key_data = element(xml, "keyData").ok_or_else(|| unsupported("no keyData"))?;
    let key = element(xml, "encryptedKey")
        .ok_or_else(|| unsupported("a certificate rather than a password"))?;
    let num = |a: &str, n: &str| -> Result<usize, CryptoError> {
        attr(a, n)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| unsupported(format!("no {n}")))
    };
    let bytes = |a: &str, n: &str| -> Result<Vec<u8>, CryptoError> {
        attr(a, n)
            .and_then(base64_decode)
            .ok_or_else(|| unsupported(format!("no {n}")))
    };
    let hash_of = |a: &str| -> Result<Hash, CryptoError> {
        let name = attr(a, "hashAlgorithm").unwrap_or("SHA1");
        Hash::named(name).ok_or_else(|| unsupported(name.to_string()))
    };
    for a in [key_data, key] {
        let cipher = attr(a, "cipherAlgorithm").unwrap_or("AES");
        let chaining = attr(a, "cipherChaining").unwrap_or("ChainingModeCBC");
        if cipher != "AES" || chaining != "ChainingModeCBC" {
            return Err(unsupported(format!("{cipher} {chaining}")));
        }
    }
    // The password's key, and the file's key it unlocks.
    let hash = hash_of(key)?;
    let salt = bytes(key, "saltValue")?;
    let bits = num(key, "keyBits")?;
    let spin = num(key, "spinCount")? as u32;
    let h = spun(hash, &salt, password, spin);
    let open = |block: &[u8], name: &str| -> Result<Vec<u8>, CryptoError> {
        let c = Cipher::new(&derived(hash, &h, block, bits))?;
        Ok(cbc_decrypt(&c, &salt, &bytes(key, name)?))
    };
    let input = fit(
        &open(&VERIFIER_INPUT, "encryptedVerifierHashInput")?,
        salt.len(),
        0,
    );
    let verifier = open(&VERIFIER_HASH, "encryptedVerifierHashValue")?;
    let expected = hash.of(&[&input]);
    if verifier.get(..expected.len()) != Some(expected.as_slice()) {
        return Err(CryptoError::WrongPassword);
    }
    let secret_bits = num(key_data, "keyBits")?;
    let secret = fit(&open(&KEY_VALUE, "encryptedKeyValue")?, secret_bits / 8, 0);
    // The package, a segment of 4096 bytes at a time, each with its own
    // IV from the key data's salt and its number.
    let data_hash = hash_of(key_data)?;
    let data_salt = bytes(key_data, "saltValue")?;
    let block = num(key_data, "blockSize")?;
    let c = Cipher::new(&secret)?;
    let size = u64::from_le_bytes(
        package[..8]
            .try_into()
            .map_err(|_| unsupported("damaged"))?,
    );
    let mut out = Vec::with_capacity(package.len());
    for (i, seg) in package[8..].chunks(SEGMENT).enumerate() {
        let iv = fit(
            &data_hash.of(&[&data_salt, &(i as u32).to_le_bytes()]),
            block,
            0x36,
        );
        out.extend(cbc_decrypt(&c, &iv, seg));
    }
    let size = usize::try_from(size).map_err(|_| unsupported("damaged"))?;
    if out.len() < size {
        return Err(unsupported("damaged"));
    }
    out.truncate(size);
    Ok(out)
}

fn le32(b: &[u8], at: usize) -> Result<u32, CryptoError> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| unsupported("damaged"))
}

/// Standard encryption (MS-OFFCRYPTO 2.3.4.5 to 2.3.4.9): AES in ECB
/// mode, the key from SHA-1 spun 50,000 times.
fn standard_decrypt(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, CryptoError> {
    let header_size = le32(info, 8)? as usize;
    let header = info
        .get(12..12 + header_size)
        .ok_or_else(|| unsupported("damaged"))?;
    let alg = le32(header, 8)?;
    let alg_hash = le32(header, 12)?;
    let key_bits = le32(header, 16)? as usize;
    if !matches!(alg, 0x660E..=0x6610) {
        return Err(unsupported(if alg == 0x6801 {
            "RC4".to_string()
        } else {
            format!("algorithm {alg:#x}")
        }));
    }
    if alg_hash != 0x8004 && alg_hash != 0 {
        return Err(unsupported(format!("hash {alg_hash:#x}")));
    }
    let v = &info[12 + header_size..];
    let salt_size = le32(v, 0)? as usize;
    let salt = v
        .get(4..4 + salt_size)
        .ok_or_else(|| unsupported("damaged"))?;
    let verifier = v
        .get(4 + salt_size..20 + salt_size)
        .ok_or_else(|| unsupported("damaged"))?;
    let hash_size = le32(v, 20 + salt_size)? as usize;
    let verifier_hash = v
        .get(24 + salt_size..24 + salt_size + 32)
        .ok_or_else(|| unsupported("damaged"))?;
    let h = spun(Hash::Sha1, salt, password, 50_000);
    let h = Hash::Sha1.of(&[&h, &0u32.to_le_bytes()]);
    let mut x1 = [0x36u8; 64];
    let mut x2 = [0x5cu8; 64];
    for (i, b) in h.iter().enumerate() {
        x1[i] ^= b;
        x2[i] ^= b;
    }
    let mut key = Hash::Sha1.of(&[&x1]);
    key.extend(Hash::Sha1.of(&[&x2]));
    let c = Cipher::new(&key[..key_bits / 8])?;
    let plain = ecb_decrypt(&c, verifier);
    let expected = Hash::Sha1.of(&[&plain]);
    let got = ecb_decrypt(&c, verifier_hash);
    if got.get(..hash_size.min(expected.len())) != Some(&expected[..hash_size.min(expected.len())])
    {
        return Err(CryptoError::WrongPassword);
    }
    let size = u64::from_le_bytes(
        package[..8]
            .try_into()
            .map_err(|_| unsupported("damaged"))?,
    );
    let mut out = ecb_decrypt(&c, &package[8..]);
    let size = usize::try_from(size).map_err(|_| unsupported("damaged"))?;
    if out.len() < size {
        return Err(unsupported("damaged"));
    }
    out.truncate(size);
    Ok(out)
}

/// `package` (an `.xlsx` or `.docx` zip) encrypted with `password` as
/// Excel and Word encrypt a file with a password to open: agile encryption, AES-256 and
/// SHA-512 spun 100,000 times, with the data integrity check and the
/// data spaces Excel and Word expect. `random` gives the salts and keys.
pub fn encrypt(
    package: &[u8],
    password: &str,
    random: &mut dyn FnMut() -> u64,
) -> Result<Vec<u8>, CryptoError> {
    let mut bytes = |n: usize| -> Vec<u8> {
        let mut v = Vec::with_capacity(n + 8);
        while v.len() < n {
            v.extend_from_slice(&random().to_le_bytes());
        }
        v.truncate(n);
        v
    };
    let hash = Hash::Sha512;
    let (key_salt, data_salt) = (bytes(16), bytes(16));
    let secret = bytes(32);
    let verifier_input = bytes(16);
    let hmac_key = bytes(64);
    const SPIN: u32 = 100_000;
    let h = spun(hash, &key_salt, password, SPIN);
    let lock = |block: &[u8], data: &[u8]| -> Result<Vec<u8>, CryptoError> {
        let c = Cipher::new(&derived(hash, &h, block, 256))?;
        Ok(cbc_encrypt(&c, &key_salt, data))
    };
    let encrypted_input = lock(&VERIFIER_INPUT, &verifier_input)?;
    let encrypted_hash = lock(&VERIFIER_HASH, &hash.of(&[&verifier_input]))?;
    let encrypted_key = lock(&KEY_VALUE, &secret)?;
    // The package, a segment at a time.
    let c = Cipher::new(&secret)?;
    let mut stream = (package.len() as u64).to_le_bytes().to_vec();
    for (i, seg) in package.chunks(SEGMENT).enumerate() {
        let iv = fit(
            &hash.of(&[&data_salt, &(i as u32).to_le_bytes()]),
            BLOCK,
            0x36,
        );
        stream.extend(cbc_encrypt(&c, &iv, seg));
    }
    // The data integrity check: an HMAC of the encrypted package.
    let iv_key = fit(&hash.of(&[&data_salt, &HMAC_KEY]), BLOCK, 0x36);
    let iv_value = fit(&hash.of(&[&data_salt, &HMAC_VALUE]), BLOCK, 0x36);
    let encrypted_hmac_key = cbc_encrypt(&c, &iv_key, &hmac_key);
    let encrypted_hmac_value = cbc_encrypt(&c, &iv_value, &hmac(hash, &hmac_key, &stream));
    let xml = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
            "<encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\" ",
            "xmlns:p=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\" ",
            "xmlns:c=\"http://schemas.microsoft.com/office/2006/keyEncryptor/certificate\">",
            "<keyData saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" hashSize=\"64\" ",
            "cipherAlgorithm=\"AES\" cipherChaining=\"ChainingModeCBC\" hashAlgorithm=\"SHA512\" ",
            "saltValue=\"{}\"/>",
            "<dataIntegrity encryptedHmacKey=\"{}\" encryptedHmacValue=\"{}\"/>",
            "<keyEncryptors><keyEncryptor uri=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\">",
            "<p:encryptedKey spinCount=\"{}\" saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" ",
            "hashSize=\"64\" cipherAlgorithm=\"AES\" cipherChaining=\"ChainingModeCBC\" ",
            "hashAlgorithm=\"SHA512\" saltValue=\"{}\" encryptedVerifierHashInput=\"{}\" ",
            "encryptedVerifierHashValue=\"{}\" encryptedKeyValue=\"{}\"/>",
            "</keyEncryptor></keyEncryptors></encryption>"
        ),
        base64_encode(&data_salt),
        base64_encode(&encrypted_hmac_key),
        base64_encode(&encrypted_hmac_value),
        SPIN,
        base64_encode(&key_salt),
        base64_encode(&encrypted_input),
        base64_encode(&encrypted_hash),
        base64_encode(&encrypted_key),
    );
    let mut info = vec![4, 0, 4, 0, 0x40, 0, 0, 0];
    info.extend_from_slice(xml.as_bytes());
    compound(&info, &stream)
}

/// A string as MS-OFFCRYPTO's UNICODE-LP-P4: its length in bytes, its
/// UTF-16, padded to four bytes.
fn lp_p4(s: &str) -> Vec<u8> {
    let u = utf16(s);
    let mut v = (u.len() as u32).to_le_bytes().to_vec();
    v.extend(&u);
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
    v
}

/// The compound file of an encrypted file: its two streams and the
/// `\u{6}DataSpaces` storage saying the package is encrypted
/// (MS-OFFCRYPTO 2.1 and 2.3.4.1), in version 3 with 512-byte sectors
/// as Excel writes it.
fn compound(info: &[u8], package: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let io = |e: std::io::Error| unsupported(e.to_string());
    let mut cf = cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))
        .map_err(io)?;
    let version = |v: &mut Vec<u8>| {
        for _ in 0..3 {
            v.extend_from_slice(&1u16.to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes());
        }
    };
    // `Version`: the data spaces' feature and its versions.
    let mut version_stream = lp_p4("Microsoft.Container.DataSpaces");
    version(&mut version_stream);
    // `DataSpaceMap`: the encrypted package is in the strong encryption
    // data space.
    let mut entry = Vec::new();
    entry.extend_from_slice(&1u32.to_le_bytes());
    entry.extend_from_slice(&0u32.to_le_bytes());
    entry.extend(lp_p4("EncryptedPackage"));
    entry.extend(lp_p4("StrongEncryptionDataSpace"));
    let mut map = Vec::new();
    map.extend_from_slice(&8u32.to_le_bytes());
    map.extend_from_slice(&1u32.to_le_bytes());
    map.extend_from_slice(&((entry.len() + 4) as u32).to_le_bytes());
    map.extend(entry);
    // `DataSpaceInfo/StrongEncryptionDataSpace`: one transform.
    let mut space = Vec::new();
    space.extend_from_slice(&8u32.to_le_bytes());
    space.extend_from_slice(&1u32.to_le_bytes());
    space.extend(lp_p4("StrongEncryptionTransform"));
    // `TransformInfo/StrongEncryptionTransform/\u{6}Primary`.
    let id = lp_p4("{FF9A3F03-56EF-4613-BDD5-5A41C1D07246}");
    let mut primary = Vec::new();
    primary.extend_from_slice(&((8 + id.len()) as u32).to_le_bytes());
    primary.extend_from_slice(&1u32.to_le_bytes());
    primary.extend(id);
    primary.extend(lp_p4("Microsoft.Container.EncryptionTransform"));
    version(&mut primary);
    primary.extend_from_slice(&0u32.to_le_bytes());
    primary.extend_from_slice(&0u32.to_le_bytes());
    primary.extend_from_slice(&0u32.to_le_bytes());
    primary.extend_from_slice(&4u32.to_le_bytes());
    for storage in [
        "/\u{6}DataSpaces",
        "/\u{6}DataSpaces/DataSpaceInfo",
        "/\u{6}DataSpaces/TransformInfo",
        "/\u{6}DataSpaces/TransformInfo/StrongEncryptionTransform",
    ] {
        cf.create_storage(storage).map_err(io)?;
    }
    for (path, data) in [
        ("/\u{6}DataSpaces/Version", version_stream.as_slice()),
        ("/\u{6}DataSpaces/DataSpaceMap", map.as_slice()),
        (
            "/\u{6}DataSpaces/DataSpaceInfo/StrongEncryptionDataSpace",
            space.as_slice(),
        ),
        (
            "/\u{6}DataSpaces/TransformInfo/StrongEncryptionTransform/\u{6}Primary",
            primary.as_slice(),
        ),
        ("/EncryptionInfo", info),
        ("/EncryptedPackage", package),
    ] {
        let mut s = cf.create_stream(path).map_err(io)?;
        s.write_all(data).map_err(io)?;
    }
    cf.flush().map_err(io)?;
    Ok(cf.into_inner().into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter() -> impl FnMut() -> u64 {
        let mut n = 0x9e37_79b9_7f4a_7c15u64;
        move || {
            n = n
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            n
        }
    }

    #[test]
    fn agile_round_trip() {
        let package: Vec<u8> = (0..10_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let file = encrypt(&package, "kalem", &mut counter()).unwrap();
        assert!(file.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]));
        // A compound file of version 3, as Excel writes it: LibreOffice
        // opened none of version 4.
        assert_eq!(u16::from_le_bytes([file[0x1A], file[0x1B]]), 3);
        assert_eq!(decrypt(&file, "kalem").unwrap(), package);
        assert_eq!(decrypt(&file, "Kalem"), Err(CryptoError::WrongPassword));
        // The data spaces Excel looks for.
        let mut cf = cfb::CompoundFile::open(Cursor::new(file)).unwrap();
        for p in [
            "/\u{6}DataSpaces/Version",
            "/\u{6}DataSpaces/DataSpaceMap",
            "/\u{6}DataSpaces/DataSpaceInfo/StrongEncryptionDataSpace",
            "/\u{6}DataSpaces/TransformInfo/StrongEncryptionTransform/\u{6}Primary",
        ] {
            assert!(cf.open_stream(p).is_ok(), "{p}");
        }
    }

    #[test]
    fn base64_both_ways() {
        for s in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            assert_eq!(base64_decode(&base64_encode(s)).unwrap(), s);
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn hmac_as_rfc_4231_gives_it() {
        // RFC 4231, test case 2.
        let mac = hmac(Hash::Sha512, b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            mac.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737"
        );
    }

    #[test]
    fn standard_encryption_read() {
        // A file in Standard encryption, made here by its rules (Excel
        // 2007's): AES-128, SHA-1 spun 50,000 times, ECB.
        let password = "kalem";
        let salt = [7u8; 16];
        let h = spun(Hash::Sha1, &salt, password, 50_000);
        let h = Hash::Sha1.of(&[&h, &0u32.to_le_bytes()]);
        let mut x1 = [0x36u8; 64];
        for (i, b) in h.iter().enumerate() {
            x1[i] ^= b;
        }
        let key = Hash::Sha1.of(&[&x1]);
        let c = Cipher::new(&key[..16]).unwrap();
        let ecb = |data: &[u8]| -> Vec<u8> {
            let mut out = Vec::new();
            for chunk in data.chunks(BLOCK) {
                let mut b = [0u8; BLOCK];
                b[..chunk.len()].copy_from_slice(chunk);
                c.encrypt(&mut b);
                out.extend(b);
            }
            out
        };
        let verifier = [3u8; 16];
        let mut info = vec![3, 0, 2, 0, 0x24, 0, 0, 0];
        let mut header = Vec::new();
        for v in [0x24u32, 0, 0x660E, 0x8004, 128, 0x18, 0, 0] {
            header.extend_from_slice(&v.to_le_bytes());
        }
        header.extend(utf16(
            "Microsoft Enhanced RSA and AES Cryptographic Provider\0",
        ));
        info.extend_from_slice(&(header.len() as u32).to_le_bytes());
        info.extend(&header);
        info.extend_from_slice(&16u32.to_le_bytes());
        info.extend(salt);
        info.extend(ecb(&verifier));
        info.extend_from_slice(&20u32.to_le_bytes());
        info.extend(ecb(&Hash::Sha1.of(&[&verifier])));
        let package = b"PK\x03\x04 a workbook".to_vec();
        let mut stream = (package.len() as u64).to_le_bytes().to_vec();
        stream.extend(ecb(&package));
        let file = compound(&info, &stream).unwrap();
        assert_eq!(decrypt(&file, password).unwrap(), package);
        assert_eq!(decrypt(&file, "other"), Err(CryptoError::WrongPassword));
    }
}
