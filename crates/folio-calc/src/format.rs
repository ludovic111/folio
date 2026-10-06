//! Number formats (Excel-compatible format codes) and date serials.
//!
//! Dates are numbers counted in days in the Excel 1900 system: 1 is 1900-01-01, and 60 is the
//! 1900-02-29 that never existed but that Excel (after Lotus 1-2-3) counts, so 61 is 1900-03-01.
//! The fraction of a number is the time of day. Keeping this quirk means serials read from or
//! written to XLSX files match.
//!
//! Supported format codes: `0`, `#`, `?` digit placeholders, `.` decimals, `,` thousands (and
//! trailing `,` to scale by 1000), `%`, `E+00` scientific, quoted literal text, `\x` escapes,
//! `_x` spaces, `*x` fills (dropped), `@` text, up to four sections `positive;negative;zero;text`,
//! `[Red]`-style colour tags and `[>100]` conditions (read and ignored), `[$€-407]` currency
//! tags (the symbol is kept), and the date and time codes `yyyy yy mmmm mmm mm m dddd ddd dd d
//! hh h mm ss .0 AM/PM A/P` with elapsed `[h] [m] [s]`. Fractions (`# ?/?`) are not supported.

use crate::value::Value;

/// Number formats offered in the format menu: (label, code).
pub const PRESETS: &[(&str, &str)] = &[
    ("General", "General"),
    ("Number", "0.00"),
    ("Thousands", "#,##0"),
    ("Thousands with decimals", "#,##0.00"),
    ("Percent", "0%"),
    ("Percent with decimals", "0.00%"),
    ("Currency ($)", "$#,##0.00"),
    ("Currency (€)", "#,##0.00 €"),
    ("Accounting", "#,##0.00;(#,##0.00);\"-\""),
    ("Date", "yyyy-mm-dd"),
    ("Long date", "dddd d mmmm yyyy"),
    ("Time", "h:mm"),
    ("Date time", "yyyy-mm-dd h:mm"),
    ("Duration", "[h]:mm:ss"),
    ("Scientific", "0.00E+00"),
    ("Text", "@"),
];

// ---------------------------------------------------------------------------------------------
// Dates

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm). The day may
/// overflow the month; it simply counts on.
pub(crate) fn days_from_civil(y: i64, m: u32, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day count from 1970-01-01.
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub(crate) fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub(crate) fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

/// 1899-12-30 as a day count from 1970-01-01: serial 0 for dates from 1900-03-01 on.
const EPOCH: i64 = -25_569;

/// The Excel serial of a date (1 = 1900-01-01, 60 = the fictitious 1900-02-29). A day past the
/// end of the month counts on into the next month.
pub fn date_to_serial(y: i32, m: u32, d: u32) -> f64 {
    if (y, m, d) == (1900, 2, 29) {
        return 60.0;
    }
    let m = m.clamp(1, 12);
    let days = days_from_civil(y as i64, m, d as i64) - EPOCH;
    (if days < 61 { days - 1 } else { days }) as f64
}

/// The date of an Excel serial (the fraction is ignored). Serial 0 gives `(1900, 1, 0)` and 60
/// gives `(1900, 2, 29)`, as Excel shows them.
pub fn serial_to_date(serial: f64) -> (i32, u32, u32) {
    let n = serial.floor() as i64;
    if n == 0 {
        return (1900, 1, 0);
    }
    if n == 60 {
        return (1900, 2, 29);
    }
    let days = if n < 60 { n + 1 } else { n };
    let (y, m, d) = civil_from_days(days + EPOCH);
    (y as i32, m, d)
}

/// The fraction of a day for a time of day: `time_fraction(12, 0, 0.0)` is 0.5.
pub fn time_fraction(h: u32, m: u32, s: f64) -> f64 {
    (h as f64 * 3600.0 + m as f64 * 60.0 + s) / 86_400.0
}

/// The weekday of a serial, 0 = Sunday … 6 = Saturday (Excel's numbering, so serial 1 is a Sunday).
pub(crate) fn weekday0(serial: f64) -> u32 {
    ((serial.floor() as i64 - 1).rem_euclid(7)) as u32
}

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

pub(crate) fn month_name(m: u32) -> &'static str {
    MONTHS[(m.clamp(1, 12) - 1) as usize]
}

