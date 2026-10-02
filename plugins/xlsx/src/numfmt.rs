//! Number formats (ECMA-376 part 1, 18.8.30 and 18.8.31): a cell's value
//! shown as Excel shows it.
//!
//! Covered: the built-in formats, sections and conditions, literals and
//! escapes, digit placeholders, thousands separators and scaling, percent,
//! scientific notation, simple fractions, text sections, dates and times
//! with elapsed hours, minutes and seconds, colors and locale tags skipped,
//! currency tags shown by their symbol. The decimal and thousands marks are
//! the file's (`.` and `,`); a locale-aware view is a later step.

/// The format code of a built-in format id (18.8.30), en-US.
pub fn builtin(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "\"$\"#,##0_);(\"$\"#,##0)",
        6 => "\"$\"#,##0_);[Red](\"$\"#,##0)",
        7 => "\"$\"#,##0.00_);(\"$\"#,##0.00)",
        8 => "\"$\"#,##0.00_);[Red](\"$\"#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        37 => "#,##0_);(#,##0)",
        38 => "#,##0_);[Red](#,##0)",
        39 => "#,##0.00_);(#,##0.00)",
        40 => "#,##0.00_);[Red](#,##0.00)",
        41 => "_(* #,##0_);_(* (#,##0);_(* \"-\"_);_(@_)",
        42 => "_(\"$\"* #,##0_);_(\"$\"* (#,##0);_(\"$\"* \"-\"_);_(@_)",
        43 => "_(* #,##0.00_);_(* (#,##0.00);_(* \"-\"??_);_(@_)",
        44 => "_(\"$\"* #,##0.00_);_(\"$\"* (#,##0.00);_(\"$\"* \"-\"??_);_(@_)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        // East Asian ids Excel maps to dates.
        27..=36 | 50..=58 => "m/d/yyyy",
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Lit(String),
    /// `0`, `#` or `?`.
    Digit(char),
    Point,
    Comma,
    Percent,
    Exp(bool),
    Slash,
    At,
    /// A date or time part: the letters as written, lower case.
    Date(String),
    AmPm(String),
    /// `[h]`, `[mm]`, `[ss]`.
    Elapsed(char, usize),
    General,
}

#[derive(Debug, Clone, PartialEq)]
enum Cond {
    Lt(f64),
    Le(f64),
    Gt(f64),
    Ge(f64),
    Eq(f64),
    Ne(f64),
}

