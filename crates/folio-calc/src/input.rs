//! What a person typed in a cell, read the way a spreadsheet reads it.

use crate::format::{date_to_serial, days_in_month, month_from_name, time_fraction};

/// The content of a cell as typed.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    /// A formula, without its leading `=`.
    Formula(String),
}

/// Parses typed text like a spreadsheet does, and suggests a number format when the text
/// carried one (a percent, a currency, a date, a time).
///
/// - `=…` is a formula (a lone `=` is text);
/// - `'…` is always text (the apostrophe is dropped);
/// - `TRUE` / `FALSE` in any case are booleans;
/// - numbers: `1234.5`, `1,234.5`, `-3`, `(3)` (negative), `+3`, `1e3` (suggests `0.00E+00`),
///   `12%` (0.12, suggests `0%`), `$12.50` / `€12.50` / `£3` (suggest a currency format),
///   `12,50 €` and `1 234,50 €` (decimal comma before a trailing euro sign, suggests `#,##0.00 €`);
/// - dates: `2026-10-06` and `2026/10/06` (`yyyy-mm-dd`), `10/6/2026` (month first unless the
///   first number is over 12: `m/d/yyyy` or `d/m/yyyy`), `6.10.2026` (`d.m.yyyy`), `6 Oct 2026`,
///   `Oct 6, 2026`, `6-Oct-2026`, `October 6 2026` (`d mmm yyyy`); two-digit years 00–29 are 20xx;
/// - times: `14:30` (`h:mm`), `14:30:15` (`h:mm:ss`), `2:30 PM` (`h:mm AM/PM`), `25:30` (`[h]:mm`);
/// - a date and a time separated by a space (or `T`) together (`yyyy-mm-dd h:mm`);
/// - anything else is text, kept exactly as typed.
pub fn parse_input(s: &str) -> (Input, Option<&'static str>) {
    if s.is_empty() {
        return (Input::Empty, None);
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return (Input::Text(rest.to_string()), None);
    }
    if let Some(rest) = s.strip_prefix('=')
        && !rest.trim().is_empty()
    {
        return (Input::Formula(rest.to_string()), None);
    }
    let t = s.trim();
    if t.eq_ignore_ascii_case("TRUE") {
        return (Input::Bool(true), None);
    }
    if t.eq_ignore_ascii_case("FALSE") {
        return (Input::Bool(false), None);
    }
    match parse_number_text(t) {
        Some((n, fmt)) => (Input::Number(n), fmt),
        None => (Input::Text(s.to_string()), None),
    }
}

/// Reads text as a number the way typing it would (numbers, percents, currencies, dates,
/// times). Formulas use this to turn text into numbers (`="3"+1`).
pub(crate) fn parse_number_text(t: &str) -> Option<(f64, Option<&'static str>)> {
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    parse_numeric(t).or_else(|| parse_datetime(t)).or_else(|| parse_date(t)).or_else(|| parse_time(t))
}

/// Just the number of [`parse_number_text`].
pub(crate) fn text_to_number(t: &str) -> Option<f64> {
    parse_number_text(t).map(|(n, _)| n)
}

#[derive(PartialEq, Clone, Copy)]
enum Currency {
    Dollar,
    Euro,
    Pound,
    Yen,
    EuroSuffix,
}

fn parse_numeric(t: &str) -> Option<(f64, Option<&'static str>)> {
    let mut s = t;
    let mut neg = false;
    if let Some(rest) = s.strip_prefix('-') {
        neg = true;
        s = rest.trim_start();
    } else if let Some(rest) = s.strip_prefix('+') {
        s = rest.trim_start();
    }
    if s.starts_with('(') && s.ends_with(')') && s.len() >= 2 {
        if neg {
            return None;
        }
        neg = true;
        s = s[1..s.len() - 1].trim();
    }
    let mut currency = None;
    for (sym, cur) in [("$", Currency::Dollar), ("€", Currency::Euro), ("£", Currency::Pound), ("¥", Currency::Yen)] {
        if let Some(rest) = s.strip_prefix(sym) {
            currency = Some(cur);
            s = rest.trim_start();
            if let Some(rest) = s.strip_prefix('-') {
                if neg {
                    return None;
                }
                neg = true;
                s = rest;
            }
            break;
        }
    }
    if currency.is_none()
        && let Some(rest) = s.strip_suffix('€')
    {
        currency = Some(Currency::EuroSuffix);
        s = rest.trim_end();
    }
    let mut percent = false;
    if let Some(rest) = s.strip_suffix('%') {
        if currency.is_some() {
            return None;
        }
        percent = true;
        s = rest.trim_end();
    }
    let core = if currency == Some(Currency::EuroSuffix) { parse_european(s)? } else { parse_us(s)? };
    let mut n = core.value;
    if neg {
        n = -n;
    }
    if percent {
        n /= 100.0;
    }
    let fmt = if percent {
        Some(if core.decimals > 0 { "0.00%" } else { "0%" })
    } else if let Some(cur) = currency {
        Some(match cur {
            Currency::Dollar => "$#,##0.00",
            Currency::Euro => "€#,##0.00",
            Currency::Pound => "£#,##0.00",
            Currency::Yen => "¥#,##0",
            Currency::EuroSuffix => "#,##0.00 €",
        })
    } else if core.exponent {
        Some("0.00E+00")
    } else if core.thousands {
        Some(if core.decimals > 0 { "#,##0.00" } else { "#,##0" })
    } else {
        None
    };
    n.is_finite().then_some((n, fmt))
}

