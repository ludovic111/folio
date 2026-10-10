//! The home screen: start a document, a sheet, a deck or a template; open a recent file; or
//! bring files over from Microsoft Office, Google Workspace, Apple iWork or LibreOffice.

use gpui::{AnyElement, App, Context, Entity, FontWeight, Render, SharedString, Window, div, prelude::*, px};
use serde_json::json;

use crate::store::{Dialog, Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, caps, icon, logo, panel_title};

pub struct Home {
    store: Entity<Store>,
}

impl Home {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store }
    }
}

/// A big tile to start something new.
fn new_tile(id: &'static str, ic: &'static str, title: &'static str, line: &'static str, on: impl Fn(&mut Window, &mut App) + 'static, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    div()
        .id(id)
        .flex_1()
        .min_w(px(150.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(16.))
        .glass(t.glass2)
        .cursor_pointer()
        .hover(|d| d.border_color(t.accent))
        .on_click(move |_, w, cx| on(w, cx))
        .child(div().size(px(40.)).flex().items_center().justify_center().bg(t.accent).text_color(t.text_on_accent).child(icon(ic).size(px(20.))))
        .child(div().text_size(px(sz::LG)).font_weight(FontWeight::SEMIBOLD).child(title))
        .child(div().text_size(px(sz::SM)).text_color(t.text_2).child(line))
        .into_any_element()
}

/// The suites people come from and what to open from each.
pub fn coming_from(cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let suites = [
        ("Microsoft Office", [("word", "Word", ".docx"), ("excel", "Excel", ".xlsx"), ("powerpoint", "PowerPoint", ".pptx")]),
        ("Google Workspace", [("google-docs", "Docs", ".docx download"), ("google-sheets", "Sheets", ".xlsx download"), ("google-slides", "Slides", ".pptx download")]),
        ("Apple iWork", [("pages", "Pages", "export to Word"), ("numbers", "Numbers", "export to Excel"), ("keynote", "Keynote", "export to PowerPoint")]),
        ("LibreOffice", [("libreoffice-writer", "Writer", ".odt, .docx"), ("libreoffice-calc", "Calc", ".ods, .xlsx"), ("libreoffice-impress", "Impress", ".odp, .pptx")]),
    ];
    div()
        .flex()
        .flex_wrap()
        .gap(px(10.))
        .children(suites.into_iter().map(|(suite, apps)| {
            div()
                .flex_1()
                .min_w(px(220.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(12.))
                .border_1()
                .border_color(t.line_strong)
                .child(div().font_family(MONO).text_size(px(10.5)).text_color(t.text_2).child(suite.to_uppercase()))
                .children(apps.into_iter().map(|(id, name, how)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(logo(id, px(22.)))
                        .child(div().font_weight(FontWeight::MEDIUM).child(name))
                        .child(div().flex_1())
                        .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(how))
                }))
        }))
        .into_any_element()
}

impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let recent = self.store.read(cx).recent.clone();
        let controls = crate::ui::window_controls(window, cx);
        let templates = folio_control::templates::list();
        let mac = cfg!(target_os = "macos") && !window.is_fullscreen();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                // A bare title bar to drag the window by.
                div()
                    .id("home-bar")
                    .h(px(46.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(if mac { 84. } else { 16. }))
                    .window_control_area(gpui::WindowControlArea::Drag)
                    .on_mouse_down(gpui::MouseButton::Left, |e, window, _| {
                        if e.click_count == 2 {
                            window.titlebar_double_click();
                        } else {
                            window.start_window_move();
                        }
                    })
                    .child(div().flex().items_center().gap(px(10.)).child(icon("mark").size(px(20.))).child(div().font_weight(FontWeight::BOLD).text_size(px(sz::LG)).child("folio")).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(format!("BETA {}", env!("CARGO_PKG_VERSION")))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .pr(px(if controls.is_some() { 0. } else { 16. }))
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(Button::icon("home-plugins", "puzzle", "Plugins").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Plugins, cx))))
                            .child(Button::icon("home-settings", "settings", "Settings").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Settings { section: None }, cx))))
                            .children(controls),
                    ),
            )
            .child(
                div().id("home-scroll").flex_1().min_h_0().overflow_y_scroll().child(
                    div().max_w(px(1100.)).mx_auto().px(px(32.)).py(px(24.)).flex().flex_col().gap(px(28.))
                        .child(
                            div().flex().flex_col().gap(px(6.))
                                .child(div().text_size(px(sz::XXL)).font_weight(FontWeight::BOLD).child("Documents, sheets and slides. One file."))
                                .child(div().text_color(t.text_2).text_size(px(sz::MD)).child("A folio file holds any mix of pages. A table in a report or a chart on a slide can show a sheet's numbers live.")),
                        )
                        .child(
                            div().flex().flex_col().gap(px(10.)).child(caps("New", cx)).child(
                                div().flex().flex_wrap().gap(px(12.))
                                    .child(new_tile("new-doc", "file-text", "Document", "Write: headings, lists, tables, pictures, comments.", |_, cx| cx.store().update(cx, |s, cx| s.run("file.new", json!({ "kind": "doc" }), cx)), cx))
                                    .child(new_tile("new-sheet", "sheet", "Sheet", "Calculate: formulas, formats, sorting, charts.", |_, cx| cx.store().update(cx, |s, cx| s.run("file.new", json!({ "kind": "sheet" }), cx)), cx))
                                    .child(new_tile("new-deck", "presentation", "Deck", "Present: layouts, shapes, pictures, notes.", |_, cx| cx.store().update(cx, |s, cx| s.run("file.new", json!({ "kind": "deck" }), cx)), cx)),
                            ),
                        )
                        .child(
                            div().flex().flex_col().gap(px(10.)).child(caps("Templates", cx)).child(
                                div().flex().flex_wrap().gap(px(8.)).children(templates.as_array().cloned().unwrap_or_default().into_iter().map(|tp| {
                                    let id = tp["id"].as_str().unwrap_or("").to_string();
                                    let name = tp["name"].as_str().unwrap_or("").to_string();
                                    let kinds: Vec<String> = tp["kinds"].as_array().map(|k| k.iter().filter_map(|x| x.as_str().map(str::to_uppercase)).collect()).unwrap_or_default();
                                    let desc = tp["description"].as_str().unwrap_or("").to_string();
                                    let (id2, name2) = (id.clone(), name.clone());
                                    div()
                                        .id(SharedString::from(format!("tpl-{id}")))
                                        .w(px(250.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .p(px(12.))
                                        .border_1()
                                        .border_color(t.line_strong)
                                        .bg(t.bg_raised.opacity(0.6))
                                        .cursor_pointer()
                                        .hover(|d| d.border_color(t.accent))
                                        .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("file.new", json!({ "template": id2, "title": name2 }), cx)))
                                        .child(div().flex().justify_between().child(div().font_weight(FontWeight::SEMIBOLD).child(name)).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(kinds.join(" + "))))
                                        .child(div().text_size(px(sz::SM)).text_color(t.text_2).child(desc))
                                })),
                            ),
                        )
                        .child(
                            div().flex().flex_col().gap(px(10.))
                                .child(div().flex().items_center().justify_between().child(caps("Recent", cx)).child(Button::new("open", "Open or import…").with_icon("folder-open").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Open, cx)))))
                                .when(recent.is_empty(), |d| d.child(div().text_color(t.text_3).child("Files you open show here.")))
                                .child(div().flex().flex_col().children(recent.into_iter().enumerate().map(|(i, r)| {
                                    let path = r["path"].as_str().unwrap_or("").to_string();
                                    let name = r["name"].as_str().unwrap_or("").to_string();
                                    let when = r["modified"].as_str().and_then(|m| chrono::DateTime::parse_from_rfc3339(m).ok()).map(|d| d.format("%-d %b %Y %H:%M").to_string()).unwrap_or_default();
                                    let dir = std::path::Path::new(&path).parent().map(|p| p.display().to_string()).unwrap_or_default();
                                    let p2 = path.clone();
                                    div()
                                        .id(("recent", i))
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .px(px(10.))
                                        .py(px(8.))
                                        .border_b_1()
                                        .border_color(t.line)
                                        .cursor_pointer()
                                        .hover(|d| d.bg(t.hover))
                                        .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("file.open", json!({ "path": p2 }), cx)))
                                        .child(icon(if path.ends_with(".folio") { "file-text" } else { "file-input" }).text_color(t.text_2))
                                        .child(div().font_weight(FontWeight::MEDIUM).child(name))
                                        .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(dir))
                                        .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(when))
                                }))),
                        )
                        .child(div().flex().flex_col().gap(px(10.)).child(div().flex().items_baseline().gap(px(10.)).child(panel_title("Coming from another office suite?")).child(div().text_color(t.text_2).text_size(px(sz::SM)).child("Open their files; folio writes them back too."))).child(coming_from(cx))),
                ),
            )
    }
}
