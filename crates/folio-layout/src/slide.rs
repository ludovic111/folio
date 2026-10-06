//! Slides: the text inside shapes, laid out in slide points, and tables on slides.

use std::sync::Arc;

use folio_core::Document;
use folio_core::deck::{Deck, DeckTheme, Shape, VAlign};
use folio_core::text::{Family, Table};

use crate::Rgba;
use crate::chart::ChartStyle;
use crate::doc::{Placed, TABLE_BORDER_WIDTH, list_numbers, table_grid};
use crate::fonts::Fonts;
use crate::text::{ParaCtx, ParaLayout, layout_paragraph_cached};

/// A shape's text: each paragraph with the y of its top (slide coordinates), and the left x.
#[derive(Clone, Debug, Default)]
pub struct ShapeText {
    pub x: f32,
    pub paras: Vec<(f32, Arc<ParaLayout>)>,
    /// Height of all the text.
    pub height: f32,
}

/// Inner margin of a text box, in points.
pub const INSET: f32 = 8.0;

/// The theme's (heading, body) families.
pub fn theme_families(theme: &DeckTheme) -> (Family, Family) {
    (Family::parse(&theme.heading_font).unwrap_or(Family::Display), Family::parse(&theme.body_font).unwrap_or(Family::Sans))
}

/// How a shape's text is set: scale from its text size, the theme's families and colour.
pub fn shape_ctx(deck: &Deck, shape: &Shape) -> ParaCtx {
    ParaCtx {
        scale: (shape.text_size / 11.0).max(0.05),
        color: crate::parse_hex(shape.color.as_deref().unwrap_or(&deck.theme.text), [0, 0, 0, 255]),
        families: Some(theme_families(&deck.theme)),
        ..Default::default()
    }
}

pub fn layout_shape_text(fonts: &mut Fonts, deck: &Deck, shape: &Shape) -> ShapeText {
    let base = shape_ctx(deck, shape);
    let w = (shape.w - 2.0 * INSET).max(4.0);
    let numbers = list_numbers(&shape.text);
    let mut y = 0.0;
    let mut paras = vec![];
    let mut last_after = 0.0;
    for (i, b) in shape.text.iter().enumerate() {
        if let Some(p) = b.para() {
            let ctx = ParaCtx { number: numbers[i], ..base.clone() };
            let l = layout_paragraph_cached(fonts, p, w, &ctx);
            if !paras.is_empty() {
                y += l.space_before.max(0.0);
            }
            paras.push((y, l.clone()));
            y += l.height + l.space_after;
            last_after = l.space_after;
        }
    }
    let height = (y - last_after).max(0.0);
    let top = match shape.valign {
        VAlign::Top => shape.y + INSET,
        VAlign::Middle => shape.y + (shape.h - height) / 2.0,
        VAlign::Bottom => shape.y + shape.h - INSET - height,
    };
    for p in &mut paras {
        p.0 += top;
    }
    ShapeText { x: shape.x + INSET, paras, height }
}

/// A table in a `w`×`h` box at (`x`, `y`) on a slide: its rows as [`Placed::TableRow`]s (block
/// 0), text scaled by `scale`, rows stretched to fill the box when they are shorter.
pub fn layout_table_box(fonts: &mut Fonts, doc: &Document, table: &Table, x: f32, y: f32, w: f32, h: f32, scale: f32) -> Vec<Placed> {
    table_box(fonts, doc, table, x, y, w, h, &ParaCtx { scale, ..Default::default() }, crate::doc::TABLE_BORDER)
}

/// A table shape on a slide, in the deck's theme (text colour and family, border).
pub fn layout_slide_table(fonts: &mut Fonts, doc: &Document, deck: &Deck, shape: &Shape, table: &Table) -> Vec<Placed> {
    let ctx = shape_ctx(deck, shape);
    let text = ctx.color;
    let border = [text[0], text[1], text[2], 110];
    table_box(fonts, doc, table, shape.x, shape.y, shape.w, shape.h, &ctx, border)
}

