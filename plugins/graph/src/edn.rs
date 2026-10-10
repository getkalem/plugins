//! A reader of EDN, the notation of Logseq's `logseq/config.edn`: maps,
//! vectors, lists, sets, keywords, strings, numbers, booleans and `nil`,
//! with comments (`;`), discarded forms (`#_`), tagged forms and regular
//! expressions (`#"…"`, read as strings). It reads what the plugin needs
//! from the file and keeps no position; a file that does not read gives
//! an error with the line, and the plugin then uses Logseq's defaults.

/// A value of EDN.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `nil`.
    Nil,
    /// `true`, `false`.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A number with a fraction or an exponent.
    Float(f64),
    /// A string, its escapes resolved.
    Str(String),
    /// A character: `\a`, `\newline`.
    Char(char),
    /// A keyword without its colon: `preferred-format`,
    /// `journal/page-title-format`.
    Keyword(String),
    /// A symbol.
    Symbol(String),
    /// `( … )`.
    List(Vec<Value>),
    /// `[ … ]`.
    Vector(Vec<Value>),
    /// `{ … }`, its pairs in order.
    Map(Vec<(Value, Value)>),
    /// `#{ … }`.
    Set(Vec<Value>),
    /// `#tag value`.
    Tagged(String, Box<Value>),
}

impl Value {
    /// A map's value for the keyword `key` (without its colon).
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(pairs) => pairs
                .iter()
                .find(|(k, _)| matches!(k, Value::Keyword(n) if n == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// A string's text, a keyword's or a symbol's name.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Keyword(s) | Value::Symbol(s) => Some(s),
            _ => None,
        }
    }

    /// A boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The items of a vector, a list or a set.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Vector(v) | Value::List(v) | Value::Set(v) => v,
            _ => &[],
        }
    }

    /// The items' texts (strings, keywords, symbols).
    pub fn strings(&self) -> Vec<String> {
        self.items()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    }
}

/// `text` read as one EDN value.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut r = Reader {
        s: text.as_bytes(),
        text,
        at: 0,
    };
    r.skip();
    let v = r.value()?;
    r.skip();
    if r.at < r.s.len() {
        return Err(r.error("more after the value"));
    }
    Ok(v)
}

struct Reader<'a> {
    s: &'a [u8],
    text: &'a str,
    at: usize,
}

/// Bytes that end a symbol, a keyword or a number.
fn delimiter(b: u8) -> bool {
    b.is_ascii_whitespace()
        || matches!(
            b,
            b',' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b';'
        )
}

impl Reader<'_> {
    fn error(&self, what: &str) -> String {
        let line = self.text[..self.at.min(self.text.len())]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
            + 1;
        format!("line {line}: {what}")
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    /// Whitespace, commas, comments and discarded forms.
    fn skip(&mut self) {
        loop {
            match self.peek() {
                Some(b) if b.is_ascii_whitespace() || b == b',' => self.at += 1,
                Some(b';') => {
                    while self.peek().is_some_and(|b| b != b'\n') {
                        self.at += 1;
                    }
                }
                Some(b'#') if self.s.get(self.at + 1) == Some(&b'_') => {
                    self.at += 2;
                    self.skip();
                    let _ = self.value();
                }
                _ => return,
            }
        }
    }

    fn token(&mut self) -> &str {
        let start = self.at;
        while self.peek().is_some_and(|b| !delimiter(b)) {
            self.at += 1;
        }
        &self.text[start..self.at]
    }

    fn seq(&mut self, close: u8) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        loop {
            self.skip();
            match self.peek() {
                None => return Err(self.error(&format!("no closing {}", close as char))),
                Some(b) if b == close => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(_) => out.push(self.value()?),
            }
        }
    }

    /// A string at its opening quote; a regular expression's (`raw`)
    /// keeps its backslashes, but for an escaped quote.
    fn string(&mut self, raw: bool) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.at..];
            let Some(c) = rest.chars().next() else {
                return Err(self.error("a string not closed"));
            };
            self.at += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(e) = self.text[self.at..].chars().next() else {
                        return Err(self.error("a string not closed"));
                    };
                    self.at += e.len_utf8();
                    if raw {
                        if e != '"' {
                            out.push('\\');
                        }
                        out.push(e);
                        continue;
                    }
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'u' => {
                            let hex = self.text.get(self.at..self.at + 4).unwrap_or("");
                            let c = u32::from_str_radix(hex, 16)
                                .ok()
                                .and_then(char::from_u32)
                                .ok_or_else(|| self.error("a bad \\u escape"))?;
                            out.push(c);
                            self.at += 4;
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip();
        let Some(b) = self.peek() else {
            return Err(self.error("a value missing"));
        };
        match b {
            b'(' => {
                self.at += 1;
                Ok(Value::List(self.seq(b')')?))
            }
            b'[' => {
                self.at += 1;
                Ok(Value::Vector(self.seq(b']')?))
            }
            b'{' => {
                self.at += 1;
                let items = self.seq(b'}')?;
                if items.len() % 2 != 0 {
                    return Err(self.error("a map with an odd number of forms"));
                }
                let mut pairs = Vec::new();
                let mut it = items.into_iter();
                while let (Some(k), Some(v)) = (it.next(), it.next()) {
                    pairs.push((k, v));
                }
                Ok(Value::Map(pairs))
            }
            b')' | b']' | b'}' => Err(self.error(&format!("an unexpected {}", b as char))),
            b'"' => Ok(Value::Str(self.string(false)?)),
            b'\'' | b'`' | b'@' => {
                self.at += 1;
                self.value()
            }
            b'~' => {
                self.at += 1;
                if self.peek() == Some(b'@') {
                    self.at += 1;
                }
                self.value()
            }
            b'^' => {
                // Metadata, then the value it is attached to.
                self.at += 1;
                let _ = self.value()?;
                self.value()
            }
            b'#' => {
                self.at += 1;
                match self.peek() {
                    Some(b'{') => {
                        self.at += 1;
                        Ok(Value::Set(self.seq(b'}')?))
                    }
                    Some(b'"') => Ok(Value::Str(self.string(true)?)),
                    Some(b'(') => {
                        // An anonymous function.
                        self.at += 1;
                        Ok(Value::List(self.seq(b')')?))
                    }
                    _ => {
                        let tag = self.token().to_string();
                        let v = self.value()?;
                        Ok(Value::Tagged(tag, Box::new(v)))
                    }
                }
            }
            b'\\' => {
                self.at += 1;
                let start = self.at;
                // One character at least, then a name.
                if let Some(c) = self.text[self.at..].chars().next() {
                    self.at += c.len_utf8();
                }
                while self.peek().is_some_and(|b| !delimiter(b)) {
                    self.at += 1;
                }
                let name = &self.text[start..self.at];
                let c = match name {
                    "newline" => '\n',
                    "space" => ' ',
                    "tab" => '\t',
                    "return" => '\r',
                    _ => name.chars().next().unwrap_or(' '),
                };
                Ok(Value::Char(c))
            }
            b':' => {
                self.at += 1;
                if self.peek() == Some(b':') {
                    self.at += 1;
                }
                Ok(Value::Keyword(self.token().to_string()))
            }
            _ => {
                let t = self.token().to_string();
                if t.is_empty() {
                    self.at += 1;
                    return Err(self.error("an unreadable character"));
                }
                Ok(match t.as_str() {
                    "nil" => Value::Nil,
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => number(&t).unwrap_or(Value::Symbol(t)),
                })
            }
        }
    }
}

