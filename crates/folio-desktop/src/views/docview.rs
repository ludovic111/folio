//! The document editor: pages of paper on the desk, laid out by folio-layout (what prints is
//! what shows), a caret and a selection, typing through the IME, and every change a `text.*`
//! command.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use folio_core::text::{self, RunStyle};
use folio_core::{Block, Document, Id, PageKind, Pos};
use folio_layout::{DocLayout, Placed};
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Render, ScrollWheelEvent, UTF16Selection, Window, canvas, div, fill, point, prelude::*, px, size,
};
use serde_json::{Value, json};

use crate::actions::*;
use crate::paint::{self, PT};
use crate::store::{Store, StoreExt, TextSel, TextTarget};
use crate::theme::ActiveTheme;

/// Space around pages on the desk, in pixels.
const GAP: f32 = 28.0;

pub struct DocView {
    store: Entity<Store>,
    focus: FocusHandle,
    /// The layout of the page shown, for the version it was made from.
    layout: Option<(u64, Id, Arc<DocLayout>)>,
    /// Where the canvas is in the window (for mouse positions).
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Scroll from the top of the first page, in pixels.
    scroll: f32,
    dragging: bool,
    /// The x a caret moving up and down tries to keep (points).
    goal_x: Option<f32>,
    /// Formatting picked with no text selected (⌘B then type).
    pub typing_style: Option<RunStyle>,
    /// The IME's composition: where it starts and how many characters it has.
    marked: Option<(Pos, usize)>,
    /// Scroll the caret into view on the next paint.
    reveal: bool,
}

impl Focusable for DocView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// What the view works on: the document, the page index and its id.
struct Ctx {
    doc: Document,
    page: usize,
    id: Id,
}

