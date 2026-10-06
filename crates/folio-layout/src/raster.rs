//! Charts and slides as PNG pictures (tiny-skia), for exports that need them.
//!
//! PLACEHOLDER.

use folio_core::{Chart, ChartData};

use crate::chart::ChartStyle;

/// A chart as a PNG `w`×`h` pixels.
pub fn chart_png(_chart: &Chart, _data: &ChartData, _w: u32, _h: u32, _style: &ChartStyle) -> Vec<u8> {
    vec![]
}
