//! Charts, slides and document pages as PNG pictures (tiny-skia), for exports and thumbnails.
//!
//! Text is drawn from the glyph outlines of the faces the layout chose, so a picture matches
//! the window and the PDF.

use std::collections::HashMap;

use cosmic_text::Command;
use folio_core::{Chart, ChartData, Document, Id};
use tiny_skia::{FillRule, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

use crate::chart::{ChartStyle, chart_prims};
use crate::doc::layout_doc;
use crate::fonts::{Face, Fonts};
use crate::paint::{Painter, paint_doc_page, paint_prims, paint_slide};
use crate::text::Glyph;
use crate::Rgba;

/// A pixmap with a transform from points to pixels.
pub struct Canvas {
    pub pixmap: Pixmap,
    pub ts: Transform,
    stack: Vec<(Transform, Option<Mask>)>,
    mask: Option<Mask>,
    images: HashMap<Id, Option<Pixmap>>,
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

/// Decodes an image file into a premultiplied pixmap.
pub fn decode_pixmap(bytes: &[u8]) -> Option<Pixmap> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut data = img.into_raw();
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3] as u16;
        if a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
    }
    Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)
}

fn polyline(pts: &[(f32, f32)], close: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        if i == 0 { pb.move_to(*x, *y) } else { pb.line_to(*x, *y) }
    }
    if close {
        pb.close();
    }
    pb.finish()
}

impl Canvas {
    /// A `w`×`h` pixel canvas where one point is `scale` pixels.
    pub fn new(w: u32, h: u32, scale: f32) -> Option<Canvas> {
        Some(Canvas { pixmap: Pixmap::new(w.max(1), h.max(1))?, ts: Transform::from_scale(scale, scale), stack: vec![], mask: None, images: HashMap::new() })
    }

    pub fn png(&self) -> Vec<u8> {
        self.pixmap.encode_png().unwrap_or_default()
    }

    pub fn clear(&mut self, c: Rgba) {
        self.pixmap.fill(tiny_skia::Color::from_rgba8(c[0], c[1], c[2], c[3]));
    }

    /// One glyph from its outline, `x`, `y` its origin on the baseline.
    pub fn glyph(&mut self, fonts: &mut Fonts, face: &Face, id: u16, x: f32, y: f32, size: f32, c: Rgba) {
        // Outlines at the pixel size for crisp curves.
        let px = (self.ts.sx * self.ts.sx + self.ts.ky * self.ts.ky).sqrt().max(0.01);
        let Some(cmds) = fonts.outline(face, id, size * px) else { return };
        if cmds.is_empty() {
            return;
        }
        let mut pb = PathBuilder::new();
        for cmd in &cmds {
            match cmd {
                Command::MoveTo(p) => pb.move_to(p.x, -p.y),
                Command::LineTo(p) => pb.line_to(p.x, -p.y),
                Command::QuadTo(a, b) => pb.quad_to(a.x, -a.y, b.x, -b.y),
                Command::CurveTo(a, b, p) => pb.cubic_to(a.x, -a.y, b.x, -b.y, p.x, -p.y),
                Command::Close => pb.close(),
            }
        }
        if let Some(path) = pb.finish() {
            let ts = self.ts.pre_translate(x, y).pre_scale(1.0 / px, 1.0 / px);
            self.pixmap.fill_path(&path, &paint(c), FillRule::Winding, ts, self.mask.as_ref());
        }
    }
}

