//! Drawing what folio-layout laid out: glyphs at their exact positions (so the screen matches
//! the PDF), decorations, charts and pictures.
//!
//! Layout units are points; `scale` turns them into window pixels (zoom × 4/3, so 100 % shows
//! a page at the size Word and Google Docs show it).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use folio_layout::chart::{Anchor, Prim};
use folio_layout::{Face, ParaLayout, Rgba};
use gpui::{App, Bounds, FontId, FontStyle, FontWeight, GlyphId, Hsla, PathBuilder, Pixels, Point, RenderImage, SharedString, TextRun, Window, fill, point, px, size};

/// Points to pixels at 100 %.
pub const PT: f32 = 4.0 / 3.0;

pub fn rgba(c: Rgba) -> Hsla {
    gpui::Rgba { r: c[0] as f32 / 255.0, g: c[1] as f32 / 255.0, b: c[2] as f32 / 255.0, a: c[3] as f32 / 255.0 }.into()
}

pub fn hex(s: &str, fallback: Hsla) -> Hsla {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return fallback;
    }
    match u32::from_str_radix(h, 16) {
        Ok(v) => gpui::Rgba { r: ((v >> 16) & 255) as f32 / 255.0, g: ((v >> 8) & 255) as f32 / 255.0, b: (v & 255) as f32 / 255.0, a: 1.0 }.into(),
        Err(_) => fallback,
    }
}

thread_local! {
    static FONTS: RefCell<HashMap<Face, FontId>> = RefCell::new(HashMap::new());
    static IMAGES: RefCell<HashMap<(usize, u32), Arc<RenderImage>>> = RefCell::new(HashMap::new());
}

fn font_of(face: &Face) -> gpui::Font {
    gpui::Font {
        family: SharedString::from(face.family.clone()),
        features: Default::default(),
        fallbacks: None,
        weight: FontWeight(face.weight as f32),
        style: if face.italic { FontStyle::Italic } else { FontStyle::Normal },
    }
}

fn font_id(face: &Face, window: &Window) -> FontId {
    FONTS.with(|f| *f.borrow_mut().entry(face.clone()).or_insert_with(|| window.text_system().resolve_font(&font_of(face))))
}

/// Paints lines `lines` of a laid-out paragraph whose top-left (in pixels) is `origin`. `dy`
/// is subtracted from line positions (when the paragraph starts on an earlier page).
pub fn para(window: &mut Window, layout: &ParaLayout, origin: Point<Pixels>, scale: f32, lines: std::ops::Range<usize>, dy: f32, color_override: Option<Hsla>) {
    let ids: Vec<FontId> = layout.faces.iter().map(|f| font_id(f, window)).collect();
    let at = |x: f32, y: f32| point(origin.x + px(x * scale), origin.y + px((y - dy) * scale));
    // The list marker before the first line.
    if lines.start == 0
        && let Some(m) = &layout.marker
    {
        for g in &m.glyphs {
            if let Some(f) = ids.get(g.face as usize) {
                let _ = window.paint_glyph(at(g.x, g.y), *f, GlyphId(g.id as u32), px(g.size * scale), color_override.unwrap_or(rgba(g.color)));
            }
        }
        if let Some((x, s, checked)) = m.checkbox {
            let top = layout.lines.first().map(|l| l.baseline - s).unwrap_or(0.0);
            let b = Bounds::new(at(x, top), size(px(s * scale), px(s * scale)));
            let c = color_override.unwrap_or(rgba(layout.lines.first().and_then(|l| l.glyphs.first()).map(|g| g.color).unwrap_or([10, 10, 10, 255])));
            window.paint_quad(gpui::outline(b, c, gpui::BorderStyle::Solid));
            if checked {
                let inner = Bounds::new(point(b.origin.x + px(2.5 * scale), b.origin.y + px(2.5 * scale)), size(b.size.width - px(5. * scale), b.size.height - px(5. * scale)));
                window.paint_quad(fill(inner, c));
            }
        }
    }
    for l in layout.lines.get(lines).unwrap_or(&[]) {
        // Backgrounds first (highlight, comments), then glyphs, then lines over them.
        for d in l.decos.iter().filter(|d| d.is_background()) {
            let b = Bounds::new(at(d.x0, d.y), size(px((d.x1 - d.x0) * scale), px(d.h * scale)));
            window.paint_quad(fill(b, rgba(d.color)));
        }
        for g in &l.glyphs {
            if let Some(f) = ids.get(g.face as usize) {
                let _ = window.paint_glyph(at(g.x, g.y), *f, GlyphId(g.id as u32), px(g.size * scale), color_override.unwrap_or(rgba(g.color)));
            }
        }
        for d in l.decos.iter().filter(|d| !d.is_background()) {
            let b = Bounds::new(at(d.x0, d.y), size(px((d.x1 - d.x0) * scale), px((d.h * scale).max(1.0))));
            window.paint_quad(fill(b, color_override.unwrap_or(rgba(d.color))));
        }
    }
}

