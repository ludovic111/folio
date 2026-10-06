//! PDF: every kind of page, written with krilla.
//!
//! Documents are paginated by `folio-layout` and every glyph is placed where the window puts
//! it, in the same embedded (subset) faces; links stay clickable. Sheets print their used
//! range as a grid on A4 landscape pages (rows and then columns split over pages, charts
//! after the grid). Decks give one page per slide at the slide's size. Opening PDF files
//! isn't supported.

use std::collections::{BTreeSet, HashMap};

use folio_calc::{Addr, Value};
use folio_core::{Document, Id, PageBody, Sheet};
use folio_layout::chart::{Anchor, ChartStyle};
use folio_layout::paint::{Painter, paint_chart, paint_doc_page, paint_label, paint_slide};
use folio_layout::{Face, Fonts, Glyph, Rgba, layout_doc, parse_hex};
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, Stroke};
use krilla::page::PageSettings;
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "pdf",
    name: "PDF",
    extensions: &["pdf"],
    kinds: &["doc", "sheet", "deck"],
    import: false,
    export: true,
    apps: &["any PDF reader", "printers"],
    notes: "Writes documents as they look in folio (fonts embedded, text searchable, links clickable), sheets as a printed grid of their used range on A4 landscape pages with charts after it, and decks one page per slide (hidden slides left out). Comments, speaker notes and formulas don't carry; folio can't open PDF files.",
};

pub fn import(_bytes: &[u8], _title: &str) -> Result<Imported, String> {
    Err("folio can't open PDF files: export them from the app that made them as DOCX, XLSX or PPTX instead.".into())
}

/// Fonts and images shared by every page of one export.
#[derive(Default)]
struct Resources {
    fonts: HashMap<Face, Option<Font>>,
    images: HashMap<Id, Option<Image>>,
    warnings: BTreeSet<String>,
}

/// A krilla surface seen as a folio-layout painter (points, y down: krilla's own space).
struct PdfPainter<'a, 'b> {
    s: &'a mut Surface<'b>,
    res: &'a mut Resources,
    links: Vec<(Rect, String)>,
    depth: usize,
}

fn fill(c: Rgba) -> Fill {
    Fill { paint: rgb::Color::new(c[0], c[1], c[2]).into(), opacity: NormalizedF32::new(c[3] as f32 / 255.0).unwrap_or(NormalizedF32::ONE), rule: FillRule::NonZero }
}

fn stroke(c: Rgba, width: f32) -> Stroke {
    Stroke {
        paint: rgb::Color::new(c[0], c[1], c[2]).into(),
        width,
        opacity: NormalizedF32::new(c[3] as f32 / 255.0).unwrap_or(NormalizedF32::ONE),
        line_cap: LineCap::Butt,
        line_join: LineJoin::Round,
        ..Default::default()
    }
}

fn path(pts: &[(f32, f32)], close: bool) -> Option<krilla::geom::Path> {
    let mut pb = PathBuilder::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        if i == 0 { pb.move_to(*x, *y) } else { pb.line_to(*x, *y) }
    }
    if close {
        pb.close();
    }
    pb.finish()
}

impl Resources {
    fn font(&mut self, fonts: &mut Fonts, face: &Face) -> Option<Font> {
        if let Some(f) = self.fonts.get(face) {
            return f.clone();
        }
        let f = fonts.face_data(face).and_then(|(data, index)| Font::new(data.into(), index));
        if f.is_none() {
            self.warnings.insert(format!("The {} font couldn't be embedded: text in it is left out.", face.family));
        }
        self.fonts.insert(face.clone(), f.clone());
        f
    }

    fn image(&mut self, doc: &Document, media: &Id) -> Option<Image> {
        if let Some(i) = self.images.get(media) {
            return i.clone();
        }
        let img = doc.media.get(media).and_then(|m| {
            let data: krilla::Data = m.bytes.clone().into();
            let direct = match folio_core::Media::sniff(&m.bytes) {
                "image/png" => Image::from_png(data, true).ok(),
                "image/jpeg" => Image::from_jpeg(data, true).ok(),
                "image/gif" => Image::from_gif(data, true).ok(),
                "image/webp" => Image::from_webp(data, true).ok(),
                _ => None,
            };
            direct.or_else(|| {
                let rgba = image::load_from_memory(&m.bytes).ok()?.to_rgba8();
                let (w, h) = rgba.dimensions();
                Some(Image::from_rgba8(rgba.into_raw(), w, h))
            })
        });
        if img.is_none() {
            let name = doc.media.get(media).map(|m| m.name.clone()).unwrap_or_else(|| media.0.clone());
            self.warnings.insert(format!("The picture \"{name}\" couldn't be read and is left out."));
        }
        self.images.insert(media.clone(), img.clone());
        img
    }
}