/// The month number of an English month name or its abbreviation ("Oct", "sept", "october").
pub(crate) fn month_from_name(s: &str) -> Option<u32> {
    let lower = s.to_ascii_lowercase();
    let lower = lower.trim_end_matches('.');
    if lower.len() < 3 {
        return None;
    }
    if lower == "sept" {
        return Some(9);
    }
    MONTHS
        .iter()
        .position(|name| {
            let name = name.to_ascii_lowercase();
            name == lower || (lower.len() == 3 && name.starts_with(lower))
        })
        .map(|i| i as u32 + 1)
}

// ---------------------------------------------------------------------------------------------
// Numbers as text

/// Rounds to `digits` significant digits (used to hide binary noise like 0.30000000000000004).
pub(crate) fn round_sig(x: f64, digits: usize) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{:.*e}", digits.saturating_sub(1), x).parse().unwrap_or(x)
}

/// Rounds half away from zero to `digits` decimals (negative: to tens, hundreds…), the way
/// Excel's ROUND does, after removing binary noise (so 2.675 rounds to 2.68).
pub(crate) fn round_half_away(x: f64, digits: i32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let digits = digits.clamp(-308, 308);
    let scale = 10f64.powi(digits.abs());
    let scaled = if digits >= 0 { x * scale } else { x / scale };
    if !scaled.is_finite() || scaled.abs() >= 1e17 {
        return x;
    }
    let rounded = round_sig(scaled, 15).round();
    if digits >= 0 { rounded / scale } else { rounded * scale }
}

/// The General format: up to 11 characters, scientific for very large or very small numbers.
pub fn general(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if !n.is_finite() {
        return "#NUM!".into();
    }
    let a = n.abs();
    let sign = if n < 0.0 { "-" } else { "" };
    if !(1e-5..1e11).contains(&a) {
        return format!("{sign}{}", scientific(a, 5));
    }
    let int_len = if a < 1.0 { 1 } else { (a.log10().floor() as i32 + 1).max(1) };
    let decimals = (10 - int_len).max(0);
    let rounded = round_half_away(a, decimals);
    let mut s = format!("{:.*}", decimals as usize, rounded);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if s.len() > 11 {
        return format!("{sign}{}", scientific(a, 5));
    }
    if s == "0" {
        return format!("{sign}{}", scientific(a, 5));
    }
    format!("{sign}{s}")
}

