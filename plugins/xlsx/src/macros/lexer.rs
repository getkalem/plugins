//! VBA source split into logical lines of tokens ([MS-VBAL] 3).
//!
//! Physical lines joined at ` _` continuations; comments (`'` and `Rem`)
//! dropped; a logical line keeps its first physical line's number for
//! messages. Statements separated by `:` stay on one line here; the parser
//! splits them, since `:` also ends a label.

use std::fmt;

/// A token.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// A name, as written (comparisons ignore case).
    Ident(String),
    /// A number literal.
    Number(f64),
    /// An integer literal (decimal, `&H`, `&O`).
    Integer(i64),
    /// A string literal, quotes resolved.
    Str(String),
    /// A date literal `#2026-10-03#`, as the text between the hashes.
    Date(String),
    /// An operator or punctuation: `+ - * / \ ^ & = < > <= >= <> ( ) , . : ;` and `:=`.
    Punct(&'static str),
}

impl Tok {
    /// Whether the token is the keyword `kw` (case-insensitive).
    pub fn is_kw(&self, kw: &str) -> bool {
        matches!(self, Tok::Ident(s) if s.eq_ignore_ascii_case(kw))
    }

    /// Whether the token is the punctuation `p`.
    pub fn is_p(&self, p: &str) -> bool {
        matches!(self, Tok::Punct(q) if *q == p)
    }
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Ident(s) => f.write_str(s),
            Tok::Number(n) => write!(f, "{n}"),
            Tok::Integer(n) => write!(f, "{n}"),
            Tok::Str(s) => write!(f, "\"{s}\""),
            Tok::Date(s) => write!(f, "#{s}#"),
            Tok::Punct(p) => f.write_str(p),
        }
    }
}

/// A logical line.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The first physical line, one-based.
    pub number: usize,
    /// The tokens.
    pub toks: Vec<Tok>,
}

/// A lexing error: the line and what went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    /// One-based line.
    pub line: usize,
    /// The message.
    pub message: String,
}

const PUNCT: [&str; 21] = [
    ":=", "<=", ">=", "<>", "+", "-", "*", "/", "\\", "^", "&", "=", "<", ">", "(", ")", ",", ".",
    ":", ";", "!",
];

/// Splits a module's source into logical lines.
pub fn lex(source: &str) -> Result<Vec<Line>, LexError> {
    let mut out = Vec::new();
    let mut pending: Option<Line> = None;
    for (i, raw) in source.lines().enumerate() {
        let number = i + 1;
        let (toks, continued) = lex_line(raw, number)?;
        let line = match pending.take() {
            Some(mut l) => {
                l.toks.extend(toks);
                l
            }
            None => Line { number, toks },
        };
        if continued {
            pending = Some(line);
        } else if !line.toks.is_empty() {
            out.push(line);
        }
    }
    if let Some(l) = pending.filter(|l| !l.toks.is_empty()) {
        out.push(l);
    }
    Ok(out)
}

