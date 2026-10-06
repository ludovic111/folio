//! The calculation engine: cells of every sheet, their formulas, and the dependency graph.
//!
//! Each formula is parsed once when it is set. The engine keeps an index from cells to the
//! formulas that read them: single cells in a hash map, ranges in buckets (small blocks of
//! cells, whole columns, whole rows, or a short list of large ranges) so a change to `A5`
//! finds `=SUM(A1:A100)` without scanning every formula. `recalc` evaluates the changed
//! formulas and everything downstream of them in dependency order (strongly connected
//! components, so cycles are found and every cell in a cycle gets `#CIRC!`).
//!
//! Volatile functions (NOW, TODAY, RAND, RANDBETWEEN, OFFSET, INDIRECT) are evaluated on every
//! recalculation. Formulas downstream of a volatile formula are evaluated after all others, so
//! the cells OFFSET and INDIRECT reach (which the graph cannot know in advance) are up to date
//! unless they themselves depend on a volatile formula.

use std::fmt;
use std::sync::Arc;

use crate::addr::{Addr, Range};
use crate::eval::{Ctx, EMPTY};
use crate::format::{date_to_serial, time_fraction};
use crate::functions::{self, builtin_functions};
use crate::input::{Input, parse_input};
use crate::parser::{Expr, calls_custom, is_volatile, parse, strip_prefixes, visit_refs};
use crate::util::{FxMap, FxSet};
use crate::value::{ErrorKind, Value};

/// A function added by a plugin. It receives the evaluated arguments.
pub type CustomFn = Arc<dyn Fn(&[Arg]) -> Value + Send + Sync>;

/// An argument passed to a custom function.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    /// A single value (a single cell reference is passed as its value).
    Value(Value),
    /// A range or an array, row by row.
    Range { rows: u32, cols: u32, values: Vec<Value> },
}

/// Documentation of a function, built-in or custom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionInfo {
    pub name: String,
    /// How to call it: `SUM(number1, [number2], …)`.
    pub syntax: String,
    pub summary: String,
    /// Math, Statistical, Logical, Lookup, Text, Date, Financial, Information or Custom.
    pub category: String,
}

#[derive(Clone)]
pub(crate) struct Custom {
    pub info: FunctionInfo,
    pub f: CustomFn,
}

/// A cell of a sheet, identified by sheet index and address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Key {
    pub sheet: u32,
    pub addr: Addr,
}

impl Key {
    fn new(sheet: usize, addr: Addr) -> Self {
        Key { sheet: sheet as u32, addr }
    }
}

#[derive(Clone)]
pub(crate) struct Formula {
    pub expr: Expr,
    /// The references, resolved to sheet indexes (unknown sheets are left out).
    pub deps: Vec<(usize, Range)>,
    pub volatile: bool,
}

#[derive(Clone)]
pub(crate) struct Cell {
    pub input: String,
    pub formula: Option<Box<Formula>>,
    pub value: Value,
}

#[derive(Clone, Default)]
pub(crate) struct Sheet {
    pub name: String,
    pub lower: String,
    pub cells: FxMap<Addr, Cell>,
    /// Largest row and column holding a cell (may be too large after cells are cleared until
    /// the next recalculation).
    bounds: Option<(u32, u32)>,
    bounds_stale: bool,
}

impl Sheet {
    fn new(name: &str) -> Self {
        Sheet { name: name.to_string(), lower: name.to_lowercase(), ..Default::default() }
    }

    /// From A1 to the largest row and column holding a cell.
    pub fn extent(&self) -> Option<Range> {
        self.bounds.map(|(r, c)| Range { start: Addr::new(0, 0), end: Addr::new(r, c) })
    }

    fn grow(&mut self, a: Addr) {
        self.bounds = Some(match self.bounds {
            None => (a.row, a.col),
            Some((r, c)) => (r.max(a.row), c.max(a.col)),
        });
    }

    fn refresh_bounds(&mut self) {
        if self.bounds_stale {
            self.bounds = self.cells.keys().fold(None, |acc, a| {
                Some(match acc {
                    None => (a.row, a.col),
                    Some((r, c)) => (a.row.max(r), a.col.max(c)),
                })
            });
            self.bounds_stale = false;
        }
    }
}

const BLOCK_ROWS: u32 = 64;
const BLOCK_COLS: u32 = 4;
const MAX_BLOCKS: u32 = 32;
const NARROW: u32 = 8;

