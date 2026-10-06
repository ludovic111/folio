//! Live links: a document table, a slide table or a chart anywhere reads a sheet range.
//!
//! A link is a reference like `'Budget'!A1:D8` (a sheet name is required). Linked things are
//! drawn and exported from the sheet's computed values as they are now, so changing the sheet
//! changes them everywhere.

use folio_calc::{Range, SheetRange, Value};

use crate::{Chart, ChartData, Document, Error, Result, bail};

/// A range's values and shown text, row by row.
pub type Grid = Vec<Vec<(Value, String)>>;

/// Resolves `'Sheet'!A1:B3` to the sheet's page index and the range.
pub fn resolve(doc: &Document, link: &str) -> Result<(usize, Range)> {
    let sr = SheetRange::parse(link.trim().trim_start_matches('=')).ok_or_else(|| Error(format!("`{link}` isn't a range like 'Sheet 1'!A1:C10.")))?;
    let Some(name) = sr.sheet.as_deref() else {
        let sheets: Vec<&str> = doc.sheets().into_iter().map(|(_, n)| n).collect();
        return bail(format!("`{link}` needs a sheet name, like '{}'!{}.", sheets.first().copied().unwrap_or("Sheet"), sr.range.a1()));
    };
    let i = doc.pages.iter().position(|p| p.kind() == crate::PageKind::Sheet && p.name.eq_ignore_ascii_case(name)).ok_or_else(|| {
        let sheets: Vec<String> = doc.sheets().into_iter().map(|(_, n)| format!("\"{n}\"")).collect();
        Error(format!("No sheet \"{name}\". Sheets: {}.", if sheets.is_empty() { "none".into() } else { sheets.join(", ") }))
    })?;
    Ok((i, sr.range))
}

/// The values of a linked range (clipped to the sheet's used area).
pub fn grid(doc: &Document, link: &str) -> Result<Grid> {
    let (i, r) = resolve(doc, link)?;
    Ok(doc.pages[i].sheet().map(|s| s.grid(r)).unwrap_or_default())
}

/// A linked table's text, row by row.
pub fn table_text(doc: &Document, link: &str) -> Result<Vec<Vec<String>>> {
    Ok(grid(doc, link)?.into_iter().map(|row| row.into_iter().map(|c| c.1).collect()).collect())
}

/// What a chart draws, read from its sheet now.
pub fn chart_data(doc: &Document, chart: &Chart) -> Result<ChartData> {
    let g = grid(doc, &chart.source)?;
    Ok(ChartData::from_grid(&g, chart.headers, chart.series_in_rows))
}

/// The canonical text of a link (`'My sheet'!A1:B3`), checked against the document.
pub fn normalize(doc: &Document, link: &str) -> Result<String> {
    let (i, r) = resolve(doc, link)?;
    Ok(SheetRange { sheet: Some(doc.pages[i].name.clone()), range: r }.to_string())
}

/// Every live link in the file: (page name, what holds it, the link).
pub fn all(doc: &Document) -> Vec<(String, String, String)> {
    let mut out = vec![];
    for p in &doc.pages {
        match &p.body {
            crate::PageBody::Doc(d) => {
                for (i, b) in d.blocks.iter().enumerate() {
                    match b {
                        crate::Block::Table(t) if t.link.is_some() => out.push((p.name.clone(), format!("table (block {})", i + 1), t.link.clone().unwrap())),
                        crate::Block::Chart(c) => out.push((p.name.clone(), format!("chart (block {})", i + 1), c.chart.source.clone())),
                        _ => {}
                    }
                }
            }
            crate::PageBody::Deck(d) => {
                for (si, s) in d.slides.iter().enumerate() {
                    for sh in &s.shapes {
                        match &sh.kind {
                            crate::ShapeKind::Chart { chart } => out.push((p.name.clone(), format!("chart on slide {}", si + 1), chart.source.clone())),
                            crate::ShapeKind::Table { table } if table.link.is_some() => out.push((p.name.clone(), format!("table on slide {}", si + 1), table.link.clone().unwrap())),
                            _ => {}
                        }
                    }
                }
            }
            crate::PageBody::Sheet(s) => {
                for c in &s.charts {
                    out.push((p.name.clone(), "chart".into(), c.chart.source.clone()));
                }
            }
        }
    }
    out
}
