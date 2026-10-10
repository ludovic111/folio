//! The editor: the title bar, the pages sidebar, the contextual toolbar, the work area (a
//! document's paper, a sheet's grid or a deck's slides), the inspector and the Agent panel.
//!
//! Every area is titled like a sidebar; the toolbar changes with the kind of page and its tools
//! are boxed by what they do (no ribbon of two hundred buttons: what people use, where they
//! look for it).

use folio_core::{Id, PageKind};
use gpui::{AnyElement, App, Context, Entity, FontWeight, MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px};
use serde_json::json;

use crate::actions::*;
use crate::store::{Dialog, MenuEntry, MenuItem, Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::{Button, GlassExt, caps, group, icon, panel_title, tool};
use crate::views::{agent_panel, deckview::DeckView, docview::DocView, inspector::Inspector, sheetview::SheetView};

pub const TOPBAR_H: f32 = 46.;
const SIDEBAR_W: f32 = 232.;
const INSPECTOR_W: f32 = 284.;
const AGENT_W: f32 = 380.;

pub struct Editor {
    store: Entity<Store>,
    pub doc: Entity<DocView>,
    pub sheet: Entity<SheetView>,
    pub deck: Entity<DeckView>,
    inspector: Entity<Inspector>,
    agent: Entity<agent_panel::AgentPanel>,
    /// Renaming a page in the sidebar (its id) with this field.
    renaming: Option<Id>,
    rename_input: Entity<TextInput>,
    /// The kind of page last shown (to focus the right view when it changes).
    shown: Option<(Id, PageKind)>,
    _subs: Vec<Subscription>,
}

pub fn kind_icon(k: PageKind) -> &'static str {
    match k {
        PageKind::Doc => "file-text",
        PageKind::Sheet => "sheet",
        PageKind::Deck => "presentation",
    }
}

