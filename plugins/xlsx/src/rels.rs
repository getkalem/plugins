//! Relationships and content types (ECMA-376 part 2, 9.3 and 10.1): the
//! shared Office layer's ([`kalem_ooxml::rels`]), with the relationship
//! types a workbook follows.

pub use kalem_ooxml::rels::*;

/// The relationship types this plugin follows.
pub mod kind {
    /// The workbook part from the package root.
    pub const OFFICE_DOCUMENT: &str = "officeDocument";
    /// A worksheet from the workbook.
    pub const WORKSHEET: &str = "worksheet";
    /// A chart sheet.
    pub const CHARTSHEET: &str = "chartsheet";
    /// A dialog sheet (Excel 5).
    pub const DIALOGSHEET: &str = "dialogsheet";
    /// An Excel 4 macro sheet.
    pub const MACROSHEET: &str = "macrosheet";
    /// The shared string table.
    pub const SHARED_STRINGS: &str = "sharedStrings";
    /// The style sheet.
    pub const STYLES: &str = "styles";
    /// The calculation chain.
    pub const CALC_CHAIN: &str = "calcChain";
    /// Cell comments of a sheet.
    pub const COMMENTS: &str = "comments";
    /// The VBA project.
    pub const VBA_PROJECT: &str = "vbaProject";
}