/// `1.23457E+11`: a mantissa with up to `decimals` decimals (trailing zeros dropped).
fn scientific(a: f64, decimals: usize) -> String {
    let s = format!("{:.*e}", decimals, a);
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let mant = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
    let exp: i32 = exp.parse().unwrap_or(0);
    format!("{mant}E{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

/// A number turned into text by a formula (`=1/3&""`): up to 15 significant digits.
pub(crate) fn number_to_text(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    let r = round_sig(n, 15);
    if r.abs() >= 1e21 || r.abs() < 1e-19 {
        let sign = if r < 0.0 { "-" } else { "" };
        return format!("{sign}{}", scientific(r.abs(), 14));
    }
    format!("{r}")
}

// ---------------------------------------------------------------------------------------------
// Format codes

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Lit(String),
    Digit(char),
    Point,
    Comma,
    Percent,
    /// `E+` (always shows the sign) or `E-`.
    Exp(bool),
    At,
    General,
    Year(usize),
    Month(usize),
    Minute(usize),
    Day(usize),
    Hour(usize),
    Second(usize),
    SubSecond(usize),
    /// `AM/PM` (`true`) or `A/P`, with the case as written.
    AmPm(bool, bool),
    Elapsed(char),
}

#[derive(Clone, Debug, Default)]
struct Section {
    toks: Vec<Tok>,
    is_date: bool,
    has_ampm: bool,
}

/// Splits a format code into sections at `;` outside quotes, brackets and escapes.
fn split_sections(code: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut in_quote, mut in_bracket, mut escaped) = (0, false, false, false);
    for (i, c) in code.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if !in_quote => escaped = true,
            '"' if !in_bracket => in_quote = !in_quote,
            '[' if !in_quote => in_bracket = true,
            ']' if !in_quote => in_bracket = false,
            ';' if !in_quote && !in_bracket => {
                out.push(&code[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&code[start..]);
    out
}

fn parse_section(src: &str) -> Section {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let run = |i: usize, lower: char| {
        let mut j = i;
        while j < chars.len() && chars[j].to_ascii_lowercase() == lower {
            j += 1;
        }
        j - i
    };
    while i < chars.len() {
        let c = chars[i];
        let lower = c.to_ascii_lowercase();
        match c {
            '"' => {
                let mut j = i + 1;
                let mut lit = String::new();
                while j < chars.len() && chars[j] != '"' {
                    lit.push(chars[j]);
                    j += 1;
                }
                toks.push(Tok::Lit(lit));
                i = j + 1;
            }
            '\\' => {
                if let Some(&next) = chars.get(i + 1) {
                    toks.push(Tok::Lit(next.to_string()));
                }
                i += 2;
            }
            '_' => {
                toks.push(Tok::Lit(" ".into()));
                i += 2;
            }
            '*' => i += 2,
            '[' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                let inner: String = chars[i + 1..j.min(chars.len())].iter().collect();
                let low = inner.to_ascii_lowercase();
                if !low.is_empty() && low.chars().all(|c| c == 'h') {
                    toks.push(Tok::Elapsed('h'));
                } else if !low.is_empty() && low.chars().all(|c| c == 'm') {
                    toks.push(Tok::Elapsed('m'));
                } else if !low.is_empty() && low.chars().all(|c| c == 's') {
                    toks.push(Tok::Elapsed('s'));
                } else if let Some(rest) = inner.strip_prefix('$') {
                    let symbol = rest.split('-').next().unwrap_or("");
                    if !symbol.is_empty() {
                        toks.push(Tok::Lit(symbol.to_string()));
                    }
                }
                // Colours and conditions are read and ignored.
                i = j + 1;
            }
            '0' | '#' | '?' => {
                toks.push(Tok::Digit(c));
                i += 1;
            }
            '.' => {
                toks.push(Tok::Point);
                i += 1;
            }
            ',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            '%' => {
                toks.push(Tok::Percent);
                i += 1;
            }
            '@' => {
                toks.push(Tok::At);
                i += 1;
            }
            'E' | 'e' if matches!(chars.get(i + 1), Some('+') | Some('-')) => {
                toks.push(Tok::Exp(chars[i + 1] == '+'));
                i += 2;
            }
            _ if lower == 'y' => {
                let n = run(i, 'y');
                toks.push(Tok::Year(n));
                i += n;
            }
            _ if lower == 'm' => {
                let n = run(i, 'm');
                toks.push(Tok::Month(n));
                i += n;
            }
            _ if lower == 'd' => {
                let n = run(i, 'd');
                toks.push(Tok::Day(n));
                i += n;
            }
            _ if lower == 'h' => {
                let n = run(i, 'h');
                toks.push(Tok::Hour(n));
                i += n;
            }
            _ if lower == 's' => {
                let n = run(i, 's');
                toks.push(Tok::Second(n));
                i += n;
            }
            _ if lower == 'a' => {
                let rest: String = chars[i..].iter().take(5).collect();
                if rest.eq_ignore_ascii_case("am/pm") {
                    toks.push(Tok::AmPm(true, c == 'a'));
                    i += 5;
                } else if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case("a/p") {
                    toks.push(Tok::AmPm(false, c == 'a'));
                    i += 3;
                } else {
                    toks.push(Tok::Lit(c.to_string()));
                    i += 1;
                }
            }
            _ if lower == 'g' => {
                let rest: String = chars[i..].iter().take(7).collect();
                if rest.eq_ignore_ascii_case("general") {
                    toks.push(Tok::General);
                    i += 7;
                } else {
                    toks.push(Tok::Lit(c.to_string()));
                    i += 1;
                }
            }
            _ => {
                toks.push(Tok::Lit(c.to_string()));
                i += 1;
            }
        }
    }
    let is_date_tok = |t: &Tok| {
        matches!(
            t,
            Tok::Year(_)
                | Tok::Month(_)
                | Tok::Minute(_)
                | Tok::Day(_)
                | Tok::Hour(_)
                | Tok::Second(_)
                | Tok::AmPm(..)
                | Tok::Elapsed(_)
        )
    };
    let is_date = toks.iter().any(is_date_tok);
    if is_date {
        // `m` right after hours or right before seconds means minutes.
        for i in 0..toks.len() {
            if let Tok::Month(n) = toks[i] {
                let prev = toks[..i].iter().rev().find(|t| is_date_tok(t));
                let next = toks[i + 1..].iter().find(|t| is_date_tok(t));
                let after_hour = matches!(prev, Some(Tok::Hour(_)) | Some(Tok::Elapsed('h')));
                let before_second = matches!(next, Some(Tok::Second(_)) | Some(Tok::Elapsed('s')));
                if after_hour || before_second {
                    toks[i] = Tok::Minute(n);
                }
            }
        }
        // `ss.00`: fractions of a second.
        let mut i = 0;
        while i + 1 < toks.len() {
            if matches!(toks[i], Tok::Second(_) | Tok::Elapsed('s')) && toks[i + 1] == Tok::Point {
                let mut n = 0;
                while matches!(toks.get(i + 2 + n), Some(Tok::Digit('0'))) {
                    n += 1;
                }
                if n > 0 {
                    toks.splice(i + 1..i + 2 + n, [Tok::SubSecond(n)]);
                }
            }
            i += 1;
        }
        // In a date section, digit placeholders and the like are plain text.
        for t in toks.iter_mut() {
            match t {
                Tok::Digit(c) => *t = Tok::Lit(c.to_string()),
                Tok::Point => *t = Tok::Lit(".".into()),
                Tok::Comma => *t = Tok::Lit(",".into()),
                _ => {}
            }
        }
    }
    let has_ampm = toks.iter().any(|t| matches!(t, Tok::AmPm(..)));
    Section { toks, is_date, has_ampm }
}

