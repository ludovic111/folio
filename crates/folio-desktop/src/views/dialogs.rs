//! Modal dialogs (tier-3 glass over the scrim, inside viewfinder brackets): export, open and
//! import, settings, plugins, the command palette, shortcuts, inserting tables,
//! charts, links and comments, page setup, about.

use folio_core::PageKind;
use gpui::{AnyElement, App, Context, Entity, FontWeight, MouseButton, Render, SharedString, Subscription, Window, deferred, div, prelude::*, px};
use serde_json::{Value, json};

use crate::actions::SHORTCUTS;
use crate::store::{Dialog, Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::{Button, GlassExt, caps, icon, kbd, logo, motion, segmented, switch};

pub struct Dialogs {
    store: Entity<Store>,
    /// Text fields reused by the dialogs (cleared when a dialog opens).
    a: Entity<TextInput>,
    b: Entity<TextInput>,
    palette: Entity<TextInput>,
    author: Entity<TextInput>,
    describe: Entity<TextInput>,
    /// What the open dialog loaded (plugin.list, plugin.toolchain…).
    data: Value,
    /// Choices inside the open dialog.
    export_format: String,
    export_scope_all: bool,
    insert_kind: String,
    insert_link: bool,
    insert_sheet: Option<String>,
    opened: Option<String>,
    _subs: Vec<Subscription>,
}

impl Dialogs {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let a = cx.new(TextInput::new);
        let b = cx.new(TextInput::new);
        let palette = cx.new(|cx| TextInput::new(cx).placeholder("Type a command…"));
        let author = cx.new(|cx| TextInput::new(cx).placeholder("Your name"));
        let describe = cx.new(|cx| TextInput::new(cx).multiline(3).placeholder("Describe the plugin you want, e.g. a function =VAT(amount, rate) that adds tax…"));
        let mut subs = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        subs.push(cx.subscribe_in(&palette, window, |d: &mut Self, _, e: &InputEvent, window, cx| match e {
            InputEvent::Submit => {
                let q = d.palette.read(cx).text().to_string();
                if let Some((_, action)) = palette_items(&q).into_iter().next() {
                    d.store.update(cx, |s, cx| s.close_dialog(cx));
                    window.dispatch_action(action, cx);
                }
            }
            InputEvent::Cancel => d.store.update(cx, |s, cx| s.close_dialog(cx)),
            _ => cx.notify(),
        }));
        subs.push(cx.subscribe_in(&author, window, |d: &mut Self, src, e: &InputEvent, _, cx| {
            if matches!(e, InputEvent::Submit | InputEvent::Blur) {
                let v = src.read(cx).text().to_string();
                d.store.update(cx, |s, cx| s.run("app.setSetting", json!({ "key": "editing.author", "value": v }), cx));
            }
        }));
        for input in [a.clone(), b.clone()] {
            subs.push(cx.subscribe_in(&input, window, |d: &mut Self, _, e: &InputEvent, window, cx| match e {
                InputEvent::Submit => d.submit(window, cx),
                InputEvent::Cancel => d.store.update(cx, |s, cx| s.close_dialog(cx)),
                _ => {}
            }));
        }
        Self {
            store,
            a,
            b,
            palette,
            author,
            describe,
            data: Value::Null,
            export_format: "pdf".into(),
            export_scope_all: true,
            insert_kind: "column".into(),
            insert_link: false,
            insert_sheet: None,
            opened: None,
            _subs: subs,
        }
    }

    /// When a dialog opens: reset fields, load what it shows.
    fn on_open(&mut self, d: &Dialog, window: &mut Window, cx: &mut Context<Self>) {
        self.data = Value::Null;
        for i in [&self.a, &self.b, &self.palette] {
            i.update(cx, |i, cx| i.set_text("", cx));
        }
        match d {
            Dialog::Palette => crate::ui::input::focus(&self.palette, window, cx),
            Dialog::Plugins => self.load("plugin.list", json!({}), cx),
            Dialog::Settings { .. } => {
                let author = self.store.read(cx).settings.editing.author.clone();
                self.author.update(cx, |i, cx| i.set_text(author, cx));
            }
            Dialog::Insert { what } => {
                self.insert_link = what == "chart";
                let first_sheet = self.store.read(cx).doc.as_ref().and_then(|d| d.pages.iter().find(|p| p.kind() == PageKind::Sheet).map(|p| p.name.clone()));
                self.insert_sheet = first_sheet;
                let (pa, pb) = match what.as_str() {
                    "table" => ("3", "3"),
                    "chart" => ("A1:B6", ""),
                    "link" => ("https://", ""),
                    _ => ("", ""),
                };
                self.a.update(cx, |i, cx| i.set_text(pa, cx));
                self.b.update(cx, |i, cx| i.set_text(pb, cx));
                if what == "chart"
                    && let Some(r) = self.sheet_used_range(cx)
                {
                    self.a.update(cx, |i, cx| i.set_text(r, cx));
                }
                crate::ui::input::focus(&self.a, window, cx);
            }
            _ => {}
        }
    }

    fn sheet_used_range(&self, cx: &App) -> Option<String> {
        let s = self.store.read(cx);
        let doc = s.doc.as_ref()?;
        let name = self.insert_sheet.as_ref()?;
        doc.pages.iter().find(|p| &p.name == name)?.sheet()?.used_range().map(|r| r.a1())
    }

    fn load(&mut self, cmd: &str, params: Value, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        let key = cmd.to_string();
        self.store.update(cx, |s, cx| {
            s.run_then(cmd, params, cx, move |_, v, cx| {
                me.update(cx, |d, cx| {
                    if !d.data.is_object() {
                        d.data = json!({});
                    }
                    d.data[key] = v;
                    cx.notify();
                })
                .ok();
            })
        });
    }

    fn close(&self, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.close_dialog(cx));
    }

    /// Enter in an insert dialog.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Insert { what }) = self.store.read(cx).dialog.clone() else { return };
        let a = self.a.read(cx).text().trim().to_string();
        let b = self.b.read(cx).text().trim().to_string();
        let kind = self.store.read(cx).page_kind();
        let sheet = self.insert_sheet.clone().unwrap_or_default();
        let link = format!("{}!{}", folio_calc::quote_sheet_name(&sheet), a);
        let slide = (self.store.read(cx).view().slide + 1).to_string();
        let sel = self.store.read(cx).view().text;
        let after = sel.map(|t| t.focus.block);
        let (cmd, params) = match (what.as_str(), kind) {
            ("table", Some(PageKind::Deck)) if self.insert_link => ("deck.addTable", json!({ "slide": slide, "link": link })),
            ("table", Some(PageKind::Deck)) => ("deck.addTable", json!({ "slide": slide, "rows": a.parse::<i64>().unwrap_or(3), "cols": b.parse::<i64>().unwrap_or(3) })),
            ("table", _) if self.insert_link => ("doc.insertTable", json!({ "link": link, "after": after })),
            ("table", _) => ("doc.insertTable", json!({ "rows": a.parse::<i64>().unwrap_or(3), "cols": b.parse::<i64>().unwrap_or(3), "after": after })),
            ("chart", Some(PageKind::Deck)) => ("deck.addChart", json!({ "slide": slide, "source": link, "kind": self.insert_kind, "title": b })),
            ("chart", _) => ("doc.insertChart", json!({ "source": link, "kind": self.insert_kind, "title": b, "after": after })),
            ("link", _) => {
                let Some(t) = sel.filter(|t| !t.is_empty()) else {
                    self.store.update(cx, |s, cx| s.error("Select the text to link first.", cx));
                    return;
                };
                let (x, y) = t.ordered();
                ("text.format", json!({ "from": { "block": x.block, "offset": x.offset }, "to": { "block": y.block, "offset": y.offset }, "link": a }))
            }
            ("comment", _) => {
                let Some(t) = sel.filter(|t| !t.is_empty()) else {
                    self.store.update(cx, |s, cx| s.error("Select the text to comment on first.", cx));
                    return;
                };
                let (x, y) = t.ordered();
                ("doc.comment", json!({ "from": { "block": x.block, "offset": x.offset }, "to": { "block": y.block, "offset": y.offset }, "text": a }))
            }
            _ => return,
        };
        let mut params = params;
        if params.get("after").is_some_and(Value::is_null) {
            params.as_object_mut().unwrap().remove("after");
        }
        self.store.update(cx, |s, cx| {
            s.close_dialog(cx);
            s.run(cmd, params, cx);
        });
        let _ = window;
    }

    // ---- the dialogs ---------------------------------------------------------------

    fn header(title: &str, sub: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .px(px(18.))
            .py(px(12.))
            .border_b_1()
            .border_color(t.line)
            .child(div().flex().items_baseline().gap(px(10.)).child(div().text_size(px(sz::LG)).font_weight(FontWeight::SEMIBOLD).child(title.to_string())).when_some(sub, |d, s| d.child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(s.to_uppercase()))))
            .child(Button::icon("close-dialog", "x", "Close (Esc)").on_click(cx.listener(|d, _, _, cx| d.close(cx))))
            .into_any_element()
    }

    fn export(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let page_name = s.page_index().and_then(|i| s.doc.as_ref().map(|d| d.pages[i].name.clone())).unwrap_or_default();
        let formats: [(&str, &str, &str, &str); 10] = [
            ("pdf", "PDF", "file-down", "Every page, as it prints"),
            ("docx", "Word (.docx)", "word", "Documents — opens in Word, Google Docs, Pages, LibreOffice"),
            ("xlsx", "Excel (.xlsx)", "excel", "Sheets with formulas, formats and charts"),
            ("pptx", "PowerPoint (.pptx)", "powerpoint", "Decks with their slides and notes"),
            ("odt", "OpenDocument text (.odt)", "libreoffice-writer", "Documents for LibreOffice"),
            ("ods", "OpenDocument spreadsheet (.ods)", "libreoffice-calc", "Sheets for LibreOffice"),
            ("odp", "OpenDocument presentation (.odp)", "libreoffice-impress", "Decks for LibreOffice"),
            ("csv", "CSV", "sheet", "One sheet's values"),
            ("md", "Markdown", "file-type", "Documents as plain text"),
            ("html", "Web page (.html)", "globe", "Every page, in one file"),
        ];
        let supported: Vec<String> = folio_io::formats().into_iter().filter(|f| f.export).map(|f| f.id.to_string()).collect();
        let chosen = self.export_format.clone();
        let header = Self::header("Export", Some("pdf · office · opendocument · web"), cx);
        let rows = formats.into_iter().filter(|(id, ..)| supported.iter().any(|s| s == id) || *id == "md").map(|(id, name, ic, line)| {
            let on = chosen == id;
            let is_logo = crate::ui::logos::logo_file(ic).is_some();
            div()
                .id(SharedString::from(format!("fmt-{id}")))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(10.))
                .py(px(7.))
                .cursor_pointer()
                .when(on, |d| d.bg(t.accent).text_color(t.text_on_accent))
                .when(!on, |d| d.hover(|h| h.bg(t.hover)))
                .on_click(cx.listener(move |d, _, _, cx| {
                    d.export_format = id.to_string();
                    cx.notify();
                }))
                .child(if is_logo { logo(ic, px(20.)).into_any_element() } else { div().size(px(20.)).flex().items_center().justify_center().child(icon(ic)).into_any_element() })
                .child(div().w(px(220.)).font_weight(FontWeight::MEDIUM).child(name))
                .child(div().flex_1().text_size(px(sz::SM)).when(!on, |d| d.text_color(t.text_2)).child(line))
        });
        let all = self.export_scope_all;
        div()
            .flex()
            .flex_col()
            .child(header)
            .child(div().id("export-list").flex().flex_col().p(px(10.)).gap(px(1.)).children(rows))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .px(px(18.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(t.line)
                    .child(segmented("scope", vec![(true, "All pages".into()), (false, SharedString::from(format!("Only \u{201c}{page_name}\u{201d}")))], all, |v, _, cx| {
                        let v = *v;
                        cx.store().update(cx, |_, cx| cx.notify());
                        DIALOG_SCOPE.with(|c| c.set(Some(v)));
                    }, cx))
                    .child(Button::new("do-export", "Export…").with_icon("share").primary().on_click(cx.listener(move |d, _, _, cx| d.do_export(cx)))),
            )
            .into_any_element()
    }

    fn do_export(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let format = self.export_format.clone();
        let (dir, name, page) = {
            let s = store.read(cx);
            let dir = s.path.as_ref().filter(|_| !s.untitled).and_then(|p| p.parent().map(|d| d.to_path_buf())).or_else(dirs::document_dir).unwrap_or_else(|| std::path::PathBuf::from("."));
            let page = s.page_index().and_then(|i| s.doc.as_ref().map(|d| d.pages[i].id.to_string()));
            (dir, format!("{}.{}", s.title(), format), page)
        };
        let only = !self.export_scope_all;
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        store.update(cx, |s, cx| s.close_dialog(cx));
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                let mut p = json!({ "path": path, "format": format });
                if only && let Some(pg) = page {
                    p["pages"] = json!([pg]);
                }
                store
                    .update(cx, |s, cx| {
                        s.run_then("file.export", p, cx, |s, v, cx| {
                            let warn = v["warnings"].as_array().map(|w| w.len()).unwrap_or(0);
                            let msg = if warn > 0 { format!("Exported to {} ({warn} things couldn't be carried: see the log)", v["path"].as_str().unwrap_or("")) } else { format!("Exported to {}", v["path"].as_str().unwrap_or("")) };
                            for w in v["warnings"].as_array().into_iter().flatten() {
                                tracing::info!("export: {}", w.as_str().unwrap_or(""));
                            }
                            s.toast(folio_control::ToastKind::Success, msg, cx);
                        })
                    });
            }
        })
        .detach();
    }

    fn open(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        div()
            .flex()
            .flex_col()
            .child(Self::header("Open or import", Some("folio · office · google · apple · libreoffice"), cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .p(px(18.))
                    .child(div().text_color(t.text_2).child("Open a .folio file, or bring in a document, spreadsheet or presentation from another suite. The original stays as it is; folio opens a copy you can save as .folio or export back."))
                    .child(crate::views::home::coming_from(cx))
                    .child(div().flex().gap(px(8.)).child(Button::new("choose-file", "Choose a file…").with_icon("folder-open").primary().on_click(cx.listener(|d, _, _, cx| d.choose_file(cx))))),
            )
            .into_any_element()
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Open".into()) });
        store.update(cx, |s, cx| s.close_dialog(cx));
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.first()
            {
                store
                    .update(cx, |s, cx| {
                        s.run_then("file.open", json!({ "path": p }), cx, |s, v, cx| {
                            if let Some(w) = v["warnings"].as_array().filter(|w| !w.is_empty()) {
                                s.toast(folio_control::ToastKind::Info, format!("Opened, with {} things folio doesn't carry over (see file.info).", w.len()), cx);
                            }
                        })
                    });
            }
        })
        .detach();
    }

    fn settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let st = self.store.read(cx).settings.clone();
        let p = st.agent.permissions;
        let set = |key: &'static str| move |on: bool, _: &mut Window, cx: &mut App| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": on }), cx));
        // Agent permissions are the person's: set them through the settings file directly.
        let perm = |key: &'static str| {
            move |on: bool, _: &mut Window, cx: &mut App| {
                cx.store().update(cx, |s, cx| {
                    let r = s.session.update_settings(|x| match key {
                        "enabled" => x.agent.permissions.enabled = on,
                        "files" => x.agent.permissions.files = on,
                        "settings" => x.agent.permissions.settings = on,
                        "plugins" => x.agent.permissions.plugins = on,
                        _ => x.agent.permissions.app_control = on,
                    });
                    if let Err(e) = r {
                        s.error(e, cx);
                    }
                })
            }
        };
        div()
            .flex()
            .flex_col()
            .child(Self::header("Settings", None, cx))
            .child(
                div()
                    .id("settings-body")
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .p(px(18.))
                    .child(
                        div().flex().flex_col().gap(px(10.)).child(caps("Appearance", cx)).child(segmented(
                            "mode",
                            vec![("system".to_string(), "Follow the system".into()), ("dark".to_string(), "Dark".into()), ("light".to_string(), "Light".into())],
                            st.appearance.mode.clone(),
                            |m, _, cx| cx.store().update(cx, |s, cx| s.run("ui.theme", json!({ "mode": m }), cx)),
                            cx,
                        ))
                        .child(switch("transparency", "Translucent panels over the grain", st.appearance.transparency, set("appearance.transparency"), cx)),
                    )
                    .child(
                        div().flex().flex_col().gap(px(10.)).child(caps("Editing", cx))
                            .child(div().flex().items_center().gap(px(10.)).child(div().w(px(200.)).text_color(t.text_2).child("Name on comments and changes")).child(self.author.clone()))
                            .child(segmented("paper", vec![("a4".to_string(), "A4 paper".into()), ("letter".to_string(), "US Letter".into())], st.editing.paper.clone(), |p, _, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": "editing.paper", "value": p }), cx)), cx)),
                    )
                    .child(
                        div().flex().flex_col().gap(px(10.)).child(caps("Agent permissions", cx))
                            .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("What the built-in agent and MCP clients may do besides editing the open file (always undoable). Only you can change these."))
                            .child(switch("p-enabled", "Agents and MCP clients may work in folio", p.enabled, perm("enabled"), cx))
                            .child(switch("p-files", "Open, import and export files", p.files, perm("files"), cx))
                            .child(switch("p-settings", "Change settings", p.settings, perm("settings"), cx))
                            .child(switch("p-plugins", "Build and install plugins", p.plugins, perm("plugins"), cx))
                            .child(switch("p-app", "Control the app and other lsuite apps", p.app_control, perm("app"), cx)),
                    )
                    .child(
                        div().flex().flex_col().gap(px(10.)).child(caps("Updates", cx))
                            .child(switch("updates", "Look for a new version when folio starts", st.updates.check_on_start, set("updates.checkOnStart"), cx))
                            .child(update_controls(cx)), 
                    )
                    .child(
                        div().flex().flex_col().gap(px(8.)).child(caps("Driving folio from other tools", cx))
                            .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Every action is a command: folio-cli runs them from a terminal, folio-mcp gives them to any MCP client."))
                            .child(div().font_family(MONO).text_size(px(sz::SM)).p(px(8.)).bg(t.bg_sunken).child(format!("claude mcp add folio -- {} --live", folio_cli_path("folio-mcp")))),
                    ),
            )
            .into_any_element()
    }

    fn plugins(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let list = self.data.get("plugin.list").cloned().unwrap_or(Value::Null);
        let stock: Vec<Value> = list["stock"].as_array().cloned().unwrap_or_default();
        let installed: Vec<Value> = list["installed"].as_array().cloned().unwrap_or_default();
        let toolchain = self.data.get("plugin.toolchain").cloned();
        let row = |p: &Value, i: usize, removable: bool| {
            let id = p["id"].as_str().unwrap_or("").to_string();
            let on = p["enabled"].as_bool().unwrap_or(true);
            let (id2, id3) = (id.clone(), id.clone());
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .py(px(6.))
                .border_b_1()
                .border_color(t.line)
                .child(icon(if p["kind"] == "filter" { "file-input" } else { "square-function" }).mt(px(2.)).text_color(t.text_2))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().flex().gap(px(8.)).child(div().font_weight(FontWeight::MEDIUM).child(p["name"].as_str().unwrap_or("").to_string())).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(format!("{} {}", p["kind"].as_str().unwrap_or(""), p["version"].as_str().unwrap_or("")))))
                        .child(div().text_size(px(sz::SM)).text_color(t.text_2).child(p["description"].as_str().unwrap_or("").to_string())),
                )
                .child(switch(SharedString::from(format!("plug-{i}-{id}")), "", on, move |v, _, cx| cx.store().update(cx, |s, cx| s.run(if v { "plugin.enable" } else { "plugin.disable" }, json!({ "id": id2 }), cx)), cx))
                .when(removable, |d| d.child(Button::icon(SharedString::from(format!("rm-{id3}")), "trash-2", "Remove").small().on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("plugin.remove", json!({ "id": id3 }), cx)))))
        };
        let stock_rows: Vec<_> = stock.iter().enumerate().map(|(i, p)| row(p, i, false)).collect();
        let installed_rows: Vec<_> = installed.iter().enumerate().map(|(i, p)| row(p, i + 1000, true)).collect();
        let rust_ok = toolchain.as_ref().map(|t| t["ok"] == true);
        div()
            .flex()
            .flex_col()
            .child(Self::header("Plugins", Some("stock · installed · formats · build with your agent"), cx))
            .child(
                div()
                    .id("plugins-body")
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .p(px(18.))
                    .child(
                        div().flex().flex_col().gap(px(8.))
                            .child(div().flex().justify_between().child(caps(format!("Installed · {}", installed.len()), cx)).child(Button::new("rescan", "Rescan").with_icon("refresh-cw").small().on_click(cx.listener(|d, _, _, cx| {
                                d.store.update(cx, |s, cx| s.run("plugin.rescan", json!({}), cx));
                                d.load("plugin.list", json!({}), cx);
                            }))))
                            .when(installed.is_empty(), |d| d.child(div().text_color(t.text_3).text_size(px(sz::SM)).child("No plugins installed yet. Ask your agent for one below.")))
                            .children(installed_rows),
                    )
                    .child(
                        div().flex().flex_col().gap(px(8.)).child(caps("Build with your agent", cx))
                            .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Describe what you want; the agent writes it in Rust with folio's plugin SDK, builds it and installs it. New spreadsheet functions work at once."))
                            .child(self.describe.clone())
                            .child(
                                div().flex().items_center().justify_between()
                                    .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(match rust_ok {
                                        Some(true) => format!("RUST {}", toolchain.as_ref().and_then(|t| t["version"].as_str()).unwrap_or("")),
                                        Some(false) => "RUST ISN'T INSTALLED: https://rustup.rs".into(),
                                        None => String::new(),
                                    }))
                                    .child(div().flex().gap(px(6.))
                                        .child(Button::new("check-rust", "Check Rust").small().on_click(cx.listener(|d, _, _, cx| d.load("plugin.toolchain", json!({}), cx))))
                                        .child(Button::new("build-plugin", "Build it").with_icon("hammer").primary().small().on_click(cx.listener(|d, _, _, cx| {
                                            let want = d.describe.read(cx).text().trim().to_string();
                                            if want.is_empty() {
                                                return;
                                            }
                                            let prompt = format!("Build a folio plugin: {want}\n\nFollow plugin_guide: check plugin_toolchain, plugin_new, write the code with plugin_writeSource, plugin_build until it is green, then plugin_publishLocal, and try it with sheet_evaluate.");
                                            d.store.update(cx, |s, cx| {
                                                s.close_dialog(cx);
                                                s.agent_open = true;
                                                s.run("agent.send", json!({ "prompt": prompt }), cx);
                                            });
                                        })))),
                            ),
                    )
                    .child(
                        div().flex().flex_col().gap(px(8.)).child(caps("Formats folio loads", cx))
                            .child(div().flex().items_center().gap(px(10.)).child(icon("mark").size(px(20.))).child(div().flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).child("folio plugins (Rust, folio-plugin SDK)")).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child("~/.lsuite/plugins/folio/<id>/ · plugin.toml + .dylib / .so / .dll"))))
                            .child(div().text_size(px(sz::SM)).text_color(t.text_3).child("Office add-ins, VBA macros and Google Apps Script don't run in folio: their documents open, their code doesn't.")),
                    )
                    .child(div().flex().flex_col().gap(px(4.)).child(caps(format!("Stock · {}", stock.len()), cx)).children(stock_rows)),
            )
            .into_any_element()
    }

    fn palette(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let q = self.palette.read(cx).text().to_string();
        let items = palette_items(&q);
        div()
            .flex()
            .flex_col()
            .child(div().p(px(12.)).border_b_1().border_color(t.line).child(self.palette.clone()))
            .child(div().id("palette-list").max_h(px(420.)).overflow_y_scroll().p(px(6.)).children(items.into_iter().take(40).enumerate().map(|(i, (s, _))| {
                let action = (s.action)();
                div()
                    .id(("pal", i))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(10.))
                    .py(px(6.))
                    .cursor_pointer()
                    .when(i == 0, |d| d.bg(t.accent_soft))
                    .hover(|h| h.bg(t.hover))
                    .on_click(move |_, w, cx| {
                        cx.store().update(cx, |s, cx| s.close_dialog(cx));
                        w.dispatch_action(action.boxed_clone(), cx);
                    })
                    .child(div().flex().gap(px(8.)).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).w(px(70.)).child(s.group.to_uppercase())).child(s.label))
                    .child(kbd(crate::actions::keys_label(s.keys[0]), cx))
            })))
            .into_any_element()
    }

    fn shortcuts(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let mut groups: Vec<&str> = vec![];
        for s in SHORTCUTS {
            if !groups.contains(&s.group) {
                groups.push(s.group);
            }
        }
        div()
            .flex()
            .flex_col()
            .child(Self::header("Keyboard shortcuts", None, cx))
            .child(div().id("keys").overflow_y_scroll().p(px(18.)).flex().flex_wrap().gap(px(18.)).children(groups.into_iter().map(|g| {
                div().w(px(260.)).flex().flex_col().gap(px(4.)).child(caps(g, cx)).children(SHORTCUTS.iter().filter(|s| s.group == g).map(|s| {
                    div().flex().justify_between().gap(px(8.)).py(px(2.)).border_b_1().border_color(t.line).child(s.label).child(kbd(crate::actions::keys_label(s.keys[0]), cx))
                }))
            })))
            .into_any_element()
    }

    fn insert(&mut self, what: &str, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let sheets: Vec<String> = self.store.read(cx).doc.as_ref().map(|d| d.pages.iter().filter(|p| p.kind() == PageKind::Sheet).map(|p| p.name.clone()).collect()).unwrap_or_default();
        let title = match what {
            "table" => "Insert a table",
            "chart" => "Insert a chart",
            "link" => "Link",
            _ => "Comment",
        };
        let sheet_picker = |d: &mut Self, cx: &mut Context<Self>| -> AnyElement {
            let current = d.insert_sheet.clone().unwrap_or_default();
            segmented("sheet-pick", sheets.iter().map(|s| (s.clone(), SharedString::from(s.clone()))).collect(), current, |v, _, cx| {
                DIALOG_SHEET.with(|c| *c.borrow_mut() = Some(v.clone()));
                cx.store().update(cx, |_, cx| cx.notify());
            }, cx)
            .into_any_element()
        };
        let body: AnyElement = match what {
            "table" => {
                let linked = self.insert_link;
                div().flex().flex_col().gap(px(10.))
                    .child(segmented("table-kind", vec![(false, "Empty table".into()), (true, "Live table of a sheet range".into())], linked, |v, _, cx| {
                        DIALOG_LINK.with(|c| c.set(Some(*v)));
                        cx.store().update(cx, |_, cx| cx.notify());
                    }, cx))
                    .when(!linked, |d| d.child(div().flex().items_center().gap(px(8.)).child("Rows").child(self.a.clone()).child("Columns").child(self.b.clone())))
                    .when(linked && sheets.is_empty(), |d| d.child(div().text_color(t.text_2).child("Add a sheet page first: live tables show a sheet's cells.")))
                    .when(linked && !sheets.is_empty(), |d| d.child(sheet_picker(self, cx)).child(div().flex().items_center().gap(px(8.)).child("Range").child(self.a.clone())).child(div().text_size(px(sz::SM)).text_color(t.text_2).child("The table shows these cells as the sheet computes them, and follows every change.")))
                    .into_any_element()
            }
            "chart" => {
                let kind = self.insert_kind.clone();
                div().flex().flex_col().gap(px(10.))
                    .when(sheets.is_empty(), |d| d.child(div().text_color(t.text_2).child("Add a sheet page first: charts draw a sheet's cells.")))
                    .when(!sheets.is_empty(), |d| {
                        d.child(sheet_picker(self, cx))
                            .child(div().flex().items_center().gap(px(8.)).child("Range").child(self.a.clone()))
                            .child(segmented("chart-kind", folio_core::ChartKind::ALL.iter().map(|k| (k.id().to_string(), SharedString::from(k.label()))).collect(), kind, |v, _, cx| {
                                DIALOG_KIND.with(|c| *c.borrow_mut() = Some(v.clone()));
                                cx.store().update(cx, |_, cx| cx.notify());
                            }, cx))
                            .child(div().flex().items_center().gap(px(8.)).child("Title").child(self.b.clone()))
                    })
                    .into_any_element()
            }
            "link" => div().flex().flex_col().gap(px(8.)).child(div().text_color(t.text_2).child("The selected text links to:")).child(self.a.clone()).into_any_element(),
            _ => div().flex().flex_col().gap(px(8.)).child(div().text_color(t.text_2).child("A comment on the selected text:")).child(self.a.clone()).into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .child(Self::header(title, None, cx))
            .child(div().p(px(18.)).child(body))
            .child(div().flex().justify_end().gap(px(8.)).px(px(18.)).py(px(12.)).border_t_1().border_color(t.line).child(Button::new("cancel", "Cancel").small().on_click(cx.listener(|d, _, _, cx| d.close(cx)))).child(Button::new("ok", "Insert").primary().small().on_click(cx.listener(|d, _, w, cx| d.submit(w, cx)))))
            .into_any_element()
    }

    fn page_setup(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let setup = s.page_index().and_then(|i| s.doc.as_ref().and_then(|d| d.pages[i].doc().map(|t| t.setup.clone()))).unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .child(Self::header("Page setup", Some(setup.size_name()), cx))
            .child(
                div().p(px(18.)).flex().flex_col().gap(px(12.))
                    .child(div().text_color(t.text_2).child(format!("{} × {} points · margins {} / {} / {} / {} points (72 points = 1 inch)", setup.width, setup.height, setup.margin_top, setup.margin_right, setup.margin_bottom, setup.margin_left)))
                    .child(segmented("ps-size", vec![("a4", "A4".into()), ("letter", "Letter".into()), ("legal", "Legal".into()), ("a5", "A5".into()), ("a3", "A3".into())], setup.size_name(), |v, _, cx| cx.store().update(cx, |s, cx| s.run("doc.setup", json!({ "size": v }), cx)), cx))
                    .child(switch("diff-first", "No header or footer on the first page", setup.different_first, |on, _, cx| cx.store().update(cx, |s, cx| s.run("doc.setup", json!({ "differentFirst": on }), cx)), cx)),
            )
            .into_any_element()
    }

    fn about(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        div()
            .flex()
            .flex_col()
            .child(Self::header("About folio", None, cx))
            .child(
                div().p(px(18.)).flex().flex_col().gap(px(10.))
                    .child(div().flex().items_center().gap(px(12.)).child(icon("mark").size(px(36.))).child(div().flex().flex_col().child(div().text_size(px(sz::XL)).font_weight(FontWeight::BOLD).child("folio")).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_2).child(format!("VERSION {} · BETA · MIT", env!("CARGO_PKG_VERSION"))))))
                    .child(div().text_color(t.text_2).child("Documents, sheets and slides in one file. Part of lsuite, the free creative suite where every app can be driven by an AI agent."))
                    .child(div().flex().gap(px(8.)).child(Button::new("site", "lsuite.xyz/folio").small().on_click(|_, _, _| folio_control::lsuite::open_url(crate::app::PAGE_URL))).child(Button::new("support", "Support folio").small().on_click(|_, _, _| folio_control::lsuite::open_url(crate::app::SUPPORT_URL)))),
            )
            .into_any_element()
    }
}

