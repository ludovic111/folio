//! folio-layout: where every glyph goes, the same in the window and in exports.
//!
//! Text is shaped and broken into lines here (cosmic-text, the bundled faces first), then the
//! window paints the glyphs with GPUI and the PDF export writes them with krilla: what is on
//! screen is what prints. Units are **points** (document pages and slides alike); the window
//! scales them.
//!
//! * [`fonts`]: the bundled faces (IBM Plex Sans, Serif, Mono, Chakra Petch) and the font system.
//! * [`text`]: one paragraph laid out at a width ([`ParaLayout`]): lines, glyphs, decorations,
//!   caret positions, hit testing.
//! * [`doc`]: a document page paginated ([`DocLayout`]): paragraphs, tables, pictures, charts,
//!   page breaks, headers, footers, footnotes.
//! * [`slide`]: a slide's shapes with their text laid out inside them.
//! * [`chart`]: a chart's geometry (bars, lines, wedges, axes, labels) as primitives anyone can draw.
//! * [`paint`]: what to draw where, on any [`Painter`] (the PNG canvas, the PDF export).
//! * [`raster`]: charts, slides and pages to PNG (tiny-skia), for exports and thumbnails.

pub mod chart;
pub mod doc;
pub mod fonts;
pub mod paint;
pub mod raster;
pub mod slide;
pub mod text;

pub use chart::{Anchor, ChartStyle, Prim, chart_prims};
pub use doc::{CellBox, DocLayout, PageLayout, Placed, layout_doc, page_count};
pub use fonts::{Face, Fonts, bundled_fonts};
pub use paint::Painter;
pub use raster::{Canvas, chart_png, page_png, slide_png};
pub use slide::{ShapeText, layout_shape_text, layout_slide_table, layout_table_box};
pub use text::{Deco, DecoKind, Glyph, Line, Marker, ParaCtx, ParaLayout, layout_paragraph, layout_paragraph_cached, shape_label};

/// A colour as RGBA bytes.
pub type Rgba = [u8; 4];

/// `#rrggbb` (or `#rgb`) to RGBA; `fallback` when it doesn't parse.
pub fn parse_hex(s: &str, fallback: Rgba) -> Rgba {
    let h = s.trim().trim_start_matches('#');
    let full: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_string() };
    if full.len() != 6 {
        return fallback;
    }
    match u32::from_str_radix(&full, 16) {
        Ok(v) => [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255],
        Err(_) => fallback,
    }
}
