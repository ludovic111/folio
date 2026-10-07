//! Excel workbooks (`.xlsx`): every worksheet in and out with its cells, formats, sizes,
//! frozen panes, filter and charts.
//!
//! Reading parses the package's XML directly ([`read`]): cells with what Excel saved (inputs,
//! shared formulas expanded, cached results), `styles.xml` for formats, drawings for charts.
//! folio's engine then computes every formula, and where its result differs from Excel's the
//! import says so, naming the functions folio doesn't know. Writing goes through
//! `rust_xlsxwriter` ([`write`]), with native Excel charts.
//!
//! [`package`] holds what the other zip-of-XML formats (PPTX, ODS, ODP) share.

pub mod package;
mod read;
mod write;

use std::collections::BTreeMap;

use folio_calc::{Addr, Input, Value};
use folio_core::Document;

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "xlsx",
    name: "Excel workbook",
    extensions: &["xlsx", "xlsm"],
    kinds: &["sheet"],
    import: true,
    export: true,
    apps: &["Microsoft Excel", "Google Sheets (File › Download › .xlsx)", "Apple Numbers (File › Export To › Excel)", "LibreOffice Calc"],
    notes: "Every sheet with its values, formulas (computed again by folio, which says where its results differ from Excel's), number formats, fonts, fills, borders, alignment, column widths, row heights, frozen panes, the filter and charts (native Excel charts on the way out). Merged cells are split, and named ranges, conditional formatting, data validation, comments, pictures on sheets, pivot tables and macros are left out.",
};

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    read::import(bytes, title)
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    write::export(doc, pages)
}

/// Excel's built-in number formats (ids 0–49) as format codes.
pub(crate) fn builtin_num_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "$#,##0;($#,##0)",
        6 => "$#,##0;[Red]($#,##0)",
        7 => "$#,##0.00;($#,##0.00)",
        8 => "$#,##0.00;[Red]($#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        // The short date follows the reader's locale in Excel; folio shows ISO dates.
        14 => "yyyy-mm-dd",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "yyyy-mm-dd h:mm",
        37 => "#,##0;(#,##0)",
        38 => "#,##0;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        41 => "_(* #,##0_);_(* (#,##0);_(* \"-\"_);_(@_)",
        42 => "_($* #,##0_);_($* (#,##0);_($* \"-\"_);_(@_)",
        43 => "_(* #,##0.00_);_(* (#,##0.00);_(* \"-\"??_);_(@_)",
        44 => "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mm:ss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// A formula from a file without the prefixes Excel adds to newer functions.
pub(crate) fn clean_formula(f: &str) -> String {
    let f = f.trim_start_matches('=');
    let mut out = f.to_string();
    for p in ["_xlfn._xlws.", "_xlfn.", "_xlws.", "_xlpm."] {
        if out.contains(p) {
            out = out.replace(p, "");
        }
    }
    out.replace("\r\n", "\n")
}

/// A number as typed input, in full precision.
pub(crate) fn num_text(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if !n.is_finite() {
        return "0".into();
    }
    let s = format!("{n}");
    // Display never uses exponents; very large or tiny numbers read better with one.
    if s.len() > 24 { format!("{n:e}").replace('e', "E") } else { s }
}

/// Text as typed input: an apostrophe in front when it would otherwise read as a number,
/// a date, a boolean or a formula.
pub(crate) fn text_input(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    match folio_calc::parse_input(s).0 {
        Input::Text(t) if t == s => s.to_string(),
        _ => format!("'{s}"),
    }
}

/// A sheet name folio accepts (no `! ' [ ] : * ? / \`, at most 80 characters), unique among `taken`.
pub(crate) fn safe_sheet_name(name: &str, taken: &[String]) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '\'' => '’',
            '!' | '[' | ']' | ':' | '*' | '?' | '/' | '\\' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    s = s.trim().to_string();
    if s.is_empty() {
        s = "Sheet".into();
    }
    if s.chars().count() > 80 {
        s = s.chars().take(80).collect();
    }
    let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !is_taken(&s) {
        return s;
    }
    (2..).map(|i| format!("{s} {i}")).find(|n| !is_taken(n)).unwrap()
}

/// Function names a formula calls (uppercase, outside text literals).
pub(crate) fn called_functions(formula: &str) -> Vec<String> {
    let b: Vec<char> = formula.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '"' {
            i += 1;
            while i < b.len() && !(b[i] == '"' && b.get(i + 1) != Some(&'"')) {
                i += if b[i] == '"' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            i += 1;
            while i < b.len() && !(b[i] == '\'' && b.get(i + 1) != Some(&'\'')) {
                i += if b[i] == '\'' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == '.') {
                i += 1;
            }
            if b.get(i) == Some(&'(') {
                out.push(b[start..i].iter().collect::<String>().to_uppercase());
            }
            continue;
        }
        i += 1;
    }
    out
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
        (Value::Empty, Value::Number(n)) | (Value::Number(n), Value::Empty) => *n == 0.0,
        (Value::Empty, Value::Text(t)) | (Value::Text(t), Value::Empty) => t.is_empty(),
        _ => a == b,
    }
}

/// Compares folio's results with another app's saved ones and lists functions folio lacks.
#[derive(Default)]
pub(crate) struct FormulaCheck {
    unknown: BTreeMap<String, usize>,
    differ: usize,
    first: Option<String>,
}

impl FormulaCheck {
    pub fn compare(&mut self, sheet: &str, a: Addr, input: &str, theirs: &Value, ours: &Value) {
        let Some(f) = input.strip_prefix('=') else { return };
        let calls = called_functions(f);
        let known: std::collections::HashSet<&str> = folio_calc::builtin_functions().iter().map(|b| b.name).collect();
        let unknown: Vec<&String> = calls.iter().filter(|c| !known.contains(c.as_str())).collect();
        if !unknown.is_empty() {
            for u in unknown {
                *self.unknown.entry(u.clone()).or_default() += 1;
            }
            return;
        }
        if calls.iter().any(|c| matches!(c.as_str(), "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" | "INFO" | "CELL")) {
            return;
        }
        if !same_value(theirs, ours) {
            self.differ += 1;
            if self.first.is_none() {
                let show = |v: &Value| if v.is_empty() { "empty".to_string() } else { v.display() };
                self.first = Some(format!("{}!{} ({}) gives {} where it showed {}", folio_calc::quote_sheet_name(sheet), a.a1(), input, show(ours), show(theirs)));
            }
        }
    }

    pub fn warnings(&self, app: &str) -> Vec<String> {
        let mut out = vec![];
        if !self.unknown.is_empty() {
            let list: Vec<String> = self.unknown.iter().map(|(f, n)| if *n == 1 { f.clone() } else { format!("{f} ({n} cells)") }).collect();
            out.push(format!("Functions folio doesn't have yet (those cells show #NAME?): {}.", list.join(", ")));
        }
        if self.differ > 0 {
            out.push(format!(
                "{} {} differently in folio than in {app}'s saved results; for example {}.",
                self.differ,
                if self.differ == 1 { "formula computes" } else { "formulas compute" },
                self.first.as_deref().unwrap_or("")
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests;
