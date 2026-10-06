//! A chart's geometry: primitives anyone can draw (the window, PDF, PNG).
//!
//! Every kind (column, bar, line, area, pie, scatter) gets a title, a value axis with round
//! ticks and light grid lines, category labels (thinned when crowded: all text is
//! horizontal), a legend at the bottom when there is more than one series (always for pies),
//! stacking, negative values, and a plain message when there is nothing to draw. Text prims
//! are set in IBM Plex Sans (SemiBold when `bold`); widths are estimated, so they never need
//! the font system.

use std::f32::consts::PI;

use folio_core::chart::nice_axis;
use folio_core::{Chart, ChartData, ChartKind};

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

impl ChartStyle {
    fn color(&self, i: usize) -> Rgba {
        if self.series.is_empty() { self.text } else { self.series[i % self.series.len()] }
    }
}

/// Estimated width of a label in IBM Plex Sans.
pub fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    let em: f32 = text
        .chars()
        .map(|c| match c {
            'i' | 'l' | 'j' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' | 'I' => 0.27,
            ' ' | 'f' | 't' | 'r' | '(' | ')' | '-' => 0.33,
            'm' | 'w' | 'M' | 'W' | '%' | '@' => 0.82,
            '0'..='9' => 0.56,
            c if c.is_uppercase() => 0.64,
            c if (c as u32) > 0x2E80 => 1.0,
            _ => 0.53,
        })
        .sum();
    em * size * if bold { 1.04 } else { 1.0 }
}

/// Cuts a label to fit a width, with an ellipsis.
fn fit(text: &str, size: f32, max: f32) -> String {
    if text_width(text, size, false) <= max {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if text_width(&out, size, false) + size * 0.6 > max {
            out.pop();
            break;
        }
    }
    if out.is_empty() { String::new() } else { format!("{}…", out.trim_end()) }
}

/// A tick value written for its step: 1.5k, 2M, 0.25, 40%…
pub fn format_tick(v: f64, step: f64) -> String {
    let a = v.abs();
    let (div, suffix) = if step >= 1e9 || a >= 1e10 {
        (1e9, "B")
    } else if step >= 1e6 || a >= 1e7 {
        (1e6, "M")
    } else if step >= 1e3 && a >= 1e4 {
        (1e3, "k")
    } else {
        (1.0, "")
    };
    let s = step / div;
    let decimals = if s >= 1.0 || s <= 0.0 { 0 } else { ((-s.log10()).ceil() as usize).min(6) };
    let mut out = format!("{:.*}", decimals, v / div);
    if out == "-0" {
        out = "0".into();
    }
    out + suffix
}

/// Pie wedges: (start angle, sweep) in degrees, clockwise from 12 o'clock, for the positive
/// values (others get no wedge, sweep 0).
pub fn pie_angles(values: &[Option<f64>]) -> Vec<(f32, f32)> {
    let total: f64 = values.iter().flatten().filter(|v| **v > 0.0).sum();
    let mut a = 0.0f32;
    values
        .iter()
        .map(|v| {
            let v = v.filter(|v| *v > 0.0).unwrap_or(0.0);
            let sweep = if total > 0.0 { (v / total * 360.0) as f32 } else { 0.0 };
            let out = (a, sweep);
            a += sweep;
            out
        })
        .collect()
}

fn rect(x: f32, y: f32, w: f32, h: f32, fill: Rgba) -> Prim {
    let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
    let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
    Prim::Rect { x, y, w, h, fill }
}

fn text(x: f32, y: f32, t: impl Into<String>, size: f32, color: Rgba, anchor: Anchor, bold: bool) -> Prim {
    Prim::Text { x, y, text: t.into(), size, color, anchor, bold }
}

/// Whether a colour is dark (for labels on it).
fn dark(c: Rgba) -> bool {
    (c[0] as f32 * 0.299 + c[1] as f32 * 0.587 + c[2] as f32 * 0.114) < 140.0
}