impl Painter for Canvas {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            self.pixmap.fill_rect(r, &paint(c), self.ts, self.mask.as_ref());
        }
    }

    fn poly(&mut self, pts: &[(f32, f32)], c: Rgba) {
        if let Some(p) = polyline(pts, true) {
            self.pixmap.fill_path(&p, &paint(c), FillRule::Winding, self.ts, self.mask.as_ref());
        }
    }

    fn line(&mut self, pts: &[(f32, f32)], width: f32, c: Rgba) {
        if let Some(p) = polyline(pts, false) {
            let stroke = Stroke { width, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
            self.pixmap.stroke_path(&p, &paint(c), &stroke, self.ts, self.mask.as_ref());
        }
    }

    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, c: Rgba) {
        if let Some(r) = Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)) {
            let p = PathBuilder::from_rect(r);
            self.pixmap.stroke_path(&p, &paint(c), &Stroke { width, ..Default::default() }, self.ts, self.mask.as_ref());
        }
    }

    fn glyphs(&mut self, fonts: &mut Fonts, faces: &[Face], glyphs: &[Glyph], _text: &str, ox: f32, oy: f32) {
        for g in glyphs {
            if let Some(face) = faces.get(g.face as usize) {
                self.glyph(fonts, face, g.id, ox + g.x, oy + g.y, g.size, g.color);
            }
        }
    }

    fn image(&mut self, doc: &Document, media: &Id, x: f32, y: f32, w: f32, h: f32) -> bool {
        let pm = self.images.entry(media.clone()).or_insert_with(|| doc.media.get(media).and_then(|m| decode_pixmap(&m.bytes)));
        let Some(pm) = pm.as_ref() else {
            return false;
        };
        let ts = self.ts.pre_translate(x, y).pre_scale(w / pm.width() as f32, h / pm.height() as f32);
        let pp = PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
        let pm = pm.clone();
        self.pixmap.draw_pixmap(0, 0, pm.as_ref(), &pp, ts, self.mask.as_ref());
        true
    }

    fn push_rotation(&mut self, deg: f32, cx: f32, cy: f32) {
        self.stack.push((self.ts, self.mask.clone()));
        self.ts = self.ts.pre_rotate_at(deg, cx, cy);
    }

    fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.stack.push((self.ts, self.mask.clone()));
        if let (Some(r), Some(mut m)) = (Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)), Mask::new(self.pixmap.width(), self.pixmap.height())) {
            m.fill_path(&PathBuilder::from_rect(r), FillRule::Winding, true, self.ts);
            if let Some(old) = &self.mask {
                // Intersect with the clip already in force.
                for (a, b) in m.data_mut().iter_mut().zip(old.data()) {
                    *a = ((*a as u16 * *b as u16) / 255) as u8;
                }
            }
            self.mask = Some(m);
        }
    }

    fn pop(&mut self) {
        if let Some((ts, mask)) = self.stack.pop() {
            self.ts = ts;
            self.mask = mask;
        }
    }
}

/// A chart as a PNG `w`×`h` pixels (one point per pixel: set `style.size` for the scale).
/// Uses the shared font system: don't call it while holding [`Fonts::shared`].
pub fn chart_png(chart: &Chart, data: &ChartData, w: u32, h: u32, style: &ChartStyle) -> Vec<u8> {
    chart_png_with(&mut Fonts::shared(), chart, data, w, h, style)
}

/// [`chart_png`] with a font system of your own.
pub fn chart_png_with(fonts: &mut Fonts, chart: &Chart, data: &ChartData, w: u32, h: u32, style: &ChartStyle) -> Vec<u8> {
    let Some(mut c) = Canvas::new(w, h, 1.0) else { return vec![] };
    let prims = chart_prims(chart, data, w as f32, h as f32, style);
    paint_prims(&mut c, fonts, &prims, 0.0, 0.0);
    c.png()
}

/// A slide of deck page `deck_page` as a PNG `width_px` wide (its height from the slide size).
pub fn slide_png(fonts: &mut Fonts, doc: &Document, deck_page: usize, slide: usize, width_px: u32) -> Vec<u8> {
    let Some(deck) = doc.pages.get(deck_page).and_then(|p| p.deck()) else { return vec![] };
    let Some(s) = deck.slides.get(slide) else { return vec![] };
    let scale = width_px as f32 / deck.size[0].max(1.0);
    let h = (deck.size[1] * scale).round().max(1.0) as u32;
    let Some(mut c) = Canvas::new(width_px, h, scale) else { return vec![] };
    paint_slide(&mut c, fonts, doc, deck, s);
    c.png()
}

/// Printed page `page_number` (0-based) of document page `page` as a PNG `width_px` wide.
pub fn page_png(fonts: &mut Fonts, doc: &Document, page: usize, page_number: usize, width_px: u32) -> Vec<u8> {
    let layout = layout_doc(fonts, doc, page);
    if layout.pages.is_empty() {
        return vec![];
    }
    let scale = width_px as f32 / layout.width.max(1.0);
    let h = (layout.height * scale).round().max(1.0) as u32;
    let Some(mut c) = Canvas::new(width_px, h, scale) else { return vec![] };
    paint_doc_page(&mut c, fonts, doc, page, &layout, page_number.min(layout.pages.len() - 1), true);
    c.png()
}