/// Range dependencies of one sheet, bucketed by shape.
#[derive(Clone, Default)]
struct RangeIndex {
    blocks: FxMap<(u32, u32), Vec<(Range, Key)>>,
    cols: FxMap<u32, Vec<(Range, Key)>>,
    rows: FxMap<u32, Vec<(Range, Key)>>,
    big: Vec<(Range, Key)>,
}

enum Slot {
    Blocks,
    Cols,
    Rows,
    Big,
}

fn slot(r: &Range) -> Slot {
    let br = r.end.row / BLOCK_ROWS - r.start.row / BLOCK_ROWS + 1;
    let bc = r.end.col / BLOCK_COLS - r.start.col / BLOCK_COLS + 1;
    if br * bc <= MAX_BLOCKS {
        Slot::Blocks
    } else if r.cols() <= NARROW {
        Slot::Cols
    } else if r.rows() <= NARROW {
        Slot::Rows
    } else {
        Slot::Big
    }
}

impl RangeIndex {
    fn edit(&mut self, r: Range, key: Key, add: bool) {
        let apply = |list: &mut Vec<(Range, Key)>| {
            if add {
                list.push((r, key));
            } else {
                list.retain(|e| !(e.0 == r && e.1 == key));
            }
        };
        match slot(&r) {
            Slot::Blocks => {
                for br in r.start.row / BLOCK_ROWS..=r.end.row / BLOCK_ROWS {
                    for bc in r.start.col / BLOCK_COLS..=r.end.col / BLOCK_COLS {
                        apply(self.blocks.entry((br, bc)).or_default());
                    }
                }
            }
            Slot::Cols => (r.start.col..=r.end.col).for_each(|c| apply(self.cols.entry(c).or_default())),
            Slot::Rows => (r.start.row..=r.end.row).for_each(|row| apply(self.rows.entry(row).or_default())),
            Slot::Big => apply(&mut self.big),
        }
    }

    fn find(&self, a: Addr, out: &mut Vec<Key>) {
        let mut take = |list: Option<&Vec<(Range, Key)>>| {
            if let Some(list) = list {
                out.extend(list.iter().filter(|(r, _)| r.contains(a)).map(|(_, k)| *k));
            }
        };
        take(self.blocks.get(&(a.row / BLOCK_ROWS, a.col / BLOCK_COLS)));
        take(self.cols.get(&a.col));
        take(self.rows.get(&a.row));
        take(Some(&self.big));
    }
}

/// Which formulas read which cells.
#[derive(Clone, Default)]
struct DepIndex {
    cells: FxMap<Key, Vec<Key>>,
    ranges: Vec<RangeIndex>,
}

impl DepIndex {
    fn edit(&mut self, key: Key, deps: &[(usize, Range)], add: bool) {
        for (sheet, r) in deps {
            if r.start == r.end {
                let target = Key::new(*sheet, r.start);
                if add {
                    self.cells.entry(target).or_default().push(key);
                } else if let Some(list) = self.cells.get_mut(&target) {
                    list.retain(|k| *k != key);
                    if list.is_empty() {
                        self.cells.remove(&target);
                    }
                }
            } else {
                if self.ranges.len() <= *sheet {
                    self.ranges.resize_with(sheet + 1, RangeIndex::default);
                }
                self.ranges[*sheet].edit(*r, key, add);
            }
        }
    }

    /// The formulas that read `key`, without duplicates.
    fn dependents(&self, key: Key) -> Vec<Key> {
        let mut out = Vec::new();
        if let Some(list) = self.cells.get(&key) {
            out.extend_from_slice(list);
        }
        if let Some(index) = self.ranges.get(key.sheet as usize) {
            index.find(key.addr, &mut out);
        }
        if out.len() > 1 {
            out.sort_unstable();
            out.dedup();
        }
        out
    }
}

/// A workbook's formula engine. See the crate documentation for an overview.
#[derive(Clone, Default)]
pub struct Engine {
    pub(crate) sheets: Vec<Sheet>,
    pub(crate) custom: FxMap<String, Custom>,
    deps: DepIndex,
    /// Formula cells to evaluate.
    dirty: FxSet<Key>,
    /// Cells whose content changed: their dependents must be evaluated.
    touched: FxSet<Key>,
    /// Cells whose value changed outside a recalculation (constants typed in, cells cleared).
    changed: FxSet<Key>,
    volatile: FxSet<Key>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Engine");
        for sheet in &self.sheets {
            d.field(&sheet.name, &sheet.cells.len());
        }
        d.finish()
    }
}

