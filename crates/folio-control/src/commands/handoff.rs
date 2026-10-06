//! `handoff.*`: working with the other lsuite apps through `~/.lsuite/apps/*.json` and their CLIs.
//!
//! Pictures come over as plain PNG files: nori exports its image, kimchi renders a frame of its
//! cut. folio runs the app's CLI (on the running app, or on a file with `--file`), reads the
//! `path` from its JSON answer, and places the picture like `doc.insertImage` / `deck.addImage`.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::discovery;
use crate::registry::{self, Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "handoff.apps" => Ok(json!(discovery::installed_apps().into_iter().filter(|e| e.app != "folio").map(|e| json!({
            "app": e.app,
            "kind": e.kind,
            "version": e.version,
            "running": e.running.is_some(),
            "cli": e.cli,
            "mcp": e.mcp,
            "folioTakes": match e.app.as_str() {
                "nori" => "its image, as a picture (handoff.image app=nori)",
                "kimchi" => "a frame of its cut, as a picture (handoff.image app=kimchi time=…)",
                "ryolune" => "nothing yet",
                _ => "nothing yet",
            },
        })).collect::<Vec<_>>())),
        "handoff.image" => {
            let app = a.str("app")?.to_string();
            let png = fetch_image(&app, a.opt_str("file"), a.opt_f64("time")).await?;
            // Place it where the person is: a slide when a deck is shown (or slide given), else a document.
            let doc = s.doc()?;
            let on_deck = a.has("slide") || {
                let shown = s.ui_state().page.and_then(|p| doc.page_index(&p));
                a.opt_str("page").and_then(|p| doc.page_index(p)).or(shown).is_some_and(|i| doc.pages[i].kind() == folio_core::PageKind::Deck)
            };
            let mut params = serde_json::Map::new();
            params.insert("path".into(), json!(png));
            for k in ["page", "slide", "after"] {
                if let Some(v) = a.get(k) {
                    if on_deck && k == "after" {
                        continue;
                    }
                    if !on_deck && k == "slide" {
                        continue;
                    }
                    params.insert(k.into(), v.clone());
                }
            }
            let command = if on_deck { "deck.addImage" } else { "doc.insertImage" };
            let spec = registry::spec(command).unwrap();
            let mut r = registry::call_boxed(s, cx.source, spec, Value::Object(params)).await?;
            r["from"] = json!(app);
            r["file"] = json!(png);
            Ok(r)
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// Runs the app's CLI and returns the path of the PNG it wrote.
async fn fetch_image(app: &str, file: Option<&str>, time: Option<f64>) -> CmdResult<String> {
    let entry = discovery::find(app).ok_or_else(|| format!("{app} isn't installed here (no ~/.lsuite/apps/{app}.json). Start {app} once so folio can find it."))?;
    let cli = entry.cli.clone().filter(|p| p.exists()).ok_or_else(|| format!("{app}'s command-line tool wasn't found ({app}-cli)."))?;
    if file.is_none() && entry.running.is_none() {
        return Err(format!("{app} isn't running. Open it with the picture you want, or give file."));
    }
    let out_dir = std::env::temp_dir().join("folio-handoff");
    let _ = std::fs::create_dir_all(&out_dir);
    let out = out_dir.join(format!("{app}-{}.png", chrono::Utc::now().timestamp_millis()));
    let mut args: Vec<String> = vec![];
    if let Some(f) = file {
        args.push("--file".into());
        args.push(f.to_string());
    }
    match app {
        "kimchi" => {
            args.push("project.renderFrame".into());
            if let Some(t) = time {
                args.push(format!("time={t}"));
            }
            args.push("width=1600".into());
        }
        "nori" => {
            // nori's export command: the first one it lists of these.
            let listed = run_cli(&cli, &["app.commands".to_string()]).await.unwrap_or(Value::Null);
            let names: Vec<String> = listed.as_array().map(|l| l.iter().filter_map(|c| c["name"].as_str().map(str::to_string)).collect()).unwrap_or_default();
            let cmd = ["file.export", "document.export", "export.image", "image.export", "export.png"].into_iter().find(|c| names.iter().any(|n| n == c)).unwrap_or("file.export");
            args.push(cmd.into());
            args.push(format!("path={}", out.display()));
            if cmd != "export.png" {
                args.push("format=png".into());
            }
        }
        other => return Err(format!("folio takes pictures from nori and kimchi, not {other}.")),
    }
    let v = run_cli(&cli, &args).await?;
    let path = v["path"].as_str().map(str::to_string).or_else(|| out.exists().then(|| out.display().to_string())).ok_or_else(|| format!("{app} didn't say where it wrote the picture: {v}"))?;
    if !std::path::Path::new(&path).exists() {
        return Err(format!("{app} answered {path}, but there is no file there."));
    }
    Ok(path)
}

async fn run_cli(cli: &std::path::Path, args: &[String]) -> CmdResult<Value> {
    let out = tokio::time::timeout(std::time::Duration::from_secs(120), tokio::process::Command::new(cli).args(args).arg("--compact").output())
        .await
        .map_err(|_| format!("{} took too long.", cli.display()))?
        .map_err(|e| format!("Couldn't run {}: {e}", cli.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{} failed: {}", cli.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), err.trim().lines().last().unwrap_or(text.trim())));
    }
    serde_json::from_str(text.trim()).map_err(|e| format!("Unexpected answer from {}: {e}", cli.display()))
}
