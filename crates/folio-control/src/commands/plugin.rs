//! `plugin.*`: the Plugins area's commands (lsuite's PLUGINS.md), for the window, agents, the
//! CLI and MCP alike. The host itself is [`crate::plugins`].
//!
//! An agent builds a plugin with `plugin.new` → `plugin.writeSource` → `plugin.build` →
//! `plugin.publishLocal`. Crates live in `~/.lsuite/plugins-src/folio/<name>/` and build against
//! the copy of the SDK folio carries (written to `plugins-src/folio/.sdk/folio-plugin-<version>/`,
//! so it works offline and always matches this folio); `FOLIO_PLUGIN_SDK` points at another
//! copy (the repository's, in development).

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::plugins::{self, Manifest, plugins_dir, sources_dir};
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Event, Session};

/// The longest a plugin build may take (a first build compiles the SDK too).
const BUILD_TIMEOUT: Duration = Duration::from_secs(600);
/// The most compiler errors `plugin.build` returns.
const MAX_ERRORS: usize = 20;

const INSTALL_HINT: &str = "Rust isn't installed. Plugins are written in Rust: ask the person whether to install it with rustup (https://rustup.rs). On macOS and Linux: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y. On Windows: download and run rustup-init.exe from https://rustup.rs. Never install it without their yes.";

/// Where the SDK's sources come from.
const GIT_URL: &str = "https://github.com/ludovic111/folio";
const GIT_TAG: &str = "v0.1.0";

const SDK_FILES: &[(&str, &str)] = &[
    ("Cargo.toml", concat!("[package]\nname = \"folio-plugin\"\nversion = \"", env!("CARGO_PKG_VERSION"), "\"\nedition = \"2024\"\nlicense = \"MIT\"\ndescription = \"SDK for folio plugins (a copy carried by folio)\"\n\n[dependencies]\n")),
    ("src/lib.rs", include_str!("../../../folio-plugin/src/lib.rs")),
    ("src/ffi.rs", include_str!("../../../folio-plugin/src/ffi.rs")),
    ("GUIDE.md", include_str!("../../../folio-plugin/GUIDE.md")),
];

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "plugin.list" => Ok(list(s)),
        "plugin.info" => info(s, a.str("id")?),
        "plugin.enable" | "plugin.disable" => switch(s, a.str("id")?, cx.spec.name == "plugin.enable"),
        "plugin.rescan" => Ok(s.plugins.rescan(s)),
        "plugin.install" => {
            let path = PathBuf::from(a.str("path")?);
            let id = s.plugins.install(s, &path)?;
            Ok(json!({ "id": id, "plugin": s.plugins.info(&id, &s.settings().plugins.disabled) }))
        }
        "plugin.remove" => {
            let id = a.str("id")?;
            if plugins::stock().iter().any(|p| p.id == id) {
                return Err(format!("`{id}` ships with folio: it can be switched off (plugin.disable), not removed."));
            }
            let dir = s.plugins.remove(s, id)?;
            Ok(json!({ "id": id, "removed": dir.display().to_string() }))
        }
        "plugin.guide" => Ok(json!({
            "guide": folio_plugin::GUIDE,
            "sdk": sdk_dir().ok().map(|p| p.display().to_string()),
            "sources": sources_dir().display().to_string(),
            "installed": plugins_dir().display().to_string(),
        })),
        "plugin.toolchain" => Ok(toolchain()),
        "plugin.new" => new_crate(s, a.str("name")?, a.opt_str("kind").unwrap_or("functions")),
        "plugin.writeSource" => write_source(a.str("name")?, a.str("path")?, a.str("contents")?),
        "plugin.build" => build(a.str("name")?).await,
        "plugin.publishLocal" => publish(s, a.str("name")?).await,
        _ => Err(super::unhandled(cx)),
    }
}

