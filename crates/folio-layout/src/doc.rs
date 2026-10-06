//! A document page paginated: what goes on each printed page, where.
//!
//! Paragraphs split between pages by line (at least two lines stay together at either end
//! when possible; headings stay with what follows them), tables split between rows (the
//! header row repeats), pictures and charts move whole to the next page when they don't fit,
//! footnotes sit at the foot of the page their anchor is on, and headers and footers get
//! `{page}`, `{pages}` and `{title}` filled in. Paragraph layouts come from the cache in
//! [`Fonts`], so paginating again after an edit only reshapes the paragraph that changed.

use std::sync::Arc;

use folio_core::text::{Align, Block, ListKind, ParaStyle, Table, TableCell};
use folio_core::{Document, Id, Paragraph, Run, RunStyle};

use crate::fonts::Fonts;
use crate::text::{ParaCtx, ParaLayout, layout_paragraph_cached};
use crate::{Rgba, parse_hex};

/// Padding inside table cells, in points.
pub const CELL_PAD: f32 = 4.0;
/// Table grid lines.
pub const TABLE_BORDER: Rgba = [150, 150, 150, 255];
pub const TABLE_BORDER_WIDTH: f32 = 0.75;
/// Header row and banded row tints.
pub const HEADER_FILL: Rgba = [0, 0, 0, 18];
pub const BAND_FILL: Rgba = [0, 0, 0, 9];

/// One table cell on a row.
#[derive(Clone, Debug)]
pub struct CellBox {
    pub col: usize,
    /// The cell's box: left edge and width in page points (the row gives y and height).
    pub x: f32,
    pub w: f32,
    pub layout: Arc<ParaLayout>,
    pub fill: Option<Rgba>,
    /// Where the cell's paragraph layout has its origin on the page (padding included): paint
    /// glyphs and hit-test carets relative to this point.
    pub tx: f32,
    pub ty: f32,
}

/// Something placed on a page, in points from the page's top-left.
#[derive(Clone, Debug)]
pub enum Placed {
    /// Lines `lines` of block `block`'s paragraph. `x` is where the layout's x coordinates
    /// start (the text's left margin; list indents are inside the layout); the first of the
    /// lines has its top at `y` (line y values are from the paragraph's top: subtract
    /// `layout.lines[lines.start].y`).
    Para { block: usize, x: f32, y: f32, layout: Arc<ParaLayout>, lines: std::ops::Range<usize> },
    /// One row of a table: `row` is the table's row index (a repeated header is row 0 with
    /// `header` set). Grid lines in `border` colour, `border_width` thick, go around every cell.
    TableRow { block: usize, row: usize, x: f32, y: f32, w: f32, height: f32, cells: Vec<CellBox>, header: bool, shaded: bool, border: Rgba, border_width: f32 },
    Image { block: usize, x: f32, y: f32, w: f32, h: f32, media: Id },
    /// An image's caption (its layout's origin at `x`, `y`).
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