impl Cond {
    fn holds(&self, v: f64) -> bool {
        match *self {
            Self::Lt(x) => v < x,
            Self::Le(x) => v <= x,
            Self::Gt(x) => v > x,
            Self::Ge(x) => v >= x,
            Self::Eq(x) => v == x,
            Self::Ne(x) => v != x,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Section {
    toks: Vec<Tok>,
    cond: Option<Cond>,
}

impl Section {
    fn is_date(&self) -> bool {
        self.toks
            .iter()
            .any(|t| matches!(t, Tok::Date(_) | Tok::AmPm(_) | Tok::Elapsed(..)))
    }
}

fn split_sections(code: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = code.as_bytes();
    let (mut i, mut start) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += 1;
                }
            }
            b'\\' | b'_' | b'*' => i += 1,
            b'[' => {
                while i < b.len() && b[i] != b']' {
                    i += 1;
                }
            }
            b';' => {
                out.push(&code[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&code[start.min(code.len())..]);
    out
}

fn parse_section(s: &str) -> Section {
    let mut sec = Section::default();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let lit = |toks: &mut Vec<Tok>, s: &str| {
        if let Some(Tok::Lit(prev)) = toks.last_mut() {
            prev.push_str(s);
        } else {
            toks.push(Tok::Lit(s.to_owned()));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().collect();
        let lower = rest.to_ascii_lowercase();
        match c {
            '"' => {
                let mut j = i + 1;
                let mut l = String::new();
                while j < chars.len() && chars[j] != '"' {
                    l.push(chars[j]);
                    j += 1;
                }
                lit(&mut sec.toks, &l);
                i = j + 1;
            }
            '\\' => {
                if let Some(&n) = chars.get(i + 1) {
                    lit(&mut sec.toks, &n.to_string());
                }
                i += 2;
            }
            '_' => {
                lit(&mut sec.toks, " ");
                i += 2;
            }
            '*' => i += 2,
            '[' => {
                let end = chars[i..]
                    .iter()
                    .position(|&c| c == ']')
                    .map_or(chars.len(), |p| i + p);
                let inner: String = chars[i + 1..end].iter().collect();
                let li = inner.to_ascii_lowercase();
                if let Some(cur) = inner.strip_prefix('$') {
                    let sym = cur.split('-').next().unwrap_or_default();
                    lit(&mut sec.toks, sym);
                } else if !li.is_empty() && li.chars().all(|c| c == 'h') {
                    sec.toks.push(Tok::Elapsed('h', li.len()));
                } else if !li.is_empty() && li.chars().all(|c| c == 'm') {
                    sec.toks.push(Tok::Elapsed('m', li.len()));
                } else if !li.is_empty() && li.chars().all(|c| c == 's') {
                    sec.toks.push(Tok::Elapsed('s', li.len()));
                } else if let Some(cond) = parse_cond(&inner) {
                    sec.cond = Some(cond);
                }
                // Colors and anything else in brackets are not shown.
                i = end + 1;
            }
            '0' | '#' | '?' => {
                sec.toks.push(Tok::Digit(c));
                i += 1;
            }
            '.' => {
                sec.toks.push(Tok::Point);
                i += 1;
            }
            ',' => {
                sec.toks.push(Tok::Comma);
                i += 1;
            }
            '%' => {
                sec.toks.push(Tok::Percent);
                i += 1;
            }
            '/' => {
                sec.toks.push(Tok::Slash);
                i += 1;
            }
            '@' => {
                sec.toks.push(Tok::At);
                i += 1;
            }
            'E' | 'e' if matches!(chars.get(i + 1), Some('+' | '-')) => {
                sec.toks.push(Tok::Exp(chars[i + 1] == '+'));
                i += 2;
            }
            _ if lower.starts_with("general") => {
                sec.toks.push(Tok::General);
                i += 7;
            }
            _ if lower.starts_with("am/pm") => {
                sec.toks.push(Tok::AmPm(rest[..5].to_owned()));
                i += 5;
            }
            _ if lower.starts_with("a/p") => {
                sec.toks.push(Tok::AmPm(rest[..3].to_owned()));
                i += 3;
            }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => {
                let lc = c.to_ascii_lowercase();
                let n = chars[i..]
                    .iter()
                    .take_while(|x| x.to_ascii_lowercase() == lc)
                    .count();
                sec.toks
                    .push(Tok::Date(std::iter::repeat_n(lc, n).collect()));
                i += n;
            }
            _ => {
                lit(&mut sec.toks, &c.to_string());
                i += 1;
            }
        }
    }
    // Fractional seconds: `ss.0` keeps its zeros as date digits.
    sec
}

fn parse_cond(s: &str) -> Option<Cond> {
    let s = s.trim();
    let (op, rest) = ["<=", ">=", "<>", "<", ">", "="]
        .iter()
        .find_map(|op| s.strip_prefix(op).map(|r| (*op, r)))?;
    let v: f64 = rest.trim().parse().ok()?;
    Some(match op {
        "<=" => Cond::Le(v),
        ">=" => Cond::Ge(v),
        "<>" => Cond::Ne(v),
        "<" => Cond::Lt(v),
        ">" => Cond::Gt(v),
        _ => Cond::Eq(v),
    })
}

/// Whether a format code shows dates or times.
pub fn is_date_format(code: &str) -> bool {
    split_sections(code)
        .first()
        .is_some_and(|s| parse_section(s).is_date())
}

/// A number shown through a format code.
pub fn format_number(v: f64, code: &str, date1904: bool) -> String {
    if v.is_nan() {
        return "#NUM!".into();
    }
    let secs: Vec<Section> = split_sections(code)
        .into_iter()
        .map(parse_section)
        .collect();
    let numeric: Vec<&Section> = secs
        .iter()
        .filter(|s| !s.toks.contains(&Tok::At) || s.toks.len() > 1)
        .collect();
    let has_conds = secs.iter().any(|s| s.cond.is_some());
    let (sec, abs) = if has_conds {
        let pick = secs
            .iter()
            .take(3)
            .find(|s| s.cond.as_ref().is_some_and(|c| c.holds(v)));
        match pick {
            Some(s) => (s, v < 0.0),
            None => match secs.iter().take(3).find(|s| s.cond.is_none()) {
                Some(s) => (
                    s,
                    v < 0.0 && secs.iter().filter(|s| s.cond.is_some()).count() >= 1,
                ),
                None => return "#".repeat(5),
            },
        }
    } else {
        match (numeric.len(), v) {
            (0, _) => return format_general(v),
            (1, _) => (numeric[0], false),
            (2, v) if v < 0.0 => (numeric[1], true),
            (2, _) => (numeric[0], false),
            (_, v) if v < 0.0 => (numeric[1], true),
            (_, 0.0) => (numeric[2], false),
            _ => (numeric[0], false),
        }
    };
    let v = if abs { v.abs() } else { v };
    if sec.is_date() {
        return format_date(v, sec, date1904);
    }
    format_with(v, sec)
}

/// A text value shown through the text section of a format code.
pub fn format_text(s: &str, code: &str) -> String {
    let secs = split_sections(code);
    let sec = if secs.len() >= 4 {
        parse_section(secs[3])
    } else {
        match secs
            .iter()
            .map(|s| parse_section(s))
            .find(|s| s.toks.contains(&Tok::At))
        {
            Some(s) => s,
            None => return s.to_owned(),
        }
    };
    let mut out = String::new();
    for t in &sec.toks {
        match t {
            Tok::At => out.push_str(s),
            Tok::Lit(l) => out.push_str(l),
            _ => {}
        }
    }
    out
}

/// A non-negative number with `places` decimals, as Excel rounds it: to
/// fifteen significant digits first, then half away from zero, in decimal.
pub(crate) fn round_decimal(v: f64, places: usize) -> String {
    let v = v.abs();
    if v == 0.0 || !v.is_finite() {
        return format!("{:.places$}", 0.0);
    }
    // Fifteen significant digits, as decimal digits and an exponent.
    let sci = format!("{v:.14e}");
    let (mant, exp) = sci.split_once('e').expect("exponent");
    let exp: i64 = exp.parse().expect("exponent digits");
    let mut digits: Vec<u8> = mant
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    // The value is 0.d1d2d3… × 10^(exp+1); keep exp+1+places digits.
    let point = exp + 1;
    let keep = point + places as i64;
    if keep < 0 {
        return format!("{:.places$}", 0.0);
    }
    let keep = keep as usize;
    if keep < digits.len() {
        let up = digits[keep] >= 5;
        digits.truncate(keep);
        if up {
            let mut i = keep;
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    return render_digits(&digits, point + 1, places);
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }
    render_digits(&digits, point, places)
}

fn render_digits(digits: &[u8], point: i64, places: usize) -> String {
    let digit = |i: i64| -> char {
        if i < 0 {
            '0'
        } else {
            digits.get(i as usize).map_or('0', |d| char::from(b'0' + d))
        }
    };
    let mut out = String::new();
    if point <= 0 {
        out.push('0');
    } else {
        for i in 0..point {
            out.push(digit(i));
        }
    }
    if places > 0 {
        out.push('.');
        for k in 0..places as i64 {
            out.push(digit(point + k));
        }
    }
    out
}

/// Excel's General format: up to eleven characters, scientific beyond.
pub fn format_general(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    if !(1e-9..1e11).contains(&a) {
        let s = format!("{v:.5E}");
        let (m, e) = s.split_once('E').unwrap_or((&s, "0"));
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{m}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let int_digits = if a >= 1.0 {
        a.log10().floor() as i32 + 1
    } else {
        1
    };
    let neg = usize::from(v < 0.0);
    let decimals = (10 - int_digits - neg as i32).max(0) as usize;
    let s = format!(
        "{}{}",
        if v < 0.0 { "-" } else { "" },
        round_decimal(v, decimals)
    );
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

fn format_with(v: f64, sec: &Section) -> String {
    let toks = &sec.toks;
    if toks.contains(&Tok::General) {
        let mut out = String::new();
        for t in toks {
            match t {
                Tok::General => out.push_str(&format_general(v)),
                Tok::Lit(l) => out.push_str(l),
                _ => {}
            }
        }
        return out;
    }
    let mut v = v;
    let percents = toks.iter().filter(|t| **t == Tok::Percent).count();
    v *= 100f64.powi(percents as i32);
    // Trailing commas after the last digit placeholder scale by a thousand.
    let last_digit = toks.iter().rposition(|t| matches!(t, Tok::Digit(_)));
    let mut scale_commas = 0;
    if let Some(ld) = last_digit {
        let mut j = ld + 1;
        while toks.get(j) == Some(&Tok::Comma) {
            scale_commas += 1;
            j += 1;
        }
    }
    v /= 1000f64.powi(scale_commas);
    if toks.contains(&Tok::Slash) && toks.iter().any(|t| matches!(t, Tok::Digit(_))) {
        return format_fraction(v, toks);
    }
    if let Some(ep) = toks.iter().position(|t| matches!(t, Tok::Exp(_))) {
        return format_scientific(v, toks, ep);
    }
    let point = toks.iter().position(|t| *t == Tok::Point);
    let int_toks: Vec<&Tok> = toks[..point.unwrap_or(toks.len())].iter().collect();
    let frac_toks: Vec<&Tok> = point.map_or(Vec::new(), |p| toks[p + 1..].iter().collect());
    let frac_places: Vec<char> = frac_toks
        .iter()
        .filter_map(|t| {
            if let Tok::Digit(d) = t {
                Some(*d)
            } else {
                None
            }
        })
        .collect();
    let thousands = last_digit.is_some_and(|ld| {
        toks[..ld].iter().enumerate().any(|(i, t)| {
            *t == Tok::Comma
                && i > 0
                && matches!(toks[i - 1], Tok::Digit(_))
                && matches!(toks.get(i + 1), Some(Tok::Digit(_)))
        })
    });
    let rounded = round_decimal(v.abs(), frac_places.len());
    let (int_str, frac_str) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let mut int_digits: Vec<char> = if int_str == "0" {
        Vec::new()
    } else {
        int_str.chars().collect()
    };
    let int_places: Vec<char> = int_toks
        .iter()
        .filter_map(|t| {
            if let Tok::Digit(d) = t {
                Some(*d)
            } else {
                None
            }
        })
        .collect();
    // Pad with the placeholders' fill on the left.
    let mut pad = Vec::new();
    if int_places.len() > int_digits.len() {
        for &p in &int_places[..int_places.len() - int_digits.len()] {
            match p {
                '0' => pad.push('0'),
                '?' => pad.push(' '),
                _ => {}
            }
        }
    }
    pad.append(&mut int_digits);
    let int_digits = pad;
    let int_text: String = if thousands {
        let digits: Vec<char> = int_digits.to_vec();
        let n = digits.iter().filter(|c| c.is_ascii_digit()).count();
        let mut s = String::new();
        let mut seen = 0;
        for c in digits {
            s.push(c);
            if c.is_ascii_digit() {
                seen += 1;
                if seen < n && (n - seen) % 3 == 0 {
                    s.push(',');
                }
            }
        }
        s
    } else {
        int_digits.iter().collect()
    };
    // Fraction digits: `#` drops trailing zeros, `?` turns them into spaces.
    let mut frac: Vec<char> = frac_str.chars().collect();
    for k in (0..frac.len()).rev() {
        if frac[k] != '0' {
            break;
        }
        match frac_places[k] {
            '#' => {
                frac.pop();
            }
            '?' => frac[k] = ' ',
            _ => break,
        }
    }
    let negative = v < 0.0 && rounded.chars().any(|c| c.is_ascii_digit() && c != '0');
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    // Literals keep their places around the digits.
    let mut int_written = false;
    for t in &int_toks {
        match t {
            Tok::Digit(_) | Tok::Comma => {
                if !int_written {
                    out.push_str(&int_text);
                    int_written = true;
                }
            }
            Tok::Lit(l) => out.push_str(l),
            Tok::Percent => out.push('%'),
            _ => {}
        }
    }
    if !int_written {
        out.push_str(&int_text);
    }
    if point.is_some() {
        out.push('.');
        let mut frac_written = false;
        for t in &frac_toks {
            match t {
                Tok::Digit(_) => {
                    if !frac_written {
                        out.extend(frac.iter());
                        frac_written = true;
                    }
                }
                Tok::Lit(l) => out.push_str(l),
                Tok::Percent => out.push('%'),
                _ => {}
            }
        }
    }
    out
}

fn format_scientific(v: f64, toks: &[Tok], ep: usize) -> String {
    let mant = &toks[..ep];
    let exp_digits = toks[ep + 1..]
        .iter()
        .filter(|t| matches!(t, Tok::Digit(_)))
        .count()
        .max(1);
    let point = mant.iter().position(|t| *t == Tok::Point);
    let int_n = mant[..point.unwrap_or(mant.len())]
        .iter()
        .filter(|t| matches!(t, Tok::Digit(_)))
        .count()
        .max(1);
    let frac_n = point.map_or(0, |p| {
        mant[p + 1..]
            .iter()
            .filter(|t| matches!(t, Tok::Digit(_)))
            .count()
    });
    let (mut m, mut e) = (v.abs(), 0i32);
    if m != 0.0 {
        e = m.log10().floor() as i32;
        // Engineering-style groups when the integer part has several places.
        if int_n > 1 {
            e = e.div_euclid(int_n as i32) * int_n as i32;
        }
        m /= 10f64.powi(e);
        let r = round_decimal(m, frac_n);
        if r.parse::<f64>().unwrap_or(0.0) >= 10f64.powi(int_n as i32) {
            m /= 10f64.powi(int_n as i32);
            e += int_n as i32;
        }
    }
    let plus = matches!(toks[ep], Tok::Exp(true));
    let sign = if e < 0 {
        "-"
    } else if plus {
        "+"
    } else {
        ""
    };
    format!(
        "{}{}E{sign}{:0exp_digits$}",
        if v < 0.0 { "-" } else { "" },
        round_decimal(m, frac_n),
        e.abs()
    )
}

fn format_fraction(v: f64, toks: &[Tok]) -> String {
    let slash = toks.iter().position(|t| *t == Tok::Slash).unwrap_or(0);
    let den_toks = &toks[slash + 1..];
    let fixed: Option<u32> = {
        let s: String = den_toks
            .iter()
            .filter_map(|t| {
                if let Tok::Lit(l) = t {
                    Some(l.trim().to_owned())
                } else {
                    None
                }
            })
            .collect();
        s.parse().ok()
    };
    let den_places = den_toks
        .iter()
        .filter(|t| matches!(t, Tok::Digit(_)))
        .count()
        .max(1);
    let has_int = toks[..slash]
        .iter()
        .position(|t| matches!(t, Tok::Lit(l) if l.contains(' ')))
        .is_some();
    let a = v.abs();
    let int = if has_int { a.trunc() } else { 0.0 };
    let frac = a - int;
    let max_den = 10u64.pow(den_places as u32) - 1;
    let (num, den) = match fixed {
        Some(d) => (((frac * f64::from(d)).round()) as u64, u64::from(d)),
        None => {
            let mut best = (0u64, 1u64, f64::MAX);
            for d in 1..=max_den {
                let n = (frac * d as f64).round();
                let err = (frac - n / d as f64).abs();
                if err < best.2 - 1e-12 {
                    best = (n as u64, d, err);
                }
            }
            (best.0, best.1)
        }
    };
    let sign = if v < 0.0 { "-" } else { "" };
    let width = den_places;
    if num == 0 {
        return if has_int {
            format!("{sign}{int}{}", " ".repeat(2 * width + 2))
        } else {
            format!("{sign}0/{den}")
        };
    }
    if has_int && int > 0.0 {
        format!("{sign}{int} {num:>width$}/{den:<width$}")
    } else if has_int {
        format!("{sign}{num:>w2$}/{den:<width$}", w2 = width + 2)
    } else {
        format!("{sign}{}/{den:<width$}", num + (int as u64) * den)
    }
}

/// Days from 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Days from 1970-01-01 of a civil date.
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A serial date as (year, month, day, weekday 0 = Sunday); 1900-02-29
/// exists in the 1900 system, as in Excel and Lotus before it.
pub(crate) fn serial_to_date(serial: i64, date1904: bool) -> (i64, u32, u32, u32) {
    if date1904 {
        let days = serial + days_from_civil(1904, 1, 1);
        let (y, m, d) = civil_from_days(days);
        return (y, m, d, (days + 4).rem_euclid(7) as u32);
    }
    if serial == 60 {
        return (1900, 2, 29, 3);
    }
    if serial == 0 {
        return (1900, 1, 0, 6);
    }
    let days = if serial < 60 { serial - 1 } else { serial - 2 } + days_from_civil(1900, 1, 1);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, (days + 4).rem_euclid(7) as u32)
}

/// The serial of a date in the workbook's system.
pub(crate) fn date_to_serial(y: i64, m: u32, d: u32, date1904: bool) -> i64 {
    let days = days_from_civil(y, m, d);
    if date1904 {
        return days - days_from_civil(1904, 1, 1);
    }
    let s = days - days_from_civil(1900, 1, 1) + 1;
    if s >= 60 { s + 1 } else { s }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

fn format_date(v: f64, sec: &Section, date1904: bool) -> String {
    if v < 0.0 {
        return "#".repeat(10);
    }
    let toks = &sec.toks;
    // Sub-second digits after `ss.` decide the rounding.
    let sub_digits = toks
        .windows(2)
        .find_map(|w| match (&w[0], &w[1]) {
            (Tok::Date(s), Tok::Point) if s.starts_with('s') => Some(()),
            _ => None,
        })
        .map_or(0, |()| {
            let p = toks.iter().position(|t| *t == Tok::Point).unwrap_or(0);
            toks[p + 1..]
                .iter()
                .take_while(|t| **t == Tok::Digit('0'))
                .count()
        });
    let unit = 86_400.0 * 10f64.powi(sub_digits as i32);
    let total = (v * unit).round();
    let days = (total / unit).floor();
    let rem = total - days * unit;
    let sub_unit = 10f64.powi(sub_digits as i32);
    let secs_of_day = (rem / sub_unit).floor() as i64;
    let sub = (rem - secs_of_day as f64 * sub_unit) as i64;
    let (y, mo, d, wd) = serial_to_date(days as i64, date1904);
    let (h, mi, s) = (secs_of_day / 3600, secs_of_day / 60 % 60, secs_of_day % 60);
    let ampm = toks.iter().any(|t| matches!(t, Tok::AmPm(_)));
    let total_secs = (v * 86_400.0).round() as i64;
    // `m` means minutes right after an hour or right before a second.
    let date_idx: Vec<usize> = toks
        .iter()
        .enumerate()
        .filter(|(_, t)| matches!(t, Tok::Date(_) | Tok::Elapsed(..)))
        .map(|(i, _)| i)
        .collect();
    let is_minute = |i: usize| -> bool {
        let pos = date_idx.iter().position(|&x| x == i).unwrap_or(0);
        let prev = pos.checked_sub(1).and_then(|p| toks.get(date_idx[p]));
        let next = date_idx.get(pos + 1).and_then(|&n| toks.get(n));
        matches!(prev, Some(Tok::Date(p)) if p.starts_with('h'))
            || matches!(prev, Some(Tok::Elapsed('h', _)))
            || matches!(next, Some(Tok::Date(n)) if n.starts_with('s'))
            || matches!(next, Some(Tok::Elapsed('s', _)))
    };
    let mut out = String::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Lit(l) => out.push_str(l),
            Tok::Date(p) => {
                let n = p.len();
                match p.as_bytes()[0] {
                    b'y' => {
                        if n <= 2 {
                            out.push_str(&format!("{:02}", y.rem_euclid(100)));
                        } else {
                            out.push_str(&format!("{y:04}"));
                        }
                    }
                    b'm' if n <= 2 && is_minute(i) => out.push_str(&format!("{mi:0n$}")),
                    b'm' => match n {
                        1 | 2 => out.push_str(&format!("{mo:0n$}")),
                        3 => out.push_str(&MONTHS[mo as usize - 1][..3]),
                        5 => out.push_str(&MONTHS[mo as usize - 1][..1]),
                        _ => out.push_str(MONTHS[mo as usize - 1]),
                    },
                    b'd' => match n {
                        1 | 2 => out.push_str(&format!("{d:0n$}")),
                        3 => out.push_str(&DAYS[wd as usize][..3]),
                        _ => out.push_str(DAYS[wd as usize]),
                    },
                    b'h' => {
                        let hh = if ampm {
                            if h % 12 == 0 { 12 } else { h % 12 }
                        } else {
                            h
                        };
                        out.push_str(&format!("{hh:0w$}", w = n.min(2)));
                    }
                    b's' => out.push_str(&format!("{s:0w$}", w = n.min(2))),
                    _ => {}
                }
            }
            Tok::Elapsed(k, n) => {
                let val = match k {
                    'h' => total_secs / 3600,
                    'm' => total_secs / 60,
                    _ => total_secs,
                };
                out.push_str(&format!("{val:0n$}"));
            }
            Tok::AmPm(f) => {
                let pm = h >= 12;
                let t = if f.len() == 3 {
                    if pm { "P" } else { "A" }
                } else if pm {
                    "PM"
                } else {
                    "AM"
                };
                if f.chars().next().is_some_and(char::is_lowercase) {
                    out.push_str(&t.to_ascii_lowercase());
                } else {
                    out.push_str(t);
                }
            }
            Tok::Point if sub_digits > 0 => {
                out.push('.');
                out.push_str(&format!("{sub:0sub_digits$}"));
                i += sub_digits;
            }
            Tok::Point => out.push('.'),
            Tok::Comma => out.push(','),
            Tok::Slash => out.push('/'),
            Tok::Digit(c) => out.push(*c),
            Tok::Percent => out.push('%'),
            _ => {}
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f64, code: &str) -> String {
        format_number(v, code, false)
    }

    #[test]
    fn general() {
        assert_eq!(format_general(1.0), "1");
        assert_eq!(format_general(0.1 + 0.2), "0.3");
        assert_eq!(format_general(1234567.891), "1234567.891");
        assert_eq!(format_general(123456789012.0), "1.23457E+11");
        assert_eq!(format_general(-2.5), "-2.5");
        assert_eq!(format_general(1.0 / 3.0), "0.333333333");
    }

    #[test]
    fn numbers() {
        assert_eq!(f(1234.567, "0.00"), "1234.57");
        assert_eq!(f(1234.567, "#,##0.00"), "1,234.57");
        assert_eq!(f(-1234.5, "#,##0_);(#,##0)"), "(1,235)");
        assert_eq!(f(0.256, "0.0%"), "25.6%");
        assert_eq!(f(12345.0, "0.00E+00"), "1.23E+04");
        assert_eq!(f(5.0, "000"), "005");
        assert_eq!(f(1.5, "0.##"), "1.5");
        assert_eq!(f(1_500_000.0, "#,##0,\"K\""), "1,500K");
        assert_eq!(f(0.0, "0;-0;\"zero\""), "zero");
        assert_eq!(f(1.25, "# ?/?"), "1 1/4");
        assert_eq!(f(12.5, "[$€-407]#,##0.00"), "€12.50");
        assert_eq!(f(-5.0, "[Red]0;[Blue]-0"), "-5");
    }

    #[test]
    fn rounding_is_decimal_half_up() {
        assert_eq!(round_decimal(1234.5, 0), "1235");
        assert_eq!(round_decimal(1.005, 2), "1.01");
        assert_eq!(round_decimal(2.675, 2), "2.68");
        assert_eq!(round_decimal(9.995, 2), "10.00");
        assert_eq!(round_decimal(0.0004, 3), "0.000");
        assert_eq!(round_decimal(0.0005, 3), "0.001");
        assert_eq!(round_decimal(123.0, 0), "123");
    }

    #[test]
    fn dates() {
        assert_eq!(f(45_000.0, "yyyy-mm-dd"), "2023-03-15");
        assert_eq!(f(45_000.0, "m/d/yyyy"), "3/15/2023");
        assert_eq!(f(45_000.0, "d-mmm-yy"), "15-Mar-23");
        assert_eq!(f(45_000.0, "dddd, mmmm d"), "Wednesday, March 15");
        assert_eq!(f(0.75, "h:mm AM/PM"), "6:00 PM");
        assert_eq!(f(1.5, "[h]:mm:ss"), "36:00:00");
        assert_eq!(f(60.0, "yyyy-mm-dd"), "1900-02-29");
        assert_eq!(f(61.0, "yyyy-mm-dd"), "1900-03-01");
        assert_eq!(format_number(0.0, "yyyy-mm-dd", true), "1904-01-01");
        assert!(is_date_format("dd/mm/yyyy"));
        assert!(!is_date_format("#,##0.00"));
        assert!(!is_date_format("\"day\" 0"));
        assert_eq!(date_to_serial(2023, 3, 15, false), 45_000);
    }

    #[test]
    fn text() {
        assert_eq!(format_text("abc", "0;0;0;\"<\"@\">\""), "<abc>");
        assert_eq!(format_text("abc", "General"), "abc");
    }
}