impl Painter for PdfPainter<'_, '_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        if w <= 0.0 || h <= 0.0 || c[3] == 0 {
            return;
        }
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            let mut pb = PathBuilder::new();
            pb.push_rect(r);
            if let Some(p) = pb.finish() {
                self.s.set_stroke(None);
                self.s.set_fill(Some(fill(c)));
                self.s.draw_path(&p);
            }
        }
    }

    fn poly(&mut self, pts: &[(f32, f32)], c: Rgba) {
        if c[3] == 0 {
            return;
        }
        if let Some(p) = path(pts, true) {
            self.s.set_stroke(None);
            self.s.set_fill(Some(fill(c)));
            self.s.draw_path(&p);
        }
    }

    fn line(&mut self, pts: &[(f32, f32)], width: f32, c: Rgba) {
        if c[3] == 0 || pts.len() < 2 {
            return;
        }
        if let Some(p) = path(pts, false) {
            self.s.set_fill(None);
            self.s.set_stroke(Some(stroke(c, width)));
            self.s.draw_path(&p);
            self.s.set_stroke(None);
        }
    }

    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, c: Rgba) {
        self.line(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)], width, c);
    }

    fn glyphs(&mut self, fonts: &mut Fonts, faces: &[Face], glyphs: &[Glyph], text: &str, ox: f32, oy: f32) {
        // Runs of glyphs sharing face, size, colour and baseline go out in one text object.
        let mut i = 0;
        while i < glyphs.len() {
            let g0 = glyphs[i];
            let mut j = i + 1;
            while j < glyphs.len() {
                let g = glyphs[j];
                if g.face != g0.face || g.size != g0.size || g.color != g0.color || (g.y - g0.y).abs() > 0.001 {
                    break;
                }
                j += 1;
            }
            let run = &glyphs[i..j];
            i = j;
            let Some(face) = faces.get(g0.face as usize) else { continue };
            let Some(font) = self.res.font(fonts, face) else { continue };
            let size = g0.size.max(0.1);
            let kg: Vec<KrillaGlyph> = run
                .iter()
                .enumerate()
                .map(|(k, g)| {
                    let next = run.get(k + 1).map(|n| n.x).unwrap_or(g.x);
                    let (a, b) = (g.cluster.0 as usize, g.cluster.1 as usize);
                    let range = if b <= text.len() && a <= b && text.is_char_boundary(a) && text.is_char_boundary(b) { a..b } else { 0..0 };
                    KrillaGlyph { glyph_id: GlyphId::new(g.id as u32), text_range: range, x_advance: (next - g.x) / size, x_offset: 0.0, y_offset: 0.0, y_advance: 0.0, location: None }
                })
                .collect();
            self.s.set_stroke(None);
            self.s.set_fill(Some(fill(g0.color)));
            self.s.draw_glyphs(Point::from_xy(ox + g0.x, oy + g0.y), &kg, font, text, size, false);
        }
    }

    fn image(&mut self, doc: &Document, media: &Id, x: f32, y: f32, w: f32, h: f32) -> bool {
        let Some(img) = self.res.image(doc, media) else { return false };
        let Some(size) = Size::from_wh(w.max(0.01), h.max(0.01)) else { return false };
        self.s.push_transform(&Transform::from_translate(x, y));
        self.s.draw_image(img, size);
        self.s.pop();
        true
    }

    fn push_rotation(&mut self, deg: f32, cx: f32, cy: f32) {
        self.s.push_transform(&Transform::from_rotate_at(deg, cx, cy));
        self.depth += 1;
    }

    fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let p = Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)).and_then(|r| {
            let mut pb = PathBuilder::new();
            pb.push_rect(r);
            pb.finish()
        });
        match p {
            Some(p) => self.s.push_clip_path(&p, &FillRule::NonZero),
            None => self.s.push_transform(&Transform::identity()),
        }
        self.depth += 1;
    }

    fn pop(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
            self.s.pop();
        }
    }

    fn link(&mut self, x: f32, y: f32, w: f32, h: f32, url: &str) {
        if let Some(r) = Rect::from_xywh(x, y, w.max(0.5), h.max(0.5)) {
            // Neighbouring pieces of one link on a line become one box.
            if let Some((last, u)) = self.links.last_mut()
                && u == url
                && (last.top() - r.top()).abs() < 0.5
                && (last.right() - r.left()).abs() < 1.0
                && let Some(m) = Rect::from_ltrb(last.left(), last.top(), r.right(), last.bottom().max(r.bottom()))
            {
                *last = m;
                return;
            }
            self.links.push((r, url.to_string()));
        }
    }
}

