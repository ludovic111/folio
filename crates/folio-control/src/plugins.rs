//! Plugins (lsuite's PLUGINS.md): folio's stock plugins, and lsuite plugins written in Rust on
//! the `folio-plugin` SDK, loaded at runtime.
//!
//! * **Stock** plugins are folio's own function categories (Math, Statistical…) and file
//!   filters (DOCX, XLSX…): listed, and switchable in settings. Switching one off is remembered
//!   but doesn't unhook it in this release ([`STOCK_NOTE`]).
//! * **Installed** plugins are bundles (a folder with `plugin.toml` and the library) in
//!   `~/.lsuite/plugins/folio/<id>/` (`LSUITE_HOME` replaces `~/.lsuite`). The host checks the
//!   manifest, copies the library to a fresh name under `<data>/plugin-cache/<pid>/` (so a
//!   rebuilt file can replace the installed one while the old copy is still loaded), loads it,
//!   checks the ABI before calling anything, and registers its functions with the open file's
//!   formula engine ([`Host::register_functions`], also called by `Session::open_doc`).
//! * **Hot reload**: [`Host::rescan`] reloads bundles whose library changed. Registered
//!   functions hold the library (an `Arc`), so the old copy is unloaded only once no engine can
//!   call it.
//! * **Crashes**: a panic in a plugin call shows `#VALUE!`, and the plugin is switched off
//!   (`settings.plugins.disabled`) and unloaded a moment later, with a toast. The app goes on.
//!
//! `FOLIO_PLUGIN_SDK` (a path to the `folio-plugin` crate) makes `plugin.new` build against
//! that folder instead of the copy of the SDK folio carries (see `commands/plugin.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::SystemTime;

use folio_calc::{Arg, ErrorKind, FunctionInfo, Value};
use folio_plugin::ffi::{self, FfiBytes, FfiValue, PluginVTable};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::session::{CmdResult, Event, Session, ToastKind};

/// What `plugin.info` says about switching off a stock plugin.
pub const STOCK_NOTE: &str = "Stock plugins ship with folio. Switching one off is remembered, but in this release it doesn't unhook its functions or file format yet.";

/// `~/.lsuite/plugins/folio`: installed bundles.
pub fn plugins_dir() -> PathBuf {
    crate::lsuite::lsuite_home().join("plugins").join("folio")
}

/// `~/.lsuite/plugins-src/folio`: plugin crates an agent writes.
pub fn sources_dir() -> PathBuf {
    crate::lsuite::lsuite_home().join("plugins-src").join("folio")
}

/// The library file name this platform loads, from `[library]`.
pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// `plugin.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub app: String,
    pub kind: String,
    pub abi: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub library: Libraries,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Libraries {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<String>,
}

impl Libraries {
    pub fn current(&self) -> Option<&str> {
        match platform() {
            "macos" => self.macos.as_deref(),
            "windows" => self.windows.as_deref(),
            _ => self.linux.as_deref(),
        }
    }

    pub fn set_current(&mut self, file: String) {
        match platform() {
            "macos" => self.macos = Some(file),
            "windows" => self.windows = Some(file),
            _ => self.linux = Some(file),
        }
    }
}

/// An id is reverse-DNS: letters, digits, `.`, `-`, `_`; it names the bundle's folder.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.starts_with('.') && !id.contains("..") && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

impl Manifest {
    pub fn read(dir: &Path) -> Result<Manifest, String> {
        let path = dir.join("plugin.toml");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        let m: Manifest = toml::from_str(&text).map_err(|e| format!("{} isn't a valid plugin.toml: {e}", path.display()))?;
        m.check()?;
        Ok(m)
    }

    pub fn check(&self) -> Result<(), String> {
        if !valid_id(&self.id) {
            return Err(format!("`{}` isn't a valid plugin id (reverse-DNS: letters, digits, dots, dashes).", self.id));
        }
        if self.app != "folio" {
            return Err(format!("This plugin is for {}, not folio (app = \"{}\").", self.app, self.app));
        }
        if self.kind != "functions" && self.kind != "filter" {
            return Err(format!("folio plugins are of kind \"functions\" or \"filter\", not \"{}\".", self.kind));
        }
        if self.abi != folio_plugin::ABI_VERSION {
            return Err(format!("This plugin speaks plugin ABI {}; this folio speaks ABI {}. Rebuild it with folio-plugin {}.", self.abi, folio_plugin::ABI_VERSION, env!("CARGO_PKG_VERSION")));
        }
        Ok(())
    }

    pub fn to_json(&self) -> Json {
        serde_json::to_value(self).unwrap_or(Json::Null)
    }
}

