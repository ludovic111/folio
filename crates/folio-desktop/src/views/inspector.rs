//! The inspector: the settings of what is selected, in titled sections. Paragraph, text and
//! page for documents; cell, sheet and charts for sheets; slide, shape and theme for decks.

use folio_core::{Align, Document, PageKind, ParaStyle};
use gpui::{AnyElement, App, Context, Entity, FontWeight, Hsla, Render, SharedString, Subscription, Window, div, prelude::*, px};
use serde_json::{Value, json};

use crate::store::{Dialog, Store, StoreExt, TextTarget};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::{Button, GlassExt, caps, panel_title, segmented, switch};

/// Colours offered for text, fills and backgrounds: greys first (the suite's look), then a few
/// plain colours for people's own documents.
pub const SWATCHES: &[&str] = &["#000000", "#404040", "#808080", "#bfbfbf", "#ffffff", "#c8291c", "#d97706", "#16a34a", "#1d4ed8", "#7c3aed"];

pub struct Inspector {
    store: Entity<Store>,
    header: Entity<TextInput>,
    footer: Entity<TextInput>,
    shape_fields: [Entity<TextInput>; 4],
    synced: Option<(u64, String)>,
    _subs: Vec<Subscription>,
}

impl Inspector {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let header = cx.new(|cx| TextInput::new(cx).placeholder("Header text ({page}, {pages}, {title})"));
        let footer = cx.new(|cx| TextInput::new(cx).placeholder("Footer text"));
        let shape_fields = [cx.new(TextInput::new), cx.new(TextInput::new), cx.new(TextInput::new), cx.new(TextInput::new)];
        let mut subs = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        for (k, input) in [("header", header.clone()), ("footer", footer.clone())] {
            subs.push(cx.subscribe_in(&input, window, move |v: &mut Self, src, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Submit | InputEvent::Blur) {
                    let text = src.read(cx).text().to_string();
                    v.store.update(cx, |s, cx| s.run("doc.setup", json!({ k: text }), cx));
                }
            }));
        }
        for (i, input) in shape_fields.clone().into_iter().enumerate() {
            subs.push(cx.subscribe_in(&input, window, move |v: &mut Self, src, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Submit | InputEvent::Blur) {
                    let Ok(val) = src.read(cx).text().trim().parse::<f32>() else { return };
                    let key = ["x", "y", "w", "h"][i];
                    let (slide, shapes) = {
                        let pv = v.store.read(cx).view();
                        (pv.slide, pv.shapes)
                    };
                    if let Some(id) = shapes.first() {
                        v.store.update(cx, |s, cx| s.run("deck.updateShape", json!({ "slide": (slide + 1).to_string(), "shape": id.to_string(), key: val }), cx));
                    }
                }
            }));
        }
        Self { store, header, footer, shape_fields, synced: None, _subs: subs }
    }

    /// Keeps the fields in step with the document (not while one is being typed in).
    fn sync(&mut self, window: &Window, doc: &Document, pi: usize, cx: &mut Context<Self>) {
        let version = self.store.read(cx).version;
        let v = self.store.read(cx).view();
        let key = format!("{pi}:{}:{:?}", v.slide, v.shapes);
        if self.synced.as_ref().is_some_and(|(ver, k)| *ver == version && *k == key) {
            return;
        }
        self.synced = Some((version, key));
        if let Some(t) = doc.pages[pi].doc() {
            for (input, val) in [(&self.header, t.setup.header.clone()), (&self.footer, t.setup.footer.clone())] {
                if !input.read(cx).is_focused(window) {
                    input.update(cx, |i, cx| i.set_text(val, cx));
                }
            }
        }
        if let Some(d) = doc.pages[pi].deck()
            && let Some(sh) = d.slides.get(v.slide).and_then(|s| v.shapes.first().and_then(|id| s.shapes.iter().find(|x| &x.id == id)))
        {
            for (input, val) in self.shape_fields.iter().zip([sh.x, sh.y, sh.w, sh.h]) {
                if !input.read(cx).is_focused(window) {
                    input.update(cx, |i, cx| i.set_text(format!("{val:.0}"), cx));
                }
            }
        }
    }
}