/// Formats a value for display with an Excel-style format code (`None` or `"General"`: General).
pub fn format_value(v: &Value, format: Option<&str>) -> String {
    let code = format.map(str::trim).filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("general"));
    match v {
        Value::Empty => String::new(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(e) => e.code().into(),
        Value::Text(s) => match code {
            None => s.clone(),
            Some(code) => {
                let sections = split_sections(code);
                let section = if sections.len() >= 4 {
                    Some(sections[3])
                } else {
                    sections.first().copied().filter(|s| s.contains('@'))
                };
                match section {
                    Some(src) => render_text(&parse_section(src), s),
                    None => s.clone(),
                }
            }
        },
        Value::Number(n) => match code {
            None => general(*n),
            Some(code) => format_number(*n, code),
        },
    }
}

fn render_text(section: &Section, text: &str) -> String {
    let mut out = String::new();
    for t in &section.toks {
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::At => out.push_str(text),
            _ => {}
        }
    }
    out
}

fn format_number(n: f64, code: &str) -> String {
    let sources = split_sections(code);
    let sections: Vec<Section> = sources.iter().take(3).map(|s| parse_section(s)).collect();
    let (section, value, auto_minus) = match sections.len() {
        1 => (&sections[0], n, true),
        2 => {
            if n < 0.0 {
                (&sections[1], -n, false)
            } else {
                (&sections[0], n, false)
            }
        }
        _ => {
            if n > 0.0 {
                (&sections[0], n, false)
            } else if n < 0.0 {
                (&sections[1], -n, false)
            } else {
                (&sections[2], n, false)
            }
        }
    };
    if section.toks.is_empty() {
        return String::new();
    }
    if section.toks.iter().all(|t| matches!(t, Tok::At | Tok::Lit(_))) && section.toks.contains(&Tok::At) {
        // A text format: numbers show as General.
        return general(n);
    }
    if section.is_date {
        if value < 0.0 || value >= 2_958_466.0 {
            return "#####".into();
        }
        return render_date(section, value);
    }
    let body = render_number(section, value.abs());
    if auto_minus && value < 0.0 && body.chars().any(|c| c.is_ascii_digit() && c != '0') {
        format!("-{body}")
    } else {
        body
    }
}

