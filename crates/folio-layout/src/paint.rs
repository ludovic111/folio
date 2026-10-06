//! Painting laid-out pages, slides and charts on any surface: the PNG canvas
//! ([`crate::raster::Canvas`]) and the PDF export implement [`Painter`], and the functions here
//! decide what goes where, so both draw the same thing.

use folio_core::deck::{Deck, Shape, ShapeKind, Slide};
use folio_core::{Document, Id};

use crate::chart::{Anchor, ChartStyle, Prim, chart_prims};
use crate::doc::{DocLayout, Placed};
use crate::fonts::{Face, Fonts};
use crate::slide::{layout_shape_text, layout_slide_table, theme_chart_style};
use crate::text::{DecoKind, Glyph, ParaLayout, shape_label};
use crate::{Rgba, parse_hex};

/// A drawing surface in points (y down).
pub trait Painter {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba);
    fn poly(&mut self, pts: &[(f32, f32)], c: Rgba);
    /// An open polyline.
    fn line(&mut self, pts: &[(f32, f32)], width: f32, c: Rgba);
    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, c: Rgba);
    /// Glyphs (their x, y relative to `ox`, `oy`), with the text their clusters index.
    fn glyphs(&mut self, fonts: &mut Fonts, faces: &[Face], glyphs: &[Glyph], text: &str, ox: f32, oy: f32);
    /// An image stretched over a box; false when it can't be drawn.
    fn image(&mut self, doc: &Document, media: &Id, x: f32, y: f32, w: f32, h: f32) -> bool;
    /// Rotates what follows by `deg` degrees clockwise around (`cx`, `cy`) until [`Painter::pop`].
    fn push_rotation(&mut self, deg: f32, cx: f32, cy: f32);
    /// Clips what follows to a box until [`Painter::pop`].
    fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32);
    fn pop(&mut self);
    /// A clickable link over a box (PDF).
    fn link(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _url: &str) {}
}

/// Lines `lines` of a paragraph whose layout origin is at (`ox`, `oy`) (line y values are
/// added as they are: pass `oy = top - layout.lines[lines.start].y` for a slice).
pub fn paint_para(p: &mut dyn Painter, fonts: &mut Fonts, l: &ParaLayout, ox: f32, oy: f32, lines: std::ops::Range<usize>) {
    let lines = lines.start.min(l.lines.len())..lines.end.min(l.lines.len());
    for line in &l.lines[lines.clone()] {
        for d in line.decos.iter().filter(|d| d.is_background()) {
            p.rect(ox + d.x0, oy + d.y, d.x1 - d.x0, d.h, d.color);
        }
    }
    if lines.start == 0
        && let Some(m) = &l.marker
    {
        if let Some((x, s, checked)) = m.checkbox
            && let Some(first) = l.lines.first()
        {
            let c = first.glyphs.first().map(|g| g.color).unwrap_or([60, 60, 60, 255]);
            let (bx, by) = (ox + x, oy + first.baseline - s);
            p.stroke_rect(bx, by, s, s, (s * 0.09).max(0.6), c);
            if checked {
                p.line(&[(bx + s * 0.2, by + s * 0.52), (bx + s * 0.42, by + s * 0.74), (bx + s * 0.8, by + s * 0.26)], (s * 0.12).max(0.8), c);
            }
        }
        p.glyphs(fonts, &l.faces, &m.glyphs, &m.text, ox, oy);
    }
    for line in &l.lines[lines] {
        p.glyphs(fonts, &l.faces, &line.glyphs, &l.text, ox, oy);
        for d in line.decos.iter().filter(|d| !d.is_background()) {
            p.rect(ox + d.x0, oy + d.y, d.x1 - d.x0, d.h, d.color);
            if let DecoKind::Link(url) = &d.kind {
                p.link(ox + d.x0, oy + line.y, d.x1 - d.x0, line.height, url);
            }
        }
    }
}

/// A line of plain text in the sans face; `x` per `anchor`, `y` the baseline.
#[allow(clippy::too_many_arguments)]
pub fn paint_label(p: &mut dyn Painter, fonts: &mut Fonts, text: &str, x: f32, y: f32, size: f32, color: Rgba, anchor: Anchor, family: &str, weight: u16, italic: bool) -> f32 {
    let (glyphs, faces, w) = shape_label(fonts, text, family, weight, italic, size, color);
    let dx = match anchor {
        Anchor::Start => 0.0,
        Anchor::Middle => -w / 2.0,
        Anchor::End => -w,
    };
    p.glyphs(fonts, &faces, &glyphs, text, x + dx, y);
    w
}

