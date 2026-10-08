//! The deck editor: the slide strip, the slide on its desk with shapes to select, move, resize
//! and type in, and the speaker notes, as in PowerPoint and Keynote. Every change is a
//! `deck.*` or `text.*` command.

use std::cell::Cell;
use std::rc::Rc;

use folio_core::deck::{Deck, Shape, ShapeKind, Slide};
use folio_core::text;
use folio_core::{Document, Id, PageKind, Pos};
use gpui::{
    App, Bounds, Context, CursorStyle, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder,
    Pixels, Point, Render, SharedString, Subscription, UTF16Selection, Window, canvas, div, fill, point, prelude::*, px, size,
};
use serde_json::{Value, json};

use crate::actions::*;
use crate::paint;
use crate::store::{Store, StoreExt, TextSel, TextTarget};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::{Button, caps, icon};

pub struct DeckView {
    store: Entity<Store>,
    focus: FocusHandle,
    notes: Entity<TextInput>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<ShapeDrag>,
    /// The slide whose notes the field shows.
    notes_for: Option<(Id, usize, u64)>,
    _subs: Vec<Subscription>,
}

#[derive(Clone, Debug)]
struct ShapeDrag {
    shape: Id,
    /// 0 moves; 1..=8 are the handles clockwise from the top-left.
    handle: u8,
    start: Point<Pixels>,
    orig: (f32, f32, f32, f32),
    gesture: String,
}

impl Focusable for DeckView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

struct Ctx {
    doc: Document,
    page: usize,
    id: Id,
}

/// Inner margin of a text box, as folio-layout lays it out.
const INSET: f32 = folio_layout::slide::INSET;