#[allow(clippy::too_many_arguments)]
fn table_box(fonts: &mut Fonts, doc: &Document, table: &Table, x: f32, y: f32, w: f32, h: f32, ctx: &ParaCtx, border: Rgba) -> Vec<Placed> {
    let grid = table_grid(fonts, doc, table, x, w, ctx);
    let total: f32 = grid.rows.iter().map(|r| r.0).sum();
    let stretch = if total > 0.0 && total < h { (h - total) / grid.rows.len() as f32 } else { 0.0 };
    let mut out = vec![];
    let mut ry = y;
    for (ri, (rh, cells, header, shaded)) in grid.rows.into_iter().enumerate() {
        let height = rh + stretch;
        let cells = cells.into_iter().map(|mut c| {
            c.ty += ry;
            c
        });
        out.push(Placed::TableRow { block: 0, row: ri, x, y: ry, w, height, cells: cells.collect(), header, shaded, border, border_width: TABLE_BORDER_WIDTH * ctx.scale.clamp(0.75, 2.0) });
        ry += height;
    }
    out
}

/// Chart colours on a slide: the theme's accent, then its text colour in lighter steps.
pub fn theme_chart_style(theme: &DeckTheme) -> ChartStyle {
    let text = crate::parse_hex(&theme.text, [0, 0, 0, 255]);
    let accent = crate::parse_hex(&theme.accent, text);
    let bg = crate::parse_hex(&theme.background, [255, 255, 255, 255]);
    let mix = |t: f32| -> Rgba {
        let m = |i: usize| (text[i] as f32 * (1.0 - t) + bg[i] as f32 * t) as u8;
        [m(0), m(1), m(2), 255]
    };
    let mut series = vec![accent];
    for t in [0.45, 0.7, 0.25, 0.82, 0.58] {
        let c = mix(t);
        if c != accent {
            series.push(c);
        }
    }
    ChartStyle { text, grid: [text[0], text[1], text[2], 45], series, size: 12.0, background: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::deck::{Slide, SlideLayout};

    #[test]
    fn placeholders_lay_out_in_theme_fonts() {
        let mut f = crate::doc::tests::fonts();
        let deck = Deck::default();
        let s = Slide::with_layout(SlideLayout::TitleContent, deck.size, "Quarterly plan", "First point\nSecond point that is long enough to wrap onto another line in the body box");
        let title = layout_shape_text(&mut f, &deck, &s.shapes[0]);
        assert_eq!(title.paras[0].1.faces[0].family, "Chakra Petch");
        // Bottom-aligned title: its text ends at the box's bottom inset.
        let sh = &s.shapes[0];
        assert!((title.paras[0].0 + title.height - (sh.y + sh.h - INSET)).abs() < 0.01);
        let body = layout_shape_text(&mut f, &deck, &s.shapes[1]);
        assert_eq!(body.paras.len(), 2);
        assert_eq!(body.paras[0].1.faces[0].family, "IBM Plex Sans");
        assert!(body.paras[0].1.marker.is_some());
        assert!(body.paras[1].0 > body.paras[0].0);
        let width = s.shapes[1].w - 2.0 * INSET;
        assert!(body.paras.iter().all(|(_, l)| l.lines.iter().all(|ln| ln.glyphs.iter().all(|g| g.x <= width))));
        let dark = Deck { theme: DeckTheme::named("ink").unwrap(), ..Deck::default() };
        let t = layout_shape_text(&mut f, &dark, &s.shapes[0]);
        assert_eq!(t.paras[0].1.lines[0].glyphs[0].color, [0xf2, 0xf2, 0xf2, 255]);
    }

    #[test]
    fn table_box_fills_its_height() {
        let mut f = crate::doc::tests::fonts();
        let doc = Document::empty("T");
        let t = Table::from_text(vec![vec!["A".into(), "B".into()], vec!["1".into(), "2".into()]], true);
        let rows = layout_table_box(&mut f, &doc, &t, 100.0, 50.0, 400.0, 300.0, 1.5);
        assert_eq!(rows.len(), 2);
        let (_, bottom) = rows[1].span();
        assert!((bottom - 350.0).abs() < 0.01);
    }
}