/// A function a plugin adds.
#[derive(Debug, Clone, Serialize)]
pub struct FnMeta {
    pub name: String,
    pub syntax: String,
    pub summary: String,
    #[serde(rename = "minArgs")]
    pub min: u32,
    #[serde(rename = "maxArgs")]
    pub max: u32,
}

/// An import filter a plugin adds.
#[derive(Debug, Clone, Serialize)]
pub struct FilterMeta {
    pub name: String,
    pub extensions: Vec<String>,
    pub summary: String,
}

/// A loaded plugin library. Dropping the last reference unloads it and deletes its copy.
pub struct Loaded {
    pub id: String,
    pub name: String,
    pub version: String,
    pub functions: Vec<FnMeta>,
    pub filters: Vec<FilterMeta>,
    call: ffi::CallFn,
    free_value: ffi::FreeValueFn,
    import: ffi::ImportFn,
    free_bytes: ffi::FreeBytesFn,
    /// Set by the first panic: no more calls go in.
    crashed: AtomicBool,
    lib: Option<libloading::Library>,
    copy: PathBuf,
}

impl Drop for Loaded {
    fn drop(&mut self) {
        drop(self.lib.take());
        let _ = std::fs::remove_file(&self.copy);
    }
}

/// A call that ended in a panic inside the plugin.
struct Crashed;

impl Loaded {
    /// Opens the library at `copy` and reads its table.
    fn open(copy: PathBuf) -> Result<Loaded, String> {
        // SAFETY: loading native code is what a plugin is; the person installed it. Nothing
        // is called before the ABI version is checked.
        let lib = unsafe { libloading::Library::new(&copy) }.map_err(|e| format!("Couldn't load the library: {e}"))?;
        let entry: ffi::EntryFn = unsafe {
            *lib.get::<ffi::EntryFn>(format!("{}\0", folio_plugin::ENTRY_SYMBOL).as_bytes()).map_err(|_| format!("The library has no `{}`: was it built with folio_plugin::export!?", folio_plugin::ENTRY_SYMBOL))?
        };
        // SAFETY: the entry point takes the host's ABI and returns a static table or null.
        let table = unsafe { entry(folio_plugin::ABI_VERSION) };
        if table.is_null() {
            return Err(format!("The plugin needs a newer folio (this one speaks plugin ABI {}).", folio_plugin::ABI_VERSION));
        }
        // SAFETY: `abi_version` is the first field of every ABI's table; read it alone first.
        let abi = unsafe { std::ptr::read(table as *const u32) };
        if abi != folio_plugin::ABI_VERSION {
            return Err(format!("The plugin speaks plugin ABI {abi}; this folio speaks ABI {}.", folio_plugin::ABI_VERSION));
        }
        // SAFETY: ABI 1 confirmed: the table has at least ABI 1's layout if its size says so.
        let size = unsafe { std::ptr::read((table as *const u32).add(1)) } as usize;
        if size < std::mem::size_of::<PluginVTable>() {
            return Err("The plugin's table is incomplete.".into());
        }
        let t: &PluginVTable = unsafe { &*table };
        let text = |s: ffi::FfiStr| -> Result<String, String> {
            if s.len > 64 * 1024 {
                return Err("The plugin's table holds an oversized string.".into());
            }
            // SAFETY: the table's strings are the library's statics.
            Ok(unsafe { s.to_string_lossy() })
        };
        if t.function_count > 10_000 || t.filter_count > 1_000 || (t.function_count > 0 && t.functions.is_null()) || (t.filter_count > 0 && t.filters.is_null()) {
            return Err("The plugin's table is malformed.".into());
        }
        let mut functions = Vec::new();
        for i in 0..t.function_count as usize {
            // SAFETY: `function_count` entries at `functions`, checked non-null.
            let f = unsafe { &*t.functions.add(i) };
            functions.push(FnMeta { name: text(f.name)?.trim().to_ascii_uppercase(), syntax: text(f.syntax)?, summary: text(f.summary)?, min: f.min_args.min(255), max: f.max_args.min(255) });
        }
        let mut filters = Vec::new();
        for i in 0..t.filter_count as usize {
            // SAFETY: `filter_count` entries at `filters`, checked non-null.
            let f = unsafe { &*t.filters.add(i) };
            let ext = text(f.extensions)?;
            filters.push(FilterMeta {
                name: text(f.name)?,
                extensions: ext.split(',').map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|e| !e.is_empty()).collect(),
                summary: text(f.summary)?,
            });
        }
        Ok(Loaded {
            id: text(t.id)?,
            name: text(t.name)?,
            version: text(t.version)?,
            functions,
            filters,
            call: t.call,
            free_value: t.free_value,
            import: t.import,
            free_bytes: t.free_bytes,
            crashed: AtomicBool::new(false),
            lib: Some(lib),
            copy,
        })
    }

    pub fn crashed(&self) -> bool {
        self.crashed.load(Ordering::Acquire)
    }

    /// Calls function `index` with the engine's arguments.
    fn call(&self, index: u32, args: &[Arg]) -> Result<Value, Crashed> {
        if self.crashed() {
            return Ok(Value::Error(ErrorKind::Value));
        }
        // Range cells first (their buffers must not move), then the arguments pointing at them.
        let ranges: Vec<Vec<FfiValue>> = args
            .iter()
            .map(|a| match a {
                Arg::Range { values, .. } => values.iter().map(lower).collect(),
                Arg::Value(_) => Vec::new(),
            })
            .collect();
        let raw: Vec<FfiValue> = args
            .iter()
            .zip(&ranges)
            .map(|(a, cells)| match a {
                Arg::Value(v) => lower(v),
                Arg::Range { rows, cols, .. } => {
                    let (r, c) = if (*rows as usize) * (*cols as usize) == cells.len() { (*rows, *cols) } else { (1, cells.len() as u32) };
                    FfiValue::range(r, c, cells)
                }
            })
            .collect();
        let mut out = FfiValue::EMPTY;
        // SAFETY: `raw`, `ranges` and the engine's strings outlive the call; `out` is ours.
        let status = unsafe { (self.call)(index, raw.as_ptr(), raw.len() as u32, &mut out) };
        // SAFETY: the plugin wrote `out` (or left Empty); its text is valid until freed.
        let v = unsafe { out.read_scalar() };
        unsafe { (self.free_value)(&mut out) };
        match status {
            ffi::OK => Ok(raise(v)),
            ffi::PANICKED => Err(Crashed),
            ffi::NO_SUCH_FUNCTION => Ok(Value::Error(ErrorKind::Name)),
            _ => Ok(Value::Error(ErrorKind::Value)),
        }
    }

    /// Runs import filter `index` on a file's bytes: the document's JSON.
    fn import(&self, index: u32, bytes: &[u8]) -> Result<Result<String, String>, Crashed> {
        if self.crashed() {
            return Ok(Err(format!("The plugin {} was switched off after a crash.", self.name)));
        }
        let mut out = FfiBytes::EMPTY;
        // SAFETY: `bytes` outlives the call; `out` is ours.
        let status = unsafe { (self.import)(index, bytes.as_ptr(), bytes.len(), &mut out) };
        let data = if out.ptr.is_null() || out.len == 0 {
            Vec::new()
        } else {
            // SAFETY: the plugin wrote `len` bytes at `ptr`; we copy them before freeing.
            unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec()
        };
        unsafe { (self.free_bytes)(out) };
        let text = String::from_utf8_lossy(&data).into_owned();
        match status {
            ffi::OK => Ok(Ok(text)),
            ffi::PANICKED => Err(Crashed),
            ffi::FAILED if !text.is_empty() => Ok(Err(text)),
            _ => Ok(Err(format!("The plugin {} couldn't read this file.", self.name))),
        }
    }
}