/// The chart drawn in a `w`×`h` box (origin top-left).
pub fn chart_prims(chart: &Chart, data: &ChartData, w: f32, h: f32, style: &ChartStyle) -> Vec<Prim> {
    let mut out = vec![];
    if w < 8.0 || h < 8.0 {
        return out;
    }
    if let Some(bg) = style.background {
        out.push(rect(0.0, 0.0, w, h, bg));
    }
    let s = style.size.max(4.0);
    let pad = (s * 0.9).min(w * 0.05).max(2.0);
    let mut top = pad;
    if !chart.title.trim().is_empty() {
        let ts = s * 1.3;
        out.push(text(w / 2.0, top + ts * 0.9, fit(chart.title.trim(), ts, w - 2.0 * pad), ts, style.text, Anchor::Middle, true));
        top += ts * 1.6;
    }
    let has_numbers = data.series.iter().any(|se| se.values.iter().any(|v| v.is_some_and(f64::is_finite)));
    if !has_numbers || data.categories.is_empty() && chart.kind != ChartKind::Scatter {
        let src = chart.source.trim();
        let msg = if src.is_empty() { "No data: pick a sheet range".to_string() } else { format!("No numbers in {}", src.replace('\'', "")) };
        out.push(text(w / 2.0, (top + h) / 2.0, fit(&msg, s, w - 2.0 * pad), s, style.text, Anchor::Middle, false));
        return out;
    }

    // Legend at the bottom.
    let pie = chart.kind == ChartKind::Pie;
    let scatter_x_series = chart.kind == ChartKind::Scatter && data.series.len() >= 2 && !data.categories.iter().all(|c| c.trim().parse::<f64>().is_ok());
    let entries: Vec<(String, Rgba)> = if pie {
        data.categories.iter().enumerate().map(|(i, c)| (c.clone(), style.color(i))).collect()
    } else {
        let skip = scatter_x_series as usize;
        data.series.iter().enumerate().skip(skip).map(|(i, se)| (se.name.clone(), style.color(i - skip))).collect()
    };
    let mut bottom = h - pad;
    if (chart.legend && entries.len() > 1) || pie {
        let sw = s * 0.8;
        let gap = s * 1.2;
        let item_w = |name: &str| sw + s * 0.4 + text_width(name, s, false).min(w * 0.4);
        // Rows of entries that fit the width.
        let mut rows: Vec<Vec<usize>> = vec![vec![]];
        let mut used = 0.0;
        for (i, (name, _)) in entries.iter().enumerate() {
            let iw = item_w(name);
            if used > 0.0 && used + gap + iw > w - 2.0 * pad {
                rows.push(vec![]);
                used = 0.0;
            }
            used += if used > 0.0 { gap } else { 0.0 } + iw;
            rows.last_mut().unwrap().push(i);
        }
        let rows: Vec<Vec<usize>> = rows.into_iter().take(((h * 0.3) / (s * 1.6)).max(1.0) as usize).collect();
        let lh = s * 1.6;
        let legend_h = rows.len() as f32 * lh;
        let mut ly = bottom - legend_h;
        for row in &rows {
            let total: f32 = row.iter().map(|i| item_w(&entries[*i].0)).sum::<f32>() + gap * row.len().saturating_sub(1) as f32;
            let mut x = (w - total) / 2.0;
            for i in row {
                let (name, c) = &entries[*i];
                out.push(rect(x, ly + (lh - sw) / 2.0, sw, sw, *c));
                let nx = x + sw + s * 0.4;
                out.push(text(nx, ly + lh / 2.0 + s * 0.35, fit(name, s, w * 0.4), s, style.text, Anchor::Start, false));
                x += item_w(name) + gap;
            }
            ly += lh;
        }
        bottom -= legend_h + s * 0.5;
    }
    if bottom - top < s * 2.0 {
        return out;
    }

    match chart.kind {
        ChartKind::Pie => pie_chart(&mut out, data, pad, top, w - pad, bottom, style),
        ChartKind::Bar => bar_chart(&mut out, chart, data, pad, top, w - pad, bottom, style),
        ChartKind::Scatter => scatter_chart(&mut out, data, scatter_x_series, pad, top, w - pad, bottom, style),
        _ => column_chart(&mut out, chart, data, pad, top, w - pad, bottom, style),
    }
    out
}