fn list(s: &Session) -> Value {
    let disabled = s.settings().plugins.disabled;
    let stock: Vec<Value> = plugins::stock().iter().map(|p| p.summary(!disabled.contains(&p.id))).collect();
    json!({
        "stock": stock,
        "installed": s.plugins.list(&disabled),
        "formats": [{
            "id": "lsuite",
            "name": "lsuite plugins (Rust)",
            "kinds": ["functions", "filter"],
            "abi": folio_plugin::ABI_VERSION,
            "where": plugins_dir().display().to_string(),
            "description": "Spreadsheet functions and import filters written in Rust on the folio-plugin SDK (plugin.guide).",
        }],
        "folder": plugins_dir().display().to_string(),
        "sources": sources_dir().display().to_string(),
    })
}

fn info(s: &Session, id: &str) -> CmdResult {
    let disabled = s.settings().plugins.disabled;
    if let Some(p) = plugins::stock().into_iter().find(|p| p.id == id) {
        return Ok(p.details(!disabled.iter().any(|d| d == id)));
    }
    s.plugins.info(id, &disabled).ok_or_else(|| not_found(s, id))
}

fn not_found(s: &Session, id: &str) -> String {
    let mut ids: Vec<String> = plugins::stock().into_iter().map(|p| p.id).collect();
    ids.extend(s.plugins.list(&[]).iter().filter_map(|p| p["id"].as_str().map(str::to_string)));
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    match crate::registry::closest(id, &refs) {
        Some(c) => format!("No plugin `{id}`. Did you mean `{c}`? (plugin.list shows them all.)"),
        None => format!("No plugin `{id}` (plugin.list shows them all)."),
    }
}

fn switch(s: &Session, id: &str, on: bool) -> CmdResult {
    let stock = plugins::stock().into_iter().any(|p| p.id == id);
    if !stock && !s.plugins.is_installed(id) {
        return Err(not_found(s, id));
    }
    s.update_settings(|st| {
        st.plugins.disabled.retain(|d| d != id);
        if !on {
            st.plugins.disabled.push(id.to_string());
        }
    })?;
    if stock {
        s.emit(Event::PluginsChanged);
        return Ok(json!({ "id": id, "enabled": on, "note": plugins::STOCK_NOTE }));
    }
    s.plugins.refresh(s);
    let disabled = s.settings().plugins.disabled;
    Ok(json!({ "id": id, "enabled": on, "plugin": s.plugins.info(id, &disabled) }))
}

// ---- the toolchain ---------------------------------------------------------------------------

/// Where to look for cargo: PATH, then the usual places (an app started from the Dock or the
/// Start menu has a bare PATH).
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = std::env::var_os("CARGO_HOME").filter(|h| !h.is_empty()) {
        dirs.push(PathBuf::from(h).join("bin"));
    }
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join(".cargo").join("bin"));
    }
    if !cfg!(windows) {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    }
    dirs
}

fn find(tool: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{tool}.exe") } else { tool.to_string() };
    search_dirs().into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

/// PATH with cargo's folder first, so cargo finds rustc next to it.
fn child_path(cargo: &Path) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = cargo.parent().map(|p| vec![p.to_path_buf()]).unwrap_or_default();
    dirs.extend(search_dirs());
    std::env::join_paths(dirs).unwrap_or_default()
}

