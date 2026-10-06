//! `file.*`: the open file as a whole: new, open, import, save, export, overview, batches.

use std::sync::Arc;

use folio_core::{Document, PageKind};
use serde_json::{Value, json};

use super::util::{self, absolute};
use crate::registry::{self, Args, Ctx};
use crate::session::{CmdResult, Event, Session, err};

/// How long `file.batch` waits for another batch to end.
const BATCH_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "file.overview" => crate::overview::overview(s),
        "file.info" => info(s),
        "file.get" => Ok(json!(s.doc()?)),
        "file.recent" => {
            let open = s.location().map(|l| l.0);
            Ok(json!(crate::recent::list(&s.config_dir).into_iter().map(|p| json!({
                "path": p,
                "name": p.file_stem().map(|n| n.to_string_lossy().into_owned()),
                "exists": p.exists(),
                "open": open.as_ref() == Some(&p),
                "modified": std::fs::metadata(&p).and_then(|m| m.modified()).ok().map(chrono::DateTime::<chrono::Utc>::from),
            })).collect::<Vec<_>>()))
        }
        "file.formats" => Ok(json!(folio_io::formats())),
        "file.templates" => Ok(json!(crate::templates::list())),
        "file.new" => {
            let title = a.opt_str("title").map(str::trim).filter(|t| !t.is_empty()).unwrap_or("Untitled").to_string();
            let paper = s.settings().editing.paper;
            let doc = match a.opt_str("template") {
                Some(t) => crate::templates::build(t, &title, &paper)?,
                None => {
                    let mut d = Document::empty(&title);
                    match a.opt_str("kind").unwrap_or("doc") {
                        "blank" | "none" => {}
                        k => {
                            let kind = PageKind::parse(k).ok_or_else(|| format!("kind is doc, sheet, deck or blank, not \"{k}\"."))?;
                            let i = d.add_page(kind, None, None).map_err(|e| e.0)?;
                            if kind == PageKind::Doc && paper == "letter"
                                && let Some(t) = d.page_mut(i).doc_mut()
                            {
                                t.setup = folio_core::text::PageSetup::letter();
                            }
                        }
                    }
                    d
                }
            };
            let path = s.untitled_path(&doc.id);
            let v = json!({ "title": doc.title, "pages": doc.pages.iter().map(|p| json!({"id": p.id, "name": p.name, "kind": p.kind()})).collect::<Vec<_>>(), "path": path, "untitled": true });
            s.open_doc(doc, path, true, None);
            Ok(v)
        }
        "file.open" => {
            let path = absolute(a.str("path")?)?;
            open_path(s, &path)
        }
        "file.import" => {
            let path = absolute(a.str("path")?)?;
            let imported = import_any(&path)?;
            let at = a.opt_i64("at");
            let names: Vec<String> = s.edit(cx.label(), cx.source, None, |d| {
                let mut at = at.map(|v| v.max(0) as usize).unwrap_or(d.pages.len()).min(d.pages.len());
                let mut names = vec![];
                // Media keeps its ids (they are random); pages get unique names.
                for (id, m) in imported.doc.media.iter() {
                    d.media.insert(id.clone(), m.clone());
                }
                for p in imported.doc.pages.iter() {
                    let mut p = (**p).clone();
                    p.id = folio_core::Id::new();
                    p.name = d.unique_name(&p.name);
                    names.push(p.name.clone());
                    d.pages.insert(at, Arc::new(p));
                    at += 1;
                }
                Ok(names)
            })?;
            Ok(json!({ "added": names, "warnings": imported.warnings }))
        }
        "file.save" => {
            let (path, untitled, _) = s.location().ok_or(crate::session::NO_FILE)?;
            if untitled {
                s.flush();
                return Err(format!("This file isn't saved in a folder yet (it is kept in {}). Use file.saveAs with a path.", path.display()));
            }
            s.flush();
            Ok(json!({ "path": path, "saved": !s.unsaved() }))
        }
        "file.saveAs" => {
            let mut path = absolute(a.str("path")?)?;
            if path.extension().is_none_or(|e| e != "folio") {
                if path.extension().is_some() && folio_io::format_for_path(&path).is_some() {
                    return Err(format!("{} is a {} file: use file.export to write other formats. file.saveAs writes .folio files.", path.display(), path.extension().unwrap().to_string_lossy()));
                }
                path.set_extension("folio");
            }
            s.move_to(&path)?;
            Ok(json!({ "path": path }))
        }
        "file.export" => export(s, &a),
        "file.rename" => {
            let title = a.str("title")?.trim().to_string();
            if title.is_empty() {
                return Err("A title can't be empty.".into());
            }
            s.edit(cx.label(), cx.source, a.coalesce(), |d| {
                d.title = title.clone();
                Ok(())
            })?;
            Ok(json!({ "title": title }))
        }
        "file.close" => {
            s.close_doc();
            Ok(json!({ "closed": true }))
        }
        "file.batch" => batch(s, cx, a).await,
        _ => Err(super::unhandled(cx)),
    }
}