thread_local! {
    static DIALOG_SCOPE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    static DIALOG_LINK: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    static DIALOG_SHEET: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static DIALOG_KIND: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Where an executable shipped with folio is (next to this one), for setup lines.
fn folio_cli_path(name: &str) -> String {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name))).filter(|p| p.exists()).map(|p| p.display().to_string()).unwrap_or_else(|| crate::ui::mcp_fallback().to_string())
}

/// Shortcuts matching a query, best first.
fn palette_items(q: &str) -> Vec<(&'static crate::actions::Shortcut, Box<dyn gpui::Action>)> {
    let q = q.trim().to_lowercase();
    let mut out: Vec<(usize, &crate::actions::Shortcut)> = SHORTCUTS
        .iter()
        .filter_map(|s| {
            let label = s.label.to_lowercase();
            if q.is_empty() {
                return Some((1, s));
            }
            if label.starts_with(&q) {
                Some((0, s))
            } else if label.contains(&q) || s.group.to_lowercase().contains(&q) {
                Some((1, s))
            } else {
                None
            }
        })
        .collect();
    out.sort_by_key(|(k, s)| (*k, s.label));
    out.into_iter().map(|(_, s)| (s, (s.action)())).collect()
}

/// The frame every dialog sits in: scrim, centred tier-3 panel inside corner brackets.
pub fn modal(name: &'static str, width: f32, content: impl IntoElement, window: &Window, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let vp = window.viewport_size();
    let w = width.min(f32::from(vp.width) - 48.0);
    let max_h = f32::from(vp.height) - 80.0;
    let panel = div()
        .id("modal")
        .occlude()
        .relative()
        .w(px(w))
        .max_h(px(max_h))
        .flex()
        .flex_col()
        .glass(t.glass3)
        .shadow(t.glass_shadow())
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(div().flex_1().min_h_0().flex().flex_col().child(content));
    let framed = div().relative().child(panel).child(crate::ui::grain::brackets(14., -9., t.text_3));
    let scrim = div()
        .id("modal-scrim")
        .occlude()
        .absolute()
        .inset_0()
        .bg(t.scrim)
        .flex()
        .justify_center()
        .items_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.store().update(cx, |s, cx| s.close_dialog(cx)))
        .child(motion::enter(framed, (name, 1usize), motion::BASE, (0., 10.)));
    deferred(motion::fade(scrim, (name, 0usize), motion::FAST)).with_priority(1).into_any_element()
}

