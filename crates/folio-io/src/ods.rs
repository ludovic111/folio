//! OpenDocument spreadsheets (`.ods`, LibreOffice Calc): sheets in and out with values,
//! formulas, number formats, basic cell formatting, column widths, row heights, frozen panes,
//! the filter, and (in) charts.
//!
//! ODF formulas (`of:=SUM([.A1:.A3]; [Data.B2])`) are turned into folio's Excel syntax
//! (`=SUM(A1:A3, Data!B2)`) and back. Number formats go between ODF's number styles and Excel
//! format codes (the common parts: decimals, grouping, percent, currency, scientific, dates
//! and times, text).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use folio_calc::{Addr, Range, SheetRange, Value};
use folio_core::sheet::{Filter, SheetChart};
use folio_core::{Align, CellFormat, Chart, ChartKind, Document, Id, PageKind, Sheet};

use crate::xlsx::package::{El, Package, ZipOut, esc, hex_color};
use crate::xlsx::{FormulaCheck, num_text, safe_sheet_name, text_input};
use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "ods",
    name: "OpenDocument spreadsheet",
    extensions: &["ods"],
    kinds: &["sheet"],
    import: true,
    export: true,
    apps: &["LibreOffice Calc", "Collabora", "Google Sheets (File › Download › .ods)", "Microsoft Excel"],
    notes: "Every sheet with values, formulas (computed again by folio), number formats, bold, italic, underline, strike, colours, fills, borders, alignment, wrap, column widths, row heights, frozen panes and the filter. Charts come in; on the way out they are left out (export XLSX to keep them). Merged cells are split; named ranges, conditional formats, comments and pictures are left out.",
};

// ---------------------------------------------------------------------------------------------
// Lengths

/// An ODF length (`0.889in`, `2.258cm`, `12pt`) in pixels at 96 per inch.
pub(crate) fn length_px(s: &str) -> Option<f32> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic() || c == '%')?;
    let (n, unit) = s.split_at(split);
    let n: f32 = n.trim().parse().ok()?;
    Some(match unit {
        "in" => n * 96.0,
        "cm" => n * 96.0 / 2.54,
        "mm" => n * 96.0 / 25.4,
        "pt" => n * 96.0 / 72.0,
        "pc" => n * 16.0,
        "px" => n,
        _ => return None,
    })
}

/// An ODF length in points.
pub(crate) fn length_pt(s: &str) -> Option<f32> {
    length_px(s).map(|px| px * 0.75)
}

// ---------------------------------------------------------------------------------------------
// Formulas

/// An ODF reference (`Sheet1.A1`, `'My sheet'.$A$1:.$B$2`, `.A1`, `$Sheet1.A1:Sheet1.B2`) as
/// its sheet and its one or two cells, `$` anchors kept.
fn odf_ref(s: &str) -> Option<(Option<String>, String, Option<String>)> {
    let s = s.trim();
    // Split at the colon outside quotes.
    let mut quoted = false;
    let mut split = None;
    for (i, c) in s.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ':' if !quoted => {
                split = Some(i);
                break;
            }
            _ => {}
        }
    }
    let (a, b) = match split {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let part = |p: &str| -> Option<(Option<String>, String)> {
        let p = p.trim();
        let p = p.strip_prefix('$').filter(|r| r.starts_with('\'') || r.contains('.')).unwrap_or(p);
        let (sheet, cell) = if let Some(rest) = p.strip_prefix('\'') {
            let mut name = String::new();
            let mut end = None;
            let chars: Vec<(usize, char)> = rest.char_indices().collect();
            let mut k = 0;
            while k < chars.len() {
                let (i, c) = chars[k];
                if c == '\'' {
                    if chars.get(k + 1).map(|x| x.1) == Some('\'') {
                        name.push('\'');
                        k += 2;
                        continue;
                    }
                    end = Some(i);
                    break;
                }
                name.push(c);
                k += 1;
            }
            (Some(name), rest[end? + 1..].trim_start_matches('.').to_string())
        } else {
            match p.rfind('.') {
                Some(0) => (None, p[1..].to_string()),
                Some(i) => (Some(p[..i].to_string()), p[i + 1..].to_string()),
                None => (None, p.to_string()),
            }
        };
        Addr::parse(&cell.replace('$', ""))?;
        Some((sheet.filter(|s| !s.is_empty()), cell.to_ascii_uppercase()))
    };
    let (sheet, c1) = part(a)?;
    let c2 = match b {
        Some(b) => Some(part(b)?.1),
        None => None,
    };
    Some((sheet, c1, c2))
}

/// An ODF cell address or range as a reference (anchors dropped).
pub(crate) fn odf_range(s: &str) -> Option<SheetRange> {
    let (sheet, a, b) = odf_ref(s)?;
    let a = Addr::parse(&a.replace('$', ""))?;
    let range = match b {
        Some(b) => Range::new(a, Addr::parse(&b.replace('$', ""))?),
        None => Range::cell(a),
    };
    Some(SheetRange { sheet, range })
}