/// An engine value in its C form, borrowing its text.
fn lower(v: &Value) -> FfiValue {
    match v {
        Value::Empty => FfiValue::EMPTY,
        Value::Number(n) => FfiValue::number(*n),
        Value::Text(s) => FfiValue::text(s),
        Value::Bool(b) => FfiValue::boolean(*b),
        Value::Error(e) => FfiValue::error(match e {
            ErrorKind::Div0 => folio_plugin::ErrorKind::Div0,
            ErrorKind::NA => folio_plugin::ErrorKind::NA,
            ErrorKind::Name => folio_plugin::ErrorKind::Name,
            ErrorKind::Null => folio_plugin::ErrorKind::Null,
            ErrorKind::Num => folio_plugin::ErrorKind::Num,
            ErrorKind::Ref => folio_plugin::ErrorKind::Ref,
            ErrorKind::Value => folio_plugin::ErrorKind::Value,
            ErrorKind::Circular => folio_plugin::ErrorKind::Circular,
        }),
    }
}

/// An SDK value as the engine's. Text is capped at a spreadsheet cell's 32,767 characters.
fn raise(v: folio_plugin::Value) -> Value {
    use folio_plugin::Value as P;
    match v {
        P::Empty => Value::Empty,
        P::Number(n) if n.is_finite() => Value::Number(n),
        P::Number(_) => Value::Error(ErrorKind::Num),
        P::Text(s) if s.chars().count() > 32_767 => Value::Text(s.chars().take(32_767).collect()),
        P::Text(s) => Value::Text(s),
        P::Bool(b) => Value::Bool(b),
        P::Error(e) => Value::Error(match e {
            folio_plugin::ErrorKind::Div0 => ErrorKind::Div0,
            folio_plugin::ErrorKind::NA => ErrorKind::NA,
            folio_plugin::ErrorKind::Name => ErrorKind::Name,
            folio_plugin::ErrorKind::Null => ErrorKind::Null,
            folio_plugin::ErrorKind::Num => ErrorKind::Num,
            folio_plugin::ErrorKind::Ref => ErrorKind::Ref,
            folio_plugin::ErrorKind::Value => ErrorKind::Value,
            folio_plugin::ErrorKind::Circular => ErrorKind::Circular,
        }),
    }
}