impl DeckView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        let notes = cx.new(|cx| TextInput::new(cx).multiline(3).placeholder("Speaker notes"));
        let subs = vec![cx.subscribe_in(&notes, window, |v: &mut Self, _, e: &InputEvent, _, cx| {
            if let InputEvent::Changed(t) = e {
                let slide = v.store.read(cx).view().slide;
                v.run("deck.setSlide", json!({ "slide": (slide + 1).to_string(), "notes": t, "coalesce": format!("notes:{slide}") }), cx);
            }
        })];
        Self { store, focus: cx.focus_handle(), notes, bounds: Rc::new(Cell::new(Bounds::default())), drag: None, notes_for: None, _subs: subs }
    }

    fn ctx(&self, cx: &App) -> Option<Ctx> {
        let s = self.store.read(cx);
        let doc = s.doc.clone()?;
        let page = s.page_index()?;
        if doc.pages[page].kind() != PageKind::Deck {
            return None;
        }
        let id = doc.pages[page].id.clone();
        Some(Ctx { doc, page, id })
    }

    fn run(&mut self, name: &str, mut params: Value, cx: &mut Context<Self>) -> Option<Value> {
        let c = self.ctx(cx)?;
        params["page"] = json!(c.id.to_string());
        self.store.update(cx, |s, cx| s.run_now(name, params, cx)).ok()
    }

    fn slide_index(&self, cx: &App) -> usize {
        self.store.read(cx).view().slide
    }

    pub fn set_slide(&mut self, i: usize, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            let v = s.view_mut();
            v.slide = i;
            v.shapes.clear();
            v.text = None;
            s.sync_ui();
            cx.notify();
        });
    }

    fn selected(&self, cx: &App) -> Vec<Id> {
        self.store.read(cx).view().shapes
    }

    fn text_sel(&self, cx: &App) -> Option<(usize, usize, TextSel)> {
        let v = self.store.read(cx).view();
        let t = v.text?;
        match t.target {
            TextTarget::Shape { slide, shape } => Some((slide, shape, t)),
            TextTarget::Doc => None,
        }
    }

    /// The slide's scale and top-left in the canvas (fit, then zoom).
    fn geometry(&self, deck: &Deck, cx: &App) -> (f32, f32, f32) {
        let b = self.bounds.get();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        let fit = ((w - 64.0) / deck.size[0]).min((h - 64.0) / deck.size[1]).max(0.05);
        let scale = fit * self.store.read(cx).zoom;
        let (sw, sh) = (deck.size[0] * scale, deck.size[1] * scale);
        (scale, (w - sw) / 2.0, (h - sh) / 2.0)
    }

    /// A window position in slide points.
    fn to_slide(&self, deck: &Deck, p: Point<Pixels>, cx: &App) -> (f32, f32) {
        let b = self.bounds.get();
        let (scale, ox, oy) = self.geometry(deck, cx);
        ((f32::from(p.x - b.origin.x) - ox) / scale, (f32::from(p.y - b.origin.y) - oy) / scale)
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(c) = self.ctx(cx) else { return };
        let deck = c.doc.pages[c.page].deck().unwrap();
        let si = self.slide_index(cx).min(deck.slides.len().saturating_sub(1));
        let Some(slide) = deck.slides.get(si) else { return };
        let (x, y) = self.to_slide(deck, e.position, cx);
        let (scale, _, _) = self.geometry(deck, cx);
        // Handles of the selected shape first.
        let sel = self.selected(cx);
        if sel.len() == 1
            && let Some(sh) = slide.shapes.iter().find(|s| s.id == sel[0])
            && let Some(h) = handle_at(sh, x, y, 6.0 / scale)
        {
            self.drag = Some(ShapeDrag { shape: sh.id.clone(), handle: h, start: e.position, orig: (sh.x, sh.y, sh.w, sh.h), gesture: format!("gesture:{}", folio_core::Id::new()) });
            return;
        }
        // Text editing: a click in the shape being edited moves the caret.
        if let Some((tsl, tsh, _)) = self.text_sel(cx)
            && tsl == si
            && let Some(sh) = slide.shapes.get(tsh)
            && inside(sh, x, y)
        {
            if let Some(p) = text_hit(deck, sh, x, y) {
                let extend = e.modifiers.shift;
                self.set_caret(si, tsh, p, extend, cx);
            }
            return;
        }
        // The topmost shape under the pointer.
        let hit = slide.shapes.iter().enumerate().rev().find(|(_, s)| inside(s, x, y));
        match hit {
            Some((i, sh)) => {
                if e.click_count >= 2 && sh.takes_text() {
                    let p = text_hit(deck, sh, x, y).unwrap_or_default();
                    self.set_caret(si, i, p, false, cx);
                    return;
                }
                let id = sh.id.clone();
                self.store.update(cx, |s, cx| {
                    let v = s.view_mut();
                    v.text = None;
                    if e.modifiers.shift {
                        if let Some(k) = v.shapes.iter().position(|x| *x == id) {
                            v.shapes.remove(k);
                        } else {
                            v.shapes.push(id.clone());
                        }
                    } else if !v.shapes.contains(&id) {
                        v.shapes = vec![id.clone()];
                    }
                    s.sync_ui();
                    cx.notify();
                });
                self.drag = Some(ShapeDrag { shape: id, handle: 0, start: e.position, orig: (sh.x, sh.y, sh.w, sh.h), gesture: format!("gesture:{}", folio_core::Id::new()) });
            }
            None => self.store.update(cx, |s, cx| {
                let v = s.view_mut();
                v.shapes.clear();
                v.text = None;
                s.sync_ui();
                cx.notify();
            }),
        }
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.drag.clone() else { return };
        if e.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let Some(c) = self.ctx(cx) else { return };
        let deck = c.doc.pages[c.page].deck().unwrap();
        let (scale, _, _) = self.geometry(deck, cx);
        let (dx, dy) = (f32::from(e.position.x - d.start.x) / scale, f32::from(e.position.y - d.start.y) / scale);
        if d.handle == 0 && dx.abs() < 2.0 && dy.abs() < 2.0 {
            return;
        }
        let (x, y, w, h) = d.orig;
        let (mut nx, mut ny, mut nw, mut nh) = (x, y, w, h);
        match d.handle {
            0 => {
                nx = x + dx;
                ny = y + dy;
            }
            1 => {
                nx = x + dx;
                ny = y + dy;
                nw = w - dx;
                nh = h - dy;
            }
            2 => {
                ny = y + dy;
                nh = h - dy;
            }
            3 => {
                ny = y + dy;
                nw = w + dx;
                nh = h - dy;
            }
            4 => nw = w + dx,
            5 => {
                nw = w + dx;
                nh = h + dy;
            }
            6 => nh = h + dy,
            7 => {
                nx = x + dx;
                nw = w - dx;
                nh = h + dy;
            }
            8 => {
                nx = x + dx;
                nw = w - dx;
            }
            _ => {}
        }
        // Shift keeps the proportions when resizing from a corner.
        if e.modifiers.shift && matches!(d.handle, 1 | 3 | 5 | 7) && w > 0.0 && h > 0.0 {
            let k = (nw / w).max(nh / h);
            nw = w * k;
            nh = h * k;
        }
        let (nw, nh) = (nw.max(4.0), nh.max(if h == 0.0 { 0.0 } else { 4.0 }));
        let snap = |v: f32| (v * 2.0).round() / 2.0;
        let si = self.slide_index(cx);
        self.run("deck.updateShape", json!({ "slide": (si + 1).to_string(), "shape": d.shape.to_string(), "x": snap(nx), "y": snap(ny), "w": snap(nw), "h": snap(nh), "coalesce": d.gesture }), cx);
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag = None;
    }

    // ---- text in shapes ---------------------------------------------------------

    fn set_caret(&mut self, slide: usize, shape: usize, p: Pos, extend: bool, cx: &mut Context<Self>) {
        let target = TextTarget::Shape { slide, shape };
        let id = self.ctx(cx).and_then(|c| c.doc.pages[c.page].deck().unwrap().slides.get(slide).and_then(|s| s.shapes.get(shape)).map(|s| s.id.clone()));
        self.store.update(cx, |s, cx| {
            let v = s.view_mut();
            let anchor = match v.text {
                Some(t) if extend && t.target == target => t.anchor,
                _ => p,
            };
            v.text = Some(TextSel { target, anchor, focus: p });
            if let Some(id) = id {
                v.shapes = vec![id];
            }
            s.sync_ui();
            cx.notify();
        });
    }

    fn shape_params(slide: usize, shape: &Shape) -> Value {
        json!({ "slide": (slide + 1).to_string(), "shape": shape.id.to_string() })
    }

    fn pos_param(p: Pos) -> Value {
        json!({ "block": p.block, "offset": p.offset })
    }

    fn edited_shape(&self, cx: &App) -> Option<(usize, usize, Shape, TextSel)> {
        let (sl, sh, t) = self.text_sel(cx)?;
        let c = self.ctx(cx)?;
        let shape = c.doc.pages[c.page].deck()?.slides.get(sl)?.shapes.get(sh)?.clone();
        Some((sl, sh, shape, t))
    }

    fn insert(&mut self, s_in: &str, cx: &mut Context<Self>) {
        let Some((sl, shi, shape, t)) = self.edited_shape(cx) else { return };
        let mut at = t.focus;
        if !t.is_empty() {
            let (a, b) = t.ordered();
            let mut p = Self::shape_params(sl, &shape);
            p["from"] = Self::pos_param(a);
            p["to"] = Self::pos_param(b);
            p["coalesce"] = json!("typing");
            at = self.run("text.delete", p, cx).map(|v| pos_from(&v["at"])).unwrap_or(a);
        }
        let mut p = Self::shape_params(sl, &shape);
        p["text"] = json!(s_in);
        p["at"] = Self::pos_param(at);
        p["coalesce"] = json!("typing");
        if let Some(v) = self.run("text.insert", p, cx) {
            self.set_caret(sl, shi, pos_from(&v["at"]), false, cx);
        }
    }

    fn delete(&mut self, forward: bool, cx: &mut Context<Self>) {
        // Shapes selected (not editing text): delete them.
        if self.text_sel(cx).is_none() {
            let si = self.slide_index(cx);
            for id in self.selected(cx) {
                self.run("deck.removeShape", json!({ "slide": (si + 1).to_string(), "shape": id.to_string() }), cx);
            }
            return;
        }
        let Some((sl, shi, shape, t)) = self.edited_shape(cx) else { return };
        let (a, b) = if t.is_empty() {
            let other = text::step(&shape.text, t.focus, forward);
            (t.focus.min(other), t.focus.max(other))
        } else {
            t.ordered()
        };
        if a == b {
            return;
        }
        let mut p = Self::shape_params(sl, &shape);
        p["from"] = Self::pos_param(a);
        p["to"] = Self::pos_param(b);
        p["coalesce"] = json!("typing");
        if let Some(v) = self.run("text.delete", p, cx) {
            self.set_caret(sl, shi, pos_from(&v["at"]), false, cx);
        }
    }

    fn arrow(&mut self, dx: i32, dy: i32, extend: bool, cx: &mut Context<Self>) {
        if let Some((sl, shi, shape, t)) = self.edited_shape(cx) {
            let p = if dx != 0 {
                text::step(&shape.text, t.focus, dx > 0)
            } else {
                // Up and down: the previous or next paragraph, same offset as far as it goes.
                let b = (t.focus.block as i64 + dy as i64).clamp(0, shape.text.len().saturating_sub(1) as i64) as usize;
                text::clamp(&shape.text, Pos::new(b, t.focus.offset))
            };
            self.set_caret(sl, shi, p, extend, cx);
            return;
        }
        // Shapes selected: nudge them (shift: 10 points).
        let step = if extend { 10.0 } else { 1.0 };
        let Some(c) = self.ctx(cx) else { return };
        let si = self.slide_index(cx);
        let Some(slide) = c.doc.pages[c.page].deck().unwrap().slides.get(si).cloned() else { return };
        let sel = self.selected(cx);
        if sel.is_empty() {
            // Nothing selected: arrows change slides.
            let n = c.doc.pages[c.page].deck().unwrap().slides.len();
            let next = (si as i64 + if dx + dy > 0 { 1 } else { -1 }).clamp(0, n.saturating_sub(1) as i64) as usize;
            self.set_slide(next, cx);
            return;
        }
        for sh in slide.shapes.iter().filter(|s| sel.contains(&s.id)) {
            self.run("deck.updateShape", json!({ "slide": (si + 1).to_string(), "shape": sh.id.to_string(), "x": sh.x + dx as f32 * step, "y": sh.y + dy as f32 * step, "coalesce": "nudge" }), cx);
        }
    }

    /// Bold, italic…: on the text selection, else on all the selected shapes' text.
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some((sl, _, shape, t)) = self.edited_shape(cx)
            && !t.is_empty()
        {
            let (a, b) = t.ordered();
            let on = !text::all_styled(&shape.text, a, b, |s| match key {
                "bold" => s.bold,
                "italic" => s.italic,
                "underline" => s.underline,
                _ => s.strike,
            });
            let mut p = Self::shape_params(sl, &shape);
            p["from"] = Self::pos_param(a);
            p["to"] = Self::pos_param(b);
            p[key] = json!(on);
            self.run("text.format", p, cx);
            return;
        }
        self.format_shapes(json!({ key: true }), Some(key), cx);
    }

    /// Formatting for the selected shapes' whole text (toggles `toggle_key` when given).
    pub fn format_shapes(&mut self, params: Value, toggle_key: Option<&str>, cx: &mut Context<Self>) {
        let Some(c) = self.ctx(cx) else { return };
        let si = self.slide_index(cx);
        let Some(slide) = c.doc.pages[c.page].deck().unwrap().slides.get(si).cloned() else { return };
        let sel = self.selected(cx);
        for sh in slide.shapes.iter().filter(|s| sel.contains(&s.id)) {
            let mut p = params.clone();
            if let Some(k) = toggle_key {
                let all = text::all_styled(&sh.text, Pos::default(), text::end(&sh.text), |s| match k {
                    "bold" => s.bold,
                    "italic" => s.italic,
                    "underline" => s.underline,
                    _ => s.strike,
                });
                p[k] = json!(!all);
            }
            p["slide"] = json!((si + 1).to_string());
            p["shape"] = json!(sh.id.to_string());
            self.run("deck.formatText", p, cx);
        }
    }

    /// Paragraph settings (alignment, lists) for the edited text or the selected shapes.
    pub fn paragraph(&mut self, params: Value, cx: &mut Context<Self>) {
        if let Some((sl, _, shape, t)) = self.edited_shape(cx) {
            let (a, b) = t.ordered();
            let mut p = params;
            p["slide"] = json!((sl + 1).to_string());
            p["shape"] = json!(shape.id.to_string());
            p["from"] = json!(a.block);
            p["to"] = json!(b.block);
            self.run("text.paragraph", p, cx);
            return;
        }
        self.format_shapes(params, None, cx);
    }

    pub fn add_shape(&mut self, kind: &str, cx: &mut Context<Self>) {
        let si = self.slide_index(cx);
        let mut p = json!({ "slide": (si + 1).to_string(), "kind": kind });
        if kind == "text" {
            p["text"] = json!("Text");
        }
        if let Some(v) = self.run("deck.addShape", p, cx)
            && let Some(id) = v["shape"].as_str()
        {
            let id = Id::from(id);
            self.store.update(cx, |s, cx| {
                let v = s.view_mut();
                v.shapes = vec![id];
                v.text = None;
                s.sync_ui();
                cx.notify();
            });
        }
    }

    pub fn add_slide(&mut self, layout: &str, cx: &mut Context<Self>) {
        let si = self.slide_index(cx);
        if let Some(v) = self.run("deck.addSlide", json!({ "layout": layout, "at": si + 1 }), cx)
            && let Some(n) = v["slide"].as_u64()
        {
            self.set_slide(n as usize - 1, cx);
        }
    }

    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let si = self.slide_index(cx);
        let sel = self.selected(cx);
        if sel.is_empty() {
            if self.run("deck.duplicateSlide", json!({ "slide": (si + 1).to_string() }), cx).is_some() {
                self.set_slide(si + 1, cx);
            }
            return;
        }
        for id in sel {
            self.run("deck.duplicateShape", json!({ "slide": (si + 1).to_string(), "shape": id.to_string() }), cx);
        }
    }

    fn sync_notes(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.ctx(cx) else { return };
        let si = self.slide_index(cx);
        let version = self.store.read(cx).version;
        let key = (c.id.clone(), si, version);
        if self.notes_for.as_ref().is_some_and(|k| k.0 == key.0 && k.1 == key.1) && self.notes.read(cx).text() == c.doc.pages[c.page].deck().unwrap().slides.get(si).map(|s| s.notes.as_str()).unwrap_or("") {
            return;
        }
        if self.notes_for.as_ref().is_some_and(|k| k.0 == key.0 && k.1 == key.1 && k.2 != version) {
            // Our own typing changed the version: keep the field as it is.
            self.notes_for = Some(key);
            return;
        }
        self.notes_for = Some(key);
        let notes = c.doc.pages[c.page].deck().unwrap().slides.get(si).map(|s| s.notes.clone()).unwrap_or_default();
        self.notes.update(cx, |i, cx| i.set_text(notes, cx));
    }
}

