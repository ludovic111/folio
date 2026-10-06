//! Cell addresses and ranges in A1 notation.
//!
//! Rows and columns are 0-based here (`Addr { row: 0, col: 0 }` is `A1`), 1-based only in text.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Rows in a sheet (like Excel).
pub const MAX_ROWS: u32 = 1_048_576;
/// Columns in a sheet (`A` to `XFD`, like Excel).
pub const MAX_COLS: u32 = 16_384;

/// A cell position, 0-based. Ordered row first, then column (reading order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Addr {
    pub row: u32,
    pub col: u32,
}

impl Addr {
    pub fn new(row: u32, col: u32) -> Self {
        Addr { row, col }
    }

    /// Reads `B12`, `$B$12`, `b12` (any case, `$` anchors ignored).
    pub fn parse(a1: &str) -> Option<Addr> {
        let a1 = a1.trim();
        match scan_body(a1.as_bytes(), 0) {
            Some((RefBody::Cell(p), end)) if end == a1.len() => Some(Addr::new(p.row, p.col)),
            _ => None,
        }
    }

    /// The address in A1 notation, such as `B12`.
    pub fn a1(&self) -> String {
        format!("{}{}", col_name(self.col), self.row + 1)
    }
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.a1())
    }
}

/// The letters of a 0-based column: 0 is `A`, 25 is `Z`, 26 is `AA`.
pub fn col_name(col: u32) -> String {
    let mut n = col as u64 + 1;
    let mut out = Vec::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.push(b'A' + rem);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The 0-based index of column letters (`A` is 0, `aa` is 26). `None` past `XFD` or for non-letters.
pub fn col_index(name: &str) -> Option<u32> {
    if name.is_empty() || name.len() > 3 {
        return None;
    }
    let mut n: u32 = 0;
    for b in name.bytes() {
        if !b.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + (b.to_ascii_uppercase() - b'A') as u32 + 1;
    }
    if n == 0 || n > MAX_COLS { None } else { Some(n - 1) }
}

/// A rectangle of cells. `start` is always the top-left corner and `end` the bottom-right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Range {
    pub start: Addr,
    pub end: Addr,
}

impl Range {
    /// A range from two corners in any order.
    pub fn new(a: Addr, b: Addr) -> Self {
        Range {
            start: Addr::new(a.row.min(b.row), a.col.min(b.col)),
            end: Addr::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }

    /// One cell.
    pub fn cell(a: Addr) -> Self {
        Range { start: a, end: a }
    }

    /// Reads `A1:C3`, `A1`, whole columns `A:C` and whole rows `2:5` (`$` anchors ignored).
    pub fn parse(s: &str) -> Option<Range> {
        let s = s.trim();
        match scan_body(s.as_bytes(), 0) {
            Some((body, end)) if end == s.len() => Some(body.range()),
            _ => None,
        }
    }

    /// The range in A1 notation: `A1`, `A1:C3`, `A:C` for whole columns, `2:5` for whole rows.
    pub fn a1(&self) -> String {
        if self.is_whole_cols() {
            format!("{}:{}", col_name(self.start.col), col_name(self.end.col))
        } else if self.is_whole_rows() {
            format!("{}:{}", self.start.row + 1, self.end.row + 1)
        } else if self.start == self.end {
            self.start.a1()
        } else {
            format!("{}:{}", self.start.a1(), self.end.a1())
        }
    }

    /// True when the range spans every row (`A:C`).
    pub fn is_whole_cols(&self) -> bool {
        self.start.row == 0 && self.end.row == MAX_ROWS - 1
    }

    /// True when the range spans every column (`2:5`).
    pub fn is_whole_rows(&self) -> bool {
        self.start.col == 0 && self.end.col == MAX_COLS - 1
    }

    pub fn contains(&self, a: Addr) -> bool {
        a.row >= self.start.row && a.row <= self.end.row && a.col >= self.start.col && a.col <= self.end.col
    }

    /// Number of rows.
    pub fn rows(&self) -> u32 {
        self.end.row - self.start.row + 1
    }

    /// Number of columns.
    pub fn cols(&self) -> u32 {
        self.end.col - self.start.col + 1
    }

    /// Number of cells.
    pub fn area(&self) -> u64 {
        self.rows() as u64 * self.cols() as u64
    }

    /// The overlap of two ranges, if any.
    pub fn intersect(&self, other: &Range) -> Option<Range> {
        let start = Addr::new(self.start.row.max(other.start.row), self.start.col.max(other.start.col));
        let end = Addr::new(self.end.row.min(other.end.row), self.end.col.min(other.end.col));
        (start.row <= end.row && start.col <= end.col).then_some(Range { start, end })
    }

    /// Every cell, row by row.
    pub fn iter(&self) -> impl Iterator<Item = Addr> + use<> {
        let (r0, r1, c0, c1) = (self.start.row, self.end.row, self.start.col, self.end.col);
        (r0..=r1).flat_map(move |row| (c0..=c1).map(move |col| Addr::new(row, col)))
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.a1())
    }
}

