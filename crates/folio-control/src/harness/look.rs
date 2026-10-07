//! `harness.look`: folio's best picture of the work, with its numbers.
//!
//! A printed page of a document, a slide, or a range of a sheet (with the charts over it),
//! rendered by folio-layout exactly as the window and the PDF draw them, written to
//! `<data>/looks/` as a PNG. The built-in agent and `folio-mcp` hand the picture itself to the
//! model ([`crate::vision`]); the answer also carries what can be measured (pages, words,
//! headings, slide text, sums of the sheet's columns) and the problems [`super::check`] finds on
//! that page, so a model that can't see still gets the facts.

use std::path::{Path, PathBuf};

use folio_calc::{Addr, Range, Value as CellValue};
use folio_core::{Document, PageBody};
use folio_layout::Fonts;
use serde_json::{Value, json};

/// Default width of a page or slide picture, in pixels (models keep up to 1568).
pub const DEFAULT_WIDTH: u32 = 1200;
/// Most pictures kept in `<data>/looks/` (the oldest go).
const KEEP: usize = 24;
/// The largest sheet range drawn at once.
const MAX_COLS: u32 = 16;
const MAX_ROWS: u32 = 60;

/// What to look at.
#[derive(Clone, Debug, Default)]
pub struct Request {
    /// The page's index.
    pub page: usize,
    /// Documents: the printed page, 0-based.
    pub page_number: Option<usize>,
    /// Decks: the slide, 0-based.
    pub slide: Option<usize>,
    /// Sheets: the range.
    pub range: Option<Range>,
    /// Width in pixels (pages and slides).
    pub width: Option<u32>,
}

/// A picture and its numbers.
pub struct Look {
    pub png: Vec<u8>,
    pub info: Value,
}