fn section(title: &str, cx: &App) -> gpui::Div {
    let t = cx.theme();
    div().flex().flex_col().gap(px(8.)).px(px(14.)).py(px(12.)).border_b_1().border_color(t.line).child(caps(title, cx))
}

fn swatches(id: &'static str, current: Option<&str>, on: impl Fn(&str, &mut Window, &mut App) + Clone + 'static, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    div()
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .children(SWATCHES.iter().enumerate().map(|(i, c)| {
            let on = on.clone();
            let chosen = current.is_some_and(|x| x.eq_ignore_ascii_case(c));
            div()
                .id((id, i))
                .size(px(20.))
                .bg(crate::paint::hex(c, t.text))
                .border_1()
                .border_color(if chosen { t.accent } else { t.line_strong })
                .when(chosen, |d| d.border_2())
                .cursor_pointer()
                .on_click(move |_, w, cx| on(c, w, cx))
        }))
        .child(
            div()
                .id((id, 99usize))
                .h(px(20.))
                .px(px(6.))
                .flex()
                .items_center()
                .border_1()
                .border_color(t.line_strong)
                .text_size(px(sz::XS))
                .cursor_pointer()
                .child("None")
                .on_click(move |_, w, cx| on("", w, cx)),
        )
        .into_any_element()
}

