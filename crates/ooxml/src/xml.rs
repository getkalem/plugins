//! A small XML pull reader that reports byte spans.
//!
//! Package parts are rewritten by splicing: an edit replaces the bytes of
//! one element and keeps every other byte, so the reader's job is to say
//! where each tag starts and ends. OOXML parts are well-formed XML without a
//! document type declaration; namespace prefixes are dropped (`x:c` is `c`),
//! since some producers prefix the main namespace and others do not.

use std::borrow::Cow;
use std::ops::Range;

/// One token of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token<'a> {
    /// `<name …>`; `empty` for `<name …/>`.
    Start(Tag<'a>),
    /// `</name>`.
    End {
        /// Local name.
        name: &'a str,
        /// The bytes of the end tag.
        span: Range<usize>,
    },
    /// Character data between tags, still escaped.
    Text {
        /// The raw text.
        raw: &'a str,
        /// Where it lies.
        span: Range<usize>,
    },
}

/// A start or empty tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag<'a> {
    /// The local name, prefix dropped.
    pub name: &'a str,
    /// The name as written, prefix included.
    pub qname: &'a str,
    /// The raw text between the name and `>` or `/>`.
    attrs: &'a str,
    /// Whether the tag closes itself.
    pub empty: bool,
    /// The bytes of the tag, `<` to `>`.
    pub span: Range<usize>,
}

impl<'a> Tag<'a> {
    /// The attributes, prefixes dropped, values unescaped.
    pub fn attrs(&self) -> Vec<(&'a str, Cow<'a, str>)> {
        let mut out = Vec::new();
        let s = self.attrs;
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let ns = i;
            while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            let name = &s[ns..i];
            while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'=') {
                i += 1;
            }
            let Some(&q) = b.get(i) else { break };
            if q != b'"' && q != b'\'' {
                break;
            }
            let vs = i + 1;
            let Some(len) = s[vs..].find(q as char) else {
                break;
            };
            i = vs + len + 1;
            if !name.is_empty() {
                out.push((local(name), unescape(&s[vs..vs + len])));
            }
        }
        out
    }

    /// One attribute by local name.
    pub fn attr(&self, name: &str) -> Option<Cow<'a, str>> {
        self.attrs()
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v)
    }

    /// One attribute by its qualified name as written (`r:id`, `xml:space`).
    pub fn attr_qualified(&self, qname: &str) -> Option<Cow<'a, str>> {
        let s = self.attrs;
        let mut from = 0;
        while let Some(p) = s[from..].find(qname) {
            let at = from + p;
            let before_ok = at == 0 || s.as_bytes()[at - 1].is_ascii_whitespace();
            let rest = s[at + qname.len()..].trim_start();
            if before_ok && rest.starts_with('=') {
                let rest = rest[1..].trim_start();
                let q = rest.chars().next()?;
                let end = rest[1..].find(q)?;
                return Some(unescape(&rest[1..1 + end]));
            }
            from = at + qname.len();
        }
        None
    }
}

/// The prefix of a qualified name with its colon (`x:`), or empty.
pub fn prefix(qname: &str) -> &str {
    qname.rfind(':').map_or("", |p| &qname[..=p])
}

/// A start tag's text with one attribute set (added before the closing
/// `>` or `/>` when absent), every other byte kept; the value escaped
/// as XML.
pub fn set_attr(tag: &str, name: &str, value: &str) -> String {
    set_attr_escaped(tag, name, &escape(value))
}

/// [`set_attr`] with a value already escaped, for a format with escapes
/// of its own (SpreadsheetML's [`escape_st_xstring`]).
pub fn set_attr_escaped(tag: &str, name: &str, value: &str) -> String {
    let b = tag.as_bytes();
    let mut i = 1;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
        i += 1;
    }
    // Walk the attributes to find `name`.
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] == b'>' || b[i] == b'/' {
            break;
        }
        let ns = i;
        while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        let an = &tag[ns..i];
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'=') {
            i += 1;
        }
        let Some(&q) = b.get(i) else { break };
        let vs = i + 1;
        let Some(len) = tag[vs..].find(q as char) else {
            break;
        };
        if an == name {
            return format!("{}{}{}", &tag[..vs], value, &tag[vs + len..]);
        }
        i = vs + len + 1;
    }
    // At the start tag's end, not the text's: an element with children
    // keeps its closing tag bare.
    let close = i.min(tag.len());
    let head = tag[..close].trim_end();
    format!("{head} {name}=\"{value}\"{}", &tag[close..])
}

