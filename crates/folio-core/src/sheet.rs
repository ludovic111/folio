//! Sheets: a grid of cells with formulas (computed by `folio-calc`), number formats, column
//! widths, frozen panes, a filter and charts.
//!
//! A cell keeps what the person typed (`input`: `12`, `Total`, `=SUM(B2:B9)`) and the value
//! computed from it (`value`, written by [`crate::recalc`]; stored in the file so other readers
//! see results without a formula engine).

use std::collections::BTreeMap;

use folio_calc::{Addr, Range, Value};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Chart, Id, text::Align};

pub type Cells = imbl::OrdMap<Addr, Cell>;

fn is_false(b: &bool) -> bool {
    !*b
}

/// Default column width and row height, in pixels at 100 %.
pub const COL_W: f32 = 96.0;
pub const ROW_H: f32 = 24.0;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CellFormat {
    /// A number format code (`0.00`, `#,##0`, `0%`, `$#,##0.00`, `yyyy-mm-dd`…); General when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Text colour `#rrggbb`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Fill `#rrggbb`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// Horizontal alignment; numbers go right and text left when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(skip_serializing_if = "is_false")]
    pub wrap: bool,
    /// Size in points (10 when absent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// Borders drawn on the cell's sides: any of `t`, `r`, `b`, `l`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub border: String,
}

impl CellFormat {
    pub fn is_default(&self) -> bool {
        *self == CellFormat::default()
    }
}

/// A change to cell formats: `Some` sets; for the optional strings `Some("")` clears.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormatPatch {
    pub number: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<String>,
    pub fill: Option<String>,
    /// `left`, `center`, `right`, or "" for automatic.
    pub align: Option<String>,
    pub wrap: Option<bool>,
    pub size: Option<f32>,
    pub border: Option<String>,
}

