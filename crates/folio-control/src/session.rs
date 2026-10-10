//! Everything folio keeps in memory: the open file with its one undo history, settings, the
//! plugins, and the event stream every client listens to.
//!
//! There is one [`Session`] per process. In the desktop app the window, the built-in agent and
//! every bridge client (CLI, MCP) share it, so they share the undo history too. `folio-cli
//! --file` and `folio-mcp --file` make their own headless session around a file.
//!
//! Files save themselves: every change is written to the file a moment later (at once in a
//! headless session, which may end right after the command). A new file that hasn't been saved
//! anywhere yet lives in `<data>/untitled/` until **Save as** puts it in a folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use folio_core::{Document, Editor, Id};
use futures::channel::{mpsc, oneshot};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::secrets::{MemorySecrets, SecretStore};
use crate::settings::Settings;

pub type CmdResult<T = Value> = Result<T, String>;

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

tokio::task_local! {
    /// Set while `file.batch` runs its commands: their edits belong to its open batch.
    static IN_BATCH: ();
}

/// Whether this task is running a `file.batch`'s commands.
pub(crate) fn in_batch_scope() -> bool {
    IN_BATCH.try_with(|_| ()).is_ok()
}

/// Runs `f` as part of the open `file.batch`.
pub(crate) async fn batch_scope<F: std::future::Future>(f: F) -> F::Output {
    IN_BATCH.scope((), f).await
}