/// Width of the row-number column and height of the column-letter row in a sheet picture, in points.
const SHEET_HEAD: (f32, f32) = (36.0, 20.0);

/// The text a sheet cell shows and how it is drawn.
struct CellText {
    text: String,
    size: f32,
    weight: u16,
    italic: bool,
    color: Rgba,
    anchor: crate::chart::Anchor,
    /// A number (or date) that must fit its column, or show `###`.
    numeric: bool,
}

fn cell_text(cell: &folio_core::sheet::Cell) -> CellText {
    use crate::chart::Anchor;
    use folio_calc::Value;
    use folio_core::text::Align;
    let f = &cell.format;
    let error = matches!(cell.value, Value::Error(_));
    let numeric = matches!(cell.value, Value::Number(_));
    let anchor = match f.align {
        Some(Align::Center) => Anchor::Middle,
        Some(Align::Right) => Anchor::End,
        Some(Align::Left) | Some(Align::Justify) => Anchor::Start,
        None if numeric => Anchor::End,
        None if matches!(cell.value, Value::Bool(_)) || error => Anchor::Middle,
        None => Anchor::Start,
    };
    let color = if error { [200, 30, 30, 255] } else { f.color.as_deref().map(|c| crate::parse_hex(c, [20, 20, 20, 255])).unwrap_or([20, 20, 20, 255]) };
    CellText { text: cell.display(), size: f.size.unwrap_or(10.0), weight: if f.bold || error { 600 } else { 400 }, italic: f.italic, color, anchor, numeric }
}

/// Inner padding of a sheet cell, in points.
const CELL_PAD: f32 = 4.0;

/// Number cells too narrow for what they show (they draw `###`), as `(row, col)` in `range`.
pub fn sheet_narrow_cells(fonts: &mut Fonts, sheet: &folio_core::sheet::Sheet, range: folio_calc::Range) -> Vec<folio_calc::Addr> {
    let mut out = vec![];
    for (a, cell) in sheet.cells.iter() {
        if !range.contains(*a) {
            continue;
        }
        let t = cell_text(cell);
        if !t.numeric || t.text.is_empty() {
            continue;
        }
        let (_, _, w) = crate::text::shape_label(fonts, &t.text, "sans", t.weight, t.italic, t.size, t.color);
        if w > sheet.col_width(a.col) - 2.0 * CELL_PAD {
            out.push(*a);
        }
    }
    out
}

