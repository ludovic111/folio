//! The first-run setup, over the whole window: where the person comes from (with the formats
//! folio opens from each suite), how the agent runs (lsuite AI first: no setup), and the name
//! on comments. One screen, three choices, then folio.

use gpui::{Context, Entity, FontWeight, MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px};
use serde_json::json;

use crate::store::{Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::{Button, GlassExt, caps, icon, logo};

pub struct Onboarding {
    store: Entity<Store>,
    from: String,
    provider: String,
    name: Entity<TextInput>,
    _subs: Vec<Subscription>,
}

impl Onboarding {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let author = store.read(cx).settings.author();
        let name = cx.new(|cx| {
            let mut i = TextInput::new(cx).placeholder("Your name");
            i.set_text(author, cx);
            i
        });
        let subs = vec![cx.observe(&store, |_, _, cx| cx.notify()), cx.subscribe_in(&name, window, |_, _, _: &InputEvent, _, _| {})];
        Self { store, from: String::new(), provider: "lsuite".into(), name, _subs: subs }
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        let author = self.name.read(cx).text().trim().to_string();
        let params = json!({ "comingFrom": if self.from.is_empty() { "none" } else { self.from.as_str() }, "agent": self.provider != "none", "provider": if self.provider == "none" { "lsuite" } else { self.provider.as_str() }, "author": author });
        self.store.update(cx, |s, cx| {
            s.run("app.finishOnboarding", params, cx);
            s.close_setup(cx);
        });
    }
}

impl Render for Onboarding {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let account = self.store.read(cx).account.clone();
        let signed = account["signedIn"] == true;
        let suites: [(&str, &str, [&str; 3]); 4] = [
            ("office", "Microsoft Office", ["word", "excel", "powerpoint"]),
            ("google", "Google Workspace", ["google-docs", "google-sheets", "google-slides"]),
            ("apple", "Apple iWork", ["pages", "numbers", "keynote"]),
            ("libreoffice", "LibreOffice", ["libreoffice-writer", "libreoffice-calc", "libreoffice-impress"]),
        ];
        let hint = match self.from.as_str() {
            "office" => "Open your .docx, .xlsx and .pptx files directly; Export writes them back.",
            "google" => "In Google Docs, Sheets or Slides: File › Download › Microsoft Word, Excel or PowerPoint, then open the file here.",
            "apple" => "In Pages, Numbers or Keynote: File › Export To › Word, Excel or PowerPoint, then open the file here (folio doesn't read .pages, .numbers or .key).",
            "libreoffice" => "Open .odt, .ods and .odp files (or their Office formats) directly.",
            _ => "folio opens Word, Excel, PowerPoint, OpenDocument, CSV and Markdown files.",
        };
        let providers: [(&str, &str, &str); 5] = [
            ("lsuite", "lsuite AI", "No setup. Sign in and your agent works."),
            ("claude-code", "Claude Code", "Uses the Claude Code you have installed."),
            ("codex", "Codex", "Uses the Codex CLI you have installed."),
            ("anthropic", "An API key", "Anthropic, OpenAI, OpenRouter, Mistral… (Settings › Agent)."),
            ("none", "No agent", "Hide the Agent panel. Change it any time."),
        ];
        div()
            .id("setup")
            .occlude()
            .absolute()
            .inset_0()
            .bg(t.scrim)
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div().relative().child(
                    div()
                        .w(px(860.))
                        .flex()
                        .flex_col()
                        .gap(px(22.))
                        .p(px(28.))
                        .glass(t.glass3)
                        .shadow(t.glass_shadow())
                        .child(div().flex().items_center().gap(px(12.)).child(icon("mark").size(px(30.))).child(div().flex().flex_col().child(div().text_size(px(sz::XL)).font_weight(FontWeight::BOLD).child("Welcome to folio")).child(div().text_color(t.text_2).child("Documents, sheets and slides in one file. Three questions and you're in."))))
                        .child(
                            div().flex().flex_col().gap(px(10.)).child(caps("1 · Coming from", cx)).child(div().flex().gap(px(8.)).children(suites.into_iter().map(|(id, name, logos)| {
                                let on = self.from == id;
                                div()
                                    .id(SharedString::from(format!("from-{id}")))
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .p(px(10.))
                                    .border_1()
                                    .border_color(if on { t.accent } else { t.line_strong })
                                    .when(on, |d| d.border_2().bg(t.accent_soft))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |o, _, _, cx| {
                                        o.from = id.to_string();
                                        cx.notify();
                                    }))
                                    .child(div().flex().gap(px(6.)).children(logos.into_iter().map(|l| logo(l, px(26.)))))
                                    .child(div().font_weight(FontWeight::MEDIUM).child(name))
                            })))
                            .child(div().text_size(px(sz::SM)).text_color(t.text_2).child(hint)),
                        )
                        .child(
                            div().flex().flex_col().gap(px(10.)).child(caps("2 · Your agent", cx)).child(div().flex().flex_col().gap(px(4.)).children(providers.into_iter().map(|(id, name, line)| {
                                let on = self.provider == id;
                                div()
                                    .id(SharedString::from(format!("prov-{id}")))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .px(px(10.))
                                    .py(px(6.))
                                    .cursor_pointer()
                                    .when(on, |d| d.bg(t.accent).text_color(t.text_on_accent))
                                    .when(!on, |d| d.hover(|h| h.bg(t.hover)))
                                    .on_click(cx.listener(move |o, _, _, cx| {
                                        o.provider = id.to_string();
                                        cx.notify();
                                    }))
                                    .child(if id == "none" || id == "anthropic" { div().size(px(22.)).flex().items_center().justify_center().child(icon(if id == "none" { "x" } else { "key-round" })).into_any_element() } else { logo(id, px(22.)).into_any_element() })
                                    .child(div().w(px(140.)).font_weight(FontWeight::MEDIUM).child(name))
                                    .child(div().flex_1().text_size(px(sz::SM)).child(line))
                                    .when(id == "lsuite", |d| {
                                        d.child(if signed {
                                            div().font_family(MONO).text_size(px(sz::XS)).child(account["summary"].as_str().unwrap_or("SIGNED IN").to_uppercase()).into_any_element()
                                        } else {
                                            Button::new("setup-signin", "Sign in").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signIn", json!({}), cx))).into_any_element()
                                        })
                                    })
                            }))),
                        )
                        .child(div().flex().flex_col().gap(px(10.)).child(caps("3 · Your name on comments and changes", cx)).child(div().w(px(320.)).child(self.name.clone())))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .child(Button::new("setup-skip", "Skip").small().on_click(cx.listener(|o, _, _, cx| {
                                    o.from.clear();
                                    o.finish(cx);
                                })))
                                .child(Button::new("setup-done", "Start using folio").with_icon("arrow-right").primary().on_click(cx.listener(|o, _, _, cx| o.finish(cx)))),
                        ),
                )
                .child(crate::ui::grain::brackets(14., -9., t.text_3)),
            )
    }
}