/// An installed bundle, loaded or not.
pub struct Bundle {
    pub dir: PathBuf,
    pub manifest: Option<Manifest>,
    pub library: Option<PathBuf>,
    /// The library's size and modification time when it was loaded (hot reload compares).
    stamp: Option<(u64, Option<SystemTime>)>,
    pub loaded: Option<Arc<Loaded>>,
    /// Why it isn't loaded (a bad manifest, a missing library, a crash).
    pub error: Option<String>,
    /// Functions it declares that folio didn't register (built-in names, taken by another plugin).
    pub skipped: Vec<String>,
}

impl Bundle {
    pub fn id(&self) -> String {
        self.manifest.as_ref().map(|m| m.id.clone()).unwrap_or_else(|| self.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
    }

    /// One line of `plugin.list`.
    pub fn summary(&self, enabled: bool) -> Json {
        let m = self.manifest.as_ref();
        let l = self.loaded.as_ref();
        json!({
            "id": self.id(),
            "name": m.map(|m| m.name.clone()).or_else(|| l.map(|l| l.name.clone())).unwrap_or_else(|| self.id()),
            "kind": m.map(|m| m.kind.as_str()).unwrap_or("functions"),
            "format": "lsuite",
            "version": m.map(|m| m.version.clone()).unwrap_or_default(),
            "description": m.map(|m| m.description.clone()).unwrap_or_default(),
            "authors": m.map(|m| m.authors.clone()).unwrap_or_default(),
            "path": self.dir.display().to_string(),
            "stock": false,
            "enabled": enabled,
            "loaded": l.is_some_and(|l| !l.crashed()),
            "functions": l.map(|l| l.functions.iter().map(|f| f.name.clone()).collect::<Vec<_>>()).unwrap_or_default(),
            "filters": l.map(|l| l.filters.iter().map(|f| f.name.clone()).collect::<Vec<_>>()).unwrap_or_default(),
            "error": self.error,
        })
    }

    /// `plugin.info`.
    pub fn details(&self, enabled: bool) -> Json {
        let mut v = self.summary(enabled);
        let l = self.loaded.as_ref();
        v["functions"] = json!(l.map(|l| l.functions.clone()).unwrap_or_default());
        v["filters"] = json!(l.map(|l| l.filters.clone()).unwrap_or_default());
        v["manifest"] = self.manifest.as_ref().map(Manifest::to_json).unwrap_or(Json::Null);
        v["library"] = json!(self.library.as_ref().map(|p| p.display().to_string()));
        v["skipped"] = json!(self.skipped);
        v["source"] = json!(format!("Installed lsuite plugin (Rust, plugin ABI {}).", folio_plugin::ABI_VERSION));
        if let Some(l) = l {
            v["libraryReports"] = json!({ "id": l.id, "name": l.name, "version": l.version });
        }
        v
    }
}

#[derive(Default)]
struct State {
    /// By id, in id order.
    bundles: BTreeMap<String, Bundle>,
    /// Function names this host has registered in an engine, to take them back out.
    registered: BTreeSet<String>,
}

pub struct Host {
    state: Mutex<State>,
    session: OnceLock<Weak<Session>>,
    cache: OnceLock<PathBuf>,
    copies: AtomicU64,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    pub fn new() -> Self {
        Host { state: Mutex::new(State::default()), session: OnceLock::new(), cache: OnceLock::new(), copies: AtomicU64::new(0) }
    }