fn group_thousands(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn render_number(section: &Section, x: f64) -> String {
    let toks = &section.toks;
    let exp_pos = toks.iter().position(|t| matches!(t, Tok::Exp(_)));
    let num_end = exp_pos.unwrap_or(toks.len());
    let point_pos = toks[..num_end].iter().position(|t| *t == Tok::Point);
    let int_end = point_pos.unwrap_or(num_end);
    let int_slots: Vec<usize> = (0..int_end).filter(|&i| matches!(toks[i], Tok::Digit(_))).collect();
    let frac_slots: Vec<usize> = (int_end..num_end).filter(|&i| matches!(toks[i], Tok::Digit(_))).collect();
    let exp_slots: Vec<usize> = (num_end..toks.len()).filter(|&i| matches!(toks[i], Tok::Digit(_))).collect();

    // Commas: between integer digits they group thousands; after the digits they scale by 1000.
    let mut thousands = false;
    let mut scale = 0;
    let mut comma_lit = vec![false; toks.len()];
    for (i, t) in toks.iter().enumerate() {
        if *t == Tok::Comma {
            let before = toks[..i].iter().any(|t| matches!(t, Tok::Digit(_)));
            let after_in_int = int_slots.iter().any(|&s| s > i);
            if before && after_in_int {
                thousands = true;
            } else if before {
                scale += 1;
            } else {
                comma_lit[i] = true;
            }
        }
    }
    let percents = toks.iter().filter(|t| **t == Tok::Percent).count() as i32;
    let mut x = x * 100f64.powi(percents) / 1000f64.powi(scale);
    let decimals = frac_slots.len();

    let mut exponent = 0i32;
    if exp_pos.is_some() {
        let k = int_slots.len().max(1) as i32;
        let engineering = k > 1 && int_slots.iter().any(|&i| toks[i] == Tok::Digit('#'));
        if x != 0.0 {
            let e = x.log10().floor() as i32;
            exponent = if engineering { e.div_euclid(k) * k } else { e - (k - 1) };
            let mut mant = round_half_away(x / 10f64.powi(exponent), decimals as i32);
            let limit = 10f64.powi(k);
            if mant >= limit {
                exponent += if engineering { k } else { 1 };
                mant = round_half_away(x / 10f64.powi(exponent), decimals as i32);
            }
            x = mant;
        }
    } else {
        x = round_half_away(x, decimals as i32);
    }
    let text = format!("{:.*}", decimals, x);
    let (int_digits, frac_digits) = match text.split_once('.') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (text.clone(), String::new()),
    };

    // Integer part.
    let zeros = int_slots.iter().filter(|&&i| toks[i] == Tok::Digit('0')).count();
    let questions = int_slots.iter().filter(|&&i| toks[i] == Tok::Digit('?')).count();
    let mut digits = if int_digits == "0" { String::new() } else { int_digits };
    while digits.len() < zeros {
        digits.insert(0, '0');
    }
    let interleaved = match (int_slots.first(), int_slots.last()) {
        (Some(&a), Some(&b)) => toks[a..=b].iter().any(|t| matches!(t, Tok::Lit(_))),
        _ => false,
    };
    let mut slot_text: Vec<String> = vec![String::new(); toks.len()];
    if !int_slots.is_empty() {
        if interleaved {
            let chars: Vec<char> = digits.chars().collect();
            let mut k = chars.len();
            for (n, &slot) in int_slots.iter().enumerate().rev() {
                if n == 0 {
                    slot_text[slot] = chars[..k].iter().collect();
                } else if k > 0 {
                    k -= 1;
                    slot_text[slot] = chars[k].to_string();
                } else {
                    slot_text[slot] = match toks[slot] {
                        Tok::Digit('0') => "0".into(),
                        Tok::Digit('?') => " ".into(),
                        _ => String::new(),
                    };
                }
            }
        } else {
            let mut chunk = if thousands { group_thousands(&digits) } else { digits.clone() };
            while chunk.chars().count() < zeros + questions {
                chunk.insert(0, ' ');
            }
            slot_text[int_slots[0]] = chunk;
        }
    }

    // Decimal part: trailing zeros under `#` vanish, under `?` become spaces.
    let frac: Vec<char> = frac_digits.chars().collect();
    let mut trimming = true;
    for (k, &slot) in frac_slots.iter().enumerate().rev() {
        let d = frac.get(k).copied().unwrap_or('0');
        let kind = &toks[slot];
        slot_text[slot] = if trimming && d == '0' && *kind == Tok::Digit('#') {
            String::new()
        } else if trimming && d == '0' && *kind == Tok::Digit('?') {
            " ".into()
        } else {
            trimming = false;
            d.to_string()
        };
    }

    // Exponent digits.
    if let Some(&first) = exp_slots.first() {
        let mut e = exponent.abs().to_string();
        while e.len() < exp_slots.len() {
            e.insert(0, '0');
        }
        slot_text[first] = e;
    }

    let mut out = String::new();
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::Digit(_) => out.push_str(&slot_text[i]),
            Tok::Point => {
                if int_slots.is_empty() && !digits.is_empty() {
                    out.push_str(&digits);
                }
                out.push('.');
            }
            Tok::Comma => {
                if comma_lit[i] {
                    out.push(',');
                }
            }
            Tok::Percent => out.push('%'),
            Tok::Exp(plus) => {
                out.push('E');
                if exponent < 0 {
                    out.push('-');
                } else if *plus {
                    out.push('+');
                }
            }
            Tok::General => out.push_str(&general(x)),
            _ => {}
        }
    }
    out
}