/// A range of a sheet page as a PNG, the way the grid shows it: column letters and row numbers,
/// gridlines, fills, borders, each cell's shown value in its format (errors in red, `###` for a
/// number its column is too narrow for), and the sheet's charts that fall in the range. One sheet
/// pixel is one point; `scale` pixels per point. Rows a filter hides are left out.
pub fn sheet_png(fonts: &mut Fonts, doc: &Document, page: usize, range: folio_calc::Range, scale: f32) -> Vec<u8> {
    use crate::chart::Anchor;
    use crate::paint::{paint_chart, paint_label};
    let Some(sheet) = doc.pages.get(page).and_then(|p| p.sheet()) else { return vec![] };
    let hidden = sheet.hidden_rows();
    let cols: Vec<u32> = (range.start.col..=range.end.col).collect();
    let rows: Vec<u32> = (range.start.row..=range.end.row).filter(|r| !hidden.contains(r)).collect();
    let (hw, hh) = SHEET_HEAD;
    let mut xs = vec![hw];
    for c in &cols {
        xs.push(xs[xs.len() - 1] + sheet.col_width(*c));
    }
    let mut ys = vec![hh];
    for r in &rows {
        ys.push(ys[ys.len() - 1] + sheet.row_height(*r));
    }
    let (w, h) = (xs[xs.len() - 1], ys[ys.len() - 1]);
    let Some(mut c) = Canvas::new((w * scale).ceil() as u32, (h * scale).ceil() as u32, scale) else { return vec![] };
    c.clear([255, 255, 255, 255]);
    let head = [236, 236, 236, 255];
    let grid = [210, 210, 210, 255];
    let ink = [20, 20, 20, 255];
    c.rect(0.0, 0.0, w, hh, head);
    c.rect(0.0, 0.0, hw, h, head);
    // Fills first, then the grid over them, then text and borders.
    for (ri, r) in rows.iter().enumerate() {
        for (ci, col) in cols.iter().enumerate() {
            if let Some(fill) = sheet.cell(folio_calc::Addr::new(*r, *col)).and_then(|cell| cell.format.fill.as_deref()) {
                c.rect(xs[ci], ys[ri], xs[ci + 1] - xs[ci], ys[ri + 1] - ys[ri], crate::parse_hex(fill, [255, 255, 255, 255]));
            }
        }
    }
    let line = 1.0 / scale.max(0.1);
    for x in &xs {
        c.line(&[(*x, 0.0), (*x, if sheet.gridlines { h } else { hh })], line, grid);
    }
    for y in &ys {
        c.line(&[(0.0, *y), (if sheet.gridlines { w } else { hw }, *y)], line, grid);
    }
    for (ci, col) in cols.iter().enumerate() {
        paint_label(&mut c, fonts, &folio_calc::col_name(*col), (xs[ci] + xs[ci + 1]) / 2.0, hh - 6.0, 9.0, [90, 90, 90, 255], Anchor::Middle, "sans", 500, false);
    }
    for (ri, r) in rows.iter().enumerate() {
        paint_label(&mut c, fonts, &(r + 1).to_string(), hw - CELL_PAD, (ys[ri] + ys[ri + 1]) / 2.0 + 3.0, 9.0, [90, 90, 90, 255], Anchor::End, "sans", 500, false);
    }
    for (ri, r) in rows.iter().enumerate() {
        for (ci, col) in cols.iter().enumerate() {
            let Some(cell) = sheet.cell(folio_calc::Addr::new(*r, *col)) else { continue };
            let (x0, x1, y0, y1) = (xs[ci], xs[ci + 1], ys[ri], ys[ri + 1]);
            let mut t = cell_text(cell);
            if !t.text.is_empty() {
                // Text runs on over empty neighbours, like the grid; numbers never do.
                let mut right = x1;
                if !t.numeric && t.anchor == Anchor::Start {
                    let mut k = ci + 1;
                    while k < cols.len() && sheet.cell(folio_calc::Addr::new(*r, cols[k])).is_none_or(|n| n.input.is_empty()) {
                        right = xs[k + 1];
                        k += 1;
                    }
                }
                if t.numeric {
                    let (_, _, tw) = crate::text::shape_label(fonts, &t.text, "sans", t.weight, t.italic, t.size, t.color);
                    if tw > x1 - x0 - 2.0 * CELL_PAD {
                        t.text = "#".repeat(((x1 - x0 - 2.0 * CELL_PAD) / (t.size * 0.6)).max(1.0) as usize);
                    }
                }
                let x = match t.anchor {
                    Anchor::Start => x0 + CELL_PAD,
                    Anchor::Middle => (x0 + x1) / 2.0,
                    Anchor::End => x1 - CELL_PAD,
                };
                c.push_clip(x0, y0, right - x0, y1 - y0);
                paint_label(&mut c, fonts, &t.text, x, (y0 + y1) / 2.0 + t.size * 0.35, t.size, t.color, t.anchor, "sans", t.weight, t.italic);
                c.pop();
            }
            let b = &cell.format.border;
            for (side, pts) in [('t', [(x0, y0), (x1, y0)]), ('b', [(x0, y1), (x1, y1)]), ('l', [(x0, y0), (x0, y1)]), ('r', [(x1, y0), (x1, y1)])] {
                if b.contains(side) {
                    c.line(&pts, 1.0, ink);
                }
            }
        }
    }
    // Charts float over the grid in pixels from A1: place those that show in this range.
    let ox: f32 = (0..range.start.col).map(|col| sheet.col_width(col)).sum();
    let oy: f32 = (0..range.start.row).filter(|r| !hidden.contains(r)).map(|r| sheet.row_height(r)).sum();
    for ch in &sheet.charts {
        let (cx, cy) = (hw + ch.x - ox, hh + ch.y - oy);
        if cx >= w || cy >= h || cx + ch.w <= hw || cy + ch.h <= hh {
            continue;
        }
        c.push_clip(hw, hh, w - hw, h - hh);
        c.rect(cx, cy, ch.w, ch.h, [255, 255, 255, 255]);
        c.stroke_rect(cx, cy, ch.w, ch.h, line, grid);
        paint_chart(&mut c, fonts, doc, &ch.chart, cx, cy, ch.w, ch.h, &ChartStyle { background: Some([255, 255, 255, 255]), size: 10.0, ..Default::default() });
        c.pop();
    }
    c.png()
}