impl FormatPatch {
    pub fn apply(&self, f: &mut CellFormat) {
        let set = |t: &mut Option<String>, v: &Option<String>| {
            if let Some(v) = v {
                *t = if v.is_empty() { None } else { Some(v.clone()) };
            }
        };
        set(&mut f.number, &self.number.as_ref().map(|n| if n.eq_ignore_ascii_case("general") { String::new() } else { n.clone() }));
        set(&mut f.color, &self.color);
        set(&mut f.fill, &self.fill);
        if let Some(v) = self.bold {
            f.bold = v;
        }
        if let Some(v) = self.italic {
            f.italic = v;
        }
        if let Some(v) = self.underline {
            f.underline = v;
        }
        if let Some(v) = self.strike {
            f.strike = v;
        }
        if let Some(v) = self.wrap {
            f.wrap = v;
        }
        if let Some(a) = &self.align {
            f.align = Align::parse(a);
        }
        if let Some(s) = self.size {
            f.size = if s > 0.0 { Some(s) } else { None };
        }
        if let Some(b) = &self.border {
            f.border = b.chars().filter(|c| "trbl".contains(*c)).collect();
            if b == "all" {
                f.border = "trbl".into();
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    /// What was typed: a number, text, TRUE/FALSE, or a formula starting with `=`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub input: String,
    /// The computed value (kept in step by the formula engine).
    #[serde(default, skip_serializing_if = "Value::is_empty")]
    pub value: Value,
    #[serde(default, skip_serializing_if = "CellFormat::is_default")]
    pub format: CellFormat,
}

impl Cell {
    pub fn is_formula(&self) -> bool {
        self.input.starts_with('=')
    }

    /// The value as the cell shows it (its number format applied).
    pub fn display(&self) -> String {
        folio_calc::format_value(&self.value, self.format.number.as_deref())
    }

    pub fn is_blank(&self) -> bool {
        self.input.is_empty() && self.format.is_default()
    }
}

/// A filter over a range: rows whose cells don't pass are hidden.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    /// The filtered range, header row included (`A1:D40`).
    pub range: String,
    /// Per column of the range (0 = its first column): the condition.
    #[serde(default, deserialize_with = "de_index_map")]
    pub rules: BTreeMap<u32, FilterRule>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterRule {
    /// Keep rows whose shown text is one of these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    /// Or a condition like COUNTIF's: `>100`, `<>done`, `=*draft*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// A chart floating over the grid (position and size in pixels from the grid's top-left).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetChart {
    pub id: Id,
    pub chart: Chart,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    #[serde(serialize_with = "ser_cells", deserialize_with = "de_cells", default)]
    pub cells: Cells,
    /// Column widths in pixels by column index (default [`COL_W`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "de_index_map")]
    pub cols: BTreeMap<u32, f32>,
    /// Row heights in pixels by row index (default [`ROW_H`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "de_index_map")]
    pub rows: BTreeMap<u32, f32>,
    #[serde(default)]
    pub freeze_rows: u32,
    #[serde(default)]
    pub freeze_cols: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub charts: Vec<SheetChart>,
    #[serde(default = "yes")]
    pub gridlines: bool,
}

fn yes() -> bool {
    true
}

impl Default for Sheet {
    fn default() -> Self {
        Sheet { cells: Cells::new(), cols: BTreeMap::new(), rows: BTreeMap::new(), freeze_rows: 0, freeze_cols: 0, filter: None, charts: vec![], gridlines: true }
    }
}

/// Maps keyed by an index, written as JSON objects ("0": …). Read by hand because pages are
/// flattened (serde can't turn string keys into numbers through a flattened buffer).
fn de_index_map<'de, D: Deserializer<'de>, V: Deserialize<'de>>(d: D) -> Result<BTreeMap<u32, V>, D::Error> {
    let map: BTreeMap<String, V> = BTreeMap::deserialize(d)?;
    map.into_iter().map(|(k, v)| k.trim().parse::<u32>().map(|k| (k, v)).map_err(|_| serde::de::Error::custom(format!("`{k}` isn't an index")))).collect()
}

fn ser_cells<S: Serializer>(cells: &Cells, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut m = s.serialize_map(Some(cells.len()))?;
    for (a, c) in cells.iter() {
        m.serialize_entry(&a.a1(), c)?;
    }
    m.end()
}

fn de_cells<'de, D: Deserializer<'de>>(d: D) -> Result<Cells, D::Error> {
    let map: BTreeMap<String, Cell> = BTreeMap::deserialize(d)?;
    let mut out = Cells::new();
    for (k, v) in map {
        let a = Addr::parse(&k).ok_or_else(|| serde::de::Error::custom(format!("`{k}` isn't a cell address")))?;
        out.insert(a, v);
    }
    Ok(out)
}

impl Sheet {
    pub fn cell(&self, a: Addr) -> Option<&Cell> {
        self.cells.get(&a)
    }

    pub fn value(&self, a: Addr) -> Value {
        self.cells.get(&a).map(|c| c.value.clone()).unwrap_or(Value::Empty)
    }

    pub fn input(&self, a: Addr) -> &str {
        self.cells.get(&a).map(|c| c.input.as_str()).unwrap_or("")
    }

    pub fn display(&self, a: Addr) -> String {
        self.cells.get(&a).map(Cell::display).unwrap_or_default()
    }

    pub fn col_width(&self, col: u32) -> f32 {
        self.cols.get(&col).copied().unwrap_or(COL_W)
    }

    pub fn row_height(&self, row: u32) -> f32 {
        self.rows.get(&row).copied().unwrap_or(ROW_H)
    }

    /// The smallest range holding every cell with content or a format.
    pub fn used_range(&self) -> Option<Range> {
        let mut it = self.cells.iter().filter(|(_, c)| !c.is_blank());
        let (first, _) = it.next()?;
        let (mut r0, mut c0, mut r1, mut c1) = (first.row, first.col, first.row, first.col);
        for (a, _) in it {
            r0 = r0.min(a.row);
            r1 = r1.max(a.row);
            c0 = c0.min(a.col);
            c1 = c1.max(a.col);
        }
        Some(Range { start: Addr::new(r0, c0), end: Addr::new(r1, c1) })
    }

    /// Range clipped to the used area (whole columns and rows would be a million cells).
    pub fn clip(&self, r: Range) -> Range {
        let Some(used) = self.used_range() else { return Range { start: r.start, end: r.start } };
        Range { start: r.start, end: Addr::new(r.end.row.min(used.end.row.max(r.start.row)), r.end.col.min(used.end.col.max(r.start.col))) }
    }

    /// Sets what a cell holds (keeping its format). An empty input with no format removes the cell.
    /// A suggested number format (dates, percentages, currency typed in) is applied when the
    /// cell has none.
    pub fn set_input(&mut self, a: Addr, input: &str) {
        let input = input.to_string();
        let (_, suggested) = folio_calc::parse_input(&input);
        match self.cells.get_mut(&a) {
            Some(c) => {
                c.input = input;
                if c.format.number.is_none()
                    && let Some(f) = suggested
                {
                    c.format.number = Some(f.to_string());
                }
                if c.is_blank() {
                    self.cells.remove(&a);
                }
            }
            None if input.is_empty() => {}
            None => {
                let format = CellFormat { number: suggested.map(str::to_string), ..Default::default() };
                self.cells.insert(a, Cell { input, value: Value::Empty, format });
            }
        }
    }

    /// Clears contents, formats or both in a range.
    pub fn clear(&mut self, r: Range, contents: bool, formats: bool) {
        let addrs: Vec<Addr> = self.cells.range(r.start..=Addr::new(r.end.row, r.end.col)).map(|(a, _)| *a).filter(|a| r.contains(*a)).collect();
        for a in addrs {
            let Some(c) = self.cells.get_mut(&a) else { continue };
            if contents {
                c.input.clear();
                c.value = Value::Empty;
            }
            if formats {
                c.format = CellFormat::default();
            }
            if c.is_blank() {
                self.cells.remove(&a);
            }
        }
    }

    /// Applies a format change to every cell of a range (creating empty cells to hold it).
    pub fn format(&mut self, r: Range, patch: &FormatPatch) {
        let r = if r.rows() as u64 * r.cols() as u64 > 200_000 { self.clip(r) } else { r };
        for a in r.iter() {
            let mut c = self.cells.get(&a).cloned().unwrap_or_default();
            patch.apply(&mut c.format);
            if c.is_blank() {
                self.cells.remove(&a);
            } else {
                self.cells.insert(a, c);
            }
        }
    }

    /// Moves cells for inserted (count > 0) or deleted (count < 0) rows or columns. Formulas are
    /// rewritten by the caller (`Document::insert_rows`), which knows every sheet.
    pub fn shift(&mut self, rows: bool, at: u32, count: i64) {
        let mut out = Cells::new();
        for (a, c) in self.cells.iter() {
            let k = if rows { a.row } else { a.col };
            let nk = if k < at {
                Some(k as i64)
            } else if count < 0 && (k as i64) < at as i64 - count {
                None
            } else {
                Some(k as i64 + count)
            };
            if let Some(nk) = nk.filter(|v| *v >= 0) {
                let na = if rows { Addr::new(nk as u32, a.col) } else { Addr::new(a.row, nk as u32) };
                out.insert(na, c.clone());
            }
        }
        self.cells = out;
        let sizes = if rows { &mut self.rows } else { &mut self.cols };
        let moved: BTreeMap<u32, f32> = sizes
            .iter()
            .filter_map(|(k, v)| {
                if *k < at {
                    Some((*k, *v))
                } else if count < 0 && (*k as i64) < at as i64 - count {
                    None
                } else {
                    Some(((*k as i64 + count).max(0) as u32, *v))
                }
            })
            .collect();
        *sizes = moved;
    }

    /// Values and shown text of a range (clipped to the used area), row by row.
    pub fn grid(&self, r: Range) -> Vec<Vec<(Value, String)>> {
        let r = self.clip(r);
        (r.start.row..=r.end.row)
            .map(|row| {
                (r.start.col..=r.end.col)
                    .map(|col| {
                        let a = Addr::new(row, col);
                        (self.value(a), self.display(a))
                    })
                    .collect()
            })
            .collect()
    }

    /// Rows hidden by the filter.
    pub fn hidden_rows(&self) -> std::collections::BTreeSet<u32> {
        let mut out = std::collections::BTreeSet::new();
        let Some(f) = &self.filter else { return out };
        let Some(r) = Range::parse(&f.range) else { return out };
        for row in r.start.row + 1..=r.end.row {
            let pass = f.rules.iter().all(|(dc, rule)| {
                let a = Addr::new(row, r.start.col + dc);
                let shown = self.display(a);
                if let Some(vals) = &rule.values
                    && !vals.iter().any(|v| v == &shown)
                {
                    return false;
                }
                if let Some(cond) = &rule.condition
                    && !folio_calc::matches_criteria(&self.value(a), cond)
                {
                    return false;
                }
                true
            });
            if !pass {
                out.insert(row);
            }
        }
        out
    }

    /// Sorts the rows of a range by columns (`(column index in the sheet, ascending)`), the
    /// first row staying put when it is a header. Formulas in moved rows are moved like a
    /// copy (their relative references follow).
    pub fn sort(&mut self, r: Range, keys: &[(u32, bool)], header: bool) {
        let first = if header { r.start.row + 1 } else { r.start.row };
        if first > r.end.row {
            return;
        }
        let rows: Vec<u32> = (first..=r.end.row).collect();
        let key_of = |row: u32| -> Vec<Value> { keys.iter().map(|(c, _)| self.value(Addr::new(row, *c))).collect() };
        let mut order: Vec<(u32, Vec<Value>)> = rows.iter().map(|&row| (row, key_of(row))).collect();
        order.sort_by(|a, b| {
            for (i, (_, asc)) in keys.iter().enumerate() {
                let o = sort_cmp(&a.1[i], &b.1[i], *asc);
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            a.0.cmp(&b.0)
        });
        let mut moved: Vec<(Addr, Option<Cell>)> = vec![];
        for (i, (src, _)) in order.iter().enumerate() {
            let dst = first + i as u32;
            for col in r.start.col..=r.end.col {
                let cell = self.cells.get(&Addr::new(*src, col)).cloned().map(|mut c| {
                    if c.is_formula() && dst != *src {
                        c.input = format!("={}", folio_calc::translate(&c.input[1..], dst as i64 - *src as i64, 0));
                    }
                    c
                });
                moved.push((Addr::new(dst, col), cell));
            }
        }
        for (a, c) in moved {
            match c {
                Some(c) => {
                    self.cells.insert(a, c);
                }
                None => {
                    self.cells.remove(&a);
                }
            }
        }
    }

    /// Copies `src` over `dst` (same size or a multiple: the source repeats), moving relative
    /// references in formulas like a paste does.
    pub fn paste(&mut self, src: &[Vec<Cell>], at: Addr, formats: bool) {
        for (dr, row) in src.iter().enumerate() {
            for (dc, cell) in row.iter().enumerate() {
                let a = Addr::new(at.row + dr as u32, at.col + dc as u32);
                let mut c = cell.clone();
                if !formats {
                    c.format = self.cells.get(&a).map(|x| x.format.clone()).unwrap_or_default();
                }
                if c.is_blank() {
                    self.cells.remove(&a);
                } else {
                    self.cells.insert(a, c);
                }
            }
        }
    }

    /// The cells of a range as a block to paste elsewhere; formulas are rewritten relative to
    /// `to` (pass `r.start` for none).
    pub fn copy_block(&self, r: Range, to: Addr) -> Vec<Vec<Cell>> {
        let (dr, dc) = (to.row as i64 - r.start.row as i64, to.col as i64 - r.start.col as i64);
        (r.start.row..=r.end.row)
            .map(|row| {
                (r.start.col..=r.end.col)
                    .map(|col| {
                        let mut c = self.cells.get(&Addr::new(row, col)).cloned().unwrap_or_default();
                        if c.is_formula() && (dr != 0 || dc != 0) {
                            c.input = format!("={}", folio_calc::translate(&c.input[1..], dr, dc));
                        }
                        c
                    })
                    .collect()
            })
            .collect()
    }

    /// Fills a range from its first row (down) or column (right): numbers and dates continue
    /// the step of the first two, other values repeat, formulas move their references.
    pub fn fill(&mut self, r: Range, down: bool) {
        let lines = if down { r.cols() } else { r.rows() };
        let len = if down { r.rows() } else { r.cols() };
        for line in 0..lines {
            let at = |i: u32| if down { Addr::new(r.start.row + i, r.start.col + line) } else { Addr::new(r.start.row + line, r.start.col + i) };
            // The seed: leading cells with content (at most two for a series).
            let seed: Vec<Cell> = (0..len.min(2)).map(|i| self.cells.get(&at(i)).cloned().unwrap_or_default()).take_while(|c| !c.input.is_empty()).collect();
            if seed.is_empty() {
                continue;
            }
            let nums: Vec<f64> = seed.iter().filter(|c| !c.is_formula()).filter_map(|c| c.value.as_number().or_else(|| c.input.trim().parse().ok())).collect();
            let series = nums.len() == 2 && seed.len() == 2;
            let start = if series { 2 } else { 1 };
            for i in start..len {
                let src = &seed[if series { 1 } else { 0 }];
                let mut c = src.clone();
                if c.is_formula() {
                    let k = i as i64 - if series { 1 } else { 0 };
                    let (dr, dc) = if down { (k, 0) } else { (0, k) };
                    c.input = format!("={}", folio_calc::translate(&src.input[1..], dr, dc));
                } else if series {
                    let v = nums[1] + (nums[1] - nums[0]) * (i as f64 - 1.0);
                    c.input = fmt_num(v);
                }
                c.value = Value::Empty;
                self.cells.insert(at(i), c);
            }
        }
    }
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// Spreadsheet sort order: numbers before text before booleans, blanks last either way.
pub fn sort_cmp(a: &Value, b: &Value, asc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let rank = |v: &Value| match v {
        Value::Number(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        Value::Error(_) => 3,
        Value::Empty => 4,
    };
    let (ra, rb) = (rank(a), rank(b));
    if ra == 4 || rb == 4 {
        return ra.cmp(&rb);
    }
    let o = if ra != rb {
        ra.cmp(&rb)
    } else {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.partial_cmp(y).unwrap_or(Equal),
            (Value::Text(x), Value::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            _ => Equal,
        }
    };
    if asc { o } else { o.reverse() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> Addr {
        Addr::parse(s).unwrap()
    }

    #[test]
    fn json_keys_are_a1() {
        let mut s = Sheet::default();
        s.cols.insert(2, 140.0);
        s.set_input(a("B3"), "12");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["cells"]["B3"]["input"], "12");
        let back: Sheet = serde_json::from_value(v).unwrap();
        assert_eq!(back.input(a("B3")), "12");
    }

    #[test]
    fn shift_rows_moves_and_deletes() {
        let mut s = Sheet::default();
        for (i, r) in ["A1", "A2", "A3", "A4"].iter().enumerate() {
            s.set_input(a(r), &i.to_string());
        }
        s.shift(true, 1, 2);
        assert_eq!(s.input(a("A1")), "0");
        assert_eq!(s.input(a("A4")), "1");
        assert_eq!(s.input(a("A6")), "3");
        s.shift(true, 1, -2);
        assert_eq!(s.input(a("A2")), "1");
        assert_eq!(s.input(a("A4")), "3");
    }

    #[test]
    fn clear_and_format() {
        let mut s = Sheet::default();
        s.set_input(a("A1"), "x");
        s.format(Range::parse("A1:B2").unwrap(), &FormatPatch { bold: Some(true), ..Default::default() });
        assert_eq!(s.cells.len(), 4);
        s.clear(Range::parse("A1:B2").unwrap(), false, true);
        assert_eq!(s.cells.len(), 1);
        s.clear(Range::parse("A1:B2").unwrap(), true, false);
        assert!(s.cells.is_empty());
    }

    #[test]
    fn sort_with_header() {
        let mut s = Sheet::default();
        s.set_input(a("A1"), "Name");
        s.set_input(a("A2"), "b");
        s.set_input(a("A3"), "a");
        s.set_input(a("A4"), "c");
        for r in ["A2", "A3", "A4"] {
            let v = s.input(a(r)).to_string();
            s.cells.get_mut(&a(r)).unwrap().value = Value::Text(v);
        }
        s.sort(Range::parse("A1:A4").unwrap(), &[(0, true)], true);
        assert_eq!([s.input(a("A1")), s.input(a("A2")), s.input(a("A3")), s.input(a("A4"))], ["Name", "a", "b", "c"]);
    }

    #[test]
    fn fill_continues_series() {
        let mut s = Sheet::default();
        s.set_input(a("A1"), "1");
        s.set_input(a("A2"), "3");
        s.fill(Range::parse("A1:A5").unwrap(), true);
        assert_eq!(s.input(a("A5")), "9");
        s.set_input(a("B1"), "=A1*2");
        s.fill(Range::parse("B1:B3").unwrap(), true);
        assert_eq!(s.input(a("B3")), "=A3*2");
    }
}
