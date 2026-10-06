//! The root view: the lsuite backdrop, the home screen or the editor, and everything that
//! floats above them (dialogs, the context menu, toasts). It also carries out the commands only
//! the window can (`ui.*`, presenting) for every client of the registry.

use std::sync::Arc;

use folio_control::{CmdResult, Session, UiCall};
use gpui::{AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable, MouseButton, Render, Subscription, Window, div, prelude::*, px};
use serde_json::{Value, json};

use crate::actions::*;
use crate::store::{Dialog, GlobalStore, Store, StoreEvent, StoreExt};
use crate::theme::{ActiveTheme, Theme, os_reduces_transparency, size as sz};
use crate::views;

pub const SUPPORT_URL: &str = "https://lsuite.xyz/folio/support";
pub const PAGE_URL: &str = "https://lsuite.xyz/folio";

pub fn init(session: Arc<Session>, cx: &mut App) {
    let settings = session.settings();
    let mode = Theme::mode_for(&settings.appearance.mode, cx.window_appearance());
    cx.set_global(Theme::new(mode, settings.appearance.transparency && !os_reduces_transparency()));
    cx.set_reduce_motion(crate::theme::os_reduces_motion());
    // The built-in agent answers `agent.*` for every client; the Agent panel draws it.
    let agent = folio_agent::Host::install(&session);
    let store = cx.new(|cx| Store::new(session, Some(agent), cx));
    cx.set_global(GlobalStore(store));
    crate::actions::bind(cx);
    cx.set_menus(crate::actions::menus());
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

/// What happens once the window is up: the first-run setup, or a file given at start.
pub fn start(session: &Arc<Session>, open: Option<std::path::PathBuf>, cx: &mut App) {
    let store = cx.store();
    let first_run = !session.settings().onboarding.is_done() && std::env::var("FOLIO_NO_SETUP").is_err();
    store.update(cx, |s, cx| {
        if first_run {
            s.setup = true;
            cx.notify();
        }
        if let Some(p) = open {
            s.run("file.open", json!({ "path": p }), cx);
        }
    });
}

/// Re-reads the appearance setting (and the OS) into the theme.
pub fn apply_theme_setting(cx: &mut App) {
    let settings = cx.store().read(cx).settings.clone();
    let mode = Theme::mode_for(&settings.appearance.mode, cx.window_appearance());
    let transparent = settings.appearance.transparency && !os_reduces_transparency();
    let t = cx.global::<Theme>();
    if t.mode != mode || t.transparent != transparent {
        cx.set_global(Theme::new(mode, transparent));
        cx.refresh_windows();
    }
}

pub struct Workspace {
    store: Entity<Store>,
    focus: FocusHandle,
    home: Entity<views::home::Home>,
    editor: Entity<views::editor::Editor>,
    dialogs: Entity<views::dialogs::Dialogs>,
    setup: Entity<views::onboarding::Onboarding>,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let home = cx.new(|cx| views::home::Home::new(window, cx));
        let editor = cx.new(|cx| views::editor::Editor::new(window, cx));
        let dialogs = cx.new(|cx| views::dialogs::Dialogs::new(window, cx));
        let setup = cx.new(|cx| views::onboarding::Onboarding::new(window, cx));
        let mut subs = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        subs.push(cx.observe_window_appearance(window, |_, _, cx| apply_theme_setting(cx)));
        subs.push(cx.subscribe_in(&store, window, |ws: &mut Self, _, e: &StoreEvent, window, cx| {
            if let StoreEvent::SetupClosed = e {
                window.focus(&ws.focus, cx);
            }
        }));

        // Commands only the window can do, from every client of the registry.
        let mut calls = store.read(cx).session.attach_ui();
        cx.spawn_in(window, async move |this, cx| {
            use futures::StreamExt;
            while let Some(call) = calls.next().await {
                let UiCall { command, params, reply } = call;
                let result = this.update_in(cx, |ws, window, cx| ws.ui_command(&command, params, window, cx)).unwrap_or_else(|_| Err("the window has closed".into()));
                let _ = reply.send(result);
            }
        })
        .detach();
        store.update(cx, |s, _| s.sync_ui());
        Self { store, focus, home, editor, dialogs, setup, _subs: subs }
    }

    /// `ui.*`, `deck.present`, `app.quit`.
    fn ui_command(&mut self, command: &str, params: Value, window: &mut Window, cx: &mut Context<Self>) -> CmdResult {
        let store = self.store.clone();
        match command {
            "ui.show" => {
                let doc = store.read(cx).doc.clone().ok_or(folio_control::session::NO_FILE)?;
                let pi = match params.get("page").and_then(Value::as_str) {
                    Some(p) => doc.page_index(p).ok_or_else(|| format!("No page \"{p}\"."))?,
                    None => store.read(cx).page_index().unwrap_or(0),
                };
                let id = doc.pages[pi].id.clone();
                store.update(cx, |s, cx| {
                    s.show_page(id.clone(), cx);
                    let v = s.view_mut();
                    if let Some(sl) = params.get("slide") {
                        let key = sl.as_str().map(str::to_string).unwrap_or_else(|| sl.to_string());
                        if let Some(i) = doc.pages[pi].deck().and_then(|d| d.slide(&key)) {
                            v.slide = i;
                            v.shapes.clear();
                        }
                    }
                    if let Some(c) = params.get("cell").and_then(Value::as_str).and_then(folio_calc::Addr::parse) {
                        v.cell = c;
                        v.anchor = c;
                    }
                    if let Some(b) = params.get("block").and_then(Value::as_u64) {
                        v.text = Some(crate::store::TextSel::caret(crate::store::TextTarget::Doc, folio_core::Pos::new(b as usize, 0)));
                    }
                    s.sync_ui();
                    cx.notify();
                });
                self.editor.update(cx, |e, cx| e.reveal(window, cx));
                Ok(json!({ "page": doc.pages[pi].name }))
            }
            "ui.select" => {
                store.update(cx, |s, cx| {
                    let v = s.view_mut();
                    if let Some(r) = params.get("range").and_then(Value::as_str).and_then(folio_calc::Range::parse) {
                        v.anchor = r.start;
                        v.cell = r.end;
                    }
                    if let Some(list) = params.get("shapes").and_then(Value::as_array) {
                        v.shapes = list.iter().filter_map(|x| x.as_str()).map(folio_core::Id::from).collect();
                    }
                    if let (Some(a), Some(b)) = (params.get("from"), params.get("to")) {
                        let pos = |x: &Value| folio_core::Pos::new(x["block"].as_u64().unwrap_or(0) as usize, x["offset"].as_u64().unwrap_or(0) as usize);
                        v.text = Some(crate::store::TextSel { target: crate::store::TextTarget::Doc, anchor: pos(a), focus: pos(b) });
                    }
                    s.sync_ui();
                    cx.notify();
                });
                Ok(json!(store.read(cx).session.ui_state()))
            }
            "ui.panel" => {
                let name = params["name"].as_str().unwrap_or("").to_string();
                let open = params["open"].as_bool().unwrap_or(true);
                store.update(cx, |s, cx| {
                    match name.as_str() {
                        "agent" => s.agent_open = open,
                        "inspector" => s.inspector_open = open,
                        "pages" | "sidebar" => s.sidebar_open = open,
                        "onboarding" | "setup" => s.setup = open,
                        "home" => {
                            if open {
                                s.run("file.close", json!({}), cx);
                            }
                        }
                        other => {
                            let d = match other {
                                "settings" => Some(Dialog::Settings { section: None }),
                                "export" => Some(Dialog::Export),
                                "open" => Some(Dialog::Open),
                                "plugins" => Some(Dialog::Plugins),
                                "account" => Some(Dialog::Account),
                                "palette" => Some(Dialog::Palette),
                                "shortcuts" => Some(Dialog::Shortcuts),
                                "pageSetup" => Some(Dialog::PageSetup),
                                _ => None,
                            };
                            match (d, open) {
                                (Some(d), true) => s.open_dialog(d, cx),
                                (Some(_), false) => s.close_dialog(cx),
                                (None, _) => {}
                            }
                        }
                    }
                    s.sync_ui();
                    cx.notify();
                });
                Ok(json!({ "panel": name, "open": open }))
            }
            "ui.zoom" => {
                let z = params["level"].as_f64().unwrap_or(1.0).clamp(0.25, 4.0) as f32;
                store.update(cx, |s, cx| {
                    s.zoom = z;
                    s.sync_ui();
                    cx.notify();
                });
                Ok(json!({ "zoom": z }))
            }
            "ui.screenshot" => Err("Screenshots from the window aren't available yet; on Linux use vscreen.".into()),
            "deck.present" => {
                let page = folio_core::Id::from(params["page"].as_str().unwrap_or(""));
                let slide = params["slide"].as_u64().unwrap_or(0) as usize;
                let presenter = params["presenter"].as_bool().unwrap_or(false);
                store.update(cx, |s, cx| {
                    s.presenting = Some((page, slide, presenter));
                    s.sync_ui();
                    cx.notify();
                });
                window.toggle_fullscreen();
                Ok(json!({ "presenting": true, "slide": slide + 1 }))
            }
            "app.quit" => {
                cx.quit();
                Ok(json!({ "quitting": true }))
            }
            other => Err(format!("The window doesn't handle `{other}`.")),
        }
    }

    // ---- app-wide actions --------------------------------------------------

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            if let Ok(v) = s.run_now("history.undo", json!({}), cx)
                && let Some(step) = v["step"].as_str()
            {
                s.flash(format!("Undid {}", step_label(step)), cx);
            }
        });
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            if let Ok(v) = s.run_now("history.redo", json!({}), cx)
                && let Some(step) = v["step"].as_str()
            {
                s.flash(format!("Redid {}", step_label(step)), cx);
            }
        });
    }

    fn new_file(&mut self, _: &NewFile, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.run("file.close", json!({}), cx));
    }

    fn open_file(&mut self, _: &OpenFile, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::Open, cx));
    }

    fn close_file(&mut self, _: &CloseFile, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.run("file.close", json!({}), cx));
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let untitled = self.store.read(cx).untitled;
        if untitled {
            self.save_as(&SaveAs, window, cx);
            return;
        }
        self.store.update(cx, |s, cx| s.run_then("file.save", json!({}), cx, |s, _, cx| s.flash("Saved", cx)));
    }

    fn save_as(&mut self, _: &SaveAs, _: &mut Window, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let (dir, name) = {
            let s = store.read(cx);
            let dir = s.path.as_ref().filter(|_| !s.untitled).and_then(|p| p.parent().map(|d| d.to_path_buf())).or_else(dirs::document_dir).unwrap_or_else(|| std::path::PathBuf::from("."));
            (dir, format!("{}.folio", s.title()))
        };
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                store.update(cx, |s, cx| s.run_then("file.saveAs", json!({ "path": path }), cx, |s, v, cx| s.flash(format!("Saved as {}", v["path"].as_str().unwrap_or("")), cx)));
            }
        })
        .detach();
    }

    fn export(&mut self, _: &Export, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::Export, cx));
    }

    fn print(&mut self, _: &Print, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::Export, cx));
    }

    fn palette(&mut self, _: &Palette, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::Palette, cx));
    }

    fn shortcuts(&mut self, _: &ShowShortcuts, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::Shortcuts, cx));
    }

    fn settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::Settings { section: None }, cx));
    }

    fn plugins(&mut self, _: &OpenPlugins, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::Plugins, cx));
    }

    fn account(&mut self, _: &OpenAccount, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::Account, cx));
    }

    fn about(&mut self, _: &About, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.open_dialog(Dialog::About, cx));
    }

    fn toggle_agent(&mut self, _: &ToggleAgent, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            s.agent_open = !s.agent_open;
            s.sync_ui();
            cx.notify();
        });
    }

    fn toggle_inspector(&mut self, _: &ToggleInspector, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            s.inspector_open = !s.inspector_open;
            s.sync_ui();
            cx.notify();
        });
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            s.sidebar_open = !s.sidebar_open;
            s.sync_ui();
            cx.notify();
        });
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, _: &mut Window, cx: &mut Context<Self>) {
        let dark = cx.theme().is_dark();
        self.store.update(cx, |s, cx| s.run("ui.theme", json!({ "mode": if dark { "light" } else { "dark" } }), cx));
    }

    fn zoom(&mut self, by: f32, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            s.zoom = if by == 0.0 { 1.0 } else { (s.zoom * by).clamp(0.25, 4.0) };
            s.flash(format!("Zoom {:.0} %", s.zoom * 100.0), cx);
            s.sync_ui();
            cx.notify();
        });
    }

    fn present(&mut self, _: &Present, window: &mut Window, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let target = {
            let s = store.read(cx);
            let doc = s.doc.as_ref();
            let shown = s.page_index().filter(|i| doc.is_some_and(|d| d.pages[*i].kind() == folio_core::PageKind::Deck));
            let any = doc.and_then(|d| d.pages.iter().position(|p| p.kind() == folio_core::PageKind::Deck));
            shown.or(any).map(|i| (doc.unwrap().pages[i].id.clone(), if shown.is_some() { s.view().slide } else { 0 }))
        };
        match target {
            Some((page, slide)) => {
                store.update(cx, |s, cx| {
                    s.presenting = Some((page, slide, false));
                    s.sync_ui();
                    cx.notify();
                });
                window.toggle_fullscreen();
            }
            None => store.update(cx, |s, cx| s.flash("Nothing to present: add a deck page first.", cx)),
        }
    }

    fn new_page(&mut self, kind: &str, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            s.run_then("page.add", json!({ "kind": kind }), cx, |s, v, cx| {
                if let Some(id) = v["id"].as_str() {
                    s.show_page(folio_core::Id::from(id), cx);
                }
            })
        });
    }

    fn step_page(&mut self, by: i64, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            let Some(doc) = &s.doc else { return };
            let n = doc.pages.len() as i64;
            if n == 0 {
                return;
            }
            let i = s.page_index().unwrap_or(0) as i64;
            let next = ((i + by) % n + n) % n;
            let id = doc.pages[next as usize].id.clone();
            s.show_page(id, cx);
        });
    }

    fn backdrop(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        crate::ui::grain::backdrop(cx.theme().bg, window, cx)
    }
}

