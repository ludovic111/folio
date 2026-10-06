//! Presenting a deck: the slide full screen on black, or the presenter view (this slide, the
//! next, the notes and a clock). Arrows, Space and clicks move; Escape ends.

use std::time::Instant;

use gpui::{AnyElement, App, FontWeight, MouseButton, Window, canvas, div, fill, point, prelude::*, px};

use crate::store::StoreExt;
use crate::theme::{ActiveTheme, MONO, size as sz};

thread_local! {
    static STARTED: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Moves the presentation by `by` slides (skipping hidden ones); past the end it ends.
pub fn step(by: i64, window: &mut Window, cx: &mut App) {
    let store = cx.store();
    let (page, slide, presenter) = match store.read(cx).presenting.clone() {
        Some(p) => p,
        None => return,
    };
    let Some(deck) = store.read(cx).doc.as_ref().and_then(|d| d.page(page.as_str()).ok().and_then(|p| p.deck().cloned())) else { return };
    let mut i = slide as i64 + by;
    while i >= 0 && (i as usize) < deck.slides.len() && deck.slides[i as usize].hidden {
        i += by.signum();
    }
    if i < 0 {
        return;
    }
    if i as usize >= deck.slides.len() {
        end(window, cx);
        return;
    }
    store.update(cx, |s, cx| {
        s.presenting = Some((page, i as usize, presenter));
        cx.notify();
    });
}

pub fn end(window: &mut Window, cx: &mut App) {
    STARTED.with(|s| s.set(None));
    cx.store().update(cx, |s, cx| {
        if let Some((page, slide, _)) = s.presenting.take() {
            s.show_page(page, cx);
            s.view_mut().slide = slide;
        }
        s.sync_ui();
        cx.notify();
    });
    if window.is_fullscreen() {
        window.toggle_fullscreen();
    }
}

pub fn present(window: &mut Window, cx: &mut App) -> AnyElement {
    let t = cx.theme().clone();
    let store = cx.store();
    let s = store.read(cx);
    let Some((page, si, presenter)) = s.presenting.clone() else { return div().into_any_element() };
    let Some(doc) = s.doc.clone() else { return div().into_any_element() };
    let Some(deck) = doc.page(page.as_str()).ok().and_then(|p| p.deck().cloned()) else { return div().into_any_element() };
    let started = STARTED.with(|c| {
        if c.get().is_none() {
            c.set(Some(Instant::now()));
        }
        c.get().unwrap()
    });
    let slide = deck.slides.get(si).cloned();
    let next = deck.slides.iter().skip(si + 1).find(|s| !s.hidden).cloned();
    let (d1, k1, d2, k2) = (doc.clone(), deck.clone(), doc.clone(), deck.clone());
    let draw = move |slide: Option<folio_core::deck::Slide>, doc: folio_core::Document, deck: folio_core::deck::Deck| {
        canvas(|_, _, _| {}, move |b, _, window, cx| {
            window.paint_quad(fill(b, gpui::black()));
            let Some(slide) = &slide else { return };
            let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
            let scale = (w / deck.size[0]).min(h / deck.size[1]);
            let (sw, sh) = (deck.size[0] * scale, deck.size[1] * scale);
            let origin = point(b.origin.x + px((w - sw) / 2.0), b.origin.y + px((h - sh) / 2.0));
            crate::views::deckview::paint_slide(window, cx, &doc, &deck, slide, origin, scale, None);
        })
        .size_full()
    };
    let base = div()
        .id("presenting")
        .absolute()
        .inset_0()
        .bg(gpui::black())
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, w, cx| step(1, w, cx))
        .on_mouse_down(MouseButton::Right, |_, w, cx| step(-1, w, cx));
    if !presenter {
        return base.child(draw(slide, d1, k1)).into_any_element();
    }
    let elapsed = started.elapsed().as_secs();
    window.request_animation_frame();
    let notes = deck.slides.get(si).map(|s| s.notes.clone()).unwrap_or_default();
    base.child(
        div()
            .size_full()
            .flex()
            .gap(px(20.))
            .p(px(24.))
            .text_color(gpui::white())
            .child(div().flex_1().flex().flex_col().gap(px(10.)).child(div().font_family(MONO).text_size(px(sz::SM)).child(format!("SLIDE {} OF {}", si + 1, deck.slides.len()))).child(div().flex_1().child(draw(slide, d1, k1))))
            .child(
                div()
                    .w(px(420.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(div().font_family(MONO).text_size(px(sz::SM)).child("NEXT"))
                    .child(div().h(px(236.)).border_1().border_color(t.line_strong).child(draw(next, d2, k2)))
                    .child(div().font_family(MONO).text_size(px(28.)).font_weight(FontWeight::SEMIBOLD).child(format!("{:02}:{:02}", elapsed / 60, elapsed % 60)))
                    .child(div().font_family(MONO).text_size(px(sz::SM)).child("NOTES"))
                    .child(div().flex_1().text_size(px(sz::LG)).child(if notes.is_empty() { "No notes for this slide.".to_string() } else { notes })),
            ),
    )
    .into_any_element()
}