/// Starts a page, paints it with `f`, and adds its links.
fn page(document: &mut krilla::Document, res: &mut Resources, w: f32, h: f32, f: impl FnOnce(&mut dyn Painter)) {
    let Some(settings) = PageSettings::from_wh(w.max(1.0), h.max(1.0)) else { return };
    let mut page = document.start_page_with(settings);
    let links = {
        let mut surface = page.surface();
        let mut p = PdfPainter { s: &mut surface, res, links: vec![], depth: 0 };
        f(&mut p);
        while p.depth > 0 {
            p.pop();
        }
        let links = std::mem::take(&mut p.links);
        surface.finish();
        links
    };
    for (rect, url) in links {
        page.add_annotation(Annotation::new_link(LinkAnnotation::new(rect, Target::Action(Action::Link(LinkAction::new(url)))), None));
    }
    page.finish();
}

/// Writes the given pages (all kinds) as one PDF.
pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut fonts = Fonts::shared();
    export_with(&mut fonts, doc, pages)
}

/// [`export`] with a font system of your own.
pub fn export_with(fonts: &mut Fonts, doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut document = krilla::Document::new();
    let mut meta = krilla::metadata::Metadata::new().creator("folio".into());
    if !doc.title.trim().is_empty() {
        meta = meta.title(doc.title.clone());
    }
    if !doc.meta.author.trim().is_empty() {
        meta = meta.authors(vec![doc.meta.author.clone()]);
    }
    document.set_metadata(meta);
    let mut res = Resources::default();
    let mut count = 0;
    for &pi in pages {
        let Some(pg) = doc.pages.get(pi) else { continue };
        match &pg.body {
            PageBody::Doc(t) => {
                let layout = layout_doc(fonts, doc, pi);
                for n in 0..layout.pages.len() {
                    page(&mut document, &mut res, layout.width, layout.height, |p| paint_doc_page(p, fonts, doc, pi, &layout, n, false));
                    count += 1;
                }
                if !t.comments.is_empty() {
                    res.warnings.insert(format!("\"{}\": comments aren't printed.", pg.name));
                }
                note_links(doc, &pg.name, t.blocks.iter().filter_map(|b| if let folio_core::Block::Chart(c) = b { Some(&c.chart) } else { None }), &mut res);
            }
            PageBody::Sheet(s) => {
                count += sheet_pages(&mut document, &mut res, fonts, doc, &pg.name, s);
                note_links(doc, &pg.name, s.charts.iter().map(|c| &c.chart), &mut res);
            }
            PageBody::Deck(d) => {
                let hidden = d.slides.iter().filter(|s| s.hidden).count();
                for s in d.slides.iter().filter(|s| !s.hidden) {
                    page(&mut document, &mut res, d.size[0], d.size[1], |p| paint_slide(p, fonts, doc, d, s));
                    count += 1;
                }
                if hidden > 0 {
                    res.warnings.insert(format!("\"{}\": {hidden} hidden slide{} left out.", pg.name, if hidden == 1 { "" } else { "s" }));
                }
                if d.slides.iter().any(|s| !s.notes.trim().is_empty()) {
                    res.warnings.insert(format!("\"{}\": speaker notes aren't printed.", pg.name));
                }
                note_links(doc, &pg.name, d.slides.iter().flat_map(|s| s.shapes.iter()).filter_map(|sh| if let folio_core::ShapeKind::Chart { chart } = &sh.kind { Some(chart) } else { None }), &mut res);
            }
        }
    }
    if count == 0 {
        // A PDF needs a page.
        page(&mut document, &mut res, 595.0, 842.0, |_| {});
    }
    let bytes = document.finish().map_err(|e| format!("Couldn't write the PDF: {e:?}"))?;
    Ok((bytes, res.warnings.into_iter().collect()))
}

