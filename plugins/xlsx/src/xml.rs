//! The span-keeping XML reader of the shared Office layer
//! ([`kalem_ooxml::xml`]), with SpreadsheetML's escaping: a string the
//! workbook writes is an ST_Xstring (ECMA-376 part 1, 22.9.2.19), whose
//! characters XML 1.0 forbids are written as `_xHHHH_`, so [`escape`] and
//! [`set_attr`] here are the ST_Xstring ones.

pub use kalem_ooxml::xml::escape_st_xstring as escape;
pub use kalem_ooxml::xml::*;

/// A start tag's text with one attribute set (added before the closing
/// `>` or `/>` when absent), every other byte kept; the value escaped as
/// an ST_Xstring.
pub fn set_attr(tag: &str, name: &str, value: &str) -> String {
    set_attr_escaped(tag, name, &escape(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreadsheet_escapes() {
        assert_eq!(escape("a\u{1}<"), "a_x0001_&lt;");
        assert_eq!(set_attr("<a/>", "k", "\u{2}"), r#"<a k="_x0002_"/>"#);
    }
}
