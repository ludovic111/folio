//! The sheet editor: a formula bar over a grid of cells, as in Excel and Google Sheets.
//!
//! Selecting is the window's own business (the selection is pushed to `ui.state`); every
//! change goes through `sheet.*` commands. Typing on a cell starts editing it; Enter and Tab
//! commit (`sheet.set`) and move, Escape cancels.

use std::cell::Cell;
use std::rc::Rc;

use folio_calc::{Addr, Range, Value as CellValue, col_name};
use folio_core::sheet::Sheet;
use folio_core::{Document, Id, PageKind};
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Render, ScrollWheelEvent, SharedString, Subscription, TextRun, UTF16Selection, Window, canvas, div, fill, point, prelude::*, px, size,
};
use serde_json::{Value, json};

use crate::actions::*;
use crate::store::{Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};

/// Header sizes in pixels at 100 %.
const HEAD_H: f32 = 24.0;
const HEAD_W: f32 = 46.0;

pub struct SheetView {
    store: Entity<Store>,
    focus: FocusHandle,
    /// The cell being edited in place, and the formula bar.
    cell_input: Entity<TextInput>,
    fx: Entity<TextInput>,
    /// Editing the active cell (in place or in the formula bar).
    pub editing: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Scroll in pixels (unzoomed).
    scroll: (f32, f32),
    dragging: Option<Drag>,
    /// The cell the formula bar shows (to refill it when the active cell changes).
    shown: Option<(Id, Addr, u64)>,
    /// Cells cut with ⌘X (moved on paste).
    cut_source: Option<(Id, Range)>,
    _subs: Vec<Subscription>,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Cells,
    /// Resizing a column (index, its width when the drag started, the x it started at).
    Column(u32, f32, f32),
    Row(u32, f32, f32),
}

impl Focusable for SheetView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

struct Ctx {
    doc: Document,
    page: usize,
    id: Id,
}

