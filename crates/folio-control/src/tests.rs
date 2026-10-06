//! Command tests: every family through the registry, on a headless session.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{Session, SessionOptions, Source, call};

fn session() -> (Arc<Session>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new(SessionOptions { data_dir: Some(dir.path().join("data")), config_dir: Some(dir.path().join("config")), secrets: None, headless: true }).unwrap();
    (s, dir)
}

async fn run(s: &Arc<Session>, cmd: &str, params: Value) -> Value {
    match call(s, Source::Cli, cmd, params.clone()).await {
        Ok(v) => v,
        Err(e) => panic!("{cmd} {params}: {e}"),
    }
}

#[tokio::test]
async fn a_file_with_three_kinds_of_page() {
    let (s, _d) = session();
    run(&s, "file.new", json!({ "title": "Plan" })).await;
    run(&s, "page.add", json!({ "kind": "sheet", "name": "Data" })).await;
    run(&s, "sheet.setRange", json!({ "page": "Data", "at": "A1", "values": [["Item", "Cost"], ["Tea", 3], ["Cake", 4.5], ["Total", "=SUM(B2:B3)"]] })).await;
    let r = run(&s, "sheet.read", json!({ "page": "Data" })).await;
    assert_eq!(r["rows"][3][1], json!(7.5));
    run(&s, "doc.write", json!({ "page": "Document", "markdown": "# Plan\n\nSome **bold** text." })).await;
    run(&s, "doc.insertTable", json!({ "page": "Document", "link": "Data!A1:B4" })).await;
    let d = run(&s, "doc.read", json!({ "page": "Document" })).await;
    let table = d["blocks"].as_array().unwrap().iter().find(|b| b["type"] == "table").unwrap();
    assert_eq!(table["rows"][3][1], "7.5");
    // The linked table follows the sheet.
    run(&s, "sheet.set", json!({ "page": "Data", "cell": "B2", "value": "10" })).await;
    let d = run(&s, "doc.read", json!({ "page": "Document" })).await;
    let table = d["blocks"].as_array().unwrap().iter().find(|b| b["type"] == "table").unwrap();
    assert_eq!(table["rows"][3][1], "14.5");
    run(&s, "page.add", json!({ "kind": "deck" })).await;
    run(&s, "deck.addSlide", json!({ "title": "Costs", "body": "One\nTwo" })).await;
    run(&s, "deck.addChart", json!({ "source": "Data!A1:B3" })).await;
    let o = run(&s, "file.overview", json!({})).await;
    assert_eq!(o["pages"].as_array().unwrap().len(), 3);
    assert_eq!(o["links"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn undo_covers_every_client_and_batches() {
    let (s, _d) = session();
    run(&s, "file.new", json!({ "kind": "sheet" })).await;
    run(&s, "file.batch", json!({ "commands": [
        { "command": "sheet.set", "params": { "cell": "A1", "value": "1" } },
        { "command": "sheet.set", "params": { "cell": "A2", "value": "=A1*2" } },
    ] })).await;
    let r = run(&s, "sheet.read", json!({})).await;
    assert_eq!(r["rows"][1][0], json!(2.0));
    run(&s, "history.undo", json!({})).await;
    let r = run(&s, "sheet.read", json!({})).await;
    assert!(r["rows"].as_array().unwrap().is_empty());
    // A failing atomic batch changes nothing.
    let e = call(&s, Source::Cli, "file.batch", json!({ "commands": [
        { "command": "sheet.set", "params": { "cell": "A1", "value": "5" } },
        { "command": "sheet.set", "params": { "page": "Nope", "cell": "A1", "value": "5" } },
    ] })).await;
    assert!(e.is_err());
    let r = run(&s, "sheet.read", json!({})).await;
    assert!(r["rows"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn text_editing_round_trip() {
    let (s, _d) = session();
    run(&s, "file.new", json!({})).await;
    let p = run(&s, "text.insert", json!({ "text": "Hello world", "at": { "block": 0, "offset": 0 } })).await;
    assert_eq!(p["at"]["offset"], 11);
    run(&s, "text.format", json!({ "from": { "block": 0, "offset": 0 }, "to": { "block": 0, "offset": 5 }, "bold": true })).await;
    run(&s, "text.insert", json!({ "text": "\nSecond", "at": { "block": 0, "offset": 11 } })).await;
    run(&s, "text.paragraph", json!({ "from": 0, "style": "heading1" })).await;
    let r = run(&s, "doc.read", json!({})).await;
    assert_eq!(r["blocks"][0]["style"], "heading1");
    assert_eq!(r["blocks"][1]["text"], "Second");
    let md = run(&s, "doc.read", json!({ "markdown": true })).await;
    assert!(md["markdown"].as_str().unwrap().starts_with("# **Hello** world"), "{md}");
    run(&s, "doc.replace", json!({ "find": "world", "replace": "folio" })).await;
    let r = run(&s, "doc.read", json!({})).await;
    assert_eq!(r["blocks"][0]["text"], "Hello folio");
}

#[tokio::test]
async fn permissions_hold_for_agents() {
    let (s, _d) = session();
    run(&s, "file.new", json!({})).await;
    let e = call(&s, Source::Agent, "app.setAgentKey", json!({ "provider": "anthropic", "key": "x" })).await.unwrap_err();
    assert!(e.contains("stays with the person"), "{e}");
    let e = call(&s, Source::Mcp, "plugin.build", json!({ "name": "x" })).await.unwrap_err();
    assert!(e.contains("plugins"), "{e}");
}

#[test]
fn commands_doc_is_generated() {
    let generated = crate::markdown();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/COMMANDS.md");
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(on_disk == generated, "docs/COMMANDS.md is out of date: run `cargo run -p folio-cli -- docs`");
}