    /// Scans the plugin folder and loads enabled plugins. Called once by `Session::new`.
    pub fn load_all(&self, session: &Arc<Session>) {
        let _ = self.session.set(Arc::downgrade(session));
        // Copies of libraries live in a folder of this process; other processes' old folders go.
        let root = session.data_dir.join("plugin-cache");
        let mine = root.join(std::process::id().to_string());
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                if e.path() != mine {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        let _ = self.cache.set(mine);
        self.scan(&session.settings().plugins.disabled, &BTreeSet::new());
    }

    /// Registers every enabled plugin's spreadsheet functions with an engine (and takes back
    /// those of plugins unloaded since). Call `Editor::recalc_all` after.
    pub fn register_functions(&self, engine: &mut folio_calc::Engine) {
        let mut st = self.state.lock();
        for name in std::mem::take(&mut st.registered) {
            engine.unregister_function(&name);
        }
        let weak = self.session.get().cloned().unwrap_or_default();
        let mut registered = BTreeSet::new();
        for bundle in st.bundles.values_mut() {
            bundle.skipped.clear();
            let Some(lib) = bundle.loaded.clone().filter(|l| !l.crashed()) else { continue };
            for (i, f) in lib.functions.iter().enumerate() {
                if registered.contains(&f.name) {
                    bundle.skipped.push(format!("{} (another plugin has it)", f.name));
                    continue;
                }
                let info = FunctionInfo { name: f.name.clone(), syntax: f.syntax.clone(), summary: f.summary.clone(), category: "Custom".into() };
                let (lib2, weak2, min, max, index) = (lib.clone(), weak.clone(), f.min as usize, f.max as usize, i as u32);
                let run: folio_calc::CustomFn = Arc::new(move |args: &[Arg]| {
                    if args.len() < min || args.len() > max {
                        return Value::Error(ErrorKind::Value);
                    }
                    match lib2.call(index, args) {
                        Ok(v) => v,
                        Err(Crashed) => {
                            crashed(&lib2, &weak2);
                            Value::Error(ErrorKind::Value)
                        }
                    }
                });
                if engine.register_function(info, run) {
                    registered.insert(f.name.clone());
                } else {
                    bundle.skipped.push(format!("{} (built into folio)", f.name));
                }
            }
        }
        st.registered = registered;
    }

    /// Registers the functions again with the open file's engine and recalculates, then tells
    /// every client.
    pub fn apply(&self, session: &Session) {
        let _ = session.with_editor(|ed| {
            self.register_functions(ed.calc_mut().engine_mut());
            ed.recalc_all();
        });
        session.emit(Event::PluginsChanged);
    }

    /// Reads the plugin folder: new bundles load, changed libraries reload (hot reload),
    /// disabled ones unload, removed ones go. `force` reloads those ids even if unchanged.
    fn scan(&self, disabled: &[String], force: &BTreeSet<String>) -> Json {
        let dir = plugins_dir();
        let mut found: BTreeMap<String, (PathBuf, Result<Manifest, String>)> = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let path = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                if !path.is_dir() || name.starts_with('.') || name.contains(".staging") || name.contains(".old") {
                    continue;
                }
                let m = Manifest::read(&path);
                let key = m.as_ref().map(|m| m.id.clone()).unwrap_or_else(|_| name.clone());
                found.insert(key, (path, m));
            }
        }
        let mut st = self.state.lock();
        let mut report = json!({ "loaded": [], "reloaded": [], "unloaded": [], "errors": [] });
        let gone: Vec<String> = st.bundles.keys().filter(|k| !found.contains_key(*k)).cloned().collect();
        for id in gone {
            if st.bundles.remove(&id).is_some_and(|b| b.loaded.is_some()) {
                push(&mut report, "unloaded", &id);
            }
        }
        for (id, (path, manifest)) in found {
            let manifest = match manifest {
                Ok(m) => m,
                Err(e) => {
                    push(&mut report, "errors", &format!("{id}: {e}"));
                    st.bundles.insert(id, Bundle { dir: path, manifest: None, library: None, stamp: None, loaded: None, error: Some(e), skipped: vec![] });
                    continue;
                }
            };
            let library = manifest.library.current().map(|f| path.join(f));
            let stamp = library.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| (m.len(), m.modified().ok()));
            let enabled = !disabled.contains(&id);
            let prev = st.bundles.remove(&id);
            let unchanged = prev.as_ref().is_some_and(|b| b.loaded.is_some() && b.stamp == stamp && b.dir == path) && !force.contains(&id);
            let mut bundle = Bundle { dir: path, manifest: Some(manifest.clone()), library: library.clone(), stamp, loaded: None, error: None, skipped: vec![] };
            if !enabled {
                if prev.is_some_and(|b| b.loaded.is_some()) {
                    push(&mut report, "unloaded", &id);
                }
            } else if unchanged {
                let prev = prev.expect("checked");
                bundle.loaded = prev.loaded;
                bundle.error = prev.error;
            } else {
                match library {
                    None => bundle.error = Some(format!("plugin.toml names no library for {} ([library] {}).", platform(), platform())),
                    Some(lib) if !lib.is_file() => bundle.error = Some(format!("The library {} is missing.", lib.display())),
                    Some(lib) => match self.load(&lib, &manifest) {
                        Ok(l) => {
                            let was = prev.as_ref().is_some_and(|b| b.loaded.is_some());
                            push(&mut report, if was { "reloaded" } else { "loaded" }, &id);
                            bundle.loaded = Some(Arc::new(l));
                        }
                        Err(e) => {
                            push(&mut report, "errors", &format!("{id}: {e}"));
                            bundle.error = Some(e);
                        }
                    },
                }
            }
            st.bundles.insert(id, bundle);
        }
        report
    }

    /// Copies a library to a fresh name and loads it, checking it is the plugin the manifest says.
    fn load(&self, library: &Path, manifest: &Manifest) -> Result<Loaded, String> {
        let cache = self.cache.get().cloned().unwrap_or_else(|| std::env::temp_dir().join(format!("folio-plugin-cache-{}", std::process::id())));
        std::fs::create_dir_all(&cache).map_err(|e| format!("Couldn't make {}: {e}", cache.display()))?;
        let n = self.copies.fetch_add(1, Ordering::Relaxed);
        let ext = library.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let copy = cache.join(format!("{}-{n}.{ext}", manifest.id));
        std::fs::copy(library, &copy).map_err(|e| format!("Couldn't copy {}: {e}", library.display()))?;
        let loaded = match Loaded::open(copy.clone()) {
            Ok(l) => l,
            Err(e) => {
                let _ = std::fs::remove_file(&copy);
                return Err(e);
            }
        };
        if loaded.id != manifest.id {
            return Err(format!("The library says it is `{}` but plugin.toml says `{}`: make export!'s id and plugin.toml's the same.", loaded.id, manifest.id));
        }
        Ok(loaded)
    }

    /// `plugin.rescan`: reloads what changed, then registers and recalculates.
    pub fn rescan(&self, session: &Session) -> Json {
        let report = self.scan(&session.settings().plugins.disabled, &BTreeSet::new());
        self.apply(session);
        report
    }

    /// Installs a bundle folder: checks it loads, copies it into the plugin folder (replacing
    /// an earlier version), switches it on and loads it. Returns its id.
    pub fn install(&self, session: &Session, from: &Path) -> CmdResult<String> {
        let manifest = Manifest::read(from)?;
        let lib_name = manifest.library.current().ok_or_else(|| format!("plugin.toml names no library for {} (add [library] {} = \"…\").", platform(), platform()))?.to_string();
        if Path::new(&lib_name).components().count() != 1 {
            return Err(format!("[library] {} should be a file name next to plugin.toml, not {lib_name}.", platform()));
        }
        let lib = from.join(&lib_name);
        if !lib.is_file() {
            return Err(format!("The library {} isn't there. Build the plugin first (plugin.build).", lib.display()));
        }
        // Refuse a library that doesn't load before touching the installed one.
        drop(self.load(&lib, &manifest)?);
        let root = plugins_dir();
        std::fs::create_dir_all(&root).map_err(|e| format!("Couldn't make {}: {e}", root.display()))?;
        let dest = root.join(&manifest.id);
        let same = from.canonicalize().ok().zip(dest.canonicalize().ok()).is_some_and(|(a, b)| a == b);
        if !same {
            let n = self.copies.fetch_add(1, Ordering::Relaxed);
            let staging = root.join(format!(".{}.staging-{}-{n}", manifest.id, std::process::id()));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
            let text = toml::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
            std::fs::write(staging.join("plugin.toml"), text).map_err(|e| e.to_string())?;
            std::fs::copy(&lib, staging.join(&lib_name)).map_err(|e| format!("Couldn't copy the library: {e}"))?;
            // Other platforms' libraries and a README travel with the bundle.
            for other in [&manifest.library.macos, &manifest.library.linux, &manifest.library.windows].into_iter().flatten().chain([&"README.md".to_string()]) {
                if *other != lib_name && Path::new(other).components().count() == 1 && from.join(other).is_file() {
                    let _ = std::fs::copy(from.join(other), staging.join(other));
                }
            }
            let old = root.join(format!(".{}.old-{}-{n}", manifest.id, std::process::id()));
            if dest.exists() {
                std::fs::rename(&dest, &old).map_err(|e| format!("Couldn't replace {}: {e}", dest.display()))?;
            }
            if let Err(e) = std::fs::rename(&staging, &dest) {
                let _ = std::fs::rename(&old, &dest);
                return Err(format!("Couldn't install into {}: {e}", dest.display()));
            }
            let _ = std::fs::remove_dir_all(&old);
        }
        let id = manifest.id.clone();
        let disabled = session.update_settings(|s| s.plugins.disabled.retain(|d| *d != id))?.plugins.disabled;
        let report = self.scan(&disabled, &BTreeSet::from([id.clone()]));
        self.apply(session);
        if let Some(e) = self.state.lock().bundles.get(&id).and_then(|b| b.error.clone()) {
            return Err(format!("Installed in {}, but it didn't load: {e}", dest.display()));
        }
        tracing::info!("installed plugin {id}: {report}");
        Ok(id)
    }

    /// Removes an installed plugin: unloads it and deletes its folder.
    pub fn remove(&self, session: &Session, id: &str) -> CmdResult<PathBuf> {
        let dir = {
            let st = self.state.lock();
            st.bundles.get(id).map(|b| b.dir.clone())
        };
        let dir = dir.or_else(|| Some(plugins_dir().join(id)).filter(|d| valid_id(id) && d.is_dir())).ok_or_else(|| format!("No installed plugin `{id}` (plugin.list shows them; stock plugins can only be disabled)."))?;
        let removed = self.state.lock().bundles.remove(id);
        drop(removed);
        self.apply(session);
        std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete {}: {e}", dir.display()))?;
        let _ = session.update_settings(|s| s.plugins.disabled.retain(|d| d != id));
        Ok(dir)
    }

    /// Brings loaded plugins in line with `settings.plugins.disabled` (after a switch).
    pub fn refresh(&self, session: &Session) {
        self.scan(&session.settings().plugins.disabled, &BTreeSet::new());
        self.apply(session);
    }

    /// Whether `id` is an installed plugin's.
    pub fn is_installed(&self, id: &str) -> bool {
        self.state.lock().bundles.contains_key(id)
    }

    /// `plugin.list`'s installed part.
    pub fn list(&self, disabled: &[String]) -> Vec<Json> {
        self.state.lock().bundles.iter().map(|(id, b)| b.summary(!disabled.contains(id))).collect()
    }

    /// `plugin.info` for an installed plugin.
    pub fn info(&self, id: &str, disabled: &[String]) -> Option<Json> {
        self.state.lock().bundles.get(id).map(|b| b.details(!disabled.contains(&id.to_string())))
    }

    /// The import filters of loaded plugins: `(plugin id, filter)`.
    pub fn filters(&self) -> Vec<(String, FilterMeta)> {
        let st = self.state.lock();
        st.bundles.values().filter_map(|b| b.loaded.as_ref()).filter(|l| !l.crashed()).flat_map(|l| l.filters.iter().map(|f| (l.id.clone(), f.clone()))).collect()
    }

    /// Reads a file with a plugin's import filter for its extension, if one has it: `None`
    /// when no plugin reads that extension. folio's own formats come first; the caller asks
    /// here for the others.
    pub fn import(&self, path: &Path) -> Option<CmdResult<folio_core::Document>> {
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        let (lib, index) = {
            let st = self.state.lock();
            st.bundles.values().filter_map(|b| b.loaded.clone()).filter(|l| !l.crashed()).find_map(|l| {
                let i = l.filters.iter().position(|f| f.extensions.contains(&ext))?;
                Some((l, i as u32))
            })?
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return Some(Err(format!("Couldn't read {}: {e}", path.display()))),
        };
        let json = match lib.import(index, &bytes) {
            Ok(Ok(json)) => json,
            Ok(Err(message)) => return Some(Err(message)),
            Err(Crashed) => {
                crashed(&lib, &self.session.get().cloned().unwrap_or_default());
                return Some(Err(format!("The plugin {} crashed reading this file and was switched off.", lib.name)));
            }
        };
        Some(document_from_json(&json, path).map_err(|e| format!("The plugin {} gave a document folio can't read: {e}", lib.name)))
    }

    /// The plugin a crash came from: switch it off for good (a setting), unload, recalculate.
    fn on_crash(&self, session: &Session, id: &str) {
        let _ = session.update_settings(|s| {
            if !s.plugins.disabled.iter().any(|d| d == id) {
                s.plugins.disabled.push(id.to_string());
            }
        });
        {
            let mut st = self.state.lock();
            if let Some(b) = st.bundles.get_mut(id) {
                b.loaded = None;
                b.error = Some("It crashed (a panic in one of its calls) and was switched off. Switch it on again once it is fixed.".into());
            }
        }
        self.apply(session);
        session.toast(ToastKind::Error, format!("The plugin {id} crashed and was switched off."));
    }
}

