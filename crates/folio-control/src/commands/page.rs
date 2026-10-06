//! `page.*` (the pages of the file), `link.list` and `media.*`.

use std::sync::Arc;

use folio_core::{PageBody, PageKind};
use serde_json::json;

use super::util::{self, page_of};
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let doc = s.doc()?;
    match cx.spec.name {
        "page.list" => Ok(json!(doc.pages.iter().enumerate().map(|(i, p)| json!({
            "number": i + 1,
            "id": p.id,
            "name": p.name,
            "kind": p.kind(),
            "summary": util::summary(p),
        })).collect::<Vec<_>>())),
        "page.get" => {
            let i = page_of(s, &doc, &a, None)?;
            Ok(json!(*doc.pages[i]))
        }
        "page.text" => {
            let i = page_of(s, &doc, &a, None)?;
            Ok(json!({ "name": doc.pages[i].name, "text": doc.pages[i].plain() }))
        }
        "page.add" => {
            let kind = PageKind::parse(a.str("kind")?).ok_or("kind is doc, sheet or deck.")?;
            let shown = s.ui_state().page.and_then(|id| doc.page_index(&id));
            let at = a.opt_i64("at").map(|v| v.max(0) as usize).or(shown.map(|i| i + 1));
            let name = a.opt_str("name").map(str::to_string);
            let paper = s.settings().editing.paper;
            let (i, page) = s.edit(cx.label(), cx.source, None, |d| {
                let i = d.add_page(kind, name.as_deref(), at)?;
                if kind == PageKind::Doc && paper == "letter"
                    && let Some(t) = d.page_mut(i).doc_mut()
                {
                    t.setup = folio_core::text::PageSetup::letter();
                }
                Ok((i, d.pages[i].clone()))
            })?;
            Ok(json!({ "index": i, "number": i + 1, "id": page.id, "name": page.name, "kind": page.kind() }))
        }
        "page.rename" => {
            let i = page_of(s, &doc, &a, None)?;
            let name = a.str("name")?.to_string();
            s.edit(cx.label(), cx.source, a.coalesce(), |d| d.rename_page(i, &name))?;
            Ok(json!({ "id": doc.pages[i].id, "name": name.trim() }))
        }
        "page.remove" => {
            let i = page_of(s, &doc, &a, None)?;
            let name = doc.pages[i].name.clone();
            s.edit(cx.label(), cx.source, None, |d| {
                d.pages.remove(i);
                d.prune_media();
                Ok(())
            })?;
            Ok(json!({ "removed": name }))
        }
        "page.move" => {
            let i = page_of(s, &doc, &a, None)?;
            let to = (a.opt_i64("to").unwrap_or(0).max(0) as usize).min(doc.pages.len() - 1);
            s.edit(cx.label(), cx.source, None, |d| {
                let p = d.pages.remove(i);
                d.pages.insert(to, p);
                Ok(())
            })?;
            Ok(json!({ "id": doc.pages[i].id, "index": to }))
        }
        "page.duplicate" => {
            let i = page_of(s, &doc, &a, None)?;
            let page = s.edit(cx.label(), cx.source, None, |d| {
                let mut p = (*d.pages[i]).clone();
                p.id = folio_core::Id::new();
                p.name = d.unique_name(&format!("{} copy", p.name));
                d.pages.insert(i + 1, Arc::new(p));
                Ok(d.pages[i + 1].clone())
            })?;
            Ok(json!({ "index": i + 1, "id": page.id, "name": page.name }))
        }
        _ => Err(super::unhandled(cx)),
    }
}

pub async fn run_misc(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "link.list" => {
            let doc = s.doc()?;
            Ok(json!(folio_core::links::all(&doc).into_iter().map(|(page, what, link)| {
                let ok = folio_core::links::resolve(&doc, &link);
                json!({ "page": page, "what": what, "link": link, "ok": ok.is_ok(), "error": ok.err().map(|e| e.0) })
            }).collect::<Vec<_>>()))
        }
        "media.list" => {
            let doc = s.doc()?;
            let mut uses: std::collections::HashMap<String, Vec<String>> = Default::default();
            for p in &doc.pages {
                match &p.body {
                    PageBody::Doc(d) => {
                        for b in d.blocks.iter() {
                            if let folio_core::Block::Image(im) = b {
                                uses.entry(im.media.to_string()).or_default().push(p.name.clone());
                            }
                        }
                    }
                    PageBody::Deck(d) => {
                        for (i, sl) in d.slides.iter().enumerate() {
                            for sh in &sl.shapes {
                                if let folio_core::ShapeKind::Image { media } = &sh.kind {
                                    uses.entry(media.to_string()).or_default().push(format!("{} slide {}", p.name, i + 1));
                                }
                            }
                        }
                    }
                    PageBody::Sheet(_) => {}
                }
            }
            Ok(json!(doc.media.values().map(|m| json!({
                "id": m.id, "name": m.name, "type": m.mime, "width": m.width, "height": m.height, "bytes": m.bytes.len(),
                "usedOn": uses.get(m.id.as_str()).cloned().unwrap_or_default(),
            })).collect::<Vec<_>>()))
        }
        "media.add" => {
            let path = util::absolute(a.str("path")?)?;
            let id = s.edit(cx.label(), cx.source, None, |d| util::add_image_file(d, &path.display().to_string()))?;
            Ok(json!({ "media": id }))
        }
        _ => Err(super::unhandled(cx)),
    }
}