impl DocView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store, focus: cx.focus_handle(), layout: None, bounds: Rc::new(Cell::new(Bounds::default())), scroll: 0.0, dragging: false, goal_x: None, typing_style: None, marked: None, reveal: false }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    fn ctx(&self, cx: &App) -> Option<Ctx> {
        let s = self.store.read(cx);
        let doc = s.doc.clone()?;
        let page = s.page_index()?;
        if doc.pages[page].kind() != PageKind::Doc {
            return None;
        }
        let id = doc.pages[page].id.clone();
        Some(Ctx { doc, page, id })
    }

    fn scale(&self, cx: &App) -> f32 {
        PT * self.store.read(cx).zoom
    }

    /// The layout of the page shown (made again when the document changed).
    fn layout(&mut self, cx: &App) -> Option<Arc<DocLayout>> {
        let c = self.ctx(cx)?;
        let version = self.store.read(cx).version;
        if let Some((v, id, l)) = &self.layout
            && *v == version
            && *id == c.id
        {
            return Some(l.clone());
        }
        let l = Arc::new(crate::views::with_fonts(|f| folio_layout::layout_doc(f, &c.doc, c.page)));
        self.layout = Some((version, c.id, l.clone()));
        Some(l)
    }

    fn sel(&self, cx: &App) -> TextSel {
        self.store.read(cx).view().text.filter(|t| t.target == TextTarget::Doc).unwrap_or(TextSel::caret(TextTarget::Doc, Pos::default()))
    }

    fn set_sel(&mut self, sel: TextSel, cx: &mut Context<Self>) {
        self.reveal = true;
        self.store.update(cx, |s, cx| s.set_text_sel(Some(sel), cx));
        cx.notify();
    }

    fn caret_to(&mut self, p: Pos, extend: bool, cx: &mut Context<Self>) {
        let mut sel = self.sel(cx);
        sel.focus = p;
        if !extend {
            sel.anchor = p;
        }
        self.typing_style = None;
        self.set_sel(sel, cx);
    }

    // ---- geometry ------------------------------------------------------------

    /// Top-left of page `i` in canvas pixels.
    fn page_origin(&self, l: &DocLayout, i: usize, width_px: f32, scale: f32) -> (f32, f32) {
        let pw = l.width * scale;
        let x = ((width_px - pw) / 2.0).max(GAP);
        let y = GAP + i as f32 * (l.height * scale + GAP) - self.scroll;
        (x, y)
    }

    fn content_height(l: &DocLayout, scale: f32) -> f32 {
        GAP + l.pages.len() as f32 * (l.height * scale + GAP)
    }

    /// Where a position is: (page, x, y, height) in page points.
    fn caret_rect(l: &DocLayout, p: Pos) -> Option<(usize, f32, f32, f32)> {
        for (pi, page) in l.pages.iter().enumerate() {
            for item in &page.items {
                if item.block() != p.block {
                    continue;
                }
                match item {
                    Placed::Para { x, y, layout, lines, .. } if p.cell.is_none() => {
                        let (li, cx) = layout.caret(p.offset);
                        if !lines.contains(&li) && !(lines.end == layout.lines.len() && li >= lines.end) {
                            continue;
                        }
                        let dy = layout.lines.get(lines.start).map(|l| l.y).unwrap_or(0.0);
                        let line = &layout.lines[li.min(layout.lines.len().saturating_sub(1))];
                        return Some((pi, x + cx, y + line.y - dy, line.height));
                    }
                    Placed::TableRow { row, cells, y, .. } => {
                        let Some((r, c)) = p.cell else { continue };
                        if *row != r {
                            continue;
                        }
                        let Some(cell) = cells.iter().find(|cb| cb.col == c) else { continue };
                        let (li, cx) = cell.layout.caret(p.offset);
                        let line = cell.layout.lines.get(li)?;
                        let (tx, ty) = cell_origin(cell, *y);
                        return Some((pi, tx + cx, ty + line.y, line.height));
                    }
                    Placed::Image { x, y, w, h, .. } | Placed::Chart { x, y, w, h, .. } => {
                        return Some((pi, if p.offset == 0 { *x - 2.0 } else { x + w + 1.0 }, *y, *h));
                    }
                    Placed::PageBreak { y, .. } => return Some((pi, 72.0, *y, 14.0)),
                    _ => {}
                }
            }
        }
        None
    }

    /// The position nearest to a point on page `pi` (points).
    fn hit(l: &DocLayout, pi: usize, x: f32, y: f32) -> Option<Pos> {
        let page = l.pages.get(pi)?;
        let mut best: Option<(f32, Pos)> = None;
        fn consider(best: &mut Option<(f32, Pos)>, d: f32, p: Pos) {
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                *best = Some((d, p));
            }
        }
        for item in &page.items {
            match item {
                Placed::Para { block, x: px0, y: py0, layout, lines } => {
                    let dy = layout.lines.get(lines.start).map(|l| l.y).unwrap_or(0.0);
                    let top = *py0;
                    let bottom = py0 + layout.lines.get(lines.end.saturating_sub(1)).map(|l| l.y + l.height - dy).unwrap_or(0.0);
                    let d = if y < top { top - y } else if y > bottom { y - bottom } else { 0.0 };
                    let ly = (y - py0 + dy).clamp(layout.lines.get(lines.start).map(|l| l.y).unwrap_or(0.0), layout.lines.get(lines.end.saturating_sub(1)).map(|l| l.y + l.height - 0.01).unwrap_or(0.0));
                    let off = layout.hit(x - px0, ly);
                    consider(&mut best, d, Pos::new(*block, off));
                }
                Placed::TableRow { block, row, y: ry, height, cells, .. } => {
                    let d = if y < *ry { ry - y } else if y > ry + height { y - ry - height } else { 0.0 };
                    if d > 0.0 && best.as_ref().is_some_and(|(bd, _)| *bd <= d) {
                        continue;
                    }
                    let cell = cells.iter().min_by(|a, b| {
                        let da = if x < a.x { a.x - x } else if x > a.x + a.w { x - a.x - a.w } else { 0.0 };
                        let db = if x < b.x { b.x - x } else if x > b.x + b.w { x - b.x - b.w } else { 0.0 };
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    if let Some(c) = cell {
                        let (tx, ty) = cell_origin(c, *ry);
                        let off = c.layout.hit(x - tx, (y - ty).max(0.0));
                        consider(&mut best, d, Pos::in_cell(*block, *row, c.col, off));
                    }
                }
                Placed::Image { block, x: ix, y: iy, w, h, .. } | Placed::Chart { block, x: ix, y: iy, w, h } => {
                    let d = if y < *iy { iy - y } else if y > iy + h { y - iy - h } else { 0.0 };
                    consider(&mut best, d, Pos::new(*block, if x < ix + w / 2.0 { 0 } else { 1 }));
                }
                Placed::PageBreak { block, y: by } => consider(&mut best, (y - by).abs() + 6.0, Pos::new(*block, 0)),
                Placed::Caption { .. } => {}
            }
        }
        best.map(|b| b.1)
    }

    /// The page and point (in page points) under a window position.
    fn locate(&self, l: &DocLayout, pos: Point<Pixels>, cx: &App) -> Option<(usize, f32, f32)> {
        let b = self.bounds.get();
        let scale = self.scale(cx);
        let (lx, ly) = (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y));
        let w = f32::from(b.size.width);
        let ph = l.height * scale + GAP;
        let pi = (((ly + self.scroll - GAP) / ph).floor().max(0.0) as usize).min(l.pages.len().saturating_sub(1));
        let (ox, oy) = self.page_origin(l, pi, w, scale);
        Some((pi, (lx - ox) / scale, (ly - oy) / scale))
    }

    // ---- commands --------------------------------------------------------------

    fn run(&mut self, name: &str, mut params: Value, cx: &mut Context<Self>) -> Option<Value> {
        let c = self.ctx(cx)?;
        params["page"] = json!(c.id.to_string());
        self.store.update(cx, |s, cx| s.run_now(name, params, cx)).ok()
    }

    fn pos_param(p: Pos) -> Value {
        match p.cell {
            Some((r, c)) => json!({ "block": p.block, "offset": p.offset, "cell": [r, c] }),
            None => json!({ "block": p.block, "offset": p.offset }),
        }
    }

    fn pos_from(v: &Value) -> Pos {
        let cell = v["cell"].as_array().map(|a| (a[0].as_u64().unwrap_or(0) as usize, a[1].as_u64().unwrap_or(0) as usize));
        Pos { block: v["block"].as_u64().unwrap_or(0) as usize, cell, offset: v["offset"].as_u64().unwrap_or(0) as usize }
    }

    /// Deletes the selection (if any); returns where the caret is.
    fn delete_selection(&mut self, cx: &mut Context<Self>) -> Pos {
        let sel = self.sel(cx);
        if sel.is_empty() {
            return sel.focus;
        }
        let (a, b) = sel.ordered();
        let at = self.run("text.delete", json!({ "from": Self::pos_param(a), "to": Self::pos_param(b), "coalesce": "typing" }), cx).map(|v| Self::pos_from(&v["at"])).unwrap_or(a);
        self.set_sel(TextSel::caret(TextTarget::Doc, at), cx);
        at
    }

    pub fn insert(&mut self, text_in: &str, cx: &mut Context<Self>) {
        let at = self.delete_selection(cx);
        let mut params = json!({ "text": text_in, "at": Self::pos_param(at), "coalesce": "typing" });
        if let Some(st) = &self.typing_style {
            params["style"] = json!(st);
        }
        if let Some(v) = self.run("text.insert", params, cx) {
            let p = Self::pos_from(&v["at"]);
            let keep = self.typing_style.clone();
            self.caret_to(p, false, cx);
            self.typing_style = keep;
        }
        self.goal_x = None;
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let sel = self.sel(cx);
        if !sel.is_empty() {
            self.delete_selection(cx);
            return;
        }
        let Some(c) = self.ctx(cx) else { return };
        let flow = &c.doc.pages[c.page].doc().unwrap().blocks;
        // At the start of a list item or a heading: back to a plain paragraph first, as Word does.
        if sel.focus.offset == 0 && sel.focus.cell.is_none()
            && let Some(p) = flow.get(sel.focus.block).and_then(|b| b.para())
            && (p.list.is_some() || p.level > 0)
        {
            self.run("text.paragraph", json!({ "from": sel.focus.block, "list": "none" }), cx);
            return;
        }
        let prev = text::step(flow, sel.focus, false);
        if prev == sel.focus || (prev.cell != sel.focus.cell && sel.focus.cell.is_some()) {
            return;
        }
        if let Some(v) = self.run("text.delete", json!({ "from": Self::pos_param(prev), "to": Self::pos_param(sel.focus), "coalesce": "typing" }), cx) {
            self.caret_to(Self::pos_from(&v["at"]), false, cx);
        }
    }

    fn delete_forward(&mut self, _: &DeleteForward, _: &mut Window, cx: &mut Context<Self>) {
        let sel = self.sel(cx);
        if !sel.is_empty() {
            self.delete_selection(cx);
            return;
        }
        let Some(c) = self.ctx(cx) else { return };
        let flow = &c.doc.pages[c.page].doc().unwrap().blocks;
        let next = text::step(flow, sel.focus, true);
        if next == sel.focus || (next.cell != sel.focus.cell && sel.focus.cell.is_some()) {
            return;
        }
        if let Some(v) = self.run("text.delete", json!({ "from": Self::pos_param(sel.focus), "to": Self::pos_param(next), "coalesce": "typing" }), cx) {
            self.caret_to(Self::pos_from(&v["at"]), false, cx);
        }
    }

    fn delete_word_back(&mut self, _: &DeleteWordBack, w: &mut Window, cx: &mut Context<Self>) {
        let sel = self.sel(cx);
        if sel.is_empty() {
            let p = self.word_step(sel.focus, false, cx);
            let mut s = sel;
            s.anchor = p;
            self.store.update(cx, |st, cx| st.set_text_sel(Some(s), cx));
        }
        self.backspace(&Backspace, w, cx);
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        self.insert("\n", cx);
    }

    fn shift_enter(&mut self, _: &ShiftEnter, _: &mut Window, cx: &mut Context<Self>) {
        self.insert("\n", cx);
    }

    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        self.indent(1, cx);
    }

    fn shift_tab(&mut self, _: &ShiftTab, _: &mut Window, cx: &mut Context<Self>) {
        self.indent(-1, cx);
    }

    /// Tab: in a list, a level deeper (or shallower); in a table, the next cell; else a tab.
    fn indent(&mut self, by: i32, cx: &mut Context<Self>) {
        let sel = self.sel(cx);
        let Some(c) = self.ctx(cx) else { return };
        let flow = &c.doc.pages[c.page].doc().unwrap().blocks;
        if let (Some((r, col)), Some(Block::Table(t))) = (sel.focus.cell, flow.get(sel.focus.block)) {
            let cols = t.cols();
            let idx = (r * cols + col) as i64 + by as i64;
            if idx >= 0 && (idx as usize) < t.rows.len() * cols {
                let (nr, nc) = (idx as usize / cols, idx as usize % cols);
                let len = t.cell(nr, nc).map(|x| x.as_para().len()).unwrap_or(0);
                self.store.update(cx, |s, cx| s.set_text_sel(Some(TextSel { target: TextTarget::Doc, anchor: Pos::in_cell(sel.focus.block, nr, nc, 0), focus: Pos::in_cell(sel.focus.block, nr, nc, len) }), cx));
            } else if by > 0 {
                // Tab in the last cell adds a row, as Word does.
                self.run("doc.editTable", json!({ "block": sel.focus.block, "insertRow": t.rows.len() }), cx);
                self.caret_to(Pos::in_cell(sel.focus.block, t.rows.len(), 0, 0), false, cx);
            }
            return;
        }
        let (a, b) = sel.ordered();
        let in_list = (a.block..=b.block).any(|i| flow.get(i).and_then(|x| x.para()).is_some_and(|p| p.list.is_some()));
        if in_list {
            let level = flow.get(a.block).and_then(|x| x.para()).map(|p| p.level as i32).unwrap_or(0);
            self.run("text.paragraph", json!({ "from": a.block, "to": b.block, "level": (level + by).clamp(0, 5) }), cx);
        } else if by > 0 {
            self.insert("\t", cx);
        }
    }

    fn word_step(&self, p: Pos, forward: bool, cx: &App) -> Pos {
        let Some(c) = self.ctx(cx) else { return p };
        let flow = &c.doc.pages[c.page].doc().unwrap().blocks;
        let text_of = |p: Pos| -> Vec<char> {
            match (flow.get(p.block), p.cell) {
                (Some(Block::Paragraph(q)), None) => q.text().chars().collect(),
                (Some(Block::Table(t)), Some((r, c))) => t.cell(r, c).map(|x| x.plain().chars().collect()).unwrap_or_default(),
                _ => vec![],
            }
        };
        let chars = text_of(p);
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut o = p.offset;
        if forward {
            if o >= chars.len() {
                return text::step(flow, p, true);
            }
            while o < chars.len() && !is_word(chars[o]) {
                o += 1;
            }
            while o < chars.len() && is_word(chars[o]) {
                o += 1;
            }
        } else {
            if o == 0 {
                return text::step(flow, p, false);
            }
            while o > 0 && !is_word(chars[o - 1]) {
                o -= 1;
            }
            while o > 0 && is_word(chars[o - 1]) {
                o -= 1;
            }
        }
        Pos { offset: o, ..p }
    }

    fn horizontal(&mut self, forward: bool, extend: bool, word: bool, cx: &mut Context<Self>) {
        let sel = self.sel(cx);
        let Some(c) = self.ctx(cx) else { return };
        let flow = &c.doc.pages[c.page].doc().unwrap().blocks;
        let target = if !extend && !sel.is_empty() && !word {
            let (a, b) = sel.ordered();
            if forward { b } else { a }
        } else if word {
            self.word_step(sel.focus, forward, cx)
        } else {
            text::step(flow, sel.focus, forward)
        };
        self.goal_x = None;
        self.caret_to(target, extend, cx);
    }

    fn vertical(&mut self, dir: f32, extend: bool, cx: &mut Context<Self>) {
        let Some(l) = self.layout(cx) else { return };
        let sel = self.sel(cx);
        let Some((pi, x, y, h)) = Self::caret_rect(&l, sel.focus) else { return };
        let gx = *self.goal_x.get_or_insert(x);
        let ty = if dir > 0.0 { y + h + 2.0 } else { y - 2.0 };
        let mut target = Self::hit(&l, pi, gx, ty);
        // Off the page's text: the next or previous page.
        if (dir > 0.0 && target.is_some_and(|t| t <= sel.focus)) || (dir < 0.0 && target.is_some_and(|t| t >= sel.focus)) {
            let np = if dir > 0.0 { pi + 1 } else { pi.wrapping_sub(1) };
            if np < l.pages.len() {
                target = Self::hit(&l, np, gx, if dir > 0.0 { 0.0 } else { l.height });
            }
        }
        if let Some(t) = target {
            let keep = self.goal_x;
            self.caret_to(t, extend, cx);
            self.goal_x = keep;
        }
    }

    fn line_edge(&mut self, end: bool, extend: bool, cx: &mut Context<Self>) {
        let Some(l) = self.layout(cx) else { return };
        let sel = self.sel(cx);
        let Some((pi, _, y, h)) = Self::caret_rect(&l, sel.focus) else { return };
        if let Some(t) = Self::hit(&l, pi, if end { 10_000.0 } else { -10_000.0 }, y + h / 2.0) {
            self.goal_x = None;
            self.caret_to(t, extend, cx);
        }
    }

    fn copy_selection(&mut self, cut: bool, cx: &mut Context<Self>) {
        let sel = self.sel(cx);
        if sel.is_empty() {
            return;
        }
        let (a, b) = sel.ordered();
        if let Some(v) = self.run("text.copy", json!({ "from": Self::pos_param(a), "to": Self::pos_param(b) }), cx) {
            // Plain text for other apps; folio's own blocks ride along as JSON metadata.
            let text_v = v["text"].as_str().unwrap_or("").to_string();
            let meta = json!({ "folio": v["blocks"] }).to_string();
            cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(text_v, meta));
            if cut {
                self.delete_selection(cx);
            }
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else { return };
        let at = self.delete_selection(cx);
        let blocks = item.entries().iter().find_map(|e| match e {
            gpui::ClipboardEntry::String(s) => s.metadata.as_deref().and_then(|m| serde_json::from_str::<Value>(m).ok()).and_then(|v| v.get("folio").cloned()),
            _ => None,
        });
        let params = match (blocks, item.text()) {
            (Some(b), _) if b.is_array() => json!({ "at": Self::pos_param(at), "blocks": b }),
            (_, Some(t)) => json!({ "at": Self::pos_param(at), "text": t }),
            _ => return,
        };
        if let Some(v) = self.run("text.paste", params, cx) {
            self.caret_to(Self::pos_from(&v["at"]), false, cx);
        }
    }

    /// Bold, italic…: on the selection, or for the next characters typed.
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        let sel = self.sel(cx);
        let Some(c) = self.ctx(cx) else { return };
        let flow = &c.doc.pages[c.page].doc().unwrap().blocks;
        let (a, b) = sel.ordered();
        let test = |s: &RunStyle| match key {
            "bold" => s.bold,
            "italic" => s.italic,
            "underline" => s.underline,
            "strike" => s.strike,
            _ => false,
        };
        if sel.is_empty() {
            let mut st = self.typing_style.clone().unwrap_or_else(|| text::style_at(flow, sel.focus));
            match key {
                "bold" => st.bold = !st.bold,
                "italic" => st.italic = !st.italic,
                "underline" => st.underline = !st.underline,
                "strike" => st.strike = !st.strike,
                _ => {}
            }
            self.typing_style = Some(st);
            cx.notify();
            return;
        }
        let on = !text::all_styled(flow, a, b, test);
        self.run("text.format", json!({ "from": Self::pos_param(a), "to": Self::pos_param(b), key: on }), cx);
    }

    /// Paragraph settings for the paragraphs the selection touches.
    pub fn paragraph(&mut self, params: Value, cx: &mut Context<Self>) {
        let (a, b) = self.sel(cx).ordered();
        let mut p = params;
        p["from"] = json!(a.block);
        p["to"] = json!(b.block);
        self.run("text.paragraph", p, cx);
    }

    /// Character formatting on the selection (colour, size, font, link…).
    pub fn format(&mut self, params: Value, cx: &mut Context<Self>) {
        let sel = self.sel(cx);
        let (a, b) = sel.ordered();
        if a == b {
            // Nothing selected: the word under the caret, like Word.
            let Some(c) = self.ctx(cx) else { return };
            if let Some(p) = c.doc.pages[c.page].doc().unwrap().blocks.get(a.block).and_then(|x| x.para()) {
                let (w0, w1) = text::word_at(p, a.offset);
                if w1 > w0 {
                    let mut q = params;
                    q["from"] = json!({ "block": a.block, "offset": w0 });
                    q["to"] = json!({ "block": a.block, "offset": w1 });
                    self.run("text.format", q, cx);
                }
            }
            return;
        }
        let mut q = params;
        q["from"] = Self::pos_param(a);
        q["to"] = Self::pos_param(b);
        self.run("text.format", q, cx);
    }

    /// The selection's state for the toolbar: (bold, italic, underline, strike, paragraph style, align, list).
    pub fn state(&self, cx: &App) -> Option<(bool, bool, bool, bool, folio_core::ParaStyle, folio_core::Align, Option<folio_core::ListKind>)> {
        let c = self.ctx(cx)?;
        let sel = self.sel(cx);
        let flow = &c.doc.pages[c.page].doc()?.blocks;
        let (a, b) = sel.ordered();
        let typing = self.typing_style.clone().filter(|_| sel.is_empty());
        let has = |f: &dyn Fn(&RunStyle) -> bool| typing.as_ref().map(f).unwrap_or_else(|| text::all_styled(flow, a, b, f));
        let para = flow.get(a.block).and_then(|x| x.para());
        Some((
            has(&|s| s.bold),
            has(&|s| s.italic),
            has(&|s| s.underline),
            has(&|s| s.strike),
            para.map(|p| p.style).unwrap_or_default(),
            para.map(|p| p.align).unwrap_or_default(),
            para.and_then(|p| p.list),
        ))
    }

    // ---- mouse ---------------------------------------------------------------

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(l) = self.layout(cx) else { return };
        let Some((pi, x, y)) = self.locate(&l, e.position, cx) else { return };
        let Some(p) = Self::hit(&l, pi, x, y) else { return };
        let Some(c) = self.ctx(cx) else { return };
        let flow = &c.doc.pages[c.page].doc().unwrap().blocks;
        if e.click_count == 2 {
            // A word.
            let para = match (flow.get(p.block), p.cell) {
                (Some(Block::Paragraph(q)), None) => Some(q.clone()),
                (Some(Block::Table(t)), Some((r, col))) => t.cell(r, col).map(|x| x.as_para()),
                _ => None,
            };
            if let Some(q) = para {
                let (w0, w1) = text::word_at(&q, p.offset);
                self.set_sel(TextSel { target: TextTarget::Doc, anchor: Pos { offset: w0, ..p }, focus: Pos { offset: w1, ..p } }, cx);
            }
            return;
        }
        if e.click_count >= 3 {
            let len = match (flow.get(p.block), p.cell) {
                (Some(Block::Table(t)), Some((r, col))) => t.cell(r, col).map(|x| x.as_para().len()).unwrap_or(0),
                (Some(b), _) => b.len(),
                _ => 0,
            };
            self.set_sel(TextSel { target: TextTarget::Doc, anchor: Pos { offset: 0, ..p }, focus: Pos { offset: len, ..p } }, cx);
            return;
        }
        self.dragging = true;
        self.goal_x = None;
        self.caret_to(p, e.modifiers.shift, cx);
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging || e.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some(l) = self.layout(cx) else { return };
        if let Some((pi, x, y)) = self.locate(&l, e.position, cx)
            && let Some(p) = Self::hit(&l, pi, x, y)
        {
            let sel = self.sel(cx);
            // Dragging out of a table cell selects whole blocks.
            let p = if p.cell != sel.anchor.cell && p.block == sel.anchor.block { Pos { cell: sel.anchor.cell, ..p } } else { p };
            if p != sel.focus {
                self.caret_to(p, true, cx);
            }
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.dragging = false;
    }

    fn scroll_wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(l) = self.layout(cx) else { return };
        let dy = f32::from(e.delta.pixel_delta(px(20.)).y);
        let max = (Self::content_height(&l, self.scale(cx)) - f32::from(self.bounds.get().size.height) + GAP).max(0.0);
        self.scroll = (self.scroll - dy).clamp(0.0, max);
        cx.notify();
    }

    /// Scrolls so the caret shows.
    pub fn reveal(&mut self, cx: &mut Context<Self>) {
        self.reveal = true;
        cx.notify();
    }

    fn do_reveal(&mut self, l: &DocLayout, cx: &App) {
        if !self.reveal {
            return;
        }
        self.reveal = false;
        let sel = self.sel(cx);
        let Some((pi, _, y, h)) = Self::caret_rect(l, sel.focus) else { return };
        let scale = self.scale(cx);
        let top = GAP + pi as f32 * (l.height * scale + GAP) + y * scale;
        let bottom = top + h * scale;
        let view = f32::from(self.bounds.get().size.height);
        if top < self.scroll + 20.0 {
            self.scroll = (top - 40.0).max(0.0);
        } else if bottom > self.scroll + view - 20.0 {
            self.scroll = bottom - view + 40.0;
        }
    }

    /// The page the view shows most of, for the area's title ("Page 2 of 5").
    pub fn current_page(&self, cx: &App) -> (usize, usize) {
        let Some((_, _, l)) = &self.layout else { return (1, 1) };
        let scale = self.scale(cx);
        let ph = l.height * scale + GAP;
        let mid = self.scroll + f32::from(self.bounds.get().size.height) / 2.0;
        (((mid / ph).floor() as usize).min(l.pages.len().saturating_sub(1)) + 1, l.pages.len().max(1))
    }
}