fn render_date(section: &Section, v: f64) -> String {
    let sub = section.toks.iter().find_map(|t| if let Tok::SubSecond(n) = t { Some(*n) } else { None }).unwrap_or(0);
    let mut day = v.floor();
    let unit = 10f64.powi(sub as i32);
    // Time of day in units of 10^-sub seconds, rounded.
    let mut ticks = ((v - day) * 86_400.0 * unit).round();
    if ticks >= 86_400.0 * unit {
        day += 1.0;
        ticks -= 86_400.0 * unit;
    }
    let total_ticks = (v * 86_400.0 * unit).round();
    let secs = (ticks / unit).floor() as i64;
    let frac_ticks = (ticks - secs as f64 * unit) as i64;
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    let (y, mo, d) = serial_to_date(day);
    let wd = weekday0(day) as usize;
    let total_secs = (total_ticks / unit).floor() as i64;

    let mut out = String::new();
    for t in &section.toks {
        match *t {
            Tok::Lit(ref s) => out.push_str(s),
            Tok::Year(n) => {
                if n <= 2 {
                    out.push_str(&format!("{:02}", y.rem_euclid(100)));
                } else {
                    out.push_str(&format!("{y:04}"));
                }
            }
            Tok::Month(n) => match n {
                1 => out.push_str(&mo.to_string()),
                2 => out.push_str(&format!("{mo:02}")),
                3 => out.push_str(&month_name(mo)[..3]),
                4 => out.push_str(month_name(mo)),
                _ => out.push_str(&month_name(mo)[..1]),
            },
            Tok::Day(n) => match n {
                1 => out.push_str(&d.to_string()),
                2 => out.push_str(&format!("{d:02}")),
                3 => out.push_str(&DAYS[wd][..3]),
                _ => out.push_str(DAYS[wd]),
            },
            Tok::Hour(n) => {
                let hour = if section.has_ampm {
                    match h % 12 {
                        0 => 12,
                        x => x,
                    }
                } else {
                    h
                };
                if n >= 2 {
                    out.push_str(&format!("{hour:02}"));
                } else {
                    out.push_str(&hour.to_string());
                }
            }
            Tok::Minute(n) => {
                if n >= 2 {
                    out.push_str(&format!("{m:02}"));
                } else {
                    out.push_str(&m.to_string());
                }
            }
            Tok::Second(n) => {
                if n >= 2 {
                    out.push_str(&format!("{s:02}"));
                } else {
                    out.push_str(&s.to_string());
                }
            }
            Tok::SubSecond(n) => {
                out.push('.');
                out.push_str(&format!("{:0width$}", frac_ticks, width = n));
            }
            Tok::AmPm(long, lower) => {
                let pm = h >= 12;
                let text = match (long, pm) {
                    (true, false) => "AM",
                    (true, true) => "PM",
                    (false, false) => "A",
                    (false, true) => "P",
                };
                if lower {
                    out.push_str(&text.to_ascii_lowercase());
                } else {
                    out.push_str(text);
                }
            }
            Tok::Elapsed(unit) => {
                let n = match unit {
                    'h' => total_secs / 3600,
                    'm' => total_secs / 60,
                    _ => total_secs,
                };
                out.push_str(&n.to_string());
            }
            _ => {}
        }
    }
    out
}