/// A range with an optional sheet name, as written in a formula: `'My sheet'!A1:B3`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SheetRange {
    pub sheet: Option<String>,
    pub range: Range,
}

impl SheetRange {
    /// Reads `'My sheet'!A1:B3`, `Sales!B2` or `A1:B2`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let tokens = crate::lexer::lex(s);
        match tokens.as_slice() {
            [token] => match &token.tok {
                crate::lexer::Tok::Ref(r) => Some(SheetRange { sheet: r.sheet.clone(), range: r.body.range() }),
                _ => None,
            },
            _ => None,
        }
    }
}

impl fmt::Display for SheetRange {
    /// Writes the reference back, quoting the sheet name when it needs quotes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(sheet) = &self.sheet {
            write!(f, "{}!", quote_sheet_name(sheet))?;
        }
        f.write_str(&self.range.a1())
    }
}

/// A sheet name as it must appear in a formula: bare when it is a plain word, otherwise in
/// single quotes with inner quotes doubled (`'My sheet'`, `'Bob''s'`, `'2024'`, `'A1'`).
pub fn quote_sheet_name(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        && scan_part(name.as_bytes(), 0).is_none_or(|(p, end)| end != name.len() || p.row.is_none())
        && !name.eq_ignore_ascii_case("TRUE")
        && !name.eq_ignore_ascii_case("FALSE")
        && !looks_like_r1c1(name);
    if plain { name.to_string() } else { format!("'{}'", name.replace('\'', "''")) }
}

fn looks_like_r1c1(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let Some(rest) = upper.strip_prefix('R') else { return false };
    let digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    digits.is_empty() || digits.strip_prefix('C').is_some_and(|d| d.chars().all(|c| c.is_ascii_digit()))
}

/// One corner of a reference as written, with its `$` anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Part {
    pub row: u32,
    pub col: u32,
    pub row_abs: bool,
    pub col_abs: bool,
}

/// The body of a reference (without the sheet), keeping anchors so it can be rewritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RefBody {
    Cell(Part),
    Area(Part, Part),
    /// Whole columns; only `col`/`col_abs` of the parts matter.
    Cols(Part, Part),
    /// Whole rows; only `row`/`row_abs` of the parts matter.
    Rows(Part, Part),
}

impl RefBody {
    pub fn range(&self) -> Range {
        match *self {
            RefBody::Cell(p) => Range::cell(Addr::new(p.row, p.col)),
            RefBody::Area(a, b) => Range::new(Addr::new(a.row, a.col), Addr::new(b.row, b.col)),
            RefBody::Cols(a, b) => Range::new(Addr::new(0, a.col), Addr::new(MAX_ROWS - 1, b.col)),
            RefBody::Rows(a, b) => Range::new(Addr::new(a.row, 0), Addr::new(b.row, MAX_COLS - 1)),
        }
    }

    /// Writes the body back in A1 notation with its anchors.
    pub fn render(&self) -> String {
        fn col(p: &Part) -> String {
            format!("{}{}", if p.col_abs { "$" } else { "" }, col_name(p.col))
        }
        fn row(p: &Part) -> String {
            format!("{}{}", if p.row_abs { "$" } else { "" }, p.row + 1)
        }
        match self {
            RefBody::Cell(p) => format!("{}{}", col(p), row(p)),
            RefBody::Area(a, b) => format!("{}{}:{}{}", col(a), row(a), col(b), row(b)),
            RefBody::Cols(a, b) => format!("{}:{}", col(a), col(b)),
            RefBody::Rows(a, b) => format!("{}:{}", row(a), row(b)),
        }
    }
}