    /// The vertical extent `(top, bottom)` on the page.
    pub fn span(&self) -> (f32, f32) {
        match self {
            Placed::Para { y, layout, lines, .. } => {
                let (a, b) = (lines.start.min(layout.lines.len()), lines.end.min(layout.lines.len()));
                let h = if a < b { layout.lines[b - 1].y + layout.lines[b - 1].height - layout.lines[a].y } else { 0.0 };
                (*y, y + h)
            }
            Placed::TableRow { y, height, .. } => (*y, y + height),
            Placed::Image { y, h, .. } | Placed::Chart { y, h, .. } => (*y, y + h),
            Placed::Caption { y, layout, .. } => (*y, y + layout.height),
            Placed::PageBreak { y, .. } => (*y, *y),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PageLayout {
    pub items: Vec<Placed>,
    /// Header and footer, laid out, with the y of their top (their x is the left margin).
    pub header: Option<(f32, Arc<ParaLayout>)>,
    pub footer: Option<(f32, Arc<ParaLayout>)>,
    /// Footnotes at the foot of the page, each with the y of its top (x is the left margin).
    pub footnotes: Vec<(f32, Arc<ParaLayout>)>,
    /// The short rule above the footnotes: (x, y, width).
    pub footnote_rule: Option<(f32, f32, f32)>,
}

#[derive(Clone, Debug, Default)]
pub struct DocLayout {
    /// Page size in points.
    pub width: f32,
    pub height: f32,
    /// The text's left margin and width.
    pub left: f32,
    pub text_width: f32,
    pub pages: Vec<PageLayout>,
}

impl DocLayout {
    /// The printed page (and item on it) where a block starts.
    pub fn find_block(&self, block: usize) -> Option<(usize, usize)> {
        self.pages.iter().enumerate().find_map(|(pi, p)| p.items.iter().position(|it| it.block() == block).map(|ii| (pi, ii)))
    }
}

const FOOTNOTE_SIZE: f32 = 9.0;
const FOOTNOTE_GAP: f32 = 12.0;
const HEADER_SIZE: f32 = 9.0;
const HEADER_COLOR: &str = "#6b6b6b";

struct Pager {
    pages: Vec<PageLayout>,
    y: f32,
    top: f32,
    bottom: f32,
    /// Height the current page's footnotes take (with the gap above them).
    notes_h: f32,
    notes: Vec<Arc<ParaLayout>>,
    empty: bool,
}

impl Pager {
    fn limit(&self, extra_notes: f32) -> f32 {
        let n = self.notes_h + extra_notes;
        self.bottom - if n > 0.0 { n + FOOTNOTE_GAP } else { 0.0 }
    }

    fn place(&mut self, item: Placed) {
        self.empty = false;
        self.pages.last_mut().unwrap().items.push(item);
    }

    fn add_notes(&mut self, notes: &[Arc<ParaLayout>]) {
        for n in notes {
            self.notes_h += n.height;
            self.notes.push(n.clone());
        }
    }

    fn new_page(&mut self) {
        self.finish_notes();
        self.pages.push(PageLayout::default());
        self.y = self.top;
        self.empty = true;
    }

    fn finish_notes(&mut self) {
        if self.notes.is_empty() {
            return;
        }
        let page = self.pages.last_mut().unwrap();
        let mut y = self.bottom - self.notes_h;
        page.footnote_rule = Some((0.0, y - FOOTNOTE_GAP / 2.0, 0.0));
        for n in self.notes.drain(..) {
            page.footnotes.push((y, n.clone()));
            y += n.height;
        }
        self.notes_h = 0.0;
    }
}

/// The note text laid out as a footnote: its number, then the text, small.
fn footnote_layout(fonts: &mut Fonts, number: u32, text: &str, width: f32) -> Arc<ParaLayout> {
    let small = RunStyle { size: Some(FOOTNOTE_SIZE), ..Default::default() };
    let p = Paragraph {
        id: Id(String::new()),
        runs: vec![Run::styled(number.to_string(), RunStyle { superscript: true, ..small.clone() }), Run::styled(format!(" {text}"), small)],
        ..Default::default()
    };
    let mut l = (*layout_paragraph_cached(fonts, &p, width, &ParaCtx::default())).clone();
    l.space_before = 0.0;
    l.space_after = 2.0;
    l.height += 2.0;
    Arc::new(l)
}

fn header_layout(fonts: &mut Fonts, text: &str, width: f32) -> Arc<ParaLayout> {
    let p = Paragraph {
        id: Id(String::new()),
        align: Align::Center,
        runs: vec![Run::styled(text, RunStyle { size: Some(HEADER_SIZE), color: Some(HEADER_COLOR.into()), ..Default::default() })],
        ..Default::default()
    };
    layout_paragraph_cached(fonts, &p, width, &ParaCtx::default())
}

/// A table's rows: its own, or a linked range's values now.
pub fn table_rows(doc: &Document, t: &Table) -> Vec<Vec<TableCell>> {
    if let Some(link) = &t.link
        && let Ok(rows) = folio_core::links::table_text(doc, link)
        && !rows.is_empty()
    {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        return rows
            .into_iter()
            .enumerate()
            .map(|(r, row)| {
                (0..cols)
                    .map(|c| {
                        let mut cell = TableCell::text(row.get(c).cloned().unwrap_or_default());
                        // Keep the stored table's cell look (fill, alignment) where it has one.
                        if let Some(old) = t.cell(r, c) {
                            cell.fill = old.fill.clone();
                            cell.align = old.align;
                        } else if r > 0 && row.get(c).is_some_and(|v| v.parse::<f64>().is_ok()) {
                            cell.align = Align::Right;
                        }
                        cell
                    })
                    .collect()
            })
            .collect();
    }
    t.rows.clone()
}

/// A table laid out at a width: each row's cells and height (y from the row's top).
pub(crate) struct TableGrid {
    pub rows: Vec<(f32, Vec<CellBox>, bool, bool)>,
}

/// Lays out a table's rows at `x`, `width` (cells' y are relative to the row's top: add the
/// row's y to `ty`). `scale` multiplies text sizes (slides).
pub(crate) fn table_grid(fonts: &mut Fonts, doc: &Document, t: &Table, x: f32, width: f32, ctx: &ParaCtx) -> TableGrid {
    let rows = table_rows(doc, t);
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let fr = if t.link.is_some() && t.widths.len() != cols { vec![1.0 / cols as f32; cols] } else { t.fractions() };
    let fr: Vec<f32> = if fr.len() == cols { fr } else { vec![1.0 / cols as f32; cols] };
    let pad = CELL_PAD * ctx.scale.max(0.5);
    let mut out = vec![];
    for (ri, row) in rows.iter().enumerate() {
        let header = t.header && ri == 0;
        let shaded = t.banded && !header && (ri - t.header as usize) % 2 == 1;
        let mut cx = x;
        let mut cells = vec![];
        let mut h: f32 = 0.0;
        for (ci, f) in fr.iter().enumerate() {
            let w = f * width;
            let empty = TableCell::default();
            let cell = row.get(ci).unwrap_or(&empty);
            let mut p = cell.as_para();
            if header {
                for r in &mut p.runs {
                    r.style.bold = true;
                }
                if p.runs.is_empty() {
                    p.runs.push(Run::bold(""));
                }
            }
            let l = layout_paragraph_cached(fonts, &p, (w - 2.0 * pad).max(4.0), ctx);
            h = h.max(l.height);
            let fill = cell.fill.as_deref().map(|c| parse_hex(c, [255, 255, 255, 255])).or(if header {
                Some(HEADER_FILL)
            } else if shaded {
                Some(BAND_FILL)
            } else {
                None
            });
            cells.push(CellBox { col: ci, x: cx, w, layout: l, fill, tx: cx + pad, ty: pad });
            cx += w;
        }
        out.push((h + 2.0 * pad, cells, header, shaded));
    }
    TableGrid { rows: out }
}

/// List numbers for every paragraph of a flow (numbered items count per level; a paragraph
/// outside a list starts the count again).
pub fn list_numbers(blocks: &folio_core::text::Flow) -> Vec<Option<usize>> {
    let mut counters = [0usize; 8];
    blocks
        .iter()
        .map(|b| match b.para() {
            Some(p) if p.list.is_some() => {
                let lvl = (p.level as usize).min(7);
                for c in counters.iter_mut().skip(lvl + 1) {
                    *c = 0;
                }
                if p.list == Some(ListKind::Number) {
                    counters[lvl] += 1;
                    Some(counters[lvl])
                } else {
                    None
                }
            }
            Some(_) => {
                counters = [0; 8];
                None
            }
            None => None,
        })
        .collect()
}

/// Paginates document page `page` of `doc`.
pub fn layout_doc(fonts: &mut Fonts, doc: &Document, page: usize) -> DocLayout {
    let Some(t) = doc.pages.get(page).and_then(|p| p.doc()) else { return DocLayout::default() };
    let s = &t.setup;
    let tw = s.text_width();
    let left = s.margin_left;
    let top = s.margin_top;
    let bottom = (s.height - s.margin_bottom).max(top + 36.0);
    let mut pg = Pager { pages: vec![PageLayout::default()], y: top, top, bottom, notes_h: 0.0, notes: vec![], empty: true };
    let numbers = list_numbers(&t.blocks);
    let mut note_no = 1u32;
    let blocks: Vec<&Block> = t.blocks.iter().collect();

    // Minimal height the next block needs on a page (headings keep with it).
    let next_min = |fonts: &mut Fonts, i: usize, note_no: u32| -> f32 {
        match blocks.get(i + 1) {
            Some(Block::Paragraph(p)) => {
                let ctx = ParaCtx { number: numbers[i + 1], first_note: note_no, ..Default::default() };
                let l = layout_paragraph_cached(fonts, p, tw, &ctx);
                let k = l.lines.len().min(2);
                l.space_before + if k > 0 { l.lines[k - 1].y + l.lines[k - 1].height } else { 0.0 }
            }
            Some(Block::Image(_)) | Some(Block::Chart(_)) | Some(Block::Table(_)) => 48.0,
            _ => 0.0,
        }
    };

    for (bi, b) in blocks.iter().enumerate() {
        match b {
            Block::Paragraph(p) => {
                let ctx = ParaCtx { number: numbers[bi], first_note: note_no, ..Default::default() };
                let l = layout_paragraph_cached(fonts, p, tw, &ctx);
                note_no += l.notes.len() as u32;
                let notes: Vec<Arc<ParaLayout>> = l.notes.iter().map(|(_, n, text)| footnote_layout(fonts, *n, text, tw)).collect();
                // The line each note's anchor is on.
                let note_line: Vec<usize> = l.notes.iter().map(|(off, _, _)| l.lines.iter().position(|ln| *off <= ln.end).unwrap_or(l.lines.len().saturating_sub(1))).collect();
                let line_notes = |li: usize| -> Vec<usize> { (0..note_line.len()).filter(|i| note_line[*i] == li).collect() };
                let space_before = if pg.empty && pg.pages.len() > 1 { 0.0 } else { l.space_before };
                let n = l.lines.len();
                // Headings keep with the next block.
                if p.style.is_heading() && !pg.empty {
                    let need = space_before + l.height + next_min(fonts, bi, note_no);
                    if pg.y + need > pg.limit(0.0) {
                        pg.new_page();
                    }
                }
                let mut sb = if pg.empty && pg.pages.len() > 1 { 0.0 } else { l.space_before };
                let mut i = 0;
                let mut placed_notes = vec![false; notes.len()];
                while i < n {
                    let first_y = l.lines[i].y;
                    let mut k = i;
                    let mut extra = 0.0;
                    let mut taken: Vec<usize> = vec![];
                    while k < n {
                        let h = l.lines[k].y + l.lines[k].height - first_y;
                        let mut nh = 0.0;
                        let mut these = vec![];
                        for ni in line_notes(k) {
                            if !placed_notes[ni] && !taken.contains(&ni) {
                                nh += notes[ni].height;
                                these.push(ni);
                            }
                        }
                        if pg.y + sb + h <= pg.limit(extra + nh) + 0.01 {
                            extra += nh;
                            taken.extend(these);
                            k += 1;
                        } else {
                            break;
                        }
                    }
                    let fit = k - i;
                    let remaining = n - i;
                    let mut take = fit;
                    if take < remaining {
                        if remaining - take < 2 && remaining >= 2 {
                            take = remaining.saturating_sub(2);
                        }
                        if i == 0 && take < 2 && remaining >= 2 {
                            take = 0;
                        }
                        if take == 0 && pg.empty {
                            take = fit.max(1);
                        }
                    }
                    if take == 0 {
                        pg.new_page();
                        sb = 0.0;
                        continue;
                    }
                    let end = i + take;
                    pg.place(Placed::Para { block: bi, x: left, y: pg.y + sb, layout: l.clone(), lines: i..end });
                    // Notes anchored on the placed lines.
                    let mut ns = vec![];
                    for li in i..end {
                        for ni in line_notes(li) {
                            if !placed_notes[ni] {
                                placed_notes[ni] = true;
                                ns.push(notes[ni].clone());
                            }
                        }
                    }
                    pg.add_notes(&ns);
                    pg.y += sb + l.lines[end - 1].y + l.lines[end - 1].height - first_y;
                    sb = 0.0;
                    i = end;
                    if i < n {
                        pg.new_page();
                    }
                }
                // Notes whose anchor wasn't matched to a line (shouldn't happen) go here.
                let rest: Vec<Arc<ParaLayout>> = placed_notes.iter().enumerate().filter(|(_, p)| !**p).map(|(i, _)| notes[i].clone()).collect();
                pg.add_notes(&rest);
                pg.y += l.space_after;
            }
            Block::Table(tb) => {
                let ctx = ParaCtx::default();
                let grid = table_grid(fonts, doc, tb, left, tw, &ctx);
                let header = if tb.header { grid.rows.first().cloned() } else { None };
                for (ri, (h, cells, is_header, shaded)) in grid.rows.iter().enumerate() {
                    if pg.y + h > pg.limit(0.0) + 0.01 && !pg.empty {
                        pg.new_page();
                        // Repeat the header row.
                        if let Some((hh, hcells, _, _)) = &header
                            && ri > 0
                        {
                            pg.place(Placed::TableRow { block: bi, row: 0, x: left, y: pg.y, w: tw, height: *hh, cells: shift_cells(hcells, pg.y), header: true, shaded: false, border: TABLE_BORDER, border_width: TABLE_BORDER_WIDTH });
                            pg.y += hh;
                        }
                    }
                    pg.place(Placed::TableRow { block: bi, row: ri, x: left, y: pg.y, w: tw, height: *h, cells: shift_cells(cells, pg.y), header: *is_header, shaded: *shaded, border: TABLE_BORDER, border_width: TABLE_BORDER_WIDTH });
                    pg.y += h;
                }
                pg.y += 10.0;
            }
            Block::Image(img) => {
                let media = doc.media.get(&img.media);
                let w = if img.width > 0.0 { img.width.min(tw) } else { tw };
                let aspect = media.filter(|m| m.width > 0 && m.height > 0).map(|m| m.height as f32 / m.width as f32).unwrap_or(0.6);
                let (mut w, mut h) = (w, w * aspect);
                let max_h = (bottom - top) * 0.92;
                if h > max_h {
                    w *= max_h / h;
                    h = max_h;
                }
                let x = match img.align {
                    Align::Left | Align::Justify => left,
                    Align::Center => left + (tw - w) / 2.0,
                    Align::Right => left + tw - w,
                };
                let caption = (!img.caption.is_empty()).then(|| {
                    let p = Paragraph { id: Id(String::new()), style: ParaStyle::Caption, align: if img.align == Align::Justify { Align::Left } else { img.align }, runs: vec![Run::plain(img.caption.clone())], ..Default::default() };
                    layout_paragraph_cached(fonts, &p, tw, &ParaCtx::default())
                });
                let cap_h = caption.as_ref().map(|c| 6.0 + c.height).unwrap_or(0.0);
                let sb = if pg.empty { 0.0 } else { 6.0 };
                if pg.y + sb + h + cap_h > pg.limit(0.0) && !pg.empty {
                    pg.new_page();
                }
                let sb = if pg.empty { 0.0 } else { sb };
                pg.y += sb;
                pg.place(Placed::Image { block: bi, x, y: pg.y, w, h, media: img.media.clone() });
                pg.y += h;
                if let Some(c) = caption {
                    pg.y += 6.0;
                    let ch = c.height + c.space_after;
                    pg.place(Placed::Caption { block: bi, x: left, y: pg.y, layout: c });
                    pg.y += ch;
                } else {
                    pg.y += 10.0;
                }
            }
            Block::Chart(c) => {
                let h = c.height.max(40.0).min((bottom - top) * 0.92);
                let sb = if pg.empty { 0.0 } else { 6.0 };
                if pg.y + sb + h > pg.limit(0.0) && !pg.empty {
                    pg.new_page();
                }
                let sb = if pg.empty { 0.0 } else { sb };
                pg.y += sb;
                pg.place(Placed::Chart { block: bi, x: left, y: pg.y, w: tw, h });
                pg.y += h + 12.0;
            }
            Block::PageBreak { .. } => {
                pg.place(Placed::PageBreak { block: bi, y: pg.y });
                pg.new_page();
            }
        }
    }
    pg.finish_notes();

    // Headers and footers, now that the page count is known.
    let total = pg.pages.len();
    let mut pages = pg.pages;
    let fill = |text: &str, n: usize| text.replace("{page}", &n.to_string()).replace("{pages}", &total.to_string()).replace("{title}", &doc.title);
    for (i, p) in pages.iter_mut().enumerate() {
        if let Some((_, y, _)) = p.footnote_rule {
            p.footnote_rule = Some((left, y, (tw / 3.0).min(144.0)));
        }
        if s.different_first && i == 0 {
            continue;
        }
        if !s.header.trim().is_empty() {
            let l = header_layout(fonts, &fill(&s.header, i + 1), tw);
            let y = ((s.margin_top - l.height) / 2.0).max(4.0);
            p.header = Some((y, l));
        }
        if !s.footer.trim().is_empty() {
            let l = header_layout(fonts, &fill(&s.footer, i + 1), tw);
            let y = (s.height - s.margin_bottom / 2.0 - l.height / 2.0).min(s.height - l.height - 4.0);
            p.footer = Some((y, l));
        }
    }
    DocLayout { width: s.width, height: s.height, left, text_width: tw, pages }
}

fn shift_cells(cells: &[CellBox], y: f32) -> Vec<CellBox> {
    cells.iter().map(|c| CellBox { ty: c.ty + y, ..c.clone() }).collect()
}

/// Printed pages of a document page (uses the shared font system: don't call it while holding
/// [`Fonts::shared`]).
pub fn page_count(doc: &Document, page: usize) -> usize {
    layout_doc(&mut Fonts::shared(), doc, page).pages.len()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use folio_core::text::{ChartBlock, ImageBlock};
    use folio_core::{Chart, ChartKind, PageKind};

    pub(crate) fn fonts() -> std::sync::MutexGuard<'static, Fonts> {
        static F: std::sync::OnceLock<std::sync::Mutex<Fonts>> = std::sync::OnceLock::new();
        F.get_or_init(|| std::sync::Mutex::new(Fonts::bundled_only())).lock().unwrap_or_else(|e| e.into_inner())
    }

    const LOREM: &str = "Folio sets every paragraph with the bundled faces so that what is on screen is what prints. Lines break at word boundaries and pages break between lines, keeping at least two lines together at either end of a page when it can.";

    pub(crate) fn doc_with(blocks: Vec<Block>) -> Document {
        let mut doc = Document::empty("Test");
        let i = doc.add_page(PageKind::Doc, Some("Doc"), None).unwrap();
        let t = doc.page_mut(i).doc_mut().unwrap();
        t.blocks = blocks.into_iter().collect();
        doc
    }

    fn check_inside(d: &DocLayout, bottom: f32) {
        for (pi, p) in d.pages.iter().enumerate() {
            for it in &p.items {
                let (a, b) = it.span();
                assert!(a >= 71.9, "page {pi}: item starts at {a}");
                assert!(b <= bottom + 0.5 || matches!(it, Placed::PageBreak { .. }), "page {pi}: {:?} ends at {b}", it.block());
            }
        }
    }

    #[test]
    fn paragraphs_paginate() {
        let blocks: Vec<Block> = (0..80).map(|i| Block::Paragraph(Paragraph::new(if i % 10 == 0 { ParaStyle::Heading1 } else { ParaStyle::Normal }, LOREM))).collect();
        let doc = doc_with(blocks);
        let mut f = fonts();
        let d = layout_doc(&mut f, &doc, 0);
        assert!(d.pages.len() > 3, "{}", d.pages.len());
        check_inside(&d, 842.0 - 72.0);
        // Every line of every paragraph is placed exactly once, in order.
        let mut seen: Vec<(usize, usize)> = vec![];
        for p in &d.pages {
            for it in &p.items {
                if let Placed::Para { block, lines, layout, .. } = it {
                    assert!(lines.end <= layout.lines.len());
                    for l in lines.clone() {
                        seen.push((*block, l));
                    }
                }
            }
        }
        let mut sorted = seen.clone();
        sorted.sort();
        assert_eq!(seen, sorted);
        sorted.dedup();
        assert_eq!(seen.len(), sorted.len());
        // Headings never end a page.
        for p in &d.pages {
            if let Some(Placed::Para { block, .. }) = p.items.last() {
                assert!(!doc.pages[0].doc().unwrap().blocks[*block].para().unwrap().style.is_heading());
            }
            assert_eq!(p.footer.as_ref().map(|f| f.1.text.clone()), Some(format!("{}", d.pages.iter().position(|q| std::ptr::eq(q, p)).unwrap() + 1)));
        }
    }

    #[test]
    fn tables_split_between_rows_and_repeat_header() {
        let rows: Vec<Vec<String>> = (0..90).map(|r| vec![format!("Row {r}"), format!("{}", r * 3), "Some longer text in the third column".into()]).collect();
        let mut t = Table::from_text(rows, true);
        t.banded = true;
        let doc = doc_with(vec![Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Before")), Block::Table(t)]);
        let mut f = fonts();
        let d = layout_doc(&mut f, &doc, 0);
        assert!(d.pages.len() >= 2);
        check_inside(&d, 842.0 - 72.0);
        for p in d.pages.iter().skip(1) {
            match &p.items[0] {
                Placed::TableRow { header, row, cells, y, .. } => {
                    assert!(*header && *row == 0);
                    assert_eq!(cells.len(), 3);
                    assert!((cells[0].ty - y - CELL_PAD).abs() < 0.01);
                    assert!((cells[1].x - (72.0 + d.text_width / 3.0)).abs() < 0.01);
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn images_charts_breaks_notes_and_headers() {
        let mut doc = doc_with(vec![]);
        let png = {
            let mut b = vec![];
            image::RgbaImage::from_pixel(400, 200, image::Rgba([10, 10, 10, 255])).write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
            b
        };
        let media = doc.add_media("pic.png", png);
        let note = Paragraph::with_runs(ParaStyle::Normal, vec![Run::plain("A claim"), Run::styled(".", RunStyle { note: Some("The source of the claim.".into()), ..Default::default() }), Run::plain(" More text.")]);
        let blocks = vec![
            Block::Paragraph(Paragraph::new(ParaStyle::Title, "Report")),
            Block::Paragraph(note),
            Block::Image(ImageBlock { id: Id::new(), media: media.clone(), width: 200.0, caption: "A picture".into(), alt: String::new(), align: Align::Center }),
            Block::Chart(ChartBlock { id: Id::new(), chart: Chart::new(ChartKind::Column, "'Sheet'!A1:B3"), height: 200.0 }),
            Block::PageBreak { id: Id::new() },
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Second page")),
        ];
        {
            let t = doc.page_mut(0).doc_mut().unwrap();
            t.blocks = blocks.into_iter().collect();
            t.setup.header = "{title} – {page} of {pages}".into();
            t.setup.different_first = true;
        }
        let mut f = fonts();
        let d = layout_doc(&mut f, &doc, 0);
        assert_eq!(d.pages.len(), 2);
        let p0 = &d.pages[0];
        assert!(p0.header.is_none() && p0.footer.is_none());
        assert_eq!(d.pages[1].header.as_ref().unwrap().1.text, "Test – 2 of 2");
        let img = p0.items.iter().find_map(|it| if let Placed::Image { x, w, h, .. } = it { Some((*x, *w, *h)) } else { None }).unwrap();
        assert_eq!(img.1, 200.0);
        assert!((img.2 - 100.0).abs() < 0.01);
        assert!((img.0 - (72.0 + (d.text_width - 200.0) / 2.0)).abs() < 0.01);
        assert!(p0.items.iter().any(|it| matches!(it, Placed::Caption { .. })));
        assert!(p0.items.iter().any(|it| matches!(it, Placed::Chart { w, .. } if (*w - d.text_width).abs() < 0.01)));
        assert_eq!(p0.footnotes.len(), 1);
        assert!(p0.footnotes[0].1.text.starts_with("1 The source"));
        let (fy, _) = p0.footnotes[0];
        assert!(fy > 700.0 && fy + p0.footnotes[0].1.height <= 842.0 - 72.0 + 0.1);
        assert!(p0.footnote_rule.is_some());
    }

    #[test]
    fn numbered_lists_count() {
        let mut blocks = vec![];
        for i in 0..3 {
            blocks.push(Block::Paragraph(Paragraph::new(ParaStyle::Normal, format!("Item {i}")).list(ListKind::Number, 0)));
        }
        blocks.push(Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Sub").list(ListKind::Number, 1)));
        blocks.push(Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Back").list(ListKind::Number, 0)));
        blocks.push(Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Plain")));
        blocks.push(Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Again").list(ListKind::Number, 0)));
        let flow: folio_core::text::Flow = blocks.into_iter().collect();
        assert_eq!(list_numbers(&flow), vec![Some(1), Some(2), Some(3), Some(1), Some(4), None, Some(1)]);
    }

    #[test]
    #[cfg_attr(debug_assertions, ignore)]
    fn speed_50_pages() {
        let mut blocks = vec![];
        for i in 0..400 {
            blocks.push(Block::Paragraph(Paragraph::new(if i % 12 == 0 { ParaStyle::Heading2 } else { ParaStyle::Normal }, format!("{i} {LOREM} {LOREM}"))));
        }
        let mut doc = doc_with(blocks);
        let mut f = Fonts::bundled_only();
        let t0 = std::time::Instant::now();
        let d = layout_doc(&mut f, &doc, 0);
        let cold = t0.elapsed();
        assert!(d.pages.len() >= 50, "{} pages", d.pages.len());
        // One keystroke in one paragraph.
        {
            let t = doc.page_mut(0).doc_mut().unwrap();
            t.blocks[200].para_mut().unwrap().insert(5, "x", RunStyle::default());
        }
        let t1 = std::time::Instant::now();
        let d2 = layout_doc(&mut f, &doc, 0);
        let warm = t1.elapsed();
        assert_eq!(d2.pages.len(), d.pages.len());
        eprintln!("50+ pages: cold {cold:?}, after one edit {warm:?}");
        assert!(cold.as_millis() < 600, "cold {cold:?}");
        assert!(warm.as_millis() < 40, "warm {warm:?}");
    }
}