fn version_of(bin: &Path, path: &std::ffi::OsString) -> Option<String> {
    let out = std::process::Command::new(bin).arg("--version").env("PATH", path).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn toolchain() -> Value {
    let Some(cargo) = find("cargo") else {
        return json!({ "ok": false, "cargo": null, "rustc": null, "version": null, "installHint": INSTALL_HINT, "installUrl": "https://rustup.rs" });
    };
    let path = child_path(&cargo);
    let rustc = Some(cargo.with_file_name(if cfg!(windows) { "rustc.exe" } else { "rustc" })).filter(|p| p.is_file()).or_else(|| find("rustc"));
    let cargo_version = version_of(&cargo, &path);
    let rustc_version = rustc.as_ref().and_then(|r| version_of(r, &path));
    let ok = cargo_version.is_some() && rustc_version.is_some();
    json!({
        "ok": ok,
        "cargo": cargo.display().to_string(),
        "rustc": rustc.map(|r| r.display().to_string()),
        "version": rustc_version.clone().or(cargo_version.clone()),
        "cargoVersion": cargo_version,
        "rustcVersion": rustc_version,
        "installHint": if ok { Value::Null } else { json!(INSTALL_HINT) },
        "installUrl": "https://rustup.rs",
    })
}

// ---- crates ----------------------------------------------------------------------------------

/// A plugin crate's folder, for a valid name.
fn crate_dir(name: &str) -> CmdResult<PathBuf> {
    let ok = !name.is_empty() && name.len() <= 64 && name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !name.ends_with('-');
    if !ok {
        return Err(format!("`{name}` isn't a plugin crate name: lowercase letters, digits and dashes, starting with a letter (finance-extra, my-stats)."));
    }
    Ok(sources_dir().join(name))
}

/// An existing crate's folder.
fn existing_crate(name: &str) -> CmdResult<PathBuf> {
    let dir = crate_dir(name)?;
    if !dir.join("Cargo.toml").is_file() {
        return Err(format!("There is no plugin crate `{name}` in {}. Start one with plugin.new.", sources_dir().display()));
    }
    Ok(dir)
}

/// The SDK to build against: `FOLIO_PLUGIN_SDK`, else the copy folio carries (written out
/// when missing or out of date).
fn sdk_dir() -> CmdResult<PathBuf> {
    if let Some(p) = std::env::var_os("FOLIO_PLUGIN_SDK").filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        if p.join("Cargo.toml").is_file() {
            return Ok(p);
        }
    }
    let dir = sources_dir().join(".sdk").join(format!("folio-plugin-{}", env!("CARGO_PKG_VERSION")));
    for (rel, text) in SDK_FILES {
        let p = dir.join(rel);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(*text) {
            std::fs::create_dir_all(p.parent().expect("a file in a folder")).map_err(|e| format!("Couldn't write the SDK in {}: {e}", dir.display()))?;
            std::fs::write(&p, text).map_err(|e| format!("Couldn't write the SDK in {}: {e}", dir.display()))?;
        }
    }
    Ok(dir)
}