fn number(t: &str) -> Option<Value> {
    let first = t.as_bytes()[0];
    let numeric = first.is_ascii_digit()
        || ((first == b'-' || first == b'+')
            && t.as_bytes().get(1).is_some_and(u8::is_ascii_digit));
    if !numeric {
        return None;
    }
    let t = t.trim_end_matches(['N', 'M']);
    if let Ok(i) = t.parse::<i64>() {
        return Some(Value::Int(i));
    }
    t.parse::<f64>().ok().map(Value::Float)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logseq_config() {
        let text = r#"
        ;; Logseq's configuration
        {:meta/version 1
         :preferred-format "Markdown"   ; or :org
         :preferred-workflow :now
         :hidden ["/archive" "drafts"]
         :journal/page-title-format "MMM do, yyyy"
         :journal/file-name-format "yyyy_MM_dd"
         :default-templates {:journals "Daily"}
         :feature/enable-timetracking? true
         :ref/linkable-properties #{:type :book}
         #_ :ignored #_ "too"
         :shortcuts {:editor/new-block "enter"}
         :macros {"poem" "Rose is $1, violet's $2."}
         :query/views {:pprint (fn [r] [:pre.code (pprint r)])}
         :commands []
         :graph/settings {:orphan-pages? true :builtin-pages? false}
         :rich-property-values? false
         :file/name-format :triple-lowbar
         :regex #"\d+"
         :ratio -1.5
         :char \a}"#;
        let v = parse(text).unwrap();
        assert_eq!(
            v.get("preferred-format").and_then(Value::as_str),
            Some("Markdown")
        );
        assert_eq!(
            v.get("preferred-workflow").and_then(Value::as_str),
            Some("now")
        );
        assert_eq!(v.get("hidden").unwrap().strings(), ["/archive", "drafts"]);
        assert_eq!(
            v.get("default-templates")
                .and_then(|t| t.get("journals"))
                .and_then(Value::as_str),
            Some("Daily")
        );
        assert_eq!(
            v.get("feature/enable-timetracking?")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            v.get("ref/linkable-properties").unwrap().strings(),
            ["type", "book"]
        );
        assert!(v.get("ignored").is_none());
        assert_eq!(
            v.get("file/name-format").and_then(Value::as_str),
            Some("triple-lowbar")
        );
        assert_eq!(v.get("regex").and_then(Value::as_str), Some("\\d+"));
        assert_eq!(v.get("ratio"), Some(&Value::Float(-1.5)));
        assert_eq!(v.get("meta/version"), Some(&Value::Int(1)));
        assert_eq!(v.get("char"), Some(&Value::Char('a')));
    }

    #[test]
    fn errors_name_the_line() {
        let e = parse("{:a 1\n :b [1 2}").unwrap_err();
        assert!(e.starts_with("line 2"), "{e}");
        assert!(parse("{:a}").is_err());
        assert!(parse("\"open").is_err());
        assert_eq!(parse("\"\\u00e7a\\n\"").unwrap(), Value::Str("ça\n".into()));
    }
}