/// Chart primitives with their box's top-left at (`ox`, `oy`).
pub fn paint_prims(p: &mut dyn Painter, fonts: &mut Fonts, prims: &[Prim], ox: f32, oy: f32) {
    for pr in prims {
        match pr {
            Prim::Rect { x, y, w, h, fill } => p.rect(ox + x, oy + y, *w, *h, *fill),
            Prim::Line { points, width, color } => p.line(&points.iter().map(|(x, y)| (ox + x, oy + y)).collect::<Vec<_>>(), *width, *color),
            Prim::Poly { points, fill } => p.poly(&points.iter().map(|(x, y)| (ox + x, oy + y)).collect::<Vec<_>>(), *fill),
            Prim::Text { x, y, text, size, color, anchor, bold } => {
                paint_label(p, fonts, text, ox + x, oy + y, *size, *color, *anchor, "sans", if *bold { 600 } else { 400 }, false);
            }
        }
    }
}

/// A chart, read from its sheet now, in a box.
#[allow(clippy::too_many_arguments)]
pub fn paint_chart(p: &mut dyn Painter, fonts: &mut Fonts, doc: &Document, chart: &folio_core::Chart, x: f32, y: f32, w: f32, h: f32, style: &ChartStyle) {
    let data = folio_core::links::chart_data(doc, chart).unwrap_or_default();
    let prims = chart_prims(chart, &data, w, h, style);
    paint_prims(p, fonts, &prims, x, y);
}

/// A placed table row: fills, text, then grid lines.
pub fn paint_table_row(p: &mut dyn Painter, fonts: &mut Fonts, item: &Placed) {
    let Placed::TableRow { y, height, cells, border, border_width, .. } = item else { return };
    for c in cells {
        if let Some(f) = c.fill {
            p.rect(c.x, *y, c.w, *height, f);
        }
        paint_para(p, fonts, &c.layout, c.tx, c.ty, 0..c.layout.lines.len());
    }
    for c in cells {
        p.stroke_rect(c.x, *y, c.w, *height, *border_width, *border);
    }
}

/// One slide shape (any kind), in slide points.
pub fn paint_shape(p: &mut dyn Painter, fonts: &mut Fonts, doc: &Document, deck: &Deck, sh: &Shape) {
    let rotated = sh.rotation != 0.0;
    if rotated {
        p.push_rotation(sh.rotation, sh.x + sh.w / 2.0, sh.y + sh.h / 2.0);
    }
    let fill = sh.fill.as_deref().map(|f| parse_hex(f, [0, 0, 0, 0]));
    let line_c = sh.line.as_deref().map(|f| parse_hex(f, [0, 0, 0, 255]));
    let lw = if sh.line_width > 0.0 { sh.line_width } else { 1.5 };
    let accent = parse_hex(&deck.theme.accent, [0, 0, 0, 255]);
    let outline = |p: &mut dyn Painter, pts: &[(f32, f32)]| {
        if let Some(lc) = line_c {
            let mut pts = pts.to_vec();
            pts.push(pts[0]);
            p.line(&pts, lw, lc);
        }
    };
    match &sh.kind {
        ShapeKind::Rect | ShapeKind::Text => {
            if let Some(f) = fill {
                p.rect(sh.x, sh.y, sh.w, sh.h, f);
            }
            if let Some(lc) = line_c {
                p.stroke_rect(sh.x, sh.y, sh.w, sh.h, lw, lc);
            }
        }
        ShapeKind::Ellipse => {
            let pts: Vec<(f32, f32)> = (0..96)
                .map(|k| {
                    let a = k as f32 / 96.0 * std::f32::consts::TAU;
                    (sh.x + sh.w / 2.0 * (1.0 + a.cos()), sh.y + sh.h / 2.0 * (1.0 + a.sin()))
                })
                .collect();
            if let Some(f) = fill {
                p.poly(&pts, f);
            }
            outline(p, &pts);
        }
        ShapeKind::Triangle => {
            let pts = [(sh.x + sh.w / 2.0, sh.y), (sh.x + sh.w, sh.y + sh.h), (sh.x, sh.y + sh.h)];
            if let Some(f) = fill {
                p.poly(&pts, f);
            }
            outline(p, &pts);
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            let col = line_c.or(fill).unwrap_or(accent);
            let (a, b) = ((sh.x, sh.y), (sh.x + sh.w, sh.y + sh.h));
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt().max(0.01);
            let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
            let arrow = matches!(sh.kind, ShapeKind::Arrow);
            let head = (lw * 4.0).max(8.0);
            let end = if arrow { (b.0 - ux * head * 0.8, b.1 - uy * head * 0.8) } else { b };
            p.line(&[a, end], lw, col);
            if arrow {
                let (px, py) = (-uy, ux);
                p.poly(&[b, (b.0 - ux * head + px * head * 0.5, b.1 - uy * head + py * head * 0.5), (b.0 - ux * head - px * head * 0.5, b.1 - uy * head - py * head * 0.5)], col);
            }
        }
        ShapeKind::Image { media } => {
            if !p.image(doc, media, sh.x, sh.y, sh.w, sh.h) {
                p.rect(sh.x, sh.y, sh.w, sh.h, [128, 128, 128, 60]);
            }
            if let Some(lc) = line_c {
                p.stroke_rect(sh.x, sh.y, sh.w, sh.h, lw, lc);
            }
        }
        ShapeKind::Chart { chart } => {
            if let Some(f) = fill {
                p.rect(sh.x, sh.y, sh.w, sh.h, f);
            }
            let mut style = theme_chart_style(&deck.theme);
            style.size = (sh.text_size * 0.7).clamp(8.0, 24.0);
            paint_chart(p, fonts, doc, chart, sh.x, sh.y, sh.w, sh.h, &style);
        }
        ShapeKind::Table { table } => {
            for row in layout_slide_table(fonts, doc, deck, sh, table) {
                paint_table_row(p, fonts, &row);
            }
        }
    }
    if !sh.text.is_empty() && sh.takes_text() {
        let st = layout_shape_text(fonts, deck, sh);
        for (y, l) in &st.paras {
            paint_para(p, fonts, l, st.x, *y, 0..l.lines.len());
        }
    }
    if rotated {
        p.pop();
    }
}