/// Value axis: (lo, hi, step).
fn axis(lo: f64, hi: f64) -> (f64, f64, f64) {
    let (lo, hi) = if (hi - lo).abs() < 1e-12 { (lo.min(0.0), hi.max(lo + 1.0)) } else { (lo, hi) };
    nice_axis(lo, hi, 5)
}

fn ticks(lo: f64, hi: f64, step: f64) -> Vec<f64> {
    let mut v = vec![];
    let mut t = lo;
    let mut guard = 0;
    while t <= hi + step * 1e-6 && guard < 50 {
        v.push(if t.abs() < step * 1e-9 { 0.0 } else { t });
        t += step;
        guard += 1;
    }
    v
}

/// Stacked tops per category and series: (base, top) of each segment.
fn stacks(data: &ChartData, stacked: bool) -> Vec<Vec<Option<(f64, f64)>>> {
    let n = data.categories.len();
    let mut pos = vec![0.0; n];
    let mut neg = vec![0.0; n];
    data.series
        .iter()
        .map(|se| {
            (0..n)
                .map(|i| {
                    let v = se.values.get(i).copied().flatten().filter(|v| v.is_finite())?;
                    if !stacked {
                        return Some((0.0, v));
                    }
                    let acc = if v >= 0.0 { &mut pos[i] } else { &mut neg[i] };
                    let base = *acc;
                    *acc += v;
                    Some((base, *acc))
                })
                .collect()
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn column_chart(out: &mut Vec<Prim>, chart: &Chart, data: &ChartData, x0: f32, top: f32, x1: f32, bottom: f32, style: &ChartStyle) {
    let s = style.size;
    let stacked = chart.stacked && chart.kind != ChartKind::Line;
    let (lo, hi) = data.bounds(stacked);
    let (lo, hi, step) = axis(lo, hi);
    let ts = ticks(lo, hi, step);
    let labels: Vec<String> = ts.iter().map(|t| format_tick(*t, step)).collect();
    let lw = labels.iter().map(|l| text_width(l, s, false)).fold(0.0, f32::max);
    let left = x0 + lw + s * 0.6;
    let right = x1;
    let cat_h = s * 1.7;
    let plot_top = top + s * 0.5;
    let plot_bottom = bottom - cat_h;
    if right - left < 10.0 || plot_bottom - plot_top < 10.0 {
        return;
    }
    let ymap = |v: f64| plot_bottom - ((v - lo) / (hi - lo)) as f32 * (plot_bottom - plot_top);
    for (t, l) in ts.iter().zip(&labels) {
        let y = ymap(*t);
        let c = if *t == 0.0 && lo < 0.0 { [style.grid[0], style.grid[1], style.grid[2], style.grid[3].saturating_mul(3)] } else { style.grid };
        out.push(Prim::Line { points: vec![(left, y), (right, y)], width: 0.5, color: c });
        out.push(text(left - s * 0.5, y + s * 0.35, l.clone(), s, style.text, Anchor::End, false));
    }
    let n = data.categories.len().max(1);
    let band = (right - left) / n as f32;
    category_labels(out, &data.categories, left, band, plot_bottom + s * 1.3, style);
    let zero = ymap(0.0f64.clamp(lo, hi));
    let st = stacks(data, stacked);
    let ns = data.series.len().max(1);
    match chart.kind {
        ChartKind::Column => {
            let group = band * 0.72;
            let bw = if stacked { group } else { group / ns as f32 };
            for (si, segs) in st.iter().enumerate() {
                for (i, seg) in segs.iter().enumerate() {
                    let Some((b, t)) = seg else { continue };
                    let gx = left + band * i as f32 + (band - group) / 2.0;
                    let x = if stacked { gx } else { gx + bw * si as f32 };
                    let (ya, yb) = if stacked { (ymap(*b), ymap(*t)) } else { (zero, ymap(*t)) };
                    out.push(rect(x + bw * 0.04, yb, bw * 0.92, ya - yb, style.color(si)));
                }
            }
        }
        ChartKind::Area => {
            let xs = |i: usize| left + band * (i as f32 + 0.5);
            let order: Vec<usize> = (0..st.len()).collect();
            for &si in order.iter().rev().filter(|_| !stacked).chain(order.iter().filter(|_| stacked)) {
                let segs = &st[si];
                let c = style.color(si);
                let fill = if stacked { c } else { [c[0], c[1], c[2], (c[3] as f32 * 0.55) as u8] };
                // One polygon per run of present values.
                let mut run: Vec<(usize, f64, f64)> = vec![];
                let flush = |run: &mut Vec<(usize, f64, f64)>, out: &mut Vec<Prim>| {
                    if run.len() >= 2 {
                        let mut pts: Vec<(f32, f32)> = run.iter().map(|(i, _, t)| (xs(*i), ymap(*t))).collect();
                        let line = pts.clone();
                        pts.extend(run.iter().rev().map(|(i, b, _)| (xs(*i), ymap(*b))));
                        out.push(Prim::Poly { points: pts, fill });
                        out.push(Prim::Line { points: line, width: 1.2, color: c });
                    }
                    run.clear();
                };
                for (i, seg) in segs.iter().enumerate() {
                    match seg {
                        Some((b, t)) => run.push((i, if stacked { *b } else { 0.0f64.clamp(lo, hi) }, *t)),
                        None => flush(&mut run, out),
                    }
                }
                flush(&mut run, out);
            }
        }
        _ => {
            // Lines with point markers.
            let xs = |i: usize| left + band * (i as f32 + 0.5);
            let marks = n <= 40;
            for (si, segs) in st.iter().enumerate() {
                let c = style.color(si);
                let mut run: Vec<(f32, f32)> = vec![];
                for (i, seg) in segs.iter().enumerate() {
                    match seg {
                        Some((_, t)) => run.push((xs(i), ymap(*t))),
                        None => {
                            if run.len() >= 2 {
                                out.push(Prim::Line { points: std::mem::take(&mut run), width: 1.6, color: c });
                            }
                            run.clear();
                        }
                    }
                    if marks && let Some((_, t)) = seg {
                        let r = (s * 0.28).max(1.5);
                        out.push(rect(xs(i) - r, ymap(*t) - r, 2.0 * r, 2.0 * r, c));
                    }
                }
                if run.len() >= 2 {
                    out.push(Prim::Line { points: run, width: 1.6, color: c });
                }
            }
        }
    }
    // The category axis line.
    out.push(Prim::Line { points: vec![(left, zero), (right, zero)], width: 0.8, color: [style.text[0], style.text[1], style.text[2], 160] });
}

/// Category labels under bands, every `k`th one when they would collide.
fn category_labels(out: &mut Vec<Prim>, cats: &[String], left: f32, band: f32, y: f32, style: &ChartStyle) {
    let s = style.size;
    let widest = cats.iter().map(|c| text_width(c, s, false)).fold(0.0, f32::max);
    let every = ((widest + s * 0.8) / band.max(0.1)).ceil().max(1.0) as usize;
    // Labels as wide as `every` bands at most (long labels are cut).
    let room = band * every as f32 - s * 0.6;
    for (i, c) in cats.iter().enumerate() {
        if i % every != 0 {
            continue;
        }
        out.push(text(left + band * (i as f32 + 0.5), y, fit(c, s, room.max(s * 2.0)), s, style.text, Anchor::Middle, false));
    }
}

#[allow(clippy::too_many_arguments)]
fn bar_chart(out: &mut Vec<Prim>, chart: &Chart, data: &ChartData, x0: f32, top: f32, x1: f32, bottom: f32, style: &ChartStyle) {
    let s = style.size;
    let (lo, hi) = data.bounds(chart.stacked);
    let (lo, hi, step) = axis(lo, hi);
    let ts = ticks(lo, hi, step);
    let widest = data.categories.iter().map(|c| text_width(c, s, false)).fold(0.0, f32::max);
    let cat_w = widest.min((x1 - x0) * 0.32);
    let left = x0 + cat_w + s * 0.6;
    let last_label = format_tick(hi, step);
    let right = x1 - text_width(&last_label, s, false) / 2.0;
    let plot_top = top + s * 0.3;
    let plot_bottom = bottom - s * 1.7;
    if right - left < 10.0 || plot_bottom - plot_top < 10.0 {
        return;
    }
    let xmap = |v: f64| left + ((v - lo) / (hi - lo)) as f32 * (right - left);
    // Thin tick labels so they don't touch.
    let label_w = ts.iter().map(|t| text_width(&format_tick(*t, step), s, false)).fold(0.0, f32::max);
    let tick_gap = (right - left) / (ts.len().max(2) - 1) as f32;
    let every = ((label_w + s) / tick_gap.max(0.1)).ceil().max(1.0) as usize;
    for (k, t) in ts.iter().enumerate() {
        let x = xmap(*t);
        out.push(Prim::Line { points: vec![(x, plot_top), (x, plot_bottom)], width: 0.5, color: style.grid });
        if k % every == 0 {
            out.push(text(x, plot_bottom + s * 1.3, format_tick(*t, step), s, style.text, Anchor::Middle, false));
        }
    }
    let n = data.categories.len().max(1);
    let band = (plot_bottom - plot_top) / n as f32;
    let every_cat = ((s * 1.25) / band.max(0.1)).ceil().max(1.0) as usize;
    for (i, c) in data.categories.iter().enumerate() {
        if i % every_cat == 0 {
            out.push(text(left - s * 0.5, plot_top + band * (i as f32 + 0.5) + s * 0.35, fit(c, s, cat_w), s, style.text, Anchor::End, false));
        }
    }
    let zero = xmap(0.0f64.clamp(lo, hi));
    let st = stacks(data, chart.stacked);
    let ns = data.series.len().max(1);
    let group = band * 0.72;
    let bh = if chart.stacked { group } else { group / ns as f32 };
    for (si, segs) in st.iter().enumerate() {
        for (i, seg) in segs.iter().enumerate() {
            let Some((b, t)) = seg else { continue };
            let gy = plot_top + band * i as f32 + (band - group) / 2.0;
            let y = if chart.stacked { gy } else { gy + bh * si as f32 };
            let (xa, xb) = if chart.stacked { (xmap(*b), xmap(*t)) } else { (zero, xmap(*t)) };
            out.push(rect(xa, y + bh * 0.04, xb - xa, bh * 0.92, style.color(si)));
        }
    }
    out.push(Prim::Line { points: vec![(zero, plot_top), (zero, plot_bottom)], width: 0.8, color: [style.text[0], style.text[1], style.text[2], 160] });
}

#[allow(clippy::too_many_arguments)]
fn scatter_chart(out: &mut Vec<Prim>, data: &ChartData, x_series: bool, x0: f32, top: f32, x1: f32, bottom: f32, style: &ChartStyle) {
    let s = style.size;
    let n = data.categories.len();
    let xs: Vec<Option<f64>> = if x_series {
        data.series[0].values.clone()
    } else if data.categories.iter().all(|c| c.trim().parse::<f64>().is_ok()) {
        data.categories.iter().map(|c| c.trim().parse::<f64>().ok()).collect()
    } else {
        (1..=n).map(|i| Some(i as f64)).collect()
    };
    let ys: Vec<&folio_core::Series> = data.series.iter().skip(x_series as usize).collect();
    let xv: Vec<f64> = xs.iter().flatten().copied().filter(|v| v.is_finite()).collect();
    let yv: Vec<f64> = ys.iter().flat_map(|se| se.values.iter().flatten().copied()).filter(|v| v.is_finite()).collect();
    if xv.is_empty() || yv.is_empty() {
        return;
    }
    let (xlo, xhi) = (xv.iter().copied().fold(f64::MAX, f64::min), xv.iter().copied().fold(f64::MIN, f64::max));
    let (ylo, yhi) = (yv.iter().copied().fold(0.0, f64::min), yv.iter().copied().fold(0.0, f64::max));
    let (xlo, xhi, xstep) = axis(xlo, xhi);
    let (ylo, yhi, ystep) = axis(ylo, yhi);
    let yts = ticks(ylo, yhi, ystep);
    let lw = yts.iter().map(|t| text_width(&format_tick(*t, ystep), s, false)).fold(0.0, f32::max);
    let left = x0 + lw + s * 0.6;
    let right = x1 - s;
    let plot_top = top + s * 0.5;
    let plot_bottom = bottom - s * 1.7;
    if right - left < 10.0 || plot_bottom - plot_top < 10.0 {
        return;
    }
    let xmap = |v: f64| left + ((v - xlo) / (xhi - xlo)) as f32 * (right - left);
    let ymap = |v: f64| plot_bottom - ((v - ylo) / (yhi - ylo)) as f32 * (plot_bottom - plot_top);
    for t in &yts {
        let y = ymap(*t);
        out.push(Prim::Line { points: vec![(left, y), (right, y)], width: 0.5, color: style.grid });
        out.push(text(left - s * 0.5, y + s * 0.35, format_tick(*t, ystep), s, style.text, Anchor::End, false));
    }
    let xts = ticks(xlo, xhi, xstep);
    let label_w = xts.iter().map(|t| text_width(&format_tick(*t, xstep), s, false)).fold(0.0, f32::max);
    let gap = (right - left) / (xts.len().max(2) - 1) as f32;
    let every = ((label_w + s) / gap.max(0.1)).ceil().max(1.0) as usize;
    for (k, t) in xts.iter().enumerate() {
        let x = xmap(*t);
        out.push(Prim::Line { points: vec![(x, plot_top), (x, plot_bottom)], width: 0.5, color: style.grid });
        if k % every == 0 {
            out.push(text(x, plot_bottom + s * 1.3, format_tick(*t, xstep), s, style.text, Anchor::Middle, false));
        }
    }
    let r = (s * 0.3).max(1.8);
    for (si, se) in ys.iter().enumerate() {
        let c = style.color(si);
        for (i, v) in se.values.iter().enumerate() {
            if let (Some(Some(x)), Some(y)) = (xs.get(i), v) {
                let (px, py) = (xmap(*x), ymap(*y));
                out.push(Prim::Poly { points: (0..12).map(|k| (px + r * (k as f32 * PI / 6.0).cos(), py + r * (k as f32 * PI / 6.0).sin())).collect(), fill: c });
            }
        }
    }
    out.push(Prim::Line { points: vec![(left, plot_bottom), (right, plot_bottom)], width: 0.8, color: [style.text[0], style.text[1], style.text[2], 160] });
}

fn pie_chart(out: &mut Vec<Prim>, data: &ChartData, x0: f32, top: f32, x1: f32, bottom: f32, style: &ChartStyle) {
    let Some(se) = data.series.first() else { return };
    let (cx, cy) = ((x0 + x1) / 2.0, (top + bottom) / 2.0);
    let r = ((x1 - x0).min(bottom - top) / 2.0 * 0.92).max(2.0);
    let total: f64 = se.values.iter().flatten().filter(|v| **v > 0.0).sum();
    let angles = pie_angles(&se.values);
    let pt = |deg: f32, rr: f32| {
        let a = (deg - 90.0) * PI / 180.0;
        (cx + rr * a.cos(), cy + rr * a.sin())
    };
    for (i, (a, sweep)) in angles.iter().enumerate() {
        if *sweep <= 0.0 {
            continue;
        }
        let c = style.color(i);
        let steps = ((sweep / 3.0).ceil() as usize).max(2);
        let mut pts = vec![(cx, cy)];
        for k in 0..=steps {
            pts.push(pt(a + sweep * k as f32 / steps as f32, r));
        }
        out.push(Prim::Poly { points: pts, fill: c });
    }
    // Thin separators between wedges, in the background colour.
    let sep = style.background.unwrap_or([255, 255, 255, 255]);
    if angles.iter().filter(|a| a.1 > 0.0).count() > 1 {
        for (a, sweep) in &angles {
            if *sweep > 0.0 {
                out.push(Prim::Line { points: vec![(cx, cy), pt(*a, r)], width: 1.0, color: sep });
            }
        }
    }
    // Percentages on wedges big enough to hold them.
    for (i, ((a, sweep), v)) in angles.iter().zip(&se.values).enumerate() {
        if *sweep < 18.0 || total <= 0.0 {
            continue;
        }
        let pct = v.unwrap_or(0.0) / total * 100.0;
        let (lx, ly) = pt(a + sweep / 2.0, r * 0.66);
        let c = if dark(style.color(i)) { [255, 255, 255, 255] } else { [20, 20, 20, 255] };
        out.push(text(lx, ly + style.size * 0.35, format!("{:.0}%", pct), style.size, c, Anchor::Middle, true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::Series;

    fn data() -> ChartData {
        ChartData {
            categories: vec!["Jan".into(), "Feb".into(), "Mar".into(), "Apr".into()],
            series: vec![Series { name: "Sales".into(), values: vec![Some(10.0), Some(14.5), None, Some(-3.0)] }, Series { name: "Costs".into(), values: vec![Some(4.0), Some(5.0), Some(6.0), Some(7.0)] }],
        }
    }

    fn inside(prims: &[Prim], w: f32, h: f32) {
        for p in prims {
            match p {
                Prim::Rect { x, y, w: rw, h: rh, .. } => assert!(*x >= -0.01 && *y >= -0.01 && x + rw <= w + 0.01 && y + rh <= h + 0.01 && *rw >= 0.0 && *rh >= 0.0, "{p:?}"),
                Prim::Line { points, .. } | Prim::Poly { points, .. } => assert!(points.iter().all(|(x, y)| *x >= -0.01 && *x <= w + 0.01 && *y >= -0.01 && *y <= h + 0.01), "{p:?}"),
                Prim::Text { x, y, .. } => assert!(*x >= 0.0 && *x <= w && *y > 0.0 && *y <= h, "{p:?}"),
            }
        }
    }

    #[test]
    fn every_kind_stays_in_its_box() {
        for kind in ChartKind::ALL {
            for stacked in [false, true] {
                let mut c = Chart::new(kind, "'Sheet'!A1:C5");
                c.title = "Monthly figures".into();
                c.stacked = stacked;
                let p = chart_prims(&c, &data(), 360.0, 220.0, &ChartStyle::default());
                assert!(p.len() > 5, "{kind:?}");
                inside(&p, 360.0, 220.0);
                assert!(p.iter().any(|x| matches!(x, Prim::Text { text, bold: true, .. } if text == "Monthly figures")));
                // Legend for two series (pies list categories).
                let names: Vec<&str> = p.iter().filter_map(|x| if let Prim::Text { text, .. } = x { Some(text.as_str()) } else { None }).collect();
                if kind == ChartKind::Pie {
                    assert!(names.contains(&"Jan"));
                } else if kind != ChartKind::Scatter {
                    assert!(names.contains(&"Sales") && names.contains(&"Costs"), "{kind:?} {names:?}");
                }
            }
        }
    }

    #[test]
    fn columns_go_below_zero_for_negatives() {
        let c = Chart::new(ChartKind::Column, "x");
        let p = chart_prims(&c, &data(), 400.0, 240.0, &ChartStyle { series: vec![[1, 1, 1, 255], [2, 2, 2, 255]], ..Default::default() });
        let bars: Vec<(f32, f32)> = p.iter().filter_map(|x| if let Prim::Rect { y, h, fill: [1, 1, 1, 255], .. } = x { Some((*y, *h)) } else { None }).collect();
        // Legend swatch + 3 bars (one value missing).
        assert_eq!(bars.len(), 4);
        let zero_line = p.iter().rev().find_map(|x| if let Prim::Line { points, width, .. } = x { (*width == 0.8).then_some(points[0].1) } else { None }).unwrap();
        // The April bar (negative) starts at the zero line and goes down.
        assert!(bars.iter().any(|(y, _)| (*y - zero_line).abs() < 0.01));
    }

    #[test]
    fn stacked_bars_sum() {
        let mut c = Chart::new(ChartKind::Column, "x");
        c.stacked = true;
        let d = ChartData { categories: vec!["A".into()], series: vec![Series { name: "a".into(), values: vec![Some(30.0)] }, Series { name: "b".into(), values: vec![Some(70.0)] }] };
        let p = chart_prims(&c, &d, 300.0, 300.0, &ChartStyle { series: vec![[1, 1, 1, 255], [2, 2, 2, 255]], ..Default::default() });
        let hs: Vec<f32> = p.iter().filter_map(|x| if let Prim::Rect { h, w, .. } = x { (*w > 20.0).then_some(*h) } else { None }).collect();
        assert_eq!(hs.len(), 2);
        assert!((hs[1] / hs[0] - 70.0 / 30.0).abs() < 0.01);
    }

    #[test]
    fn pie_angles_sum_to_a_turn() {
        let a = pie_angles(&[Some(1.0), Some(2.0), None, Some(-4.0), Some(3.0)]);
        let total: f32 = a.iter().map(|x| x.1).sum();
        assert!((total - 360.0).abs() < 0.01);
        assert_eq!(a[2].1, 0.0);
        assert!((a[1].0 - 60.0).abs() < 0.01);
    }

    #[test]
    fn empty_data_says_so() {
        let c = Chart::new(ChartKind::Line, "'Sheet'!A1:B2");
        let p = chart_prims(&c, &ChartData::default(), 300.0, 200.0, &ChartStyle::default());
        assert!(matches!(&p[0], Prim::Text { text, .. } if text == "No numbers in Sheet!A1:B2"));
    }

    #[test]
    fn crowded_labels_thin_out() {
        let d = ChartData { categories: (0..60).map(|i| format!("Category {i}")).collect(), series: vec![Series { name: "v".into(), values: (0..60).map(|i| Some(i as f64)).collect() }] };
        let p = chart_prims(&Chart::new(ChartKind::Column, "x"), &d, 400.0, 200.0, &ChartStyle::default());
        let labels = p.iter().filter(|x| matches!(x, Prim::Text { text, .. } if text.starts_with("Category"))).count();
        assert!(labels > 1 && labels < 15, "{labels}");
    }

    #[test]
    fn ticks_read_well() {
        assert_eq!(format_tick(2500.0, 500.0), "2500");
        assert_eq!(format_tick(25000.0, 5000.0), "25k");
        assert_eq!(format_tick(0.25, 0.05), "0.25");
        assert_eq!(format_tick(3_000_000.0, 1_000_000.0), "3M");
    }
}
