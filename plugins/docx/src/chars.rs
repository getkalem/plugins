//! Characters as Word writes them in symbol fonts, and numbers as list
//! labels write them.
//!
//! A bullet or a `w:sym` in Symbol or Wingdings is written as a character
//! of the Private Use Area, `U+F000` plus the font's own code (Symbol's
//! `•` is `U+F0B7`), which shows only in that font: [`symbol`] gives the
//! Unicode character that looks the same. [`format_number`] writes a
//! counter in one of the number formats of ST_NumberFormat (17.18.59).

/// The Symbol font's characters from code 0x20 to 0xFE (Adobe's
/// encoding), as Unicode.
const SYMBOL: &str = " !∀#∃%&∋()∗+,−./0123456789:;<=>?≅ΑΒΧΔΕΦΓΗΙϑΚΛΜΝΟΠΘΡΣΤΥςΩΞΨΖ[∴]⊥_‾αβχδεφγηιϕκλμνοπθρστυϖωξψζ{|}∼\u{7f}\
\u{80}\u{81}\u{82}\u{83}\u{84}\u{85}\u{86}\u{87}\u{88}\u{89}\u{8a}\u{8b}\u{8c}\u{8d}\u{8e}\u{8f}\
\u{90}\u{91}\u{92}\u{93}\u{94}\u{95}\u{96}\u{97}\u{98}\u{99}\u{9a}\u{9b}\u{9c}\u{9d}\u{9e}\u{9f}\
€ϒ′≤⁄∞ƒ♣♦♥♠↔←↑→↓°±″≥×∝∂•÷≠≡≈…⏐⎯↵ℵℑℜ℘⊗⊕∅∩∪⊃⊇⊄⊂⊆∈∉∠∇®©™∏√⋅¬∧∨⇔⇐⇑⇒⇓◊〈®©™∑⎛⎜⎝⎡⎢⎣⎧⎨⎩⎪\u{f0}〉∫⌠⎮⌡⎞⎟⎠⎤⎥⎦⎫⎬⎭";

/// The Wingdings characters lists and check boxes use, by code.
const WINGDINGS: &[(u8, char)] = &[
    (0x22, '✂'),
    (0x28, '☎'),
    (0x2A, '✉'),
    (0x3F, '✍'),
    (0x41, '✌'),
    (0x46, '☞'),
    (0x4A, '☺'),
    (0x4C, '☹'),
    (0x4E, '☠'),
    (0x51, '✈'),
    (0x54, '❄'),
    (0x6C, '●'),
    (0x6D, '❍'),
    (0x6E, '■'),
    (0x6F, '□'),
    (0x71, '❑'),
    (0x72, '❒'),
    (0x73, '⬧'),
    (0x74, '⧫'),
    (0x75, '◆'),
    (0x76, '❖'),
    (0x77, '⬥'),
    (0x7B, '❀'),
    (0x7C, '✿'),
    (0x7D, '❝'),
    (0x7E, '❞'),
    (0x9F, '•'),
    (0xA1, '○'),
    (0xA4, '◉'),
    (0xA5, '◎'),
    (0xA7, '▪'),
    (0xA8, '◻'),
    (0xAA, '✦'),
    (0xAB, '★'),
    (0xD8, '➢'),
    (0xE8, '➔'),
    (0xEF, '⇦'),
    (0xF0, '⇨'),
    (0xF1, '⇧'),
    (0xF2, '⇩'),
    (0xFB, '✗'),
    (0xFC, '✓'),
    (0xFD, '☒'),
    (0xFE, '☑'),
];

/// The character `c` of `font` as Unicode: a symbol font's code (written
/// as `U+F0xx` or as the code itself) mapped to the character it draws;
/// any other font's character as it is.
pub fn symbol(font: &str, c: char) -> char {
    let code = c as u32;
    let low = if (0xF020..=0xF0FF).contains(&code) {
        code - 0xF000
    } else {
        code
    };
    let f = font.trim().to_ascii_lowercase();
    if f == "symbol" && (0x20..=0xFE).contains(&low) {
        if let Some(m) = SYMBOL.chars().nth((low - 0x20) as usize)
            && !m.is_control()
        {
            return m;
        }
    } else if f == "wingdings"
        && let Some((_, m)) = WINGDINGS.iter().find(|(k, _)| u32::from(*k) == low)
    {
        return *m;
    }
    // A Private Use character of a font not known here: the font, when
    // the system has it, draws it.
    c
}

/// A `w:sym`'s `w:char` (hexadecimal, `F0FC`) in its font, as Unicode.
pub fn sym(font: &str, code: &str) -> char {
    let c = u32::from_str_radix(code.trim(), 16)
        .ok()
        .and_then(char::from_u32)
        .unwrap_or('\u{FFFD}');
    symbol(font, c)
}

