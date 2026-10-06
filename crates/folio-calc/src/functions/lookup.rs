//! Lookup and reference functions.
//!
//! Approximate matches (VLOOKUP's default, MATCH types 1 and -1, LOOKUP) assume sorted data as
//! in Excel: the scan stops at the first value past the one looked up. XLOOKUP's next-smaller
//! and next-larger modes work on unsorted data. Only values of the same type match (numbers
//! with numbers, text with text without case); exact matches of text accept `*`, `?` and `~`
//! wildcards in VLOOKUP, HLOOKUP, MATCH and XMATCH/XLOOKUP match mode 2.

use std::cmp::Ordering;

use super::criteria::wildcard_eq;
use super::{Builtin, Imp, def, num_arg, scalar_arg};
use crate::addr::{Addr, MAX_COLS, MAX_ROWS, Range, SheetRange};
use crate::eval::{Array, Ctx, Ev, Grid, R, num_cmp, text_cmp, to_bool, to_text};
use crate::value::{ErrorKind, Value};

const L: &str = "Lookup";

pub(crate) const FUNCTIONS: &[Builtin] = &[
    def(
        "VLOOKUP",
        L,
        "VLOOKUP(lookup_value, table, col_index, [approximate])",
        "Finds a value in the first column of a table and returns the value in another column of that row.",
        3,
        4,
        Imp::Eager(vlookup),
    ),
    def(
        "HLOOKUP",
        L,
        "HLOOKUP(lookup_value, table, row_index, [approximate])",
        "Finds a value in the first row of a table and returns the value in another row of that column.",
        3,
        4,
        Imp::Eager(hlookup),
    ),
    def(
        "XLOOKUP",
        L,
        "XLOOKUP(lookup_value, lookup_array, return_array, [if_not_found], [match_mode], [search_mode])",
        "Finds a value in one range and returns the matching item of another.",
        3,
        6,
        Imp::Eager(xlookup),
    ),
    def(
        "INDEX",
        L,
        "INDEX(array, row, [column])",
        "The value (or reference) at a row and column of a range.",
        2,
        3,
        Imp::Eager(index),
    ),
    def(
        "MATCH",
        L,
        "MATCH(lookup_value, lookup_array, [match_type])",
        "The position of a value in a row or column (1: sorted, largest ≤; 0: exact; -1: sorted descending, smallest ≥).",
        2,
        3,
        Imp::Eager(match_),
    ),
    def(
        "XMATCH",
        L,
        "XMATCH(lookup_value, lookup_array, [match_mode], [search_mode])",
        "The position of a value in a row or column.",
        2,
        4,
        Imp::Eager(xmatch),
    ),
    def(
        "LOOKUP",
        L,
        "LOOKUP(lookup_value, lookup_vector, [result_vector])",
        "Finds a value in a sorted row or column and returns the matching item of another.",
        2,
        3,
        Imp::Eager(lookup),
    ),
    def(
        "ROW",
        L,
        "ROW([reference])",
        "The row number of a reference (or of the formula's cell).",
        0,
        1,
        Imp::Eager(row),
    ),
    def("ROWS", L, "ROWS(array)", "The number of rows of a range or array.", 1, 1, Imp::Eager(rows)),
    def(
        "COLUMN",
        L,
        "COLUMN([reference])",
        "The column number of a reference (or of the formula's cell).",
        0,
        1,
        Imp::Eager(column),
    ),
    def("COLUMNS", L, "COLUMNS(array)", "The number of columns of a range or array.", 1, 1, Imp::Eager(columns)),
    def(
        "OFFSET",
        L,
        "OFFSET(reference, rows, cols, [height], [width])",
        "A reference moved from another by rows and columns.",
        3,
        5,
        Imp::Eager(offset),
    ),
    def(
        "INDIRECT",
        L,
        "INDIRECT(ref_text, [a1])",
        "The reference written in a text, such as \"Sheet2!B3\".",
        1,
        2,
        Imp::Eager(indirect),
    ),
];

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Exact {
        wildcard: bool,
    },
    /// Sorted ascending: the last value ≤ the key.
    SortedLe,
    /// Sorted descending: the last value ≥ the key.
    SortedGe,
    /// Exact, or else the largest value smaller than the key.
    NextSmaller,
    /// Exact, or else the smallest value larger than the key.
    NextLarger,
}