fn label_row(label: &str, child: impl IntoElement, cx: &App) -> gpui::Div {
    div().flex().items_center().justify_between().gap(px(8.)).child(div().text_color(cx.theme().text_2).child(label.to_string())).child(child)
}

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let Some(doc) = s.doc.clone() else { return div().into_any_element() };
        let Some(pi) = s.page_index() else { return div().into_any_element() };
        let view = s.view();
        let kind = doc.pages[pi].kind();
        self.sync(window, &doc, pi, cx);
        let mut body: Vec<AnyElement> = vec![];
        match kind {
            PageKind::Doc => {
                let td = doc.pages[pi].doc().unwrap();
                let sel = view.text.filter(|t| t.target == TextTarget::Doc);
                let block = sel.map(|s| s.ordered().0.block).unwrap_or(0);
                let para = td.blocks.get(block).and_then(|b| b.para()).cloned();
                let style = para.as_ref().map(|p| p.style).unwrap_or_default();
                let align = para.as_ref().map(|p| p.align).unwrap_or_default();
                let store = self.store.clone();
                let para_cmd = move |p: Value| {
                    let store = store.clone();
                    move |cx: &mut App| {
                        let (a, b) = store.read(cx).view().text.map(|t| t.ordered()).map(|(a, b)| (a.block, b.block)).unwrap_or((0, 0));
                        let mut p = p.clone();
                        p["from"] = json!(a);
                        p["to"] = json!(b);
                        store.update(cx, |s, cx| s.run("text.paragraph", p, cx));
                    }
                };
                // Paragraph styles, each shown in its own look.
                let styles = div().flex().flex_col().gap(px(2.)).children(ParaStyle::ALL.iter().map(|ps| {
                    let chosen = *ps == style;
                    let spec = ps.spec();
                    let f = para_cmd(json!({ "style": ps.id() }));
                    div()
                        .id(SharedString::from(format!("style-{}", ps.id())))
                        .px(px(8.))
                        .py(px(4.))
                        .cursor_pointer()
                        .when(chosen, |d| d.bg(t.accent).text_color(t.text_on_accent))
                        .when(!chosen, |d| d.hover(|h| h.bg(t.hover)))
                        .text_size(px((spec.size * 1.1).clamp(11.0, 19.0)))
                        .when(spec.bold, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .when(spec.italic, |d| d.italic())
                        .when(spec.family == folio_core::text::Family::Mono, |d| d.font_family(MONO))
                        .child(ps.label())
                        .on_click(move |_, _, cx| f(cx))
                }));
                let a1 = para_cmd.clone();
                body.push(
                    section("Paragraph", cx)
                        .child(styles)
                        .child(segmented(
                            "align",
                            vec![(Align::Left, "Left".into()), (Align::Center, "Centre".into()), (Align::Right, "Right".into()), (Align::Justify, "Justify".into())],
                            align,
                            move |a, _, cx| a1(json!({ "align": a.id() }))(cx),
                            cx,
                        ))
                        .into_any_element(),
                );
                // Text: family, size, colour of the selection.
                let store2 = self.store.clone();
                let fmt = move |p: Value| {
                    let store = store2.clone();
                    move |cx: &mut App| {
                        let Some(sel) = store.read(cx).view().text else { return };
                        let (a, b) = sel.ordered();
                        let mut p = p.clone();
                        p["from"] = json!({ "block": a.block, "offset": a.offset });
                        p["to"] = json!({ "block": b.block, "offset": b.offset });
                        if a.cell.is_none() && b.cell.is_none() {
                            store.update(cx, |s, cx| s.run("text.format", p, cx));
                        }
                    }
                };
                let f2 = fmt.clone();
                let f3 = fmt.clone();
                body.push(
                    section("Text", cx)
                        .child(div().flex().gap(px(4.)).children(["sans", "serif", "mono", "display"].into_iter().map(|fam| {
                            let f = fmt(json!({ "font": fam }));
                            Button::new(SharedString::from(format!("fam-{fam}")), fam[..1].to_uppercase() + &fam[1..]).small().full_width().on_click(move |_, _, cx| f(cx))
                        })))
                        .child(div().flex().gap(px(4.)).children([9, 11, 14, 18, 24, 36].into_iter().map(|size| {
                            let f = f2(json!({ "size": size }));
                            Button::new(SharedString::from(format!("size-{size}")), size.to_string()).small().full_width().on_click(move |_, _, cx| f(cx))
                        })))
                        .child(swatches("text-color", None, move |c, _, cx| f3(json!({ "color": c }))(cx), cx))
                        .into_any_element(),
                );
                // Page setup.
                let setup = td.setup.clone();
                let size_name = setup.size_name();
                body.push(
                    section("Page", cx)
                        .child(segmented("paper", vec![("a4", "A4".into()), ("letter", "Letter".into()), ("legal", "Legal".into())], size_name, |v, _, cx| cx.store().update(cx, |s, cx| s.run("doc.setup", json!({ "size": v }), cx)), cx))
                        .child(segmented("orient", vec![(false, "Portrait".into()), (true, "Landscape".into())], setup.landscape(), |v, _, cx| cx.store().update(cx, |s, cx| s.run("doc.setup", json!({ "orientation": if *v { "landscape" } else { "portrait" } }), cx)), cx))
                        .child(segmented(
                            "margins",
                            vec![(36, "Narrow".into()), (72, "Normal".into()), (108, "Wide".into())],
                            setup.margin_left.round() as i32,
                            |v, _, cx| cx.store().update(cx, |s, cx| s.run("doc.setup", json!({ "margins": v }), cx)),
                            cx,
                        ))
                        .child(div().flex().flex_col().gap(px(4.)).child(div().text_color(t.text_2).child("Header")).child(self.header.clone()))
                        .child(div().flex().flex_col().gap(px(4.)).child(div().text_color(t.text_2).child("Footer")).child(self.footer.clone()))
                        .child(Button::new("page-setup", "More page settings…").small().full_width().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::PageSetup, cx))))
                        .into_any_element(),
                );
                // Review: tracked changes and comments.
                let open: Vec<_> = td.comments.iter().filter(|c| !c.resolved).cloned().collect();
                let tracking = td.track_changes;
                body.push(
                    section("Review", cx)
                        .child(switch("track", "Track changes", tracking, |on, _, cx| cx.store().update(cx, |s, cx| s.run("doc.trackChanges", json!({ "on": on }), cx)), cx))
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(Button::new("accept-all", "Accept all").small().full_width().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("doc.resolveChanges", json!({ "accept": true }), cx))))
                                .child(Button::new("reject-all", "Reject all").small().full_width().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("doc.resolveChanges", json!({ "accept": false }), cx)))),
                        )
                        .when(open.is_empty(), |d| d.child(div().text_color(t.text_3).text_size(px(sz::SM)).child("No open comments.")))
                        .children(open.into_iter().enumerate().map(|(i, c)| {
                            let id = c.id.to_string();
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .p(px(8.))
                                .border_1()
                                .border_color(t.line_strong)
                                .child(div().flex().justify_between().child(div().font_weight(FontWeight::SEMIBOLD).child(c.author.clone())).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(c.at.format("%-d %b %H:%M").to_string())))
                                .child(div().text_size(px(sz::SM)).child(c.text.clone()))
                                .children(c.replies.iter().map(|r| div().pl(px(8.)).border_l_1().border_color(t.line_strong).text_size(px(sz::SM)).child(format!("{}: {}", r.author, r.text))))
                                .child(Button::new(("resolve", i), "Resolve").small().on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("doc.resolveComment", json!({ "comment": id }), cx))))
                        }))
                        .into_any_element(),
                );
            }
            PageKind::Sheet => {
                let sh = doc.pages[pi].sheet().unwrap();
                let cell = sh.cell(view.cell).cloned().unwrap_or_default();
                let r = view.range().a1();
                let fmt = move |p: Value| {
                    let r = r.clone();
                    move |cx: &mut App| {
                        let mut p = p.clone();
                        p["range"] = json!(r);
                        cx.store().update(cx, |s, cx| s.run("sheet.format", p, cx));
                    }
                };
                let f1 = fmt.clone();
                let f2 = fmt.clone();
                let f3 = fmt.clone();
                let presets = div().flex().flex_col().gap(px(1.)).children(folio_calc::PRESETS.iter().enumerate().map(|(i, (label, code))| {
                    let chosen = cell.format.number.as_deref() == Some(code) || (cell.format.number.is_none() && *code == "General");
                    let f = fmt(json!({ "number": code }));
                    let example = folio_calc::format_value(&folio_calc::Value::Number(if code.contains('y') || code.contains('h') { 46301.5625 } else if code.contains('%') { 0.1234 } else { -1234.5 }), Some(code));
                    div()
                        .id(("preset", i))
                        .flex()
                        .justify_between()
                        .px(px(8.))
                        .py(px(3.))
                        .cursor_pointer()
                        .when(chosen, |d| d.bg(t.accent).text_color(t.text_on_accent))
                        .when(!chosen, |d| d.hover(|h| h.bg(t.hover)))
                        .child(label.to_string())
                        .child(div().font_family(MONO).text_size(px(sz::XS)).child(example))
                        .on_click(move |_, _, cx| f(cx))
                }));
                body.push(
                    section("Cell", cx)
                        .child(div().flex().justify_between().child(div().font_family(MONO).child(view.cell.a1())).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(if cell.is_formula() { "FORMULA" } else if cell.input.is_empty() { "EMPTY" } else { "VALUE" })))
                        .when(cell.is_formula(), |d| d.child(div().font_family(MONO).text_size(px(sz::SM)).p(px(6.)).bg(t.bg_sunken).child(cell.input.clone())))
                        .child(caps("Number format", cx))
                        .child(presets)
                        .into_any_element(),
                );
                body.push(
                    section("Look", cx)
                        .child(segmented("cell-align", vec![(None, "Auto".into()), (Some(Align::Left), "Left".into()), (Some(Align::Center), "Centre".into()), (Some(Align::Right), "Right".into())], cell.format.align, move |a, _, cx| f1(json!({ "align": a.map(|x| x.id()).unwrap_or("") }))(cx), cx))
                        .child(label_row("Fill", div(), cx))
                        .child(swatches("fill", cell.format.fill.as_deref(), move |c, _, cx| f2(json!({ "fill": c }))(cx), cx))
                        .child(label_row("Text colour", div(), cx))
                        .child(swatches("color", cell.format.color.as_deref(), move |c, _, cx| f3(json!({ "color": c }))(cx), cx))
                        .child(
                            div().flex().gap(px(4.)).children([("all", "All borders"), ("b", "Bottom"), ("none", "None")].into_iter().map(|(b, label)| {
                                let f = fmt(json!({ "border": b }));
                                Button::new(SharedString::from(format!("border-{b}")), label).small().full_width().on_click(move |_, _, cx| f(cx))
                            })),
                        )
                        .into_any_element(),
                );
                let (fr, fc, grid) = (sh.freeze_rows, sh.freeze_cols, sh.gridlines);
                body.push(
                    section("Sheet", cx)
                        .child(switch("freeze-row", "Freeze the top row", fr > 0, move |on, _, cx| cx.store().update(cx, |s, cx| s.run("sheet.freeze", json!({ "rows": if on { 1 } else { 0 }, "columns": fc }), cx)), cx))
                        .child(switch("freeze-col", "Freeze the first column", fc > 0, move |on, _, cx| cx.store().update(cx, |s, cx| s.run("sheet.freeze", json!({ "rows": fr, "columns": if on { 1 } else { 0 } }), cx)), cx))
                        .child(switch("grid", "Gridlines", grid, |on, _, cx| cx.store().update(cx, |s, cx| s.run("sheet.setGridlines", json!({ "on": on }), cx)), cx))
                        .when(sh.filter.is_some(), |d| d.child(Button::new("clear-filter", "Remove the filter").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("sheet.filter", json!({ "clear": true }), cx)))))
                        .into_any_element(),
                );
                if !sh.charts.is_empty() {
                    body.push(
                        section("Charts", cx)
                            .children(sh.charts.iter().enumerate().map(|(i, c)| {
                                let id = c.chart.clone();
                                let cid = c.id.to_string();
                                let cid2 = c.id.to_string();
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(div().flex().justify_between().child(div().child(if id.title.is_empty() { id.kind.label().to_string() } else { id.title.clone() })).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(id.source.clone())))
                                    .child(segmented(SharedString::from(format!("chart-kind-{i}")), folio_core::ChartKind::ALL.iter().map(|k| (*k, SharedString::from(k.label()))).collect(), id.kind, move |k, _, cx| cx.store().update(cx, |s, cx| s.run("sheet.updateChart", json!({ "chart": cid, "kind": k.id() }), cx)), cx))
                                    .child(Button::new(("remove-chart", i), "Remove").small().danger().on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("sheet.removeChart", json!({ "chart": cid2 }), cx))))
                            }))
                            .into_any_element(),
                    );
                }
            }
            PageKind::Deck => {
                let deck = doc.pages[pi].deck().unwrap();
                let si = view.slide.min(deck.slides.len().saturating_sub(1));
                if let Some(slide) = deck.slides.get(si) {
                    let n = (si + 1).to_string();
                    let n2 = n.clone();
                    let n3 = n.clone();
                    body.push(
                        section("Slide", cx)
                            .child(div().flex().flex_col().gap(px(1.)).children(folio_core::deck::SlideLayout::ALL.iter().map(|l| {
                                let chosen = *l == slide.layout;
                                let id = l.id();
                                let n = n.clone();
                                div()
                                    .id(SharedString::from(format!("layout-{id}")))
                                    .px(px(8.))
                                    .py(px(3.))
                                    .cursor_pointer()
                                    .when(chosen, |d| d.bg(t.accent).text_color(t.text_on_accent))
                                    .when(!chosen, |d| d.hover(|h| h.bg(t.hover)))
                                    .child(l.label())
                                    .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("deck.setSlide", json!({ "slide": n, "layout": id }), cx)))
                            })))
                            .child(label_row("Background", div(), cx))
                            .child(swatches("slide-bg", slide.background.as_deref(), move |c, _, cx| cx.store().update(cx, |s, cx| s.run("deck.setSlide", json!({ "slide": n2, "background": c }), cx)), cx))
                            .child(switch("hide-slide", "Skip when presenting", slide.hidden, move |on, _, cx| cx.store().update(cx, |s, cx| s.run("deck.setSlide", json!({ "slide": n3, "hidden": on }), cx)), cx))
                            .into_any_element(),
                    );
                    if let Some(sh) = view.shapes.first().and_then(|id| slide.shapes.iter().find(|x| &x.id == id)) {
                        let id = sh.id.to_string();
                        let slide_n = (si + 1).to_string();
                        let upd = move |p: Value| {
                            let (id, slide_n) = (id.clone(), slide_n.clone());
                            move |cx: &mut App| {
                                let mut p = p.clone();
                                p["slide"] = json!(slide_n);
                                p["shape"] = json!(id);
                                cx.store().update(cx, |s, cx| s.run("deck.updateShape", p, cx));
                            }
                        };
                        let (u1, u2, u3, u4) = (upd.clone(), upd.clone(), upd.clone(), upd.clone());
                        let labels = ["X", "Y", "W", "H"];
                        body.push(
                            section(&format!("Shape · {}", sh.kind.id()), cx)
                                .child(div().grid().grid_cols(2).gap(px(6.)).children(self.shape_fields.iter().zip(labels).map(|(f, l)| div().flex().items_center().gap(px(6.)).child(div().w(px(14.)).font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(l)).child(f.clone()))))
                                .child(label_row("Fill", div(), cx))
                                .child(swatches("shape-fill", sh.fill.as_deref(), move |c, _, cx| u1(json!({ "fill": c }))(cx), cx))
                                .child(label_row("Outline", div(), cx))
                                .child(swatches("shape-line", sh.line.as_deref(), move |c, _, cx| u2(json!({ "line": c, "lineWidth": if c.is_empty() { 0.0 } else { 2.0 } }))(cx), cx))
                                .child(label_row("Text colour", div(), cx))
                                .child(swatches("shape-color", sh.color.as_deref(), move |c, _, cx| u3(json!({ "color": c }))(cx), cx))
                                .child(div().flex().gap(px(4.)).children([14, 18, 24, 32, 44].into_iter().map(|size| {
                                    let f = u4(json!({ "textSize": size }));
                                    Button::new(SharedString::from(format!("tsize-{size}")), size.to_string()).small().full_width().on_click(move |_, _, cx| f(cx))
                                })))
                                .into_any_element(),
                        );
                    }
                }
                let current = deck.theme.name.clone();
                body.push(
                    section("Theme", cx)
                        .child(div().flex().flex_col().gap(px(4.)).children(folio_core::deck::DeckTheme::NAMES.iter().map(|name| {
                            let th = folio_core::deck::DeckTheme::named(name).unwrap();
                            let chosen = current == *name;
                            let n = name.to_string();
                            let (bg, fg): (Hsla, Hsla) = (crate::paint::hex(&th.background, t.bg), crate::paint::hex(&th.text, t.text));
                            div()
                                .id(SharedString::from(format!("theme-{name}")))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .p(px(4.))
                                .border_1()
                                .border_color(if chosen { t.accent } else { t.line })
                                .when(chosen, |d| d.border_2())
                                .cursor_pointer()
                                .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("deck.setTheme", json!({ "theme": n }), cx)))
                                .child(div().w(px(56.)).h(px(32.)).bg(bg).border_1().border_color(t.line).flex().items_center().justify_center().text_color(fg).font_weight(FontWeight::BOLD).child("Aa"))
                                .child(div().child(name[..1].to_uppercase() + &name[1..]))
                        })))
                        .into_any_element(),
                );
            }
        }
        div()
            .id("inspector")
            .size_full()
            .flex()
            .flex_col()
            .glass(t.glass1)
            .border_0()
            .border_l_1()
            .border_color(t.line)
            .child(div().flex().flex_none().items_center().h(px(40.)).px(px(14.)).child(panel_title("Inspector")))
            .child(div().id("inspector-scroll").flex_1().min_h_0().overflow_y_scroll().children(body))
            .into_any_element()
    }
}
