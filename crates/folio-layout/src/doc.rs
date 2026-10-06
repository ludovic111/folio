//! A document page paginated: what goes on each printed page, where.
//!
//! PLACEHOLDER: the real pagination replaces `layout_doc`.

use std::sync::Arc;

use folio_core::{Document, Id};

use crate::fonts::Fonts;
use crate::text::{ParaCtx, ParaLayout, layout_paragraph};
use crate::Rgba;

/// One table cell on a row.
#[derive(Clone, Debug)]
pub struct CellBox {
    pub col: usize,
    pub x: f32,
    pub w: f32,
    pub layout: Arc<ParaLayout>,
    pub fill: Option<Rgba>,
}

/// Something placed on a page, in points from the page's top-left.
#[derive(Clone, Debug)]
pub enum Placed {
    /// Lines `lines` of block `block`'s paragraph; the first of them has its top at `y`
    /// (line y values are from the paragraph's top: subtract `layout.lines[lines.start].y`).
    Para { block: usize, x: f32, y: f32, layout: Arc<ParaLayout>, lines: std::ops::Range<usize> },
    /// One row of a table.
    TableRow { block: usize, row: usize, x: f32, y: f32, w: f32, height: f32, cells: Vec<CellBox>, header: bool, shaded: bool },
    Image { block: usize, x: f32, y: f32, w: f32, h: f32, media: Id },
    /// An image's caption.
    Caption { block: usize, x: f32, y: f32, layout: Arc<ParaLayout> },
    Chart { block: usize, x: f32, y: f32, w: f32, h: f32 },
    PageBreak { block: usize, y: f32 },
}

impl Placed {
    pub fn block(&self) -> usize {
        match self {
            Placed::Para { block, .. } | Placed::TableRow { block, .. } | Placed::Image { block, .. } | Placed::Caption { block, .. } | Placed::Chart { block, .. } | Placed::PageBreak { block, .. } => *block,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PageLayout {
    pub items: Vec<Placed>,
    /// Header and footer, laid out, with the y of their top.
    pub header: Option<(f32, Arc<ParaLayout>)>,
    pub footer: Option<(f32, Arc<ParaLayout>)>,
    /// Footnotes at the foot of the page, each with the y of its top.
    pub footnotes: Vec<(f32, Arc<ParaLayout>)>,
}

#[derive(Clone, Debug, Default)]
pub struct DocLayout {
    /// Page size in points.
    pub width: f32,
    pub height: f32,
    pub pages: Vec<PageLayout>,
}

/// Paginates document page `page` of `doc`.
pub fn layout_doc(fonts: &mut Fonts, doc: &Document, page: usize) -> DocLayout {
    let Some(t) = doc.pages.get(page).and_then(|p| p.doc()) else { return DocLayout::default() };
    let s = &t.setup;
    let mut pages = vec![PageLayout::default()];
    let mut y = s.margin_top;
    for (i, b) in t.blocks.iter().enumerate() {
        if let Some(p) = b.para() {
            let l = Arc::new(layout_paragraph(fonts, p, s.text_width(), &ParaCtx::default()));
            if y + l.height > s.height - s.margin_bottom && y > s.margin_top {
                pages.push(PageLayout::default());
                y = s.margin_top;
            }
            let n = l.lines.len();
            pages.last_mut().unwrap().items.push(Placed::Para { block: i, x: s.margin_left, y: y + l.space_before, layout: l.clone(), lines: 0..n });
            y += l.space_before + l.height + l.space_after;
        }
    }
    DocLayout { width: s.width, height: s.height, pages }
}

/// Printed pages of a document page.
pub fn page_count(doc: &Document, page: usize) -> usize {
    layout_doc(&mut Fonts::new(), doc, page).pages.len()
}