/// `my-stats` → `My stats`.
fn title(name: &str) -> String {
    let words = name.replace('-', " ");
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The library file names cargo makes for a crate.
fn library_names(name: &str) -> (String, String, String) {
    let u = name.replace('-', "_");
    (format!("lib{u}.dylib"), format!("lib{u}.so"), format!("{u}.dll"))
}

fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

const TEMPLATE_FUNCTIONS: &str = r#"//! {TITLE}: a folio plugin. `plugin.guide` explains the SDK; build with `plugin.build`,
//! install with `plugin.publishLocal`, then try it with `sheet.evaluate`.

use folio_plugin::prelude::*;

/// DOUBLE(number): twice the number. Replace it with your own functions.
fn double(args: &[Arg]) -> Result<f64, ErrorKind> {
    Ok(args[0].number()? * 2.0)
}

export! {
    id: "{ID}",
    name: "{TITLE}",
    version: env!("CARGO_PKG_VERSION"),
    functions: [
        // fn_def!(NAME, syntax, summary, min args, max args, function)
        fn_def!("DOUBLE", "DOUBLE(number)", "Twice the number.", 1, 1, double),
    ],
}
"#;

const TEMPLATE_FILTER: &str = r##"//! {TITLE}: a folio plugin with an import filter. `plugin.guide` explains the SDK and the
//! document's JSON; build with `plugin.build`, install with `plugin.publishLocal`.

use folio_plugin::prelude::*;

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Reads a file into a document: here, one paragraph per line. Replace it with your format.
fn import(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "This file isn't UTF-8 text.".to_string())?;
    let blocks: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| format!(r#"{{"type":"paragraph","runs":[{{"text":{}}}]}}"#, json_str(l.trim())))
        .collect();
    Ok(format!(r#"{{"title":"{TITLE}","pages":[{{"name":"{TITLE}","kind":"doc","blocks":[{}]}}]}}"#, blocks.join(",")))
}

export! {
    id: "{ID}",
    name: "{TITLE}",
    version: env!("CARGO_PKG_VERSION"),
    functions: [],
    filters: [
        // filter_def!(name, "ext1,ext2", summary, function)
        filter_def!("{TITLE}", "{EXT}", "One paragraph per line.", import),
    ],
}
"##;

fn new_crate(s: &Session, name: &str, kind: &str) -> CmdResult {
    let name = name.trim();
    if kind != "functions" && kind != "filter" {
        return Err(format!("A folio plugin's kind is functions or filter, not {kind}."));
    }
    let dir = crate_dir(name)?;
    if dir.join("Cargo.toml").exists() {
        return Err(format!("{} already exists: write to it with plugin.writeSource, or pick another name.", dir.display()));
    }
    let sdk = sdk_dir()?;
    let id = format!("local.{name}");
    let title = title(name);
    let (macos, linux, windows) = library_names(name);
    let author = s.settings().author();
    let cargo = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\n\
         # The SDK folio carries (works offline and matches this folio). Without folio on the machine:\n\
         # folio-plugin = {{ git = \"{GIT_URL}\", tag = \"{GIT_TAG}\" }}\n\
         folio-plugin = {{ path = {} }}\n\n\
         [profile.release]\n# So folio can catch a panic and switch the plugin off instead of crashing.\npanic = \"unwind\"\n\n\
         # A plugin builds on its own, outside any workspace.\n[workspace]\n",
        toml_str(&sdk.display().to_string())
    );
    let manifest = format!(
        "id = \"{id}\"\nname = {}\nversion = \"0.1.0\"\napp = \"folio\"\nkind = \"{kind}\"\nabi = {}\ndescription = \"What it adds, in one sentence.\"\nauthors = [{}]\n\n[library]\nmacos = \"{macos}\"\nlinux = \"{linux}\"\nwindows = \"{windows}\"\n",
        toml_str(&title),
        folio_plugin::ABI_VERSION,
        toml_str(&author)
    );
    let lib = if kind == "functions" { TEMPLATE_FUNCTIONS } else { TEMPLATE_FILTER };
    let ext: String = name.chars().filter(char::is_ascii_alphanumeric).take(8).collect();
    let lib = lib.replace("{ID}", &id).replace("{TITLE}", &title).replace("{EXT}", &ext);
    let files = [("Cargo.toml", cargo), ("plugin.toml", manifest), ("src/lib.rs", lib), (".gitignore", "/target\n".to_string())];
    for (rel, text) in &files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file in a folder")).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
        std::fs::write(&p, text).map_err(|e| format!("Couldn't write {}: {e}", p.display()))?;
    }
    Ok(json!({
        "name": name,
        "id": id,
        "kind": kind,
        "path": dir.display().to_string(),
        "files": files.iter().map(|(rel, _)| *rel).collect::<Vec<_>>(),
        "sdk": sdk.display().to_string(),
        "next": "Write src/lib.rs (plugin.writeSource or your own file tools), keep plugin.toml's id and export!'s the same, then plugin.build and plugin.publishLocal.",
    }))
}

/// A path inside a crate: relative, no `..`, not into `target/`.
fn inside(dir: &Path, rel: &str) -> CmdResult<PathBuf> {
    let p = Path::new(rel.trim());
    if rel.trim().is_empty() || p.is_absolute() || p.has_root() {
        return Err(format!("`{rel}` must be a path inside the crate, such as src/lib.rs."));
    }
    if !p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(format!("`{rel}` leaves the crate: use a path inside it, such as src/lib.rs."));
    }
    if p.components().find(|c| matches!(c, Component::Normal(_))).is_some_and(|c| c.as_os_str() == "target") {
        return Err("target/ is cargo's build output: write sources elsewhere.".into());
    }
    Ok(dir.join(p))
}