/// True when a format code shows dates or times (used to pick how to show a typed value).
pub fn is_date_format(code: &str) -> bool {
    split_sections(code).first().is_some_and(|s| parse_section(s).is_date)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: f64, code: &str) -> String {
        format_value(&Value::Number(n), Some(code))
    }

    #[test]
    fn serials() {
        assert_eq!(date_to_serial(1900, 1, 1), 1.0);
        assert_eq!(date_to_serial(1900, 2, 28), 59.0);
        assert_eq!(date_to_serial(1900, 2, 29), 60.0);
        assert_eq!(date_to_serial(1900, 3, 1), 61.0);
        assert_eq!(date_to_serial(1899, 12, 31), 0.0);
        assert_eq!(date_to_serial(2025, 1, 1), 45_658.0);
        assert_eq!(date_to_serial(2025, 10, 6), 45_936.0);
        assert_eq!(date_to_serial(2026, 10, 6), 46_301.0);
        assert_eq!(date_to_serial(1970, 1, 1), 25_569.0);
        assert_eq!(date_to_serial(2000, 2, 29), 36_585.0);
        assert_eq!(date_to_serial(9999, 12, 31), 2_958_465.0);
        assert_eq!(date_to_serial(2026, 1, 32), date_to_serial(2026, 2, 1));
        assert_eq!(serial_to_date(1.0), (1900, 1, 1));
        assert_eq!(serial_to_date(59.0), (1900, 2, 28));
        assert_eq!(serial_to_date(60.0), (1900, 2, 29));
        assert_eq!(serial_to_date(61.0), (1900, 3, 1));
        assert_eq!(serial_to_date(45_936.75), (2025, 10, 6));
        assert_eq!(serial_to_date(0.0), (1900, 1, 0));
        for serial in [1.0, 59.0, 61.0, 366.0, 36_585.0, 45_936.0, 2_958_465.0] {
            let (y, m, d) = serial_to_date(serial);
            assert_eq!(date_to_serial(y, m, d), serial);
        }
        assert_eq!(time_fraction(12, 0, 0.0), 0.5);
        assert_eq!(time_fraction(6, 30, 0.0), 0.270_833_333_333_333_3);
        assert_eq!(weekday0(1.0), 0, "1900-01-01 is a Sunday for Excel");
        assert_eq!(weekday0(45_936.0), 1, "2025-10-06 is a Monday");
    }

    #[test]
    fn general_format() {
        assert_eq!(general(0.0), "0");
        assert_eq!(general(1.0), "1");
        assert_eq!(general(-12.5), "-12.5");
        assert_eq!(general(1.0 / 3.0), "0.333333333");
        assert_eq!(general(2.0 / 3.0), "0.666666667");
        assert_eq!(general(123_456.789_012), "123456.789");
        assert_eq!(general(12_345_678_901.0), "12345678901");
        assert_eq!(general(123_456_789_012.0), "1.23457E+11");
        assert_eq!(general(1e20), "1E+20");
        assert_eq!(general(0.0001), "0.0001");
        assert_eq!(general(0.000_001_234), "1.234E-06");
        assert_eq!(general(0.1 + 0.2), "0.3");
        assert_eq!(number_to_text(1.0 / 3.0), "0.333333333333333");
        assert_eq!(number_to_text(0.1 + 0.2), "0.3");
        assert_eq!(number_to_text(42.0), "42");
        assert_eq!(number_to_text(-1.5), "-1.5");
    }

    #[test]
    fn rounding() {
        assert_eq!(round_half_away(2.675, 2), 2.68);
        assert_eq!(round_half_away(2.5, 0), 3.0);
        assert_eq!(round_half_away(-2.5, 0), -3.0);
        assert_eq!(round_half_away(1234.0, -2), 1200.0);
        assert_eq!(round_half_away(1250.0, -2), 1300.0);
    }

    #[test]
    fn number_formats() {
        assert_eq!(f(1234.567, "0"), "1235");
        assert_eq!(f(1234.567, "0.00"), "1234.57");
        assert_eq!(f(1234.567, "#,##0"), "1,235");
        assert_eq!(f(1234.567, "#,##0.00"), "1,234.57");
        assert_eq!(f(1_234_567.891, "#,##0.00"), "1,234,567.89");
        assert_eq!(f(-1234.5, "#,##0.00"), "-1,234.50");
        assert_eq!(f(0.5, "0%"), "50%");
        assert_eq!(f(0.1234, "0.00%"), "12.34%");
        assert_eq!(f(12345.0, "0.00E+00"), "1.23E+04");
        assert_eq!(f(0.00012, "0.00E+00"), "1.20E-04");
        assert_eq!(f(0.0, "0.00E+00"), "0.00E+00");
        assert_eq!(f(1234.5, "$#,##0.00"), "$1,234.50");
        assert_eq!(f(-1234.5, "$#,##0.00"), "-$1,234.50");
        assert_eq!(f(1234.5, "#,##0.00 €"), "1,234.50 €");
        assert_eq!(f(1234.5, "[$€-407] #,##0.00"), "€ 1,234.50");
        assert_eq!(f(-5.0, "0.00;(0.00)"), "(5.00)");
        assert_eq!(f(-5.0, "0.00;[Red](0.00)"), "(5.00)");
        assert_eq!(f(0.0, "0.00;(0.00);\"-\""), "-");
        assert_eq!(f(3.0, "0 \"items\""), "3 items");
        assert_eq!(f(0.5, "#.##"), ".5");
        assert_eq!(f(1.5, "0.0#"), "1.5");
        assert_eq!(f(1.25, "0.0#"), "1.25");
        assert_eq!(f(42.0, "00000"), "00042");
        assert_eq!(f(5_551_234.0, "000-0000"), "555-1234");
        assert_eq!(f(1_500_000.0, "#,##0.0,,\"M\""), "1.5M");
        assert_eq!(f(-0.001, "0.00"), "0.00");
        assert_eq!(f(7.0, "General"), "7");
        assert_eq!(f(7.0, "@"), "7");
    }

    #[test]
    fn text_formats() {
        let t = Value::Text("abc".into());
        assert_eq!(format_value(&t, Some("0.00")), "abc");
        assert_eq!(format_value(&t, Some("\"Name: \"@")), "Name: abc");
        assert_eq!(format_value(&t, Some("0;-0;0;\"<\"@\">\"")), "<abc>");
        assert_eq!(format_value(&Value::Bool(true), Some("0.00")), "TRUE");
        assert_eq!(format_value(&Value::Empty, Some("0.00")), "");
        assert_eq!(format_value(&Value::Error(crate::ErrorKind::NA), None), "#N/A");
    }

    #[test]
    fn date_formats() {
        let serial = date_to_serial(2026, 10, 6) + time_fraction(14, 5, 9.0);
        assert_eq!(f(serial, "yyyy-mm-dd"), "2026-10-06");
        assert_eq!(f(serial, "d/m/yy"), "6/10/26");
        assert_eq!(f(serial, "m/d/yyyy"), "10/6/2026");
        assert_eq!(f(serial, "dddd d mmmm yyyy"), "Tuesday 6 October 2026");
        assert_eq!(f(serial, "ddd, mmm d"), "Tue, Oct 6");
        assert_eq!(f(serial, "mmmmm"), "O");
        assert_eq!(f(serial, "h:mm"), "14:05");
        assert_eq!(f(serial, "hh:mm:ss"), "14:05:09");
        assert_eq!(f(serial, "h:mm AM/PM"), "2:05 PM");
        assert_eq!(f(serial, "h:mm am/pm"), "2:05 pm");
        assert_eq!(f(serial, "yyyy-mm-dd h:mm"), "2026-10-06 14:05");
        assert_eq!(f(time_fraction(0, 30, 0.0), "h:mm AM/PM"), "12:30 AM");
        assert_eq!(f(1.5 + time_fraction(1, 2, 3.0), "[h]:mm:ss"), "37:02:03");
        assert_eq!(f(time_fraction(0, 90, 0.0), "[m]"), "90");
        assert_eq!(f(time_fraction(0, 0, 1.25), "mm:ss.00"), "00:01.25");
        assert_eq!(f(60.0, "yyyy-mm-dd"), "1900-02-29");
        assert_eq!(f(-1.0, "yyyy-mm-dd"), "#####");
        assert_eq!(f(time_fraction(23, 59, 59.9), "h:mm:ss"), "0:00:00");
        assert!(is_date_format("yyyy-mm-dd"));
        assert!(!is_date_format("#,##0.00"));
    }
}
