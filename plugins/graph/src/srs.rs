//! Logseq's flashcards (its `srs`, as Logseq 0.10.9 keeps them): a block
//! tagged `#card` is a card, its children the answer, its `{{cloze …}}`
//! hidden until shown. A review scores it 1 (forgotten), 3 (recalled
//! with effort) or 5 (remembered), and its next review is worked out as
//! Logseq works it out (SM-5), written into the block's `card-*`
//! properties. Logseq's matrix of optimal factors
//! (`logseq/srs-of-matrix.edn`) is read, never written: the plugin
//! writes nothing under `logseq/`, so only Logseq's own reviews teach it.

use std::collections::BTreeSet;

use crate::date::Date;
use crate::edn::Value;
use crate::index::Index;

/// A card: its block, its text and its answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// The block's file, relative.
    pub rel: String,
    /// The block's first line.
    pub line: u32,
    /// The block's text (its first line), the question.
    pub text: String,
    /// Its children's lines and texts, each text with two spaces a level
    /// under it.
    pub answer: Vec<(u32, String)>,
    /// When it is due, in milliseconds since 1970; none for a new card.
    pub due: Option<i64>,
    /// How many times it was remembered in a row (`card-repeats`).
    pub repeats: i64,
    /// Its ease factor (`card-ease-factor`).
    pub ease: f64,
}

/// The cards of the graph: its blocks tagged `#card` (or naming `[[card]]`),
/// those due first (new ones, then the longest due), then the others.
pub fn cards(index: &Index) -> Vec<Card> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for r in index.backlinks("card") {
        let Some(i) = r.block else { continue };
        let Some(d) = index.file(&r.path) else {
            continue;
        };
        let Some(b) = d.scanned.blocks.get(i) else {
            continue;
        };
        if !seen.insert((r.path.to_string(), b.line)) {
            continue;
        }
        let mut answer = Vec::new();
        for c in d.scanned.blocks.iter().skip(i + 1) {
            let mut depth = 0;
            let mut p = c.parent;
            let mut under = false;
            while let Some(x) = p {
                depth += 1;
                if x == i {
                    under = true;
                    break;
                }
                p = d.scanned.blocks[x].parent;
            }
            if !under {
                break;
            }
            answer.push((c.line, format!("{}{}", "  ".repeat(depth - 1), c.text)));
        }
        let prop = |k: &str| {
            b.props
                .iter()
                .find(|(pk, _)| pk == k)
                .map(|(_, v)| v.trim().to_string())
        };
        out.push(Card {
            rel: r.path.to_string(),
            line: b.line,
            text: b.text.clone(),
            answer,
            due: prop("card-next-schedule").as_deref().and_then(iso_millis),
            repeats: prop("card-repeats")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            ease: prop("card-ease-factor")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2.5),
        });
    }
    out.sort_by_key(|c| (c.due.is_some(), c.due, c.rel.clone(), c.line));
    out
}

/// Whether `card` is due at `now` (milliseconds since 1970), as Logseq
/// asks it: new, never remembered, or its moment past.
pub fn due(card: &Card, now: i64) -> bool {
    card.repeats < 1 || card.due.is_none_or(|d| d <= now)
}

/// A card's text with its clozes hidden (`{{cloze x}}` as `[…]`), or shown.
pub fn cloze(text: &str, shown: bool) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find("{{cloze ") {
        let Some(q) = rest[p..].find("}}") else { break };
        out.push_str(&rest[..p]);
        let inner = rest[p + 8..p + q].trim();
        if shown {
            out.push_str(inner);
        } else {
            out.push_str("[…]");
        }
        rest = &rest[p + q + 2..];
    }
    out.push_str(rest);
    out
}

/// Logseq's matrix of optimal factors: by repeats and ease factor.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Matrix(Vec<(i64, f64, f64)>);

impl Matrix {
    /// The matrix in the text of `logseq/srs-of-matrix.edn`.
    pub fn read(text: &str) -> Matrix {
        let mut out = Vec::new();
        let num = |v: &Value| match v {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        };
        if let Ok(Value::Map(rows)) = crate::edn::parse(text) {
            for (n, row) in &rows {
                let (Some(n), Value::Map(cells)) = (num(n), row) else {
                    continue;
                };
                for (ef, of) in cells {
                    if let (Some(ef), Some(of)) = (num(ef), num(of)) {
                        out.push((n as i64, ef, of));
                    }
                }
            }
        }
        Matrix(out)
    }