/// A start tag's text without one attribute.
pub fn remove_attr(tag: &str, name: &str) -> String {
    let needle_at = |from: usize| -> Option<(usize, usize)> {
        let mut at = from;
        while let Some(p) = tag[at..].find(name) {
            let s = at + p;
            let before = s > 0 && tag.as_bytes()[s - 1].is_ascii_whitespace();
            let rest = tag[s + name.len()..].trim_start();
            if before && rest.starts_with('=') {
                let off = tag.len() - rest.len() + 1;
                let r2 = tag[off..].trim_start();
                let qpos = tag.len() - r2.len();
                let q = r2.chars().next()?;
                let end = qpos + 1 + r2[1..].find(q)? + 1;
                let mut start = s;
                while start > 0 && tag.as_bytes()[start - 1].is_ascii_whitespace() {
                    start -= 1;
                }
                return Some((start, end));
            }
            at = s + name.len();
        }
        None
    };
    match needle_at(0) {
        Some((s, e)) => format!("{}{}", &tag[..s], &tag[e..]),
        None => tag.to_owned(),
    }
}

/// The local part of a qualified name.
pub fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// A pull reader over a part's text.
#[derive(Debug)]
pub struct Reader<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `s`.
    pub fn new(s: &'a str) -> Self {
        Self { s, pos: 0 }
    }

    /// The next token; comments, processing instructions and the XML
    /// declaration are skipped, CDATA comes as text.
    pub fn next_token(&mut self) -> Option<Token<'a>> {
        let s = self.s;
        loop {
            if self.pos >= s.len() {
                return None;
            }
            let start = self.pos;
            if !s[start..].starts_with('<') {
                let end = s[start..].find('<').map_or(s.len(), |p| start + p);
                self.pos = end;
                return Some(Token::Text {
                    raw: &s[start..end],
                    span: start..end,
                });
            }
            let rest = &s[start..];
            if rest.starts_with("<?") {
                self.pos = rest.find("?>").map_or(s.len(), |p| start + p + 2);
                continue;
            }
            if rest.starts_with("<!--") {
                self.pos = rest.find("-->").map_or(s.len(), |p| start + p + 3);
                continue;
            }
            if rest.starts_with("<![CDATA[") {
                let end = rest.find("]]>").map_or(s.len(), |p| start + p + 3);
                self.pos = end;
                return Some(Token::Text {
                    raw: &s[start..end],
                    span: start..end,
                });
            }
            if rest.starts_with("<!") {
                self.pos = rest.find('>').map_or(s.len(), |p| start + p + 1);
                continue;
            }
            // A tag: find its `>` outside quoted attribute values.
            let b = rest.as_bytes();
            let mut i = 1;
            let mut quote = 0u8;
            while i < b.len() {
                match b[i] {
                    q @ (b'"' | b'\'') if quote == 0 => quote = q,
                    q if q == quote => quote = 0,
                    b'>' if quote == 0 => break,
                    _ => {}
                }
                i += 1;
            }
            let end = (start + i + 1).min(s.len());
            self.pos = end;
            let inner = &s[start + 1..end - 1];
            if let Some(name) = inner.strip_prefix('/') {
                return Some(Token::End {
                    name: local(name.trim()),
                    span: start..end,
                });
            }
            let empty = inner.ends_with('/');
            let inner = if empty {
                &inner[..inner.len() - 1]
            } else {
                inner
            };
            let name_end = inner
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(inner.len());
            return Some(Token::Start(Tag {
                name: local(&inner[..name_end]),
                qname: &inner[..name_end],
                attrs: &inner[name_end..],
                empty,
                span: start..end,
            }));
        }
    }

    /// The text content of the element whose start tag was just read, up to
    /// its end tag, unescaped, nested markup dropped; and the end tag's span.
    pub fn text_until_end(&mut self, name: &str) -> (String, usize) {
        let mut out = String::new();
        let mut depth = 0;
        while let Some(t) = self.next_token() {
            match t {
                Token::Text { raw, .. } => out.push_str(&text(raw)),
                Token::Start(tag) if !tag.empty => depth += 1,
                Token::Start(_) => {}
                Token::End { name: n, span } => {
                    if depth == 0 && n == name {
                        return (out, span.end);
                    }
                    depth -= 1;
                }
            }
        }
        (out, self.s.len())
    }

    /// Skips to the end of the element whose start tag was just read and
    /// returns the position after its end tag.
    pub fn skip_element(&mut self) -> usize {
        let mut depth = 0;
        while let Some(t) = self.next_token() {
            match t {
                Token::Start(tag) if !tag.empty => depth += 1,
                Token::End { span, .. } => {
                    if depth == 0 {
                        return span.end;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        self.s.len()
    }

    /// The current byte position.
    pub fn pos(&self) -> usize {
        self.pos
    }
}

/// Character data, CDATA sections opened, references resolved.
pub fn text(raw: &str) -> Cow<'_, str> {
    if let Some(inner) = raw.strip_prefix("<![CDATA[") {
        return Cow::Borrowed(inner.strip_suffix("]]>").unwrap_or(inner));
    }
    unescape(raw)
}

/// Resolves the five predefined entities and character references.
pub fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(semi) = rest.find(';').filter(|&n| n < 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Escapes text for character data or an attribute value: the five
/// characters XML gives meaning to, nothing else. Characters XML 1.0
/// forbids (controls but tab, line feed and carriage return) are the
/// caller's to refuse; a format with escapes for them has its own
/// ([`escape_st_xstring`]).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// Whether XML 1.0 allows `c` in a document.
pub fn allowed_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && !matches!(c, '\u{FFFE}' | '\u{FFFF}'))
}

/// Escapes text for SpreadsheetML (ST_Xstring, ECMA-376 part 1,
/// 22.9.2.19): as [`escape`], and the characters XML 1.0 forbids written
/// as `_xHHHH_` escapes, a literal `_xHHHH_` keeping its `_` as `_x005F_`.
pub fn escape_st_xstring(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.char_indices() {
        match c {
            // A literal `_xHHHH_` would read as an escape: its `_` is escaped.
            '_' if looks_escaped(&s[i..]) => out.push_str("_x005F_"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {
                out.push_str(&format!("_x{:04X}_", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn looks_escaped(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 7 && b[1] == b'x' && b[2..6].iter().all(u8::is_ascii_hexdigit) && b[6] == b'_'
}

/// Resolves SpreadsheetML's `_xHHHH_` escapes in a string value.
pub fn unescape_st_xstring(s: &str) -> Cow<'_, str> {
    if !s.contains("_x") {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("_x") {
        out.push_str(&rest[..p]);
        let cand = &rest[p..];
        let hex = cand.get(2..6);
        let decoded = hex
            .filter(|h| {
                h.bytes().all(|b| b.is_ascii_hexdigit()) && cand.as_bytes().get(6) == Some(&b'_')
            })
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .and_then(char::from_u32);
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &cand[7..];
            }
            None => {
                out.push_str("_x");
                rest = &cand[2..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_set_on_the_start_tag() {
        assert_eq!(set_attr(r#"<a k="1"/>"#, "k", "2"), r#"<a k="2"/>"#);
        assert_eq!(set_attr("<a/>", "k", "2"), r#"<a k="2"/>"#);
        assert_eq!(set_attr("<a>", "k", "2"), r#"<a k="2">"#);
        // An element with children: the start tag gets it, not the end.
        assert_eq!(
            set_attr(r#"<xf a="1"><b/></xf>"#, "c", "2"),
            r#"<xf a="1" c="2"><b/></xf>"#
        );
    }

    #[test]
    fn tokens_and_spans() {
        let s = r#"<?xml version="1.0"?><x:a k="1 &amp; 2"><b/>t&lt;<!-- c --></x:a>"#;
        let mut r = Reader::new(s);
        let Some(Token::Start(a)) = r.next_token() else {
            panic!()
        };
        assert_eq!(a.name, "a");
        assert_eq!(a.attr("k").as_deref(), Some("1 & 2"));
        assert_eq!(&s[a.span.clone()], r#"<x:a k="1 &amp; 2">"#);
        let Some(Token::Start(b)) = r.next_token() else {
            panic!()
        };
        assert!(b.empty);
        let Some(Token::Text { raw, .. }) = r.next_token() else {
            panic!()
        };
        assert_eq!(text(raw), "t<");
        assert!(matches!(r.next_token(), Some(Token::End { name: "a", .. })));
        assert_eq!(r.next_token(), None);
    }

    #[test]
    fn greater_than_inside_attribute() {
        let mut r = Reader::new(r#"<f a="x>y">1</f>"#);
        let Some(Token::Start(t)) = r.next_token() else {
            panic!()
        };
        assert_eq!(t.attr("a").as_deref(), Some("x>y"));
        assert_eq!(r.text_until_end("f").0, "1");
    }

    #[test]
    fn attributes_set_and_removed() {
        assert_eq!(
            set_attr(r#"<row r="2" spans="1:2">"#, "spans", "1:5"),
            r#"<row r="2" spans="1:5">"#
        );
        assert_eq!(
            set_attr(r#"<calcPr calcId="1"/>"#, "fullCalcOnLoad", "1"),
            r#"<calcPr calcId="1" fullCalcOnLoad="1"/>"#
        );
        assert_eq!(set_attr("<a>", "k", "v"), r#"<a k="v">"#);
        assert_eq!(
            remove_attr(r#"<c r="A1" t="s" s="2">"#, "t"),
            r#"<c r="A1" s="2">"#
        );
        assert_eq!(prefix("x:row"), "x:");
    }

    #[test]
    fn st_xstring() {
        assert_eq!(unescape_st_xstring("a_x000D_b"), "a\rb");
        assert_eq!(unescape_st_xstring("_x_y"), "_x_y");
        assert_eq!(escape_st_xstring("a\u{1}<"), "a_x0001_&lt;");
        assert_eq!(
            unescape_st_xstring(&escape_st_xstring("_x0041_")),
            "_x0041_"
        );
        assert_eq!(escape("_x0041_ & <"), "_x0041_ &amp; &lt;");
        assert!(!allowed_char('\u{1}') && allowed_char('\t') && allowed_char('ğ'));
    }
}