/// A whole slide, in slide points.
pub fn paint_slide(p: &mut dyn Painter, fonts: &mut Fonts, doc: &Document, deck: &Deck, slide: &Slide) {
    let bg = parse_hex(slide.background.as_deref().unwrap_or(&deck.theme.background), [255, 255, 255, 255]);
    p.rect(0.0, 0.0, deck.size[0], deck.size[1], bg);
    for sh in &slide.shapes {
        paint_shape(p, fonts, doc, deck, sh);
    }
}

/// Printed page `n` (0-based) of document page `page`, laid out as `layout`. The page is
/// painted white first unless `background` is false.
pub fn paint_doc_page(p: &mut dyn Painter, fonts: &mut Fonts, doc: &Document, page: usize, layout: &DocLayout, n: usize, background: bool) {
    if background {
        p.rect(0.0, 0.0, layout.width, layout.height, [255, 255, 255, 255]);
    }
    let Some(pl) = layout.pages.get(n) else { return };
    let blocks = doc.pages.get(page).and_then(|pg| pg.doc()).map(|t| &t.blocks);
    for it in &pl.items {
        match it {
            Placed::Para { x, y, layout: l, lines, .. } => {
                let top = l.lines.get(lines.start).map(|ln| ln.y).unwrap_or(0.0);
                paint_para(p, fonts, l, *x, y - top, lines.clone());
            }
            Placed::TableRow { .. } => paint_table_row(p, fonts, it),
            Placed::Image { x, y, w, h, media, .. } => {
                if !p.image(doc, media, *x, *y, *w, *h) {
                    p.rect(*x, *y, *w, *h, [0, 0, 0, 20]);
                }
            }
            Placed::Caption { x, y, layout: l, .. } => paint_para(p, fonts, l, *x, *y, 0..l.lines.len()),
            Placed::Chart { block, x, y, w, h } => {
                if let Some(folio_core::text::Block::Chart(cb)) = blocks.and_then(|b| b.get(*block)) {
                    paint_chart(p, fonts, doc, &cb.chart, *x, *y, *w, *h, &ChartStyle::default());
                }
            }
            Placed::PageBreak { .. } => {}
        }
    }
    if let Some((y, l)) = &pl.header {
        paint_para(p, fonts, l, layout.left, *y, 0..l.lines.len());
    }
    if let Some((y, l)) = &pl.footer {
        paint_para(p, fonts, l, layout.left, *y, 0..l.lines.len());
    }
    if let Some((x, y, w)) = pl.footnote_rule {
        p.rect(x, y, w, 0.5, [0, 0, 0, 160]);
    }
    for (y, l) in &pl.footnotes {
        paint_para(p, fonts, l, layout.left, *y, 0..l.lines.len());
    }
}
