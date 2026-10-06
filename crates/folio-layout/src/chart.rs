//! A chart's geometry: primitives anyone can draw (the window, PDF, PNG).
//!
//! PLACEHOLDER: the real implementation replaces `chart_prims`.

use folio_core::{Chart, ChartData};

use crate::Rgba;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Rect { x: f32, y: f32, w: f32, h: f32, fill: Rgba },
    Line { points: Vec<(f32, f32)>, width: f32, color: Rgba },
    Poly { points: Vec<(f32, f32)>, fill: Rgba },
    /// `y` is the baseline.
    Text { x: f32, y: f32, text: String, size: f32, color: Rgba, anchor: Anchor, bold: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartStyle {
    pub text: Rgba,
    pub grid: Rgba,
    pub series: Vec<Rgba>,
    /// Label size in points.
    pub size: f32,
    pub background: Option<Rgba>,
}

impl Default for ChartStyle {
    fn default() -> Self {
        ChartStyle { text: [40, 40, 40, 255], grid: [0, 0, 0, 40], series: vec![[10, 10, 10, 255], [120, 120, 120, 255], [190, 190, 190, 255]], size: 9.0, background: None }
    }
}

/// The chart drawn in a `w`×`h` box (origin top-left).
pub fn chart_prims(_chart: &Chart, data: &ChartData, w: f32, h: f32, style: &ChartStyle) -> Vec<Prim> {
    let mut out = vec![];
    let (lo, hi) = data.bounds(false);
    let span = (hi - lo).max(1e-9) as f32;
    let n = data.categories.len().max(1);
    let bw = w / n as f32;
    for (si, s) in data.series.iter().enumerate() {
        for (i, v) in s.values.iter().enumerate() {
            let v = v.unwrap_or(0.0) as f32;
            let bh = (v - lo as f32) / span * h * 0.9;
            let x = i as f32 * bw + si as f32 * bw / (data.series.len() as f32 + 1.0);
            out.push(Prim::Rect { x, y: h - bh, w: bw / (data.series.len() as f32 + 1.0), h: bh, fill: style.series[si % style.series.len()] });
        }
    }
    out
}