/// What [`scan_part`] found: column letters and/or row digits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scanned {
    pub col: Option<(u32, bool)>,
    pub row: Option<(u32, bool)>,
}

/// Scans `$?LETTERS$?DIGITS` (either half may be missing) at `i`.
pub(crate) fn scan_part(s: &[u8], i: usize) -> Option<(Scanned, usize)> {
    let mut j = i;
    let first_dollar = s.get(j) == Some(&b'$');
    if first_dollar {
        j += 1;
    }
    let letters_start = j;
    while j < s.len() && s[j].is_ascii_alphabetic() {
        j += 1;
    }
    let letters = &s[letters_start..j];
    let mut col = None;
    let mut row_abs = false;
    if !letters.is_empty() {
        if letters.len() > 3 {
            return None;
        }
        let name = std::str::from_utf8(letters).ok()?;
        col = Some((col_index(name)?, first_dollar));
        if s.get(j) == Some(&b'$') {
            row_abs = true;
            j += 1;
        }
    } else {
        row_abs = first_dollar;
    }
    let digits_start = j;
    while j < s.len() && s[j].is_ascii_digit() {
        j += 1;
    }
    let digits = &s[digits_start..j];
    let mut row = None;
    if !digits.is_empty() {
        if digits.len() > 7 || digits[0] == b'0' {
            return None;
        }
        let n: u32 = std::str::from_utf8(digits).ok()?.parse().ok()?;
        if n == 0 || n > MAX_ROWS {
            return None;
        }
        row = Some((n - 1, row_abs));
    } else if row_abs {
        // A `$` with nothing after it.
        return None;
    }
    if col.is_none() && row.is_none() {
        return None;
    }
    Some((Scanned { col, row }, j))
}

fn is_ref_end(s: &[u8], j: usize) -> bool {
    match s.get(j) {
        None => true,
        Some(&b) => !(b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'(' || b == b'!' || b == b'$' || b >= 0x80),
    }
}

/// Scans a reference body (`A1`, `$A$1:B2`, `A:C`, `3:5`) at `i`; returns it and where it ends.
pub(crate) fn scan_body(s: &[u8], i: usize) -> Option<(RefBody, usize)> {
    let (p1, j) = scan_part(s, i)?;
    let part = |sc: &Scanned| Part {
        row: sc.row.map_or(0, |r| r.0),
        col: sc.col.map_or(0, |c| c.0),
        row_abs: sc.row.is_some_and(|r| r.1),
        col_abs: sc.col.is_some_and(|c| c.1),
    };
    let second = if s.get(j) == Some(&b':') { scan_part(s, j + 1) } else { None };
    let (body, end) = match (p1.col, p1.row) {
        (Some(_), Some(_)) => match second {
            Some((p2, k)) if p2.col.is_some() && p2.row.is_some() && is_ref_end(s, k) => {
                let (a, b) = order(part(&p1), part(&p2));
                (RefBody::Area(a, b), k)
            }
            _ => (RefBody::Cell(part(&p1)), j),
        },
        (Some(_), None) => match second {
            Some((p2, k)) if p2.col.is_some() && p2.row.is_none() => {
                let (a, b) = order(part(&p1), part(&p2));
                (RefBody::Cols(a, b), k)
            }
            _ => return None,
        },
        (None, Some(_)) => match second {
            Some((p2, k)) if p2.col.is_none() && p2.row.is_some() => {
                let (a, b) = order(part(&p1), part(&p2));
                (RefBody::Rows(a, b), k)
            }
            _ => return None,
        },
        (None, None) => return None,
    };
    is_ref_end(s, end).then_some((body, end))
}