fn info(s: &Session) -> CmdResult {
    let (path, untitled, origin) = s.location().ok_or(crate::session::NO_FILE)?;
    let doc = s.doc()?;
    Ok(json!({
        "title": doc.title,
        "path": path,
        "untitled": untitled,
        "importedFrom": origin.as_ref().map(|(p, f)| json!({"path": p, "format": f})),
        "pages": doc.pages.iter().map(|p| json!({"id": p.id, "name": p.name, "kind": p.kind()})).collect::<Vec<_>>(),
        "media": doc.media.len(),
        "saved": !s.unsaved(),
    }))
}

/// What a file holds once read: a document and what didn't come through.
pub struct Imported {
    pub doc: Document,
    pub warnings: Vec<String>,
    pub format: String,
}

/// Reads a .folio file or imports any other format folio knows.
pub fn import_any(path: &std::path::Path) -> CmdResult<Imported> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("folio")) {
        let doc = folio_core::file::load(path).map_err(|e| e.0)?;
        return Ok(Imported { doc, warnings: vec![], format: "folio".into() });
    }
    if !path.exists() {
        return Err(format!("{} doesn't exist.", path.display()));
    }
    let r = folio_io::import(path).map_err(err)?;
    Ok(Imported { doc: r.doc, warnings: r.warnings, format: r.format.to_string() })
}

/// Opens a path: .folio files in place, anything else imported as a new untitled file.
pub fn open_path(s: &Arc<Session>, path: &std::path::Path) -> CmdResult {
    let imported = import_any(path)?;
    let doc = imported.doc;
    let summary = json!({
        "title": doc.title,
        "pages": doc.pages.iter().map(|p| json!({"id": p.id, "name": p.name, "kind": p.kind(), "summary": util::summary(p)})).collect::<Vec<_>>(),
    });
    if imported.format == "folio" {
        s.open_doc(doc, path.to_path_buf(), false, None);
        let mut v = summary;
        v["path"] = json!(path);
        return Ok(v);
    }
    let target = s.untitled_path(&doc.id);
    s.open_doc(doc, target.clone(), true, Some((path.to_path_buf(), imported.format.clone())));
    crate::recent::add(&s.config_dir, path);
    let mut v = summary;
    v["importedFrom"] = json!({ "path": path, "format": imported.format });
    v["warnings"] = json!(imported.warnings);
    v["path"] = json!(target);
    v["untitled"] = json!(true);
    Ok(v)
}

fn export(s: &Arc<Session>, a: &Args) -> CmdResult {
    let path = absolute(a.str("path")?)?;
    let doc = s.doc()?;
    let pages: Option<Vec<usize>> = match a.array("pages") {
        Some(list) => Some(
            list.iter()
                .map(|v| {
                    let key = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
                    doc.page_index(&key).ok_or_else(|| doc.page(&key).err().map(|e| e.0).unwrap_or_default())
                })
                .collect::<Result<_, _>>()?,
        ),
        None => None,
    };
    let report = folio_io::export(&doc, &path, a.opt_str("format"), pages.as_deref()).map_err(err)?;
    s.emit(Event::Saved { path: path.display().to_string() });
    Ok(json!({ "path": path, "format": report.format, "warnings": report.warnings, "bytes": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) }))
}

async fn batch(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let commands = a.array("commands").cloned().unwrap_or_default();
    let atomic = a.bool_or("atomic", true);
    let label = a.opt_str("label").unwrap_or("batch").to_string();
    let mut calls = Vec::with_capacity(commands.len());
    for (i, c) in commands.iter().enumerate() {
        let name = c.get("command").and_then(Value::as_str).ok_or_else(|| format!("commands[{i}] needs \"command\""))?;
        let spec = registry::spec(name).ok_or_else(|| format!("commands[{i}]: unknown command `{name}`"))?;
        if spec.name == "file.batch" {
            return Err("file.batch can't contain another batch".into());
        }
        if matches!(spec.name, "file.new" | "file.open" | "file.close") {
            return Err(format!("commands[{i}]: `{name}` switches files and can't be part of a batch"));
        }
        let params = c.get("params").cloned().unwrap_or(json!({}));
        registry::allowed(s, cx.source, spec)?;
        registry::validate(spec, &params).map_err(|e| format!("commands[{i}]: {e}"))?;
        calls.push((spec, params));
    }
    let lock = s.batch_lock.clone();
    let _guard = tokio::time::timeout(BATCH_WAIT, lock.lock()).await.map_err(|_| "Another batch is still running.".to_string())?;
    s.with_editor(|ed| ed.begin_batch(&label, cx.source.as_str()))?;
    let mut results = vec![];
    let mut failed = None;
    for (i, (spec, params)) in calls.into_iter().enumerate() {
        let r = crate::session::batch_scope(registry::call_boxed(s, cx.source, spec, params)).await;
        match r {
            Ok(v) => results.push(json!({ "command": spec.name, "ok": true, "result": v })),
            Err(e) => {
                results.push(json!({ "command": spec.name, "ok": false, "error": e }));
                if atomic {
                    failed = Some((i, e));
                    break;
                }
            }
        }
    }
    if let Some((i, e)) = failed {
        s.with_editor(|ed| ed.rollback_batch())?;
        return Err(format!("commands[{i}] ({}) failed, so nothing was changed: {e}", results[i]["command"].as_str().unwrap_or("")));
    }
    s.with_editor(|ed| ed.end_batch())?;
    s.emit(Event::DocChanged { version: s.read(|ed| ed.version()).unwrap_or(0) });
    Ok(json!({ "results": results }))
}