/// Warns about charts whose sheet range doesn't resolve (they print as "No numbers…").
fn note_links<'a>(doc: &Document, page: &str, charts: impl Iterator<Item = &'a folio_core::Chart>, res: &mut Resources) {
    for c in charts {
        if let Err(e) = folio_core::links::chart_data(doc, c) {
            res.warnings.insert(format!("\"{page}\": a chart's data couldn't be read ({e}); it prints empty."));
        }
    }
}

// ---- sheets -------------------------------------------------------------------------------

const SHEET_W: f32 = 842.0;
const SHEET_H: f32 = 595.0;
const SHEET_MARGIN: f32 = 36.0;
const PX: f32 = 0.75;

/// Prints a sheet: its used range as a grid over as many pages as it takes (down, then
/// across), then its charts. Returns how many pages it wrote.
fn sheet_pages(document: &mut krilla::Document, res: &mut Resources, fonts: &mut Fonts, doc: &Document, name: &str, s: &Sheet) -> usize {
    let title_h = 22.0;
    let top = SHEET_MARGIN + title_h;
    let avail_w = SHEET_W - 2.0 * SHEET_MARGIN;
    let avail_h = SHEET_H - SHEET_MARGIN - top;
    let mut count = 0;
    let title = |p: &mut dyn Painter, fonts: &mut Fonts, part: &str| {
        paint_label(p, fonts, &format!("{name}{part}"), SHEET_MARGIN, SHEET_MARGIN + 11.0, 10.0, [60, 60, 60, 255], Anchor::Start, "sans", 600, false);
    };
    if let Some(used) = s.used_range() {
        let hidden = s.hidden_rows();
        let rows: Vec<u32> = (used.start.row..=used.end.row).filter(|r| !hidden.contains(r)).collect();
        let cols: Vec<u32> = (used.start.col..=used.end.col).collect();
        let col_groups = groups(&cols, |c| s.col_width(*c) * PX, avail_w);
        let row_groups = groups(&rows, |r| s.row_height(*r) * PX, avail_h);
        let pages_total = col_groups.len() * row_groups.len();
        for cg in &col_groups {
            for rg in &row_groups {
                count += 1;
                let part = if pages_total > 1 { format!(" ({count} of {pages_total})") } else { String::new() };
                page(document, res, SHEET_W, SHEET_H, |p| {
                    title(p, fonts, &part);
                    grid(p, fonts, s, cg, rg, SHEET_MARGIN, top);
                });
            }
        }
    }
    // Charts after the grid, as many per page as fit.
    if !s.charts.is_empty() {
        let mut placed: Vec<Vec<(usize, f32, f32, f32, f32)>> = vec![vec![]];
        let mut y = top;
        for (i, c) in s.charts.iter().enumerate() {
            let (w, h) = (c.w * PX, c.h * PX);
            let k = (avail_w / w).min(avail_h / h).min(1.0);
            let (w, h) = (w * k, h * k);
            if y + h > SHEET_H - SHEET_MARGIN && !placed.last().unwrap().is_empty() {
                placed.push(vec![]);
                y = top;
            }
            placed.last_mut().unwrap().push((i, SHEET_MARGIN + (avail_w - w) / 2.0, y, w, h));
            y += h + 18.0;
        }
        for group in placed {
            count += 1;
            page(document, res, SHEET_W, SHEET_H, |p| {
                title(p, fonts, " (charts)");
                for (i, x, y, w, h) in group {
                    paint_chart(p, fonts, doc, &s.charts[i].chart, x, y, w, h, &ChartStyle::default());
                }
            });
        }
    }
    if count == 0 {
        count += 1;
        page(document, res, SHEET_W, SHEET_H, |p| {
            title(p, fonts, "");
            paint_label(p, fonts, "This sheet is empty.", SHEET_MARGIN, top + 14.0, 10.0, [120, 120, 120, 255], Anchor::Start, "sans", 400, false);
        });
    }
    count
}