fn push(report: &mut Json, key: &str, item: &str) {
    if let Some(a) = report[key].as_array_mut() {
        a.push(json!(item));
    }
}

/// The first panic of a plugin: no more calls go in, and a moment later (once the formula
/// engine that called it lets go of the file) the plugin is switched off and unloaded.
fn crashed(lib: &Arc<Loaded>, session: &Weak<Session>) {
    if lib.crashed.swap(true, Ordering::AcqRel) {
        return;
    }
    tracing::warn!("plugin {} panicked; switching it off", lib.id);
    let (id, session) = (lib.id.clone(), session.clone());
    std::thread::spawn(move || {
        if let Some(s) = session.upgrade() {
            s.plugins.on_crash(&s, &id);
        }
    });
}

/// A plugin's `document.json`, with `format`, the document's `id` and every page's and
/// block's `id` filled in where the plugin left them out.
pub fn document_from_json(json: &str, path: &Path) -> Result<folio_core::Document, String> {
    let mut v: Json = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
    let o = v.as_object_mut().ok_or("not a JSON object")?;
    o.entry("format").or_insert(json!(folio_core::FORMAT));
    o.entry("id").or_insert_with(|| json!(folio_core::Id::new().to_string()));
    o.entry("title").or_insert_with(|| json!(path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported".into())));
    if let Some(pages) = o.get_mut("pages").and_then(Json::as_array_mut) {
        for page in pages.iter_mut().filter_map(Json::as_object_mut) {
            page.entry("id").or_insert_with(|| json!(folio_core::Id::new().to_string()));
            for key in ["blocks", "slides"] {
                if let Some(items) = page.get_mut(key).and_then(Json::as_array_mut) {
                    for item in items.iter_mut().filter_map(Json::as_object_mut) {
                        item.entry("id").or_insert_with(|| json!(folio_core::Id::new().to_string()));
                    }
                }
            }
        }
    }
    let doc: folio_core::Document = serde_json::from_value(v).map_err(|e| e.to_string())?;
    if doc.pages.is_empty() {
        return Err("the document has no pages".into());
    }
    Ok(doc)
}

// ---- stock plugins ---------------------------------------------------------------------------

/// A stock plugin: a category of built-in functions or a file format folio reads and writes.
#[derive(Debug, Clone)]
pub struct Stock {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub description: String,
    /// Functions (name, syntax, summary) or formats (id, name, extensions).
    pub details: Json,
}

/// The function categories, in the function picker's order.
const CATEGORIES: &[&str] = &["Math", "Statistical", "Logical", "Lookup", "Text", "Date", "Financial", "Information"];

/// folio's stock plugins: its function categories and its file filters (ODT, ODS and ODP
/// together as OpenDocument).
pub fn stock() -> Vec<Stock> {
    let builtins = folio_calc::builtin_functions();
    let mut cats: Vec<&str> = CATEGORIES.to_vec();
    for b in builtins {
        if !cats.contains(&b.category) {
            cats.push(b.category);
        }
    }
    let mut out = Vec::new();
    for cat in cats {
        let fns: Vec<_> = builtins.iter().filter(|b| b.category == cat).collect();
        if fns.is_empty() {
            continue;
        }
        let names: Vec<&str> = fns.iter().map(|b| b.name).collect();
        let shown = if names.len() > 6 { format!("{}…", names[..6].join(", ")) } else { names.join(", ") };
        out.push(Stock {
            id: format!("folio.functions.{}", cat.to_ascii_lowercase()),
            name: format!("{cat} functions"),
            kind: "functions",
            description: format!("{} built-in functions: {shown}", fns.len()),
            details: json!(fns.iter().map(|b| json!({ "name": b.name, "syntax": b.syntax, "summary": b.summary })).collect::<Vec<_>>()),
        });
    }
    let mut groups: Vec<(String, String, Vec<folio_io::Format>)> = Vec::new();
    for f in folio_io::formats() {
        let (id, name) = if f.id.starts_with("od") { ("odf".to_string(), "OpenDocument (ODT, ODS, ODP)".to_string()) } else { (f.id.to_string(), f.name.to_string()) };
        match groups.iter_mut().find(|g| g.0 == id) {
            Some(g) => g.2.push(f),
            None => groups.push((id, name, vec![f])),
        }
    }
    for (id, name, formats) in groups {
        let exts: Vec<String> = formats.iter().flat_map(|f| f.extensions.iter().map(|e| format!(".{e}"))).collect();
        let (import, export) = (formats.iter().any(|f| f.import), formats.iter().any(|f| f.export));
        let how = match (import, export) {
            (true, true) => "Opens and saves",
            (true, false) => "Opens",
            _ => "Saves",
        };
        out.push(Stock {
            id: format!("folio.filters.{id}"),
            name: format!("{name} filter"),
            kind: "filter",
            description: format!("{how} {}.", exts.join(", ")),
            details: json!(formats.iter().map(|f| json!({ "id": f.id, "name": f.name, "extensions": f.extensions, "kinds": f.kinds, "import": f.import, "export": f.export, "apps": f.apps, "notes": f.notes })).collect::<Vec<_>>()),
        });
    }
    out
}

impl Stock {
    pub fn summary(&self, enabled: bool) -> Json {
        json!({
            "id": self.id,
            "name": self.name,
            "kind": self.kind,
            "format": "stock",
            "version": env!("CARGO_PKG_VERSION"),
            "description": self.description,
            "path": null,
            "stock": true,
            "enabled": enabled,
        })
    }

    pub fn details(&self, enabled: bool) -> Json {
        let mut v = self.summary(enabled);
        v[if self.kind == "functions" { "functions" } else { "formats" }] = self.details.clone();
        v["source"] = json!("Ships with folio.");
        v["note"] = json!(STOCK_NOTE);
        v
    }
}