fn write_source(name: &str, rel: &str, contents: &str) -> CmdResult {
    let dir = existing_crate(name)?;
    if contents.len() > 1024 * 1024 {
        return Err("That file is over 1 MB: plugin sources are smaller.".into());
    }
    let path = inside(&dir, rel)?;
    let root = dir.canonicalize().map_err(|e| e.to_string())?;
    let parent = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't make {}: {e}", parent.display()))?;
    // A symbolic link anywhere on the way could lead out of the crate.
    let real_parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if !real_parent.starts_with(&root) {
        return Err(format!("`{rel}` leads out of the crate (a symbolic link): refused."));
    }
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("`{rel}` is a symbolic link: refused."));
    }
    std::fs::write(&path, contents).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    Ok(json!({ "name": name, "path": path.display().to_string(), "bytes": contents.len() }))
}

/// `cargo build --release` in a crate: `{ok, errors, warnings, library, seconds}`.
async fn build(name: &str) -> CmdResult {
    let dir = existing_crate(name)?;
    let cargo = find("cargo").ok_or(INSTALL_HINT)?;
    let target = dir.join("target");
    let started = Instant::now();
    let mut cmd = tokio::process::Command::new(&cargo);
    cmd.args(["build", "--release", "--message-format=json"])
        .current_dir(&dir)
        .env("PATH", child_path(&cargo))
        // The crate's own target folder, whatever the app was started with.
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("CARGO_BUILD_TARGET")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let out = match tokio::time::timeout(BUILD_TIMEOUT, cmd.output()).await {
        Err(_) => return Ok(json!({ "ok": false, "errors": [{ "file": null, "line": null, "column": null, "message": format!("The build took over {} minutes and was stopped.", BUILD_TIMEOUT.as_secs() / 60), "rendered": null }], "warnings": 0 })),
        Ok(Err(e)) => return Err(format!("Couldn't run {}: {e}", cargo.display())),
        Ok(Ok(out)) => out,
    };
    let (mut errors, warnings, library) = parse_cargo(&String::from_utf8_lossy(&out.stdout));
    let ok = out.status.success() && library.is_some();
    if !ok && errors.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr.lines().filter(|l| !l.trim_start().starts_with("Compiling") && !l.trim_start().starts_with("Blocking")).collect::<Vec<_>>().join("\n");
        let tail = last_chars(&tail, 2000);
        let message = if out.status.success() { "The build made no library: is crate-type = [\"cdylib\"] in Cargo.toml's [lib]?".to_string() } else { tail.clone() };
        errors.push(json!({ "file": null, "line": null, "column": null, "message": message, "rendered": tail }));
    }
    Ok(json!({
        "ok": ok,
        "errors": errors,
        "warnings": warnings,
        "library": library.map(|l| l.display().to_string()),
        "seconds": (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        "path": dir.display().to_string(),
    }))
}

fn last_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n { s.to_string() } else { format!("…{}", s.chars().skip(count - n).collect::<String>()) }
}

/// Reads cargo's JSON messages: errors (short), the warning count, and the cdylib it made.
fn parse_cargo(stdout: &str) -> (Vec<Value>, usize, Option<PathBuf>) {
    let ext = std::env::consts::DLL_EXTENSION;
    let (mut errors, mut warnings, mut library) = (Vec::new(), 0, None);
    for line in stdout.lines() {
        let Ok(msg) = serde_json::from_str::<Value>(line) else { continue };
        match msg["reason"].as_str() {
            Some("compiler-message") => {
                let m = &msg["message"];
                match m["level"].as_str() {
                    Some("error") | Some("error: internal compiler error") => {
                        let text = m["message"].as_str().unwrap_or("");
                        if text.starts_with("aborting due to") || errors.len() >= MAX_ERRORS {
                            continue;
                        }
                        let span = m["spans"].as_array().and_then(|s| s.iter().find(|s| s["is_primary"] == true).or(s.first()));
                        errors.push(json!({
                            "file": span.and_then(|s| s["file_name"].as_str()),
                            "line": span.and_then(|s| s["line_start"].as_u64()),
                            "column": span.and_then(|s| s["column_start"].as_u64()),
                            "message": text,
                            "rendered": m["rendered"].as_str().map(|r| last_chars(r, 2000)),
                        }));
                    }
                    Some("warning") => warnings += 1,
                    _ => {}
                }
            }
            Some("compiler-artifact") if msg["target"]["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "cdylib")) => {
                if let Some(files) = msg["filenames"].as_array() {
                    library = files.iter().filter_map(Value::as_str).map(PathBuf::from).find(|p| p.extension().is_some_and(|e| e == ext)).or(library);
                }
            }
            _ => {}
        }
    }
    (errors, warnings, library)
}

