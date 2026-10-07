//! `harness.*`: the agent harness (lsuite's HARNESS.md), the same for every agent: the brief, the
//! skills, the live context, a look at the work and the objective checks.

use std::sync::Arc;

use folio_core::PageBody;
use serde_json::{Value, json};

use super::util::page_of;
use crate::harness::{self, look};
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "harness.brief" => Ok(Value::String(harness::brief().to_string())),
        "harness.skills" => Ok(json!(harness::skills().iter().map(|k| json!({ "name": k.name, "title": k.title, "when": k.when })).collect::<Vec<_>>())),
        "harness.skill" => Ok(Value::String(harness::skill(a.str("name")?)?.markdown.to_string())),
        "harness.context" => {
            let since = a.opt_i64("since").map(|v| v.max(0) as u64);
            let c = harness::context::context(s, since);
            Ok(json!({ "text": c.text(), "short": c.short, "seq": c.seq }))
        }
        "harness.check" => {
            let doc = s.doc()?;
            let page = if a.has("page") { Some(page_of(s, &doc, &a, None)?) } else { None };
            let report = tokio::task::spawn_blocking(move || harness::check::check(&doc, page)).await.map_err(|e| format!("The check failed: {e}"))?;
            let mut v = json!(report);
            v["summary"] = json!(report.summary());
            Ok(v)
        }
        "harness.look" => {
            let doc = s.doc()?;
            let page = page_of(s, &doc, &a, None)?;
            let mut req = look::Request { page, width: a.opt_u32("width"), ..Default::default() };
            match &doc.pages[page].body {
                PageBody::Doc(_) => {
                    if let Some(n) = a.opt_i64("pageNumber") {
                        if n < 1 {
                            return Err("pageNumber counts printed pages from 1.".into());
                        }
                        req.page_number = Some(n as usize - 1);
                    }
                }
                PageBody::Deck(_) => req.slide = Some(super::text::slide_of(s, &doc, page, &a)?),
                PageBody::Sheet(_) => {
                    if let Some(r) = a.opt_str("range") {
                        let r = r.rsplit_once('!').map(|(_, r)| r).unwrap_or(r);
                        req.range = Some(folio_calc::Range::parse(r).ok_or_else(|| format!("`{r}` isn't a range like A1:D20."))?);
                    }
                }
            }
            let data_dir = s.data_dir.clone();
            tokio::task::spawn_blocking(move || {
                let l = look::render(&doc, &req)?;
                let path = look::save(&data_dir, &l.png)?;
                let pic = tiny_skia::Pixmap::decode_png(&l.png).map(|p| (p.width(), p.height())).unwrap_or((0, 0));
                let mut info = l.info;
                info["path"] = json!(path);
                info["width"] = json!(pic.0);
                info["height"] = json!(pic.1);
                Ok(info)
            })
            .await
            .map_err(|e| format!("Drawing the picture failed: {e}"))?
        }
        _ => Err(super::unhandled(cx)),
    }
}
