//! The window's view of the session.
//!
//! The window is one client of the command registry, never a privileged one: every change goes
//! through [`Store::run`] (or [`Store::run_now`] for typing and other quick edits that must show
//! in the same frame), and the document shown is whatever the session holds after the change.
//! The store adds view state (the page shown, the caret and selections, zoom, panels, dialogs)
//! and mirrors what the session announces on its event stream.

use std::collections::HashMap;
use std::sync::Arc;

use folio_calc::Addr;
use folio_control::{CmdResult, CommandRecord, Event, Session, Settings, Source, ToastKind, UiState};
use folio_core::{Document, Id, PageKind, Pos};
use gpui::{App, Context, Entity, EventEmitter, Global, Pixels, Point, SharedString, Task};
use serde_json::{Value, json};

/// Dialogs over the window (one at a time).
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Settings { section: Option<String> },
    Export,
    Open,
    Plugins,
    Account,
    Palette,
    Shortcuts,
    /// Insert a table / chart / link: what to insert and on which page.
    Insert { what: String },
    PageSetup,
    About,
}

impl Dialog {
    pub fn name(&self) -> &'static str {
        match self {
            Dialog::Settings { .. } => "settings",
            Dialog::Export => "export",
            Dialog::Open => "open",
            Dialog::Plugins => "plugins",
            Dialog::Account => "account",
            Dialog::Palette => "palette",
            Dialog::Shortcuts => "shortcuts",
            Dialog::Insert { .. } => "insert",
            Dialog::PageSetup => "pageSetup",
            Dialog::About => "about",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub text: SharedString,
    /// A passing status ("Undid typing"): the next one replaces it.
    pub flash: bool,
}

/// Where the text caret is: a document page, or a text box on a slide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextTarget {
    Doc,
    Shape { slide: usize, shape: usize },
}

/// A text selection: anchor and focus (the caret).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSel {
    pub target: TextTarget,
    pub anchor: Pos,
    pub focus: Pos,
}

impl TextSel {
    pub fn caret(target: TextTarget, p: Pos) -> Self {
        TextSel { target, anchor: p, focus: p }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }

    pub fn ordered(&self) -> (Pos, Pos) {
        (self.anchor.min(self.focus), self.anchor.max(self.focus))
    }
}

/// What is selected on one page (kept per page, so switching pages and back finds it again).
#[derive(Clone, Debug)]
pub struct PageView {
    /// Documents, and text boxes being edited on slides.
    pub text: Option<TextSel>,
    /// Sheets: the active cell and the other corner of the selected range.
    pub cell: Addr,
    pub anchor: Addr,
    /// Sheets: the top-left visible cell (scrolling).
    pub scroll: Addr,
    /// Decks: the slide shown and the selected shapes.
    pub slide: usize,
    pub shapes: Vec<Id>,
    /// Documents: vertical scroll in points.
    pub scroll_y: f32,
}

impl Default for PageView {
    fn default() -> Self {
        PageView { text: None, cell: Addr::new(0, 0), anchor: Addr::new(0, 0), scroll: Addr::new(0, 0), slide: 0, shapes: vec![], scroll_y: 0.0 }
    }
}

impl PageView {
    pub fn range(&self) -> folio_calc::Range {
        folio_calc::Range {
            start: Addr::new(self.cell.row.min(self.anchor.row), self.cell.col.min(self.anchor.col)),
            end: Addr::new(self.cell.row.max(self.anchor.row), self.cell.col.max(self.anchor.col)),
        }
    }
}

/// One entry of a context menu.
#[derive(Clone)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
}

pub type MenuAction = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut App)>;

#[derive(Clone)]
pub struct MenuItem {
    pub label: SharedString,
    pub icon: Option<&'static str>,
    pub shortcut: Option<SharedString>,
    pub danger: bool,
    pub disabled: bool,
    pub action: MenuAction,
}