/// Renders what `req` names. Runs layout: call it off the async threads.
pub fn render(doc: &Document, req: &Request) -> Result<Look, String> {
    let page = doc.pages.get(req.page).ok_or("No such page.")?;
    let width = req.width.unwrap_or(DEFAULT_WIDTH).clamp(200, crate::vision::MAX_SIDE);
    let problems = super::check::check(doc, Some(req.page));
    let mut fonts = Fonts::shared();
    let (png, mut info) = match &page.body {
        PageBody::Doc(d) => {
            let layout = folio_layout::layout_doc(&mut fonts, doc, req.page);
            let count = layout.pages.len().max(1);
            let n = req.page_number.unwrap_or(0);
            if n >= count {
                return Err(format!("\"{}\" has {count} printed page{} (pageNumber 1 to {count}).", page.name, if count == 1 { "" } else { "s" }));
            }
            let png = folio_layout::page_png(&mut fonts, doc, req.page, n, width);
            let outline: Vec<Value> = d.outline().into_iter().take(20).map(|(b, l, t)| json!({ "block": b, "level": l, "text": t })).collect();
            let tables = d.blocks.iter().filter(|b| matches!(b, folio_core::Block::Table(_))).count();
            let charts = d.blocks.iter().filter(|b| matches!(b, folio_core::Block::Chart(_))).count();
            (
                png,
                json!({
                    "kind": "doc",
                    "what": format!("Printed page {} of {count} of the document \"{}\"", n + 1, page.name),
                    "pageNumber": n + 1,
                    "printedPages": count,
                    "words": d.word_count(),
                    "outline": outline,
                    "tables": tables,
                    "charts": charts,
                    "setup": { "size": d.setup.size_name(), "landscape": d.setup.landscape(), "header": d.setup.header, "footer": d.setup.footer },
                }),
            )
        }
        PageBody::Deck(d) => {
            if d.slides.is_empty() {
                return Err(format!("The deck \"{}\" has no slides yet.", page.name));
            }
            let i = req.slide.unwrap_or(0);
            let slide = d.slides.get(i).ok_or_else(|| format!("\"{}\" has {} slides (slide 1 to {}).", page.name, d.slides.len(), d.slides.len()))?;
            let png = folio_layout::slide_png(&mut fonts, doc, req.page, i, width);
            let shapes: Vec<Value> = slide
                .shapes
                .iter()
                .map(|s| {
                    let (below, beside) = folio_layout::text_overflow(&mut fonts, d, s);
                    let mut v = json!({ "id": s.id, "name": s.name, "kind": s.kind.id(), "box": [s.x.round(), s.y.round(), s.w.round(), s.h.round()] });
                    let text = s.plain();
                    if !text.trim().is_empty() {
                        v["text"] = json!(text.chars().take(300).collect::<String>());
                        v["textSize"] = json!(s.text_size);
                    }
                    if below > 1.0 || beside > 0.0 {
                        v["overflow"] = json!({ "below": below.round(), "beside": beside.round() });
                    }
                    v
                })
                .collect();
            (
                png,
                json!({
                    "kind": "slide",
                    "what": format!("Slide {} of {} of the deck \"{}\"", i + 1, d.slides.len(), page.name),
                    "slide": i + 1,
                    "slides": d.slides.len(),
                    "title": slide.title(),
                    "layout": slide.layout.id(),
                    "theme": d.theme.name,
                    "size": d.size,
                    "notes": !slide.notes.is_empty(),
                    "shapes": shapes,
                }),
            )
        }
        PageBody::Sheet(s) => {
            let range = match req.range {
                Some(r) => {
                    let r = s.clip(r);
                    Range::new(r.start, Addr::new(r.end.row.min(r.start.row + MAX_ROWS * 4 - 1), r.end.col.min(r.start.col + MAX_COLS * 2 - 1)))
                }
                None => super::check::default_range(s, MAX_COLS, MAX_ROWS).unwrap_or(Range::new(Addr::new(0, 0), Addr::new(9, 5))),
            };
            let (w, h) = folio_layout::sheet_png_size(s, range);
            let scale = (crate::vision::MAX_SIDE as f32 / w.max(h)).min(1.6);
            let png = folio_layout::sheet_png(&mut fonts, doc, req.page, range, scale);
            let errors: Vec<Value> = range.iter().filter_map(|a| s.cell(a).filter(|c| matches!(c.value, CellValue::Error(_))).map(|c| json!({ "cell": a.a1(), "shows": c.display(), "formula": c.input }))).take(20).collect();
            let formulas = range.iter().filter(|a| s.cell(*a).is_some_and(|c| c.is_formula())).count();
            let columns: Vec<Value> = (range.start.col..=range.end.col)
                .filter_map(|col| {
                    let nums: Vec<f64> = (range.start.row..=range.end.row).filter_map(|row| if let CellValue::Number(n) = s.value(Addr::new(row, col)) { Some(n) } else { None }).collect();
                    if nums.is_empty() {
                        return None;
                    }
                    let header = s.display(Addr::new(range.start.row, col));
                    let sum: f64 = nums.iter().sum();
                    Some(json!({
                        "column": folio_calc::col_name(col),
                        "header": header,
                        "numbers": nums.len(),
                        "sum": round(sum),
                        "min": round(nums.iter().copied().fold(f64::INFINITY, f64::min)),
                        "max": round(nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
                    }))
                })
                .collect();
            (
                png,
                json!({
                    "kind": "sheet",
                    "what": format!("Cells {} of the sheet \"{}\"", range.a1(), page.name),
                    "range": range.a1(),
                    "usedRange": s.used_range().map(|r| r.a1()),
                    "formulas": formulas,
                    "errors": errors,
                    "columns": columns,
                    "charts": s.charts.iter().map(|c| json!({ "id": c.id, "kind": c.chart.kind, "title": c.chart.title, "source": c.chart.source })).collect::<Vec<_>>(),
                    "note": "The column sums add every number in the range, totals rows included.",
                }),
            )
        }
    };
    drop(fonts);
    if png.is_empty() {
        return Err("Couldn't draw that page.".into());
    }
    info["page"] = json!(page.name);
    info["problems"] = json!(problems.problems);
    if !problems.problems.is_empty() {
        info["check"] = json!(problems.summary());
    }
    Ok(Look { png, info })
}

fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// Writes a look's picture into `<data>/looks/`, keeping the newest [`KEEP`].
pub fn save(data_dir: &Path, png: &[u8]) -> Result<PathBuf, String> {
    let dir = data_dir.join("looks");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let path = dir.join(format!("look-{nanos}.png"));
    std::fs::write(&path, png).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "png")).collect();
        if files.len() > KEEP {
            files.sort();
            for old in &files[..files.len() - KEEP] {
                let _ = std::fs::remove_file(old);
            }
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::PageKind;

    fn ink(png: &[u8]) -> usize {
        let p = tiny_skia::Pixmap::decode_png(png).unwrap();
        p.pixels().iter().filter(|c| (c.red() as u16 + c.green() as u16 + c.blue() as u16) < 200).count()
    }

    #[test]
    fn looks_at_pages_slides_and_ranges() {
        let mut d = Document::new("t");
        let s = d.add_page(PageKind::Sheet, Some("Budget"), None).unwrap();
        {
            let sh = d.page_mut(s).sheet_mut().unwrap();
            for (a, v) in [("A1", "Item"), ("B1", "Cost"), ("A2", "Rent"), ("B2", "1200"), ("A3", "Food"), ("B3", "300"), ("A4", "Total"), ("B4", "=SUM(B2:B3)")] {
                sh.set_input(Addr::parse(a).unwrap(), v);
            }
        }
        let k = d.add_page(PageKind::Deck, Some("Slides"), None).unwrap();
        let deck = d.page_mut(k).deck_mut().unwrap();
        deck.slides = vec![folio_core::deck::Slide::with_layout(folio_core::deck::SlideLayout::TitleContent, deck.size, "Costs are flat", "Rent\nFood")];
        let t = d.add_page(PageKind::Doc, Some("Report"), None).unwrap();
        d.page_mut(t).doc_mut().unwrap().blocks = vec![folio_core::Block::Paragraph(folio_core::Paragraph::new(folio_core::ParaStyle::Title, "Report"))].into();
        let mut ed = folio_core::Editor::new(d);
        ed.recalc_all();
        let d = ed.doc().clone();

        let sheet = render(&d, &Request { page: s, ..Default::default() }).unwrap();
        assert_eq!(sheet.info["range"], "A1:B4");
        assert_eq!(sheet.info["columns"][0]["sum"], 3000.0);
        assert!(ink(&sheet.png) > 100);
        let slide = render(&d, &Request { page: k, slide: Some(0), width: Some(960), ..Default::default() }).unwrap();
        assert_eq!(slide.info["title"], "Costs are flat");
        let pic = tiny_skia::Pixmap::decode_png(&slide.png).unwrap();
        assert_eq!((pic.width(), pic.height()), (960, 540));
        let page = render(&d, &Request { page: t, ..Default::default() }).unwrap();
        assert_eq!(page.info["printedPages"], 1);
        assert!(render(&d, &Request { page: t, page_number: Some(3), ..Default::default() }).unwrap_err().contains("1 printed page"));

        let dir = tempfile::tempdir().unwrap();
        let path = save(dir.path(), &page.png).unwrap();
        assert!(path.starts_with(dir.path().join("looks")) && path.is_file());
    }
}
