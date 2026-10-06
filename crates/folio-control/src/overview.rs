//! `file.overview`: the whole open file in one bounded answer, for agents to read first.

use folio_calc::Value as CellValue;
use folio_core::{Block, PageBody};
use serde_json::{Value, json};

use crate::session::{CmdResult, Session};

/// Paragraphs of text shown per document page.
const DOC_PARAS: usize = 12;
/// Cells of values shown per sheet.
const SHEET_CELLS: usize = 120;

pub fn overview(s: &Session) -> CmdResult {
    let doc = s.doc()?;
    let (path, untitled, origin) = s.location().ok_or(crate::session::NO_FILE)?;
    let pages: Vec<Value> = doc
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut v = json!({ "number": i + 1, "id": p.id, "name": p.name, "kind": p.kind() });
            match &p.body {
                PageBody::Doc(d) => {
                    v["words"] = json!(d.word_count());
                    v["blocks"] = json!(d.blocks.len());
                    v["outline"] = json!(d.outline().into_iter().map(|(b, l, t)| json!({"block": b, "level": l, "text": t})).collect::<Vec<_>>());
                    let paras: Vec<Value> = d
                        .blocks
                        .iter()
                        .enumerate()
                        .filter(|(_, b)| !matches!(b, Block::Paragraph(p) if p.is_empty()))
                        .take(DOC_PARAS)
                        .map(|(bi, b)| {
                            let text: String = b.plain().chars().take(200).collect();
                            json!({ "block": bi, "type": b.kind(), "text": text })
                        })
                        .collect();
                    v["start"] = json!(paras);
                    v["setup"] = json!({ "size": d.setup.size_name(), "landscape": d.setup.landscape(), "header": d.setup.header, "footer": d.setup.footer });
                    if !d.comments.is_empty() {
                        v["comments"] = json!(d.comments.iter().filter(|c| !c.resolved).count());
                    }
                    if d.track_changes {
                        v["trackChanges"] = json!(true);
                    }
                }
                PageBody::Sheet(sh) => {
                    let used = sh.used_range();
                    v["usedRange"] = json!(used.map(|r| r.a1()));
                    v["cells"] = json!(sh.cells.len());
                    let formulas: Vec<Value> = sh.cells.iter().filter(|(_, c)| c.is_formula()).take(40).map(|(a, c)| json!({ "cell": a.a1(), "formula": c.input, "shows": c.display() })).collect();
                    v["formulas"] = json!(formulas);
                    v["formulaCount"] = json!(sh.cells.values().filter(|c| c.is_formula()).count());
                    let errors: Vec<Value> = sh.cells.iter().filter(|(_, c)| matches!(c.value, CellValue::Error(_))).take(20).map(|(a, c)| json!({ "cell": a.a1(), "input": c.input, "error": c.display() })).collect();
                    if !errors.is_empty() {
                        v["errors"] = json!(errors);
                    }
                    if let Some(r) = used {
                        let mut rows = vec![];
                        let mut n = 0;
                        for row in r.start.row..=r.end.row {
                            if n >= SHEET_CELLS {
                                break;
                            }
                            let cells: Vec<String> = (r.start.col..=r.end.col.min(r.start.col + 11)).map(|c| sh.display(folio_calc::Addr::new(row, c))).collect();
                            n += cells.len();
                            rows.push(json!({ "row": row + 1, "cells": cells }));
                        }
                        v["sample"] = json!(rows);
                    }
                    if !sh.charts.is_empty() {
                        v["charts"] = json!(sh.charts.iter().map(|c| json!({ "id": c.id, "kind": c.chart.kind, "source": c.chart.source, "title": c.chart.title })).collect::<Vec<_>>());
                    }
                    if sh.filter.is_some() {
                        v["filter"] = json!(sh.filter);
                    }
                }
                PageBody::Deck(d) => {
                    v["theme"] = json!(d.theme.name);
                    v["slides"] = json!(d.slides.iter().enumerate().map(|(si, sl)| {
                        let text: Vec<String> = sl.shapes.iter().filter(|s| s.placeholder.as_deref() != Some("title")).map(|s| s.plain()).filter(|t| !t.is_empty()).map(|t| t.chars().take(160).collect()).collect();
                        json!({
                            "number": si + 1,
                            "id": sl.id,
                            "layout": sl.layout.id(),
                            "title": sl.title(),
                            "text": text,
                            "shapes": sl.shapes.len(),
                            "notes": !sl.notes.is_empty(),
                        })
                    }).collect::<Vec<_>>());
                }
            }
            v
        })
        .collect();
    let links: Vec<Value> = folio_core::links::all(&doc).into_iter().map(|(page, what, link)| json!({ "page": page, "what": what, "link": link, "ok": folio_core::links::resolve(&doc, &link).is_ok() })).collect();
    let history = s.read(|ed| json!({ "undo": ed.history().undo_list().into_iter().take(8).collect::<Vec<_>>(), "canRedo": ed.can_redo() }))?;
    Ok(json!({
        "title": doc.title,
        "path": path,
        "untitled": untitled,
        "importedFrom": origin.map(|(p, f)| json!({"path": p, "format": f})),
        "pages": pages,
        "links": links,
        "media": doc.media.len(),
        "history": history,
        "window": s.ui_state(),
        "hint": "Documents: doc.read / doc.write (Markdown). Sheets: sheet.read / sheet.setRange (formulas start with =). Decks: deck.read / deck.addSlide. Tables and charts can show a sheet range live (link / source like 'Sheet'!A1:C9).",
    }))
}