    fn get(&self, n: i64, ef: f64) -> Option<f64> {
        self.0
            .iter()
            .find(|(m, e, _)| *m == n && (e - ef).abs() < 1e-9)
            .map(|(_, _, of)| *of)
    }

    fn set(&mut self, n: i64, ef: f64, of: f64) {
        match self
            .0
            .iter_mut()
            .find(|(m, e, _)| *m == n && (*e - ef).abs() < 1e-9)
        {
            Some(cell) => cell.2 = of,
            None => self.0.push((n, ef, of)),
        }
    }
}

/// What a review sets: Logseq's settings of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// `:srs/learning-fraction`, 0.5.
    pub learning_fraction: f64,
    /// `:srs/initial-interval`, 4 days.
    pub initial_interval: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            learning_fraction: 0.5,
            initial_interval: 4.0,
        }
    }
}

/// Logseq's settings of reviews in the text of `logseq/config.edn`
/// (`:srs/learning-fraction`, `:srs/initial-interval`), its defaults
/// where it has none or they are out of range.
pub fn settings(config: &str) -> Settings {
    let c = crate::edn::parse(config).unwrap_or(Value::Map(Vec::new()));
    let num = |k: &str| match c.get(k) {
        Some(Value::Int(n)) => Some(*n as f64),
        Some(Value::Float(f)) => Some(*f),
        _ => None,
    };
    let d = Settings::default();
    Settings {
        learning_fraction: num("srs/learning-fraction")
            .filter(|f| *f > 0.0 && *f < 1.0)
            .unwrap_or(d.learning_fraction),
        initial_interval: num("srs/initial-interval")
            .filter(|f| *f > 0.0)
            .unwrap_or(d.initial_interval),
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn get_of(m: &Matrix, n: i64, ef: f64, s: &Settings) -> f64 {
    m.get(n, ef)
        .unwrap_or(if n <= 1 { s.initial_interval } else { ef })
}

fn interval(n: i64, ef: f64, m: &Matrix, s: &Settings) -> f64 {
    if n <= 1 {
        get_of(m, 1, ef, s)
    } else {
        get_of(m, n, ef, s) * interval(n - 1, ef, m, s)
    }
}

/// The next interval in days, repeats and ease factor of a card scored
/// `quality` (1, 3 or 5), as Logseq's `next-interval` works them out.
pub fn next(repeats: i64, ef: f64, quality: f64, m: &Matrix, s: &Settings) -> (f64, i64, f64) {
    let q5 = 5.0 - quality;
    let next_ef = (ef + (0.1 - q5 * (0.08 + 0.02 * q5))).max(1.3);
    let of = get_of(m, repeats, ef, s);
    let lf = s.learning_fraction;
    let mut m2 = m.clone();
    m2.set(
        repeats,
        ef,
        round2((1.0 - lf) * of + of * (0.72 + 0.07 * quality) * lf),
    );
    let next = interval(repeats, next_ef, &m2, s);
    if quality < 3.0 {
        (-1.0, 1, ef)
    } else {
        (round2(next), repeats + 1, round2(next_ef))
    }
}

/// The properties a review of `card` scored `score` at `now` (milliseconds
/// since 1970) writes, as Logseq writes them.
pub fn review(
    card: &Card,
    score: u8,
    now: i64,
    m: &Matrix,
    s: &Settings,
) -> Vec<(&'static str, String)> {
    let (interval, repeats, ef) = next(card.repeats, card.ease, f64::from(score), m, s);
    let next_at = now + (interval.max(0.0) * 86_400_000.0).round() as i64;
    vec![
        ("card-last-interval", number(interval)),
        ("card-repeats", repeats.to_string()),
        ("card-ease-factor", number(ef)),
        ("card-next-schedule", iso(next_at)),
        ("card-last-reviewed", iso(now)),
        ("card-last-score", score.to_string()),
    ]
}

/// A number as JavaScript prints it: `4`, `2.6`, `-1`.
fn number(x: f64) -> String {
    format!("{x}")
}

/// `2026-10-14T17:46:40.000Z`, as Logseq's dates of cards are written.
pub fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let d = Date::from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{}T{:02}:{:02}:{:02}.{:03}Z",
        d.iso(),
        t / 3600,
        t / 60 % 60,
        t % 60,
        ms.rem_euclid(1000)
    )
}