/// Build → bundle (`target/bundle/<id>/`: plugin.toml and the library) → install.
async fn publish(s: &Arc<Session>, name: &str) -> CmdResult {
    let dir = existing_crate(name)?;
    let mut manifest = Manifest::read(&dir).map_err(|e| format!("{e} (plugin.new writes one; the crate needs it to be installed)"))?;
    let built = build(name).await?;
    if built["ok"] != true {
        let mut v = built;
        v["installed"] = json!(false);
        return Ok(v);
    }
    let library = PathBuf::from(built["library"].as_str().ok_or("the build made no library")?);
    let file = library.file_name().ok_or("the library has no name")?.to_string_lossy().into_owned();
    manifest.library.set_current(file.clone());
    let bundle = dir.join("target").join("bundle").join(&manifest.id);
    let _ = std::fs::remove_dir_all(&bundle);
    std::fs::create_dir_all(&bundle).map_err(|e| format!("Couldn't make {}: {e}", bundle.display()))?;
    std::fs::write(bundle.join("plugin.toml"), toml::to_string_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::copy(&library, bundle.join(&file)).map_err(|e| format!("Couldn't copy {}: {e}", library.display()))?;
    if dir.join("README.md").is_file() {
        let _ = std::fs::copy(dir.join("README.md"), bundle.join("README.md"));
    }
    let id = s.plugins.install(s, &bundle)?;
    let info = s.plugins.info(&id, &s.settings().plugins.disabled).unwrap_or(Value::Null);
    Ok(json!({
        "ok": true,
        "installed": true,
        "id": id,
        "path": plugins_dir().join(&id).display().to_string(),
        "functions": info["functions"].as_array().map(|f| f.iter().map(|f| f["name"].clone()).collect::<Vec<_>>()).unwrap_or_default(),
        "filters": info["filters"].clone(),
        "skipped": info["skipped"].clone(),
        "warnings": built["warnings"].clone(),
        "next": "Try it: sheet.evaluate {formula: \"=YOURFUNCTION(…)\"}.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_crate() {
        let d = Path::new("/x/crate");
        assert!(inside(d, "src/lib.rs").is_ok());
        assert!(inside(d, "./src/a.rs").is_ok());
        for bad in ["../evil.rs", "src/../../evil.rs", "/etc/passwd", "", "target/release/x"] {
            assert!(inside(d, bad).is_err(), "{bad}");
        }
        assert!(crate_dir("my-stats").is_ok());
        for bad in ["My", "-a", "a-", "a/b", "a..b", ""] {
            assert!(crate_dir(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn cargo_messages_become_short_errors() {
        let out = r#"{"reason":"compiler-message","message":{"message":"cannot find value `x` in this scope","level":"error","spans":[{"file_name":"src/lib.rs","line_start":4,"column_start":9,"is_primary":true}],"rendered":"error[E0425]: cannot find value `x`\n"}}
{"reason":"compiler-message","message":{"message":"unused variable","level":"warning","spans":[],"rendered":"warning"}}
{"reason":"compiler-message","message":{"message":"aborting due to 1 previous error","level":"error","spans":[],"rendered":""}}
{"reason":"build-finished","success":false}"#;
        let (errors, warnings, lib) = parse_cargo(out);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["file"], "src/lib.rs");
        assert_eq!(errors[0]["line"], 4);
        assert_eq!(warnings, 1);
        assert!(lib.is_none());
    }
}
