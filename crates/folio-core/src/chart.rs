//! Charts: a kind and a live source range in a sheet. The same chart can sit on a sheet, on a
//! slide or in a document; its data is always read from the sheet's computed values.

use folio_calc::Value;
use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChartKind {
    #[default]
    Column,
    Bar,
    Line,
    Area,
    Pie,
    Scatter,
}

impl ChartKind {
    pub const ALL: [ChartKind; 6] = [ChartKind::Column, ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Pie, ChartKind::Scatter];

    pub fn parse(s: &str) -> Option<ChartKind> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "column" | "columns" | "col" | "vbar" => ChartKind::Column,
            "bar" | "bars" | "hbar" => ChartKind::Bar,
            "line" | "lines" => ChartKind::Line,
            "area" => ChartKind::Area,
            "pie" | "donut" | "doughnut" => ChartKind::Pie,
            "scatter" | "xy" | "points" => ChartKind::Scatter,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            ChartKind::Column => "column",
            ChartKind::Bar => "bar",
            ChartKind::Line => "line",
            ChartKind::Area => "area",
            ChartKind::Pie => "pie",
            ChartKind::Scatter => "scatter",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChartKind::Column => "Column",
            ChartKind::Bar => "Bar",
            ChartKind::Line => "Line",
            ChartKind::Area => "Area",
            ChartKind::Pie => "Pie",
            ChartKind::Scatter => "Scatter",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    #[serde(default)]
    pub kind: ChartKind,
    /// The live source: a sheet range like `'Sales'!A1:C13`.
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Each row of the range is a series (otherwise each column is).
    #[serde(default, skip_serializing_if = "is_false")]
    pub series_in_rows: bool,
    /// The first row and column hold names (series names and category labels).
    #[serde(default = "yes")]
    pub headers: bool,
    #[serde(default = "yes")]
    pub legend: bool,
    /// Bars and areas stack.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stacked: bool,
}

impl Chart {
    pub fn new(kind: ChartKind, source: impl Into<String>) -> Self {
        Chart { kind, source: source.into(), title: String::new(), series_in_rows: false, headers: true, legend: true, stacked: false }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    /// `None` where the cell is empty or not a number.
    pub values: Vec<Option<f64>>,
}

/// What a chart draws: category labels and series of numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChartData {
    pub categories: Vec<String>,
    pub series: Vec<Series>,
}

impl ChartData {
    /// Reads a range's values (`grid[row][col]`, each with its formatted text) the way
    /// spreadsheets do: with headers, the first row names the series and the first column
    /// labels the categories when it holds text.
    pub fn from_grid(grid: &[Vec<(Value, String)>], headers: bool, series_in_rows: bool) -> ChartData {
        // Work on columns-as-series; transpose for series in rows.
        let rows = grid.len();
        let cols = grid.iter().map(Vec::len).max().unwrap_or(0);
        let at = |r: usize, c: usize| -> Option<&(Value, String)> { if series_in_rows { grid.get(c).and_then(|row| row.get(r)) } else { grid.get(r).and_then(|row| row.get(c)) } };
        let (rows, cols) = if series_in_rows { (cols, rows) } else { (rows, cols) };
        if rows == 0 || cols == 0 {
            return ChartData::default();
        }
        let is_text = |v: Option<&(Value, String)>| matches!(v.map(|x| &x.0), Some(Value::Text(_)));
        let header_row = headers && (0..cols).any(|c| is_text(at(0, c))) || headers && rows > 1 && (0..cols).all(|c| !matches!(at(0, c).map(|x| &x.0), Some(Value::Number(_))));
        let first_data_row = if header_row { 1 } else { 0 };
        let label_col = headers && cols > 1 && (first_data_row..rows).any(|r| is_text(at(r, 0)));
        let first_series_col = if label_col { 1 } else { 0 };
        let categories = (first_data_row..rows)
            .map(|r| if label_col { at(r, 0).map(|x| x.1.clone()).unwrap_or_default() } else { (r - first_data_row + 1).to_string() })
            .collect();
        let series = (first_series_col..cols)
            .map(|c| Series {
                name: if header_row { at(0, c).map(|x| x.1.clone()).unwrap_or_default() } else { format!("Series {}", c - first_series_col + 1) },
                values: (first_data_row..rows).map(|r| at(r, c).and_then(|x| x.0.as_number())).collect(),
            })
            .collect();
        ChartData { categories, series }
    }

    /// The lowest and highest values (stacked sums when `stacked`), always including 0.
    pub fn bounds(&self, stacked: bool) -> (f64, f64) {
        let mut lo: f64 = 0.0;
        let mut hi: f64 = 0.0;
        if stacked {
            for i in 0..self.categories.len() {
                let (mut pos, mut neg) = (0.0, 0.0);
                for s in &self.series {
                    let v = s.values.get(i).copied().flatten().unwrap_or(0.0);
                    if v >= 0.0 {
                        pos += v
                    } else {
                        neg += v
                    }
                }
                hi = hi.max(pos);
                lo = lo.min(neg);
            }
        } else {
            for v in self.series.iter().flat_map(|s| s.values.iter().flatten()) {
                hi = hi.max(*v);
                lo = lo.min(*v);
            }
        }
        (lo, hi)
    }
}

/// Round axis steps: the bounds widened to "nice" numbers and the step between ticks.
pub fn nice_axis(lo: f64, hi: f64, ticks: usize) -> (f64, f64, f64) {
    let span = (hi - lo).abs().max(1e-9);
    let raw = span / ticks.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 2.5 {
        2.5
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    } * mag;
    ((lo / step).floor() * step, (hi / step).ceil() * step, step)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> (Value, String) {
        (Value::Text(s.into()), s.into())
    }
    fn n(v: f64) -> (Value, String) {
        (Value::Number(v), v.to_string())
    }

    #[test]
    fn headers_and_labels() {
        let grid = vec![vec![t("Month"), t("Sales"), t("Costs")], vec![t("Jan"), n(10.0), n(4.0)], vec![t("Feb"), n(12.0), n(5.0)]];
        let d = ChartData::from_grid(&grid, true, false);
        assert_eq!(d.categories, ["Jan", "Feb"]);
        assert_eq!(d.series.len(), 2);
        assert_eq!(d.series[0].name, "Sales");
        assert_eq!(d.series[1].values, [Some(4.0), Some(5.0)]);
        let rows = ChartData::from_grid(&grid, true, true);
        assert_eq!(rows.categories, ["Sales", "Costs"]);
        assert_eq!(rows.series[0].name, "Jan");
    }

    #[test]
    fn numbers_only() {
        let grid = vec![vec![n(1.0), n(2.0)], vec![n(3.0), n(4.0)]];
        let d = ChartData::from_grid(&grid, true, false);
        assert_eq!(d.series.len(), 2);
        assert_eq!(d.categories, ["1", "2"]);
    }

    #[test]
    fn nice_steps() {
        let (lo, hi, step) = nice_axis(0.0, 87.0, 5);
        assert_eq!((lo, hi, step), (0.0, 100.0, 20.0));
    }
}
