//! The Office Open XML layer the Office plugins share (ECMA-376 part 2,
//! and MS-OFFCRYPTO): a package read and written part by part
//! ([`package`]), a pull reader that reports the byte span of every tag
//! ([`xml`]), relationships and content types ([`rels`]), files with a
//! password to open ([`crypto`]) and the theme ([`theme`]).
//!
//! Everything here serves the rule the plugins keep: an edit rewrites
//! the bytes of the element it touches, every other byte of the part and
//! every other entry of the package is copied as it was, and a save
//! without edits returns the input unchanged. The workbook plugin
//! (`xlsx`) and the document plugin (`docx`) build their formats on it.

pub mod crypto;
pub mod package;
pub mod rels;
pub mod theme;
pub mod xml;
