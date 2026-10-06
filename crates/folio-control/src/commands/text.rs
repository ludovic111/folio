//! `text.*`: editing rich text, on a document page or in a text box on a slide. The window's
//! typing, deleting, formatting and pasting all come through here.

use std::sync::Arc;

use folio_core::text::{self, Flow, Pos, RunStyle};
use folio_core::{Block, Document, PageKind};
use serde_json::{Value, json};

use super::util::{self, page_of, pos_json, pos_of};
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

/// Where text lives: a document page, or a shape on a slide.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Doc { page: usize },
    Shape { page: usize, slide: usize, shape: usize },
}

impl Target {
    pub fn flow<'a>(&self, d: &'a Document) -> &'a Flow {
        match *self {
            Target::Doc { page } => &d.pages[page].doc().unwrap().blocks,
            Target::Shape { page, slide, shape } => &d.pages[page].deck().unwrap().slides[slide].shapes[shape].text,
        }
    }

    pub fn flow_mut<'a>(&self, d: &'a mut Document) -> &'a mut Flow {
        match *self {
            Target::Doc { page } => &mut d.page_mut(page).doc_mut().unwrap().blocks,
            Target::Shape { page, slide, shape } => &mut d.page_mut(page).deck_mut().unwrap().slides[slide].shapes[shape].text,
        }
    }

    pub fn page(&self) -> usize {
        match *self {
            Target::Doc { page } | Target::Shape { page, .. } => page,
        }
    }

    /// Whether this is a document with tracked changes on.
    pub fn tracking(&self, d: &Document) -> bool {
        match *self {
            Target::Doc { page } => d.pages[page].doc().is_some_and(|t| t.track_changes),
            Target::Shape { .. } => false,
        }
    }
}

/// The slide a command means: `slide` (id or 1-based number), else the window's, else the first.
pub fn slide_of(s: &Session, d: &Document, page: usize, a: &Args) -> CmdResult<usize> {
    let deck = d.pages[page].deck().ok_or_else(|| format!("\"{}\" isn't a deck.", d.pages[page].name))?;
    if deck.slides.is_empty() {
        return Err(format!("\"{}\" has no slides. Add one with deck.addSlide.", d.pages[page].name));
    }
    match a.get("slide") {
        Some(v) => {
            let key = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            deck.slide(&key).ok_or_else(|| format!("No slide \"{key}\": the deck has {} slides (1 to {}), or use a slide id from deck.read.", deck.slides.len(), deck.slides.len()))
        }
        None => {
            let ui = s.ui_state();
            Ok(ui.slide.filter(|i| ui.page.as_deref() == Some(d.pages[page].id.as_str()) && *i < deck.slides.len()).unwrap_or(0))
        }
    }
}

pub fn shape_of(d: &Document, page: usize, slide: usize, key: &str) -> CmdResult<usize> {
    let sl = &d.pages[page].deck().unwrap().slides[slide];
    sl.shape(key).ok_or_else(|| {
        let names: Vec<String> = sl.shapes.iter().map(|s| if s.name.is_empty() { format!("{} ({})", s.id, s.kind.id()) } else { format!("\"{}\" ({})", s.name, s.id) }).collect();
        format!("No shape \"{key}\" on slide {}. Shapes: {}.", slide + 1, if names.is_empty() { "none".into() } else { names.join(", ") })
    })
}