fn lex_line(s: &str, line: usize) -> Result<(Vec<Tok>, bool), LexError> {
    let err = |m: &str| LexError {
        line,
        message: m.into(),
    };
    let chars: Vec<char> = s.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == ' ' || c == '\t' {
            i += 1;
            continue;
        }
        // A continuation: ` _` at the end of the line.
        if c == '_'
            && chars[i + 1..].iter().all(|c| c.is_whitespace())
            && (i == 0 || chars[i - 1].is_whitespace())
        {
            return Ok((toks, true));
        }
        if c == '\'' {
            break;
        }
        if c == '"' {
            let mut v = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err(err("a string is not closed")),
                    Some('"') if chars.get(i + 1) == Some(&'"') => {
                        v.push('"');
                        i += 2;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        v.push(ch);
                        i += 1;
                    }
                }
            }
            toks.push(Tok::Str(v));
            continue;
        }
        if c == '#' {
            // A date literal, when a closing `#` follows on the line.
            if let Some(len) = chars[i + 1..].iter().position(|&c| c == '#') {
                let inner: String = chars[i + 1..i + 1 + len].iter().collect();
                if inner.chars().any(|c| c.is_ascii_digit()) {
                    toks.push(Tok::Date(inner));
                    i += len + 2;
                    continue;
                }
            }
            return Err(err(
                "`#` is not supported here (file I/O is not available to macros)",
            ));
        }
        if c == '&' && matches!(chars.get(i + 1), Some('H' | 'h' | 'O' | 'o')) {
            let radix = if matches!(chars[i + 1], 'H' | 'h') {
                16
            } else {
                8
            };
            let mut j = i + 2;
            while j < chars.len() && chars[j].is_digit(radix) {
                j += 1;
            }
            let digits: String = chars[i + 2..j].iter().collect();
            let v = i64::from_str_radix(&digits, radix)
                .map_err(|_| err("a malformed hexadecimal or octal number"))?;
            // `&HFFFF` is an Integer, -1; `&HFFFF&` a Long.
            let (v, j) = match chars.get(j) {
                Some('&') => (v, j + 1),
                _ if radix == 16 && digits.len() <= 4 && v > 0x7FFF => (v - 0x10000, j),
                _ => (v, j),
            };
            toks.push(Tok::Integer(v));
            i = j;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            if j < chars.len()
                && matches!(chars[j], 'e' | 'E')
                && chars
                    .get(j + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == '-' || *c == '+')
            {
                j += 2;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
            }
            let text: String = chars[i..j].iter().collect();
            // Type characters: `%` `&` `!` `#` `@`.
            let suffix = chars.get(j).copied();
            let float_suffix = matches!(suffix, Some('!' | '#' | '@'));
            if matches!(suffix, Some('%' | '&' | '!' | '#' | '@')) {
                j += 1;
            }
            let is_int = !text.contains(['.', 'e', 'E']) && !float_suffix;
            if is_int && let Ok(v) = text.parse::<i64>() {
                toks.push(Tok::Integer(v));
            } else {
                toks.push(Tok::Number(
                    text.parse().map_err(|_| err("a malformed number"))?,
                ));
            }
            i = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '[' {
            if c == '[' {
                // A bracketed name: `[A1]` evaluates a reference, `[Name With Space]`.
                let len = chars[i..]
                    .iter()
                    .position(|&c| c == ']')
                    .ok_or_else(|| err("`[` is not closed"))?;
                let inner: String = chars[i + 1..i + len].iter().collect();
                toks.push(Tok::Ident(format!("[{inner}]")));
                i += len + 1;
                continue;
            }
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            // A type character after a name (`Left$`, `count%`) is dropped.
            if matches!(chars.get(j), Some('$' | '%' | '&' | '#' | '@'))
                && !chars.get(j + 1).is_some_and(|c| c.is_alphanumeric())
            {
                j += 1;
            }
            if word.eq_ignore_ascii_case("rem") && toks.is_empty() {
                break;
            }
            toks.push(Tok::Ident(word));
            i = j;
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
        match PUNCT.iter().find(|p| rest.starts_with(**p)) {
            Some(p) => {
                toks.push(Tok::Punct(p));
                i += p.chars().count();
            }
            None => return Err(err(&format!("unexpected character `{c}`"))),
        }
    }
    Ok((toks, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_tokens_and_continuations() {
        let src = "Sub A() ' comment\r\n  x = \"a\"\"b\" & _\r\n    &HFF + 1.5e2 ' tail\r\nRem whole line\r\n  If x <> 3 Then y = #2026-10-03#\r\nEnd Sub";
        let lines = lex(src).unwrap();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1].number, 2);
        assert_eq!(
            lines[1].toks,
            vec![
                Tok::Ident("x".into()),
                Tok::Punct("="),
                Tok::Str("a\"b".into()),
                Tok::Punct("&"),
                Tok::Integer(255),
                Tok::Punct("+"),
                Tok::Number(150.0),
            ]
        );
        assert!(lines[2].toks.contains(&Tok::Punct("<>")));
        assert!(lines[2].toks.contains(&Tok::Date("2026-10-03".into())));
        assert_eq!(
            lex("x = Left$(s, 2)").unwrap()[0].toks[2],
            Tok::Ident("Left".into())
        );
        assert_eq!(lex("x = &HFFFF").unwrap()[0].toks[2], Tok::Integer(-1));
    }
}