fn pos_from(v: &Value) -> Pos {
    Pos::new(v["block"].as_u64().unwrap_or(0) as usize, v["offset"].as_u64().unwrap_or(0) as usize)
}

fn inside(s: &Shape, x: f32, y: f32) -> bool {
    let pad = if s.h < 6.0 || s.w < 6.0 { 6.0 } else { 0.0 };
    x >= s.x - pad && x <= s.x + s.w + pad && y >= s.y - pad && y <= s.y + s.h + pad
}

/// Which resize handle (1..=8, clockwise from the top-left) is under a point.
fn handle_at(s: &Shape, x: f32, y: f32, r: f32) -> Option<u8> {
    let (x0, y0, x1, y1, xm, ym) = (s.x, s.y, s.x + s.w, s.y + s.h, s.x + s.w / 2.0, s.y + s.h / 2.0);
    let pts = [(x0, y0), (xm, y0), (x1, y0), (x1, ym), (x1, y1), (xm, y1), (x0, y1), (x0, ym)];
    pts.iter().position(|(hx, hy)| (x - hx).abs() <= r && (y - hy).abs() <= r).map(|i| i as u8 + 1)
}

/// The text position under a point inside a shape.
fn text_hit(deck: &Deck, sh: &Shape, x: f32, y: f32) -> Option<Pos> {
    let st = crate::views::with_fonts(|f| folio_layout::slide::layout_shape_text(f, deck, sh));
    let mut best: Option<Pos> = None;
    for (i, (py, l)) in st.paras.iter().enumerate() {
        if y >= *py || best.is_none() {
            best = Some(Pos::new(i, l.hit(x - st.x, y - py)));
        }
    }
    best
}