impl Render for Dialogs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = self.store.read(cx).dialog.clone();
        let name = dialog.as_ref().map(|d| format!("{d:?}"));
        if name != self.opened {
            self.opened = name;
            if let Some(d) = &dialog {
                self.on_open(d, window, cx);
            }
        }
        // Choices made in segmented controls (they can't reach `self` from their closures).
        if let Some(v) = DIALOG_SCOPE.with(|c| c.take()) {
            self.export_scope_all = v;
        }
        if let Some(v) = DIALOG_LINK.with(|c| c.take()) {
            self.insert_link = v;
            if v && let Some(r) = self.sheet_used_range(cx) {
                self.a.update(cx, |i, cx| i.set_text(r, cx));
            }
        }
        if let Some(v) = DIALOG_SHEET.with(|c| c.borrow_mut().take()) {
            self.insert_sheet = Some(v);
            if let Some(r) = self.sheet_used_range(cx) {
                self.a.update(cx, |i, cx| i.set_text(r, cx));
            }
        }
        if let Some(v) = DIALOG_KIND.with(|c| c.borrow_mut().take()) {
            self.insert_kind = v;
        }
        let content: Option<(&'static str, f32, AnyElement)> = match dialog {
            Some(Dialog::Export) => Some(("export", 640., self.export(cx))),
            Some(Dialog::Open) => Some(("open", 760., self.open(cx))),
            Some(Dialog::Settings { .. }) => Some(("settings", 640., self.settings(cx))),
            Some(Dialog::Plugins) => Some(("plugins", 720., self.plugins(cx))),
            Some(Dialog::Palette) => Some(("palette", 560., self.palette(cx))),
            Some(Dialog::Shortcuts) => Some(("shortcuts", 900., self.shortcuts(cx))),
            Some(Dialog::Insert { what }) => Some(("insert", 520., self.insert(&what, cx))),
            Some(Dialog::PageSetup) => Some(("page-setup", 520., self.page_setup(cx))),
            Some(Dialog::About) => Some(("about", 480., self.about(cx))),
            None => None,
        };
        div()
            .key_context("Modal")
            .on_action(cx.listener(|d, _: &crate::actions::Escape, _, cx| d.close(cx)))
            .when(content.is_some(), |d| d.absolute().inset_0())
            .children(content.map(|(name, w, c)| modal(name, w, c, window, cx)))
    }
}