/// Resolves the target from `page`, `slide`, `shape`.
pub fn target_of(s: &Session, d: &Document, a: &Args) -> CmdResult<Target> {
    if let Some(sh) = a.opt_str("shape") {
        let page = page_of(s, d, a, Some(PageKind::Deck))?;
        let slide = slide_of(s, d, page, a)?;
        let shape = shape_of(d, page, slide, sh)?;
        return Ok(Target::Shape { page, slide, shape });
    }
    let page = page_of(s, d, a, Some(PageKind::Doc))?;
    Ok(Target::Doc { page })
}

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let doc = s.doc()?;
    let t = target_of(s, &doc, &a)?;
    let flow = t.flow(&doc);
    match cx.spec.name {
        "text.read" => {
            let from = a.opt_i64("from").unwrap_or(0).max(0) as usize;
            let to = a.opt_i64("to").map(|v| v.max(0) as usize).unwrap_or(usize::MAX);
            let blocks: Vec<Value> = util::blocks_json(&doc, flow).into_iter().enumerate().filter(|(i, _)| *i >= from && *i <= to).map(|(_, b)| b).collect();
            Ok(json!({ "blocks": blocks, "count": flow.len() }))
        }
        "text.insert" => {
            let text_in = a.str("text")?.to_string();
            let at = match a.get("at") {
                Some(v) => pos_of(flow, v)?,
                None => caret_or_end(s, &doc, t),
            };
            let mut style: Option<RunStyle> = match a.get("style") {
                Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("`style` isn't formatting: {e}"))?),
                None => None,
            };
            let author = s.settings().author();
            if t.tracking(&doc) {
                let mut st = style.take().unwrap_or_else(|| text::style_at(flow, at));
                st.inserted = Some(author);
                style = Some(st);
            }
            let p = s.edit(cx.label(), cx.source, a.coalesce(), |d| Ok(text::insert_text(t.flow_mut(d), at, &text_in, style.clone())))?;
            Ok(json!({ "at": pos_json(p) }))
        }
        "text.delete" => {
            let from = pos_of(flow, a.get("from").unwrap())?;
            let to = pos_of(flow, a.get("to").unwrap())?;
            if t.tracking(&doc) {
                // Tracked: mark as deleted instead (text the same author inserted just goes).
                let author = s.settings().author();
                let p = s.edit(cx.label(), cx.source, a.coalesce(), |d| {
                    let f = t.flow_mut(d);
                    mark_deleted(f, from.min(to), from.max(to), &author);
                    Ok(from.min(to))
                })?;
                return Ok(json!({ "at": pos_json(p), "tracked": true }));
            }
            let p = s.edit(cx.label(), cx.source, a.coalesce(), |d| Ok(text::delete(t.flow_mut(d), from, to)))?;
            Ok(json!({ "at": pos_json(p) }))
        }
        "text.format" => {
            let from = pos_of(flow, a.get("from").unwrap())?;
            let to = pos_of(flow, a.get("to").unwrap())?;
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
            s.edit(cx.label(), cx.source, a.coalesce(), |d| {
                text::format(t.flow_mut(d), from, to, &patch);
                Ok(())
            })?;
            Ok(json!({ "from": pos_json(from), "to": pos_json(to) }))
        }
        "text.paragraph" => {
            let from = util::block_of(flow, a.get("from").unwrap())?;
            let to = match a.get("to") {
                Some(v) => util::block_of(flow, v)?,
                None => from,
            };
            let patch = util::para_patch(&a)?;
            s.edit(cx.label(), cx.source, a.coalesce(), |d| {
                text::set_paragraphs(t.flow_mut(d), from, to, &patch);
                Ok(())
            })?;
            Ok(json!({ "from": from, "to": to }))
        }
        "text.paste" => {
            let at = pos_of(flow, a.get("at").unwrap())?;
            let blocks: Vec<Block> = if let Some(list) = a.array("blocks") {
                list.iter().map(|b| serde_json::from_value(b.clone()).map_err(|e| format!("A block isn't valid: {e}"))).collect::<Result<_, _>>()?
            } else if let Some(md) = a.opt_str("markdown") {
                folio_io::markdown::to_blocks(md)
            } else if let Some(text_in) = a.opt_str("text") {
                let text_in = text_in.to_string();
                let p = s.edit(cx.label(), cx.source, None, |d| Ok(text::insert_text(t.flow_mut(d), at, &text_in, None)))?;
                return Ok(json!({ "at": pos_json(p) }));
            } else {
                return Err("Give blocks, markdown or text to paste.".into());
            };
            // Slides take paragraphs only.
            let blocks: Vec<Block> = if matches!(t, Target::Shape { .. }) { blocks.into_iter().filter(|b| matches!(b, Block::Paragraph(_))).collect() } else { blocks };
            let p = s.edit(cx.label(), cx.source, None, |d| Ok(text::insert_blocks(t.flow_mut(d), at, blocks.clone())))?;
            Ok(json!({ "at": pos_json(p) }))
        }
        "text.copy" => {
            let from = pos_of(flow, a.get("from").unwrap())?;
            let to = pos_of(flow, a.get("to").unwrap())?;
            let blocks = text::slice(flow, from, to);
            let flow2: Flow = blocks.iter().cloned().collect();
            Ok(json!({ "blocks": blocks, "markdown": folio_io::markdown::from_blocks(&doc, &flow2), "text": text::plain(flow, from, to) }))
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// The window's caret on this target, else the end.
pub fn caret_or_end(s: &Session, d: &Document, t: Target) -> Pos {
    let ui = s.ui_state();
    if let (Target::Doc { page }, Some(sel)) = (t, ui.text)
        && ui.page.as_deref() == Some(d.pages[page].id.as_str())
    {
        return text::clamp(t.flow(d), sel.focus);
    }
    text::end(t.flow(d))
}

/// Tracked deletion: marks the text `deleted` by `author` (text that was a tracked insertion goes at once).
fn mark_deleted(flow: &mut Flow, a: Pos, b: Pos, author: &str) {
    for i in a.block..=b.block.min(flow.len().saturating_sub(1)) {
        let Block::Paragraph(p) = &mut flow[i] else { continue };
        let from = if i == a.block { a.offset } else { 0 };
        let to = if i == b.block { b.offset } else { p.len() };
        let mut pos = 0;
        let mut out = vec![];
        for r in p.runs.drain(..) {
            let n = text::clen(&r.text);
            let (s0, s1) = (pos, pos + n);
            pos = s1;
            if s1 <= from || s0 >= to {
                out.push(r);
                continue;
            }
            let ba = text::byte_at(&r.text, from.saturating_sub(s0).min(n));
            let bb = text::byte_at(&r.text, (to - s0).min(n));
            let (head, mid, tail) = (&r.text[..ba], &r.text[ba..bb], &r.text[bb..]);
            if !head.is_empty() {
                out.push(folio_core::Run { text: head.into(), style: r.style.clone() });
            }
            if r.style.inserted.is_none() {
                let mut st = r.style.clone();
                st.deleted = Some(author.to_string());
                out.push(folio_core::Run { text: mid.into(), style: st });
            }
            if !tail.is_empty() {
                out.push(folio_core::Run { text: tail.into(), style: r.style.clone() });
            }
        }
        p.runs = out;
        p.normalize();
    }
}