impl EntityInputHandler for DeckView {
    fn text_for_range(&mut self, _: std::ops::Range<usize>, _: &mut Option<std::ops::Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        Some(String::new())
    }
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        None
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}
    fn replace_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, t: &str, _: &mut Window, cx: &mut Context<Self>) {
        if t.is_empty() {
            return;
        }
        if self.text_sel(cx).is_some() {
            self.insert(t, cx);
            return;
        }
        // Typing with one text shape selected starts editing it at its end, like Keynote.
        let Some(c) = self.ctx(cx) else { return };
        let si = self.slide_index(cx);
        let sel = self.selected(cx);
        if sel.len() == 1
            && let Some((i, sh)) = c.doc.pages[c.page].deck().unwrap().slides.get(si).and_then(|s| s.shapes.iter().enumerate().find(|(_, x)| x.id == sel[0]))
            && sh.takes_text()
        {
            let end = text::end(&sh.text);
            self.set_caret(si, i, end, false, cx);
            self.insert(t, cx);
        }
    }
    fn replace_and_mark_text_in_range(&mut self, r: Option<std::ops::Range<usize>>, t: &str, _: Option<std::ops::Range<usize>>, w: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(r, t, w, cx);
    }
    fn bounds_for_range(&mut self, _: std::ops::Range<usize>, b: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        Some(b)
    }
    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Render for DeckView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_notes(cx);
        let t = cx.theme().clone();
        let Some(c) = self.ctx(cx) else { return div().into_any_element() };
        let deck = c.doc.pages[c.page].deck().unwrap().clone();
        let si = self.slide_index(cx).min(deck.slides.len().saturating_sub(1));
        let sel = self.selected(cx);
        let text_sel = self.text_sel(cx);
        let focused = self.focus.is_focused(window);
        let doc = c.doc.clone();
        let bounds_cell = self.bounds.clone();
        let focus = self.focus.clone();
        let entity = cx.entity();
        let zoom = self.store.read(cx).zoom;
        let editing_text = text_sel.is_some();

        // The slide strip.
        let thumb_w = 150.0;
        let strip = div()
            .id("slides-strip")
            .flex_none()
            .w(px(thumb_w + 44.))
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .h(px(36.))
                    .px(px(12.))
                    .child(crate::ui::panel_title("Slides"))
                    .child(Button::icon("add-slide", "plus", "New slide").small().on_click(cx.listener(|v, _, _, cx| v.add_slide("titleContent", cx)))),
            )
            .child(
                div().id("slides-scroll").flex_1().min_h_0().overflow_y_scroll().px(px(10.)).pb(px(12.)).flex().flex_col().gap(px(10.)).children(deck.slides.iter().enumerate().map(|(i, s)| {
                    let current = i == si;
                    let (deck2, doc2, s2) = (deck.clone(), doc.clone(), s.clone());
                    let th = thumb_w * deck.size[1] / deck.size[0];
                    div()
                        .id(("slide-thumb", i))
                        .flex()
                        .gap(px(6.))
                        .cursor_pointer()
                        .on_click(cx.listener(move |v, _, _, cx| v.set_slide(i, cx)))
                        .child(div().w(px(18.)).flex_none().font_family(MONO).text_size(px(sz::XS)).text_color(if current { t.text } else { t.text_3 }).child(format!("{}", i + 1)))
                        .child(
                            div()
                                .relative()
                                .flex_none()
                                .w(px(thumb_w))
                                .h(px(th))
                                .border_1()
                                .border_color(if current { t.accent } else { t.line_strong })
                                .when(current, |d| d.shadow(t.chip_shadow()))
                                .when(s.hidden, |d| d.opacity(0.4))
                                .child(canvas(|_, _, _| {}, move |b, _, window, cx| paint_slide(window, cx, &doc2, &deck2, &s2, b.origin, thumb_w / deck2.size[0], None)).size_full()),
                        )
                })),
            );

        let canvas_el = div()
            .key_context(if editing_text { "DeckEditor DeckText" } else { "DeckEditor" })
            .track_focus(&self.focus)
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(t.desk)
            .cursor(if editing_text { CursorStyle::IBeam } else { CursorStyle::Arrow })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_action(cx.listener(|v, _: &Backspace, _, cx| v.delete(false, cx)))
            .on_action(cx.listener(|v, _: &DeleteForward, _, cx| v.delete(true, cx)))
            .on_action(cx.listener(|v, _: &Enter, w, cx| {
                if v.text_sel(cx).is_some() {
                    v.insert("\n", cx);
                } else {
                    // Enter on a selected text shape starts editing it.
                    let _ = w;
                    let si = v.slide_index(cx);
                    let sel = v.selected(cx);
                    if let Some(c) = v.ctx(cx)
                        && sel.len() == 1
                        && let Some((i, sh)) = c.doc.pages[c.page].deck().unwrap().slides.get(si).and_then(|s| s.shapes.iter().enumerate().find(|(_, x)| x.id == sel[0]))
                        && sh.takes_text()
                    {
                        let end = text::end(&sh.text);
                        v.set_caret(si, i, end, false, cx);
                    }
                }
            }))
            .on_action(cx.listener(|v, _: &Escape, _, cx| {
                v.store.update(cx, |s, cx| {
                    let pv = s.view_mut();
                    if pv.text.is_some() {
                        pv.text = None;
                    } else {
                        pv.shapes.clear();
                    }
                    s.sync_ui();
                    cx.notify();
                })
            }))
            .on_action(cx.listener(|v, _: &Left, _, cx| v.arrow(-1, 0, false, cx)))
            .on_action(cx.listener(|v, _: &Right, _, cx| v.arrow(1, 0, false, cx)))
            .on_action(cx.listener(|v, _: &Up, _, cx| v.arrow(0, -1, false, cx)))
            .on_action(cx.listener(|v, _: &Down, _, cx| v.arrow(0, 1, false, cx)))
            .on_action(cx.listener(|v, _: &SelectLeft, _, cx| v.arrow(-1, 0, true, cx)))
            .on_action(cx.listener(|v, _: &SelectRight, _, cx| v.arrow(1, 0, true, cx)))
            .on_action(cx.listener(|v, _: &SelectUp, _, cx| v.arrow(0, -1, true, cx)))
            .on_action(cx.listener(|v, _: &SelectDown, _, cx| v.arrow(0, 1, true, cx)))
            .on_action(cx.listener(|v, _: &SelectAll, _, cx| {
                if let Some((sl, shi, shape, _)) = v.edited_shape(cx) {
                    let end = text::end(&shape.text);
                    v.set_caret(sl, shi, Pos::default(), false, cx);
                    v.set_caret(sl, shi, end, true, cx);
                } else if let Some(c) = v.ctx(cx) {
                    let si = v.slide_index(cx);
                    let ids: Vec<Id> = c.doc.pages[c.page].deck().unwrap().slides.get(si).map(|s| s.shapes.iter().map(|x| x.id.clone()).collect()).unwrap_or_default();
                    v.store.update(cx, |s, cx| {
                        s.view_mut().shapes = ids;
                        s.sync_ui();
                        cx.notify();
                    });
                }
            }))
            .on_action(cx.listener(|v, _: &Duplicate, _, cx| v.duplicate(cx)))
            .on_action(cx.listener(|v, _: &NewSlide, _, cx| v.add_slide("titleContent", cx)))
            .on_action(cx.listener(|v, _: &Bold, _, cx| v.toggle("bold", cx)))
            .on_action(cx.listener(|v, _: &Italic, _, cx| v.toggle("italic", cx)))
            .on_action(cx.listener(|v, _: &Underline, _, cx| v.toggle("underline", cx)))
            .on_action(cx.listener(|v, _: &AlignLeft, _, cx| v.paragraph(json!({ "align": "left" }), cx)))
            .on_action(cx.listener(|v, _: &AlignCenter, _, cx| v.paragraph(json!({ "align": "center" }), cx)))
            .on_action(cx.listener(|v, _: &AlignRight, _, cx| v.paragraph(json!({ "align": "right" }), cx)))
            .on_action(cx.listener(|v, _: &BulletList, _, cx| v.paragraph(json!({ "list": "bullet" }), cx)))
            .on_action(cx.listener(|v, _: &NumberList, _, cx| v.paragraph(json!({ "list": "number" }), cx)))
            .child(
                canvas(
                    move |bounds, _, _| {
                        bounds_cell.set(bounds);
                    },
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                        let Some(slide) = deck.slides.get(si) else { return };
                        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                        let fit = ((w - 64.0) / deck.size[0]).min((h - 64.0) / deck.size[1]).max(0.05);
                        let scale = fit * zoom;
                        let (sw, sh) = (deck.size[0] * scale, deck.size[1] * scale);
                        let origin = point(bounds.origin.x + px((w - sw) / 2.0), bounds.origin.y + px((h - sh) / 2.0));
                        let t = cx.theme().clone();
                        window.paint_quad(fill(Bounds::new(point(origin.x + px(5.), origin.y + px(5.)), size(px(sw), px(sh))), t.drop));
                        paint_slide(window, cx, &doc, &deck, slide, origin, scale, text_sel.filter(|(sl, _, _)| *sl == si).map(|(_, sh, ts)| (sh, ts, focused)));
                        // Selection: a hairline box and square handles.
                        for s in slide.shapes.iter().filter(|s| sel.contains(&s.id)) {
                            let b = Bounds::new(point(origin.x + px(s.x * scale), origin.y + px(s.y * scale)), size(px((s.w * scale).max(1.)), px((s.h * scale).max(1.))));
                            window.paint_quad(gpui::outline(b, t.accent, gpui::BorderStyle::Solid));
                            if sel.len() == 1 && text_sel.is_none() {
                                for (hx, hy) in [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.0, 0.5)] {
                                    let c = point(b.origin.x + b.size.width * hx, b.origin.y + b.size.height * hy);
                                    let hb = Bounds::new(point(c.x - px(4.), c.y - px(4.)), size(px(8.), px(8.)));
                                    window.paint_quad(fill(hb, t.accent));
                                    window.paint_quad(fill(Bounds::new(point(c.x - px(3.), c.y - px(3.)), size(px(6.), px(6.))), t.bg_raised));
                                }
                            }
                        }
                    },
                )
                .size_full(),
            );

        div()
            .flex()
            .size_full()
            .child(strip)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(canvas_el)
                    .child(
                        div()
                            .flex_none()
                            .border_t_1()
                            .border_color(t.line)
                            .bg(t.bg_raised)
                            .px(px(14.))
                            .py(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().flex().items_center().gap(px(6.)).child(icon("notebook-pen").text_color(t.text_2)).child(caps("Speaker notes", cx)))
                            .child(self.notes.clone()),
                    ),
            )
            .into_any_element()
    }
}