/// The milliseconds since 1970 of `2026-10-14T17:46:40.000Z` (the
/// fraction and the `Z` optional).
pub fn iso_millis(s: &str) -> Option<i64> {
    let d = Date::parse_iso(s.get(..10)?)?;
    let time = s.get(11..)?.trim_end_matches('Z');
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut p = hms.split(':');
    let h: i64 = p.next()?.parse().ok()?;
    let m: i64 = p.next()?.parse().ok()?;
    let sec: i64 = p.next().unwrap_or("0").parse().ok()?;
    let ms: i64 = format!("{frac:0<3}").get(..3)?.parse().ok()?;
    Some((d.to_days() * 86_400 + h * 3600 + m * 60 + sec) * 1000 + ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logseqs_intervals_worked_out() {
        let (m, s) = (Matrix::default(), Settings::default());
        // A new card: remembered, its first interval (4 days); remembered
        // with effort, the same days and an easier factor less; forgotten,
        // asked again.
        assert_eq!(next(0, 2.5, 5.0, &m, &s), (4.0, 1, 2.6));
        assert_eq!(next(0, 2.5, 3.0, &m, &s), (4.0, 1, 2.36));
        assert_eq!(next(0, 2.5, 1.0, &m, &s), (-1.0, 1, 2.5));
        // Remembered again and again: the factor times the last interval.
        assert_eq!(next(1, 2.6, 5.0, &m, &s), (4.0, 2, 2.7));
        assert_eq!(next(2, 2.7, 5.0, &m, &s), (11.2, 3, 2.8));
        // Logseq's matrix, learnt from its reviews, read.
        let m = Matrix::read("{2 {2.8 3.0, 2.5 2.4}}");
        assert_eq!(next(2, 2.7, 5.0, &m, &s), (12.0, 3, 2.8));
        // Its settings.
        let s = settings("{:srs/learning-fraction 0.6 :srs/initial-interval 2}");
        assert_eq!((s.learning_fraction, s.initial_interval), (0.6, 2.0));
        assert_eq!(next(0, 2.5, 5.0, &Matrix::default(), &s), (2.0, 1, 2.6));
        assert_eq!(
            settings("{:srs/learning-fraction 1.5}"),
            Settings::default()
        );
        assert_eq!(settings("not edn {"), Settings::default());
    }

    #[test]
    fn a_review_written_as_logseq_writes_it() {
        let now = iso_millis("2026-10-14T17:46:40.000Z").unwrap();
        assert_eq!(now, 1_792_000_000_000);
        let card = Card {
            rel: "pages/Capitals.md".into(),
            line: 0,
            text: "What is the capital of France? #card".into(),
            answer: vec![(1, "Paris".into())],
            due: None,
            repeats: 0,
            ease: 2.5,
        };
        let props = review(&card, 5, now, &Matrix::default(), &Settings::default());
        assert_eq!(
            props,
            [
                ("card-last-interval", "4".to_string()),
                ("card-repeats", "1".into()),
                ("card-ease-factor", "2.6".into()),
                ("card-next-schedule", "2026-10-18T17:46:40.000Z".into()),
                ("card-last-reviewed", "2026-10-14T17:46:40.000Z".into()),
                ("card-last-score", "5".into()),
            ]
        );
        // Forgotten: due at once.
        let props = review(&card, 1, now, &Matrix::default(), &Settings::default());
        assert_eq!(props[0], ("card-last-interval", "-1".to_string()));
        assert_eq!(props[3], ("card-next-schedule", iso(now)));
        // Due: new, never remembered, or its moment past.
        let at = |due, repeats| Card {
            due,
            repeats,
            ..card.clone()
        };
        assert!(due(&at(None, 0), now));
        assert!(due(&at(Some(now + 1), 0), now));
        assert!(!due(&at(Some(now + 1), 1), now));
        assert!(due(&at(Some(now - 1), 3), now));
        assert_eq!(iso_millis("2026-10-14T17:46:40Z"), Some(now));
        assert_eq!(iso_millis("nil"), None);
    }

    #[test]
    fn clozes_hidden_and_shown() {
        let text = "The capital of {{cloze Italy}} is {{cloze  Rome }}.";
        assert_eq!(cloze(text, false), "The capital of […] is […].");
        assert_eq!(cloze(text, true), "The capital of Italy is Rome.");
        assert_eq!(cloze("{{cloze open", false), "{{cloze open");
    }
}