/// A picture from bytes, decoded once (BGRA, as GPUI wants) and kept.
pub fn image(bytes: &Arc<Vec<u8>>, max_px: u32) -> Option<Arc<RenderImage>> {
    let key = (Arc::as_ptr(bytes) as usize, max_px);
    if let Some(i) = IMAGES.with(|m| m.borrow().get(&key).cloned()) {
        return Some(i);
    }
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max_px || img.height() > max_px { img.thumbnail(max_px, max_px) } else { img };
    let mut buf = img.to_rgba8();
    for p in buf.pixels_mut() {
        p.0.swap(0, 2);
    }
    let r = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(buf)]));
    IMAGES.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 64 {
            m.clear();
        }
        m.insert(key, r.clone());
    });
    Some(r)
}

/// Paints chart primitives in a box at `origin` (pixels), points scaled by `scale`.
pub fn prims(window: &mut Window, cx: &mut App, list: &[Prim], origin: Point<Pixels>, scale: f32) {
    let at = |x: f32, y: f32| point(origin.x + px(x * scale), origin.y + px(y * scale));
    for p in list {
        match p {
            Prim::Rect { x, y, w, h, fill: c } => window.paint_quad(fill(Bounds::new(at(*x, *y), size(px(w * scale), px(h * scale))), rgba(*c))),
            Prim::Line { points, width, color } => {
                if points.len() < 2 {
                    continue;
                }
                let mut b = PathBuilder::stroke(px((width * scale).max(1.0)));
                b.move_to(at(points[0].0, points[0].1));
                for (x, y) in &points[1..] {
                    b.line_to(at(*x, *y));
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, rgba(*color));
                }
            }
            Prim::Poly { points, fill: c } => {
                if points.len() < 3 {
                    continue;
                }
                let mut b = PathBuilder::fill();
                b.move_to(at(points[0].0, points[0].1));
                for (x, y) in &points[1..] {
                    b.line_to(at(*x, *y));
                }
                b.close();
                if let Ok(path) = b.build() {
                    window.paint_path(path, rgba(*c));
                }
            }
            Prim::Text { x, y, text, size: s, color, anchor, bold } => {
                let font = gpui::Font { family: "IBM Plex Sans".into(), features: Default::default(), fallbacks: None, weight: if *bold { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }, style: FontStyle::Normal };
                let run = TextRun { len: text.len(), font, color: rgba(*color), background_color: None, underline: None, strikethrough: None };
                let fs = px(s * scale);
                let line = window.text_system().shape_line(SharedString::from(text.clone()), fs, &[run], None);
                let w = f32::from(line.width);
                let dx = match anchor {
                    Anchor::Start => 0.0,
                    Anchor::Middle => -w / 2.0,
                    Anchor::End => -w,
                };
                let o = at(*x, *y);
                let top = point(o.x + px(dx), o.y - fs * 0.8);
                let _ = line.paint(top, fs * 1.0, gpui::TextAlign::Left, None, window, cx);
            }
        }
    }
}