/// `of:=SUM([.A1:.A3];[Data.B2])` to `SUM(A1:A3,Data!B2)` (without the `=`).
pub(crate) fn from_odf_formula(f: &str) -> String {
    let f = f.trim();
    let f = f.split_once(":=").filter(|(ns, _)| !ns.contains(['(', '"', '['])).map(|(_, rest)| rest).unwrap_or(f);
    let f = f.strip_prefix('=').unwrap_or(f);
    let b: Vec<char> = f.chars().collect();
    let mut out = String::with_capacity(f.len());
    let mut i = 0;
    let mut braces = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            '"' => {
                out.push(c);
                i += 1;
                while i < b.len() {
                    out.push(b[i]);
                    if b[i] == '"' {
                        if b.get(i + 1) == Some(&'"') {
                            out.push('"');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            '[' => {
                let end = (i + 1..b.len()).find(|&j| b[j] == ']').unwrap_or(b.len());
                let inner: String = b[i + 1..end].iter().collect();
                match odf_ref(&inner) {
                    Some((sheet, a1, a2)) => {
                        if let Some(sh) = sheet {
                            let _ = write!(out, "{}!", folio_calc::quote_sheet_name(&sh));
                        }
                        out.push_str(&a1);
                        if let Some(a2) = a2 {
                            out.push(':');
                            out.push_str(&a2);
                        }
                    }
                    None => out.push_str(if inner.contains("#REF") { "#REF!" } else { &inner }),
                }
                i = end + 1;
            }
            '{' => {
                braces += 1;
                out.push(c);
                i += 1;
            }
            '}' => {
                braces -= 1;
                out.push(c);
                i += 1;
            }
            ';' => {
                out.push(',');
                i += 1;
            }
            '|' if braces > 0 => {
                out.push(';');
                i += 1;
            }
            '~' => {
                // ODF's union operator; Excel writes a comma inside parentheses.
                out.push(',');
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    for p in ["COM.MICROSOFT.", "_xlfn.", "ORG.OPENOFFICE."] {
        if out.contains(p) {
            out = out.replace(p, "");
        }
    }
    out
}

/// `SUM(A1:A3,Data!B2)` to `of:=SUM([.A1:.A3];[Data.B2])`.
pub(crate) fn to_odf_formula(f: &str) -> String {
    let f = f.strip_prefix('=').unwrap_or(f);
    let b: Vec<char> = f.chars().collect();
    let mut out = String::from("of:=");
    let mut i = 0;
    let mut braces = 0;
    let cell_at = |b: &[char], i: usize| -> Option<usize> {
        // `$?[A-Za-z]{1,3}$?[0-9]+` starting at i; returns the end.
        let mut j = i;
        if b.get(j) == Some(&'$') {
            j += 1;
        }
        let l0 = j;
        while j < b.len() && b[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j == l0 || j - l0 > 3 {
            return None;
        }
        if b.get(j) == Some(&'$') {
            j += 1;
        }
        let d0 = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == d0 {
            return None;
        }
        if j < b.len() && (b[j].is_alphanumeric() || b[j] == '_' || b[j] == '(' || b[j] == '.') {
            return None;
        }
        Some(j)
    };
    while i < b.len() {
        let c = b[i];
        let prev_ident = i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_' || b[i - 1] == '.');
        if c == '"' {
            out.push(c);
            i += 1;
            while i < b.len() {
                out.push(b[i]);
                if b[i] == '"' {
                    if b.get(i + 1) == Some(&'"') {
                        out.push('"');
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        // A sheet prefix: 'Name'! or Name!
        let mut sheet: Option<String> = None;
        let mut j = i;
        if !prev_ident && c == '\'' {
            let mut name = String::new();
            let mut k = i + 1;
            while k < b.len() {
                if b[k] == '\'' {
                    if b.get(k + 1) == Some(&'\'') {
                        name.push('\'');
                        k += 2;
                        continue;
                    }
                    break;
                }
                name.push(b[k]);
                k += 1;
            }
            if b.get(k + 1) == Some(&'!') {
                sheet = Some(name);
                j = k + 2;
            }
        } else if !prev_ident && (c.is_alphabetic() || c == '_') {
            let mut k = i;
            while k < b.len() && (b[k].is_alphanumeric() || b[k] == '_' || b[k] == '.') {
                k += 1;
            }
            if b.get(k) == Some(&'!') {
                sheet = Some(b[i..k].iter().collect());
                j = k + 1;
            }
        }
        if (sheet.is_some() || !prev_ident)
            && let Some(e1) = cell_at(&b, j)
        {
            let first: String = b[j..e1].iter().collect();
            let mut end = e1;
            let mut second = None;
            if b.get(e1) == Some(&':')
                && let Some(e2) = cell_at(&b, e1 + 1)
            {
                second = Some(b[e1 + 1..e2].iter().collect::<String>());
                end = e2;
            }
            let prefix = match &sheet {
                Some(s) => {
                    let plain = s.chars().all(|c| c.is_alphanumeric() || c == '_');
                    if plain { s.clone() } else { format!("'{}'", s.replace('\'', "''")) }
                }
                None => String::new(),
            };
            match second {
                Some(s2) => {
                    let _ = write!(out, "[{prefix}.{}:.{}]", first.to_uppercase(), s2.to_uppercase());
                }
                None => {
                    let _ = write!(out, "[{prefix}.{}]", first.to_uppercase());
                }
            }
            i = end;
            continue;
        }
        match c {
            '{' => {
                braces += 1;
                out.push(c);
            }
            '}' => {
                braces -= 1;
                out.push(c);
            }
            ',' => out.push(';'),
            ';' if braces > 0 => out.push('|'),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Number formats

/// An ODF number style as an Excel format code.
fn number_style_code(st: &El) -> Option<String> {
    let mut code = String::new();
    let number = |n: &El| -> String {
        let dec = n.attr_any("decimal-places").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        let min_int = n.attr_any("min-integer-digits").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
        let grouping = n.attr_any("grouping") == Some("true");
        let mut s = if grouping { "#,##0".to_string() } else { "0".repeat(min_int.max(1)) };
        if grouping && min_int == 0 {
            s = "#,###".into();
        }
        if dec > 0 {
            s.push('.');
            s.push_str(&"0".repeat(dec));
        }
        s
    };
    for e in st.elements() {
        match e.name.as_str() {
            "number" => code.push_str(&number(e)),
            "scientific-number" => {
                code.push_str(&number(e));
                let _ = write!(code, "E+{}", "0".repeat(e.attr_any("min-exponent-digits").and_then(|v| v.parse::<usize>().ok()).unwrap_or(2)));
            }
            "text" => {
                let t = e.text();
                if t == "%" || t == "-" || t == "/" || t == ":" || t == " " || t == "." || t == "," {
                    code.push_str(&t);
                } else if !t.is_empty() {
                    let _ = write!(code, "\"{}\"", t.replace('"', ""));
                }
            }
            "currency-symbol" => {
                let t = e.text();
                code.push_str(if t.chars().all(|c| "$€£¥".contains(c)) { t.clone() } else { format!("\"{t}\"") }.as_str());
            }
            "year" => code.push_str(if e.attr_any("style") == Some("long") { "yyyy" } else { "yy" }),
            "month" => code.push_str(match (e.attr_any("textual") == Some("true"), e.attr_any("style") == Some("long")) {
                (true, true) => "mmmm",
                (true, false) => "mmm",
                (false, true) => "mm",
                (false, false) => "m",
            }),
            "day" => code.push_str(if e.attr_any("style") == Some("long") { "dd" } else { "d" }),
            "day-of-week" => code.push_str(if e.attr_any("style") == Some("long") { "dddd" } else { "ddd" }),
            "hours" => code.push_str(if e.attr_any("style") == Some("long") { "hh" } else { "h" }),
            "minutes" => code.push_str(if e.attr_any("style") == Some("long") { "mm" } else { "m" }),
            "seconds" => {
                code.push_str(if e.attr_any("style") == Some("long") { "ss" } else { "s" });
                if let Some(d) = e.attr_any("decimal-places").and_then(|v| v.parse::<usize>().ok()).filter(|d| *d > 0) {
                    code.push('.');
                    code.push_str(&"0".repeat(d));
                }
            }
            "am-pm" => code.push_str("AM/PM"),
            "boolean" => return None,
            "text-content" => code.push('@'),
            "fraction" => code.push_str("# ?/?"),
            _ => {}
        }
    }
    if st.name == "time-style" && st.attr_any("truncate-on-overflow") == Some("false") && code.starts_with('h') {
        // Durations show hours past 24.
        let h = code.chars().take_while(|c| *c == 'h').count();
        code = format!("[{}]{}", "h".repeat(h), &code[h..]);
    }
    (!code.is_empty() && code != "0" || st.name != "number-style").then_some(code).filter(|c| !c.is_empty())
}

/// An Excel format code as an ODF number style named `name` (`None` for General).
fn code_to_style(name: &str, code: &str) -> Option<String> {
    let first = code.split(';').next().unwrap_or(code);
    if first.eq_ignore_ascii_case("general") || first.is_empty() {
        return None;
    }
    let lower = first.to_ascii_lowercase();
    // Strip colours, conditions, locale tags, padding and fill characters.
    let mut clean = String::new();
    let mut chars = first.chars().peekable();
    let mut literal = String::new();
    let mut parts: Vec<(String, String)> = vec![]; // (kind, text)
    while let Some(c) = chars.next() {
        match c {
            '[' => {
                let tag: String = chars.by_ref().take_while(|c| *c != ']').collect();
                if let Some(sym) = tag.strip_prefix('$') {
                    let sym = sym.split('-').next().unwrap_or("");
                    if !sym.is_empty() {
                        parts.push(("currency".into(), sym.to_string()));
                    }
                } else if tag.chars().all(|c| "hHmMsS".contains(c)) {
                    clean.push_str(&tag.to_ascii_lowercase());
                    parts.push(("elapsed".into(), tag.to_ascii_lowercase()));
                }
            }
            '"' => {
                let t: String = chars.by_ref().take_while(|c| *c != '"').collect();
                literal.push_str(&t);
                parts.push(("text".into(), t));
            }
            '\\' => {
                if let Some(n) = chars.next() {
                    parts.push(("text".into(), n.to_string()));
                }
            }
            '_' | '*' => {
                chars.next();
            }
            '$' | '€' | '£' | '¥' => parts.push(("currency".into(), c.to_string())),
            c => {
                clean.push(c);
                parts.push(("code".into(), c.to_string()));
            }
        }
    }
    let _ = literal;
    let is_date = folio_calc::is_date_format(first);
    let mut s = String::new();
    if is_date {
        let time_only = !lower.contains('y') && !lower.contains('d') && !(lower.contains("mmm"));
        let tag = if time_only { "time-style" } else { "date-style" };
        let truncate = if clean.starts_with("[h") || first.starts_with("[h") || first.starts_with("[H") { r#" number:truncate-on-overflow="false""# } else { "" };
        let _ = write!(s, r#"<number:{tag} style:name="{name}"{truncate}>"#);
        // Tokenise the code part into date/time pieces.
        let code_str: String = parts.iter().map(|(k, t)| if k == "code" { t.clone() } else if k == "elapsed" { t.clone() } else { "\u{1}".to_string() + t + "\u{2}" }).collect();
        let cs: Vec<char> = code_str.chars().collect();
        let mut i = 0;
        let mut last_was_hour = false;
        while i < cs.len() {
            let c = cs[i];
            if c == '\u{1}' {
                let end = (i..cs.len()).find(|&j| cs[j] == '\u{2}').unwrap_or(cs.len());
                let t: String = cs[i + 1..end].iter().collect();
                let _ = write!(s, "<number:text>{}</number:text>", esc(&t));
                i = end + 1;
                continue;
            }
            let run = cs[i..].iter().take_while(|x| x.eq_ignore_ascii_case(&c)).count();
            let lc = c.to_ascii_lowercase();
            let long = if run >= 2 { r#" number:style="long""# } else { "" };
            match lc {
                'y' => {
                    let _ = write!(s, "<number:year{}/>", if run >= 3 { r#" number:style="long""# } else { "" });
                }
                'm' => {
                    let rest: String = cs[i + run..].iter().collect::<String>().to_ascii_lowercase();
                    let minutes = last_was_hour || rest.trim_start_matches([':', ' ']).starts_with('s');
                    if minutes && run <= 2 {
                        let _ = write!(s, "<number:minutes{long}/>");
                    } else if run >= 4 {
                        s.push_str(r#"<number:month number:style="long" number:textual="true"/>"#);
                    } else if run == 3 {
                        s.push_str(r#"<number:month number:textual="true"/>"#);
                    } else {
                        let _ = write!(s, "<number:month{long}/>");
                    }
                }
                'd' => {
                    if run >= 4 {
                        s.push_str(r#"<number:day-of-week number:style="long"/>"#);
                    } else if run == 3 {
                        s.push_str("<number:day-of-week/>");
                    } else {
                        let _ = write!(s, "<number:day{long}/>");
                    }
                }
                'h' => {
                    let _ = write!(s, "<number:hours{long}/>");
                }
                's' => {
                    let _ = write!(s, "<number:seconds{long}/>");
                }
                'a' if cs[i..].iter().collect::<String>().to_ascii_lowercase().starts_with("am/pm") => {
                    s.push_str("<number:am-pm/>");
                    i += 5;
                    continue;
                }
                '[' | ']' => {}
                _ => {
                    let _ = write!(s, "<number:text>{}</number:text>", esc(&cs[i..i + run].iter().collect::<String>()));
                }
            }
            last_was_hour = lc == 'h' || (lc == ':' && last_was_hour);
            i += run;
        }
        let _ = write!(s, "</number:{tag}>");
        return Some(s);
    }
    if first.trim() == "@" {
        return Some(format!(r#"<number:text-style style:name="{name}"><number:text-content/></number:text-style>"#));
    }
    let digits: String = clean.chars().filter(|c| "0#?.,%Ee+-".contains(*c)).collect();
    let dec = digits.split_once('.').map(|(_, d)| d.chars().take_while(|c| *c == '0' || *c == '#' || *c == '?').filter(|c| *c == '0').count()).unwrap_or(0);
    let grouping = digits.split('.').next().unwrap_or("").contains(',');
    let int_part = digits.split('.').next().unwrap_or("");
    let min_int = int_part.chars().filter(|c| *c == '0').count();
    let number = format!(
        r#"<number:number number:decimal-places="{dec}" number:min-decimal-places="{dec}" number:min-integer-digits="{min_int}"{}/>"#,
        if grouping { r#" number:grouping="true""# } else { "" }
    );
    if clean.contains('%') {
        return Some(format!(r#"<number:percentage-style style:name="{name}">{number}<number:text>%</number:text></number:percentage-style>"#));
    }
    if lower.contains("e+") || lower.contains("e-") {
        return Some(format!(r#"<number:number-style style:name="{name}"><number:scientific-number number:decimal-places="{dec}" number:min-decimal-places="{dec}" number:min-integer-digits="1" number:min-exponent-digits="2"/></number:number-style>"#));
    }
    if let Some((_, sym)) = parts.iter().find(|(k, _)| k == "currency") {
        let sym_xml = format!("<number:currency-symbol>{}</number:currency-symbol>", esc(sym));
        let before = parts.iter().position(|(k, _)| k == "currency").unwrap_or(0) < parts.iter().position(|(k, t)| k == "code" && (t == "0" || t == "#")).unwrap_or(usize::MAX);
        return Some(if before {
            format!(r#"<number:currency-style style:name="{name}">{sym_xml}{number}</number:currency-style>"#)
        } else {
            format!(r#"<number:currency-style style:name="{name}">{number}<number:text> </number:text>{sym_xml}</number:currency-style>"#)
        });
    }
    let mut out = format!(r#"<number:number-style style:name="{name}">{number}"#);
    for (k, t) in parts.iter().skip_while(|(k, _)| k == "code") {
        if k == "text" {
            let _ = write!(out, "<number:text>{}</number:text>", esc(t));
        }
    }
    out.push_str("</number:number-style>");
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// Reading

#[derive(Clone, Default)]
struct CellStyle {
    format: CellFormat,
    data_style: Option<String>,
}

/// Cell styles by name, parents resolved, and number styles as codes.
struct Styles {
    cells: HashMap<String, CellStyle>,
    numbers: HashMap<String, String>,
    columns: HashMap<String, f32>,
    rows: HashMap<String, (f32, bool)>,
}

fn border_side(v: Option<&str>) -> bool {
    v.is_some_and(|v| !v.trim().is_empty() && v.trim() != "none" && !v.trim().starts_with("0pt") && !v.contains("hidden"))
}

impl Styles {
    fn load(roots: &[&El]) -> Styles {
        let mut raw: HashMap<String, &El> = HashMap::new();
        let mut numbers = HashMap::new();
        let mut columns = HashMap::new();
        let mut rows = HashMap::new();
        for root in roots {
            for holder in ["styles", "automatic-styles"] {
                let Some(h) = root.child(holder) else { continue };
                for st in h.elements() {
                    let Some(name) = st.attr_any("name") else { continue };
                    match st.name.as_str() {
                        "style" => match st.attr_any("family") {
                            Some("table-cell") => {
                                raw.insert(name.to_string(), st);
                            }
                            Some("table-column") => {
                                if let Some(w) = st.child("table-column-properties").and_then(|p| p.attr_any("column-width")).and_then(length_px) {
                                    columns.insert(name.to_string(), w.round());
                                }
                            }
                            Some("table-row") => {
                                if let Some(p) = st.child("table-row-properties")
                                    && let Some(h) = p.attr_any("row-height").and_then(length_px)
                                {
                                    rows.insert(name.to_string(), (h.round(), p.attr_any("use-optimal-row-height") == Some("true")));
                                }
                            }
                            _ => {}
                        },
                        "number-style" | "percentage-style" | "currency-style" | "date-style" | "time-style" | "text-style" => {
                            if let Some(code) = number_style_code(st) {
                                numbers.insert(name.to_string(), code);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut cells = HashMap::new();
        for name in raw.keys() {
            // Parents first, the child's properties over them.
            let mut chain = vec![];
            let mut at = Some(name.clone());
            while let Some(n) = at {
                let Some(st) = raw.get(&n) else { break };
                if chain.len() > 8 {
                    break;
                }
                chain.push(*st);
                at = st.attr_any("parent-style-name").map(str::to_string);
            }
            let mut cs = CellStyle::default();
            for st in chain.iter().rev() {
                if let Some(d) = st.attr_any("data-style-name") {
                    cs.data_style = Some(d.to_string());
                }
                let f = &mut cs.format;
                if let Some(t) = st.child("text-properties") {
                    if let Some(w) = t.attr_any("font-weight") {
                        f.bold = w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600);
                    }
                    if let Some(s) = t.attr_any("font-style") {
                        f.italic = s == "italic" || s == "oblique";
                    }
                    if let Some(u) = t.attr("style:text-underline-style") {
                        f.underline = u != "none";
                    }
                    if let Some(u) = t.attr("style:text-line-through-style") {
                        f.strike = u != "none";
                    }
                    if let Some(c) = t.attr("fo:color").and_then(hex_color) {
                        f.color = (c != "#000000").then_some(c);
                    }
                    if let Some(s) = t.attr("fo:font-size").and_then(length_pt) {
                        f.size = ((s - 10.0).abs() > 0.05).then_some(s);
                    }
                }
                if let Some(c) = st.child("table-cell-properties") {
                    if let Some(bg) = c.attr("fo:background-color") {
                        f.fill = hex_color(bg);
                    }
                    if let Some(w) = c.attr("fo:wrap-option") {
                        f.wrap = w == "wrap";
                    }
                    let all = c.attr("fo:border");
                    let mut b = String::new();
                    for (side, key) in [('t', "fo:border-top"), ('r', "fo:border-right"), ('b', "fo:border-bottom"), ('l', "fo:border-left")] {
                        if border_side(c.attr(key).or(all)) {
                            b.push(side);
                        }
                    }
                    if all.is_some() || ["fo:border-top", "fo:border-right", "fo:border-bottom", "fo:border-left"].iter().any(|k| c.attr(k).is_some()) {
                        f.border = b;
                    }
                }
                if let Some(p) = st.child("paragraph-properties")
                    && let Some(a) = p.attr_any("text-align")
                {
                    f.align = match a {
                        "start" | "left" => Some(Align::Left),
                        "center" => Some(Align::Center),
                        "end" | "right" => Some(Align::Right),
                        "justify" => Some(Align::Justify),
                        _ => None,
                    };
                }
            }
            cells.insert(name.clone(), cs);
        }
        Styles { cells, numbers, columns, rows }
    }

    fn format(&self, name: Option<&str>) -> CellFormat {
        let Some(cs) = name.and_then(|n| self.cells.get(n)) else { return CellFormat::default() };
        let mut f = cs.format.clone();
        f.number = cs.data_style.as_ref().and_then(|d| self.numbers.get(d)).cloned().filter(|c| !c.eq_ignore_ascii_case("general"));
        f
    }
}

/// The text of a cell's paragraphs (`text:s` spaces, tabs and line breaks kept).
fn cell_text(cell: &El) -> String {
    fn walk(e: &El, out: &mut String) {
        for k in &e.kids {
            match k {
                crate::xlsx::package::Node::Text(t) => out.push_str(t),
                crate::xlsx::package::Node::El(c) => match c.name.as_str() {
                    "s" => out.push_str(&" ".repeat(c.attr_any("c").and_then(|v| v.parse().ok()).unwrap_or(1))),
                    "tab" => out.push('\t'),
                    "line-break" => out.push('\n'),
                    "annotation" | "note" => {}
                    _ => walk(c, out),
                },
            }
        }
    }
    let mut paras = vec![];
    for p in cell.children("p") {
        let mut s = String::new();
        walk(p, &mut s);
        paras.push(s);
    }
    paras.join("\n")
}

/// `PT12H30M15S` to a fraction of a day.
fn duration_days(s: &str) -> Option<f64> {
    let s = s.trim();
    let neg = s.starts_with('-');
    let t = s.trim_start_matches('-').strip_prefix('P')?;
    let (days, time) = t.split_once('T').unwrap_or((t, ""));
    let mut total = days.trim_end_matches('D').parse::<f64>().unwrap_or(0.0);
    let mut num = String::new();
    for c in time.chars() {
        match c {
            'H' => {
                total += num.parse::<f64>().ok()? / 24.0;
                num.clear();
            }
            'M' => {
                total += num.parse::<f64>().ok()? / 1440.0;
                num.clear();
            }
            'S' => {
                total += num.parse::<f64>().ok()? / 86_400.0;
                num.clear();
            }
            c => num.push(c),
        }
    }
    Some(if neg { -total } else { total })
}

fn date_serial(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    let mut p = date.split('-');
    let (y, m, d) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    let mut serial = folio_calc::date_to_serial(y, m, d);
    if !time.is_empty() {
        let t: Vec<f64> = time.split(':').filter_map(|v| v.parse().ok()).collect();
        serial += folio_calc::time_fraction(*t.first().unwrap_or(&0.0) as u32, *t.get(1).unwrap_or(&0.0) as u32, *t.get(2).unwrap_or(&0.0));
    }
    Some(serial)
}

#[derive(Default)]
struct Counts {
    merged: usize,
    charts_out: usize,
    images: usize,
    comments: usize,
}

struct RawSheet {
    name: String,
    sheet: Sheet,
    cached: Vec<(Addr, Value)>,
    frames: Vec<([f32; 4], String)>,
}

/// Rows and their cells inside a table, header rows and row groups included.
fn table_rows<'a>(t: &'a El, out: &mut Vec<&'a El>) {
    for e in t.elements() {
        match e.name.as_str() {
            "table-row" => out.push(e),
            "table-header-rows" | "table-rows" | "table-row-group" => table_rows(e, out),
            _ => {}
        }
    }
}

fn table_columns<'a>(t: &'a El, out: &mut Vec<&'a El>) {
    for e in t.elements() {
        match e.name.as_str() {
            "table-column" => out.push(e),
            "table-header-columns" | "table-columns" | "table-column-group" => table_columns(e, out),
            _ => {}
        }
    }
}

fn read_table(t: &El, styles: &Styles, counts: &mut Counts) -> RawSheet {
    let mut sheet = Sheet::default();
    let mut cached = vec![];
    let mut frames = vec![];
    // Columns: widths and default cell styles.
    let mut cols = vec![];
    table_columns(t, &mut cols);
    let mut col_style: Vec<Option<String>> = vec![];
    let mut col_px: Vec<f32> = vec![];
    for c in cols {
        let n = c.attr_any("number-columns-repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(1024);
        let px = c.attr_any("style-name").and_then(|s| styles.columns.get(s)).copied().unwrap_or(folio_core::sheet::COL_W);
        let px = if c.attr_any("visibility") == Some("collapse") { 0.0 } else { px };
        for _ in 0..n {
            col_px.push(px);
            col_style.push(c.attr_any("default-cell-style-name").map(str::to_string));
        }
    }
    let mut rows = vec![];
    table_rows(t, &mut rows);
    let mut r: u32 = 0;
    let mut max_col = 0u32;
    for row in rows {
        let repeat = row.attr_any("number-rows-repeated").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
        let has_content = row.elements().any(|c| c.attr_any("value-type").is_some() || c.attr_any("formula").is_some() || c.child("p").is_some() || c.find("frame").is_some());
        if let Some((h, optimal)) = row.attr_any("style-name").and_then(|s| styles.rows.get(s)) {
            let h = if row.attr_any("visibility") == Some("collapse") { 0.0 } else { *h };
            if (h - folio_core::sheet::ROW_H).abs() > 0.5 && (!optimal || h > folio_core::sheet::ROW_H) && repeat < 1000 {
                for k in 0..repeat {
                    sheet.rows.insert(r + k, h);
                }
            }
        }
        if !has_content && repeat > 1 {
            r = r.saturating_add(repeat);
            continue;
        }
        for rr in 0..repeat.min(if has_content { 10_000 } else { 1 }) {
            let mut c: u32 = 0;
            for cell in row.elements() {
                if cell.name != "table-cell" && cell.name != "covered-table-cell" {
                    continue;
                }
                let n = cell.attr_any("number-columns-repeated").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
                let has = cell.attr_any("value-type").is_some() || cell.attr_any("formula").is_some() || cell.child("p").is_some();
                let style = cell.attr_any("style-name").map(str::to_string).or_else(|| col_style.get(c as usize).cloned().flatten());
                let format = styles.format(style.as_deref());
                if cell.name == "table-cell" && cell.attr_any("number-columns-spanned").or(cell.attr_any("number-rows-spanned")).is_some_and(|v| v != "1") {
                    counts.merged += 1;
                }
                if cell.find("annotation").is_some() {
                    counts.comments += 1;
                }
                for frame in cell.children("frame") {
                    let x0: f32 = (0..c).map(|i| col_px.get(i as usize).copied().unwrap_or(folio_core::sheet::COL_W)).sum();
                    let y0: f32 = (0..r + rr).map(|i| sheet.row_height(i)).sum();
                    let g = |k: &str| frame.attr_any(k).and_then(length_px).unwrap_or(0.0);
                    match frame.child("object").and_then(|o| o.attr_any("href")) {
                        Some(href) => frames.push(([x0 + g("x"), y0 + g("y"), g("width"), g("height")], href.trim_start_matches("./").to_string())),
                        None => counts.images += 1,
                    }
                }
                if has && cell.name == "table-cell" && n <= 1024 {
                    for k in 0..n {
                        let a = Addr::new(r + rr, c + k);
                        let vt = cell.attr_any("value-type").unwrap_or("string");
                        let num = |key: &str| cell.attr_any(key).and_then(|v| v.trim().parse::<f64>().ok());
                        let value = match vt {
                            "float" | "percentage" | "currency" => num("value").map(Value::Number),
                            "date" => cell.attr_any("date-value").and_then(date_serial).map(Value::Number),
                            "time" => cell.attr_any("time-value").and_then(duration_days).map(Value::Number),
                            "boolean" => cell.attr_any("boolean-value").map(|b| Value::Bool(b == "true" || b == "1")),
                            "error" => Some(Value::Error(folio_calc::ErrorKind::parse(&cell_text(cell)).unwrap_or(folio_calc::ErrorKind::Value))),
                            _ => Some(Value::Text(cell.attr_any("string-value").map(str::to_string).unwrap_or_else(|| cell_text(cell)))),
                        }
                        .unwrap_or(Value::Empty);
                        let mut format = format.clone();
                        if format.number.is_none() {
                            format.number = match vt {
                                "percentage" => Some("0%".into()),
                                "date" => Some("yyyy-mm-dd".into()),
                                "time" => Some("h:mm:ss".into()),
                                _ => None,
                            };
                        }
                        let input = match cell.attr_any("formula") {
                            Some(f) => {
                                let input = format!("={}", from_odf_formula(f));
                                if !matches!(value, Value::Error(folio_calc::ErrorKind::Value)) || vt != "error" {
                                    cached.push((a, value.clone()));
                                }
                                input
                            }
                            None => match &value {
                                Value::Number(n) => num_text(*n),
                                Value::Text(s) => text_input(s),
                                Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
                                Value::Error(e) => format!("={}", e.code()),
                                Value::Empty => String::new(),
                            },
                        };
                        if input.is_empty() && format.is_default() {
                            continue;
                        }
                        max_col = max_col.max(a.col);
                        sheet.cells.insert(a, folio_core::Cell { input, value, format });
                    }
                } else if cell.name == "table-cell" && !format.is_default() && n <= 64 && style.is_some() && cell.attr_any("style-name").is_some() && style.as_deref() != Some("Default") {
                    for k in 0..n {
                        sheet.cells.insert(Addr::new(r + rr, c + k), folio_core::Cell { format: format.clone(), ..Default::default() });
                    }
                }
                c = c.saturating_add(n);
            }
        }
        r = r.saturating_add(repeat);
    }
    for (i, px) in col_px.iter().enumerate().take((max_col as usize + 1).max(1).min(col_px.len())) {
        if (*px - folio_core::sheet::COL_W).abs() > 0.5 {
            sheet.cols.insert(i as u32, *px);
        }
    }
    RawSheet { name: t.attr_any("name").unwrap_or("Sheet").to_string(), sheet, cached, frames }
}

/// A chart object's kind, title and data range.
fn read_chart_object(x: &El, names: &HashMap<String, String>) -> Result<Chart, String> {
    // office:chart wraps chart:chart; the XML helper stores local names.
    let holder = x.find("chart").ok_or("it has no chart")?;
    let chart = if holder.child("plot-area").is_some() { holder } else { holder.child("chart").ok_or("it has no chart")? };
    let class = chart.attr_any("class").unwrap_or("chart:bar");
    let plot = chart.child("plot-area").ok_or("it has no plot area")?;
    let vertical = plot.attr_any("vertical") == Some("true");
    let kind = match class.trim_start_matches("chart:") {
        "bar" => {
            if vertical {
                ChartKind::Bar
            } else {
                ChartKind::Column
            }
        }
        "line" | "stock" | "radar" | "filled-radar" => ChartKind::Line,
        "area" => ChartKind::Area,
        "circle" | "ring" => ChartKind::Pie,
        "scatter" | "bubble" => ChartKind::Scatter,
        other => return Err(format!("{other} charts aren't supported")),
    };
    let mut ranges = vec![];
    let mut add = |s: Option<&str>| {
        for part in s.unwrap_or("").split_whitespace() {
            if let Some(r) = odf_range(part) {
                ranges.push(r);
            }
        }
    };
    add(plot.attr_any("cell-range-address"));
    for e in plot.all("categories") {
        add(e.attr_any("cell-range-address"));
    }
    let mut series_in_rows = None;
    for s in plot.children("series") {
        if series_in_rows.is_none() {
            series_in_rows = s.attr_any("values-cell-range-address").and_then(odf_range).map(|r| r.range.rows() == 1 && r.range.cols() > 1);
        }
        add(s.attr_any("values-cell-range-address"));
        add(s.attr_any("label-cell-address"));
    }
    let sheet = ranges.first().and_then(|r| r.sheet.clone()).ok_or("its data isn't in cells")?;
    let on_sheet: Vec<&SheetRange> = ranges.iter().filter(|r| r.sheet.as_deref().is_none_or(|s| s == sheet)).collect();
    let bbox = on_sheet.iter().skip(1).fold(on_sheet[0].range, |b, r| {
        Range::new(Addr::new(b.start.row.min(r.range.start.row), b.start.col.min(r.range.start.col)), Addr::new(b.end.row.max(r.range.end.row), b.end.col.max(r.range.end.col)))
    });
    let sheet = names.get(&sheet.to_lowercase()).cloned().unwrap_or(sheet);
    let mut c = Chart::new(kind, SheetRange { sheet: Some(sheet), range: bbox }.to_string());
    c.series_in_rows = series_in_rows.unwrap_or(false);
    c.legend = chart.child("legend").is_some();
    c.title = chart.child("title").map(cell_text).unwrap_or_default();
    // Stacking lives in the plot area's style.
    if let Some(sn) = plot.attr_any("style-name")
        && let Some(st) = x.find("automatic-styles").and_then(|a| a.elements().find(|s| s.attr_any("name") == Some(sn)))
    {
        c.stacked = st.child("chart-properties").is_some_and(|p| p.attr_any("stacked") == Some("true") || p.attr_any("percentage") == Some("true")) && matches!(kind, ChartKind::Column | ChartKind::Bar | ChartKind::Area);
    }
    Ok(c)
}

/// Frozen panes per sheet from `settings.xml`: (columns, rows).
fn frozen(settings: Option<&El>) -> HashMap<String, (u32, u32)> {
    let mut out = HashMap::new();
    let Some(s) = settings else { return out };
    for map in s.all("config-item-map-named") {
        if map.attr_any("name") != Some("Tables") {
            continue;
        }
        for entry in map.children("config-item-map-entry") {
            let Some(name) = entry.attr_any("name") else { continue };
            let item = |k: &str| entry.children("config-item").find(|i| i.attr_any("name") == Some(k)).map(|i| i.text());
            let num = |k: &str| item(k).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
            let cols = if num("HorizontalSplitMode") == 2 { num("HorizontalSplitPosition") } else { 0 };
            let rows = if num("VerticalSplitMode") == 2 { num("VerticalSplitPosition") } else { 0 };
            if cols > 0 || rows > 0 {
                out.insert(name.to_string(), (cols, rows));
            }
        }
    }
    out
}

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    let mut pkg = Package::open(bytes).map_err(|_| "This file isn't an OpenDocument spreadsheet (it isn't a zip package).".to_string())?;
    if let Some(m) = pkg.read("mimetype")
        && !String::from_utf8_lossy(&m).contains("spreadsheet")
    {
        return Err(format!("This OpenDocument file isn't a spreadsheet ({}).", String::from_utf8_lossy(&m).trim()));
    }
    let content = pkg.xml("content.xml").ok_or("This file isn't an OpenDocument spreadsheet (no content.xml).")?;
    let styles_x = pkg.xml("styles.xml");
    let mut roots = vec![&content];
    if let Some(s) = &styles_x {
        roots.push(s);
    }
    let styles = Styles::load(&roots);
    let spreadsheet = content.path(&["body", "spreadsheet"]).ok_or("This file has no spreadsheet body.")?;
    let mut counts = Counts::default();
    let raw: Vec<RawSheet> = spreadsheet.children("table").map(|t| read_table(t, &styles, &mut counts)).collect();
    if raw.is_empty() {
        return Err("This spreadsheet has no sheets.".into());
    }
    let freeze = frozen(pkg.xml("settings.xml").as_ref());
    let mut doc = Document::empty(title);
    let mut warnings = vec![];
    let mut names: HashMap<String, String> = HashMap::new();
    let mut taken = vec![];
    for r in &raw {
        let safe = safe_sheet_name(&r.name, &taken);
        taken.push(safe.clone());
        names.insert(r.name.to_lowercase(), safe);
    }
    let renamed: Vec<(String, String)> = raw.iter().filter_map(|r| names.get(&r.name.to_lowercase()).filter(|n| **n != r.name).map(|n| (r.name.clone(), n.clone()))).collect();
    if !renamed.is_empty() {
        warnings.push(format!("Renamed sheets folio's formulas can't name as they were: {}.", renamed.iter().map(|(a, b)| format!("\"{a}\" → \"{b}\"")).collect::<Vec<_>>().join(", ")));
    }
    let mut checks = vec![];
    let mut frames = vec![];
    for r in raw {
        let name = names[&r.name.to_lowercase()].clone();
        let i = doc.add_page(PageKind::Sheet, Some(&name), None).map_err(|e| e.0)?;
        let mut sheet = r.sheet;
        if let Some((c, rw)) = freeze.get(&r.name) {
            sheet.freeze_cols = *c;
            sheet.freeze_rows = *rw;
        }
        if !renamed.is_empty() {
            let formulas: Vec<Addr> = sheet.cells.iter().filter(|(_, c)| c.is_formula()).map(|(a, _)| *a).collect();
            for a in formulas {
                if let Some(c) = sheet.cells.get_mut(&a) {
                    let mut f = c.input[1..].to_string();
                    for (old, new) in &renamed {
                        f = folio_calc::rename_sheet(&f, old, new);
                    }
                    c.input = format!("={f}");
                }
            }
        }
        *doc.page_mut(i).sheet_mut().unwrap() = sheet;
        checks.push((i, r.cached));
        frames.push((i, r.frames));
    }
    // The filter (an anonymous database range per sheet).
    if let Some(dbs) = spreadsheet.child("database-ranges") {
        for db in dbs.children("database-range") {
            if db.attr_any("display-filter-buttons") != Some("true") {
                continue;
            }
            let Some(sr) = db.attr_any("target-range-address").and_then(odf_range) else { continue };
            let Some(sheet_name) = sr.sheet.as_ref().and_then(|s| names.get(&s.to_lowercase())) else { continue };
            if let Some(i) = doc.page_index(sheet_name)
                && let Some(s) = doc.page_mut(i).sheet_mut()
            {
                s.filter = Some(Filter { range: sr.range.a1(), rules: BTreeMap::new() });
            }
        }
    }
    // Charts.
    for (i, list) in frames {
        for (rect, href) in list {
            let Some(x) = pkg.xml(&format!("{href}/content.xml")) else { continue };
            match read_chart_object(&x, &names) {
                Ok(chart) => doc.page_mut(i).sheet_mut().unwrap().charts.push(SheetChart { id: Id::new(), chart, x: rect[0], y: rect[1], w: rect[2].max(40.0), h: rect[3].max(30.0) }),
                Err(why) => {
                    counts.charts_out += 1;
                    warnings.push(format!("A chart on \"{}\" was left out: {why}.", doc.pages[i].name));
                }
            }
        }
    }
    folio_core::recalc::Calc::new().sync(&mut doc);
    let mut check = FormulaCheck::default();
    for (i, cached) in &checks {
        let s = doc.pages[*i].sheet().unwrap();
        for (a, v) in cached {
            if let Some(c) = s.cell(*a) {
                check.compare(&doc.pages[*i].name, *a, &c.input, v, &c.value);
            }
        }
    }
    warnings.extend(check.warnings("LibreOffice"));
    if counts.merged > 0 {
        warnings.push(format!("{} merged ranges were split: folio has no merged cells, each top-left cell keeps the content.", counts.merged));
    }
    if counts.images > 0 {
        warnings.push(format!("{} pictures or drawings on sheets were left out.", counts.images));
    }
    if counts.comments > 0 {
        warnings.push(format!("{} cell comments were left out.", counts.comments));
    }
    if let Some(n) = spreadsheet.child("named-expressions") {
        let list: Vec<String> = n.elements().filter_map(|e| e.attr_any("name")).map(|n| format!("\"{n}\"")).collect();
        if !list.is_empty() {
            warnings.push(format!("Named ranges aren't supported yet ({}): formulas that use them show #NAME?.", list.join(", ")));
        }
    }
    Ok(Imported { doc, warnings, format: "ods" })
}

// ---------------------------------------------------------------------------------------------
// Writing

const CONTENT_NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0" office:version="1.3""#;

fn px_in(px: f32) -> String {
    format!("{:.4}in", px / 96.0)
}

/// Cell formats as automatic styles, numbered as they are met.
#[derive(Default)]
struct StyleSet {
    cells: Vec<(String, String)>,
    numbers: Vec<(String, String)>,
    xml: String,
}

impl StyleSet {
    fn cell(&mut self, f: &CellFormat) -> String {
        let key = serde_json::to_string(f).unwrap_or_default();
        if let Some((_, n)) = self.cells.iter().find(|(k, _)| *k == key) {
            return n.clone();
        }
        let name = format!("ce{}", self.cells.len() + 1);
        let data = f.number.as_deref().and_then(|code| {
            if let Some((_, n)) = self.numbers.iter().find(|(c, _)| c == code) {
                return Some(n.clone());
            }
            let n = format!("N{}", self.numbers.len() + 100);
            let xml = code_to_style(&n, code)?;
            self.xml.push_str(&xml);
            self.numbers.push((code.to_string(), n.clone()));
            Some(n)
        });
        let _ = write!(self.xml, r#"<style:style style:name="{name}" style:family="table-cell" style:parent-style-name="Default"{}>"#, data.map(|d| format!(r#" style:data-style-name="{d}""#)).unwrap_or_default());
        let mut cellp = String::new();
        if let Some(fill) = &f.fill {
            let _ = write!(cellp, r#" fo:background-color="{fill}""#);
        }
        if f.wrap {
            cellp.push_str(r#" fo:wrap-option="wrap""#);
        }
        for (c, side) in [('t', "top"), ('r', "right"), ('b', "bottom"), ('l', "left")] {
            if f.border.contains(c) {
                let _ = write!(cellp, r#" fo:border-{side}="0.75pt solid #000000""#);
            }
        }
        if !cellp.is_empty() {
            let _ = write!(self.xml, "<style:table-cell-properties{cellp}/>");
        }
        if let Some(a) = f.align {
            let _ = write!(
                self.xml,
                r#"<style:paragraph-properties fo:text-align="{}"/>"#,
                match a {
                    Align::Left => "start",
                    Align::Center => "center",
                    Align::Right => "end",
                    Align::Justify => "justify",
                }
            );
        }
        let mut tp = String::new();
        if f.bold {
            tp.push_str(r#" fo:font-weight="bold""#);
        }
        if f.italic {
            tp.push_str(r#" fo:font-style="italic""#);
        }
        if f.underline {
            tp.push_str(r#" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color""#);
        }
        if f.strike {
            tp.push_str(r#" style:text-line-through-style="solid""#);
        }
        if let Some(c) = &f.color {
            let _ = write!(tp, r#" fo:color="{c}""#);
        }
        if let Some(s) = f.size {
            let _ = write!(tp, r#" fo:font-size="{s}pt""#);
        }
        if !tp.is_empty() {
            let _ = write!(self.xml, "<style:text-properties{tp}/>");
        }
        self.xml.push_str("</style:style>");
        self.cells.push((key, name.clone()));
        name
    }
}

fn cell_xml(c: &folio_core::Cell, style: Option<&str>, renames: &[(String, String)], repeat: u32) -> String {
    let mut attrs = String::new();
    if let Some(s) = style {
        let _ = write!(attrs, r#" table:style-name="{s}""#);
    }
    if repeat > 1 {
        let _ = write!(attrs, r#" table:number-columns-repeated="{repeat}""#);
    }
    if c.input.is_empty() {
        return format!("<table:table-cell{attrs}/>");
    }
    let is_date = c.format.number.as_deref().is_some_and(folio_calc::is_date_format);
    if let Some(f) = c.input.strip_prefix('=') {
        let mut f = f.to_string();
        for (old, new) in renames {
            f = folio_calc::rename_sheet(&f, old, new);
        }
        let _ = write!(attrs, r#" table:formula="{}""#, esc(&to_odf_formula(&f)));
    }
    let shown = esc(&c.display());
    let value = match &c.value {
        Value::Number(n) if is_date && *n >= 1.0 => {
            let (y, m, d) = folio_calc::serial_to_date(*n);
            let secs = ((n.fract()) * 86_400.0).round() as u32;
            if secs > 0 {
                format!(r#" office:value-type="date" office:date-value="{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}""#, secs / 3600, secs / 60 % 60, secs % 60)
            } else {
                format!(r#" office:value-type="date" office:date-value="{y:04}-{m:02}-{d:02}""#)
            }
        }
        Value::Number(n) if c.format.number.as_deref().is_some_and(|f| f.contains('%')) => format!(r#" office:value-type="percentage" office:value="{}""#, num_text(*n)),
        Value::Number(n) => format!(r#" office:value-type="float" office:value="{}""#, num_text(*n)),
        Value::Bool(b) => format!(r#" office:value-type="boolean" office:boolean-value="{b}""#),
        Value::Text(t) => format!(r#" office:value-type="string" office:string-value="{}""#, esc(t)),
        Value::Error(_) => r#" office:value-type="string" office:string-value="""#.to_string(),
        Value::Empty => String::new(),
    };
    let paras: String = shown.split('\n').map(|l| format!("<text:p>{l}</text:p>")).collect();
    format!("<table:table-cell{attrs}{value}>{paras}</table:table-cell>")
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let (keep, mut warnings) = crate::pages_of_kind(doc, pages, PageKind::Sheet, "ODS");
    if keep.is_empty() {
        return Err("Nothing to write: an OpenDocument spreadsheet holds sheets, and none of the chosen pages is a sheet.".into());
    }
    let mut styles = StyleSet::default();
    let mut col_styles: Vec<(u32, String)> = vec![];
    let mut row_styles: Vec<(u32, String)> = vec![];
    let mut auto = String::new();
    let mut body = String::new();
    let mut settings = String::new();
    let mut filters = String::new();
    let mut charts = 0;
    let renames: Vec<(String, String)> = doc.sheets().into_iter().map(|(_, n)| (n.to_string(), n.to_string())).collect();
    for &i in &keep {
        let page = &doc.pages[i];
        let sheet = page.sheet().unwrap();
        charts += sheet.charts.len();
        let used = sheet.used_range();
        let last_col = used.map(|r| r.end.col).unwrap_or(0).max(sheet.cols.keys().last().copied().unwrap_or(0));
        let last_row = used.map(|r| r.end.row).unwrap_or(0).max(sheet.rows.keys().last().copied().unwrap_or(0));
        let _ = write!(body, r#"<table:table table:name="{}">"#, esc(&page.name));
        // Columns, runs of equal widths together.
        let mut col = 0;
        while col <= last_col {
            let w = sheet.col_width(col).round() as u32;
            let run = (col..=last_col).take_while(|c| sheet.col_width(*c).round() as u32 == w).count() as u32;
            let name = match col_styles.iter().find(|(px, _)| *px == w) {
                Some((_, n)) => n.clone(),
                None => {
                    let n = format!("co{}", col_styles.len() + 1);
                    let _ = write!(auto, r#"<style:style style:name="{n}" style:family="table-column"><style:table-column-properties fo:break-before="auto" style:column-width="{}"/></style:style>"#, px_in(w.max(1) as f32));
                    col_styles.push((w, n.clone()));
                    n
                }
            };
            let vis = if w == 0 { r#" table:visibility="collapse""# } else { "" };
            let _ = write!(body, r#"<table:table-column table:style-name="{name}"{}{vis} table:default-cell-style-name="Default"/>"#, if run > 1 { format!(r#" table:number-columns-repeated="{run}""#) } else { String::new() });
            col += run;
        }
        let mut row = 0;
        while row <= last_row {
            let h = sheet.row_height(row).round() as u32;
            let rname = match row_styles.iter().find(|(px, _)| *px == h) {
                Some((_, n)) => n.clone(),
                None => {
                    let n = format!("ro{}", row_styles.len() + 1);
                    let _ = write!(
                        auto,
                        r#"<style:style style:name="{n}" style:family="table-row"><style:table-row-properties style:row-height="{}" fo:break-before="auto" style:use-optimal-row-height="false"/></style:style>"#,
                        px_in(h.max(1) as f32)
                    );
                    row_styles.push((h, n.clone()));
                    n
                }
            };
            let cells: Vec<(Addr, &folio_core::Cell)> = sheet.cells.range(Addr::new(row, 0)..=Addr::new(row, folio_calc::MAX_COLS - 1)).map(|(a, c)| (*a, c)).collect();
            if cells.is_empty() {
                // Empty rows of the same height together.
                let run = (row..=last_row).take_while(|r| sheet.row_height(*r).round() as u32 == h && sheet.cells.range(Addr::new(*r, 0)..=Addr::new(*r, folio_calc::MAX_COLS - 1)).next().is_none()).count() as u32;
                let vis = if h == 0 { r#" table:visibility="collapse""# } else { "" };
                let _ = write!(body, r#"<table:table-row table:style-name="{rname}"{}{vis}><table:table-cell/></table:table-row>"#, if run > 1 { format!(r#" table:number-rows-repeated="{run}""#) } else { String::new() });
                row += run;
                continue;
            }
            let vis = if h == 0 { r#" table:visibility="collapse""# } else { "" };
            let _ = write!(body, r#"<table:table-row table:style-name="{rname}"{vis}>"#);
            let mut c = 0u32;
            for (a, cell) in cells {
                if a.col > c {
                    let _ = write!(body, "{}", cell_xml(&folio_core::Cell::default(), None, &renames, a.col - c));
                }
                let style = (!cell.format.is_default()).then(|| styles.cell(&cell.format));
                body.push_str(&cell_xml(cell, style.as_deref(), &renames, 1));
                c = a.col + 1;
            }
            body.push_str("</table:table-row>");
            row += 1;
        }
        body.push_str("</table:table>");
        if sheet.freeze_rows > 0 || sheet.freeze_cols > 0 || !sheet.gridlines {
            let (fc, fr) = (sheet.freeze_cols, sheet.freeze_rows);
            let mode = |n: u32| if n > 0 { 2 } else { 0 };
            let _ = write!(
                settings,
                r#"<config:config-item-map-entry config:name="{}"><config:config-item config:name="HorizontalSplitMode" config:type="short">{}</config:config-item><config:config-item config:name="VerticalSplitMode" config:type="short">{}</config:config-item><config:config-item config:name="HorizontalSplitPosition" config:type="int">{fc}</config:config-item><config:config-item config:name="VerticalSplitPosition" config:type="int">{fr}</config:config-item><config:config-item config:name="ActiveSplitRange" config:type="short">2</config:config-item><config:config-item config:name="PositionLeft" config:type="int">0</config:config-item><config:config-item config:name="PositionRight" config:type="int">{fc}</config:config-item><config:config-item config:name="PositionTop" config:type="int">0</config:config-item><config:config-item config:name="PositionBottom" config:type="int">{fr}</config:config-item><config:config-item config:name="ShowGrid" config:type="boolean">{}</config:config-item></config:config-item-map-entry>"#,
                esc(&page.name),
                mode(fc),
                mode(fr),
                sheet.gridlines
            );
        }
        if let Some(f) = &sheet.filter
            && let Some(r) = Range::parse(&f.range)
        {
            let q = format!("'{}'", page.name.replace('\'', "''"));
            let _ = write!(
                filters,
                r#"<table:database-range table:name="__Anonymous_Sheet_DB__{i}" table:target-range-address="{}" table:display-filter-buttons="true"/>"#,
                esc(&format!("{q}.{}:{q}.{}", r.start.a1(), r.end.a1()))
            );
        }
    }
    if charts > 0 {
        warnings.push(format!("{charts} charts were left out (OpenDocument charts aren't written yet; export XLSX to keep them)."));
    }
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {CONTENT_NS}><office:font-face-decls/><office:automatic-styles>{auto}{}</office:automatic-styles><office:body><office:spreadsheet>{body}{}</office:spreadsheet></office:body></office:document-content>"#,
        styles.xml,
        if filters.is_empty() { String::new() } else { format!("<table:database-ranges>{filters}</table:database-ranges>") }
    );
    let styles_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {CONTENT_NS}><office:styles><style:default-style style:family="table-cell"><style:text-properties fo:font-size="10pt" style:font-name="Liberation Sans"/></style:default-style><style:style style:name="Default" style:family="table-cell"/></office:styles></office:document-styles>"#
    );
    let meta = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta {CONTENT_NS}><office:meta><meta:generator>folio</meta:generator><dc:title>{}</dc:title></office:meta></office:document-meta>"#,
        esc(&doc.title)
    );
    let settings_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-settings {CONTENT_NS}><office:settings><config:config-item-set config:name="ooo:view-settings"><config:config-item-map-indexed config:name="Views"><config:config-item-map-entry><config:config-item config:name="ViewId" config:type="string">view1</config:config-item><config:config-item-map-named config:name="Tables">{settings}</config:config-item-map-named></config:config-item-map-entry></config:config-item-map-indexed></config:config-item-set></office:settings></office:document-settings>"#
    );
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="settings.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    let mut zip = ZipOut::new();
    zip.stored("mimetype", "application/vnd.oasis.opendocument.spreadsheet")?;
    zip.add("META-INF/manifest.xml", manifest)?;
    zip.add("content.xml", content)?;
    zip.add("styles.xml", styles_xml)?;
    zip.add("meta.xml", meta)?;
    zip.add("settings.xml", settings_xml)?;
    Ok((zip.finish()?, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> Addr {
        Addr::parse(s).unwrap()
    }

    #[test]
    fn formulas_both_ways() {
        assert_eq!(from_odf_formula("of:=SUM([.A1:.A3];[Data.B2])"), "SUM(A1:A3,Data!B2)");
        assert_eq!(from_odf_formula("of:=[.C3]/[.$C$9]"), "C3/$C$9");
        assert_eq!(from_odf_formula("of:=VLOOKUP(\"Home\";['Rates & Notes'.A2:.B5];2;0)"), "VLOOKUP(\"Home\",'Rates & Notes'!A2:B5,2,0)");
        assert_eq!(from_odf_formula("of:=[$Budget.$A$1]&\"a;b\""), "Budget!$A$1&\"a;b\"");
        assert_eq!(from_odf_formula("of:=SUM({1;2|3;4})"), "SUM({1,2;3,4})");
        assert_eq!(to_odf_formula("SUM(A1:A3,Data!B2)"), "of:=SUM([.A1:.A3];[Data.B2])");
        assert_eq!(to_odf_formula("C3/$C$9&\"x,A1\""), "of:=[.C3]/[.$C$9]&\"x,A1\"");
        assert_eq!(to_odf_formula("'Bob''s list'!A1+LOG10(B2)"), "of:=['Bob''s list'.A1]+LOG10([.B2])");
        assert_eq!(to_odf_formula("SUM({1,2;3,4})"), "of:=SUM({1;2|3;4})");
        for f in ["SUM(A1:A3,Data!B2)", "IF(A1>2,\"yes\",\"no\")", "'My sheet'!$B$2*2"] {
            assert_eq!(from_odf_formula(&to_odf_formula(f)), f);
        }
    }

    #[test]
    fn number_formats_both_ways() {
        for code in ["0.00", "#,##0", "#,##0.00", "0%", "0.0%", "yyyy-mm-dd", "d mmm yyyy", "h:mm", "h:mm:ss", "@", "0.00E+00"] {
            let xml = code_to_style("N1", code).unwrap();
            let el = crate::xlsx::package::parse_xml(format!("<r xmlns:number=\"n\" xmlns:style=\"s\">{xml}</r>").as_bytes()).unwrap();
            let st = el.elements().next().unwrap();
            assert_eq!(number_style_code(st).as_deref(), Some(code), "{xml}");
        }
    }

    const BUDGET: &[u8] = include_bytes!("../tests/fixtures/budget.ods");

    #[test]
    fn libreoffice_budget() {
        let im = import(BUDGET, "budget").unwrap();
        let names: Vec<&str> = im.doc.pages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Budget", "Rates & Notes", "Limits", "Bob’s list", "Trend"]);
        let w = im.warnings.join("\n");
        assert!(!w.contains("differently"), "{w}");
        let b = im.doc.pages[0].sheet().unwrap();
        assert!((b.value(a("C9")).as_number().unwrap() - 1847.65).abs() < 1e-9);
        assert_eq!(b.input(a("C12")), "=VLOOKUP(\"Home\",'Rates & Notes'!A2:B5,2,FALSE())");
        assert_eq!(b.value(a("C12")), Value::Number(0.4));
        assert_eq!(b.display(a("D3")), "2026-01-01");
        assert_eq!(b.display(a("E3")), "64.9%");
        assert_eq!(b.input(a("C14")), "'00123");
        assert_eq!(b.value(a("C16")), Value::Text("Bob's Ann".into()));
        let head = &b.cell(a("A2")).unwrap().format;
        assert!(head.bold);
        assert_eq!(head.fill.as_deref(), Some("#1f4e78"));
        assert!(head.border.contains('b'));
        assert_eq!(b.filter.as_ref().unwrap().range, "A2:E7");
        assert_eq!(b.charts.len(), 1);
        assert_eq!(b.charts[0].chart.source, "Budget!A2:C7");
        assert_eq!(b.charts[0].chart.title, "Spending");
        assert!(w.contains("merged"), "{w}");
    }

    #[test]
    fn round_trip() {
        let im = crate::xlsx::import(include_bytes!("../tests/fixtures/budget-lo.xlsx"), "budget").unwrap();
        let mut doc = im.doc;
        {
            let s = doc.page_mut(0).sheet_mut().unwrap();
            s.freeze_rows = 2;
            s.freeze_cols = 1;
        }
        let all: Vec<usize> = (0..doc.pages.len()).collect();
        let (bytes, _) = export(&doc, &all).unwrap();
        let back = import(&bytes, "budget").unwrap();
        let w = back.warnings.join("\n");
        assert!(!w.contains("differently"), "{w}");
        for (p0, p1) in doc.pages.iter().zip(&back.doc.pages) {
            assert_eq!(p0.name, p1.name);
            let (s0, s1) = (p0.sheet().unwrap(), p1.sheet().unwrap());
            for (addr, c) in s0.cells.iter() {
                if c.input.is_empty() && c.format.is_default() {
                    continue;
                }
                let c1 = s1.cell(*addr).unwrap_or_else(|| panic!("{}!{} is missing", p0.name, addr.a1()));
                assert_eq!(c.value, c1.value, "{}!{}", p0.name, addr.a1());
                assert_eq!(c.format, c1.format, "{}!{}", p0.name, addr.a1());
                if c.is_formula() {
                    assert_eq!(c.input, c1.input, "{}!{}", p0.name, addr.a1());
                }
            }
            for (col, w) in &s0.cols {
                assert!((s1.col_width(*col) - w).abs() <= 1.0, "{} col {col}", p0.name);
            }
        }
        let b = back.doc.pages[0].sheet().unwrap();
        assert_eq!((b.freeze_rows, b.freeze_cols), (2, 1));
        assert_eq!(b.filter.as_ref().map(|f| f.range.as_str()), Some("A2:E7"));
    }
}