impl SheetView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        let cell_input = cx.new(|cx| {
            let mut i = TextInput::new(cx);
            i.bare = true;
            i
        });
        let fx = cx.new(|cx| TextInput::new(cx).placeholder("Value or =formula"));
        let mut subs = vec![];
        for input in [cell_input.clone(), fx.clone()] {
            subs.push(cx.subscribe_in(&input, window, |v: &mut Self, src, e: &InputEvent, window, cx| match e {
                InputEvent::Submit => {
                    let shift = window.modifiers().shift;
                    v.commit(src.read(cx).text().to_string(), if shift { (-1, 0) } else { (1, 0) }, window, cx);
                }
                InputEvent::Cancel => v.cancel(window, cx),
                InputEvent::Changed(text) => {
                    // Keep the two fields in step.
                    let other = if src == &v.fx { v.cell_input.clone() } else { v.fx.clone() };
                    let text = text.clone();
                    other.update(cx, |i, cx| i.set_text(text, cx));
                    v.editing = true;
                }
                InputEvent::Blur => {}
            }));
        }
        Self { store, focus: cx.focus_handle(), cell_input, fx, editing: false, bounds: Rc::new(Cell::new(Bounds::default())), scroll: (0.0, 0.0), dragging: None, shown: None, cut_source: None, _subs: subs }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    fn ctx(&self, cx: &App) -> Option<Ctx> {
        let s = self.store.read(cx);
        let doc = s.doc.clone()?;
        let page = s.page_index()?;
        if doc.pages[page].kind() != PageKind::Sheet {
            return None;
        }
        let id = doc.pages[page].id.clone();
        Some(Ctx { doc, page, id })
    }

    fn zoom(&self, cx: &App) -> f32 {
        self.store.read(cx).zoom
    }

    fn run(&mut self, name: &str, mut params: Value, cx: &mut Context<Self>) -> Option<Value> {
        let c = self.ctx(cx)?;
        params["page"] = json!(c.id.to_string());
        self.store.update(cx, |s, cx| s.run_now(name, params, cx)).ok()
    }

    fn active(&self, cx: &App) -> (Addr, Addr) {
        let v = self.store.read(cx).view();
        (v.cell, v.anchor)
    }

    fn select(&mut self, cell: Addr, extend: bool, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            let v = s.view_mut();
            v.cell = cell;
            if !extend {
                v.anchor = cell;
            }
            s.sync_ui();
            cx.notify();
        });
        self.reveal(cell, cx);
    }

    /// Scrolls so a cell shows.
    fn reveal(&mut self, a: Addr, cx: &mut Context<Self>) {
        let Some(c) = self.ctx(cx) else { return };
        let sh = c.doc.pages[c.page].sheet().unwrap();
        let z = self.zoom(cx);
        let b = self.bounds.get();
        let (vw, vh) = ((f32::from(b.size.width) / z - HEAD_W).max(50.0), (f32::from(b.size.height) / z - HEAD_H).max(50.0));
        let fx: f32 = (0..sh.freeze_cols).map(|i| sh.col_width(i)).sum();
        let fy: f32 = (0..sh.freeze_rows).map(|i| sh.row_height(i)).sum();
        if a.col >= sh.freeze_cols {
            let x0: f32 = (sh.freeze_cols..a.col).map(|i| sh.col_width(i)).sum();
            let x1 = x0 + sh.col_width(a.col);
            if x0 < self.scroll.0 {
                self.scroll.0 = x0;
            } else if x1 > self.scroll.0 + vw - fx {
                self.scroll.0 = x1 - (vw - fx);
            }
        }
        if a.row >= sh.freeze_rows {
            let y0: f32 = (sh.freeze_rows..a.row).map(|i| sh.row_height(i)).sum();
            let y1 = y0 + sh.row_height(a.row);
            if y0 < self.scroll.1 {
                self.scroll.1 = y0;
            } else if y1 > self.scroll.1 + vh - fy {
                self.scroll.1 = y1 - (vh - fy);
            }
        }
        cx.notify();
    }

    fn move_by(&mut self, dr: i64, dc: i64, extend: bool, cx: &mut Context<Self>) {
        let (cell, _) = self.active(cx);
        let hidden = self.ctx(cx).map(|c| c.doc.pages[c.page].sheet().unwrap().hidden_rows()).unwrap_or_default();
        let mut row = (cell.row as i64 + dr).clamp(0, folio_calc::MAX_ROWS as i64 - 1) as u32;
        while dr != 0 && hidden.contains(&row) && row > 0 {
            row = (row as i64 + dr.signum()).clamp(0, folio_calc::MAX_ROWS as i64 - 1) as u32;
        }
        let col = (cell.col as i64 + dc).clamp(0, folio_calc::MAX_COLS as i64 - 1) as u32;
        self.select(Addr::new(row, col), extend, cx);
    }

    /// ⌘ + arrow: to the edge of the data, as spreadsheets do.
    fn jump(&mut self, dr: i64, dc: i64, cx: &mut Context<Self>) {
        let Some(c) = self.ctx(cx) else { return };
        let sh = c.doc.pages[c.page].sheet().unwrap();
        let (mut a, _) = self.active(cx);
        let filled = |a: Addr| !sh.input(a).is_empty();
        let step = |a: Addr| -> Option<Addr> {
            let r = a.row as i64 + dr;
            let c2 = a.col as i64 + dc;
            (r >= 0 && c2 >= 0 && r < 100_000 && c2 < 16_384).then(|| Addr::new(r as u32, c2 as u32))
        };
        let Some(first) = step(a) else { return };
        if filled(a) && filled(first) {
            while let Some(n) = step(a) {
                if !filled(n) {
                    break;
                }
                a = n;
            }
        } else {
            a = first;
            while !filled(a) {
                match step(a) {
                    Some(n) if (n.row as usize) < sh.used_range().map(|r| r.end.row as usize + 1).unwrap_or(0) + 1 || dr <= 0 => a = n,
                    _ => break,
                }
                if a.row == 0 && dr < 0 || a.col == 0 && dc < 0 {
                    break;
                }
            }
        }
        self.select(a, false, cx);
    }

    // ---- editing -------------------------------------------------------------

    /// Starts editing the active cell, with `text` (typed) or its content.
    pub fn begin_edit(&mut self, text: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.ctx(cx) else { return };
        let (cell, _) = self.active(cx);
        let content = text.unwrap_or_else(|| c.doc.pages[c.page].sheet().unwrap().input(cell).to_string());
        self.editing = true;
        for i in [self.cell_input.clone(), self.fx.clone()] {
            i.update(cx, |i, cx| i.set_text(content.clone(), cx));
        }
        crate::ui::input::focus(&self.cell_input, window, cx);
        cx.notify();
    }

    fn commit(&mut self, text_v: String, (dr, dc): (i64, i64), window: &mut Window, cx: &mut Context<Self>) {
        let (cell, _) = self.active(cx);
        let before = self.ctx(cx).map(|c| c.doc.pages[c.page].sheet().unwrap().input(cell).to_string()).unwrap_or_default();
        self.editing = false;
        if text_v != before {
            self.run("sheet.set", json!({ "cell": cell.a1(), "value": text_v }), cx);
        }
        self.move_by(dr, dc, false, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = false;
        self.shown = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        let v = self.store.read(cx).view();
        self.run("sheet.clear", json!({ "range": v.range().a1() }), cx);
    }

    fn copy(&mut self, cut: bool, cx: &mut Context<Self>) {
        let Some(c) = self.ctx(cx) else { return };
        let sh = c.doc.pages[c.page].sheet().unwrap();
        let r = self.store.read(cx).view().range();
        let r = if r.rows() as u64 * r.cols() as u64 > 100_000 { sh.clip(r) } else { r };
        let text_v: String = (r.start.row..=r.end.row).map(|row| (r.start.col..=r.end.col).map(|col| sh.display(Addr::new(row, col))).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n");
        let meta = json!({ "folioCells": { "page": c.id.to_string(), "range": r.a1() } }).to_string();
        cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(text_v, meta));
        if cut {
            self.cut_source = Some((c.id, r));
        } else {
            self.cut_source = None;
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else { return };
        let (cell, _) = self.active(cx);
        // From a folio sheet: copy the cells themselves (formulas move their references).
        let meta = item.entries().iter().find_map(|e| match e {
            gpui::ClipboardEntry::String(s) => s.metadata.as_deref().and_then(|m| serde_json::from_str::<Value>(m).ok()),
            _ => None,
        });
        if let Some(m) = meta.as_ref().and_then(|m| m.get("folioCells"))
            && let (Some(page), Some(range)) = (m["page"].as_str(), m["range"].as_str())
        {
            let mv = self.cut_source.as_ref().is_some_and(|(p, r)| p.as_str() == page && r.a1() == range);
            let me = self.ctx(cx).map(|c| c.id.to_string()).unwrap_or_default();
            let params = json!({ "page": page, "from": range, "to": cell.a1(), "toPage": me, "move": mv });
            self.store.update(cx, |s, cx| {
                let _ = s.run_now("sheet.copy", params, cx);
            });
            self.cut_source = None;
            return;
        }
        // Text from elsewhere: rows by lines, cells by tabs.
        if let Some(t) = item.text() {
            let rows: Vec<Vec<String>> = t.trim_end_matches('\n').split('\n').map(|l| l.trim_end_matches('\r').split('\t').map(str::to_string).collect()).collect();
            self.run("sheet.setRange", json!({ "at": cell.a1(), "values": rows }), cx);
        }
    }

    // ---- geometry ------------------------------------------------------------

    /// Column x positions (unzoomed px from the grid's left, after the row headers) for the
    /// visible columns: (col, x, width).
    fn columns(sh: &Sheet, scroll_x: f32, width: f32) -> Vec<(u32, f32, f32)> {
        let mut out = vec![];
        let mut x = 0.0;
        for c in 0..sh.freeze_cols {
            let w = sh.col_width(c);
            out.push((c, x, w));
            x += w;
        }
        let frozen = x;
        // Skip scrolled-off columns.
        let mut c = sh.freeze_cols;
        let mut sx = 0.0;
        while c < folio_calc::MAX_COLS && sx + sh.col_width(c) <= scroll_x {
            sx += sh.col_width(c);
            c += 1;
        }
        let mut x = frozen + sx - scroll_x;
        while c < folio_calc::MAX_COLS && x < width {
            let w = sh.col_width(c);
            out.push((c, x, w));
            x += w;
            c += 1;
        }
        out
    }

    fn rows(sh: &Sheet, scroll_y: f32, height: f32) -> Vec<(u32, f32, f32)> {
        let hidden = sh.hidden_rows();
        let h_of = |r: u32| if hidden.contains(&r) { 0.0 } else { sh.row_height(r) };
        let mut out = vec![];
        let mut y = 0.0;
        for r in 0..sh.freeze_rows {
            let h = h_of(r);
            out.push((r, y, h));
            y += h;
        }
        let frozen = y;
        let mut r = sh.freeze_rows;
        let mut sy = 0.0;
        while r < folio_calc::MAX_ROWS && sy + h_of(r) <= scroll_y {
            sy += h_of(r);
            r += 1;
        }
        let mut y = frozen + sy - scroll_y;
        while r < folio_calc::MAX_ROWS && y < height {
            let h = h_of(r);
            if h > 0.0 {
                out.push((r, y, h));
            }
            y += h;
            r += 1;
        }
        out
    }

    /// The cell (or header) under a window position.
    fn hit(&self, pos: Point<Pixels>, cx: &App) -> Option<Hit> {
        let c = self.ctx(cx)?;
        let sh = c.doc.pages[c.page].sheet().unwrap();
        let z = self.zoom(cx);
        let b = self.bounds.get();
        let x = f32::from(pos.x - b.origin.x) / z;
        let y = f32::from(pos.y - b.origin.y) / z;
        let (w, h) = (f32::from(b.size.width) / z, f32::from(b.size.height) / z);
        let cols = Self::columns(sh, self.scroll.0, w - HEAD_W);
        let rows = Self::rows(sh, self.scroll.1, h - HEAD_H);
        let col = cols.iter().find(|(_, cx0, cw)| x - HEAD_W >= *cx0 && x - HEAD_W < cx0 + cw).map(|c| c.0);
        let row = rows.iter().find(|(_, ry, rh)| y - HEAD_H >= *ry && y - HEAD_H < ry + rh).map(|r| r.0);
        // Column borders in the header resize.
        if y < HEAD_H {
            if let Some((ci, cx0, cw)) = cols.iter().find(|(_, cx0, cw)| ((x - HEAD_W) - (cx0 + cw)).abs() < 4.0) {
                let _ = cx0;
                return Some(Hit::ColBorder(*ci, *cw));
            }
            return col.map(Hit::Col);
        }
        if x < HEAD_W {
            if let Some((ri, _, rh)) = rows.iter().find(|(_, ry, rh)| ((y - HEAD_H) - (ry + rh)).abs() < 3.0) {
                return Some(Hit::RowBorder(*ri, *rh));
            }
            return row.map(Hit::Row);
        }
        Some(Hit::Cell(Addr::new(row?, col?)))
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing {
            // Clicking another cell commits what was typed (formula references by click: not yet).
            let t = self.cell_input.read(cx).text().to_string();
            self.commit(t, (0, 0), window, cx);
        }
        window.focus(&self.focus, cx);
        let Some(hit) = self.hit(e.position, cx) else { return };
        let used = self.ctx(cx).and_then(|c| c.doc.pages[c.page].sheet().unwrap().used_range());
        match hit {
            Hit::Cell(a) => {
                if e.click_count >= 2 {
                    self.select(a, false, cx);
                    self.begin_edit(None, window, cx);
                    return;
                }
                self.select(a, e.modifiers.shift, cx);
                self.dragging = Some(Drag::Cells);
            }
            Hit::Col(c) => {
                let end = used.map(|r| r.end.row.max(99)).unwrap_or(999);
                self.store.update(cx, |s, cx| {
                    let v = s.view_mut();
                    v.anchor = Addr::new(0, if e.modifiers.shift { v.anchor.col } else { c });
                    v.cell = Addr::new(end, c);
                    s.sync_ui();
                    cx.notify();
                });
            }
            Hit::Row(r) => {
                let end = used.map(|u| u.end.col.max(25)).unwrap_or(25);
                self.store.update(cx, |s, cx| {
                    let v = s.view_mut();
                    v.anchor = Addr::new(if e.modifiers.shift { v.anchor.row } else { r }, 0);
                    v.cell = Addr::new(r, end);
                    s.sync_ui();
                    cx.notify();
                });
            }
            Hit::ColBorder(c, w) => self.dragging = Some(Drag::Column(c, w, f32::from(e.position.x))),
            Hit::RowBorder(r, h) => self.dragging = Some(Drag::Row(r, h, f32::from(e.position.y))),
        }
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.dragging else { return };
        if e.pressed_button != Some(MouseButton::Left) {
            self.dragging = None;
            return;
        }
        let z = self.zoom(cx);
        match d {
            Drag::Cells => {
                if let Some(Hit::Cell(a)) = self.hit(e.position, cx) {
                    let (cell, _) = self.active(cx);
                    if a != cell {
                        self.select(a, true, cx);
                    }
                }
            }
            Drag::Column(c, w, x0) => {
                let nw = (w + (f32::from(e.position.x) - x0) / z).clamp(20.0, 1200.0).round();
                self.run("sheet.resize", json!({ "columns": { col_name(c): nw }, "coalesce": format!("gesture:col{c}") }), cx);
            }
            Drag::Row(r, h, y0) => {
                let nh = (h + (f32::from(e.position.y) - y0) / z).clamp(12.0, 600.0).round();
                self.run("sheet.resize", json!({ "rows": { (r + 1).to_string(): nh }, "coalesce": format!("gesture:row{r}") }), cx);
            }
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.dragging = None;
    }

    fn scroll_wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let d = e.delta.pixel_delta(px(24.));
        let z = self.zoom(cx);
        self.scroll.0 = (self.scroll.0 - f32::from(d.x) / z).max(0.0);
        self.scroll.1 = (self.scroll.1 - f32::from(d.y) / z).max(0.0);
        cx.notify();
    }

    /// Sum, average and count of the selected numbers ("Sum 1,234").
    pub fn stats(&self, cx: &App) -> Option<String> {
        let c = self.ctx(cx)?;
        let sh = c.doc.pages[c.page].sheet()?;
        let r = self.store.read(cx).view().range();
        if r.rows() == 1 && r.cols() == 1 {
            return None;
        }
        let r = sh.clip(r);
        let mut sum = 0.0;
        let mut n = 0usize;
        let mut count = 0usize;
        for (a, cell) in sh.cells.iter() {
            if !r.contains(*a) {
                continue;
            }
            if !cell.value.is_empty() {
                count += 1;
            }
            if let CellValue::Number(v) = cell.value {
                sum += v;
                n += 1;
            }
        }
        if count == 0 {
            return None;
        }
        let f = |v: f64| folio_calc::format_value(&CellValue::Number((v * 1e6).round() / 1e6), Some("#,##0.##"));
        Some(if n > 0 { format!("Sum {} · Average {} · Count {count}", f(sum), f(sum / n as f64)) } else { format!("Count {count}") })
    }

    /// The active cell's name and what it holds, for the formula bar.
    fn sync_fx(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.ctx(cx) else { return };
        let (cell, _) = self.active(cx);
        let version = self.store.read(cx).version;
        if self.editing || self.shown == Some((c.id.clone(), cell, version)) {
            return;
        }
        self.shown = Some((c.id.clone(), cell, version));
        let input = c.doc.pages[c.page].sheet().unwrap().input(cell).to_string();
        self.fx.update(cx, |i, cx| i.set_text(input, cx));
    }

    /// Insert a chart of the selection (toolbar).
    pub fn chart_selection(&mut self, kind: &str, cx: &mut Context<Self>) {
        let r = self.store.read(cx).view().range();
        let r = if r.rows() == 1 && r.cols() == 1 { self.ctx(cx).and_then(|c| c.doc.pages[c.page].sheet().unwrap().used_range()).unwrap_or(r) } else { r };
        self.run("sheet.addChart", json!({ "range": r.a1(), "kind": kind }), cx);
    }

    /// Applies a format to the selection (toolbar and inspector).
    pub fn format(&mut self, params: Value, cx: &mut Context<Self>) {
        let r = self.store.read(cx).view().range();
        let mut p = params;
        p["range"] = json!(r.a1());
        self.run("sheet.format", p, cx);
    }

    /// The active cell's format, for the toolbar's state.
    pub fn active_format(&self, cx: &App) -> folio_core::CellFormat {
        let Some(c) = self.ctx(cx) else { return Default::default() };
        let (cell, _) = self.active(cx);
        c.doc.pages[c.page].sheet().unwrap().cell(cell).map(|c| c.format.clone()).unwrap_or_default()
    }

    pub fn sort(&mut self, descending: bool, cx: &mut Context<Self>) {
        let Some(c) = self.ctx(cx) else { return };
        let v = self.store.read(cx).view();
        let r = v.range();
        let r = if r.rows() == 1 { c.doc.pages[c.page].sheet().unwrap().used_range().unwrap_or(r) } else { r };
        self.run("sheet.sort", json!({ "range": r.a1(), "by": [{ "column": col_name(v.cell.col), "descending": descending }] }), cx);
    }

    pub fn autosum(&mut self, cx: &mut Context<Self>) {
        // The numbers above the active cell, like Excel's AutoSum.
        let Some(c) = self.ctx(cx) else { return };
        let sh = c.doc.pages[c.page].sheet().unwrap();
        let (cell, _) = self.active(cx);
        let mut top = cell.row;
        while top > 0 && matches!(sh.value(Addr::new(top - 1, cell.col)), CellValue::Number(_)) {
            top -= 1;
        }
        if top == cell.row {
            return;
        }
        let f = format!("=SUM({}:{})", Addr::new(top, cell.col).a1(), Addr::new(cell.row - 1, cell.col).a1());
        self.run("sheet.set", json!({ "cell": cell.a1(), "value": f }), cx);
    }
}

#[derive(Clone, Copy, Debug)]
enum Hit {
    Cell(Addr),
    Col(u32),
    Row(u32),
    ColBorder(u32, f32),
    RowBorder(u32, f32),
}

impl EntityInputHandler for SheetView {
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
    fn replace_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Typing on a cell starts editing it with what was typed.
        if !text.is_empty() && !self.editing {
            self.begin_edit(Some(text.to_string()), window, cx);
        }
    }
    fn replace_and_mark_text_in_range(&mut self, r: Option<std::ops::Range<usize>>, text: &str, _: Option<std::ops::Range<usize>>, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(r, text, window, cx);
    }
    fn bounds_for_range(&mut self, _: std::ops::Range<usize>, b: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        Some(b)
    }
    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Render for SheetView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_fx(cx);
        let t = cx.theme().clone();
        let Some(c) = self.ctx(cx) else { return div().into_any_element() };
        let view = self.store.read(cx).view();
        let z = self.zoom(cx);
        let sheet = c.doc.pages[c.page].sheet().unwrap().clone();
        let doc = c.doc.clone();
        let (cell, _) = (view.cell, view.anchor);
        let range = view.range();
        let scroll = self.scroll;
        let bounds_cell = self.bounds.clone();
        let focus = self.focus.clone();
        let entity = cx.entity();
        let editing = self.editing;
        // Where the in-place editor goes.
        let edit_box = {
            let b = self.bounds.get();
            let cols = Self::columns(&sheet, scroll.0, f32::from(b.size.width) / z - HEAD_W);
            let rows = Self::rows(&sheet, scroll.1, f32::from(b.size.height) / z - HEAD_H);
            match (cols.iter().find(|x| x.0 == cell.col), rows.iter().find(|x| x.0 == cell.row)) {
                (Some(&(_, x, w)), Some(&(_, y, h))) => Some((x + HEAD_W, y + HEAD_H, w, h)),
                _ => None,
            }
        };
        let name_box = if range.rows() > 1 || range.cols() > 1 { range.a1() } else { cell.a1() };
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                // The formula bar: the cell's name, fx, what it holds.
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .h(px(40.))
                    .px(px(10.))
                    .border_b_1()
                    .border_color(t.line)
                    .bg(t.bg_raised)
                    .child(div().flex_none().w(px(84.)).h(px(26.)).px(px(8.)).flex().items_center().border_1().border_color(t.line_strong).font_family(MONO).text_size(px(sz::SM)).child(name_box))
                    .child(div().flex_none().font_family(MONO).text_size(px(sz::SM)).text_color(t.text_2).font_weight(FontWeight::SEMIBOLD).child("fx"))
                    .child(div().flex_1().min_w_0().font_family(MONO).child(self.fx.clone())),
            )
            .child(
                div()
                    .key_context("SheetEditor")
                    .track_focus(&self.focus)
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .cursor(CursorStyle::Arrow)
                    .bg(t.bg_raised)
                    .on_action(cx.listener(|v, _: &Up, _, cx| v.move_by(-1, 0, false, cx)))
                    .on_action(cx.listener(|v, _: &Down, _, cx| v.move_by(1, 0, false, cx)))
                    .on_action(cx.listener(|v, _: &Left, _, cx| v.move_by(0, -1, false, cx)))
                    .on_action(cx.listener(|v, _: &Right, _, cx| v.move_by(0, 1, false, cx)))
                    .on_action(cx.listener(|v, _: &SelectUp, _, cx| v.move_by(-1, 0, true, cx)))
                    .on_action(cx.listener(|v, _: &SelectDown, _, cx| v.move_by(1, 0, true, cx)))
                    .on_action(cx.listener(|v, _: &SelectLeft, _, cx| v.move_by(0, -1, true, cx)))
                    .on_action(cx.listener(|v, _: &SelectRight, _, cx| v.move_by(0, 1, true, cx)))
                    .on_action(cx.listener(|v, _: &JumpUp, _, cx| v.jump(-1, 0, cx)))
                    .on_action(cx.listener(|v, _: &JumpDown, _, cx| v.jump(1, 0, cx)))
                    .on_action(cx.listener(|v, _: &JumpLeft, _, cx| v.jump(0, -1, cx)))
                    .on_action(cx.listener(|v, _: &JumpRight, _, cx| v.jump(0, 1, cx)))
                    .on_action(cx.listener(|v, _: &PageDown, _, cx| v.move_by(20, 0, false, cx)))
                    .on_action(cx.listener(|v, _: &PageUp, _, cx| v.move_by(-20, 0, false, cx)))
                    .on_action(cx.listener(|v, _: &Enter, _, cx| v.move_by(1, 0, false, cx)))
                    .on_action(cx.listener(|v, _: &ShiftEnter, _, cx| v.move_by(-1, 0, false, cx)))
                    .on_action(cx.listener(|v, _: &Tab, w, cx| {
                        if v.editing {
                            let t = v.cell_input.read(cx).text().to_string();
                            v.commit(t, (0, 1), w, cx);
                        } else {
                            v.move_by(0, 1, false, cx);
                        }
                    }))
                    .on_action(cx.listener(|v, _: &ShiftTab, w, cx| {
                        if v.editing {
                            let t = v.cell_input.read(cx).text().to_string();
                            v.commit(t, (0, -1), w, cx);
                        } else {
                            v.move_by(0, -1, false, cx);
                        }
                    }))
                    .on_action(cx.listener(|v, _: &EditCell, w, cx| v.begin_edit(None, w, cx)))
                    .on_action(cx.listener(|v, _: &Backspace, _, cx| v.clear_selection(cx)))
                    .on_action(cx.listener(|v, _: &DeleteForward, _, cx| v.clear_selection(cx)))
                    .on_action(cx.listener(|v, _: &Copy, _, cx| v.copy(false, cx)))
                    .on_action(cx.listener(|v, _: &Cut, _, cx| v.copy(true, cx)))
                    .on_action(cx.listener(|v, _: &Paste, _, cx| v.paste(cx)))
                    .on_action(cx.listener(|v, _: &SelectAll, _, cx| {
                        let end = v.ctx(cx).and_then(|c| c.doc.pages[c.page].sheet().unwrap().used_range()).map(|r| r.end).unwrap_or(Addr::new(0, 0));
                        v.store.update(cx, |s, cx| {
                            let pv = s.view_mut();
                            pv.anchor = Addr::new(0, 0);
                            pv.cell = end;
                            s.sync_ui();
                            cx.notify();
                        });
                    }))
                    .on_action(cx.listener(|v, _: &Bold, _, cx| {
                        let on = !v.active_format(cx).bold;
                        v.format(json!({ "bold": on }), cx);
                    }))
                    .on_action(cx.listener(|v, _: &Italic, _, cx| {
                        let on = !v.active_format(cx).italic;
                        v.format(json!({ "italic": on }), cx);
                    }))
                    .on_action(cx.listener(|v, _: &Underline, _, cx| {
                        let on = !v.active_format(cx).underline;
                        v.format(json!({ "underline": on }), cx);
                    }))
                    .on_action(cx.listener(|v, _: &FillDown, _, cx| {
                        let r = v.store.read(cx).view().range();
                        v.run("sheet.fill", json!({ "range": r.a1(), "direction": "down" }), cx);
                    }))
                    .on_action(cx.listener(|v, _: &FillRight, _, cx| {
                        let r = v.store.read(cx).view().range();
                        v.run("sheet.fill", json!({ "range": r.a1(), "direction": "right" }), cx);
                    }))
                    .on_action(cx.listener(|v, _: &AutoSum, _, cx| v.autosum(cx)))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_move(cx.listener(Self::mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_scroll_wheel(cx.listener(Self::scroll_wheel))
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                bounds_cell.set(bounds);
                            },
                            move |bounds, _, window, cx| {
                                window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                                paint_grid(window, cx, &doc, &sheet, bounds, scroll, z, cell, range, editing);
                            },
                        )
                        .size_full(),
                    )
                    .when_some(edit_box.filter(|_| editing), |d, (x, y, w, h)| {
                        d.child(
                            div()
                                .absolute()
                                .left(px(x * z - 1.))
                                .top(px(y * z - 1.))
                                .min_w(px(w * z + 2.))
                                .h(px(h * z + 2.))
                                .bg(t.bg_raised)
                                .border_2()
                                .border_color(t.accent)
                                .shadow(t.chip_shadow())
                                .flex()
                                .items_center()
                                .child(self.cell_input.clone()),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// Paints the grid: headers, cells (values formatted, fills, borders), the selection, charts.
#[allow(clippy::too_many_arguments)]
fn paint_grid(window: &mut Window, cx: &mut App, doc: &Document, sh: &Sheet, bounds: Bounds<Pixels>, scroll: (f32, f32), z: f32, active: Addr, range: Range, editing: bool) {
    let t = cx.theme().clone();
    let (w, h) = (f32::from(bounds.size.width) / z, f32::from(bounds.size.height) / z);
    let cols = SheetView::columns(sh, scroll.0, w - HEAD_W);
    let rows = SheetView::rows(sh, scroll.1, h - HEAD_H);
    let o = bounds.origin;
    let at = |x: f32, y: f32| point(o.x + px(x * z), o.y + px(y * z));
    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(at(x, y), size(px(w * z), px(h * z)));
    let font = |bold: bool, italic: bool| gpui::Font {
        family: "IBM Plex Sans".into(),
        features: Default::default(),
        fallbacks: None,
        weight: if bold { FontWeight::SEMIBOLD } else { FontWeight::NORMAL },
        style: if italic { gpui::FontStyle::Italic } else { gpui::FontStyle::Normal },
    };
    let line = t.grid_line;
    // Selection fill under the cells.
    let in_range = |a: Addr| range.contains(a);
    for &(r, ry, rh) in &rows {
        for &(c, cx0, cw) in &cols {
            let a = Addr::new(r, c);
            let (x, y) = (HEAD_W + cx0, HEAD_H + ry);
            let cell = sh.cell(a);
            if let Some(f) = cell.and_then(|c| c.format.fill.as_ref()) {
                window.paint_quad(fill(rect(x, y, cw, rh), crate::paint::hex(f, t.bg_raised)));
            }
            if in_range(a) && (range.rows() > 1 || range.cols() > 1) {
                window.paint_quad(fill(rect(x, y, cw, rh), t.accent_soft));
            }
            if sh.gridlines {
                window.paint_quad(fill(rect(x + cw - 1.0 / z, y, 1.0 / z, rh), line));
                window.paint_quad(fill(rect(x, y + rh - 1.0 / z, cw, 1.0 / z), line));
            }
        }
    }
    // Cell text, which may run over empty neighbours on the right.
    for &(r, ry, rh) in &rows {
        for (ci, &(c, cx0, cw)) in cols.iter().enumerate() {
            let a = Addr::new(r, c);
            let Some(cell) = sh.cell(a) else { continue };
            if (editing && a == active) || cell.value.is_empty() && cell.input.is_empty() {
                continue;
            }
            let text_v = cell.display();
            if text_v.is_empty() {
                continue;
            }
            let f = &cell.format;
            let size_pt = f.size.unwrap_or(10.0);
            let fs = px(size_pt * 4.0 / 3.0 * z);
            let color = f.color.as_deref().map(|c| crate::paint::hex(c, t.text)).unwrap_or(match cell.value {
                CellValue::Error(_) => t.danger,
                _ => t.text,
            });
            let run = TextRun { len: text_v.len(), font: font(f.bold, f.italic), color, background_color: None, underline: f.underline.then(|| gpui::UnderlineStyle { color: Some(color), thickness: px(1.), wavy: false }), strikethrough: f.strike.then(|| gpui::StrikethroughStyle { color: Some(color), thickness: px(1.) }) };
            let shaped = window.text_system().shape_line(SharedString::from(text_v.clone()), fs, &[run], None);
            let tw = f32::from(shaped.width) / z;
            let numeric = matches!(cell.value, CellValue::Number(_)) && !cell.input.starts_with('\'');
            let align = f.align.unwrap_or(if numeric { folio_core::Align::Right } else if matches!(cell.value, CellValue::Bool(_) | CellValue::Error(_)) { folio_core::Align::Center } else { folio_core::Align::Left });
            // Text runs over empty cells to its right (left-aligned text only).
            let mut room = cw;
            if align == folio_core::Align::Left && !numeric {
                for &(c2, _, w2) in cols.iter().skip(ci + 1) {
                    if sh.cell(Addr::new(r, c2)).is_some_and(|x| !x.input.is_empty()) || room >= tw + 8.0 {
                        break;
                    }
                    room += w2;
                }
            }
            let x = HEAD_W + cx0;
            let tx = match align {
                folio_core::Align::Right => x + cw - 5.0 - tw,
                folio_core::Align::Center => x + (cw - tw) / 2.0,
                _ => x + 5.0,
            };
            let ty = HEAD_H + ry + (rh - size_pt * 4.0 / 3.0 * 1.25) / 2.0;
            let clip = rect(x, HEAD_H + ry, room, rh);
            window.with_content_mask(Some(gpui::ContentMask { bounds: clip }), |window| {
                let _ = shaped.paint(at(tx, ty), fs * 1.25, gpui::TextAlign::Left, None, window, cx);
            });
            // Borders.
            if !f.border.is_empty() {
                let ink = t.text;
                let y = HEAD_H + ry;
                for side in f.border.chars() {
                    let b = match side {
                        't' => rect(x, y, cw, 1.0),
                        'b' => rect(x, y + rh - 1.0, cw, 1.0),
                        'l' => rect(x, y, 1.0, rh),
                        'r' => rect(x + cw - 1.0, y, 1.0, rh),
                        _ => continue,
                    };
                    window.paint_quad(fill(b, ink));
                }
            }
        }
    }
    // Headers: column letters and row numbers, the selection's lit.
    window.paint_quad(fill(rect(0.0, 0.0, w, HEAD_H), t.grid_header));
    window.paint_quad(fill(rect(0.0, 0.0, HEAD_W, h), t.grid_header));
    let small = px(11.0 * z);
    let head_font = gpui::Font { family: MONO.into(), features: Default::default(), fallbacks: None, weight: FontWeight::NORMAL, style: gpui::FontStyle::Normal };
    for &(c, cx0, cw) in &cols {
        let lit = c >= range.start.col && c <= range.end.col;
        let x = HEAD_W + cx0;
        if lit {
            window.paint_quad(fill(rect(x, 0.0, cw, HEAD_H), t.accent));
        }
        let label = col_name(c);
        let color = if lit { t.text_on_accent } else { t.text_2 };
        let run = TextRun { len: label.len(), font: head_font.clone(), color, background_color: None, underline: None, strikethrough: None };
        let s = window.text_system().shape_line(SharedString::from(label), small, &[run], None);
        let lw = f32::from(s.width) / z;
        let _ = s.paint(at(x + (cw - lw) / 2.0, 5.0), small * 1.2, gpui::TextAlign::Left, None, window, cx);
        window.paint_quad(fill(rect(x + cw - 1.0 / z, 0.0, 1.0 / z, HEAD_H), t.line));
    }
    for &(r, ry, rh) in &rows {
        let lit = r >= range.start.row && r <= range.end.row;
        let y = HEAD_H + ry;
        if lit {
            window.paint_quad(fill(rect(0.0, y, HEAD_W, rh), t.accent));
        }
        let label = (r + 1).to_string();
        let color = if lit { t.text_on_accent } else { t.text_2 };
        let run = TextRun { len: label.len(), font: head_font.clone(), color, background_color: None, underline: None, strikethrough: None };
        let s = window.text_system().shape_line(SharedString::from(label), small, &[run], None);
        let lw = f32::from(s.width) / z;
        let _ = s.paint(at(HEAD_W - 6.0 - lw, y + (rh - 14.0) / 2.0), small * 1.2, gpui::TextAlign::Left, None, window, cx);
        window.paint_quad(fill(rect(0.0, y + rh - 1.0 / z, HEAD_W, 1.0 / z), t.line));
    }
    window.paint_quad(fill(rect(HEAD_W - 1.0 / z, 0.0, 1.0 / z, h), t.line_strong));
    window.paint_quad(fill(rect(0.0, HEAD_H - 1.0 / z, w, 1.0 / z), t.line_strong));
    // Frozen panes: a stronger line.
    if sh.freeze_rows > 0 {
        let fy: f32 = (0..sh.freeze_rows).map(|r| sh.row_height(r)).sum();
        window.paint_quad(fill(rect(0.0, HEAD_H + fy - 1.0, w, 2.0 / z), t.line_strong));
    }
    if sh.freeze_cols > 0 {
        let fx: f32 = (0..sh.freeze_cols).map(|c| sh.col_width(c)).sum();
        window.paint_quad(fill(rect(HEAD_W + fx - 1.0, 0.0, 2.0 / z, h), t.line_strong));
    }
    // The range's outline and the active cell (2 px ink, the fill handle at the corner).
    let find_col = |c: u32| cols.iter().find(|x| x.0 == c).copied();
    let find_row = |r: u32| rows.iter().find(|x| x.0 == r).copied();
    if let (Some(c0), Some(r0)) = (find_col(range.start.col).or(cols.first().copied()), find_row(range.start.row).or(rows.first().copied())) {
        let c1 = find_col(range.end.col).or(cols.last().copied()).unwrap();
        let r1 = find_row(range.end.row).or(rows.last().copied()).unwrap();
        let (x0, y0) = (HEAD_W + c0.1, HEAD_H + r0.1);
        let (x1, y1) = (HEAD_W + c1.1 + c1.2, HEAD_H + r1.1 + r1.2);
        let ink = t.accent;
        let th = 2.0 / z;
        for b in [rect(x0, y0, x1 - x0, th), rect(x0, y1 - th, x1 - x0, th), rect(x0, y0, th, y1 - y0), rect(x1 - th, y0, th, y1 - y0)] {
            window.paint_quad(fill(b, ink));
        }
        window.paint_quad(fill(rect(x1 - 4.0, y1 - 4.0, 7.0, 7.0), ink));
        window.paint_quad(fill(rect(x1 - 3.0, y1 - 3.0, 5.0, 5.0), t.bg_raised));
        window.paint_quad(fill(rect(x1 - 2.0, y1 - 2.0, 3.0, 3.0), ink));
    }
    if let (Some(c), Some(r)) = (find_col(active.col), find_row(active.row)) {
        let (x, y) = (HEAD_W + c.1, HEAD_H + r.1);
        let th = 2.0 / z;
        for b in [rect(x, y, c.2, th), rect(x, y + r.2 - th, c.2, th), rect(x, y, th, r.2), rect(x + c.2 - th, y, th, r.2)] {
            window.paint_quad(fill(b, t.accent));
        }
    }
    // Charts floating over the grid.
    for ch in &sh.charts {
        let x = HEAD_W + ch.x - scroll.0;
        let y = HEAD_H + ch.y - scroll.1;
        if x > w || y > h || x + ch.w < 0.0 || y + ch.h < 0.0 {
            continue;
        }
        let b = rect(x, y, ch.w, ch.h);
        window.paint_quad(fill(Bounds::new(point(b.origin.x + px(4.), b.origin.y + px(4.)), b.size), t.drop));
        window.paint_quad(fill(b, t.bg_raised));
        window.paint_quad(gpui::outline(b, t.line_strong, gpui::BorderStyle::Solid));
        let data = folio_core::links::chart_data(doc, &ch.chart).unwrap_or_default();
        let dark = t.is_dark();
        let style = folio_layout::chart::ChartStyle {
            text: if dark { [220, 220, 220, 255] } else { [30, 30, 30, 255] },
            grid: if dark { [255, 255, 255, 30] } else { [0, 0, 0, 30] },
            series: if dark { vec![[240, 240, 240, 255], [140, 140, 140, 255], [80, 80, 80, 255], [190, 190, 190, 255]] } else { vec![[12, 12, 12, 255], [130, 130, 130, 255], [200, 200, 200, 255], [70, 70, 70, 255]] },
            size: 9.0,
            background: None,
        };
        let prims = folio_layout::chart_prims(&ch.chart, &data, ch.w * 0.75, ch.h * 0.75, &style);
        crate::paint::prims(window, cx, &prims, b.origin, z * 4.0 / 3.0);
    }
}