/// Splits items into runs whose sizes fit `room` (an item bigger than the room goes alone).
fn groups<T: Copy>(items: &[T], size: impl Fn(&T) -> f32, room: f32) -> Vec<Vec<(T, f32)>> {
    let mut out: Vec<Vec<(T, f32)>> = vec![];
    let mut used = room + 1.0;
    for it in items {
        let s = size(it);
        if used + s > room + 0.01 {
            out.push(vec![]);
            used = 0.0;
        }
        out.last_mut().unwrap().push((*it, s));
        used += s;
    }
    out
}

/// One page of the grid: columns `cg`, rows `rg`, top-left at (`x0`, `y0`).
fn grid(p: &mut dyn Painter, fonts: &mut Fonts, s: &Sheet, cg: &[(u32, f32)], rg: &[(u32, f32)], x0: f32, y0: f32) {
    let xs: Vec<f32> = cg.iter().scan(x0, |x, (_, w)| {
        let at = *x;
        *x += w;
        Some(at)
    }).collect();
    let ys: Vec<f32> = rg.iter().scan(y0, |y, (_, h)| {
        let at = *y;
        *y += h;
        Some(at)
    }).collect();
    let right = xs.last().map(|x| x + cg.last().unwrap().1).unwrap_or(x0);
    let bottom = ys.last().map(|y| y + rg.last().unwrap().1).unwrap_or(y0);
    // Fills.
    for (ri, (r, h)) in rg.iter().enumerate() {
        for (ci, (c, w)) in cg.iter().enumerate() {
            if let Some(f) = s.cell(Addr::new(*r, *c)).and_then(|cell| cell.format.fill.as_deref()) {
                p.rect(xs[ci], ys[ri], *w, *h, parse_hex(f, [255, 255, 255, 255]));
            }
        }
    }
    // Grid lines.
    if s.gridlines {
        let g = [0, 0, 0, 38];
        for y in ys.iter().chain([&bottom]) {
            p.line(&[(x0, *y), (right, *y)], 0.3, g);
        }
        for x in xs.iter().chain([&right]) {
            p.line(&[(*x, y0), (*x, bottom)], 0.3, g);
        }
    }
    // Values.
    for (ri, (r, h)) in rg.iter().enumerate() {
        for (ci, (c, w)) in cg.iter().enumerate() {
            let Some(cell) = s.cell(Addr::new(*r, *c)) else { continue };
            let text = cell.display();
            let f = &cell.format;
            if !text.is_empty() {
                let size = f.size.unwrap_or(10.0).clamp(4.0, 72.0);
                let number = matches!(cell.value, Value::Number(_));
                let align = f.align.unwrap_or(if number || matches!(cell.value, Value::Bool(_) | Value::Error(_)) { folio_core::Align::Right } else { folio_core::Align::Left });
                // Text runs on over empty neighbours, as on screen.
                let mut room = *w;
                if !number && align == folio_core::Align::Left {
                    for (c2, w2) in cg.iter().skip(ci + 1) {
                        if s.cell(Addr::new(*r, *c2)).is_some_and(|x| !x.input.is_empty()) {
                            break;
                        }
                        room += w2;
                    }
                }
                let color = f.color.as_deref().map(|c| parse_hex(c, [10, 10, 10, 255])).unwrap_or(if matches!(cell.value, Value::Error(_)) { [185, 28, 28, 255] } else { [10, 10, 10, 255] });
                let pad = 3.0;
                let (x, anchor) = match align {
                    folio_core::Align::Right => (xs[ci] + w - pad, Anchor::End),
                    folio_core::Align::Center => (xs[ci] + w / 2.0, Anchor::Middle),
                    _ => (xs[ci] + pad, Anchor::Start),
                };
                let baseline = ys[ri] + h / 2.0 + size * 0.35;
                p.push_clip(xs[ci], ys[ri], room, *h);
                let tw = paint_label(p, fonts, &text, x, baseline, size, color, anchor, "sans", if f.bold { 700 } else { 400 }, f.italic);
                let lx = match anchor {
                    Anchor::Start => x,
                    Anchor::Middle => x - tw / 2.0,
                    Anchor::End => x - tw,
                };
                if f.underline {
                    p.rect(lx, baseline + size * 0.12, tw, (size * 0.06).max(0.5), color);
                }
                if f.strike {
                    p.rect(lx, baseline - size * 0.27, tw, (size * 0.06).max(0.5), color);
                }
                p.pop();
            }
            // Borders.
            let b = [10, 10, 10, 255];
            let (x, y) = (xs[ci], ys[ri]);
            for side in f.border.chars() {
                match side {
                    't' => p.line(&[(x, y), (x + w, y)], 0.75, b),
                    'b' => p.line(&[(x, y + h), (x + w, y + h)], 0.75, b),
                    'l' => p.line(&[(x, y), (x, y + h)], 0.75, b),
                    'r' => p.line(&[(x + w, y), (x + w, y + h)], 0.75, b),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::deck::{Shape, ShapeKind, Slide, SlideLayout};
    use folio_core::text::{Align, Block, ChartBlock, ImageBlock, ListKind, ParaStyle, Table};
    use folio_core::{Chart, ChartKind, PageKind, Paragraph, Run, RunStyle};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![];
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 160, 255]));
        img.write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
        b
    }

    /// A document with every block kind, a sheet with a chart, and a deck.
    pub(crate) fn sample() -> Document {
        let mut doc = Document::empty("Sample report");
        let si = doc.add_page(PageKind::Sheet, Some("Sales"), None).unwrap();
        {
            let s = doc.page_mut(si).sheet_mut().unwrap();
            let rows = [["Month", "North", "South"], ["Jan", "120", "80"], ["Feb", "135", "95"], ["Mar", "150", "70"], ["Apr", "160", "110"]];
            for (r, row) in rows.iter().enumerate() {
                for (c, v) in row.iter().enumerate() {
                    s.set_input(Addr::new(r as u32, c as u32), v);
                }
            }
            s.set_input(Addr::new(5, 0), "Total");
            s.set_input(Addr::new(5, 1), "=SUM(B2:B5)");
            s.format(folio_calc::Range { start: Addr::new(0, 0), end: Addr::new(0, 2) }, &folio_core::sheet::FormatPatch { bold: Some(true), fill: Some("#eeeeee".into()), border: Some("b".into()), ..Default::default() });
            s.charts.push(folio_core::sheet::SheetChart { id: Id::new(), chart: Chart::new(ChartKind::Line, "'Sales'!A1:C5"), x: 300.0, y: 20.0, w: 480.0, h: 300.0 });
        }
        folio_core::recalc::Calc::new().sync(&mut doc);
        let media = doc.add_media("gradient.png", png(320, 160));
        let di = doc.add_page(PageKind::Doc, Some("Report"), Some(0)).unwrap();
        {
            let t = doc.page_mut(di).doc_mut().unwrap();
            t.setup.header = "{title}".into();
            t.setup.footer = "Page {page} of {pages}".into();
            let mut table = Table::from_text(vec![], true);
            table.link = Some("'Sales'!A1:C6".into());
            table.banded = true;
            let mut chart = Chart::new(ChartKind::Column, "'Sales'!A1:C5");
            chart.title = "Sales by month".into();
            t.blocks = vec![
                Block::Paragraph(Paragraph::new(ParaStyle::Title, "Quarterly report")),
                Block::Paragraph(Paragraph::new(ParaStyle::Subtitle, "North and South, January to April")),
                Block::Paragraph(Paragraph::new(ParaStyle::Heading1, "Summary")),
                Block::Paragraph(Paragraph::with_runs(
                    ParaStyle::Normal,
                    vec![
                        Run::plain("Sales grew in both regions. The "),
                        Run::bold("North"),
                        Run::plain(" led with "),
                        Run::italic("steady"),
                        Run::plain(" growth, see "),
                        Run::styled("lsuite.xyz", RunStyle { link: Some("https://lsuite.xyz".into()), ..Default::default() }),
                        Run::styled(" for details", RunStyle { note: Some("Figures are from the Sales sheet.".into()), ..Default::default() }),
                        Run::plain(". Some text is "),
                        Run::styled("highlighted", RunStyle { highlight: Some("#fff176".into()), ..Default::default() }),
                        Run::plain(", "),
                        Run::styled("struck", RunStyle { strike: true, ..Default::default() }),
                        Run::plain(" or "),
                        Run::styled("code()", RunStyle { code: true, ..Default::default() }),
                        Run::plain(", and H"),
                        Run::styled("2", RunStyle { subscript: true, ..Default::default() }),
                        Run::plain("O, E = mc"),
                        Run::styled("2", RunStyle { superscript: true, ..Default::default() }),
                        Run::plain("."),
                    ],
                )
                .align(Align::Justify)),
                Block::Paragraph(Paragraph::new(ParaStyle::Normal, "First point").list(ListKind::Bullet, 0)),
                Block::Paragraph(Paragraph::new(ParaStyle::Normal, "A nested point").list(ListKind::Bullet, 1)),
                Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Step one").list(ListKind::Number, 0)),
                Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Step two").list(ListKind::Number, 0)),
                Block::Paragraph(Paragraph { checked: true, ..Paragraph::new(ParaStyle::Normal, "Done item").list(ListKind::Check, 0) }),
                Block::Paragraph(Paragraph::new(ParaStyle::Quote, "A quote, set in the serif face.")),
                Block::Paragraph(Paragraph::new(ParaStyle::Code, "fn main() { println!(\"folio\"); }")),
                Block::Table(table),
                Block::Image(ImageBlock { id: Id::new(), media, width: 240.0, caption: "Figure 1: a gradient".into(), alt: String::new(), align: Align::Center }),
                Block::Chart(ChartBlock { id: Id::new(), chart, height: 220.0 }),
                Block::PageBreak { id: Id::new() },
                Block::Paragraph(Paragraph::new(ParaStyle::Heading2, "Second page")),
                Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Text after the page break.").align(Align::Center)),
            ]
            .into_iter()
            .collect();
        }
        let ki = doc.add_page(PageKind::Deck, Some("Slides"), None).unwrap();
        {
            let d = doc.page_mut(ki).deck_mut().unwrap();
            let size = d.size;
            d.slides = vec![Slide::with_layout(SlideLayout::Title, size, "Quarterly report", "Sales, April")];
            let mut s2 = Slide::with_layout(SlideLayout::TitleContent, size, "Highlights", "North up 33%\nSouth up 38%\nNew stores open in May");
            s2.shapes[1].w = 420.0;
            let mut ch = Shape::new(ShapeKind::Chart { chart: Chart::new(ChartKind::Pie, "'Sales'!A1:B5") }, 520.0, 150.0, 380.0, 320.0);
            ch.text_size = 16.0;
            s2.shapes.push(ch);
            let mut r = Shape::new(ShapeKind::Ellipse, 820.0, 30.0, 80.0, 80.0);
            r.fill = Some("#0a0a0a".into());
            r.rotation = 15.0;
            s2.shapes.push(r);
            d.slides.push(s2);
            let mut s3 = Slide::with_layout(SlideLayout::TitleOnly, size, "Numbers", "");
            let mut tb = Table::from_text(vec![], true);
            tb.link = Some("'Sales'!A1:C5".into());
            let mut ts = Shape::new(ShapeKind::Table { table: tb }, 72.0, 150.0, 816.0, 280.0);
            ts.text_size = 16.0;
            s3.shapes.push(ts);
            let mut a = Shape::new(ShapeKind::Arrow, 100.0, 470.0, 300.0, 0.0);
            a.line = Some("#0a0a0a".into());
            a.line_width = 3.0;
            s3.shapes.push(a);
            d.slides.push(s3);
            let mut hidden = Slide::with_layout(SlideLayout::Blank, size, "", "");
            hidden.hidden = true;
            d.slides.push(hidden);
        }
        doc
    }

    #[test]
    fn exports_every_kind() {
        let doc = sample();
        let mut fonts = Fonts::bundled_only();
        let all: Vec<usize> = (0..doc.pages.len()).collect();
        let (bytes, warnings) = export_with(&mut fonts, &doc, &all).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        assert!(warnings.iter().any(|w| w.contains("hidden slide")), "{warnings:?}");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/URI") || bytes.windows(4).any(|w| w == b"/URI"), "link annotation");
        let dir = std::env::var("FOLIO_PDF_OUT").ok();
        if let Some(dir) = dir {
            std::fs::write(std::path::Path::new(&dir).join("folio-sample.pdf"), &bytes).unwrap();
        }
        // Document pages, the sheet's grid and chart pages, three visible slides.
        let pages = bytes.windows(10).filter(|w| w == b"/Type /Pag" || w == b"/Type/Page").count();
        assert!(pages >= 6, "{pages}");
    }

    #[test]
    fn import_is_refused() {
        assert!(import(b"%PDF-1.7", "x").is_err());
    }
}