/// Where a table cell's text starts on the page.
fn cell_origin(cell: &folio_layout::doc::CellBox, _row_y: f32) -> (f32, f32) {
    (cell.tx, cell.ty)
}

impl EntityInputHandler for DocView {
    fn text_for_range(&mut self, range: std::ops::Range<usize>, actual: &mut Option<std::ops::Range<usize>>, _: &mut Window, cx: &mut Context<Self>) -> Option<String> {
        // The paragraph at the caret, in UTF-16.
        let c = self.ctx(cx)?;
        let sel = self.sel(cx);
        let text: String = match (c.doc.pages[c.page].doc()?.blocks.get(sel.focus.block)?, sel.focus.cell) {
            (Block::Paragraph(p), None) => p.text(),
            (Block::Table(t), Some((r, col))) => t.cell(r, col)?.plain(),
            _ => String::new(),
        };
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let r = range.start.min(utf16.len())..range.end.min(utf16.len());
        actual.replace(r.clone());
        Some(String::from_utf16_lossy(&utf16[r]))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, cx: &mut Context<Self>) -> Option<UTF16Selection> {
        let sel = self.sel(cx);
        let o = utf16_offset(self, sel.focus, cx);
        Some(UTF16Selection { range: o..o, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, cx: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        let (p, n) = self.marked?;
        let o = utf16_offset(self, p, cx);
        Some(o..o + n)
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, _range: Option<std::ops::Range<usize>>, new_text: &str, _: &mut Window, cx: &mut Context<Self>) {
        // A composition being committed replaces its marked text.
        if let Some((p, n)) = self.marked.take() {
            let end = Pos { offset: p.offset + n, ..p };
            self.store.update(cx, |s, cx| s.set_text_sel(Some(TextSel { target: TextTarget::Doc, anchor: p, focus: end }), cx));
        }
        if new_text.is_empty() {
            self.delete_selection(cx);
            return;
        }
        self.insert(new_text, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<std::ops::Range<usize>>, new_text: &str, _sel: Option<std::ops::Range<usize>>, window: &mut Window, cx: &mut Context<Self>) {
        let start = match self.marked {
            Some((p, _)) => p,
            None => {
                let sel = self.sel(cx);
                sel.ordered().0
            }
        };
        self.replace_text_in_range(range, new_text, window, cx);
        self.marked = (!new_text.is_empty()).then(|| (start, new_text.chars().count()));
    }

    fn bounds_for_range(&mut self, _range: std::ops::Range<usize>, element_bounds: Bounds<Pixels>, _: &mut Window, cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let l = self.layout(cx)?;
        let sel = self.sel(cx);
        let (pi, x, y, h) = Self::caret_rect(&l, sel.focus)?;
        let scale = self.scale(cx);
        let (ox, oy) = self.page_origin(&l, pi, f32::from(element_bounds.size.width), scale);
        Some(Bounds::new(point(element_bounds.origin.x + px(ox + x * scale), element_bounds.origin.y + px(oy + y * scale)), size(px(2.0), px(h * scale))))
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

fn utf16_offset(v: &DocView, p: Pos, cx: &App) -> usize {
    let Some(c) = v.ctx(cx) else { return 0 };
    let text: String = match (c.doc.pages[c.page].doc().and_then(|d| d.blocks.get(p.block)), p.cell) {
        (Some(Block::Paragraph(q)), None) => q.text(),
        (Some(Block::Table(t)), Some((r, col))) => t.cell(r, col).map(|x| x.plain()).unwrap_or_default(),
        _ => String::new(),
    };
    text.chars().take(p.offset).map(char::len_utf16).sum()
}

impl Render for DocView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let layout = self.layout(cx);
        let scale = self.scale(cx);
        let sel = self.sel(cx);
        let focused = self.focus.is_focused(window);
        let doc = self.ctx(cx).map(|c| (c.doc, c.page));
        let bounds_cell = self.bounds.clone();
        let entity = cx.entity();
        let focus = self.focus.clone();
        if let Some(l) = &layout {
            self.do_reveal(l, cx);
        }
        let scroll = self.scroll;
        div()
            .key_context("DocEditor")
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete_forward))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::shift_enter))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::shift_tab))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|v, _: &Left, _, cx| v.horizontal(false, false, false, cx)))
            .on_action(cx.listener(|v, _: &Right, _, cx| v.horizontal(true, false, false, cx)))
            .on_action(cx.listener(|v, _: &SelectLeft, _, cx| v.horizontal(false, true, false, cx)))
            .on_action(cx.listener(|v, _: &SelectRight, _, cx| v.horizontal(true, true, false, cx)))
            .on_action(cx.listener(|v, _: &WordLeft, _, cx| v.horizontal(false, false, true, cx)))
            .on_action(cx.listener(|v, _: &WordRight, _, cx| v.horizontal(true, false, true, cx)))
            .on_action(cx.listener(|v, _: &SelectWordLeft, _, cx| v.horizontal(false, true, true, cx)))
            .on_action(cx.listener(|v, _: &SelectWordRight, _, cx| v.horizontal(true, true, true, cx)))
            .on_action(cx.listener(|v, _: &Up, _, cx| v.vertical(-1.0, false, cx)))
            .on_action(cx.listener(|v, _: &Down, _, cx| v.vertical(1.0, false, cx)))
            .on_action(cx.listener(|v, _: &SelectUp, _, cx| v.vertical(-1.0, true, cx)))
            .on_action(cx.listener(|v, _: &SelectDown, _, cx| v.vertical(1.0, true, cx)))
            .on_action(cx.listener(|v, _: &LineStart, _, cx| v.line_edge(false, false, cx)))
            .on_action(cx.listener(|v, _: &LineEnd, _, cx| v.line_edge(true, false, cx)))
            .on_action(cx.listener(|v, _: &SelectLineStart, _, cx| v.line_edge(false, true, cx)))
            .on_action(cx.listener(|v, _: &SelectLineEnd, _, cx| v.line_edge(true, true, cx)))
            .on_action(cx.listener(|v, _: &DocStart, _, cx| v.caret_to(Pos::default(), false, cx)))
            .on_action(cx.listener(|v, _: &DocEnd, _, cx| {
                if let Some(c) = v.ctx(cx) {
                    let e = text::end(&c.doc.pages[c.page].doc().unwrap().blocks);
                    v.caret_to(e, false, cx);
                }
            }))
            .on_action(cx.listener(|v, _: &SelectAll, _, cx| {
                if let Some(c) = v.ctx(cx) {
                    let e = text::end(&c.doc.pages[c.page].doc().unwrap().blocks);
                    v.set_sel(TextSel { target: TextTarget::Doc, anchor: Pos::default(), focus: e }, cx);
                }
            }))
            .on_action(cx.listener(|v, _: &Copy, _, cx| v.copy_selection(false, cx)))
            .on_action(cx.listener(|v, _: &Cut, _, cx| v.copy_selection(true, cx)))
            .on_action(cx.listener(|v, _: &Bold, _, cx| v.toggle("bold", cx)))
            .on_action(cx.listener(|v, _: &Italic, _, cx| v.toggle("italic", cx)))
            .on_action(cx.listener(|v, _: &Underline, _, cx| v.toggle("underline", cx)))
            .on_action(cx.listener(|v, _: &Strike, _, cx| v.toggle("strike", cx)))
            .on_action(cx.listener(|v, _: &Heading1, _, cx| v.paragraph(json!({ "style": "heading1" }), cx)))
            .on_action(cx.listener(|v, _: &Heading2, _, cx| v.paragraph(json!({ "style": "heading2" }), cx)))
            .on_action(cx.listener(|v, _: &Heading3, _, cx| v.paragraph(json!({ "style": "heading3" }), cx)))
            .on_action(cx.listener(|v, _: &NormalText, _, cx| v.paragraph(json!({ "style": "normal" }), cx)))
            .on_action(cx.listener(|v, _: &BulletList, _, cx| v.toggle_list("bullet", cx)))
            .on_action(cx.listener(|v, _: &NumberList, _, cx| v.toggle_list("number", cx)))
            .on_action(cx.listener(|v, _: &CheckList, _, cx| v.toggle_list("check", cx)))
            .on_action(cx.listener(|v, _: &AlignLeft, _, cx| v.paragraph(json!({ "align": "left" }), cx)))
            .on_action(cx.listener(|v, _: &AlignCenter, _, cx| v.paragraph(json!({ "align": "center" }), cx)))
            .on_action(cx.listener(|v, _: &AlignRight, _, cx| v.paragraph(json!({ "align": "right" }), cx)))
            .on_action(cx.listener(|v, _: &AlignJustify, _, cx| v.paragraph(json!({ "align": "justify" }), cx)))
            .on_action(cx.listener(|v, _: &PageBreak, _, cx| {
                let sel = v.sel(cx);
                v.run("doc.insertPageBreak", json!({ "after": sel.focus.block }), cx);
            }))
            .on_action(cx.listener(|v, _: &Escape, _, cx| {
                let s = v.sel(cx);
                v.caret_to(s.focus, false, cx);
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_scroll_wheel(cx.listener(Self::scroll_wheel))
            .size_full()
            .overflow_hidden()
            .bg(t.desk)
            .child(
                canvas(
                    move |bounds, _, _| {
                        bounds_cell.set(bounds);
                        bounds
                    },
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                        let (Some(l), Some((doc, page))) = (layout, doc) else { return };
                        paint_pages(window, cx, &l, &doc, page, bounds, scroll, scale, sel, focused);
                    },
                )
                .size_full(),
            )
    }
}