/// "doc.write" → "writing", for "Undid writing".
pub fn step_label(command: &str) -> String {
    let verb = command.split('.').nth(1).unwrap_or(command);
    let mut out = String::new();
    for c in verb.chars() {
        if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    match out.as_str() {
        "insert" => "typing".into(),
        "delete" => "deleting".into(),
        "set" => "a cell".into(),
        other => other.to_string(),
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let store = self.store.read(cx);
        let has_doc = store.doc.is_some();
        let modal = store.dialog.is_some() || store.setup;
        let setup = store.setup;
        let presenting = store.presenting.is_some();
        let title = if has_doc { format!("{} — folio", store.title()) } else { "folio".into() };
        window.set_window_title(&title);
        let menu = store.menu.clone();
        let toasts = store.toasts.clone();

        div()
            .key_context(if presenting { "Workspace Presenting" } else if modal { "Workspace Modal" } else { "Workspace" })
            .on_action(cx.listener(|_, _: &Right, w, cx| views::present::step(1, w, cx)))
            .on_action(cx.listener(|_, _: &Left, w, cx| views::present::step(-1, w, cx)))
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::new_file))
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::close_file))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::export))
            .on_action(cx.listener(Self::print))
            .on_action(cx.listener(Self::palette))
            .on_action(cx.listener(Self::shortcuts))
            .on_action(cx.listener(Self::settings))
            .on_action(cx.listener(Self::plugins))
            .on_action(cx.listener(Self::account))
            .on_action(cx.listener(Self::about))
            .on_action(cx.listener(Self::toggle_agent))
            .on_action(cx.listener(Self::toggle_inspector))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(Self::present))
            .on_action(cx.listener(|ws, _: &Escape, window, cx| {
                ws.store.update(cx, |s, cx| {
                    if s.menu.is_some() {
                        s.close_menu(cx);
                    } else if s.dialog.is_some() {
                        s.close_dialog(cx);
                    }
                });
                if ws.store.read(cx).presenting.is_some() {
                    views::present::end(window, cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &ZoomIn, _, cx| ws.zoom(1.2, cx)))
            .on_action(cx.listener(|ws, _: &ZoomOut, _, cx| ws.zoom(1.0 / 1.2, cx)))
            .on_action(cx.listener(|ws, _: &ZoomReset, _, cx| ws.zoom(0.0, cx)))
            .on_action(cx.listener(|ws, _: &NewDocPage, _, cx| ws.new_page("doc", cx)))
            .on_action(cx.listener(|ws, _: &NewSheetPage, _, cx| ws.new_page("sheet", cx)))
            .on_action(cx.listener(|ws, _: &NewDeckPage, _, cx| ws.new_page("deck", cx)))
            .on_action(cx.listener(|ws, _: &NextPage, _, cx| ws.step_page(1, cx)))
            .on_action(cx.listener(|ws, _: &PrevPage, _, cx| ws.step_page(-1, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|ws, _, _, cx| ws.store.update(cx, |s, cx| s.close_menu(cx))))
            .on_drop(cx.listener(|ws, paths: &gpui::ExternalPaths, _, cx| {
                // Pictures dropped on a document or a deck go in; any other file opens.
                for p in paths.paths() {
                    let is_image = p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"));
                    let kind = ws.store.read(cx).page_kind();
                    ws.store.update(cx, |s, cx| match (is_image, kind) {
                        (true, Some(folio_core::PageKind::Doc)) => s.run("doc.insertImage", json!({ "path": p }), cx),
                        (true, Some(folio_core::PageKind::Deck)) => s.run("deck.addImage", json!({ "path": p }), cx),
                        _ => s.run("file.open", json!({ "path": p }), cx),
                    });
                }
            }))
            .relative()
            .size_full()
            .font_family(crate::theme::SANS)
            .text_size(px(sz::BASE))
            .text_color(t.text)
            .child(self.backdrop(window, cx))
            .child(if presenting {
                views::present::present(window, cx)
            } else if has_doc {
                self.editor.clone().into_any_element()
            } else {
                self.home.clone().into_any_element()
            })
            .when(setup && !presenting, |d| d.child(self.setup.clone()))
            .when(!presenting, |d| d.child(self.dialogs.clone()))
            .when_some(menu, |d, m| d.child(views::overlays::context_menu(m, window, cx)))
            .when(!toasts.is_empty(), |d| d.child(views::overlays::toasts(toasts, cx)))
    }
}