/// Puts two corners in top-left / bottom-right order, keeping each axis's anchors.
pub(crate) fn order(a: Part, b: Part) -> (Part, Part) {
    let (r0, r1) = if a.row <= b.row { ((a.row, a.row_abs), (b.row, b.row_abs)) } else { ((b.row, b.row_abs), (a.row, a.row_abs)) };
    let (c0, c1) = if a.col <= b.col { ((a.col, a.col_abs), (b.col, b.col_abs)) } else { ((b.col, b.col_abs), (a.col, a.col_abs)) };
    (
        Part { row: r0.0, row_abs: r0.1, col: c0.0, col_abs: c0.1 },
        Part { row: r1.0, row_abs: r1.1, col: c1.0, col_abs: c1.1 },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(col_name(702), "AAA");
        assert_eq!(col_name(MAX_COLS - 1), "XFD");
        assert_eq!(col_index("A"), Some(0));
        assert_eq!(col_index("aa"), Some(26));
        assert_eq!(col_index("XFD"), Some(MAX_COLS - 1));
        assert_eq!(col_index("XFE"), None);
        assert_eq!(col_index("A1"), None);
        assert_eq!(col_index(""), None);
    }

    #[test]
    fn addresses() {
        assert_eq!(Addr::parse("B12"), Some(Addr::new(11, 1)));
        assert_eq!(Addr::parse("$b$12"), Some(Addr::new(11, 1)));
        assert_eq!(Addr::parse("A0"), None);
        assert_eq!(Addr::parse("A1048577"), None);
        assert_eq!(Addr::parse("A1:B2"), None);
        assert_eq!(Addr::parse("12"), None);
        assert_eq!(Addr::new(0, 27).a1(), "AB1");
        assert!(Addr::new(0, 5) < Addr::new(1, 0), "row-major order");
    }

    #[test]
    fn ranges() {
        let r = Range::parse("C3:A1").unwrap();
        assert_eq!(r.a1(), "A1:C3");
        assert_eq!((r.rows(), r.cols()), (3, 3));
        assert!(r.contains(Addr::new(1, 1)));
        assert!(!r.contains(Addr::new(3, 0)));
        assert_eq!(Range::parse("B2").unwrap().a1(), "B2");
        let cols = Range::parse("A:C").unwrap();
        assert_eq!(cols.rows(), MAX_ROWS);
        assert_eq!(cols.a1(), "A:C");
        let rows = Range::parse("2:5").unwrap();
        assert_eq!((rows.start.row, rows.end.row, rows.cols()), (1, 4, MAX_COLS));
        assert_eq!(rows.a1(), "2:5");
        assert_eq!(Range::parse("A1:B"), None);
        assert_eq!(Range::parse("SUM"), None);
        let cells: Vec<String> = Range::parse("A1:B2").unwrap().iter().map(|a| a.a1()).collect();
        assert_eq!(cells, ["A1", "B1", "A2", "B2"]);
    }

    #[test]
    fn sheet_ranges() {
        let r = SheetRange::parse("'My sheet'!A1:B3").unwrap();
        assert_eq!(r.sheet.as_deref(), Some("My sheet"));
        assert_eq!(r.range.a1(), "A1:B3");
        assert_eq!(r.to_string(), "'My sheet'!A1:B3");
        let r = SheetRange::parse("Sales!B2").unwrap();
        assert_eq!(r.to_string(), "Sales!B2");
        let r = SheetRange::parse("A1:B2").unwrap();
        assert_eq!(r.sheet, None);
        let r = SheetRange::parse("'Bob''s'!C:C").unwrap();
        assert_eq!(r.sheet.as_deref(), Some("Bob's"));
        assert_eq!(r.to_string(), "'Bob''s'!C:C");
        assert_eq!(SheetRange::parse("A1+1"), None);
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_sheet_name("Sheet1"), "Sheet1");
        assert_eq!(quote_sheet_name("My sheet"), "'My sheet'");
        assert_eq!(quote_sheet_name("2024"), "'2024'");
        assert_eq!(quote_sheet_name("A1"), "'A1'");
        assert_eq!(quote_sheet_name("R1C1"), "'R1C1'");
        assert_eq!(quote_sheet_name("TRUE"), "'TRUE'");
        assert_eq!(quote_sheet_name("Données"), "Données");
        assert_eq!(quote_sheet_name("Raw"), "Raw");
        assert_eq!(quote_sheet_name("Data"), "Data");
        assert_eq!(quote_sheet_name("XFD1"), "'XFD1'");
    }
}
