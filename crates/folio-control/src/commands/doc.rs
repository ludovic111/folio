//! `doc.*`: document pages: writing Markdown, paragraphs, find and replace, tables (live-linked
//! too), pictures, charts, page setup, comments, footnotes and tracked changes.

use std::sync::Arc;

use folio_core::text::{self, ChartBlock, Comment, ImageBlock, PageSetup, Pos, Reply, StylePatch, Table, TableCell};
use folio_core::{Block, Chart, ChartKind, Document, Id, PageKind, ParaStyle, Paragraph};
use serde_json::{Value, json};

use super::util::{self, block_of, page_of, pos_json, pos_of};
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let doc = s.doc()?;
    let pi = page_of(s, &doc, &a, Some(PageKind::Doc))?;
    let page_id = doc.pages[pi].id.to_string();
    let td = doc.pages[pi].doc().unwrap();
    let flow = &td.blocks;
    let label = cx.label();
    match cx.spec.name {
        "doc.read" => {
            if a.bool_or("markdown", false) {
                return Ok(json!({ "name": doc.pages[pi].name, "markdown": folio_io::markdown::from_blocks(&doc, flow) }));
            }
            Ok(json!({ "name": doc.pages[pi].name, "id": page_id, "setup": td.setup, "trackChanges": td.track_changes, "comments": td.comments.len(), "blocks": util::blocks_json(&doc, flow) }))
        }
        "doc.write" => {
            let blocks = folio_io::markdown::to_blocks(a.str("markdown")?);
            if blocks.is_empty() {
                return Err("The Markdown has no content.".into());
            }
            let n = blocks.len();
            let replace = a.bool_or("replace", false);
            let at = if replace { 0 } else { util::insert_index(s, &page_id, flow, &a)? };
            // Writing after an empty first paragraph (a new page) replaces it.
            let fill_empty = !replace && flow.len() == 1 && flow[0].para().is_some_and(|p| p.is_empty());
            s.edit(label, cx.source, None, |d| {
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                if replace || fill_empty {
                    f.clear();
                }
                let at = if replace || fill_empty { 0 } else { at };
                for (k, b) in blocks.into_iter().enumerate() {
                    f.insert(at + k, b);
                }
                Ok(())
            })?;
            Ok(json!({ "blocks": n, "first": if replace || fill_empty { 0 } else { at } }))
        }
        "doc.addParagraph" => {
            let mut p = Paragraph::new(ParaStyle::Normal, a.str("text")?);
            util::para_patch(&a)?.apply(&mut p);
            let at = util::insert_index(s, &page_id, flow, &a)?;
            let id = p.id.clone();
            s.edit(label, cx.source, None, |d| {
                d.page_mut(pi).doc_mut().unwrap().blocks.insert(at, Block::Paragraph(p));
                Ok(())
            })?;
            Ok(json!({ "index": at, "id": id }))
        }
        "doc.setParagraph" => {
            let from = block_of(flow, a.get("block").unwrap())?;
            let to = match a.get("toBlock") {
                Some(v) => block_of(flow, v)?,
                None => from,
            };
            let patch = util::para_patch(&a)?;
            s.edit(label, cx.source, None, |d| {
                text::set_paragraphs(&mut d.page_mut(pi).doc_mut().unwrap().blocks, from, to, &patch);
                Ok(())
            })?;
            Ok(json!({ "from": from, "to": to }))
        }
        "doc.setText" => {
            let b = block_of(flow, a.get("block").unwrap())?;
            if flow[b].para().is_none() {
                return Err(format!("Block {b} is a {}, not a paragraph.", flow[b].kind()));
            }
            let t = a.str("text")?.to_string();
            s.edit(label, cx.source, None, |d| {
                if let Some(p) = d.page_mut(pi).doc_mut().unwrap().blocks[b].para_mut() {
                    let keep = p.style_at(0);
                    p.runs = vec![folio_core::Run { text: t.replace('\n', " "), style: keep }];
                    p.normalize();
                }
                Ok(())
            })?;
            Ok(json!({ "block": b }))
        }
        "doc.format" => {
            let mut patch = util::style_patch(&a);
            if let Some(c) = util::color(&a, "color")? {
                patch.color = Some(c);
            }
            if let Some(c) = util::color(&a, "highlight")? {
                patch.highlight = Some(c);
            }
            if patch.is_empty() {
                return Err("Say what to change: bold, italic, underline, strike, code, superscript, subscript, color, highlight, link, size or font.".into());
            }
            let ranges = ranges(flow, &a)?;
            let n = ranges.len();
            s.edit(label, cx.source, None, |d| {
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                for (x, y) in &ranges {
                    text::format(f, *x, *y, &patch);
                }
                Ok(())
            })?;
            Ok(json!({ "formatted": n }))
        }
        "doc.replace" => {
            let find = a.str("find")?.to_string();
            let with = a.str("replace")?.to_string();
            let case = a.bool_or("caseSensitive", false);
            let all = a.bool_or("all", true);
            let n = s.edit(label, cx.source, None, |d| {
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                if all {
                    return Ok(text::replace_all(f, &find, &with, case));
                }
                let Some((x, y)) = text::find_all(f, &find, case).into_iter().next() else { return Ok(0) };
                let style = text::style_at(f, Pos { offset: x.offset + 1, ..x });
                let p = text::delete(f, x, y);
                text::insert_text(f, p, &with, Some(style));
                Ok(1)
            })?;
            Ok(json!({ "replaced": n }))
        }
        "doc.find" => {
            let needle = a.str("text")?;
            let hits = text::find_all(flow, needle, a.bool_or("caseSensitive", false));
            Ok(json!({ "count": hits.len(), "matches": hits.iter().take(200).map(|(x, y)| {
                let context = text::plain(flow, Pos { offset: x.offset.saturating_sub(30), ..*x }, Pos { offset: y.offset + 30, ..*y });
                json!({ "from": pos_json(*x), "to": pos_json(*y), "context": context })
            }).collect::<Vec<_>>() }))
        }
        "doc.deleteBlocks" => {
            let from = block_of(flow, a.get("from").unwrap())?;
            let to = match a.get("to") {
                Some(v) => block_of(flow, v)?,
                None => from,
            };
            let (from, to) = (from.min(to), from.max(to));
            s.edit(label, cx.source, None, |d| {
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                for i in (from..=to).rev() {
                    f.remove(i);
                }
                text::ensure_nonempty(f);
                Ok(())
            })?;
            Ok(json!({ "deleted": to - from + 1 }))
        }
        "doc.moveBlock" => {
            let b = block_of(flow, a.get("block").unwrap())?;
            let to = (a.opt_i64("to").unwrap_or(0).max(0) as usize).min(flow.len() - 1);
            s.edit(label, cx.source, None, |d| {
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                let blk = f.remove(b);
                f.insert(to, blk);
                Ok(())
            })?;
            Ok(json!({ "index": to }))
        }
        "doc.insertTable" => {
            let mut t = match (a.array("data"), a.opt_str("link")) {
                (_, Some(link)) => {
                    let link = folio_core::links::normalize(&doc, link).map_err(|e| e.0)?;
                    let mut t = Table::from_text(folio_core::links::table_text(&doc, &link).map_err(|e| e.0)?, true);
                    t.link = Some(link);
                    t
                }
                (Some(rows), None) => Table::from_text(rows_text(rows), true),
                (None, None) => Table::new(a.opt_i64("rows").unwrap_or(3).clamp(1, 500) as usize, a.opt_i64("cols").unwrap_or(3).clamp(1, 50) as usize),
            };
            if let Some(h) = a.opt_bool("header") {
                t.header = h;
            }
            let at = util::insert_index(s, &page_id, flow, &a)?;
            let id = t.id.clone();
            s.edit(label, cx.source, None, |d| {
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                f.insert(at, Block::Table(t));
                // Something to type after the table.
                if at + 1 >= f.len() {
                    f.push_back(Block::Paragraph(Paragraph::default()));
                }
                Ok(())
            })?;
            Ok(json!({ "index": at, "id": id }))
        }
        "doc.editTable" => {
            let b = block_of(flow, a.get("block").unwrap())?;
            let Block::Table(_) = &flow[b] else { return Err(format!("Block {b} is a {}, not a table.", flow[b].kind())) };
            let link = match a.opt_str("link") {
                Some("") => Some(None),
                Some(l) => Some(Some(folio_core::links::normalize(&doc, l).map_err(|e| e.0)?)),
                None => None,
            };
            let data = a.array("data").map(|r| rows_text(r));
            let cell = a.array("cell").map(|v| (v.first().and_then(Value::as_u64).unwrap_or(0) as usize, v.get(1).and_then(Value::as_u64).unwrap_or(0) as usize));
            let text_in = a.opt_str("text").map(str::to_string);
            let snapshot = doc.clone();
            s.edit(label, cx.source, None, |d| {
                let Block::Table(t) = &mut d.page_mut(pi).doc_mut().unwrap().blocks[b] else { unreachable!() };
                if let Some(l) = link {
                    if l.is_none()
                        && let Some(old) = &t.link
                    {
                        // Unlinking keeps the values as text.
                        let rows = folio_core::links::table_text(&snapshot, old).unwrap_or_default();
                        let keep = Table::from_text(rows, t.header);
                        t.rows = keep.rows;
                    }
                    t.link = l;
                }
                if let Some(rows) = &data {
                    let keep = Table::from_text(rows.clone(), t.header);
                    t.rows = keep.rows;
                    t.link = None;
                }
                if let (Some((r, c)), Some(tx)) = (cell, &text_in) {
                    if t.link.is_some() {
                        return folio_core::bail("This table shows a sheet range: change the sheet, or unlink it first (link \"\").");
                    }
                    let cell = t.cell_mut(r, c).ok_or_else(|| folio_core::Error(format!("No cell [{r}, {c}] in this table.")))?;
                    *cell = TableCell { runs: if tx.is_empty() { vec![] } else { vec![folio_core::Run::plain(tx.clone())] }, ..cell.clone() };
                }
                if let Some(h) = a.opt_bool("header") {
                    t.header = h;
                }
                if let Some(v) = a.opt_bool("banded") {
                    t.banded = v;
                }
                let cols = t.cols();
                if let Some(r) = a.opt_i64("insertRow") {
                    let r = (r.max(0) as usize).min(t.rows.len());
                    t.rows.insert(r, vec![TableCell::default(); cols]);
                }
                if let Some(r) = a.opt_i64("removeRow") {
                    let r = r.max(0) as usize;
                    if r < t.rows.len() && t.rows.len() > 1 {
                        t.rows.remove(r);
                    }
                }
                if let Some(c) = a.opt_i64("insertColumn") {
                    let c = (c.max(0) as usize).min(cols);
                    for row in &mut t.rows {
                        row.insert(c.min(row.len()), TableCell::default());
                    }
                    t.widths.clear();
                }
                if let Some(c) = a.opt_i64("removeColumn") {
                    let c = c.max(0) as usize;
                    if cols > 1 {
                        for row in &mut t.rows {
                            if c < row.len() {
                                row.remove(c);
                            }
                        }
                        t.widths.clear();
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "block": b }))
        }
        "doc.insertImage" => {
            let at = util::insert_index(s, &page_id, flow, &a)?;
            let path = a.opt_str("path").map(|p| util::absolute(p).map(|p| p.display().to_string())).transpose()?;
            let media = a.opt_str("media").map(Id::from);
            if path.is_none() && media.is_none() {
                return Err("Give path (a picture file) or media (an id from media.list).".into());
            }
            let width = a.opt_f64("width").map(|w| w as f32);
            let caption = a.opt_str("caption").unwrap_or("").to_string();
            let alt = a.opt_str("alt").unwrap_or("").to_string();
            let tw = td.setup.text_width();
            let id = s.edit(label, cx.source, None, |d| {
                let m = match (&media, &path) {
                    (Some(m), _) => {
                        if !d.media.contains_key(m) {
                            return folio_core::bail(format!("No media \"{m}\" (media.list)."));
                        }
                        m.clone()
                    }
                    (None, Some(p)) => util::add_image_file(d, p)?,
                    _ => unreachable!(),
                };
                let natural = d.media.get(&m).map(|m| m.width as f32 * 0.75).filter(|w| *w > 0.0).unwrap_or(tw);
                let blk = ImageBlock { id: Id::new(), media: m, width: width.unwrap_or(natural.min(tw)).min(tw), caption: caption.clone(), alt: alt.clone(), align: folio_core::Align::Center };
                let id = blk.id.clone();
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                f.insert(at, Block::Image(blk));
                if at + 1 >= f.len() {
                    f.push_back(Block::Paragraph(Paragraph::default()));
                }
                Ok(id)
            })?;
            Ok(json!({ "index": at, "id": id }))
        }
        "doc.insertChart" => {
            let source = folio_core::links::normalize(&doc, a.str("source")?).map_err(|e| e.0)?;
            let kind = chart_kind(&a)?;
            let mut chart = Chart::new(kind, source);
            chart.title = a.opt_str("title").unwrap_or("").to_string();
            let at = util::insert_index(s, &page_id, flow, &a)?;
            let blk = ChartBlock { id: Id::new(), chart, height: a.opt_f64("height").map(|h| h as f32).unwrap_or(240.0).clamp(60.0, 700.0) };
            let id = blk.id.clone();
            s.edit(label, cx.source, None, |d| {
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                f.insert(at, Block::Chart(blk));
                if at + 1 >= f.len() {
                    f.push_back(Block::Paragraph(Paragraph::default()));
                }
                Ok(())
            })?;
            Ok(json!({ "index": at, "id": id }))
        }
        "doc.insertPageBreak" => {
            let at = util::insert_index(s, &page_id, flow, &a)?;
            s.edit(label, cx.source, None, |d| {
                let f = &mut d.page_mut(pi).doc_mut().unwrap().blocks;
                f.insert(at, Block::PageBreak { id: Id::new() });
                if at + 1 >= f.len() {
                    f.push_back(Block::Paragraph(Paragraph::default()));
                }
                Ok(())
            })?;
            Ok(json!({ "index": at }))
        }
        "doc.setup" => {
            let mut setup = td.setup.clone();
            if let Some(name) = a.opt_str("size") {
                let (w, h) = PageSetup::size_named(name).ok_or_else(|| format!("size is a4, letter, legal, a5 or a3, not \"{name}\"."))?;
                let land = setup.landscape();
                (setup.width, setup.height) = if land { (h, w) } else { (w, h) };
            }
            if let Some(o) = a.opt_str("orientation") {
                let land = match o {
                    "landscape" => true,
                    "portrait" => false,
                    _ => return Err("orientation is portrait or landscape.".into()),
                };
                let (a1, b1) = (setup.width.min(setup.height), setup.width.max(setup.height));
                (setup.width, setup.height) = if land { (b1, a1) } else { (a1, b1) };
            }
            if let Some(m) = a.opt_f64("margins") {
                let m = m as f32;
                (setup.margin_top, setup.margin_bottom, setup.margin_left, setup.margin_right) = (m, m, m, m);
            }
            for (k, slot) in [("marginTop", &mut setup.margin_top), ("marginBottom", &mut setup.margin_bottom), ("marginLeft", &mut setup.margin_left), ("marginRight", &mut setup.margin_right)] {
                if let Some(v) = a.opt_f64(k) {
                    *slot = v as f32;
                }
            }
            for m in [setup.margin_top, setup.margin_bottom, setup.margin_left, setup.margin_right] {
                if !(0.0..=setup.width.min(setup.height) / 3.0).contains(&m) {
                    return Err(format!("A margin of {m} points doesn't fit the page."));
                }
            }
            if let Some(h) = a.opt_str("header") {
                setup.header = h.to_string();
            }
            if let Some(f) = a.opt_str("footer") {
                setup.footer = f.to_string();
            }
            if let Some(v) = a.opt_bool("differentFirst") {
                setup.different_first = v;
            }
            let out = setup.clone();
            s.edit(label, cx.source, None, |d| {
                d.page_mut(pi).doc_mut().unwrap().setup = setup;
                Ok(())
            })?;
            Ok(json!(out))
        }
        "doc.outline" => Ok(json!(td.outline().into_iter().map(|(i, l, t)| json!({ "block": i, "level": l, "text": t })).collect::<Vec<_>>())),
        "doc.stats" => {
            let plain = td.plain();
            let pages = folio_layout::page_count(&doc, pi);
            Ok(json!({
                "words": td.word_count(),
                "characters": plain.chars().filter(|c| *c != '\n').count(),
                "charactersNoSpaces": plain.chars().filter(|c| !c.is_whitespace()).count(),
                "paragraphs": flow.iter().filter(|b| b.para().is_some_and(|p| !p.is_empty())).count(),
                "tables": flow.iter().filter(|b| matches!(b, Block::Table(_))).count(),
                "images": flow.iter().filter(|b| matches!(b, Block::Image(_))).count(),
                "charts": flow.iter().filter(|b| matches!(b, Block::Chart(_))).count(),
                "pages": pages,
                "comments": td.comments.iter().filter(|c| !c.resolved).count(),
            }))
        }
        "doc.comment" => {
            let ranges = ranges(flow, &a)?;
            let (x, y) = *ranges.first().ok_or("Say what to comment on: find (text) or from/to.")?;
            if x == y {
                return Err("A comment needs some text to be on (from and to are the same).".into());
            }
            let c = Comment { id: Id::new(), author: s.settings().author(), text: a.str("text")?.to_string(), at: chrono::Utc::now(), resolved: false, replies: vec![] };
            let id = c.id.clone();
            s.edit(label, cx.source, None, |d| {
                let td = d.page_mut(pi).doc_mut().unwrap();
                text::format(&mut td.blocks, x, y, &StylePatch { comment: Some(c.id.to_string()), ..Default::default() });
                td.comments.push(c);
                Ok(())
            })?;
            Ok(json!({ "comment": id }))
        }
        "doc.comments" => {
            let anchors = comment_anchors(&doc, pi);
            Ok(json!(td.comments.iter().map(|c| json!({
                "id": c.id, "author": c.author, "text": c.text, "at": c.at, "resolved": c.resolved, "replies": c.replies,
                "on": anchors.get(c.id.as_str()).cloned().unwrap_or_default(),
            })).collect::<Vec<_>>()))
        }
        "doc.replyComment" => {
            let id = a.str("comment")?.to_string();
            let author = s.settings().author();
            let t = a.str("text")?.to_string();
            s.edit(label, cx.source, None, |d| {
                let td = d.page_mut(pi).doc_mut().unwrap();
                let c = td.comments.iter_mut().find(|c| c.id == id.as_str()).ok_or_else(|| folio_core::Error(format!("No comment \"{id}\" (doc.comments).")))?;
                c.replies.push(Reply { author, text: t, at: chrono::Utc::now() });
                Ok(())
            })?;
            Ok(json!({ "comment": id }))
        }
        "doc.resolveComment" => {
            let id = a.str("comment")?.to_string();
            let delete = a.bool_or("delete", false);
            let resolved = a.bool_or("resolved", true);
            s.edit(label, cx.source, None, |d| {
                let td = d.page_mut(pi).doc_mut().unwrap();
                let i = td.comments.iter().position(|c| c.id == id.as_str()).ok_or_else(|| folio_core::Error(format!("No comment \"{id}\" (doc.comments).")))?;
                if delete {
                    td.comments.remove(i);
                    let end = text::end(&td.blocks);
                    // Remove the mark wherever it is.
                    for bi in 0..td.blocks.len() {
                        if let Some(p) = td.blocks[bi].para_mut() {
                            for r in &mut p.runs {
                                if r.style.comment.as_ref().is_some_and(|c| c == id.as_str()) {
                                    r.style.comment = None;
                                }
                            }
                            p.normalize();
                        }
                    }
                    let _ = end;
                } else {
                    td.comments[i].resolved = resolved;
                }
                Ok(())
            })?;
            Ok(json!({ "comment": id, "deleted": delete }))
        }
        "doc.footnote" => {
            let note = a.str("text")?.to_string();
            let (x, y) = if a.has("find") {
                *ranges(flow, &a)?.first().ok_or("Nothing found.")?
            } else {
                let at = match a.get("at") {
                    Some(v) => pos_of(flow, v)?,
                    None => super::text::caret_or_end(s, &doc, super::text::Target::Doc { page: pi }),
                };
                // The note hangs on the word before the position.
                let p = flow[at.block].para().ok_or("Footnotes go in paragraphs.")?;
                let (w0, w1) = text::word_at(p, at.offset.saturating_sub(1));
                (Pos { offset: w0, ..at }, Pos { offset: w1.max(at.offset), ..at })
            };
            s.edit(label, cx.source, None, |d| {
                text::format(&mut d.page_mut(pi).doc_mut().unwrap().blocks, x, y, &StylePatch { note: Some(note.clone()), ..Default::default() });
                Ok(())
            })?;
            Ok(json!({ "from": pos_json(x), "to": pos_json(y) }))
        }
        "doc.trackChanges" => {
            let on = a.opt_bool("on").unwrap_or(true);
            s.edit(label, cx.source, None, |d| {
                d.page_mut(pi).doc_mut().unwrap().track_changes = on;
                Ok(())
            })?;
            Ok(json!({ "trackChanges": on }))
        }
        "doc.resolveChanges" => {
            let accept = a.opt_bool("accept").unwrap_or(true);
            let n = s.edit(label, cx.source, None, |d| Ok(text::resolve_changes(&mut d.page_mut(pi).doc_mut().unwrap().blocks, accept)))?;
            Ok(json!({ "changes": n, "accepted": accept }))
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// Rows of cell text from JSON rows (numbers and booleans as written).
pub fn rows_text(rows: &[Value]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| match r {
            Value::Array(cells) => cells.iter().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| if c.is_null() { String::new() } else { c.to_string() })).collect(),
            other => vec![other.as_str().map(str::to_string).unwrap_or_else(|| other.to_string())],
        })
        .collect()
}