fn roman(mut n: i64, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    const DIGITS: [(i64, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, s) in DIGITS {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    if upper { out.to_uppercase() } else { out }
}

/// Word's letters: a to z, then aa to zz, aaa…: the letter repeated.
fn letters(n: i64, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    let i = (n - 1) % 26;
    let times = ((n - 1) / 26 + 1) as usize;
    let c = (b'a' + i as u8) as char;
    let s: String = std::iter::repeat_n(c, times).collect();
    if upper { s.to_uppercase() } else { s }
}

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

fn cardinal(n: i64) -> String {
    if n < 0 {
        return format!("minus {}", cardinal(-n));
    }
    let n = n as u64;
    if n < 20 {
        return ONES[n as usize].into();
    }
    if n < 100 {
        let t = TENS[(n / 10) as usize];
        return if n.is_multiple_of(10) {
            t.into()
        } else {
            format!("{t}-{}", ONES[(n % 10) as usize])
        };
    }
    for (unit, name) in [
        (1_000_000_000u64, "billion"),
        (1_000_000, "million"),
        (1000, "thousand"),
        (100, "hundred"),
    ] {
        if n >= unit {
            let rest = n % unit;
            let head = format!("{} {name}", cardinal((n / unit) as i64));
            return if rest == 0 {
                head
            } else {
                format!("{head} {}", cardinal(rest as i64))
            };
        }
    }
    n.to_string()
}

fn ordinal_text(n: i64) -> String {
    let c = cardinal(n);
    // The last word turned into its ordinal.
    let (head, last) = match c.rfind([' ', '-']) {
        Some(i) => (&c[..=i], &c[i + 1..]),
        None => ("", c.as_str()),
    };
    let last = match last {
        "one" => "first".to_string(),
        "two" => "second".into(),
        "three" => "third".into(),
        "five" => "fifth".into(),
        "eight" => "eighth".into(),
        "nine" => "ninth".into(),
        "twelve" => "twelfth".into(),
        l if l.ends_with('y') => format!("{}ieth", &l[..l.len() - 1]),
        l => format!("{l}th"),
    };
    format!("{head}{last}")
}

fn ordinal(n: i64) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn capitalized(s: String) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// `n` written in the number format `fmt` (ST_NumberFormat); a format not
/// written here is decimal.
pub fn format_number(n: i64, fmt: &str) -> String {
    match fmt {
        "upperRoman" => roman(n, true),
        "lowerRoman" => roman(n, false),
        "upperLetter" => letters(n, true),
        "lowerLetter" => letters(n, false),
        "ordinal" => ordinal(n),
        "cardinalText" => capitalized(cardinal(n)),
        "ordinalText" => capitalized(ordinal_text(n)),
        "decimalZero" if (0..10).contains(&n) => format!("0{n}"),
        "decimalEnclosedCircle" | "decimalEnclosedCircleChinese" if (1..=20).contains(&n) => {
            char::from_u32(0x2460 + n as u32 - 1).map_or(n.to_string(), String::from)
        }
        "decimalEnclosedParen" if (1..=20).contains(&n) => {
            char::from_u32(0x2474 + n as u32 - 1).map_or(n.to_string(), String::from)
        }
        "decimalEnclosedFullstop" if (1..=20).contains(&n) => {
            char::from_u32(0x2488 + n as u32 - 1).map_or(n.to_string(), String::from)
        }
        "decimalFullWidth" | "decimalFullWidth2" => n
            .to_string()
            .chars()
            .map(|c| {
                c.to_digit(10)
                    .and_then(|d| char::from_u32(0xFF10 + d))
                    .unwrap_or(c)
            })
            .collect(),
        "chicago" => {
            const SIGNS: [&str; 4] = ["*", "†", "‡", "§"];
            if n <= 0 {
                n.to_string()
            } else {
                SIGNS[((n - 1) % 4) as usize].repeat(((n - 1) / 4 + 1) as usize)
            }
        }
        "hex" => format!("{n:X}"),
        "none" => String::new(),
        _ => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_fonts() {
        assert_eq!(symbol("Symbol", '\u{F0B7}'), '•');
        assert_eq!(symbol("Symbol", '\u{F061}'), 'α');
        assert_eq!(symbol("Wingdings", '\u{F0A7}'), '▪');
        assert_eq!(symbol("Wingdings", '\u{F0FC}'), '✓');
        assert_eq!(sym("Wingdings", "F0FC"), '✓');
        assert_eq!(symbol("Courier New", 'o'), 'o');
        assert_eq!(symbol("Wingdings 3", '\u{F07D}'), '\u{F07D}');
        assert_eq!(SYMBOL.chars().count(), 0xFF - 0x20);
    }

    #[test]
    fn number_formats() {
        assert_eq!(format_number(4, "decimal"), "4");
        assert_eq!(format_number(14, "upperRoman"), "XIV");
        assert_eq!(format_number(1994, "lowerRoman"), "mcmxciv");
        assert_eq!(format_number(1, "lowerLetter"), "a");
        assert_eq!(format_number(27, "lowerLetter"), "aa");
        assert_eq!(format_number(28, "upperLetter"), "BB");
        assert_eq!(format_number(22, "ordinal"), "22nd");
        assert_eq!(format_number(13, "ordinal"), "13th");
        assert_eq!(format_number(21, "cardinalText"), "Twenty-one");
        assert_eq!(format_number(121, "cardinalText"), "One hundred twenty-one");
        assert_eq!(format_number(20, "ordinalText"), "Twentieth");
        assert_eq!(format_number(23, "ordinalText"), "Twenty-third");
        assert_eq!(format_number(5, "decimalZero"), "05");
        assert_eq!(format_number(3, "decimalEnclosedCircle"), "③");
        assert_eq!(format_number(5, "chicago"), "**");
        assert_eq!(format_number(9, "none"), "");
        assert_eq!(format_number(9, "someNewFormat"), "9");
    }
}