impl MenuItem {
    pub fn new(label: impl Into<SharedString>, action: impl Fn(&mut gpui::Window, &mut App) + 'static) -> Self {
        Self { label: label.into(), icon: None, shortcut: None, danger: false, disabled: false, action: std::rc::Rc::new(action) }
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn shortcut(mut self, s: impl Into<SharedString>) -> Self {
        self.shortcut = Some(s.into());
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn entry(self) -> MenuEntry {
        MenuEntry::Item(self)
    }
}

#[derive(Clone)]
pub struct ContextMenu {
    pub position: Point<Pixels>,
    pub entries: Vec<MenuEntry>,
}

#[derive(Clone, Debug)]
pub enum StoreEvent {
    /// The document changed (any client): views relayout.
    DocChanged,
    /// Another file was opened, or it was closed.
    Switched,
    /// The first-run setup closed: the workspace takes the keyboard again.
    SetupClosed,
    /// Start editing the active cell (with this text, or its content).
    EditCell(Option<String>),
}

pub struct Store {
    pub session: Arc<Session>,
    pub agent: Option<Arc<folio_agent::Host>>,
    pub doc: Option<Document>,
    pub version: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Where the file is: path, still untitled, imported from.
    pub path: Option<std::path::PathBuf>,
    pub untitled: bool,
    pub saved: bool,
    pub settings: Settings,
    /// The page shown (id).
    pub page: Option<Id>,
    pub views: HashMap<Id, PageView>,
    /// Zoom of the work area, 1 = 100 %.
    pub zoom: f32,
    pub sidebar_open: bool,
    pub inspector_open: bool,
    pub agent_open: bool,
    pub dialog: Option<Dialog>,
    /// The first-run setup, over the whole window.
    pub setup: bool,
    pub menu: Option<ContextMenu>,
    pub toasts: Vec<Toast>,
    /// Presenting a deck: (page, slide, presenter view).
    pub presenting: Option<(Id, usize, bool)>,
    /// Commands from the agent, MCP and the CLI (the agent panel's cards).
    pub commands: Vec<CommandRecord>,
    /// lsuite AI, as `account.status` last answered.
    pub account: Value,
    /// Recent files (`file.recent`).
    pub recent: Vec<Value>,
    next_toast: u64,
    _pump: Task<()>,
}

impl EventEmitter<StoreEvent> for Store {}

/// The store, reachable from anywhere (`cx.store()`).
pub struct GlobalStore(pub Entity<Store>);
impl Global for GlobalStore {}

pub trait StoreExt {
    fn store(&self) -> Entity<Store>;
}

impl StoreExt for App {
    fn store(&self) -> Entity<Store> {
        self.global::<GlobalStore>().0.clone()
    }
}

impl Store {
    pub fn new(session: Arc<Session>, agent: Option<Arc<folio_agent::Host>>, cx: &mut Context<Self>) -> Self {
        let mut rx = session.subscribe();
        let pump = cx.spawn(async move |this, cx| {
            loop {
                let event = match rx.recv().await {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if this.update(cx, |s, cx| s.refresh(cx)).is_err() {
                            break;
                        }
                        continue;
                    }
                    Err(_) => break,
                };
                if this.update(cx, |s, cx| s.on_event(event, cx)).is_err() {
                    break;
                }
            }
        });
        let settings = session.settings();
        let mut s = Self {
            session,
            agent,
            doc: None,
            version: 0,
            can_undo: false,
            can_redo: false,
            path: None,
            untitled: false,
            saved: true,
            settings,
            page: None,
            views: HashMap::new(),
            zoom: 1.0,
            sidebar_open: true,
            inspector_open: true,
            agent_open: false,
            dialog: None,
            setup: false,
            menu: None,
            toasts: vec![],
            presenting: None,
            commands: vec![],
            account: json!({ "signedIn": false }),
            recent: vec![],
            next_toast: 1,
            _pump: pump,
        };
        s.pull();
        s.refresh_recent();
        s.refresh_account(cx);
        s
    }

    // ---- the document ------------------------------------------------------

    /// Takes the session's document if it changed.
    fn pull(&mut self) -> bool {
        match self.session.snapshot() {
            Some((doc, version)) => {
                let changed = self.doc.as_ref().is_none_or(|d| d.id != doc.id) || version != self.version;
                if changed {
                    let switched = self.doc.as_ref().is_none_or(|d| d.id != doc.id);
                    self.doc = Some(doc);
                    self.version = version;
                    if switched {
                        self.views.clear();
                        self.page = None;
                    }
                    self.fix_page();
                }
                let (can_undo, can_redo) = self.session.read(|ed| (ed.can_undo(), ed.can_redo())).unwrap_or((false, false));
                self.can_undo = can_undo;
                self.can_redo = can_redo;
                if let Some((path, untitled, _)) = self.session.location() {
                    self.path = Some(path);
                    self.untitled = untitled;
                }
                self.saved = !self.session.unsaved();
                changed
            }
            None => {
                let had = self.doc.is_some();
                self.doc = None;
                self.page = None;
                self.views.clear();
                self.path = None;
                had
            }
        }
    }

    /// Keeps the shown page valid (the first page when it went away).
    fn fix_page(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.page.as_ref().is_none_or(|p| doc.page_index(p.as_str()).is_none()) {
            self.page = doc.pages.first().map(|p| p.id.clone());
        }
        // Clamp selections that point past the end after other clients' edits.
        let ids: Vec<(Id, usize)> = doc.pages.iter().enumerate().map(|(i, p)| (p.id.clone(), i)).collect();
        for (id, i) in ids {
            let Some(v) = self.views.get_mut(&id) else { continue };
            let page = &doc.pages[i];
            if let (Some(t), Some(td)) = (&mut v.text, page.doc())
                && t.target == TextTarget::Doc
            {
                t.anchor = folio_core::text::clamp(&td.blocks, t.anchor);
                t.focus = folio_core::text::clamp(&td.blocks, t.focus);
            }
            if let Some(d) = page.deck() {
                v.slide = v.slide.min(d.slides.len().saturating_sub(1));
                let shapes: Vec<Id> = d.slides.get(v.slide).map(|s| s.shapes.iter().map(|x| x.id.clone()).collect()).unwrap_or_default();
                v.shapes.retain(|s| shapes.contains(s));
                if let Some(t) = &mut v.text
                    && let TextTarget::Shape { slide, shape } = t.target
                {
                    match d.slides.get(slide).and_then(|s| s.shapes.get(shape)) {
                        Some(sh) => {
                            t.anchor = folio_core::text::clamp(&sh.text, t.anchor);
                            t.focus = folio_core::text::clamp(&sh.text, t.focus);
                        }
                        None => v.text = None,
                    }
                }
            }
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let switched_before = self.doc.as_ref().map(|d| d.id.clone());
        if self.pull() {
            let now = self.doc.as_ref().map(|d| d.id.clone());
            cx.emit(if now != switched_before { StoreEvent::Switched } else { StoreEvent::DocChanged });
        }
        self.sync_ui();
        cx.notify();
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::DocChanged { .. } | Event::DocSwitched { .. } => {
                if matches!(event, Event::DocSwitched { .. }) {
                    self.refresh_recent();
                }
                self.refresh(cx);
            }
            Event::Saved { .. } => {
                self.saved = !self.session.unsaved();
                if let Some((path, untitled, _)) = self.session.location() {
                    self.path = Some(path);
                    self.untitled = untitled;
                }
                cx.notify();
            }
            Event::Toast { kind, text } => self.toast(kind, text, cx),
            Event::Command { record } => {
                if record.source != Source::Window {
                    self.commands.push(record);
                    if self.commands.len() > 500 {
                        self.commands.drain(..100);
                    }
                    cx.notify();
                }
            }
            Event::SettingsChanged => {
                self.settings = self.session.settings();
                crate::app::apply_theme_setting(cx);
                cx.notify();
            }
            Event::AccountChanged => self.refresh_account(cx),
            Event::PluginsChanged => cx.notify(),
        }
    }

    pub fn refresh_recent(&mut self) {
        let list = folio_control::recent::list(&self.session.config_dir);
        self.recent = list
            .into_iter()
            .filter(|p| p.exists())
            .take(12)
            .map(|p| json!({ "path": p, "name": p.file_name().map(|n| n.to_string_lossy().into_owned()), "modified": std::fs::metadata(&p).and_then(|m| m.modified()).ok().map(chrono::DateTime::<chrono::Utc>::from) }))
            .collect();
    }

    pub fn refresh_account(&mut self, cx: &mut Context<Self>) {
        let task = gpui_tokio::Tokio::spawn(cx, async move { folio_control::account::status(true).await });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(v)) = task.await {
                this.update(cx, |s, cx| {
                    s.account = v;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ---- running commands ------------------------------------------------

    /// Runs a command in the background; errors become toasts.
    pub fn run(&mut self, name: &str, params: Value, cx: &mut Context<Self>) {
        self.run_then(name, params, cx, |_, _, _| {});
    }

    /// Runs a command in the background and hands its result to `then` (errors become toasts).
    pub fn run_then(&mut self, name: &str, params: Value, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, Value, &mut Context<Self>) + 'static) {
        let session = self.session.clone();
        let name = name.to_string();
        let task = gpui_tokio::Tokio::spawn(cx, async move { folio_control::call(&session, Source::Window, &name, params).await });
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|e| Err(format!("the command stopped: {e}")));
            this.update(cx, |s, cx| {
                s.refresh(cx);
                match result {
                    Ok(v) => then(s, v, cx),
                    Err(e) => s.error(e, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Runs a quick edit now, on this thread (typing, cell entry, moving a shape): the window
    /// shows its result in the same frame. Errors become toasts and are returned.
    pub fn run_now(&mut self, name: &str, params: Value, cx: &mut Context<Self>) -> CmdResult {
        let _rt = self.session.runtime().enter();
        let result = futures::executor::block_on(folio_control::call(&self.session, Source::Window, name, params));
        if let Err(e) = &result {
            self.error(e.clone(), cx);
        }
        if self.pull() {
            cx.emit(StoreEvent::DocChanged);
        }
        self.sync_ui();
        cx.notify();
        result
    }

    // ---- view state --------------------------------------------------------

    pub fn page_index(&self) -> Option<usize> {
        let doc = self.doc.as_ref()?;
        self.page.as_ref().and_then(|p| doc.page_index(p.as_str()))
    }

    pub fn page_kind(&self) -> Option<PageKind> {
        let i = self.page_index()?;
        Some(self.doc.as_ref()?.pages[i].kind())
    }

    pub fn view(&self) -> PageView {
        self.page.as_ref().and_then(|p| self.views.get(p)).cloned().unwrap_or_default()
    }

    pub fn view_mut(&mut self) -> &mut PageView {
        let id = self.page.clone().unwrap_or_default();
        self.views.entry(id).or_default()
    }

    pub fn show_page(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.page.as_ref() != Some(&id) {
            self.page = Some(id);
            self.menu = None;
            self.session.with_editor(|ed| ed.break_coalesce()).ok();
            self.sync_ui();
            cx.notify();
        }
    }

    pub fn set_text_sel(&mut self, sel: Option<TextSel>, cx: &mut Context<Self>) {
        self.view_mut().text = sel;
        self.sync_ui();
        cx.notify();
    }

    /// Tells the session what the window shows (`ui.state`, and defaults for commands).
    pub fn sync_ui(&self) {
        let v = self.view();
        let kind = self.page_kind();
        let state = UiState {
            screen: if self.doc.is_some() { "editor".into() } else { "home".into() },
            page: self.page.as_ref().map(|p| p.to_string()),
            text: v.text.filter(|t| t.target == TextTarget::Doc && kind == Some(PageKind::Doc)).map(|t| folio_control::session::TextSelection { anchor: t.anchor, focus: t.focus }),
            cell: (kind == Some(PageKind::Sheet)).then(|| v.cell.a1()),
            range: (kind == Some(PageKind::Sheet)).then(|| v.range().a1()),
            slide: (kind == Some(PageKind::Deck)).then_some(v.slide),
            shapes: if kind == Some(PageKind::Deck) { v.shapes.iter().map(|s| s.to_string()).collect() } else { vec![] },
            zoom: self.zoom,
            theme: if crate::theme::is_dark_now() { "dark".into() } else { "light".into() },
            open: [
                self.agent_open.then_some("agent"),
                self.inspector_open.then_some("inspector"),
                self.sidebar_open.then_some("pages"),
                self.dialog.as_ref().map(|d| d.name()),
                self.setup.then_some("onboarding"),
            ]
            .into_iter()
            .flatten()
            .map(str::to_string)
            .collect(),
            presenting: self.presenting.is_some(),
            window: [0.0, 0.0],
        };
        self.session.set_ui_state(state);
    }

    // ---- toasts, dialogs ----------------------------------------------------

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push_toast(kind, text.into(), false, cx);
    }

    /// A passing status that replaces the previous one ("Undid typing").
    pub fn flash(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push_toast(ToastKind::Info, text.into(), true, cx);
    }

    pub fn error(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push_toast(ToastKind::Error, text.into(), false, cx);
    }

    fn push_toast(&mut self, kind: ToastKind, text: SharedString, flash: bool, cx: &mut Context<Self>) {
        if flash {
            self.toasts.retain(|t| !t.flash);
        }
        let id = self.next_toast;
        self.next_toast += 1;
        self.toasts.push(Toast { id, kind, text, flash });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        cx.notify();
        let ttl = if kind == ToastKind::Error { 6000 } else { 2600 };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(ttl)).await;
            this.update(cx, |s, cx| {
                s.toasts.retain(|t| t.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_dialog(&mut self, d: Dialog, cx: &mut Context<Self>) {
        self.menu = None;
        self.dialog = Some(d);
        self.sync_ui();
        cx.notify();
    }

    pub fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        self.sync_ui();
        cx.notify();
    }

    pub fn open_menu(&mut self, position: Point<Pixels>, entries: Vec<MenuEntry>, cx: &mut Context<Self>) {
        self.menu = Some(ContextMenu { position, entries });
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    pub fn close_setup(&mut self, cx: &mut Context<Self>) {
        self.setup = false;
        cx.emit(StoreEvent::SetupClosed);
        self.sync_ui();
        cx.notify();
    }

    /// The title shown: the file's title, with a dot when changes aren't on disk yet.
    pub fn title(&self) -> String {
        self.doc.as_ref().map(|d| d.title.clone()).unwrap_or_else(|| "folio".into())
    }
}