pub fn chart_kind(a: &Args) -> CmdResult<ChartKind> {
    match a.opt_str("kind") {
        Some(k) => ChartKind::parse(k).ok_or_else(|| format!("kind is column, bar, line, area, pie or scatter, not \"{k}\".")),
        None => Ok(ChartKind::Column),
    }
}

/// The ranges a command means: `find` (+ `all`) or `from`/`to`.
fn ranges(flow: &text::Flow, a: &Args) -> CmdResult<Vec<(Pos, Pos)>> {
    if let Some(f) = a.opt_str("find") {
        let hits = text::find_all(flow, f, false);
        if hits.is_empty() {
            return Err(format!("\"{f}\" isn't on the page."));
        }
        return Ok(if a.bool_or("all", false) { hits } else { hits.into_iter().take(1).collect() });
    }
    let from = a.get("from").ok_or("Give find (text) or from and to (positions).")?;
    let x = pos_of(flow, from)?;
    let y = match a.get("to") {
        Some(v) => pos_of(flow, v)?,
        None if from.get("offset").is_none() => text::last_in(flow, x.block),
        None => x,
    };
    Ok(vec![(x.min(y), x.max(y))])
}

/// For each comment id, the text it is on.
fn comment_anchors(doc: &Document, pi: usize) -> std::collections::HashMap<String, String> {
    let mut out: std::collections::HashMap<String, String> = Default::default();
    if let Some(td) = doc.pages[pi].doc() {
        for b in td.blocks.iter() {
            if let Some(p) = b.para() {
                for r in &p.runs {
                    if let Some(c) = &r.style.comment {
                        out.entry(c.to_string()).or_default().push_str(&r.text);
                    }
                }
            }
        }
    }
    out
}