/// Compares values of the same type only.
fn same_type_cmp(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Some(num_cmp(*x, *y)),
        (Value::Text(x), Value::Text(y)) => Some(text_cmp(x, y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

fn is_equal(v: &Value, key: &Value, wildcard: bool) -> bool {
    match (key, v) {
        (Value::Text(k), Value::Text(s)) if wildcard => wildcard_eq(k, s),
        _ => same_type_cmp(v, key) == Some(Ordering::Equal),
    }
}

/// Finds `key` among `len` values given by `get`.
fn find(len: usize, get: &dyn Fn(usize) -> Value, key: &Value, mode: Mode, reverse: bool) -> Option<usize> {
    let order: Box<dyn Iterator<Item = usize>> = if reverse { Box::new((0..len).rev()) } else { Box::new(0..len) };
    match mode {
        Mode::Exact { wildcard } => order.into_iter().find(|&i| is_equal(&get(i), key, wildcard)),
        Mode::SortedLe | Mode::SortedGe => {
            let mut best = None;
            for i in 0..len {
                match same_type_cmp(&get(i), key) {
                    Some(Ordering::Equal) => best = Some(i),
                    Some(Ordering::Less) if mode == Mode::SortedLe => best = Some(i),
                    Some(Ordering::Greater) if mode == Mode::SortedGe => best = Some(i),
                    Some(_) => break,
                    None => {}
                }
            }
            best
        }
        Mode::NextSmaller | Mode::NextLarger => {
            let mut best: Option<(usize, Value)> = None;
            for i in order {
                let v = get(i);
                match same_type_cmp(&v, key) {
                    Some(Ordering::Equal) => return Some(i),
                    Some(ord) => {
                        let wanted = if mode == Mode::NextSmaller { Ordering::Less } else { Ordering::Greater };
                        if ord != wanted {
                            continue;
                        }
                        let better = match &best {
                            None => true,
                            Some((_, b)) => same_type_cmp(&v, b) == Some(wanted.reverse()),
                        };
                        if better {
                            best = Some((i, v));
                        }
                    }
                    None => {}
                }
            }
            best.map(|(i, _)| i)
        }
    }
}

fn key_arg(ctx: &Ctx, args: &[Ev]) -> R<Value> {
    match ctx.scalar(&args[0]) {
        Value::Error(e) => Err(e),
        v => Ok(v),
    }
}

fn table_lookup(ctx: &Ctx, args: Vec<Ev>, vertical: bool) -> R<Ev> {
    let key = key_arg(ctx, &args)?;
    let index = num_arg(ctx, &args, 2, 1.0)?.trunc();
    let approximate = match scalar_arg(ctx, &args, 3) {
        Some(v) => to_bool(&v)?,
        None => true,
    };
    if index < 1.0 {
        return Err(ErrorKind::Value);
    }
    let (rows, cols) = args[1].dims();
    let index = index as usize - 1;
    if index >= if vertical { cols } else { rows } {
        return Err(ErrorKind::Ref);
    }
    let grid = ctx.grid(&args[1]);
    let mode = if approximate { Mode::SortedLe } else { Mode::Exact { wildcard: true } };
    let found = if vertical {
        find(grid.rows(), &|i| grid.get(i, 0).clone(), &key, mode, false)
    } else {
        find(grid.cols(), &|i| grid.get(0, i).clone(), &key, mode, false)
    };
    let i = found.ok_or(ErrorKind::NA)?;
    Ok(Ev::V(if vertical { grid.get(i, index).clone() } else { grid.get(index, i).clone() }))
}

fn vlookup(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    table_lookup(ctx, args, true)
}

fn hlookup(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    table_lookup(ctx, args, false)
}

fn vector<'a>(grid: &'a Grid<'a>) -> R<(usize, impl Fn(usize) -> Value + 'a)> {
    let len = grid.vector_len().ok_or(ErrorKind::NA)?;
    Ok((len, move |i| grid.at(i).clone()))
}

fn match_(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let key = key_arg(ctx, &args)?;
    let kind = num_arg(ctx, &args, 2, 1.0)?;
    let mode = if kind > 0.0 {
        Mode::SortedLe
    } else if kind < 0.0 {
        Mode::SortedGe
    } else {
        Mode::Exact { wildcard: true }
    };
    let grid = ctx.grid(&args[1]);
    let (len, get) = vector(&grid)?;
    let i = find(len, &get, &key, mode, false).ok_or(ErrorKind::NA)?;
    Ok(Ev::num(i as f64 + 1.0))
}

fn x_modes(ctx: &Ctx, args: &[Ev], match_at: usize) -> R<(Mode, bool)> {
    let match_mode = match scalar_arg(ctx, args, match_at) {
        Some(Value::Empty) | None => 0.0,
        Some(v) => crate::eval::to_num(&v)?,
    };
    let search_mode = match scalar_arg(ctx, args, match_at + 1) {
        Some(Value::Empty) | None => 1.0,
        Some(v) => crate::eval::to_num(&v)?,
    };
    let mode = match match_mode as i64 {
        0 => Mode::Exact { wildcard: false },
        -1 => Mode::NextSmaller,
        1 => Mode::NextLarger,
        2 => Mode::Exact { wildcard: true },
        _ => return Err(ErrorKind::Value),
    };
    let reverse = match search_mode as i64 {
        1 | 2 => false,
        -1 | -2 => true,
        _ => return Err(ErrorKind::Value),
    };
    Ok((mode, reverse))
}

fn xmatch(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let key = key_arg(ctx, &args)?;
    let (mode, reverse) = x_modes(ctx, &args, 2)?;
    let grid = ctx.grid(&args[1]);
    let (len, get) = vector(&grid)?;
    let i = find(len, &get, &key, mode, reverse).ok_or(ErrorKind::NA)?;
    Ok(Ev::num(i as f64 + 1.0))
}

/// Row `i` (or column `i` when `by_col`) of a result.
fn slice(ctx: &Ctx, ev: &Ev, i: usize, by_col: bool) -> R<Ev> {
    match ev {
        Ev::R { sheet, range } => {
            let r = if by_col {
                let col = range.start.col + i as u32;
                Range { start: Addr::new(range.start.row, col), end: Addr::new(range.end.row, col) }
            } else {
                let row = range.start.row + i as u32;
                Range { start: Addr::new(row, range.start.col), end: Addr::new(row, range.end.col) }
            };
            Ok(Ev::R { sheet: *sheet, range: r })
        }
        Ev::A(a) => {
            if by_col {
                let data = (0..a.rows).map(|r| a.get(r, i).clone()).collect();
                Ok(Ev::A(Array::new(a.rows, 1, data)))
            } else {
                let data = (0..a.cols).map(|c| a.get(i, c).clone()).collect();
                Ok(Ev::A(Array::new(1, a.cols, data)))
            }
        }
        Ev::V(v) => {
            if i == 0 {
                Ok(Ev::V(v.clone()))
            } else {
                let _ = ctx;
                Err(ErrorKind::NA)
            }
        }
    }
}

fn xlookup(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let key = key_arg(ctx, &args)?;
    let (mode, reverse) = x_modes(ctx, &args, 4)?;
    let (lrows, lcols) = args[1].dims();
    let vertical = lcols == 1;
    if !vertical && lrows != 1 {
        return Err(ErrorKind::Value);
    }
    let (rrows, rcols) = args[2].dims();
    if (vertical && rrows != lrows) || (!vertical && rcols != lcols) {
        return Err(ErrorKind::Value);
    }
    let grid = ctx.grid(&args[1]);
    let (len, get) = vector(&grid)?;
    match find(len, &get, &key, mode, reverse) {
        Some(i) => slice(ctx, &args[2], i, !vertical),
        None => match args.get(3) {
            Some(Ev::V(Value::Empty)) | None => Err(ErrorKind::NA),
            Some(other) => Ok(other.clone()),
        },
    }
}

fn lookup(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let key = key_arg(ctx, &args)?;
    let grid = ctx.grid(&args[1]);
    if let Some(result) = args.get(2) {
        let (len, get) = vector(&grid)?;
        let i = find(len, &get, &key, Mode::SortedLe, false).ok_or(ErrorKind::NA)?;
        let out = ctx.grid(result);
        return Ok(Ev::V(out.at(i).clone()));
    }
    let (rows, cols) = (grid.rows(), grid.cols());
    if cols > rows {
        let i = find(cols, &|i| grid.get(0, i).clone(), &key, Mode::SortedLe, false).ok_or(ErrorKind::NA)?;
        Ok(Ev::V(grid.get(rows - 1, i).clone()))
    } else {
        let i = find(rows, &|i| grid.get(i, 0).clone(), &key, Mode::SortedLe, false).ok_or(ErrorKind::NA)?;
        Ok(Ev::V(grid.get(i, cols - 1).clone()))
    }
}

fn index(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    if let Ev::V(Value::Error(e)) = &args[0] {
        return Err(*e);
    }
    let (rows, cols) = args[0].dims();
    let mut r = num_arg(ctx, &args, 1, 0.0)?.trunc();
    let mut c = num_arg(ctx, &args, 2, 0.0)?.trunc();
    if args.len() == 2 && rows == 1 && cols > 1 {
        (r, c) = (1.0, r);
    }
    if r < 0.0 || c < 0.0 {
        return Err(ErrorKind::Value);
    }
    let (r, c) = (r as usize, c as usize);
    if r > rows || c > cols {
        return Err(ErrorKind::Ref);
    }
    let rows_span = if r == 0 { (0, rows - 1) } else { (r - 1, r - 1) };
    let cols_span = if c == 0 { (0, cols - 1) } else { (c - 1, c - 1) };
    match &args[0] {
        Ev::R { sheet, range } => {
            let start = Addr::new(range.start.row + rows_span.0 as u32, range.start.col + cols_span.0 as u32);
            let end = Addr::new(range.start.row + rows_span.1 as u32, range.start.col + cols_span.1 as u32);
            Ok(Ev::R { sheet: *sheet, range: Range { start, end } })
        }
        Ev::A(a) => {
            let mut data = Vec::new();
            for rr in rows_span.0..=rows_span.1 {
                for cc in cols_span.0..=cols_span.1 {
                    data.push(a.get(rr, cc).clone());
                }
            }
            let shape = (rows_span.1 - rows_span.0 + 1, cols_span.1 - cols_span.0 + 1);
            if data.len() == 1 {
                return Ok(Ev::V(data.pop().unwrap_or_default()));
            }
            Ok(Ev::A(Array::new(shape.0, shape.1, data)))
        }
        Ev::V(v) => Ok(Ev::V(v.clone())),
    }
}

fn position(ctx: &Ctx, args: &[Ev], rows: bool) -> R<Ev> {
    match args.first() {
        None => Ok(Ev::num(if rows { ctx.at.row } else { ctx.at.col } as f64 + 1.0)),
        Some(Ev::R { range, .. }) => {
            let (first, n) = if rows { (range.start.row, range.rows()) } else { (range.start.col, range.cols()) };
            if n == 1 {
                return Ok(Ev::num(first as f64 + 1.0));
            }
            let data: Vec<Value> = (0..n.min(MAX_ROWS)).map(|i| Value::Number((first + i) as f64 + 1.0)).collect();
            let len = data.len();
            Ok(Ev::A(if rows { Array::new(len, 1, data) } else { Array::new(1, len, data) }))
        }
        Some(Ev::V(Value::Error(e))) => Err(*e),
        Some(_) => Err(ErrorKind::Value),
    }
}

fn row(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    position(ctx, &args, true)
}

fn column(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    position(ctx, &args, false)
}

fn rows(_: &Ctx, args: Vec<Ev>) -> R<Ev> {
    if let Ev::V(Value::Error(e)) = &args[0] {
        return Err(*e);
    }
    Ok(Ev::num(args[0].dims().0 as f64))
}

fn columns(_: &Ctx, args: Vec<Ev>) -> R<Ev> {
    if let Ev::V(Value::Error(e)) = &args[0] {
        return Err(*e);
    }
    Ok(Ev::num(args[0].dims().1 as f64))
}

fn offset(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let (sheet, base) = match &args[0] {
        Ev::R { sheet, range } => (*sheet, *range),
        Ev::V(Value::Error(e)) => return Err(*e),
        _ => return Err(ErrorKind::Value),
    };
    let dr = num_arg(ctx, &args, 1, 0.0)?.trunc() as i64;
    let dc = num_arg(ctx, &args, 2, 0.0)?.trunc() as i64;
    let size = |i: usize, default: u32| -> R<i64> {
        match scalar_arg(ctx, &args, i) {
            Some(Value::Empty) | None => Ok(default as i64),
            Some(v) => Ok(crate::eval::to_num(&v)?.trunc() as i64),
        }
    };
    let h = size(3, base.rows())?;
    let w = size(4, base.cols())?;
    if h < 1 || w < 1 {
        return Err(ErrorKind::Ref);
    }
    let r0 = base.start.row as i64 + dr;
    let c0 = base.start.col as i64 + dc;
    let (r1, c1) = (r0 + h - 1, c0 + w - 1);
    if r0 < 0 || c0 < 0 || r1 >= MAX_ROWS as i64 || c1 >= MAX_COLS as i64 {
        return Err(ErrorKind::Ref);
    }
    let range = Range { start: Addr::new(r0 as u32, c0 as u32), end: Addr::new(r1 as u32, c1 as u32) };
    Ok(Ev::R { sheet, range })
}

fn indirect(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let text = to_text(&ctx.scalar(&args[0]))?;
    if let Some(v) = scalar_arg(ctx, &args, 1)
        && !v.is_empty()
        && !to_bool(&v)?
    {
        // R1C1 references are not supported.
        return Err(ErrorKind::Ref);
    }
    let parsed = SheetRange::parse(&text).ok_or(ErrorKind::Ref)?;
    let sheet = match &parsed.sheet {
        None => ctx.sheet,
        Some(name) => ctx.engine.sheet_index_lower(&name.to_lowercase()).ok_or(ErrorKind::Ref)?,
    };
    Ok(Ev::R { sheet, range: parsed.range })
}