struct Core {
    value: f64,
    thousands: bool,
    decimals: usize,
    exponent: bool,
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Checks `1,234,567`-style grouping: 1–3 digits, then groups of exactly 3.
fn valid_groups(parts: &[&str]) -> bool {
    parts.len() > 1
        && (1..=3).contains(&parts[0].len())
        && parts.iter().all(|p| all_digits(p))
        && parts[1..].iter().all(|p| p.len() == 3)
}

fn parse_us(s: &str) -> Option<Core> {
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (mantissa, None),
    };
    let thousands = int_part.contains(',');
    let int_digits = if thousands {
        let parts: Vec<&str> = int_part.split(',').collect();
        if !valid_groups(&parts) {
            return None;
        }
        parts.concat()
    } else {
        if !int_part.is_empty() && !all_digits(int_part) {
            return None;
        }
        int_part.to_string()
    };
    let frac = frac_part.unwrap_or("");
    if !frac.is_empty() && !all_digits(frac) {
        return None;
    }
    if int_digits.is_empty() && frac.is_empty() {
        return None;
    }
    let mut text = format!("{}.{}", if int_digits.is_empty() { "0" } else { &int_digits }, if frac.is_empty() { "0" } else { frac });
    if let Some(e) = exp {
        let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
        if !all_digits(digits) {
            return None;
        }
        text.push('e');
        text.push_str(e);
    }
    Some(Core { value: text.parse().ok()?, thousands, decimals: frac.len(), exponent: exp.is_some() })
}

/// `1 234,50`, `1.234,50`, `12,5` or `12.50` before a trailing euro sign.
fn parse_european(s: &str) -> Option<Core> {
    let cleaned: String = s.chars().filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}')).collect();
    let spaced = cleaned.len() != s.len();
    let (int_part, frac) = if let Some((a, b)) = cleaned.rsplit_once(',') {
        (a.to_string(), b.to_string())
    } else if let Some((a, b)) = cleaned.rsplit_once('.') {
        if b.len() == 3 && !spaced { (format!("{a}{b}"), String::new()) } else { (a.to_string(), b.to_string()) }
    } else {
        (cleaned.clone(), String::new())
    };
    let thousands = int_part.contains('.') || spaced;
    let int_digits = if int_part.contains('.') {
        let parts: Vec<&str> = int_part.split('.').collect();
        if !valid_groups(&parts) {
            return None;
        }
        parts.concat()
    } else {
        int_part
    };
    if !all_digits(&int_digits) || (!frac.is_empty() && !all_digits(&frac)) {
        return None;
    }
    let value = format!("{int_digits}.{}", if frac.is_empty() { "0" } else { &frac }).parse().ok()?;
    Some(Core { value, thousands, decimals: frac.len(), exponent: false })
}

fn year_from(s: &str) -> Option<i32> {
    if !all_digits(s) {
        return None;
    }
    let y: i32 = s.parse().ok()?;
    match s.len() {
        1 | 2 => Some(if y < 30 { 2000 + y } else { 1900 + y }),
        4 => Some(y),
        _ => None,
    }
}

fn small_number(s: &str) -> Option<u32> {
    if !all_digits(s) || s.len() > 2 {
        return None;
    }
    s.parse().ok()
}

fn valid_date(y: i32, m: u32, d: u32) -> Option<f64> {
    if !(1900..=9999).contains(&y) || !(1..=12).contains(&m) || d == 0 {
        return None;
    }
    if (y, m, d) != (1900, 2, 29) && d > days_in_month(y as i64, m) {
        return None;
    }
    Some(date_to_serial(y, m, d))
}