/// The size in points of [`sheet_png`]'s picture of `range` (before `scale`).
pub fn sheet_png_size(sheet: &folio_core::sheet::Sheet, range: folio_calc::Range) -> (f32, f32) {
    let hidden = sheet.hidden_rows();
    let w: f32 = SHEET_HEAD.0 + (range.start.col..=range.end.col).map(|c| sheet.col_width(c)).sum::<f32>();
    let h: f32 = SHEET_HEAD.1 + (range.start.row..=range.end.row).filter(|r| !hidden.contains(r)).map(|r| sheet.row_height(r)).sum::<f32>();
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::deck::{Shape, ShapeKind, Slide, SlideLayout};
    use folio_core::text::{Align, Block, ListKind, ParaStyle};
    use folio_core::{ChartKind, PageKind, Paragraph, Run, RunStyle, Series};

    fn out(name: &str, png: &[u8]) {
        if let Ok(dir) = std::env::var("FOLIO_PNG_OUT") {
            std::fs::write(std::path::Path::new(&dir).join(name), png).unwrap();
        }
    }

    fn ink_pixels(png: &[u8]) -> usize {
        let img = image::load_from_memory(png).unwrap().to_rgba8();
        img.pixels().filter(|p| p[3] > 0 && (p[0] as u16 + p[1] as u16 + p[2] as u16) < 300).count()
    }

    #[test]
    fn chart_picture_has_ink() {
        let mut f = crate::doc::tests::fonts();
        let data = ChartData {
            categories: vec!["Jan".into(), "Feb".into(), "Mar".into(), "Apr".into()],
            series: vec![Series { name: "North".into(), values: vec![Some(120.0), Some(135.0), Some(150.0), Some(160.0)] }, Series { name: "South".into(), values: vec![Some(80.0), Some(95.0), Some(70.0), Some(110.0)] }],
        };
        for kind in ChartKind::ALL {
            let mut c = Chart::new(kind, "'Sales'!A1:C5");
            c.title = format!("{} chart", kind.label());
            let png = chart_png_with(&mut f, &c, &data, 480, 300, &ChartStyle { background: Some([255, 255, 255, 255]), size: 11.0, ..Default::default() });
            assert!(ink_pixels(&png) > 500, "{kind:?}");
            out(&format!("chart-{}.png", kind.id()), &png);
        }
    }

    #[test]
    fn page_and_slide_pictures() {
        let mut f = crate::doc::tests::fonts();
        let mut doc = crate::doc::tests::doc_with(vec![
            Block::Paragraph(Paragraph::new(ParaStyle::Title, "Raster check")),
            Block::Paragraph(Paragraph::with_runs(ParaStyle::Normal, vec![Run::plain("Plain, "), Run::bold("bold"), Run::plain(", "), Run::italic("italic"), Run::plain(" and "), Run::styled("underlined", RunStyle { underline: true, ..Default::default() }), Run::plain(" text that wraps across the width of the page so the lines can be checked by eye.")]).align(Align::Justify)),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "A bullet").list(ListKind::Bullet, 0)),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "A number").list(ListKind::Number, 0)),
            Block::Paragraph(Paragraph::new(ParaStyle::Quote, "Quoted in the serif face.")),
        ]);
        let png = page_png(&mut f, &doc, 0, 0, 800);
        assert!(ink_pixels(&png) > 2000);
        out("page.png", &png);
        let di = doc.add_page(PageKind::Deck, Some("Deck"), None).unwrap();
        {
            let d = doc.page_mut(di).deck_mut().unwrap();
            let mut s = Slide::with_layout(SlideLayout::TitleContent, d.size, "A slide title", "First point\nSecond point");
            let mut e = Shape::new(ShapeKind::Ellipse, 700.0, 200.0, 160.0, 160.0);
            e.fill = Some("#0a0a0a".into());
            s.shapes.push(e);
            let mut a = Shape::new(ShapeKind::Arrow, 500.0, 420.0, 180.0, -60.0);
            a.line = Some("#0a0a0a".into());
            a.line_width = 3.0;
            s.shapes.push(a);
            d.slides = vec![s];
        }
        let png = slide_png(&mut f, &doc, di, 0, 960);
        assert!(ink_pixels(&png) > 2000);
        out("slide.png", &png);
    }
}