/// Paints a slide at `origin` (pixels), points scaled by `scale`. `editing` is the shape whose
/// text has a caret: (shape index, selection, show the caret).
#[allow(clippy::too_many_arguments)]
pub fn paint_slide(window: &mut Window, cx: &mut App, doc: &Document, deck: &Deck, slide: &Slide, origin: Point<Pixels>, scale: f32, editing: Option<(usize, TextSel, bool)>) {
    let at = |x: f32, y: f32| point(origin.x + px(x * scale), origin.y + px(y * scale));
    let bg = paint::hex(slide.background.as_deref().unwrap_or(&deck.theme.background), gpui::white());
    window.paint_quad(fill(Bounds::new(origin, size(px(deck.size[0] * scale), px(deck.size[1] * scale))), bg));
    for (i, s) in slide.shapes.iter().enumerate() {
        let b = Bounds::new(at(s.x, s.y), size(px((s.w * scale).max(0.5)), px((s.h * scale).max(0.5))));
        let fill_c = s.fill.as_deref().map(|f| paint::hex(f, gpui::transparent_black()));
        let line_c = s.line.as_deref().map(|l| paint::hex(l, gpui::black()));
        let lw = px((s.line_width * scale).max(if s.line.is_some() { 1.0 } else { 0.0 }));
        match &s.kind {
            ShapeKind::Rect | ShapeKind::Text => {
                if let Some(f) = fill_c {
                    window.paint_quad(fill(b, f));
                }
                if let Some(l) = line_c {
                    window.paint_quad(gpui::quad(b, px(0.), gpui::transparent_black(), lw, l, gpui::BorderStyle::Solid));
                }
            }
            ShapeKind::Ellipse => {
                if let Some(f) = fill_c {
                    paint_ellipse(window, b, f, None);
                }
                if let Some(l) = line_c {
                    paint_ellipse(window, b, gpui::transparent_black(), Some((l, lw)));
                }
            }
            ShapeKind::Triangle => {
                let mut pb = PathBuilder::fill();
                pb.move_to(point(b.origin.x + b.size.width / 2.0, b.origin.y));
                pb.line_to(point(b.origin.x + b.size.width, b.origin.y + b.size.height));
                pb.line_to(point(b.origin.x, b.origin.y + b.size.height));
                pb.close();
                if let (Some(f), Ok(p)) = (fill_c, pb.build()) {
                    window.paint_path(p, f);
                }
            }
            ShapeKind::Line | ShapeKind::Arrow => {
                let c = line_c.unwrap_or(gpui::black());
                let (p0, p1) = (at(s.x, s.y), at(s.x + s.w, s.y + s.h));
                let mut pb = PathBuilder::stroke(lw.max(px(1.)));
                pb.move_to(p0);
                pb.line_to(p1);
                if let Ok(p) = pb.build() {
                    window.paint_path(p, c);
                }
                if matches!(s.kind, ShapeKind::Arrow) {
                    let (dx, dy) = (f32::from(p1.x - p0.x), f32::from(p1.y - p0.y));
                    let len = (dx * dx + dy * dy).sqrt().max(0.001);
                    let (ux, uy) = (dx / len, dy / len);
                    let head = (s.line_width.max(2.0) * 4.0 * scale).max(8.0);
                    let mut pb = PathBuilder::fill();
                    pb.move_to(p1);
                    pb.line_to(point(p1.x - px(ux * head - uy * head * 0.5), p1.y - px(uy * head + ux * head * 0.5)));
                    pb.line_to(point(p1.x - px(ux * head + uy * head * 0.5), p1.y - px(uy * head - ux * head * 0.5)));
                    pb.close();
                    if let Ok(p) = pb.build() {
                        window.paint_path(p, c);
                    }
                }
            }
            ShapeKind::Image { media } => match doc.media.get(media).and_then(|m| paint::image(&m.bytes, 2048)) {
                Some(img) => {
                    let _ = window.paint_image(b, b, Default::default(), img, 0, false);
                }
                None => window.paint_quad(fill(b, gpui::Rgba { r: 0.5, g: 0.5, b: 0.5, a: 0.2 })),
            },
            ShapeKind::Chart { chart } => {
                let data = folio_core::links::chart_data(doc, chart).unwrap_or_default();
                let text = folio_layout::parse_hex(&deck.theme.text, [20, 20, 20, 255]);
                let accent = folio_layout::parse_hex(&deck.theme.accent, [20, 20, 20, 255]);
                let mut grid = text;
                grid[3] = 40;
                let mut second = text;
                second[3] = 140;
                let style = folio_layout::chart::ChartStyle { text, grid, series: vec![accent, second, [150, 150, 150, 255], [90, 90, 90, 255]], size: 12.0, background: None };
                let prims = folio_layout::chart_prims(chart, &data, s.w, s.h, &style);
                paint::prims(window, cx, &prims, b.origin, scale);
            }
            ShapeKind::Table { table } => {
                let rows: Vec<Vec<String>> = match &table.link {
                    Some(l) => folio_core::links::table_text(doc, l).unwrap_or_default(),
                    None => table.rows.iter().map(|r| r.iter().map(|c| c.plain()).collect()).collect(),
                };
                let n = rows.len().max(1) as f32;
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1) as f32;
                let (rh, cw) = (s.h / n, s.w / cols);
                let text_c = paint::hex(s.color.as_deref().unwrap_or(&deck.theme.text), gpui::black());
                for (ri, row) in rows.iter().enumerate() {
                    let y = s.y + ri as f32 * rh;
                    if ri == 0 && table.header {
                        window.paint_quad(fill(Bounds::new(at(s.x, y), size(px(s.w * scale), px(rh * scale))), text_c.opacity(0.08)));
                    }
                    window.paint_quad(fill(Bounds::new(at(s.x, y + rh), size(px(s.w * scale), px(1.))), text_c.opacity(0.25)));
                    for (ci, v) in row.iter().enumerate() {
                        let fs = px(s.text_size * scale);
                        let font = gpui::Font { family: "IBM Plex Sans".into(), features: Default::default(), fallbacks: None, weight: if ri == 0 && table.header { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }, style: gpui::FontStyle::Normal };
                        let run = gpui::TextRun { len: v.len(), font, color: text_c, background_color: None, underline: None, strikethrough: None };
                        let line = window.text_system().shape_line(SharedString::from(v.clone()), fs, &[run], None);
                        let numeric = v.trim().trim_start_matches(['$', '€', '-']).chars().next().is_some_and(|c| c.is_ascii_digit());
                        let tw = f32::from(line.width) / scale;
                        let x = if numeric && ri > 0 { s.x + (ci as f32 + 1.0) * cw - 8.0 - tw } else { s.x + ci as f32 * cw + 8.0 };
                        let clip = Bounds::new(at(s.x + ci as f32 * cw, y), size(px(cw * scale), px(rh * scale)));
                        window.with_content_mask(Some(gpui::ContentMask { bounds: clip }), |window| {
                            let _ = line.paint(at(x, y + (rh - s.text_size * 1.3) / 2.0), fs * 1.3, gpui::TextAlign::Left, None, window, cx);
                        });
                    }
                }
            }
        }
        // Text inside the shape.
        if !s.text.is_empty() && s.takes_text() {
            let st = crate::views::with_fonts(|f| folio_layout::slide::layout_shape_text(f, deck, s));
            let edit = editing.filter(|(si, _, _)| *si == i);
            let clip = Bounds::new(at(s.x, s.y.min(st.paras.first().map(|p| p.0).unwrap_or(s.y))), size(px(s.w * scale), px((s.h.max(st.height + 2.0 * INSET)) * scale)));
            window.with_content_mask(Some(gpui::ContentMask { bounds: clip }), |window| {
                for (pi, (py, l)) in st.paras.iter().enumerate() {
                    if let Some((_, sel, _)) = edit
                        && !sel.is_empty()
                    {
                        let (a, b) = sel.ordered();
                        if pi >= a.block && pi <= b.block {
                            let from = if pi == a.block { a.offset } else { 0 };
                            let to = if pi == b.block { b.offset } else { usize::MAX };
                            for (rx, ry, rw, rh) in l.rects(from, to) {
                                window.paint_quad(fill(Bounds::new(at(st.x + rx, py + ry), size(px(rw * scale), px(rh * scale))), gpui::Rgba { r: 0.4, g: 0.55, b: 1.0, a: 0.3 }));
                            }
                        }
                    }
                    let n = l.lines.len();
                    paint::para(window, l, at(st.x, *py), scale, 0..n, 0.0, None);
                }
            });
            if let Some((_, sel, true)) = edit
                && sel.is_empty()
                && let Some((py, l)) = st.paras.get(sel.focus.block)
            {
                let (li, x) = l.caret(sel.focus.offset);
                if let Some(line) = l.lines.get(li) {
                    let color = paint::hex(s.color.as_deref().unwrap_or(&deck.theme.text), gpui::black());
                    window.paint_quad(fill(Bounds::new(at(st.x + x, py + line.y), size(px(1.6), px(line.height * scale))), color));
                }
            }
        } else if let Some((_, _, true)) = editing.filter(|(si, _, _)| *si == i) {
            // An empty text box being edited: the caret at its start.
            let color = paint::hex(s.color.as_deref().unwrap_or(&deck.theme.text), gpui::black());
            window.paint_quad(fill(Bounds::new(at(s.x + INSET, s.y + INSET), size(px(1.6), px(s.text_size * 1.3 * scale))), color));
        }
    }
}

/// An ellipse filling (or outlining) a box.
fn paint_ellipse(window: &mut Window, b: Bounds<Pixels>, color: gpui::Hsla, stroke: Option<(gpui::Hsla, Pixels)>) {
    let (cx0, cy0) = (b.origin.x + b.size.width / 2.0, b.origin.y + b.size.height / 2.0);
    let (rx, ry) = (f32::from(b.size.width) / 2.0, f32::from(b.size.height) / 2.0);
    let mut pb = match stroke {
        Some((_, w)) => PathBuilder::stroke(w),
        None => PathBuilder::fill(),
    };
    let n = 64;
    for k in 0..=n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        let p = point(cx0 + px(rx * a.cos()), cy0 + px(ry * a.sin()));
        if k == 0 {
            pb.move_to(p);
        } else {
            pb.line_to(p);
        }
    }
    if stroke.is_none() {
        pb.close();
    }
    if let Ok(p) = pb.build() {
        window.paint_path(p, stroke.map(|s| s.0).unwrap_or(color));
    }
}
