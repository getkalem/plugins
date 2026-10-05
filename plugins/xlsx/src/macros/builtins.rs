//! VBA's library functions that need no object ([MS-VBAL] 6.1): strings,
//! math, dates, conversions, arrays.

use super::value::*;
use crate::numfmt::{self, date_to_serial, serial_to_date};

/// A date serial as VBA prints it: `10/3/2026`, `2:30:00 PM`, or both.
pub fn date_text(d: f64) -> String {
    let (y, m, day, _) = serial_to_date(d.floor() as i64, false);
    let secs = ((d - d.floor()) * 86_400.0).round() as i64;
    let date = format!("{m}/{day}/{y}");
    if secs == 0 {
        return date;
    }
    let (h, mi, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    let time = format!("{h12}:{mi:02}:{s:02} {}", if h < 12 { "AM" } else { "PM" });
    if d.floor() == 0.0 {
        time
    } else {
        format!("{date} {time}")
    }
}

/// Text read as a date: ISO `2026-10-03`, `10/3/2026` (month first, as
/// VBA in the en-US locale), times `14:30`, `2:30 PM`, and both.
pub fn parse_date(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || !s.chars().next()?.is_ascii_digit() {
        return None;
    }
    let ok = s.contains(['-', '/', ':']) || s.contains(' ');
    if !ok {
        return None;
    }
    super::parser::parse_date_literal(s)
}

fn arg(args: &[V], i: usize) -> &V {
    args.get(i).unwrap_or(&V::Missing)
}

fn present(args: &[V], i: usize) -> bool {
    !matches!(args.get(i), None | Some(V::Missing))
}

fn s(args: &[V], i: usize) -> R<String> {
    to_str(arg(args, i))
}

fn n(args: &[V], i: usize) -> R<f64> {
    to_num(arg(args, i))
}

fn int(args: &[V], i: usize) -> R<i64> {
    to_int(arg(args, i))
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn need(args: &[V], k: usize, name: &str) -> R<()> {
    if args.len() < k || args[..k].iter().any(|a| matches!(a, V::Missing)) {
        return Err(RtError::new(
            450,
            format!("Wrong number of arguments for {name}"),
        ));
    }
    Ok(())
}

/// VBA's `Round`: half to even, at `digits` decimals.
pub fn vba_round(x: f64, digits: i64) -> f64 {
    let f = 10f64.powi(digits as i32);
    // Through text at fifteen digits, so that 2.675 * 100 rounds as typed.
    let scaled: f64 = format!("{:.15e}", x * f).parse().unwrap_or(x * f);
    round_even(scaled) / f
}

fn add_months(serial: f64, months: i64) -> f64 {
    let (y, m, d, _) = serial_to_date(serial.floor() as i64, false);
    let total = y * 12 + i64::from(m) - 1 + months;
    let (ny, nm) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    let last = days_in_month(ny, nm);
    date_to_serial(ny, nm, d.min(last), false) as f64 + (serial - serial.floor())
}

fn days_in_month(y: i64, m: u32) -> u32 {
    let next = if m == 12 {
        date_to_serial(y + 1, 1, 1, false)
    } else {
        date_to_serial(y, m + 1, 1, false)
    };
    (next - date_to_serial(y, m, 1, false)) as u32
}

/// `DateSerial(y, m, d)` with months and days past their ranges carried.
pub fn date_serial(y: i64, m: i64, d: i64) -> f64 {
    let total = y * 12 + m - 1;
    let (ny, nm) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    (date_to_serial(ny, nm, 1, false) + d - 1) as f64
}

fn interval_seconds(i: &str) -> Option<f64> {
    Some(match i {
        "d" | "y" | "w" => 86_400.0,
        "ww" => 7.0 * 86_400.0,
        "h" => 3600.0,
        "n" => 60.0,
        "s" => 1.0,
        _ => return None,
    })
}

/// `Like`: `*`, `?`, `#`, `[a-z]`, `[!a-z]`.
pub fn like(text: &str, pattern: &str) -> bool {
    fn go(t: &[char], p: &[char]) -> bool {
        let Some(&c) = p.first() else {
            return t.is_empty();
        };
        match c {
            '*' => (0..=t.len()).any(|k| go(&t[k..], &p[1..])),
            '?' => !t.is_empty() && go(&t[1..], &p[1..]),
            '#' => t.first().is_some_and(char::is_ascii_digit) && go(&t[1..], &p[1..]),
            '[' => {
                let Some(close) = p.iter().position(|&x| x == ']') else {
                    return false;
                };
                let set = &p[1..close];
                let (neg, set) = match set.first() {
                    Some('!') => (true, &set[1..]),
                    _ => (false, set),
                };
                let Some(&ch) = t.first() else { return false };
                let mut hit = false;
                let mut k = 0;
                while k < set.len() {
                    if k + 2 < set.len() && set[k + 1] == '-' {
                        hit |= (set[k]..=set[k + 2]).contains(&ch);
                        k += 3;
                    } else {
                        hit |= set[k] == ch;
                        k += 1;
                    }
                }
                hit != neg && go(&t[1..], &p[close + 1..])
            }
            _ => t.first() == Some(&c) && go(&t[1..], &p[1..]),
        }
    }
    go(&chars(text), &chars(pattern))
}

/// `Format(value, format)`.
pub fn format(v: &V, f: &str) -> R<String> {
    let named = match f.to_ascii_lowercase().as_str() {
        "" => return to_str(v),
        "general number" => "General",
        "currency" => "$#,##0.00",
        "fixed" => "0.00",
        "standard" => "#,##0.00",
        "percent" => "0.00%",
        "scientific" => "0.00E+00",
        "short date" => "m/d/yyyy",
        "medium date" => "dd-mmm-yy",
        "long date" => "dddd, mmmm d, yyyy",
        "short time" => "hh:mm",
        "medium time" => "h:mm AM/PM",
        "long time" => "h:mm:ss AM/PM",
        "yes/no" => return Ok(if to_bool(v)? { "Yes" } else { "No" }.into()),
        "true/false" => return Ok(if to_bool(v)? { "True" } else { "False" }.into()),
        "on/off" => return Ok(if to_bool(v)? { "On" } else { "Off" }.into()),
        _ => f,
    };
    match v {
        V::Str(t) if !v.is_numeric() && parse_date(t).is_none() => {
            Ok(numfmt::format_text(t, named))
        }
        _ => {
            // VBA's `nn` is minutes, Excel's is not a code: map it.
            let code = named.replace("nn", "mm");
            Ok(numfmt::format_number(to_num(v)?, &code, false))
        }
    }
}

fn now_serial() -> f64 {
    let secs = crate::time::SystemTime::now()
        .duration_since(crate::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    // UTC: a sandboxed plugin has no time zone; the host may pass one later.
    25_569.0 + secs / 86_400.0
}

/// Calls a library function by lower-case name; `None` when there is none.
pub fn call(name: &str, args: &[V]) -> Option<R<V>> {
    Some(call_inner(name, args))
}

#[allow(clippy::too_many_lines)]
fn call_inner(name: &str, a: &[V]) -> R<V> {
    let str_v = |s: String| Ok(V::Str(s));
    match name {
        // Strings.
        "len" => match arg(a, 0) {
            V::Null => Ok(V::Null),
            v => Ok(V::Int(to_str(v)?.chars().count() as i64)),
        },
        "left" => {
            need(a, 2, "Left")?;
            let c = chars(&s(a, 0)?);
            let k = (int(a, 1)?.max(0) as usize).min(c.len());
            str_v(c[..k].iter().collect())
        }
        "right" => {
            need(a, 2, "Right")?;
            let c = chars(&s(a, 0)?);
            let k = (int(a, 1)?.max(0) as usize).min(c.len());
            str_v(c[c.len() - k..].iter().collect())
        }
        "mid" => {
            need(a, 2, "Mid")?;
            let c = chars(&s(a, 0)?);
            let start = (int(a, 1)?.max(1) - 1) as usize;
            if start >= c.len() {
                return str_v(String::new());
            }
            let len = if present(a, 2) {
                int(a, 2)?.max(0) as usize
            } else {
                c.len()
            };
            str_v(c[start..(start + len).min(c.len())].iter().collect())
        }
        "instr" => {
            // InStr([start,] s1, s2 [, compare])
            let (start, hay, needle, cmp) =
                if a.len() >= 3 && arg(a, 0).is_numeric() && !matches!(arg(a, 0), V::Str(_)) {
                    (
                        int(a, 0)?.max(1) as usize,
                        s(a, 1)?,
                        s(a, 2)?,
                        present(a, 3) && int(a, 3)? == 1,
                    )
                } else {
                    (1, s(a, 0)?, s(a, 1)?, false)
                };
            let (h, nd) = if cmp {
                (hay.to_lowercase(), needle.to_lowercase())
            } else {
                (hay, needle)
            };
            let hc = chars(&h);
            let nc = chars(&nd);
            if nc.is_empty() {
                return Ok(V::Int(start.min(hc.len() + 1) as i64));
            }
            let pos = (start - 1..hc.len().saturating_sub(nc.len()) + 1)
                .find(|&i| hc[i..].starts_with(&nc));
            Ok(V::Int(pos.map_or(0, |p| p as i64 + 1)))
        }
        "instrrev" => {
            let hc = chars(&s(a, 0)?);
            let nc = chars(&s(a, 1)?);
            let start = if present(a, 2) && int(a, 2)? > 0 {
                int(a, 2)? as usize
            } else {
                hc.len()
            };
            if nc.is_empty() || nc.len() > hc.len() {
                return Ok(V::Int(0));
            }
            let last = start.min(hc.len()).saturating_sub(nc.len());
            let pos = (0..=last).rev().find(|&i| hc[i..].starts_with(&nc));
            Ok(V::Int(pos.map_or(0, |p| p as i64 + 1)))
        }
        "replace" => {
            need(a, 3, "Replace")?;
            let (t, f, r) = (s(a, 0)?, s(a, 1)?, s(a, 2)?);
            if f.is_empty() {
                return str_v(t);
            }
            let count = if present(a, 4) { int(a, 4)? } else { -1 };
            let ci = present(a, 5) && int(a, 5)? == 1;
            let mut out = String::new();
            let mut rest: &str = &t;
            let mut done = 0;
            loop {
                let found = if ci {
                    rest.to_lowercase().find(&f.to_lowercase())
                } else {
                    rest.find(&f)
                };
                match found {
                    Some(p) if count < 0 || done < count => {
                        out.push_str(&rest[..p]);
                        out.push_str(&r);
                        rest = &rest[p + f.len()..];
                        done += 1;
                    }
                    _ => break,
                }
            }
            out.push_str(rest);
            str_v(out)
        }
        "trim" => str_v(s(a, 0)?.trim_matches(' ').to_owned()),
        "ltrim" => str_v(s(a, 0)?.trim_start_matches(' ').to_owned()),
        "rtrim" => str_v(s(a, 0)?.trim_end_matches(' ').to_owned()),
        "ucase" => str_v(s(a, 0)?.to_uppercase()),
        "lcase" => str_v(s(a, 0)?.to_lowercase()),
        "space" => str_v(" ".repeat(int(a, 0)?.max(0) as usize)),
        "string" => {
            let c = match arg(a, 1) {
                V::Str(t) => t.chars().next().unwrap_or(' '),
                v => char::from_u32(to_int(v)? as u32).unwrap_or(' '),
            };
            str_v(std::iter::repeat_n(c, int(a, 0)?.max(0) as usize).collect())
        }
        "strreverse" => str_v(s(a, 0)?.chars().rev().collect()),
        "strcomp" => {
            let (x, y) = (s(a, 0)?, s(a, 1)?);
            let ci = present(a, 2) && int(a, 2)? == 1;
            let (x, y) = if ci {
                (x.to_lowercase(), y.to_lowercase())
            } else {
                (x, y)
            };
            Ok(V::Int(match x.cmp(&y) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }))
        }
        "chr" | "chrw" => str_v(
            char::from_u32(int(a, 0)? as u32)
                .map(String::from)
                .ok_or_else(|| RtError::new(5, "Invalid procedure call or argument"))?,
        ),
        "asc" | "ascw" => s(a, 0)?
            .chars()
            .next()
            .map(|c| V::Int(c as i64))
            .ok_or_else(|| RtError::new(5, "Invalid procedure call or argument")),
        "split" => {
            let t = s(a, 0)?;
            let d = if present(a, 1) { s(a, 1)? } else { " ".into() };
            let parts: Vec<V> = if t.is_empty() {
                Vec::new()
            } else if d.is_empty() {
                vec![V::Str(t)]
            } else {
                t.split(d.as_str()).map(|p| V::Str(p.to_owned())).collect()
            };
            let mut arr = Array::new(&[(0, parts.len() as i64 - 1)]);
            arr.data = parts;
            Ok(V::Arr(Box::new(arr)))
        }
        "join" => {
            let V::Arr(arr) = arg(a, 0) else {
                return Err(RtError::mismatch());
            };
            let d = if present(a, 1) { s(a, 1)? } else { " ".into() };
            let parts: R<Vec<String>> = arr.data.iter().map(to_str).collect();
            str_v(parts?.join(&d))
        }
        "format" | "format$" => {
            let f = if present(a, 1) {
                s(a, 1)?
            } else {
                String::new()
            };
            str_v(format(arg(a, 0), &f)?)
        }
        "formatnumber" => {
            let d = if present(a, 1) { int(a, 1)? } else { 2 };
            let code = if d > 0 {
                format!("#,##0.{}", "0".repeat(d as usize))
            } else {
                "#,##0".into()
            };
            str_v(numfmt::format_number(n(a, 0)?, &code, false))
        }
        "formatpercent" => {
            let d = if present(a, 1) { int(a, 1)? } else { 2 };
            let code = if d > 0 {
                format!("0.{}%", "0".repeat(d as usize))
            } else {
                "0%".into()
            };
            str_v(numfmt::format_number(n(a, 0)?, &code, false))
        }
        "hex" => str_v(format!("{:X}", int(a, 0)?)),
        "oct" => str_v(format!("{:o}", int(a, 0)?)),
        "str" => {
            let v = n(a, 0)?;
            str_v(if v >= 0.0 {
                format!(" {}", num_text(v))
            } else {
                num_text(v)
            })
        }
        "val" => {
            // The longest numeric prefix, spaces ignored.
            let t: String = s(a, 0)?.chars().filter(|c| !c.is_whitespace()).collect();
            let mut best = 0.0;
            for k in (1..=t.len()).rev() {
                if !t.is_char_boundary(k) {
                    continue;
                }
                if let Ok(v) = t[..k].parse::<f64>() {
                    best = v;
                    break;
                }
            }
            Ok(V::Num(best))
        }
        // Conversions.
        "cstr" => str_v(s(a, 0)?),
        "cint" | "clng" | "clnglng" | "cbyte" => Ok(V::Int(int(a, 0)?)),
        "cdbl" | "csng" | "ccur" | "cdec" => Ok(V::Num(n(a, 0)?)),
        "cbool" => Ok(V::Bool(to_bool(arg(a, 0))?)),
        "cdate" | "datevalue" => match arg(a, 0) {
            V::Str(t) => parse_date(t)
                .map(|d| V::Date(if name == "datevalue" { d.floor() } else { d }))
                .ok_or_else(RtError::mismatch),
            v => Ok(V::Date(to_num(v)?)),
        },
        "timevalue" => match arg(a, 0) {
            V::Str(t) => parse_date(t)
                .map(|d| V::Date(d - d.floor()))
                .ok_or_else(RtError::mismatch),
            v => Ok(V::Date(to_num(v)?.fract())),
        },
        "cvar" => Ok(arg(a, 0).clone()),
        "cverr" => Ok(V::Error(int(a, 0)?)),
        // Tests.
        "isnumeric" => Ok(V::Bool(arg(a, 0).is_numeric())),
        "isempty" => Ok(V::Bool(matches!(arg(a, 0), V::Empty))),
        "isnull" => Ok(V::Bool(matches!(arg(a, 0), V::Null))),
        "isarray" => Ok(V::Bool(matches!(arg(a, 0), V::Arr(_)))),
        "isobject" => Ok(V::Bool(matches!(arg(a, 0), V::Obj(_) | V::Nothing))),
        "ismissing" => Ok(V::Bool(matches!(arg(a, 0), V::Missing))),
        "iserror" => Ok(V::Bool(matches!(arg(a, 0), V::Error(_)))),
        "isdate" => Ok(V::Bool(match arg(a, 0) {
            V::Date(_) => true,
            V::Str(t) => parse_date(t).is_some(),
            _ => false,
        })),
        "typename" => str_v(arg(a, 0).type_name().into()),
        "vartype" => Ok(V::Int(arg(a, 0).var_type())),
        // Math.
        "abs" => Ok(match arg(a, 0) {
            V::Int(i) => V::Int(i.abs()),
            v => V::Num(to_num(v)?.abs()),
        }),
        "int" => Ok(V::Num(n(a, 0)?.floor())),
        "fix" => Ok(V::Num(n(a, 0)?.trunc())),
        "sgn" => Ok(V::Int(
            n(a, 0)?.signum() as i64 * i64::from(n(a, 0)? != 0.0),
        )),
        "sqr" => {
            let x = n(a, 0)?;
            if x < 0.0 {
                return Err(RtError::new(5, "Invalid procedure call or argument"));
            }
            Ok(V::Num(x.sqrt()))
        }
        "exp" => Ok(V::Num(n(a, 0)?.exp())),
        "log" => {
            let x = n(a, 0)?;
            if x <= 0.0 {
                return Err(RtError::new(5, "Invalid procedure call or argument"));
            }
            Ok(V::Num(x.ln()))
        }
        "sin" => Ok(V::Num(n(a, 0)?.sin())),
        "cos" => Ok(V::Num(n(a, 0)?.cos())),
        "tan" => Ok(V::Num(n(a, 0)?.tan())),
        "atn" => Ok(V::Num(n(a, 0)?.atan())),
        "round" => Ok(V::Num(vba_round(
            n(a, 0)?,
            if present(a, 1) { int(a, 1)? } else { 0 },
        ))),
        "rnd" => {
            // A small generator: macros that need randomness get it, not reproducibly.
            let t = crate::time::SystemTime::now()
                .duration_since(crate::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let x = (u64::from(t)
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407)
                >> 11) as f64;
            Ok(V::Num(x / (1u64 << 53) as f64))
        }
        "randomize" => Ok(V::Empty),
        // Dates.
        "now" => Ok(V::Date(now_serial())),
        "date" => Ok(V::Date(now_serial().floor())),
        "time" => Ok(V::Date(now_serial().fract())),
        "timer" => Ok(V::Num(now_serial().fract() * 86_400.0)),
        "year" | "month" | "day" | "weekday" => {
            let d = n(a, 0)?;
            let (y, m, dd, wd) = serial_to_date(d.floor() as i64, false);
            Ok(V::Int(match name {
                "year" => y,
                "month" => i64::from(m),
                "day" => i64::from(dd),
                _ => {
                    let first = if present(a, 1) { int(a, 1)?.max(1) } else { 1 };
                    (i64::from(wd) + 1 - first).rem_euclid(7) + 1
                }
            }))
        }
        "hour" | "minute" | "second" => {
            let d = n(a, 0)?;
            let secs = ((d - d.floor()) * 86_400.0).round() as i64;
            Ok(V::Int(match name {
                "hour" => secs / 3600,
                "minute" => secs / 60 % 60,
                _ => secs % 60,
            }))
        }
        "dateserial" => Ok(V::Date(date_serial(int(a, 0)?, int(a, 1)?, int(a, 2)?))),
        "timeserial" => Ok(V::Date(
            (int(a, 0)? * 3600 + int(a, 1)? * 60 + int(a, 2)?) as f64 / 86_400.0,
        )),
        "dateadd" => {
            let i = s(a, 0)?.to_lowercase();
            let k = n(a, 1)?;
            let d = n(a, 2)?;
            Ok(V::Date(match i.as_str() {
                "m" => add_months(d, k as i64),
                "q" => add_months(d, 3 * k as i64),
                "yyyy" => add_months(d, 12 * k as i64),
                other => {
                    d + k * interval_seconds(other)
                        .ok_or_else(|| RtError::new(5, format!("unknown interval {other}")))?
                        / 86_400.0
                }
            }))
        }
        "datediff" => {
            let i = s(a, 0)?.to_lowercase();
            let (d1, d2) = (n(a, 1)?, n(a, 2)?);
            let (y1, m1, _, _) = serial_to_date(d1.floor() as i64, false);
            let (y2, m2, _, _) = serial_to_date(d2.floor() as i64, false);
            Ok(V::Int(match i.as_str() {
                "yyyy" => y2 - y1,
                "q" => (y2 * 4 + i64::from(m2 - 1) / 3) - (y1 * 4 + i64::from(m1 - 1) / 3),
                "m" => (y2 * 12 + i64::from(m2)) - (y1 * 12 + i64::from(m1)),
                "d" | "y" | "w" => (d2.floor() - d1.floor()) as i64,
                "ww" => ((d2.floor() - d1.floor()) / 7.0).trunc() as i64,
                other => {
                    let unit = interval_seconds(other)
                        .ok_or_else(|| RtError::new(5, format!("unknown interval {other}")))?;
                    ((d2 - d1) * 86_400.0 / unit).round() as i64
                }
            }))
        }
        "monthname" => {
            const M: [&str; 12] = [
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
            let m = int(a, 0)?;
            let full = M
                .get((m - 1).clamp(0, 11) as usize)
                .copied()
                .unwrap_or_default();
            str_v(if present(a, 1) && to_bool(arg(a, 1))? {
                full[..3].to_owned()
            } else {
                full.to_owned()
            })
        }
        "weekdayname" => {
            const D: [&str; 7] = [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ];
            let w = int(a, 0)?;
            let full = D
                .get((w - 1).clamp(0, 6) as usize)
                .copied()
                .unwrap_or_default();
            str_v(if present(a, 1) && to_bool(arg(a, 1))? {
                full[..3].to_owned()
            } else {
                full.to_owned()
            })
        }
        // Arrays and choices.
        "array" => {
            let mut arr = Array::new(&[(0, a.len() as i64 - 1)]);
            arr.data = a.to_vec();
            Ok(V::Arr(Box::new(arr)))
        }
        "lbound" | "ubound" => {
            let V::Arr(arr) = arg(a, 0) else {
                return Err(RtError::mismatch());
            };
            let dim = if present(a, 1) { int(a, 1)? } else { 1 };
            let Some(&(l, len)) = arr.dims.get((dim - 1).max(0) as usize) else {
                return Err(RtError::new(9, "Subscript out of range"));
            };
            Ok(V::Int(if name == "lbound" {
                l
            } else {
                l + len as i64 - 1
            }))
        }
        "iif" => Ok(if to_bool(arg(a, 0))? {
            arg(a, 1).clone()
        } else {
            arg(a, 2).clone()
        }),
        "choose" => {
            let i = int(a, 0)?;
            Ok(if i >= 1 && (i as usize) < a.len() {
                a[i as usize].clone()
            } else {
                V::Null
            })
        }
        "switch" => {
            for pair in a.chunks(2) {
                if pair.len() == 2 && to_bool(&pair[0])? {
                    return Ok(pair[1].clone());
                }
            }
            Ok(V::Null)
        }
        "nz" => Ok(match arg(a, 0) {
            V::Null | V::Empty => arg(a, 1).clone(),
            v => v.clone(),
        }),
        _ => Err(RtError::new(0, String::new())),
    }
}

/// Whether `name` is a library function this module knows.
pub fn known(name: &str) -> bool {
    !matches!(call_inner(name, &[V::Missing; 0]), Err(e) if e.number == 0 && e.description.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, args: &[V]) -> V {
        call(name, args).unwrap().unwrap()
    }

    fn st(x: &str) -> V {
        V::Str(x.into())
    }

    #[test]
    fn strings() {
        assert_eq!(c("left", &[st("Kalem"), V::Int(3)]), st("Kal"));
        assert_eq!(c("mid", &[st("Kalem"), V::Int(2), V::Int(2)]), st("al"));
        assert_eq!(c("instr", &[st("abcabc"), st("c")]), V::Int(3));
        assert_eq!(c("instr", &[V::Int(4), st("abcabc"), st("c")]), V::Int(6));
        assert_eq!(c("instrrev", &[st("abcabc"), st("b")]), V::Int(5));
        assert_eq!(c("replace", &[st("a-b-c"), st("-"), st("+")]), st("a+b+c"));
        assert_eq!(c("ucase", &[st("ışık")]), st("IŞIK"));
        let parts = c("split", &[st("a,b,c"), st(",")]);
        assert_eq!(c("ubound", std::slice::from_ref(&parts)), V::Int(2));
        assert_eq!(c("join", &[parts, st("|")]), st("a|b|c"));
        assert_eq!(
            c("format", &[V::Num(1234.5), st("#,##0.00")]),
            st("1,234.50")
        );
        assert_eq!(
            c("format", &[V::Date(45_000.0), st("yyyy-mm-dd")]),
            st("2023-03-15")
        );
        assert!(
            like("Kalem", "K*m") && like("a1", "a#") && like("b", "[a-c]") && !like("d", "[a-c]")
        );
    }

    #[test]
    fn math_and_dates() {
        assert_eq!(c("round", &[V::Num(2.5)]), V::Num(2.0));
        assert_eq!(c("round", &[V::Num(2.675), V::Int(2)]), V::Num(2.68));
        assert_eq!(c("int", &[V::Num(-1.5)]), V::Num(-2.0));
        assert_eq!(c("fix", &[V::Num(-1.5)]), V::Num(-1.0));
        assert_eq!(
            c("dateserial", &[V::Int(2023), V::Int(3), V::Int(15)]),
            V::Date(45_000.0)
        );
        assert_eq!(
            c("dateserial", &[V::Int(2023), V::Int(14), V::Int(1)]),
            V::Date(date_serial(2024, 2, 1))
        );
        assert_eq!(c("year", &[V::Date(45_000.0)]), V::Int(2023));
        assert_eq!(c("weekday", &[V::Date(45_000.0)]), V::Int(4));
        assert_eq!(
            c(
                "dateadd",
                &[st("m"), V::Int(1), V::Date(date_serial(2023, 1, 31))]
            ),
            V::Date(date_serial(2023, 2, 28))
        );
        assert_eq!(
            c("datediff", &[st("d"), V::Date(45_000.0), V::Date(45_010.0)]),
            V::Int(10)
        );
        assert_eq!(date_text(45_000.5), "3/15/2023 12:00:00 PM");
        assert!(known("left") && !known("msgbox"));
    }
}
