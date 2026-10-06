//! `deck.*`: slides, layouts, shapes, pictures, charts and tables on slides, themes, presenting.

use std::sync::Arc;

use folio_core::deck::{DeckTheme, Shape, ShapeKind, Slide, SlideLayout, VAlign};
use folio_core::text::{self, StylePatch, Table};
use folio_core::{Block, Chart, Id, PageKind, ParaStyle, Paragraph};
use serde_json::{Value, json};

use super::text::{shape_of, slide_of};
use super::util::{self, page_of};
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

fn lines_to_flow(text: &str, bullets: bool) -> text::Flow {
    text.split('\n')
        .map(|l| {
            let mut p = Paragraph::new(ParaStyle::Normal, l.trim_start_matches("- ").trim_start_matches("• "));
            if bullets && !l.trim().is_empty() {
                p.list = Some(folio_core::ListKind::Bullet);
            }
            Block::Paragraph(p)
        })
        .collect()
}

/// Replaces a placeholder's text, keeping its paragraph style (titles stay titles).
fn set_placeholder(slide: &mut Slide, which: &str, text: &str, layout_size: [f32; 2]) {
    let i = match slide.shapes.iter().position(|s| s.placeholder.as_deref() == Some(which) || which == "body" && s.placeholder.as_deref() == Some("subtitle")) {
        Some(i) => i,
        None => {
            // Add the placeholder the layout would have.
            let tmpl = Slide::with_layout(if which == "title" { SlideLayout::TitleOnly } else { SlideLayout::TitleContent }, layout_size, "", "");
            match tmpl.shapes.into_iter().find(|s| s.placeholder.as_deref() == Some(which)) {
                Some(s) => {
                    slide.shapes.push(s);
                    slide.shapes.len() - 1
                }
                None => return,
            }
        }
    };
    let sh = &mut slide.shapes[i];
    let style = sh.text.iter().find_map(|b| b.para().map(|p| p.style)).unwrap_or(ParaStyle::Normal);
    let bullets = sh.placeholder.as_deref() == Some("body");
    let mut flow = lines_to_flow(text, bullets);
    for b in flow.iter_mut() {
        if let Some(p) = b.para_mut() {
            p.style = style;
        }
    }
    sh.text = flow;
}

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    if cx.spec.name == "deck.themes" {
        return Ok(json!(DeckTheme::NAMES.iter().map(|n| DeckTheme::named(n).unwrap()).collect::<Vec<_>>()));
    }
    let doc = s.doc()?;
    let pi = page_of(s, &doc, &a, Some(PageKind::Deck))?;
    let deck = doc.pages[pi].deck().unwrap();
    let label = cx.label();
    let src = cx.source;
    let size = deck.size;
    match cx.spec.name {
        "deck.read" => Ok(json!({
            "name": doc.pages[pi].name,
            "size": deck.size,
            "theme": deck.theme,
            "slides": deck.slides.iter().enumerate().map(|(i, sl)| json!({
                "number": i + 1,
                "id": sl.id,
                "layout": sl.layout.id(),
                "title": sl.title(),
                "notes": sl.notes,
                "background": sl.background,
                "hidden": sl.hidden,
                "shapes": sl.shapes.iter().map(|sh| {
                    let mut v = json!({ "id": sh.id, "kind": sh.kind.id(), "x": sh.x, "y": sh.y, "w": sh.w, "h": sh.h });
                    if !sh.name.is_empty() { v["name"] = json!(sh.name); }
                    if let Some(p) = &sh.placeholder { v["placeholder"] = json!(p); }
                    if !sh.text.is_empty() { v["text"] = json!(sh.plain()); v["textSize"] = json!(sh.text_size); }
                    if let Some(f) = &sh.fill { v["fill"] = json!(f); }
                    if let Some(l) = &sh.line { v["line"] = json!(l); }
                    if sh.rotation != 0.0 { v["rotation"] = json!(sh.rotation); }
                    match &sh.kind {
                        ShapeKind::Chart { chart } => v["chart"] = json!(chart),
                        ShapeKind::Image { media } => v["media"] = json!(media),
                        ShapeKind::Table { table } => {
                            v["link"] = json!(table.link);
                            v["rows"] = json!(match &table.link { Some(l) => folio_core::links::table_text(&doc, l).unwrap_or_default(), None => table.rows.iter().map(|r| r.iter().map(|c| c.plain()).collect()).collect() });
                        }
                        _ => {}
                    }
                    v
                }).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })),
        "deck.addSlide" => {
            let layout = match a.opt_str("layout") {
                Some(l) => SlideLayout::parse(l).ok_or_else(|| format!("layout is title, titleContent, section, twoContent, titleOnly or blank, not \"{l}\"."))?,
                None => {
                    if deck.slides.is_empty() {
                        SlideLayout::Title
                    } else {
                        SlideLayout::TitleContent
                    }
                }
            };
            let mut sl = Slide::with_layout(layout, size, a.opt_str("title").unwrap_or(""), a.opt_str("body").unwrap_or(""));
            sl.notes = a.opt_str("notes").unwrap_or("").to_string();
            let ui = s.ui_state();
            let shown = ui.slide.filter(|_| ui.page.as_deref() == Some(doc.pages[pi].id.as_str()));
            let at = a.opt_i64("at").map(|v| v.max(0) as usize).or(shown.map(|i| i + 1)).unwrap_or(deck.slides.len()).min(deck.slides.len());
            let id = sl.id.clone();
            s.edit(label, src, None, |d| {
                d.page_mut(pi).deck_mut().unwrap().slides.insert(at, sl);
                Ok(())
            })?;
            Ok(json!({ "slide": at + 1, "id": id }))
        }
        "deck.removeSlide" => {
            let si = slide_of(s, &doc, pi, &a)?;
            s.edit(label, src, None, |d| {
                d.page_mut(pi).deck_mut().unwrap().slides.remove(si);
                d.prune_media();
                Ok(())
            })?;
            Ok(json!({ "removed": si + 1 }))
        }
        "deck.moveSlide" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let to = (a.opt_i64("to").unwrap_or(0).max(0) as usize).min(deck.slides.len() - 1);
            s.edit(label, src, None, |d| {
                let sl = d.page_mut(pi).deck_mut().unwrap();
                let x = sl.slides.remove(si);
                sl.slides.insert(to, x);
                Ok(())
            })?;
            Ok(json!({ "slide": to + 1 }))
        }
        "deck.duplicateSlide" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let id = s.edit(label, src, None, |d| {
                let dk = d.page_mut(pi).deck_mut().unwrap();
                let mut c = dk.slides[si].clone();
                c.id = Id::new();
                for sh in &mut c.shapes {
                    sh.id = Id::new();
                }
                let id = c.id.clone();
                dk.slides.insert(si + 1, c);
                Ok(id)
            })?;
            Ok(json!({ "slide": si + 2, "id": id }))
        }
        "deck.setSlide" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let layout = a.opt_str("layout").map(|l| SlideLayout::parse(l).ok_or_else(|| format!("Unknown layout \"{l}\"."))).transpose()?;
            let bg = util::color(&a, "background")?;
            s.edit(label, src, None, |d| {
                let dk = d.page_mut(pi).deck_mut().unwrap();
                let sl = &mut dk.slides[si];
                if let Some(l) = layout {
                    // Keep what is there; add the layout's placeholders that are missing.
                    let tmpl = Slide::with_layout(l, size, "", "");
                    for ph in tmpl.shapes {
                        let kind = ph.placeholder.clone();
                        if !sl.shapes.iter().any(|x| x.placeholder == kind) {
                            sl.shapes.push(ph);
                        } else if let Some(x) = sl.shapes.iter_mut().find(|x| x.placeholder == kind) {
                            (x.x, x.y, x.w, x.h) = (ph.x, ph.y, ph.w, ph.h);
                        }
                    }
                    sl.layout = l;
                }
                if let Some(t) = a.opt_str("title") {
                    set_placeholder(sl, "title", t, size);
                }
                if let Some(b) = a.opt_str("body") {
                    set_placeholder(sl, "body", b, size);
                }
                if let Some(b) = &bg {
                    sl.background = if b.is_empty() { None } else { Some(b.clone()) };
                }
                if let Some(n) = a.opt_str("notes") {
                    sl.notes = n.to_string();
                }
                if let Some(h) = a.opt_bool("hidden") {
                    sl.hidden = h;
                }
                Ok(())
            })?;
            Ok(json!({ "slide": si + 1 }))
        }
        "deck.addShape" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let k = a.str("kind")?;
            let kind = ShapeKind::parse_basic(k).ok_or_else(|| format!("kind is text, rect, ellipse, triangle, line or arrow, not \"{k}\" (deck.addImage, deck.addChart and deck.addTable add the others)."))?;
            let is_text = matches!(kind, ShapeKind::Text);
            let line_like = matches!(kind, ShapeKind::Line | ShapeKind::Arrow);
            let mut sh = Shape::new(kind, 0.0, 0.0, 0.0, 0.0);
            sh.x = a.opt_f64("x").map(|v| v as f32).unwrap_or(size[0] * 0.3);
            sh.y = a.opt_f64("y").map(|v| v as f32).unwrap_or(size[1] * 0.35);
            sh.w = a.opt_f64("w").map(|v| v as f32).unwrap_or(if is_text { size[0] * 0.4 } else { 200.0 });
            sh.h = a.opt_f64("h").map(|v| v as f32).unwrap_or(if is_text { 60.0 } else if line_like { 0.0 } else { 140.0 });
            sh.text_size = a.opt_f64("textSize").map(|v| v as f32).unwrap_or(20.0);
            if let Some(t) = a.opt_str("text") {
                sh.text = lines_to_flow(t, false);
            }
            sh.fill = util::color(&a, "fill")?.filter(|c| !c.is_empty());
            if sh.fill.is_none() && !is_text && !line_like {
                sh.fill = Some(deck.theme.accent.clone());
                if sh.color.is_none() {
                    sh.color = Some(deck.theme.background.clone());
                }
            }
            sh.line = util::color(&a, "line")?.filter(|c| !c.is_empty());
            if line_like && sh.line.is_none() {
                sh.line = Some(deck.theme.text.clone());
            }
            sh.line_width = a.opt_f64("lineWidth").map(|v| v as f32).unwrap_or(if line_like { 3.0 } else if sh.line.is_some() { 1.5 } else { 0.0 });
            if let Some(c) = util::color(&a, "color")?.filter(|c| !c.is_empty()) {
                sh.color = Some(c);
            }
            if !is_text && !line_like {
                sh.valign = VAlign::Middle;
            }
            if let Some(al) = a.opt_str("align") {
                let al = folio_core::Align::parse(al).ok_or("align is left, center, right or justify.")?;
                text::set_paragraphs(&mut sh.text, 0, usize::MAX, &text::ParaPatch { align: Some(al), ..Default::default() });
            } else if !is_text {
                text::set_paragraphs(&mut sh.text, 0, usize::MAX, &text::ParaPatch { align: Some(folio_core::Align::Center), ..Default::default() });
            }
            sh.name = a.opt_str("name").unwrap_or("").to_string();
            let id = sh.id.clone();
            s.edit(label, src, None, |d| {
                d.page_mut(pi).deck_mut().unwrap().slides[si].shapes.push(sh);
                Ok(())
            })?;
            Ok(json!({ "shape": id, "slide": si + 1 }))
        }
        "deck.updateShape" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let shi = shape_of(&doc, pi, si, a.str("shape")?)?;
            let fill = util::color(&a, "fill")?;
            let line = util::color(&a, "line")?;
            let color = util::color(&a, "color")?;
            let valign = match a.opt_str("valign") {
                Some("top") => Some(VAlign::Top),
                Some("middle") | Some("center") => Some(VAlign::Middle),
                Some("bottom") => Some(VAlign::Bottom),
                Some(v) => return Err(format!("valign is top, middle or bottom, not \"{v}\".")),
                None => None,
            };
            s.edit(label, src, a.coalesce(), |d| {
                let sh = &mut d.page_mut(pi).deck_mut().unwrap().slides[si].shapes[shi];
                for (k, slot) in [("x", &mut sh.x), ("y", &mut sh.y), ("w", &mut sh.w), ("h", &mut sh.h), ("rotation", &mut sh.rotation)] {
                    if let Some(v) = a.opt_f64(k) {
                        *slot = v as f32;
                    }
                }
                sh.w = sh.w.max(0.0);
                sh.h = sh.h.max(0.0);
                if let Some(t) = a.opt_str("text") {
                    let style = sh.text.iter().find_map(|b| b.para().map(|p| (p.style, p.align, p.list)));
                    let mut flow = lines_to_flow(t, false);
                    if let Some((st, al, li)) = style {
                        for b in flow.iter_mut() {
                            if let Some(p) = b.para_mut() {
                                (p.style, p.align, p.list) = (st, al, li);
                            }
                        }
                    }
                    sh.text = flow;
                }
                if let Some(f) = &fill {
                    sh.fill = if f.is_empty() { None } else { Some(f.clone()) };
                }
                if let Some(l) = &line {
                    sh.line = if l.is_empty() { None } else { Some(l.clone()) };
                }
                if let Some(c) = &color {
                    sh.color = if c.is_empty() { None } else { Some(c.clone()) };
                }
                if let Some(w) = a.opt_f64("lineWidth") {
                    sh.line_width = w.max(0.0) as f32;
                }
                if let Some(t) = a.opt_f64("textSize") {
                    sh.text_size = (t as f32).clamp(4.0, 400.0);
                }
                if let Some(v) = valign {
                    sh.valign = v;
                }
                if let Some(n) = a.opt_str("name") {
                    sh.name = n.to_string();
                }
                Ok(())
            })?;
            Ok(json!({ "shape": doc.pages[pi].deck().unwrap().slides[si].shapes[shi].id }))
        }
        "deck.removeShape" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let shi = shape_of(&doc, pi, si, a.str("shape")?)?;
            s.edit(label, src, None, |d| {
                d.page_mut(pi).deck_mut().unwrap().slides[si].shapes.remove(shi);
                d.prune_media();
                Ok(())
            })?;
            Ok(json!({ "removed": true }))
        }
        "deck.duplicateShape" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let shi = shape_of(&doc, pi, si, a.str("shape")?)?;
            let id = s.edit(label, src, None, |d| {
                let shapes = &mut d.page_mut(pi).deck_mut().unwrap().slides[si].shapes;
                let mut c = shapes[shi].clone();
                c.id = Id::new();
                c.x += 16.0;
                c.y += 16.0;
                c.placeholder = None;
                let id = c.id.clone();
                shapes.push(c);
                Ok(id)
            })?;
            Ok(json!({ "shape": id }))
        }
        "deck.arrange" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let shi = shape_of(&doc, pi, si, a.str("shape")?)?;
            let order = a.str("order")?.to_string();
            s.edit(label, src, None, |d| {
                let shapes = &mut d.page_mut(pi).deck_mut().unwrap().slides[si].shapes;
                let sh = shapes.remove(shi);
                let to = match order.as_str() {
                    "front" => shapes.len(),
                    "back" => 0,
                    "forward" => (shi + 1).min(shapes.len()),
                    "backward" => shi.saturating_sub(1),
                    o => return folio_core::bail(format!("order is front, back, forward or backward, not \"{o}\".")),
                };
                shapes.insert(to, sh);
                Ok(())
            })?;
            Ok(json!({ "order": order }))
        }
        "deck.formatText" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let shi = shape_of(&doc, pi, si, a.str("shape")?)?;
            let mut patch: StylePatch = util::style_patch(&a);
            if let Some(c) = util::color(&a, "color")? {
                patch.color = Some(c);
            }
            let para = util::para_patch(&a)?;
            s.edit(label, src, None, |d| {
                let sh = &mut d.page_mut(pi).deck_mut().unwrap().slides[si].shapes[shi];
                let (start, end) = (text::Pos::new(0, 0), text::end(&sh.text));
                if !patch.is_empty() {
                    text::format(&mut sh.text, start, end, &patch);
                }
                let n = sh.text.len().saturating_sub(1);
                text::set_paragraphs(&mut sh.text, 0, n, &para);
                Ok(())
            })?;
            Ok(json!({ "shape": doc.pages[pi].deck().unwrap().slides[si].shapes[shi].id }))
        }
        "deck.addImage" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let path = a.opt_str("path").map(|p| util::absolute(p).map(|p| p.display().to_string())).transpose()?;
            let media = a.opt_str("media").map(Id::from);
            if path.is_none() && media.is_none() {
                return Err("Give path (a picture file) or media (an id from media.list).".into());
            }
            let id = s.edit(label, src, None, |d| {
                let m = match (&media, &path) {
                    (Some(m), _) => {
                        if !d.media.contains_key(m) {
                            return folio_core::bail(format!("No media \"{m}\"."));
                        }
                        m.clone()
                    }
                    (None, Some(p)) => util::add_image_file(d, p)?,
                    _ => unreachable!(),
                };
                let (pw, ph) = d.media.get(&m).map(|m| (m.width as f32, m.height as f32)).filter(|(w, h)| *w > 0.0 && *h > 0.0).unwrap_or((4.0, 3.0));
                let w = a.opt_f64("w").map(|v| v as f32).unwrap_or((size[0] * 0.5).min(pw));
                let h = a.opt_f64("h").map(|v| v as f32).unwrap_or(w * ph / pw);
                let x = a.opt_f64("x").map(|v| v as f32).unwrap_or((size[0] - w) / 2.0);
                let y = a.opt_f64("y").map(|v| v as f32).unwrap_or((size[1] - h) / 2.0);
                let sh = Shape::new(ShapeKind::Image { media: m }, x, y, w, h);
                let id = sh.id.clone();
                d.page_mut(pi).deck_mut().unwrap().slides[si].shapes.push(sh);
                Ok(id)
            })?;
            Ok(json!({ "shape": id }))
        }
        "deck.addChart" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let source = folio_core::links::normalize(&doc, a.str("source")?).map_err(|e| e.0)?;
            let mut chart = Chart::new(super::doc::chart_kind(&a)?, source);
            chart.title = a.opt_str("title").unwrap_or("").to_string();
            let (w, h) = (a.opt_f64("w").map(|v| v as f32).unwrap_or(size[0] * 0.7), a.opt_f64("h").map(|v| v as f32).unwrap_or(size[1] * 0.6));
            let sh = Shape::new(ShapeKind::Chart { chart }, a.opt_f64("x").map(|v| v as f32).unwrap_or((size[0] - w) / 2.0), a.opt_f64("y").map(|v| v as f32).unwrap_or(size[1] * 0.28), w, h);
            let id = sh.id.clone();
            s.edit(label, src, None, |d| {
                d.page_mut(pi).deck_mut().unwrap().slides[si].shapes.push(sh);
                Ok(())
            })?;
            Ok(json!({ "shape": id }))
        }
        "deck.addTable" => {
            let si = slide_of(s, &doc, pi, &a)?;
            let mut table = match (a.array("data"), a.opt_str("link")) {
                (_, Some(link)) => {
                    let link = folio_core::links::normalize(&doc, link).map_err(|e| e.0)?;
                    let mut t = Table::from_text(folio_core::links::table_text(&doc, &link).map_err(|e| e.0)?, true);
                    t.link = Some(link);
                    t
                }
                (Some(rows), None) => Table::from_text(super::doc::rows_text(rows), true),
                (None, None) => Table::new(a.opt_i64("rows").unwrap_or(3).clamp(1, 60) as usize, a.opt_i64("cols").unwrap_or(3).clamp(1, 20) as usize),
            };
            table.header = true;
            let rows = table.rows.len().max(1) as f32;
            let (w, h) = (a.opt_f64("w").map(|v| v as f32).unwrap_or(size[0] * 0.8), a.opt_f64("h").map(|v| v as f32).unwrap_or((rows * 34.0).min(size[1] * 0.65)));
            let mut sh = Shape::new(ShapeKind::Table { table }, a.opt_f64("x").map(|v| v as f32).unwrap_or((size[0] - w) / 2.0), a.opt_f64("y").map(|v| v as f32).unwrap_or(size[1] * 0.28), w, h);
            sh.text_size = 16.0;
            let id = sh.id.clone();
            s.edit(label, src, None, |d| {
                d.page_mut(pi).deck_mut().unwrap().slides[si].shapes.push(sh);
                Ok(())
            })?;
            Ok(json!({ "shape": id }))
        }
        "deck.setTheme" => {
            let mut theme = match a.opt_str("theme") {
                Some(n) => DeckTheme::named(n).ok_or_else(|| format!("theme is {}, not \"{n}\".", DeckTheme::NAMES.join(", ")))?,
                None => deck.theme.clone(),
            };
            for (k, slot) in [("background", &mut theme.background), ("text", &mut theme.text), ("accent", &mut theme.accent)] {
                if let Some(c) = util::color(&a, k)?.filter(|c| !c.is_empty()) {
                    *slot = c;
                }
            }
            for (k, slot) in [("headingFont", &mut theme.heading_font), ("bodyFont", &mut theme.body_font)] {
                if let Some(f) = a.opt_str(k) {
                    if folio_core::text::Family::parse(f).is_none() {
                        return Err(format!("{k} is display, sans, serif or mono, not \"{f}\"."));
                    }
                    *slot = f.to_string();
                }
            }
            let out = theme.clone();
            s.edit(label, src, None, |d| {
                d.page_mut(pi).deck_mut().unwrap().theme = theme;
                Ok(())
            })?;
            Ok(json!(out))
        }
        "deck.present" => {
            let si = slide_of(s, &doc, pi, &a)?;
            s.ui_call("deck.present", json!({ "page": doc.pages[pi].id, "slide": si, "presenter": a.bool_or("presenter", false) })).await
        }
        _ => Err(super::unhandled(cx)),
    }
    .map(|v: Value| v)
}