/// The current local date and time as a serial.
fn now_serial() -> f64 {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    date_to_serial(now.year(), now.month(), now.day())
        + time_fraction(now.hour(), now.minute(), now.second() as f64 + now.nanosecond() as f64 / 1e9)
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sheets by position (the index is the sheet id used everywhere else).
    ///
    /// A sheet keeps its cells when its name is still in the list (case-insensitive), even at
    /// another position; otherwise the sheet that was at the same position, if its name is
    /// gone, is taken as renamed and keeps its cells. Sheets matched by neither are dropped
    /// with their cells. References between sheets resolve by name, so every formula is
    /// re-linked and recalculated by the next [`Engine::recalc`]; redraw everything after it.
    ///
    /// Sheets that were never named are created as `Sheet1`, `Sheet2`… when a cell is first set.
    pub fn set_sheets(&mut self, names: &[String]) {
        let mut old: Vec<Option<Sheet>> = std::mem::take(&mut self.sheets).into_iter().map(Some).collect();
        let mut new: Vec<Option<Sheet>> = (0..names.len()).map(|_| None).collect();
        for (i, name) in names.iter().enumerate() {
            let lower = name.to_lowercase();
            if let Some(j) = old.iter().position(|s| s.as_ref().is_some_and(|s| s.lower == lower)) {
                new[i] = old[j].take();
            }
        }
        for (i, slot) in new.iter_mut().enumerate() {
            if slot.is_none() {
                let renamed = old.get_mut(i).and_then(Option::take);
                *slot = Some(renamed.unwrap_or_default());
            }
        }
        self.sheets = new
            .into_iter()
            .zip(names)
            .map(|(sheet, name)| {
                let mut sheet = sheet.unwrap_or_default();
                sheet.name = name.clone();
                sheet.lower = name.to_lowercase();
                sheet
            })
            .collect();
        self.relink();
    }

    /// The sheet names, by position.
    pub fn sheet_names(&self) -> Vec<String> {
        self.sheets.iter().map(|s| s.name.clone()).collect()
    }

    /// Rebuilds the dependency graph and marks every formula for recalculation.
    fn relink(&mut self) {
        self.deps = DepIndex::default();
        self.volatile.clear();
        self.touched.clear();
        self.dirty.clear();
        // Sheet indexes may have moved: the caller redraws everything anyway.
        self.changed.clear();
        let mut links = Vec::new();
        for (s, sheet) in self.sheets.iter().enumerate() {
            for (addr, cell) in &sheet.cells {
                if let Some(f) = &cell.formula {
                    links.push((Key::new(s, *addr), self.resolve_deps(s, &f.expr)));
                }
            }
        }
        for (key, deps) in links {
            self.deps.edit(key, &deps, true);
            let cell = self.sheets[key.sheet as usize].cells.get_mut(&key.addr);
            if let Some(f) = cell.and_then(|c| c.formula.as_mut()) {
                if f.volatile {
                    self.volatile.insert(key);
                }
                f.deps = deps;
            }
            self.dirty.insert(key);
        }
    }

    fn ensure_sheet(&mut self, sheet: usize) {
        while self.sheets.len() <= sheet {
            let name = format!("Sheet{}", self.sheets.len() + 1);
            self.sheets.push(Sheet::new(&name));
        }
    }

    pub(crate) fn sheet_index_lower(&self, lower: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.lower == lower)
    }

    pub(crate) fn cell_value(&self, sheet: usize, addr: Addr) -> &Value {
        self.sheets.get(sheet).and_then(|s| s.cells.get(&addr)).map_or(&EMPTY, |c| &c.value)
    }

    /// The largest row and column holding a cell in any sheet.
    pub(crate) fn extent(&self) -> (u32, u32) {
        self.sheets.iter().filter_map(|s| s.bounds).fold((0, 0), |(r, c), (r2, c2)| (r.max(r2), c.max(c2)))
    }

    fn resolve_deps(&self, sheet: usize, expr: &Expr) -> Vec<(usize, Range)> {
        let mut out: Vec<(usize, Range)> = Vec::new();
        visit_refs(expr, &mut |name, range| {
            let s = match name {
                None => Some(sheet),
                Some(lower) => self.sheet_index_lower(lower),
            };
            if let Some(s) = s
                && !out.contains(&(s, *range))
            {
                out.push((s, *range));
            }
        });
        out
    }

    /// Sets what was typed in a cell (`""` clears it). The text is read with
    /// [`parse_input`]; a formula that does not parse shows `#NAME?`. Values of formulas are
    /// computed by the next [`Engine::recalc`].
    pub fn set_cell(&mut self, sheet: usize, addr: Addr, input: &str) {
        self.ensure_sheet(sheet);
        let key = Key::new(sheet, addr);
        let old = self.sheets[sheet].cells.remove(&addr);
        let old_value = match old {
            Some(cell) => {
                if let Some(f) = &cell.formula {
                    self.deps.edit(key, &f.deps, false);
                }
                cell.value
            }
            None => Value::Empty,
        };
        self.volatile.remove(&key);
        self.dirty.remove(&key);
        self.touched.insert(key);
        let cell = match parse_input(input).0 {
            Input::Empty => None,
            Input::Formula(text) => {
                let expr = parse(&text).unwrap_or(Expr::Error(ErrorKind::Name));
                let deps = self.resolve_deps(sheet, &expr);
                let volatile = is_volatile(&expr);
                self.deps.edit(key, &deps, true);
                if volatile {
                    self.volatile.insert(key);
                }
                self.dirty.insert(key);
                Some(Cell {
                    input: input.to_string(),
                    formula: Some(Box::new(Formula { expr, deps, volatile })),
                    value: old_value.clone(),
                })
            }
            Input::Number(n) => Some(Cell { input: input.to_string(), formula: None, value: Value::Number(n) }),
            Input::Text(s) => Some(Cell { input: input.to_string(), formula: None, value: Value::Text(s) }),
            Input::Bool(b) => Some(Cell { input: input.to_string(), formula: None, value: Value::Bool(b) }),
        };
        let new_value = cell.as_ref().map_or(Value::Empty, |c| c.value.clone());
        if new_value != old_value {
            self.changed.insert(key);
        }
        let sheet_data = &mut self.sheets[sheet];
        match cell {
            Some(cell) => {
                sheet_data.cells.insert(addr, cell);
                sheet_data.grow(addr);
            }
            None => {
                if sheet_data.bounds.is_some_and(|(r, c)| addr.row == r || addr.col == c) {
                    sheet_data.bounds_stale = true;
                }
            }
        }
    }

    /// Removes every cell of a sheet.
    pub fn clear_sheet(&mut self, sheet: usize) {
        let Some(data) = self.sheets.get_mut(sheet) else { return };
        let cells = std::mem::take(&mut data.cells);
        data.bounds = None;
        data.bounds_stale = false;
        for (addr, cell) in cells {
            let key = Key::new(sheet, addr);
            if let Some(f) = &cell.formula {
                self.deps.edit(key, &f.deps, false);
            }
            self.volatile.remove(&key);
            self.dirty.remove(&key);
            self.touched.insert(key);
            if !cell.value.is_empty() {
                self.changed.insert(key);
            }
        }
    }

    /// Recomputes changed formulas, everything that depends on changed cells, and volatile
    /// formulas, in dependency order. Every cell of a cycle gets `#CIRC!`. Returns the cells
    /// whose value changed since the last recalculation (typed constants included), sorted.
    pub fn recalc(&mut self) -> Vec<(usize, Addr)> {
        for sheet in &mut self.sheets {
            sheet.refresh_bounds();
        }
        let now = now_serial();

        // Every formula to evaluate: dirty and volatile ones, and everything downstream.
        let mut stack: Vec<Key> = self.dirty.iter().chain(self.volatile.iter()).copied().collect();
        for key in &self.touched {
            stack.extend(self.deps.dependents(*key));
        }
        let mut index: FxMap<Key, usize> = FxMap::default();
        let mut nodes: Vec<Key> = Vec::new();
        let mut edges: Vec<Vec<usize>> = Vec::new();
        let mut pending: Vec<(usize, Vec<Key>)> = Vec::new();
        while let Some(key) = stack.pop() {
            if index.contains_key(&key) || !self.is_formula(key) {
                continue;
            }
            index.insert(key, nodes.len());
            let deps = self.deps.dependents(key);
            stack.extend(deps.iter().copied());
            pending.push((nodes.len(), deps));
            nodes.push(key);
            edges.push(Vec::new());
        }
        for (i, deps) in pending {
            edges[i] = deps.iter().filter_map(|k| index.get(k).copied()).collect();
        }

        // Formulas downstream of volatile ones go last (see the module documentation).
        let mut late = vec![false; nodes.len()];
        let mut stack: Vec<usize> =
            nodes.iter().enumerate().filter(|(_, k)| self.volatile.contains(k)).map(|(i, _)| i).collect();
        while let Some(i) = stack.pop() {
            if !late[i] {
                late[i] = true;
                stack.extend(edges[i].iter().copied());
            }
        }

        let components = strongly_connected(&edges);
        let mut changed: FxSet<Key> = std::mem::take(&mut self.changed);
        for pass_late in [false, true] {
            for comp in components.iter().rev() {
                if late[comp[0]] != pass_late {
                    continue;
                }
                let cyclic = comp.len() > 1 || edges[comp[0]].contains(&comp[0]);
                for &i in comp {
                    let key = nodes[i];
                    let value = if cyclic { Value::Error(ErrorKind::Circular) } else { self.evaluate_cell(key, now) };
                    if let Some(cell) = self.sheets[key.sheet as usize].cells.get_mut(&key.addr)
                        && cell.value != value
                    {
                        cell.value = value;
                        changed.insert(key);
                    }
                }
            }
        }
        self.dirty.clear();
        self.touched.clear();
        let mut out: Vec<(usize, Addr)> = changed.into_iter().map(|k| (k.sheet as usize, k.addr)).collect();
        out.sort_unstable();
        out
    }

    /// Recomputes every formula (for example after loading a file).
    pub fn recalc_all(&mut self) -> Vec<(usize, Addr)> {
        for (s, sheet) in self.sheets.iter().enumerate() {
            for (addr, cell) in &sheet.cells {
                if cell.formula.is_some() {
                    self.dirty.insert(Key::new(s, *addr));
                }
            }
        }
        self.recalc()
    }

    fn is_formula(&self, key: Key) -> bool {
        self.sheets.get(key.sheet as usize).and_then(|s| s.cells.get(&key.addr)).is_some_and(|c| c.formula.is_some())
    }

    fn evaluate_cell(&self, key: Key, now: f64) -> Value {
        let sheet = key.sheet as usize;
        let Some(f) = self.sheets[sheet].cells.get(&key.addr).and_then(|c| c.formula.as_ref()) else {
            return Value::Empty;
        };
        let ctx = Ctx::new(self, sheet, key.addr, now);
        let ev = ctx.eval(&f.expr);
        ctx.finish(&ev)
    }

    /// The value of a cell (`Empty` when there is nothing).
    pub fn value(&self, sheet: usize, addr: Addr) -> Value {
        self.cell_value(sheet, addr).clone()
    }

    /// What was typed in a cell, as given to [`Engine::set_cell`].
    pub fn input(&self, sheet: usize, addr: Addr) -> Option<&str> {
        self.sheets.get(sheet)?.cells.get(&addr).map(|c| c.input.as_str())
    }

    /// The values of a range, row by row. Whole columns and rows stop at the last used row or
    /// column (they still start at row 1 / column A); an empty sheet gives no rows for them.
    pub fn range_values(&self, sheet: usize, range: Range) -> Vec<Vec<Value>> {
        let mut r = range;
        if range.is_whole_cols() || range.is_whole_rows() {
            let Some(used) = self.used_range(sheet) else { return Vec::new() };
            if range.is_whole_cols() {
                r.end.row = used.end.row;
            }
            if range.is_whole_rows() {
                r.end.col = used.end.col;
            }
            if r.end.row < r.start.row || r.end.col < r.start.col {
                return Vec::new();
            }
        }
        (r.start.row..=r.end.row)
            .map(|row| (r.start.col..=r.end.col).map(|col| self.value(sheet, Addr::new(row, col))).collect())
            .collect()
    }

    /// The smallest range holding every cell of a sheet, or `None` for an empty sheet.
    pub fn used_range(&self, sheet: usize) -> Option<Range> {
        let cells = &self.sheets.get(sheet)?.cells;
        let mut it = cells.keys();
        let first = *it.next()?;
        let (mut r0, mut c0, mut r1, mut c1) = (first.row, first.col, first.row, first.col);
        for a in it {
            r0 = r0.min(a.row);
            c0 = c0.min(a.col);
            r1 = r1.max(a.row);
            c1 = c1.max(a.col);
        }
        Some(Range { start: Addr::new(r0, c0), end: Addr::new(r1, c1) })
    }

    /// Evaluates a formula (without `=`) as if typed in `at`, without storing it: formula bar
    /// previews, or an agent asking a question. A formula that does not parse gives `#NAME?`.
    pub fn evaluate(&self, sheet: usize, at: Addr, formula: &str) -> Value {
        let formula = formula.strip_prefix('=').unwrap_or(formula);
        match parse(formula) {
            Ok(expr) => {
                let ctx = Ctx::new(self, sheet, at, now_serial());
                let ev = ctx.eval(&expr);
                ctx.finish(&ev)
            }
            Err(_) => Value::Error(ErrorKind::Name),
        }
    }

    /// Adds a custom function (from a plugin). Names are case-insensitive. Built-in functions
    /// win: registering a built-in name does nothing and returns `false`. Registering a name
    /// again replaces the earlier function. Formulas calling it are recalculated by the next
    /// [`Engine::recalc`].
    pub fn register_function(&mut self, info: FunctionInfo, f: CustomFn) -> bool {
        let name = strip_prefixes(info.name.trim()).to_ascii_uppercase();
        if name.is_empty() || functions::lookup(&name).is_some() {
            return false;
        }
        let info = FunctionInfo { name: name.clone(), ..info };
        self.custom.insert(name.clone(), Custom { info, f });
        self.mark_callers(&name);
        true
    }

    /// Removes a custom function; formulas calling it show `#NAME?` after the next recalculation.
    pub fn unregister_function(&mut self, name: &str) {
        let name = strip_prefixes(name.trim()).to_ascii_uppercase();
        if self.custom.remove(&name).is_some() {
            self.mark_callers(&name);
        }
    }

    fn mark_callers(&mut self, name: &str) {
        for (s, sheet) in self.sheets.iter().enumerate() {
            for (addr, cell) in &sheet.cells {
                if cell.formula.as_ref().is_some_and(|f| calls_custom(&f.expr, name)) {
                    self.dirty.insert(Key::new(s, *addr));
                }
            }
        }
    }

    /// Every function, built-in and custom, sorted by name.
    pub fn functions(&self) -> Vec<FunctionInfo> {
        let mut all: Vec<FunctionInfo> = builtin_functions()
            .iter()
            .map(|b| FunctionInfo {
                name: b.name.to_string(),
                syntax: b.syntax.to_string(),
                summary: b.summary.to_string(),
                category: b.category.to_string(),
            })
            .chain(self.custom.values().map(|c| c.info.clone()))
            .collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    /// The ranges a cell's formula reads, with their sheet indexes (empty for constants).
    pub fn precedents(&self, sheet: usize, addr: Addr) -> Vec<(usize, Range)> {
        self.sheets
            .get(sheet)
            .and_then(|s| s.cells.get(&addr))
            .and_then(|c| c.formula.as_ref())
            .map(|f| f.deps.clone())
            .unwrap_or_default()
    }

    /// The formula cells that read a cell directly (for tracing dependents).
    pub fn dependents(&self, sheet: usize, addr: Addr) -> Vec<(usize, Addr)> {
        self.deps.dependents(Key::new(sheet, addr)).into_iter().map(|k| (k.sheet as usize, k.addr)).collect()
    }
}

/// Tarjan's strongly connected components, without recursion. Components come out in reverse
/// topological order (a component after every component it points to).
fn strongly_connected(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = edges.len();
    const UNSEEN: usize = usize::MAX;
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    let mut next = 0;
    let mut call: Vec<(usize, usize)> = Vec::new();
    for root in 0..n {
        if index[root] != UNSEEN {
            continue;
        }
        call.push((root, 0));
        while let Some(&mut (v, ref mut child)) = call.last_mut() {
            if *child == 0 && index[v] == UNSEEN {
                index[v] = next;
                low[v] = next;
                next += 1;
                stack.push(v);
                on_stack[v] = true;
            }
            if let Some(&w) = edges[v].get(*child) {
                *child += 1;
                if index[w] == UNSEEN {
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            call.pop();
            if let Some(&(parent, _)) = call.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                let mut comp = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    comp.push(w);
                    if w == v {
                        break;
                    }
                }
                out.push(comp);
            }
        }
    }
    out
}