fn update_controls(cx: &App) -> AnyElement {
    let store = cx.store();
    let s = store.read(cx);
    let status = folio_control::update::status(&s.session);
    let message = if let Some(error) = &status.error { error.clone() }
        else if status.ready { "Update installed. Restart to use it.".into() }
        else if let Some(p) = status.progress { format!("Downloading and verifying: {:.0}%", p * 100.) }
        else if let Some(version) = &status.available { format!("folio {version} is available.") }
        else if status.checked_at.is_some() { format!("folio {} is up to date.", status.current) }
        else { format!("folio {} · signed updates", status.current) };
    div().flex().flex_col().gap(px(10.))
        .child(div().text_size(px(sz::SM)).child(message))
        .child(crate::ui::switch("auto-install-updates", "Install verified updates automatically", s.settings.updates.auto_install,
            |on, _, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({"key":"updates.autoInstall","value":on}), cx)), cx))
        .child(div().flex().flex_wrap().gap(px(8.))
            .child(Button::new("check-updates-now", "Check now").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("app.checkUpdates", json!({}), cx))))
            .when(status.can_install && status.progress.is_none() && !status.ready, |d| d.child(Button::new("install-update", "Install update").small().primary().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("app.installUpdate", json!({}), cx)))))
            .when(status.ready, |d| d.child(Button::new("restart-update", "Restart folio").small().primary().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("app.restart", json!({}), cx)))))
            .when(status.available.is_some() && !status.can_install && !status.ready, |d| d.child(Button::new("download-update", "Open the lsuite app").small().on_click(|_, _, cx| cx.open_url(folio_control::update::RELEASES_URL)))))
        .children(status.install_blocked.map(|message| div().text_size(px(sz::SM)).child(message)))
        .into_any_element()
}