impl DocView {
    fn toggle_list(&mut self, kind: &str, cx: &mut Context<Self>) {
        let on = self.state(cx).and_then(|s| s.6).map(|l| format!("{l:?}").to_lowercase());
        let next = if on.as_deref() == Some(kind) { "none" } else { kind };
        self.paragraph(json!({ "list": next }), cx);
    }
}

/// Paints the visible pages: paper, its shadow, the content, the selection and the caret.
#[allow(clippy::too_many_arguments)]
fn paint_pages(window: &mut Window, cx: &mut App, l: &DocLayout, doc: &Document, page_index: usize, bounds: Bounds<Pixels>, scroll: f32, scale: f32, sel: TextSel, focused: bool) {
    let t = cx.theme().clone();
    let w = f32::from(bounds.size.width);
    let view_h = f32::from(bounds.size.height);
    let pw = l.width * scale;
    let ph = l.height * scale;
    let x0 = ((w - pw) / 2.0).max(GAP);
    let (sa, sb) = sel.ordered();
    let margin_x = doc.pages.get(page_index).and_then(|p| p.doc()).map(|t| t.setup.margin_left).unwrap_or(72.0);
    for (pi, page) in l.pages.iter().enumerate() {
        let y0 = GAP + pi as f32 * (ph + GAP) - scroll;
        if y0 > view_h || y0 + ph < 0.0 {
            continue;
        }
        let origin = point(bounds.origin.x + px(x0), bounds.origin.y + px(y0));
        let at = |x: f32, y: f32| point(origin.x + px(x * scale), origin.y + px(y * scale));
        // The hard offset shadow of the design system, then the paper.
        window.paint_quad(fill(Bounds::new(point(origin.x + px(4.), origin.y + px(4.)), size(px(pw), px(ph))), t.drop));
        window.paint_quad(fill(Bounds::new(origin, size(px(pw), px(ph))), t.paper));
        // Selection behind the text.
        let sel_color = gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.14 };
        for item in &page.items {
            match item {
                Placed::Para { block, x, y, layout, lines } => {
                    let dy = layout.lines.get(lines.start).map(|l| l.y).unwrap_or(0.0);
                    if sb.block >= *block && sa.block <= *block && sa != sb && sa.cell.is_none() {
                        let from = if sa.block == *block { sa.offset } else { 0 };
                        let to = if sb.block == *block { sb.offset } else { usize::MAX };
                        for (rx, ry, rw, rh) in layout.rects(from, to) {
                            if ry + 0.01 < layout.lines.get(lines.start).map(|l| l.y).unwrap_or(0.0) || ry > layout.lines.get(lines.end.saturating_sub(1)).map(|l| l.y).unwrap_or(0.0) {
                                continue;
                            }
                            window.paint_quad(fill(Bounds::new(at(x + rx, y + ry - dy), size(px(rw * scale), px(rh * scale))), sel_color));
                        }
                    }
                    paint::para(window, layout, at(*x, *y), scale, lines.clone(), dy, None);
                }
                Placed::Caption { x, y, layout, .. } => {
                    let n = layout.lines.len();
                    paint::para(window, layout, at(*x, *y), scale, 0..n, 0.0, None);
                }
                Placed::TableRow { block, row, x, y, w: rw, height, cells, header, shaded, .. } => {
                    if *shaded {
                        window.paint_quad(fill(Bounds::new(at(*x, *y), size(px(rw * scale), px(height * scale))), gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.035 }));
                    }
                    for c in cells {
                        if let Some(f) = c.fill {
                            window.paint_quad(fill(Bounds::new(at(c.x, *y), size(px(c.w * scale), px(height * scale))), paint::rgba(f)));
                        }
                        let (tx, ty) = cell_origin(c, *y);
                        if sa.block == *block && sb.block == *block && sa.cell == Some((*row, c.col)) && sa != sb {
                            for (rx, ry, rw2, rh) in c.layout.rects(sa.offset, sb.offset) {
                                window.paint_quad(fill(Bounds::new(at(tx + rx, ty + ry), size(px(rw2 * scale), px(rh * scale))), sel_color));
                            }
                        }
                        let n = c.layout.lines.len();
                        paint::para(window, &c.layout, at(tx, ty), scale, 0..n, 0.0, None);
                    }
                    // Grid lines.
                    let line = gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: if *header { 0.55 } else { 0.22 } };
                    let thin = px((0.75 * scale).max(1.0));
                    window.paint_quad(fill(Bounds::new(at(*x, *y), size(px(rw * scale), thin)), line));
                    window.paint_quad(fill(Bounds::new(at(*x, y + height), size(px(rw * scale), thin)), gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.22 }));
                    for c in cells {
                        window.paint_quad(fill(Bounds::new(at(c.x, *y), size(thin, px(height * scale))), gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.22 }));
                    }
                    window.paint_quad(fill(Bounds::new(at(x + rw, *y), size(thin, px(height * scale))), gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.22 }));
                }
                Placed::Image { block, x, y, w: iw, h: ih, media } => {
                    let b = Bounds::new(at(*x, *y), size(px(iw * scale), px(ih * scale)));
                    match doc.media.get(media).and_then(|m| paint::image(&m.bytes, 2048)) {
                        Some(img) => {
                            let _ = window.paint_image(b, b, Default::default(), img, 0, false);
                        }
                        None => window.paint_quad(fill(b, gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.06 })),
                    }
                    if sa.block <= *block && sb.block >= *block && sa != sb {
                        window.paint_quad(fill(b, sel_color));
                    }
                }
                Placed::Chart { block, x, y, w: cw, h: ch } => {
                    let chart = match doc.pages.get(page_index).and_then(|p| p.doc()).and_then(|t| t.blocks.get(*block)) {
                        Some(Block::Chart(c)) => Some(c.chart.clone()),
                        _ => None,
                    };
                    if let Some(chart) = chart {
                        let style = folio_layout::chart::ChartStyle { text: [30, 30, 30, 255], grid: [0, 0, 0, 36], series: vec![[12, 12, 12, 255], [130, 130, 130, 255], [200, 200, 200, 255], [70, 70, 70, 255]], size: 9.0, background: None };
                        let data = folio_core::links::chart_data(doc, &chart).unwrap_or_default();
                        let prims = folio_layout::chart_prims(&chart, &data, *cw, *ch, &style);
                        paint::prims(window, cx, &prims, at(*x, *y), scale);
                    }
                    if sa.block <= *block && sb.block >= *block && sa != sb {
                        window.paint_quad(fill(Bounds::new(at(*x, *y), size(px(cw * scale), px(ch * scale))), sel_color));
                    }
                }
                Placed::PageBreak { y, .. } => {
                    let mut xx = 0.0;
                    while xx < l.width - 144.0 {
                        window.paint_quad(fill(Bounds::new(at(72.0 + xx, *y + 6.0), size(px(4.0 * scale), px(1.0))), gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.3 }));
                        xx += 8.0;
                    }
                }
            }
        }
        if let Some((hy, hl)) = &page.header {
            let n = hl.lines.len();
            paint::para(window, hl, at(margin_x, *hy), scale, 0..n, 0.0, Some(gpui::Rgba { r: 0.35, g: 0.35, b: 0.35, a: 1.0 }.into()));
        }
        if let Some((fy, fl)) = &page.footer {
            let n = fl.lines.len();
            paint::para(window, fl, at(margin_x, *fy), scale, 0..n, 0.0, Some(gpui::Rgba { r: 0.35, g: 0.35, b: 0.35, a: 1.0 }.into()));
        }
        for (ny, nl) in &page.footnotes {
            let n = nl.lines.len();
            paint::para(window, nl, at(margin_x, *ny), scale, 0..n, 0.0, None);
        }
    }
    // The caret.
    if focused && sel.is_empty()
        && let Some((pi, cx_, cy, ch)) = DocView::caret_rect(l, sel.focus)
    {
        let y0 = GAP + pi as f32 * (ph + GAP) - scroll;
        let b = Bounds::new(point(bounds.origin.x + px(x0 + cx_ * scale), bounds.origin.y + px(y0 + cy * scale)), size(px(1.6), px(ch * scale)));
        window.paint_quad(fill(b, gpui::black()));
    }
}