/// Who is calling a command. Agent and MCP calls are checked against
/// `settings.agent.permissions`; the window and plain CLI calls are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Window,
    Agent,
    Cli,
    Mcp,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Window => "window",
            Source::Agent => "agent",
            Source::Cli => "cli",
            Source::Mcp => "mcp",
        }
    }

    /// Whether agent permissions apply.
    pub fn is_agent(self) -> bool {
        matches!(self, Source::Agent | Source::Mcp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

/// One command any client ran, as shown on the agent panel's cards and in its list of changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRecord {
    pub seq: u64,
    pub source: Source,
    pub command: String,
    pub params: Value,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    pub mutates: bool,
    pub at: DateTime<Utc>,
    /// The result, when it is small (a few KB).
    #[serde(default)]
    pub result: Option<Value>,
    /// For agents and MCP clients: the checkpoint taken before their first change in this
    /// connection, so a whole session from a terminal can be reverted (`history.revertTo`).
    #[serde(default)]
    pub checkpoint: Option<u64>,
}

/// What the window shows. The window pushes it with [`Session::set_ui_state`]; `ui.state`
/// returns it and commands use the caret, the selected cells and the slide as defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    /// `home` or `editor`.
    pub screen: String,
    /// The page shown (id).
    pub page: Option<String>,
    /// Documents: the selection, `anchor` and `focus` positions (`{block, cell?, offset}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextSelection>,
    /// Sheets: the active cell and the selected range (A1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// Decks: the slide shown (0-based) and the selected shapes (ids).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<String>,
    /// Zoom, 1 = 100 %.
    pub zoom: f32,
    /// `dark` or `light`.
    pub theme: String,
    /// Open panels and dialogs (`agent`, `inspector`, `settings`, `export`…).
    pub open: Vec<String>,
    /// Presenting a deck.
    pub presenting: bool,
    /// The window's size in pixels.
    pub window: [f32; 2],
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct TextSelection {
    pub anchor: folio_core::Pos,
    pub focus: folio_core::Pos,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Update { status: crate::update::UpdateStatus },
    /// The open file changed (any client, an undo, a plugin's results).
    DocChanged { version: u64 },
    /// Another file was opened, or the file was closed (`None`).
    DocSwitched { id: Option<String> },
    /// The file was written to disk.
    Saved { path: String },
    Toast { kind: ToastKind, text: String },
    Command { record: CommandRecord },
    SettingsChanged,
    /// Plugins were installed, removed, switched or reloaded.
    PluginsChanged,
}

/// The built-in agent, installed by the app (`folio-agent` depends on this crate, so the
/// `agent.*` commands reach it through this trait).
pub trait AgentHost: Send + Sync {
    fn call(self: Arc<Self>, session: Arc<Session>, source: Source, command: &'static str, args: crate::registry::Args) -> futures::future::BoxFuture<'static, CmdResult>;
}

/// A command only the window can carry out (`ui.*`, presenting, screenshots).
pub struct UiCall {
    pub command: String,
    pub params: Value,
    pub reply: oneshot::Sender<CmdResult>,
}

/// The open file.
pub struct OpenDoc {
    pub editor: Editor,
    /// Where it saves itself.
    pub path: PathBuf,
    /// Not saved in a folder yet (it lives in `<data>/untitled/`).
    pub untitled: bool,
    /// The file it was imported from, and the format (`docx`…), for exporting back.
    pub origin: Option<(PathBuf, String)>,
    /// The editor version last written to disk.
    pub saved: u64,
}

pub struct SessionOptions {
    /// Defaults to `<OS data dir>/folio` (`FOLIO_DATA_DIR`).
    pub data_dir: Option<PathBuf>,
    /// Defaults to `<OS config dir>/folio` (`FOLIO_CONFIG_DIR`).
    pub config_dir: Option<PathBuf>,
    pub secrets: Option<Arc<dyn SecretStore>>,
    /// No window will attach (CLI or MCP working on a file).
    pub headless: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self { data_dir: None, config_dir: None, secrets: None, headless: true }
    }
}

pub struct Session {
    pub update: Mutex<crate::update::UpdateState>,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub headless: bool,
    secrets: Arc<dyn SecretStore>,
    doc: Mutex<Option<OpenDoc>>,
    settings: RwLock<Settings>,
    events: broadcast::Sender<Event>,
    ui: Mutex<Option<mpsc::UnboundedSender<UiCall>>>,
    ui_state: RwLock<UiState>,
    agent: RwLock<Option<Arc<dyn AgentHost>>>,
    seq: AtomicU64,
    runtime: tokio::runtime::Handle,
    pub(crate) bridge_port: Mutex<Option<u16>>,
    /// Held by a running `file.batch`; other changes wait for it (briefly) before starting.
    pub(crate) batch_lock: Arc<tokio::sync::Mutex<()>>,
    save_scheduled: AtomicBool,
    pub plugins: crate::plugins::Host,
    /// Itself, for background work that outlives a call (saving a moment later).
    me: std::sync::Weak<Session>,
}

impl Session {
    /// Must be called inside a Tokio runtime.
    pub fn new(opts: SessionOptions) -> std::io::Result<Arc<Self>> {
        let data_dir = opts.data_dir.unwrap_or_else(default_data_dir);
        let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&config_dir)?;
        let settings = Settings::load(&config_dir);
        let (events, _) = broadcast::channel(1024);
        let session = Arc::new_cyclic(|me| Self {
            data_dir,
            config_dir,
            headless: opts.headless,
            update: Mutex::new(crate::update::UpdateState::default()),
            secrets: opts.secrets.unwrap_or_else(|| Arc::new(MemorySecrets::default())),
            doc: Mutex::new(None),
            settings: RwLock::new(settings),
            events,
            ui: Mutex::new(None),
            ui_state: RwLock::new(UiState { zoom: 1.0, ..Default::default() }),
            agent: RwLock::new(None),
            seq: AtomicU64::new(1),
            runtime: tokio::runtime::Handle::current(),
            bridge_port: Mutex::new(None),
            batch_lock: Arc::new(tokio::sync::Mutex::new(())),
            save_scheduled: AtomicBool::new(false),
            plugins: crate::plugins::Host::new(),
            me: me.clone(),
        });
        session.plugins.load_all(&session);
        Ok(session)
    }

    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }

    // ---- events ---------------------------------------------------------

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    pub fn toast(&self, kind: ToastKind, text: impl Into<String>) {
        self.emit(Event::Toast { kind, text: text.into() });
    }

    pub(crate) fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    // ---- settings -------------------------------------------------------

    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> CmdResult<Settings> {
        let s = {
            let mut s = self.settings.write();
            f(&mut s);
            s.clone()
        };
        s.save(&self.config_dir).map_err(|e| format!("Couldn't save settings in {}: {e}", self.config_dir.display()))?;
        self.emit(Event::SettingsChanged);
        Ok(s)
    }

    /// A stored API key by id (`anthropic`, `openai`…).
    pub fn secret(&self, id: &str) -> Option<String> {
        self.secrets.get(id).filter(|k| !k.trim().is_empty())
    }

    /// Stores (`Some`) or removes (`None`) a key. The person's own action, never an agent's.
    pub fn set_secret(&self, id: &str, key: Option<&str>) -> CmdResult<()> {
        match key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(k) => self.secrets.set(id, k),
            None => self.secrets.delete(id),
        }
    }

    // ---- the window -----------------------------------------------------

    /// Called by the window once: commands that need it arrive on the returned channel.
    pub fn attach_ui(&self) -> mpsc::UnboundedReceiver<UiCall> {
        let (tx, rx) = mpsc::unbounded();
        *self.ui.lock() = Some(tx);
        rx
    }

    pub fn has_ui(&self) -> bool {
        self.ui.lock().as_ref().is_some_and(|tx| !tx.is_closed())
    }

    /// Hands a command to the window and waits for its answer.
    pub async fn ui_call(&self, command: &str, params: Value) -> CmdResult {
        let tx = self.ui.lock().clone().filter(|tx| !tx.is_closed()).ok_or_else(|| format!("`{command}` needs the folio window. Start the app and use folio-cli without --file, or folio-mcp --live."))?;
        let (reply, rx) = oneshot::channel();
        tx.unbounded_send(UiCall { command: command.into(), params, reply }).map_err(|_| "the folio window has closed".to_string())?;
        rx.await.map_err(|_| "the folio window didn't answer".to_string())?
    }

    pub fn set_ui_state(&self, state: UiState) {
        *self.ui_state.write() = state;
    }

    pub fn update_ui_state(&self, f: impl FnOnce(&mut UiState)) {
        f(&mut self.ui_state.write());
    }

    pub fn ui_state(&self) -> UiState {
        self.ui_state.read().clone()
    }

    // ---- the built-in agent ---------------------------------------------

    pub fn set_agent_host(&self, host: Arc<dyn AgentHost>) {
        *self.agent.write() = Some(host);
    }

    pub fn agent_host(&self) -> Option<Arc<dyn AgentHost>> {
        self.agent.read().clone()
    }

    // ---- the open file --------------------------------------------------

    pub fn is_open(&self) -> bool {
        self.doc.lock().is_some()
    }

    pub fn current_id(&self) -> Option<Id> {
        self.doc.lock().as_ref().map(|d| d.editor.doc().id.clone())
    }

    /// Where the open file is, whether it is still untitled, and where it came from.
    pub fn location(&self) -> Option<(PathBuf, bool, Option<(PathBuf, String)>)> {
        self.doc.lock().as_ref().map(|d| (d.path.clone(), d.untitled, d.origin.clone()))
    }

    /// A copy of the open document (cheap: pages are shared).
    pub fn doc(&self) -> CmdResult<Document> {
        self.read(|ed| ed.doc().clone())
    }

    /// The open document and its version, at once.
    pub fn snapshot(&self) -> Option<(Document, u64)> {
        self.doc.lock().as_ref().map(|d| (d.editor.doc().clone(), d.editor.version()))
    }

    pub fn read<R>(&self, f: impl FnOnce(&Editor) -> R) -> CmdResult<R> {
        let guard = self.doc.lock();
        let doc = guard.as_ref().ok_or(NO_FILE)?;
        Ok(f(&doc.editor))
    }

    /// Changes the open document (one undo step unless `coalesce` folds it into the last one),
    /// then saves it a moment later and tells every client.
    pub fn edit<R>(&self, label: &str, source: Source, coalesce: Option<&str>, f: impl FnOnce(&mut Document) -> folio_core::Result<R>) -> CmdResult<R> {
        let mut guard = self.doc.lock();
        let doc = guard.as_mut().ok_or(NO_FILE)?;
        if doc.editor.in_batch() && !in_batch_scope() && source != Source::Window {
            return Err("An agent's batch of changes is running; try again when it ends.".into());
        }
        let r = doc.editor.edit(label, source.as_str(), coalesce, f).map_err(|e| e.0)?;
        let version = doc.editor.version();
        drop(guard);
        self.changed(version);
        Ok(r)
    }

    /// Runs `f` on the editor itself (undo, redo, batches, registering functions).
    pub fn with_editor<R>(&self, f: impl FnOnce(&mut Editor) -> R) -> CmdResult<R> {
        let mut guard = self.doc.lock();
        let doc = guard.as_mut().ok_or(NO_FILE)?;
        let before = doc.editor.version();
        let r = f(&mut doc.editor);
        let version = doc.editor.version();
        drop(guard);
        if version != before {
            self.changed(version);
        }
        Ok(r)
    }

    fn changed(&self, version: u64) {
        self.emit(Event::DocChanged { version });
        self.schedule_save();
    }

    /// Remembers the history position of the open file, for reverting an agent's run.
    pub fn checkpoint(&self) -> Option<(Id, u64)> {
        let guard = self.doc.lock();
        let d = guard.as_ref()?;
        Some((d.editor.doc().id.clone(), d.editor.checkpoint()))
    }

    /// Opens a document (saving and closing the one open).
    pub fn open_doc(&self, doc: Document, path: PathBuf, untitled: bool, origin: Option<(PathBuf, String)>) {
        self.flush();
        let id = doc.id.to_string();
        let mut editor = Editor::new(doc);
        self.plugins.register_functions(editor.calc_mut().engine_mut());
        editor.recalc_all();
        let saved = if untitled || !path.exists() { u64::MAX } else { editor.version() };
        *self.doc.lock() = Some(OpenDoc { editor, path: path.clone(), untitled, origin, saved });
        if !untitled {
            crate::recent::add(&self.config_dir, &path);
        }
        self.emit(Event::DocSwitched { id: Some(id) });
        if saved == u64::MAX {
            self.schedule_save();
        }
    }

    pub fn close_doc(&self) {
        self.flush();
        let was_open = self.doc.lock().take().is_some();
        if was_open {
            self.emit(Event::DocSwitched { id: None });
        }
    }

    /// Where a new untitled file goes.
    pub fn untitled_path(&self, id: &Id) -> PathBuf {
        self.data_dir.join("untitled").join(format!("{id}.folio"))
    }

    /// Moves the open file to `path` (Save as): it saves there from now on.
    pub fn move_to(&self, path: &Path) -> CmdResult<()> {
        let (doc, old, untitled) = {
            let g = self.doc.lock();
            let d = g.as_ref().ok_or(NO_FILE)?;
            (d.editor.doc().clone(), d.path.clone(), d.untitled)
        };
        folio_core::file::save(&doc, path).map_err(|e| e.0)?;
        {
            let mut g = self.doc.lock();
            if let Some(d) = g.as_mut() {
                d.path = path.to_path_buf();
                d.untitled = false;
                d.saved = d.editor.version();
                d.editor.mark_saved();
            }
        }
        if untitled && old != path {
            let _ = std::fs::remove_file(&old);
        }
        crate::recent::add(&self.config_dir, path);
        self.emit(Event::Saved { path: path.display().to_string() });
        Ok(())
    }

    /// Writes the open file now if it changed since the last write.
    pub fn flush(&self) {
        let job = {
            let g = self.doc.lock();
            // A headless session (the CLI converting a file) keeps untitled files in memory.
            g.as_ref().filter(|d| d.saved != d.editor.version() && !(self.headless && d.untitled)).map(|d| (d.editor.doc().clone(), d.path.clone(), d.editor.version()))
        };
        let Some((doc, path, version)) = job else { return };
        match folio_core::file::save(&doc, &path) {
            Ok(()) => {
                let mut g = self.doc.lock();
                if let Some(d) = g.as_mut().filter(|d| d.path == path) {
                    d.saved = version;
                    if d.editor.version() == version {
                        d.editor.mark_saved();
                    }
                }
                drop(g);
                self.emit(Event::Saved { path: path.display().to_string() });
            }
            Err(e) => {
                tracing::warn!("couldn't save {}: {e}", path.display());
                self.toast(ToastKind::Error, format!("Couldn't save {}: {e}", path.display()));
            }
        }
    }

    /// Saves at once in a headless session, a moment later in the app (typing doesn't write
    /// the file on every key).
    fn schedule_save(&self) {
        if self.headless {
            self.flush();
            return;
        }
        if self.save_scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(this) = self.me.upgrade() else { return };
        self.runtime.spawn(async move {
            tokio::time::sleep(Duration::from_millis(700)).await;
            this.save_scheduled.store(false, Ordering::Release);
            let _ = tokio::task::spawn_blocking(move || this.flush()).await;
        });
    }

    /// Whether the open file has changes not yet on disk.
    pub fn unsaved(&self) -> bool {
        self.doc.lock().as_ref().is_some_and(|d| d.saved != d.editor.version())
    }

    pub fn bridge_port(&self) -> Option<u16> {
        *self.bridge_port.lock()
    }
}

pub const NO_FILE: &str = "No file is open. Open one with file.open or create one with file.new.";

pub fn default_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("FOLIO_DATA_DIR").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("folio")
}

pub fn default_config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("FOLIO_CONFIG_DIR").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("folio")
}