impl Editor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let doc = cx.new(|cx| DocView::new(window, cx));
        let sheet = cx.new(|cx| SheetView::new(window, cx));
        let deck = cx.new(|cx| DeckView::new(window, cx));
        let inspector = cx.new(|cx| Inspector::new(window, cx));
        let agent = cx.new(|cx| agent_panel::AgentPanel::new(window, cx));
        let rename_input = cx.new(TextInput::new);
        let mut subs = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        subs.push(cx.subscribe_in(&rename_input, window, |e: &mut Self, input, ev: &InputEvent, window, cx| match ev {
            InputEvent::Submit | InputEvent::Blur => {
                if let Some(id) = e.renaming.take() {
                    let name = input.read(cx).text().trim().to_string();
                    if !name.is_empty() {
                        e.store.update(cx, |s, cx| s.run("page.rename", json!({ "page": id.to_string(), "name": name }), cx));
                    }
                    e.focus_view(window, cx);
                    cx.notify();
                }
            }
            InputEvent::Cancel => {
                e.renaming = None;
                e.focus_view(window, cx);
                cx.notify();
            }
            _ => {}
        }));
        Self { store, doc, sheet, deck, inspector, agent, renaming: None, rename_input, shown: None, _subs: subs }
    }

    /// Focuses the view of the page shown.
    pub fn focus_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.read(cx).page_kind() {
            Some(PageKind::Doc) => crate::views::focus(&self.doc, window, cx),
            Some(PageKind::Sheet) => crate::views::focus(&self.sheet, window, cx),
            Some(PageKind::Deck) => crate::views::focus(&self.deck, window, cx),
            None => {}
        }
    }

    /// Scrolls the shown page to its selection (after `ui.show`).
    pub fn reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_view(window, cx);
        self.doc.update(cx, |d, cx| d.reveal(cx));
    }

    fn show(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.show_page(id, cx));
        self.focus_view(window, cx);
    }

    // ---- title bar -----------------------------------------------------------

    fn title_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let (can_undo, can_redo) = (s.can_undo, s.can_redo);
        let (sidebar, inspector, agent) = (s.sidebar_open, s.inspector_open, s.agent_open);
        let title: SharedString = s.title().into();
        let status: SharedString = if s.untitled {
            "Not in a folder yet".into()
        } else {
            s.path.as_ref().and_then(|p| p.parent()).map(|p| p.display().to_string().replace(&dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default(), "~")).unwrap_or_default().into()
        };
        let saved = s.saved;
        let untitled = s.untitled;
        let fullscreen = window.is_fullscreen();
        let controls = crate::ui::window_controls(window, cx);
        let wide = f32::from(window.viewport_size().width) > 1180.;
        let store = self.store.clone();
        div()
            .id("title-bar")
            .h(px(TOPBAR_H))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .pl(px(if fullscreen || !cfg!(target_os = "macos") { 12. } else { 84. }))
            .when(controls.is_none(), |d| d.pr(px(12.)))
            .window_control_area(gpui::WindowControlArea::Drag)
            .glass(t.glass1)
            .border_0()
            .border_b_1()
            .border_color(t.line)
            .on_mouse_down(MouseButton::Left, |e, window, _| {
                if e.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .id("home")
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(30.))
                            .cursor_pointer()
                            .hover(|d| d.bg(t.hover))
                            .tooltip(|_, cx| crate::ui::tooltip("All files (home)".into(), cx))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|e, _, _, cx| e.store.update(cx, |s, cx| s.run("file.close", json!({}), cx))))
                            .child(icon("mark").size(px(18.)).text_color(t.text)),
                    )
                    .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_size(px(sz::MD)).child(title))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(6.))
                            .font_family(MONO)
                            .text_size(px(sz::XS))
                            .text_color(t.text_3)
                            .child(if saved { "SAVED" } else { "SAVING…" })
                            .when(wide, |d| d.child(div().max_w(px(260.)).truncate().child(status))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(group(
                        [
                            tool("undo", "undo-2", "Undo", false, crate::actions::tip("Undo", &Undo)).disabled(!can_undo).on_click(|_, w, cx| w.dispatch_action(Box::new(Undo), cx)).into_any_element(),
                            tool("redo", "redo-2", "Redo", false, crate::actions::tip("Redo", &Redo)).disabled(!can_redo).on_click(|_, w, cx| w.dispatch_action(Box::new(Redo), cx)).into_any_element(),
                        ],
                        cx,
                    ))
                    .child(group(
                        [
                            tool("pages", "panel-left", "Pages", wide, crate::actions::tip("Pages", &ToggleSidebar)).selected(sidebar).on_click(|_, w, cx| w.dispatch_action(Box::new(ToggleSidebar), cx)).into_any_element(),
                            tool("inspector", "panel-right", "Inspector", wide, crate::actions::tip("Inspector", &ToggleInspector)).selected(inspector).on_click(|_, w, cx| w.dispatch_action(Box::new(ToggleInspector), cx)).into_any_element(),
                        ],
                        cx,
                    ))
                    .child(group([tool("agent", "sparkles", "Agent", true, crate::actions::tip("Agent", &ToggleAgent)).selected(agent).on_click(|_, w, cx| w.dispatch_action(Box::new(ToggleAgent), cx)).into_any_element()], cx))
                    .child(group(
                        [
                            tool("plugins", "puzzle", "Plugins", false, "Plugins").on_click(|_, w, cx| w.dispatch_action(Box::new(OpenPlugins), cx)).into_any_element(),
                            tool("settings", "settings", "Settings", false, crate::actions::tip("Settings", &OpenSettings)).on_click(|_, w, cx| w.dispatch_action(Box::new(OpenSettings), cx)).into_any_element(),
                            tool("more", "ellipsis", "More", false, "More").on_click(move |_, window, cx| {
                                let pos = window.mouse_position();
                                let entries = vec![
                                    MenuItem::new("Open or import…", |w, cx| w.dispatch_action(Box::new(OpenFile), cx)).icon("folder-open").shortcut(crate::actions::hint(&OpenFile).unwrap_or_default()).entry(),
                                    MenuItem::new(if untitled { "Save in a folder…" } else { "Save as…" }, |w, cx| w.dispatch_action(Box::new(SaveAs), cx)).icon("download").entry(),
                                    MenuEntry::Separator,
                                    MenuItem::new("Keyboard shortcuts", |w, cx| w.dispatch_action(Box::new(ShowShortcuts), cx)).icon("keyboard").shortcut(crate::actions::hint(&ShowShortcuts).unwrap_or_default()).entry(),
                                    MenuItem::new("Command palette", |w, cx| w.dispatch_action(Box::new(Palette), cx)).icon("command").shortcut(crate::actions::hint(&Palette).unwrap_or_default()).entry(),
                                    MenuItem::new("Dark or light", |w, cx| w.dispatch_action(Box::new(ToggleTheme), cx)).icon("sun").entry(),
                                    MenuEntry::Separator,
                                    MenuItem::new("About folio", |w, cx| w.dispatch_action(Box::new(About), cx)).icon("info").entry(),
                                ];
                                store.update(cx, |s, cx| s.open_menu(pos, entries, cx));
                            })
                            .into_any_element(),
                        ],
                        cx,
                    ))
                    .child(Button::new("export", "Export").with_icon("share").primary().small().tooltip(crate::actions::tip("Export: PDF, Word, Excel, PowerPoint…", &Export)).on_click(|_, w, cx| w.dispatch_action(Box::new(Export), cx))),
            )
            .children(controls)
            .into_any_element()
    }

    // ---- sidebar ---------------------------------------------------------------

    fn sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let Some(doc) = s.doc.clone() else { return div().into_any_element() };
        let current = s.page.clone();
        let store = self.store.clone();
        let add_menu = move |window: &mut Window, cx: &mut App| {
            let pos = window.mouse_position();
            let entries = vec![
                MenuItem::new("Document", |w, cx| w.dispatch_action(Box::new(NewDocPage), cx)).icon("file-text").entry(),
                MenuItem::new("Sheet", |w, cx| w.dispatch_action(Box::new(NewSheetPage), cx)).icon("sheet").entry(),
                MenuItem::new("Deck", |w, cx| w.dispatch_action(Box::new(NewDeckPage), cx)).icon("presentation").entry(),
                MenuEntry::Separator,
                MenuItem::new("Import pages from a file…", |_, cx| {
                    let store = cx.store();
                    let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add pages".into()) });
                    cx.spawn(async move |cx| {
                        if let Ok(Ok(Some(paths))) = rx.await
                            && let Some(p) = paths.first()
                        {
                            store.update(cx, |s, cx| s.run_then("file.import", json!({ "path": p }), cx, |s, v, cx| s.flash(format!("Added {}", v["added"].as_array().map(|a| a.len()).unwrap_or(0)), cx)));
                        }
                    })
                    .detach();
                })
                .icon("file-input")
                .entry(),
            ];
            store.update(cx, |s, cx| s.open_menu(pos, entries, cx));
        };
        let renaming = self.renaming.clone();
        let rows = doc.pages.iter().enumerate().map(|(i, p)| {
            let selected = current.as_ref() == Some(&p.id);
            let id = p.id.clone();
            let id2 = p.id.clone();
            let name = p.name.clone();
            let kind = p.kind();
            let n = doc.pages.len();
            let summary = folio_control::commands::util::summary(p);
            let store = self.store.clone();
            if renaming.as_ref() == Some(&p.id) {
                return div().px(px(8.)).py(px(4.)).child(self.rename_input.clone()).into_any_element();
            }
            let (fg, fg2, bg) = if selected { (t.text_on_accent, t.text_on_accent.opacity(0.7), Some(t.accent)) } else { (t.text, t.text_3, None) };
            div()
                .id(("page", i))
                .flex()
                .items_start()
                .gap(px(10.))
                .px(px(10.))
                .py(px(8.))
                .cursor_pointer()
                .when_some(bg, |d, b| d.bg(b))
                .when(!selected, |d| d.hover(|h| h.bg(t.hover)))
                .on_click(cx.listener(move |e, ev: &gpui::ClickEvent, window, cx| {
                    if ev.click_count() >= 2 {
                        e.renaming = Some(id.clone());
                        let name = name.clone();
                        e.rename_input.update(cx, |i, cx| {
                            i.set_text(name, cx);
                            i.select_all_text(cx);
                        });
                        crate::ui::input::focus(&e.rename_input, window, cx);
                        cx.notify();
                        return;
                    }
                    e.show(id.clone(), window, cx);
                }))
                .on_mouse_down(MouseButton::Right, move |ev, _, cx| {
                    let pos = ev.position;
                    let (a, b, c, d, e2) = (id2.to_string(), id2.to_string(), id2.to_string(), id2.to_string(), id2.to_string());
                    let entries = vec![
                        MenuItem::new("Rename", move |_, cx| cx.store().update(cx, |s, cx| s.flash("Double-click a page to rename it.", cx))).icon("pen-line").entry(),
                        MenuItem::new("Duplicate", move |_, cx| cx.store().update(cx, |s, cx| s.run("page.duplicate", json!({ "page": a }), cx))).icon("copy").entry(),
                        MenuItem::new("Move up", move |_, cx| cx.store().update(cx, |s, cx| s.run("page.move", json!({ "page": b, "to": i.saturating_sub(1) }), cx))).icon("arrow-up").disabled(i == 0).entry(),
                        MenuItem::new("Move down", move |_, cx| cx.store().update(cx, |s, cx| s.run("page.move", json!({ "page": c, "to": (i + 1).min(n - 1) }), cx))).icon("arrow-down").disabled(i + 1 >= n).entry(),
                        MenuItem::new("Export this page…", move |_, cx| {
                            let _ = &d;
                            cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Export, cx))
                        })
                        .icon("share")
                        .entry(),
                        MenuEntry::Separator,
                        MenuItem::new("Delete", move |_, cx| cx.store().update(cx, |s, cx| s.run("page.remove", json!({ "page": e2 }), cx))).icon("trash-2").danger().disabled(n <= 1).entry(),
                    ];
                    cx.stop_propagation();
                    store.update(cx, |s, cx| s.open_menu(pos, entries, cx));
                })
                .child(icon(kind_icon(kind)).size(px(16.)).mt(px(1.)).text_color(fg))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_color(fg).font_weight(FontWeight::MEDIUM).child(p.name.clone()))
                        .child(div().font_family(MONO).text_size(px(10.5)).text_color(fg2).truncate().child(summary)),
                )
                .into_any_element()
        });
        div()
            .id("sidebar")
            .flex_none()
            .w(px(SIDEBAR_W))
            .h_full()
            .flex()
            .flex_col()
            .glass(t.glass1)
            .border_0()
            .border_r_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .h(px(40.))
                    .px(px(12.))
                    .child(div().flex().items_baseline().gap(px(8.)).child(panel_title("Pages")).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(format!("{}", doc.pages.len()))))
                    .child(Button::icon("add-page", "plus", "Add a page: document, sheet or deck").small().on_click(move |_, w, cx| add_menu(w, cx))),
            )
            .child(div().id("pages").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap(px(2.)).px(px(6.)).children(rows))
            .child(
                div().flex_none().p(px(10.)).border_t_1().border_color(t.line).flex().flex_col().gap(px(6.)).child(caps("Add", cx)).child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(Button::new("add-doc", "Doc").small().full_width().on_click(|_, w, cx| w.dispatch_action(Box::new(NewDocPage), cx)))
                        .child(Button::new("add-sheet", "Sheet").small().full_width().on_click(|_, w, cx| w.dispatch_action(Box::new(NewSheetPage), cx)))
                        .child(Button::new("add-deck", "Deck").small().full_width().on_click(|_, w, cx| w.dispatch_action(Box::new(NewDeckPage), cx))),
                ),
            )
            .into_any_element()
    }

    // ---- the toolbar -------------------------------------------------------------

    fn toolbar(&mut self, kind: PageKind, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let wide = f32::from(window.viewport_size().width) > 1500.;
        let bar = div().flex().flex_none().flex_wrap().items_center().gap(px(8.)).px(px(12.)).py(px(7.)).border_b_1().border_color(t.line).glass(t.glass1).border_0().border_b_1();
        match kind {
            PageKind::Doc => {
                let st = self.doc.read(cx).state(cx);
                let (b, i, u, s, style, align, list) = st.unwrap_or((false, false, false, false, Default::default(), Default::default(), None));
                let docv = self.doc.clone();
                let style_menu = {
                    let docv = docv.clone();
                    let store = self.store.clone();
                    move |window: &mut Window, cx: &mut App| {
                        let pos = window.mouse_position();
                        let entries = folio_core::ParaStyle::ALL
                            .iter()
                            .map(|ps| {
                                let docv = docv.clone();
                                let id = ps.id();
                                MenuItem::new(ps.label(), move |w, cx| {
                                    docv.update(cx, |v, cx| v.paragraph(json!({ "style": id }), cx));
                                    crate::views::focus(&docv, w, cx);
                                })
                                .entry()
                            })
                            .collect();
                        store.update(cx, |s, cx| s.open_menu(pos, entries, cx));
                    }
                };
                let d1 = docv.clone();
                let act = move |f: fn(&mut DocView, &mut Context<DocView>)| {
                    let d = d1.clone();
                    move |_: &gpui::ClickEvent, w: &mut Window, cx: &mut App| {
                        d.update(cx, f);
                        crate::views::focus(&d, w, cx);
                    }
                };
                bar.child(
                    div()
                        .id("style-picker")
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .w(px(150.))
                        .h(px(crate::ui::GROUP_H))
                        .px(px(10.))
                        .border_1()
                        .border_color(t.line_strong)
                        .cursor_pointer()
                        .hover(|d| d.bg(t.hover))
                        .on_click(move |_, w, cx| style_menu(w, cx))
                        .child(div().truncate().child(style.label()))
                        .child(icon("chevron-down")),
                )
                .child(group(
                    [
                        tool("b", "bold", "Bold", false, crate::actions::tip("Bold", &Bold)).selected(b).on_click(act(|v, cx| v.toggle("bold", cx))).into_any_element(),
                        tool("i", "italic", "Italic", false, crate::actions::tip("Italic", &Italic)).selected(i).on_click(act(|v, cx| v.toggle("italic", cx))).into_any_element(),
                        tool("u", "underline", "Underline", false, crate::actions::tip("Underline", &Underline)).selected(u).on_click(act(|v, cx| v.toggle("underline", cx))).into_any_element(),
                        tool("s", "strikethrough", "Strikethrough", false, crate::actions::tip("Strikethrough", &Strike)).selected(s).on_click(act(|v, cx| v.toggle("strike", cx))).into_any_element(),
                    ],
                    cx,
                ))
                .child(group(
                    [
                        tool("al", "align-left", "Left", false, "Align left").selected(align == folio_core::Align::Left).on_click(act(|v, cx| v.paragraph(json!({ "align": "left" }), cx))).into_any_element(),
                        tool("ac", "align-center", "Centre", false, "Centre").selected(align == folio_core::Align::Center).on_click(act(|v, cx| v.paragraph(json!({ "align": "center" }), cx))).into_any_element(),
                        tool("ar", "align-right", "Right", false, "Align right").selected(align == folio_core::Align::Right).on_click(act(|v, cx| v.paragraph(json!({ "align": "right" }), cx))).into_any_element(),
                        tool("aj", "align-justify", "Justify", false, "Justify").selected(align == folio_core::Align::Justify).on_click(act(|v, cx| v.paragraph(json!({ "align": "justify" }), cx))).into_any_element(),
                    ],
                    cx,
                ))
                .child(group(
                    [
                        tool("lb", "list", "Bullets", false, crate::actions::tip("Bulleted list", &BulletList)).selected(list == Some(folio_core::ListKind::Bullet)).on_click(|_, w, cx| w.dispatch_action(Box::new(BulletList), cx)).into_any_element(),
                        tool("ln", "list-ordered", "Numbers", false, crate::actions::tip("Numbered list", &NumberList)).selected(list == Some(folio_core::ListKind::Number)).on_click(|_, w, cx| w.dispatch_action(Box::new(NumberList), cx)).into_any_element(),
                        tool("lc", "list-checks", "Checklist", false, crate::actions::tip("Checklist", &CheckList)).selected(list == Some(folio_core::ListKind::Check)).on_click(|_, w, cx| w.dispatch_action(Box::new(CheckList), cx)).into_any_element(),
                    ],
                    cx,
                ))
                .child(group(
                    [
                        tool("ins-table", "table", "Table", wide, "Insert a table (or a live table of a sheet range)").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Insert { what: "table".into() }, cx))).into_any_element(),
                        tool("ins-image", "image", "Picture", wide, "Insert a picture").on_click(cx.listener(|e, _, _, cx| e.insert_picture(cx))).into_any_element(),
                        tool("ins-chart", "chart-column", "Chart", wide, "Insert a live chart of a sheet range").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Insert { what: "chart".into() }, cx))).into_any_element(),
                        tool("ins-link", "link", "Link", false, crate::actions::tip("Link", &InsertLink)).on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Insert { what: "link".into() }, cx))).into_any_element(),
                        tool("ins-comment", "message-square", "Comment", false, crate::actions::tip("Comment", &AddComment)).on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Insert { what: "comment".into() }, cx))).into_any_element(),
                        tool("ins-break", "separator-horizontal", "Page break", false, crate::actions::tip("Page break", &PageBreak)).on_click(|_, w, cx| w.dispatch_action(Box::new(PageBreak), cx)).into_any_element(),
                    ],
                    cx,
                ))
                .into_any_element()
            }
            PageKind::Sheet => {
                let f = self.sheet.read(cx).active_format(cx);
                let sv = self.sheet.clone();
                let act = move |p: serde_json::Value| {
                    let sv = sv.clone();
                    move |_: &gpui::ClickEvent, w: &mut Window, cx: &mut App| {
                        sv.update(cx, |v, cx| v.format(p.clone(), cx));
                        crate::views::focus(&sv, w, cx);
                    }
                };
                let sv2 = self.sheet.clone();
                let sheet_do = move |f: fn(&mut SheetView, &mut Context<SheetView>)| {
                    let sv = sv2.clone();
                    move |_: &gpui::ClickEvent, w: &mut Window, cx: &mut App| {
                        sv.update(cx, f);
                        crate::views::focus(&sv, w, cx);
                    }
                };
                let number_menu = {
                    let sv = self.sheet.clone();
                    let store = self.store.clone();
                    move |window: &mut Window, cx: &mut App| {
                        let pos = window.mouse_position();
                        let entries = folio_calc::PRESETS
                            .iter()
                            .map(|(label, code)| {
                                let sv = sv.clone();
                                let code = code.to_string();
                                let example = folio_calc::format_value(&folio_calc::Value::Number(if code.contains('y') || code.contains('h') { 46301.5625 } else if code.contains('%') { 0.1234 } else { 1234.5 }), Some(&code));
                                MenuItem::new(format!("{label}   {example}"), move |w, cx| {
                                    sv.update(cx, |v, cx| v.format(json!({ "number": code }), cx));
                                    crate::views::focus(&sv, w, cx);
                                })
                                .entry()
                            })
                            .collect();
                        store.update(cx, |s, cx| s.open_menu(pos, entries, cx));
                    }
                };
                let label = f.number.as_deref().and_then(|n| folio_calc::PRESETS.iter().find(|(_, c)| *c == n).map(|(l, _)| l.to_string())).unwrap_or_else(|| if f.number.is_some() { "Custom".into() } else { "General".into() });
                bar.child(
                    div()
                        .id("number-picker")
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .w(px(130.))
                        .h(px(crate::ui::GROUP_H))
                        .px(px(10.))
                        .border_1()
                        .border_color(t.line_strong)
                        .cursor_pointer()
                        .hover(|d| d.bg(t.hover))
                        .on_click(move |_, w, cx| number_menu(w, cx))
                        .child(div().truncate().child(label))
                        .child(icon("chevron-down")),
                )
                .child(group(
                    [
                        tool("cur", "dollar-sign", "Currency", false, "Currency").on_click(act(json!({ "number": "$#,##0.00" }))).into_any_element(),
                        tool("pct", "percent", "Percent", false, "Percent").on_click(act(json!({ "number": "0%" }))).into_any_element(),
                        tool("dec", "hash", "Number", false, "Number with two decimals").on_click(act(json!({ "number": "#,##0.00" }))).into_any_element(),
                        tool("date", "calendar", "Date", false, "Date").on_click(act(json!({ "number": "yyyy-mm-dd" }))).into_any_element(),
                    ],
                    cx,
                ))
                .child(group(
                    [
                        tool("b", "bold", "Bold", false, crate::actions::tip("Bold", &Bold)).selected(f.bold).on_click(act(json!({ "bold": !f.bold }))).into_any_element(),
                        tool("i", "italic", "Italic", false, crate::actions::tip("Italic", &Italic)).selected(f.italic).on_click(act(json!({ "italic": !f.italic }))).into_any_element(),
                        tool("u", "underline", "Underline", false, crate::actions::tip("Underline", &Underline)).selected(f.underline).on_click(act(json!({ "underline": !f.underline }))).into_any_element(),
                    ],
                    cx,
                ))
                .child(group(
                    [
                        tool("al", "align-left", "Left", false, "Align left").selected(f.align == Some(folio_core::Align::Left)).on_click(act(json!({ "align": "left" }))).into_any_element(),
                        tool("ac", "align-center", "Centre", false, "Centre").selected(f.align == Some(folio_core::Align::Center)).on_click(act(json!({ "align": "center" }))).into_any_element(),
                        tool("ar", "align-right", "Right", false, "Align right").selected(f.align == Some(folio_core::Align::Right)).on_click(act(json!({ "align": "right" }))).into_any_element(),
                        tool("wrap", "wrap-text", "Wrap", false, "Wrap text").selected(f.wrap).on_click(act(json!({ "wrap": !f.wrap }))).into_any_element(),
                    ],
                    cx,
                ))
                .child(group(
                    [
                        tool("sum", "sigma", "Sum", wide, crate::actions::tip("AutoSum", &AutoSum)).on_click(sheet_do(|v, cx| v.autosum(cx))).into_any_element(),
                        tool("sort-a", "arrow-down-a-z", "Sort", wide, "Sort ascending by this column").on_click(sheet_do(|v, cx| v.sort(false, cx))).into_any_element(),
                        tool("sort-z", "arrow-up-z-a", "Sort", false, "Sort descending by this column").on_click(sheet_do(|v, cx| v.sort(true, cx))).into_any_element(),
                        tool("chart", "chart-column", "Chart", true, "Chart of the selection").on_click(sheet_do(|v, cx| v.chart_selection("column", cx))).into_any_element(),
                        tool("freeze", "snowflake", "Freeze", wide, "Freeze the top row").on_click(cx.listener(|e, _, _, cx| {
                            let frozen = e.store.read(cx).doc.as_ref().and_then(|d| e.store.read(cx).page_index().and_then(|i| d.pages[i].sheet().map(|s| s.freeze_rows))).unwrap_or(0);
                            e.store.update(cx, |s, cx| s.run("sheet.freeze", json!({ "rows": if frozen > 0 { 0 } else { 1 } }), cx));
                        }))
                        .into_any_element(),
                    ],
                    cx,
                ))
                .into_any_element()
            }
            PageKind::Deck => {
                let dv = self.deck.clone();
                let deck_do = {
                    let dv = dv.clone();
                    move |kind: &'static str| {
                        let dv = dv.clone();
                        move |_: &gpui::ClickEvent, w: &mut Window, cx: &mut App| {
                            dv.update(cx, |v, cx| v.add_shape(kind, cx));
                            crate::views::focus(&dv, w, cx);
                        }
                    }
                };
                let layout_menu = {
                    let dv = dv.clone();
                    let store = self.store.clone();
                    move |window: &mut Window, cx: &mut App| {
                        let pos = window.mouse_position();
                        let entries = folio_core::deck::SlideLayout::ALL
                            .iter()
                            .map(|l| {
                                let dv = dv.clone();
                                let id = l.id();
                                MenuItem::new(l.label(), move |w, cx| {
                                    dv.update(cx, |v, cx| v.add_slide(id, cx));
                                    crate::views::focus(&dv, w, cx);
                                })
                                .entry()
                            })
                            .collect();
                        store.update(cx, |s, cx| s.open_menu(pos, entries, cx));
                    }
                };
                let theme_menu = {
                    let store = self.store.clone();
                    move |window: &mut Window, cx: &mut App| {
                        let pos = window.mouse_position();
                        let entries = folio_core::deck::DeckTheme::NAMES
                            .iter()
                            .map(|n| {
                                let n = n.to_string();
                                MenuItem::new(n[..1].to_uppercase() + &n[1..], move |_, cx| cx.store().update(cx, |s, cx| s.run("deck.setTheme", json!({ "theme": n }), cx))).entry()
                            })
                            .collect();
                        store.update(cx, |s, cx| s.open_menu(pos, entries, cx));
                    }
                };
                let d2 = dv.clone();
                let fmt = move |key: &'static str| {
                    let d = d2.clone();
                    move |_: &gpui::ClickEvent, w: &mut Window, cx: &mut App| {
                        d.update(cx, |v, cx| v.toggle(key, cx));
                        crate::views::focus(&d, w, cx);
                    }
                };
                let d3 = dv.clone();
                let para = move |p: serde_json::Value| {
                    let d = d3.clone();
                    move |_: &gpui::ClickEvent, w: &mut Window, cx: &mut App| {
                        d.update(cx, |v, cx| v.paragraph(p.clone(), cx));
                        crate::views::focus(&d, w, cx);
                    }
                };
                bar.child(Button::new("new-slide", "New slide").with_icon("plus").icon_after("chevron-down").small().on_click(move |_, w, cx| layout_menu(w, cx)))
                    .child(group(
                        [
                            tool("text", "type", "Text", true, "Text box").on_click(deck_do("text")).into_any_element(),
                            tool("rect", "square", "Rectangle", false, "Rectangle").on_click(deck_do("rect")).into_any_element(),
                            tool("ellipse", "circle", "Ellipse", false, "Ellipse").on_click(deck_do("ellipse")).into_any_element(),
                            tool("tri", "triangle", "Triangle", false, "Triangle").on_click(deck_do("triangle")).into_any_element(),
                            tool("line", "minus", "Line", false, "Line").on_click(deck_do("line")).into_any_element(),
                            tool("arrow", "arrow-right", "Arrow", false, "Arrow").on_click(deck_do("arrow")).into_any_element(),
                        ],
                        cx,
                    ))
                    .child(group(
                        [
                            tool("img", "image", "Picture", wide, "Picture").on_click(cx.listener(|e, _, _, cx| e.insert_picture(cx))).into_any_element(),
                            tool("chart", "chart-column", "Chart", wide, "A live chart of a sheet range").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Insert { what: "chart".into() }, cx))).into_any_element(),
                            tool("table", "table", "Table", wide, "A table (or a live table of a sheet range)").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Insert { what: "table".into() }, cx))).into_any_element(),
                        ],
                        cx,
                    ))
                    .child(group(
                        [
                            tool("b", "bold", "Bold", false, crate::actions::tip("Bold", &Bold)).on_click(fmt("bold")).into_any_element(),
                            tool("i", "italic", "Italic", false, crate::actions::tip("Italic", &Italic)).on_click(fmt("italic")).into_any_element(),
                            tool("u", "underline", "Underline", false, crate::actions::tip("Underline", &Underline)).on_click(fmt("underline")).into_any_element(),
                        ],
                        cx,
                    ))
                    .child(group(
                        [
                            tool("al", "align-left", "Left", false, "Align left").on_click(para(json!({ "align": "left" }))).into_any_element(),
                            tool("ac", "align-center", "Centre", false, "Centre").on_click(para(json!({ "align": "center" }))).into_any_element(),
                            tool("ar", "align-right", "Right", false, "Align right").on_click(para(json!({ "align": "right" }))).into_any_element(),
                            tool("lb", "list", "Bullets", false, "Bulleted list").on_click(para(json!({ "list": "bullet" }))).into_any_element(),
                        ],
                        cx,
                    ))
                    .child(Button::new("theme", "Theme").with_icon("palette").icon_after("chevron-down").small().on_click(move |_, w, cx| theme_menu(w, cx)))
                    .child(div().flex_1())
                    .child(Button::new("present", "Present").with_icon("play").small().primary().tooltip(crate::actions::tip("Present", &Present)).on_click(|_, w, cx| w.dispatch_action(Box::new(Present), cx)))
                    .into_any_element()
            }
        }
    }

    fn insert_picture(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let kind = store.read(cx).page_kind();
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Insert a picture".into()) });
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.first()
            {
                let cmd = if kind == Some(PageKind::Deck) { "deck.addImage" } else { "doc.insertImage" };
                store.update(cx, |s, cx| s.run(cmd, json!({ "path": p }), cx));
            }
        })
        .detach();
    }

    /// The work area's title: its name, what it shows (mono), zoom.
    fn work_title(&mut self, kind: PageKind, name: String, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let zoom = s.zoom;
        let info: String = match kind {
            PageKind::Doc => {
                let (page, pages) = self.doc.read(cx).current_page(cx);
                let words = s.doc.as_ref().and_then(|d| s.page_index().and_then(|i| d.pages[i].doc().map(|t| t.word_count()))).unwrap_or(0);
                let setup = s.doc.as_ref().and_then(|d| s.page_index().and_then(|i| d.pages[i].doc().map(|t| t.setup.size_name().to_uppercase()))).unwrap_or_default();
                format!("{setup} · PAGE {page} OF {pages} · {words} WORDS")
            }
            PageKind::Sheet => {
                let v = s.view();
                let r = v.range();
                let sel = if r.rows() > 1 || r.cols() > 1 { r.a1() } else { v.cell.a1() };
                match self.sheet.read(cx).stats(cx) {
                    Some(st) => format!("{sel} · {}", st.to_uppercase()),
                    None => sel,
                }
            }
            PageKind::Deck => {
                let n = s.doc.as_ref().and_then(|d| s.page_index().and_then(|i| d.pages[i].deck().map(|d| d.slides.len()))).unwrap_or(0);
                format!("SLIDE {} OF {n} · 16:9", s.view().slide + 1)
            }
        };
        let kind_label = match kind {
            PageKind::Doc => "Document",
            PageKind::Sheet => "Sheet",
            PageKind::Deck => "Deck",
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(32.))
            .px(px(12.))
            .border_b_1()
            .border_color(t.line)
            .bg(t.bg_raised)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_w_0()
                    .child(icon(kind_icon(kind)).text_color(t.text_2))
                    .child(panel_title(name))
                    .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(kind_label.to_uppercase()))
                    .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_2).truncate().child(info)),
            )
            .child(group(
                [
                    tool("zoom-out", "zoom-out", "Zoom out", false, crate::actions::tip("Zoom out", &ZoomOut)).on_click(|_, w, cx| w.dispatch_action(Box::new(ZoomOut), cx)).into_any_element(),
                    div()
                        .id("zoom-level")
                        .px(px(8.))
                        .font_family(MONO)
                        .text_size(px(sz::XS))
                        .cursor_pointer()
                        .on_click(|_, w, cx| w.dispatch_action(Box::new(ZoomReset), cx))
                        .child(format!("{:.0} %", zoom * 100.))
                        .into_any_element(),
                    tool("zoom-in", "zoom-in", "Zoom in", false, crate::actions::tip("Zoom in", &ZoomIn)).on_click(|_, w, cx| w.dispatch_action(Box::new(ZoomIn), cx)).into_any_element(),
                ],
                cx,
            ))
            .into_any_element()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let Some(doc) = s.doc.clone() else { return div().into_any_element() };
        let pi = s.page_index();
        let (sidebar, inspector, agent) = (s.sidebar_open, s.inspector_open, s.agent_open && s.settings.agent.enabled);
        let page = pi.map(|i| (doc.pages[i].id.clone(), doc.pages[i].kind(), doc.pages[i].name.clone()));
        // Focus the view when the page shown changes kind or page.
        if let Some((id, kind, _)) = &page
            && self.shown.as_ref().is_none_or(|(sid, _)| sid != id)
        {
            self.shown = Some((id.clone(), *kind));
            let modal = s.dialog.is_some() || s.setup;
            if !modal {
                self.focus_view(window, cx);
            }
        }
        let narrow = f32::from(window.viewport_size().width) < 1280.;
        let title = self.title_bar(window, cx);
        let side = sidebar.then(|| self.sidebar(window, cx));
        let center = match &page {
            Some((_, kind, name)) => {
                let toolbar = self.toolbar(*kind, window, cx);
                let wt = self.work_title(*kind, name.clone(), cx);
                let view: AnyElement = match kind {
                    PageKind::Doc => self.doc.clone().into_any_element(),
                    PageKind::Sheet => self.sheet.clone().into_any_element(),
                    PageKind::Deck => self.deck.clone().into_any_element(),
                };
                div().flex_1().min_w_0().h_full().flex().flex_col().child(toolbar).child(wt).child(div().flex_1().min_h_0().child(view)).into_any_element()
            }
            None => div().flex_1().flex().items_center().justify_center().text_color(t.text_2).child("This file has no pages. Add a document, a sheet or a deck from the sidebar.").into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(title)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .children(side)
                    .child(center)
                    .when(inspector && !(agent && narrow), |d| d.child(div().flex_none().w(px(INSPECTOR_W)).h_full().child(self.inspector.clone())))
                    .when(agent, |d| d.child(div().flex_none().w(px(AGENT_W)).h_full().border_l_1().border_color(t.line).child(self.agent.clone()))),
            )
            .into_any_element()
    }
}