fn parse_date(s: &str) -> Option<(f64, Option<&'static str>)> {
    // Numeric dates with one kind of separator.
    for sep in ['-', '/', '.'] {
        let parts: Vec<&str> = s.split(sep).collect();
        if parts.len() == 3 && parts.iter().all(|p| all_digits(p)) {
            if parts[0].len() == 4 {
                let (y, m, d) = (year_from(parts[0])?, small_number(parts[1])?, small_number(parts[2])?);
                return valid_date(y, m, d).map(|n| (n, Some("yyyy-mm-dd")));
            }
            let (a, b, y) = (small_number(parts[0])?, small_number(parts[1])?, year_from(parts[2])?);
            return match sep {
                '/' if a > 12 => valid_date(y, b, a).map(|n| (n, Some("d/m/yyyy"))),
                '/' => valid_date(y, a, b).map(|n| (n, Some("m/d/yyyy"))),
                '.' => valid_date(y, b, a).map(|n| (n, Some("d.m.yyyy"))),
                _ => None,
            };
        }
    }
    // Dates with a month name: "6 Oct 2026", "Oct 6, 2026", "6-Oct-2026".
    let words: Vec<&str> = s.split(|c: char| c == ' ' || c == ',' || c == '-').filter(|w| !w.is_empty()).collect();
    if words.len() == 3 {
        let (d, m, y) = if let Some(m) = month_from_name(words[1]) {
            (small_number(words[0])?, m, words[2])
        } else if let Some(m) = month_from_name(words[0]) {
            (small_number(words[1])?, m, words[2])
        } else {
            return None;
        };
        let y = year_from(y)?;
        return valid_date(y, m, d).map(|n| (n, Some("d mmm yyyy")));
    }
    None
}

