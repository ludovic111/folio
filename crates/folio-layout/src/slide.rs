//! Slides: the text inside shapes, laid out in slide points.
//!
//! PLACEHOLDER: the real implementation replaces `layout_shape_text`.

use std::sync::Arc;

use folio_core::deck::{Deck, Shape};

use crate::fonts::Fonts;
use crate::text::{ParaCtx, ParaLayout, layout_paragraph};

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

pub fn layout_shape_text(fonts: &mut Fonts, deck: &Deck, shape: &Shape) -> ShapeText {
    let ctx = ParaCtx { scale: shape.text_size / 11.0, color: crate::parse_hex(shape.color.as_deref().unwrap_or(&deck.theme.text), [0, 0, 0, 255]), ..Default::default() };
    let w = (shape.w - 2.0 * INSET).max(4.0);
    let mut y = 0.0;
    let mut paras = vec![];
    for b in shape.text.iter() {
        if let Some(p) = b.para() {
            let l = Arc::new(layout_paragraph(fonts, p, w, &ctx));
            paras.push((y, l.clone()));
            y += l.height + l.space_after;
        }
    }
    let top = match shape.valign {
        folio_core::deck::VAlign::Top => shape.y + INSET,
        folio_core::deck::VAlign::Middle => shape.y + (shape.h - y) / 2.0,
        folio_core::deck::VAlign::Bottom => shape.y + shape.h - INSET - y,
    };
    for p in &mut paras {
        p.0 += top;
    }
    ShapeText { x: shape.x + INSET, paras, height: y }
}
