//! Plugins end to end: the example plugins are built with cargo, installed, used in formulas,
//! hot-reloaded, switched off and on, crash, and removed. Skipped (with a note) when cargo
//! isn't installed. One test: `LSUITE_HOME` is the process's.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use folio_control::{Session, SessionOptions, Source, call};
use serde_json::{Value, json};

async fn ok(s: &Arc<Session>, cmd: &str, params: Value) -> Value {
    match call(s, Source::Cli, cmd, params.clone()).await {
        Ok(v) => v,
        Err(e) => panic!("{cmd} {params}: {e}"),
    }
}

async fn eval(s: &Arc<Session>, formula: &str) -> Value {
    ok(s, "sheet.evaluate", json!({ "formula": formula })).await["value"].clone()
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Copies an example plugin crate into the sources folder, pointing it at this repository's SDK.
fn copy_example(name: &str, sources: &Path) {
    let from = repo().join("examples/plugins").join(name);
    let to = sources.join(name);
    std::fs::create_dir_all(to.join("src")).unwrap();
    for rel in ["plugin.toml", "src/lib.rs"] {
        std::fs::copy(from.join(rel), to.join(rel)).unwrap();
    }
    let sdk = repo().join("crates/folio-plugin");
    let cargo = std::fs::read_to_string(from.join("Cargo.toml")).unwrap().replace("\"../../../crates/folio-plugin\"", &format!("{:?}", sdk.display().to_string()));
    std::fs::write(to.join("Cargo.toml"), cargo).unwrap();
}

async fn disabled(s: &Arc<Session>) -> Vec<String> {
    s.settings().plugins.disabled
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugins_build_load_compute_reload_and_go() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("lsuite");
    // SAFETY: set before anything else in this test binary reads the environment.
    unsafe {
        std::env::set_var("LSUITE_HOME", &home);
        std::env::set_var("FOLIO_PLUGIN_SDK", repo().join("crates/folio-plugin"));
    }
    let s = Session::new(SessionOptions { data_dir: Some(tmp.path().join("data")), config_dir: Some(tmp.path().join("config")), secrets: None, headless: true }).unwrap();

    // Stock plugins: eight function categories and eight file filters.
    let list = ok(&s, "plugin.list", json!({})).await;
    let stock: Vec<&str> = list["stock"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert!(stock.len() >= 16, "{stock:?}");
    for id in ["folio.functions.math", "folio.functions.financial", "folio.functions.information", "folio.filters.docx", "folio.filters.odf", "folio.filters.pdf", "folio.filters.md"] {
        assert!(stock.contains(&id), "{id} missing from {stock:?}");
    }
    assert!(list["installed"].as_array().unwrap().is_empty());
    let info = ok(&s, "plugin.info", json!({ "id": "folio.functions.math" })).await;
    assert!(info["functions"].as_array().unwrap().iter().any(|f| f["name"] == "SUM"));
    assert!(call(&s, Source::Cli, "plugin.remove", json!({ "id": "folio.filters.docx" })).await.is_err(), "stock plugins can't be removed");
    let off = ok(&s, "plugin.disable", json!({ "id": "folio.filters.docx" })).await;
    assert_eq!(off["enabled"], false);
    assert!(off["note"].as_str().unwrap().contains("doesn't unhook"));
    ok(&s, "plugin.enable", json!({ "id": "folio.filters.docx" })).await;
    assert!(ok(&s, "plugin.guide", json!({})).await["guide"].as_str().unwrap().contains("export!"));
    // Agents need the plugins permission (off by default).
    let refused = call(&s, Source::Agent, "plugin.build", json!({ "name": "x" })).await.unwrap_err();
    assert!(refused.contains("plugins"), "{refused}");

    let tc = ok(&s, "plugin.toolchain", json!({})).await;
    if tc["ok"] != true {
        eprintln!("cargo isn't installed: skipping the build half of the plugin test ({tc})");
        return;
    }

    ok(&s, "file.new", json!({ "kind": "sheet" })).await;
    assert_eq!(eval(&s, "=GEOMEAN(2,8)").await, json!("#NAME?"));
    let sources = home.join("plugins-src/folio");
    copy_example("finance-extra", &sources);
    copy_example("text-tools", &sources);

    // Build, bundle, install: the functions work at once.
    let p = ok(&s, "plugin.publishLocal", json!({ "name": "finance-extra" })).await;
    assert_eq!(p["ok"], true, "{p}");
    assert_eq!(p["id"], "xyz.lsuite.folio.finance-extra");
    let installed = home.join("plugins/folio/xyz.lsuite.folio.finance-extra");
    assert!(installed.join("plugin.toml").is_file());
    assert_eq!(eval(&s, "=GEOMEAN(2,8)").await, json!(4.0));
    assert!((eval(&s, "=CAGR(100,121,2)").await.as_f64().unwrap() - 0.1).abs() < 1e-12);
    assert_eq!(eval(&s, "=GEOMEAN(-1)").await, json!("#NUM!"));
    assert_eq!(eval(&s, "=CAGR(1)").await, json!("#VALUE!"), "too few arguments");
    ok(&s, "sheet.setRange", json!({ "at": "A1", "values": [[2], [8], ["=GEOMEAN(A1:A2)"]] })).await;
    assert_eq!(ok(&s, "sheet.read", json!({})).await["rows"][2][0], json!(4.0));
    let fns = ok(&s, "sheet.functions", json!({ "category": "Custom" })).await;
    assert!(fns.as_array().unwrap().iter().any(|f| f["name"] == "XIRR"), "{fns}");
    let listed = ok(&s, "plugin.list", json!({})).await;
    assert_eq!(listed["installed"][0]["enabled"], true);
    assert_eq!(listed["installed"][0]["loaded"], true);

    // A file opened later gets the functions too.
    ok(&s, "file.new", json!({ "kind": "sheet" })).await;
    assert_eq!(eval(&s, "=GEOMEAN(1,4,16)").await, json!(4.0));
    ok(&s, "sheet.setRange", json!({ "at": "A1", "values": [[2], [8], ["=GEOMEAN(A1:A2)"]] })).await;

    // Hot reload: rebuild with one more function, put the library in place, rescan.
    let src = std::fs::read_to_string(sources.join("finance-extra/src/lib.rs")).unwrap();
    let src = src
        .replace("/// GEOMEAN(number1", "fn half(args: &[Arg]) -> Result<f64, ErrorKind> {\n    Ok(args[0].number()? / 2.0)\n}\n\n/// GEOMEAN(number1")
        .replace("    ],\n}", "        fn_def!(\"HALF\", \"HALF(number)\", \"Half.\", 1, 1, half),\n    ],\n}");
    ok(&s, "plugin.writeSource", json!({ "name": "finance-extra", "path": "src/lib.rs", "contents": src })).await;
    let b = ok(&s, "plugin.build", json!({ "name": "finance-extra" })).await;
    assert_eq!(b["ok"], true, "{b}");
    let lib = PathBuf::from(b["library"].as_str().unwrap());
    std::fs::copy(&lib, installed.join(lib.file_name().unwrap())).unwrap();
    let r = ok(&s, "plugin.rescan", json!({})).await;
    assert_eq!(r["reloaded"], json!(["xyz.lsuite.folio.finance-extra"]), "{r}");
    assert_eq!(eval(&s, "=HALF(8)").await, json!(4.0));
    assert_eq!(eval(&s, "=GEOMEAN(2,8)").await, json!(4.0));

    // Off: formulas show #NAME?; on again: they compute.
    ok(&s, "plugin.disable", json!({ "id": "xyz.lsuite.folio.finance-extra" })).await;
    assert_eq!(eval(&s, "=GEOMEAN(2,8)").await, json!("#NAME?"));
    assert_eq!(ok(&s, "sheet.read", json!({})).await["rows"][2][0], json!("#NAME?"));
    ok(&s, "plugin.enable", json!({ "id": "xyz.lsuite.folio.finance-extra" })).await;
    assert_eq!(ok(&s, "sheet.read", json!({})).await["rows"][2][0], json!(4.0));

    // The second example: text functions and an import filter.
    let p = ok(&s, "plugin.publishLocal", json!({ "name": "text-tools" })).await;
    assert_eq!(p["ok"], true, "{p}");
    assert_eq!(eval(&s, "=SLUG(\"Crème brûlée, 2 pots!\")").await, json!("creme-brulee-2-pots"));
    assert_eq!(eval(&s, "=REGEXMATCH(\"INV-2026-10\", \"^INV-\\d{4}-\\d\\d$\")").await, json!(true));
    assert_eq!(eval(&s, "=WORDCOUNT(\"one two three\")").await, json!(3.0));
    let txt = tmp.path().join("notes.txt");
    std::fs::write(&txt, "# Notes\n\nFirst paragraph.\n\nSecond one.").unwrap();
    let doc = s.plugins.import(&txt).expect("a plugin reads .txt").unwrap();
    assert_eq!(doc.title, "Notes");
    assert!(s.plugins.import(&tmp.path().join("x.unknown")).is_none());

    // A new crate from the template builds and works.
    let n = ok(&s, "plugin.new", json!({ "name": "my-stats" })).await;
    assert_eq!(n["id"], "local.my-stats");
    let p = ok(&s, "plugin.publishLocal", json!({ "name": "my-stats" })).await;
    assert_eq!(p["ok"], true, "{p}");
    assert_eq!(eval(&s, "=DOUBLE(21)").await, json!(42.0));

    // Sources stay inside their crate.
    for bad in ["../escape.rs", "/tmp/escape.rs", "src/../../escape.rs"] {
        assert!(call(&s, Source::Cli, "plugin.writeSource", json!({ "name": "my-stats", "path": bad, "contents": "x" })).await.is_err(), "{bad}");
    }
    #[cfg(unix)]
    {
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, sources.join("my-stats/out")).unwrap();
        assert!(call(&s, Source::Cli, "plugin.writeSource", json!({ "name": "my-stats", "path": "out/x.rs", "contents": "x" })).await.is_err());
        assert!(!outside.join("x.rs").exists());
    }

    // Compiler errors come back short and structured.
    ok(&s, "plugin.writeSource", json!({ "name": "my-stats", "path": "src/lib.rs", "contents": "use folio_plugin::prelude::*;\n\nfn f(_: &[Arg]) -> f64 {\n    undefined_thing\n}\n" })).await;
    let b = ok(&s, "plugin.build", json!({ "name": "my-stats" })).await;
    assert_eq!(b["ok"], false);
    assert_eq!(b["errors"][0]["file"], "src/lib.rs", "{b}");
    assert_eq!(b["errors"][0]["line"], 4, "{b}");
    assert!(b["errors"][0]["message"].as_str().unwrap().contains("undefined_thing"));

    // A panic shows #VALUE! and switches the plugin off; the app and other plugins go on.
    let crashy = "use folio_plugin::prelude::*;\n\nfn boom(_: &[Arg]) -> f64 {\n    panic!(\"boom\")\n}\n\nexport! {\n    id: \"local.my-stats\",\n    name: \"My stats\",\n    version: \"0.1.0\",\n    functions: [fn_def!(\"BOOM\", \"BOOM()\", \"Panics.\", 0, 0, boom)],\n}\n";
    ok(&s, "plugin.writeSource", json!({ "name": "my-stats", "path": "src/lib.rs", "contents": crashy })).await;
    let p = ok(&s, "plugin.publishLocal", json!({ "name": "my-stats" })).await;
    assert_eq!(p["ok"], true, "{p}");
    assert_eq!(eval(&s, "=BOOM()").await, json!("#VALUE!"));
    for _ in 0..100 {
        if disabled(&s).await.contains(&"local.my-stats".to_string()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(disabled(&s).await.contains(&"local.my-stats".to_string()), "a crash switches the plugin off");
    let info = ok(&s, "plugin.info", json!({ "id": "local.my-stats" })).await;
    assert_eq!(info["enabled"], false);
    assert_eq!(eval(&s, "=BOOM()").await, json!("#NAME?"));
    assert_eq!(eval(&s, "=GEOMEAN(2,8)").await, json!(4.0));

    // Remove: gone from disk and from formulas.
    ok(&s, "plugin.remove", json!({ "id": "xyz.lsuite.folio.finance-extra" })).await;
    assert!(!installed.exists());
    assert_eq!(eval(&s, "=GEOMEAN(2,8)").await, json!("#NAME?"));
    let left: Vec<String> = ok(&s, "plugin.list", json!({})).await["installed"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(left, ["local.my-stats", "xyz.lsuite.folio.text-tools"]);

    // A bundle for another app, or with a bad ABI, is refused.
    let bad = tmp.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("plugin.toml"), "id = \"x.y\"\nname = \"X\"\nversion = \"1\"\napp = \"nori\"\nkind = \"filter\"\nabi = 1\n").unwrap();
    let e = call(&s, Source::Cli, "plugin.install", json!({ "path": bad.display().to_string() })).await.unwrap_err();
    assert!(e.contains("nori"), "{e}");
}