fn parse_time(s: &str) -> Option<(f64, Option<&'static str>)> {
    let lower = s.to_ascii_lowercase();
    let (body, ampm) = if let Some(rest) = lower.strip_suffix("am").or_else(|| lower.strip_suffix('a')) {
        (rest.trim_end().to_string(), Some(false))
    } else if let Some(rest) = lower.strip_suffix("pm").or_else(|| lower.strip_suffix('p')) {
        (rest.trim_end().to_string(), Some(true))
    } else {
        (lower.clone(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    if !all_digits(parts[0]) || parts[0].len() > 4 {
        return None;
    }
    let mut h: u32 = parts[0].parse().ok()?;
    let m = small_number(parts[1])?;
    if m >= 60 {
        return None;
    }
    let mut sec = 0.0;
    if let Some(sp) = parts.get(2) {
        let (whole, frac) = sp.split_once('.').unwrap_or((sp, ""));
        if !all_digits(whole) || whole.len() > 2 || (!frac.is_empty() && !all_digits(frac)) {
            return None;
        }
        sec = sp.parse().ok()?;
        if sec >= 60.0 {
            return None;
        }
    }
    let seconds = parts.len() == 3;
    let fmt = if let Some(pm) = ampm {
        if !(1..=12).contains(&h) {
            return None;
        }
        h %= 12;
        if pm {
            h += 12;
        }
        if seconds { "h:mm:ss AM/PM" } else { "h:mm AM/PM" }
    } else if h >= 24 {
        if seconds { "[h]:mm:ss" } else { "[h]:mm" }
    } else if seconds {
        "h:mm:ss"
    } else {
        "h:mm"
    };
    Some((time_fraction(h, m, sec), Some(fmt)))
}

fn parse_datetime(s: &str) -> Option<(f64, Option<&'static str>)> {
    for (i, c) in s.char_indices() {
        if c == ' ' || c == 'T' {
            let (left, right) = (s[..i].trim(), s[i + 1..].trim());
            if right.contains(':')
                && let (Some((d, _)), Some((t, tf))) = (parse_date(left), parse_time(right))
                && t < 1.0
            {
                let with_seconds = tf.is_some_and(|f| f.contains("ss"));
                return Some((d + t, Some(if with_seconds { "yyyy-mm-dd h:mm:ss" } else { "yyyy-mm-dd h:mm" })));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(s: &str) -> (f64, Option<&'static str>) {
        match parse_input(s) {
            (Input::Number(n), f) => (n, f),
            other => panic!("{s:?} gave {other:?}"),
        }
    }

    fn is_text(s: &str) -> bool {
        matches!(parse_input(s).0, Input::Text(_))
    }

    #[test]
    fn basics() {
        assert_eq!(parse_input(""), (Input::Empty, None));
        assert_eq!(parse_input("=SUM(A1)").0, Input::Formula("SUM(A1)".into()));
        assert_eq!(parse_input("=").0, Input::Text("=".into()));
        assert_eq!(parse_input("'123").0, Input::Text("123".into()));
        assert_eq!(parse_input("'=1").0, Input::Text("=1".into()));
        assert_eq!(parse_input("true").0, Input::Bool(true));
        assert_eq!(parse_input("FALSE").0, Input::Bool(false));
        assert_eq!(parse_input("hello").0, Input::Text("hello".into()));
        assert_eq!(parse_input("  hi ").0, Input::Text("  hi ".into()));
    }

    #[test]
    fn numbers() {
        assert_eq!(num("42"), (42.0, None));
        assert_eq!(num(" 42 "), (42.0, None));
        assert_eq!(num("-3"), (-3.0, None));
        assert_eq!(num("+3.5"), (3.5, None));
        assert_eq!(num(".5"), (0.5, None));
        assert_eq!(num("(12)"), (-12.0, None));
        assert_eq!(num("1,234.5"), (1234.5, Some("#,##0.00")));
        assert_eq!(num("1,234"), (1234.0, Some("#,##0")));
        assert_eq!(num("1e3"), (1000.0, Some("0.00E+00")));
        assert_eq!(num("1.5E-2"), (0.015, Some("0.00E+00")));
        assert_eq!(num("12%"), (0.12, Some("0%")));
        assert_eq!(num("12.5%"), (0.125, Some("0.00%")));
        assert_eq!(num("$12.50"), (12.5, Some("$#,##0.00")));
        assert_eq!(num("-$1,200"), (-1200.0, Some("$#,##0.00")));
        assert_eq!(num("€12.50"), (12.5, Some("€#,##0.00")));
        assert_eq!(num("12,50 €"), (12.5, Some("#,##0.00 €")));
        assert_eq!(num("1 234,50 €"), (1234.5, Some("#,##0.00 €")));
        assert_eq!(num("1.234,50 €"), (1234.5, Some("#,##0.00 €")));
        assert!(is_text("1,2"));
        assert!(is_text("12abc"));
        assert!(is_text("1.2.3.4"));
        assert!(is_text("$"));
        assert!(is_text("--5"));
        assert!(is_text("e5"));
    }

    #[test]
    fn dates_and_times() {
        let oct6 = date_to_serial(2026, 10, 6);
        assert_eq!(num("2026-10-06"), (oct6, Some("yyyy-mm-dd")));
        assert_eq!(num("2026/10/6"), (oct6, Some("yyyy-mm-dd")));
        assert_eq!(num("10/6/2026"), (oct6, Some("m/d/yyyy")));
        assert_eq!(num("25/12/2026"), (date_to_serial(2026, 12, 25), Some("d/m/yyyy")));
        assert_eq!(num("6.10.2026"), (oct6, Some("d.m.yyyy")));
        assert_eq!(num("6 Oct 2026"), (oct6, Some("d mmm yyyy")));
        assert_eq!(num("Oct 6, 2026"), (oct6, Some("d mmm yyyy")));
        assert_eq!(num("6-Oct-26"), (oct6, Some("d mmm yyyy")));
        assert_eq!(num("October 6 2026"), (oct6, Some("d mmm yyyy")));
        assert!(is_text("2026-02-30"));
        assert!(is_text("13/13/2026"));
        assert_eq!(num("14:30"), (time_fraction(14, 30, 0.0), Some("h:mm")));
        assert_eq!(num("14:30:15"), (time_fraction(14, 30, 15.0), Some("h:mm:ss")));
        assert_eq!(num("2:30 PM"), (time_fraction(14, 30, 0.0), Some("h:mm AM/PM")));
        assert_eq!(num("12:00 am"), (0.0, Some("h:mm AM/PM")));
        assert_eq!(num("25:30"), (time_fraction(25, 30, 0.0), Some("[h]:mm")));
        assert!(is_text("14:75"));
        assert_eq!(num("2026-10-06 14:30"), (oct6 + time_fraction(14, 30, 0.0), Some("yyyy-mm-dd h:mm")));
        assert_eq!(num("2026-10-06T08:00:30"), (oct6 + time_fraction(8, 0, 30.0), Some("yyyy-mm-dd h:mm:ss")));
    }
}
