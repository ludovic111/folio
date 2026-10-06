//! Evaluates expression trees: references, arrays, operators and coercions.
//!
//! During evaluation a result is an [`Ev`]: a single value, an array, or a reference that is
//! read lazily (so functions such as ROW, ISBLANK, INDEX and OFFSET can see the reference
//! itself, and SUM can skip text in ranges). Operators and most functions work element by
//! element on arrays, so `SUMPRODUCT(A1:A3*B1:B3)` and `SUM(IF(A1:A3>1, A1:A3))` work. A cell
//! keeps a single value: the top-left element of an array, or for a range the cell in the same
//! row or column as the formula (implicit intersection). Dynamic arrays do not spill yet.

use std::cmp::Ordering;

use crate::addr::{Addr, MAX_COLS, MAX_ROWS, Range};
use crate::engine::{Arg, Engine};
use crate::format::number_to_text;
use crate::functions::Imp;
use crate::input::text_to_number;
use crate::parser::{BinOp, Expr, Func};
use crate::value::{ErrorKind, Value};

pub(crate) type R<T> = Result<T, ErrorKind>;

pub(crate) static EMPTY: Value = Value::Empty;

/// A rectangle of values, row by row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Array {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<Value>,
}

impl Array {
    pub fn new(rows: usize, cols: usize, data: Vec<Value>) -> Self {
        debug_assert_eq!(rows * cols, data.len());
        Array { rows, cols, data }
    }

    pub fn get(&self, r: usize, c: usize) -> &Value {
        if r < self.rows && c < self.cols { &self.data[r * self.cols + c] } else { &EMPTY }
    }

    /// The element for position (r, c) when broadcasting this array to a larger shape:
    /// a single row or column repeats, anything else outside the array is `#N/A`.
    fn broadcast(&self, r: usize, c: usize) -> Value {
        let r = if self.rows == 1 { 0 } else { r };
        let c = if self.cols == 1 { 0 } else { c };
        if r < self.rows && c < self.cols { self.data[r * self.cols + c].clone() } else { Value::Error(ErrorKind::NA) }
    }
}

/// An intermediate result.
#[derive(Clone, Debug)]
pub(crate) enum Ev {
    V(Value),
    A(Array),
    R { sheet: usize, range: Range },
}

impl Ev {
    pub fn err(e: ErrorKind) -> Ev {
        Ev::V(Value::Error(e))
    }

    pub fn num(n: f64) -> Ev {
        Ev::V(num_value(n))
    }

    /// Rows and columns (a reference's full size, not clipped).
    pub fn dims(&self) -> (usize, usize) {
        match self {
            Ev::V(_) => (1, 1),
            Ev::A(a) => (a.rows, a.cols),
            Ev::R { range, .. } => (range.rows() as usize, range.cols() as usize),
        }
    }

    /// True for arrays and references to more than one cell.
    pub fn is_multi(&self) -> bool {
        match self {
            Ev::V(_) => false,
            Ev::A(a) => a.rows * a.cols != 1,
            Ev::R { range, .. } => range.start != range.end,
        }
    }
}

/// A number result: infinities and NaN become `#NUM!`.
pub(crate) fn num_value(n: f64) -> Value {
    if n.is_finite() { Value::Number(if n == 0.0 { 0.0 } else { n }) } else { Value::Error(ErrorKind::Num) }
}

/// A value as a number: empty is 0, TRUE is 1, text is read like typed input, or `#VALUE!`.
pub(crate) fn to_num(v: &Value) -> R<f64> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Empty => Ok(0.0),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Text(s) => text_to_number(s).ok_or(ErrorKind::Value),
        Value::Error(e) => Err(*e),
    }
}

/// A value as text: numbers with up to 15 significant digits, `TRUE`/`FALSE`, empty is "".
pub(crate) fn to_text(v: &Value) -> R<String> {
    match v {
        Value::Number(n) => Ok(number_to_text(*n)),
        Value::Empty => Ok(String::new()),
        Value::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.to_string()),
        Value::Text(s) => Ok(s.clone()),
        Value::Error(e) => Err(*e),
    }
}

/// A value as a boolean: numbers are true unless 0, text must read TRUE or FALSE.
pub(crate) fn to_bool(v: &Value) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(*n != 0.0),
        Value::Empty => Ok(false),
        Value::Text(s) if s.eq_ignore_ascii_case("TRUE") => Ok(true),
        Value::Text(s) if s.eq_ignore_ascii_case("FALSE") => Ok(false),
        Value::Text(_) => Err(ErrorKind::Value),
        Value::Error(e) => Err(*e),
    }
}

/// Numbers compare equal when they agree to about 15 significant digits (so 0.1+0.2 = 0.3).
pub(crate) fn num_cmp(a: f64, b: f64) -> Ordering {
    if a == b || (a - b).abs() <= 1e-15 * a.abs().max(b.abs()) {
        Ordering::Equal
    } else if a < b {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// Text compares without regard to case.
pub(crate) fn text_cmp(a: &str, b: &str) -> Ordering {
    if a.eq_ignore_ascii_case(b) {
        return Ordering::Equal;
    }
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// Excel's ordering for comparisons: numbers < text < booleans; empty acts as 0, "" or FALSE.
pub(crate) fn compare(a: &Value, b: &Value) -> R<Ordering> {
    if let Value::Error(e) = a {
        return Err(*e);
    }
    if let Value::Error(e) = b {
        return Err(*e);
    }
    let fill = |other: &Value| match other {
        Value::Text(_) => Value::Text(String::new()),
        Value::Bool(_) => Value::Bool(false),
        _ => Value::Number(0.0),
    };
    let a2;
    let b2;
    let (a, b) = match (a, b) {
        (Value::Empty, Value::Empty) => return Ok(Ordering::Equal),
        (Value::Empty, other) => {
            a2 = fill(other);
            (&a2, b)
        }
        (other, Value::Empty) => {
            b2 = fill(other);
            (a, &b2)
        }
        _ => (a, b),
    };
    let rank = |v: &Value| match v {
        Value::Number(_) => 0,
        Value::Text(_) => 1,
        _ => 2,
    };
    Ok(match (a, b) {
        (Value::Number(x), Value::Number(y)) => num_cmp(*x, *y),
        (Value::Text(x), Value::Text(y)) => text_cmp(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    })
}

/// Applies a binary operator to two single values.
pub(crate) fn apply_op(op: BinOp, a: &Value, b: &Value) -> Value {
    let result = (|| -> R<Value> {
        Ok(match op {
            BinOp::Concat => {
                let x = to_text(a)?;
                let y = to_text(b)?;
                Value::Text(x + &y)
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                let ord = compare(a, b)?;
                Value::Bool(match op {
                    BinOp::Eq => ord == Ordering::Equal,
                    BinOp::Ne => ord != Ordering::Equal,
                    BinOp::Lt => ord == Ordering::Less,
                    BinOp::Gt => ord == Ordering::Greater,
                    BinOp::Le => ord != Ordering::Greater,
                    _ => ord != Ordering::Less,
                })
            }
            _ => {
                let x = to_num(a)?;
                let y = to_num(b)?;
                match op {
                    BinOp::Add => num_value(x + y),
                    BinOp::Sub => num_value(x - y),
                    BinOp::Mul => num_value(x * y),
                    BinOp::Div => {
                        if y == 0.0 {
                            return Err(ErrorKind::Div0);
                        }
                        num_value(x / y)
                    }
                    _ => power(x, y)?,
                }
            }
        })
    })();
    result.unwrap_or_else(Value::Error)
}

pub(crate) fn power(x: f64, y: f64) -> R<Value> {
    if x == 0.0 && y == 0.0 {
        return Err(ErrorKind::Num);
    }
    if x == 0.0 && y < 0.0 {
        return Err(ErrorKind::Div0);
    }
    Ok(num_value(x.powf(y)))
}

/// What evaluation needs to know: the engine (cells, sheets, custom functions), where the
/// formula sits, and the time of this calculation.
pub(crate) struct Ctx<'a> {
    pub engine: &'a Engine,
    pub sheet: usize,
    pub at: Addr,
    pub now: f64,
}

/// A read-only grid view of a value, an array or a reference, used by lookups and criteria.
pub(crate) enum Grid<'a> {
    Ref { engine: &'a Engine, sheet: usize, start: Addr, rows: usize, cols: usize },
    Arr(&'a Array),
    One(&'a Value),
}

impl Grid<'_> {
    pub fn rows(&self) -> usize {
        match self {
            Grid::Ref { rows, .. } => *rows,
            Grid::Arr(a) => a.rows,
            Grid::One(_) => 1,
        }
    }

    pub fn cols(&self) -> usize {
        match self {
            Grid::Ref { cols, .. } => *cols,
            Grid::Arr(a) => a.cols,
            Grid::One(_) => 1,
        }
    }

    /// The value at (r, c) from the top-left; references are read past their clipped size too.
    pub fn get(&self, r: usize, c: usize) -> &Value {
        match self {
            Grid::Ref { engine, sheet, start, .. } => {
                let (row, col) = (start.row as usize + r, start.col as usize + c);
                if row >= MAX_ROWS as usize || col >= MAX_COLS as usize {
                    return &EMPTY;
                }
                engine.cell_value(*sheet, Addr::new(row as u32, col as u32))
            }
            Grid::Arr(a) => a.get(r, c),
            Grid::One(v) => {
                if r == 0 && c == 0 {
                    v
                } else {
                    &EMPTY
                }
            }
        }
    }

    /// Element `i` of a single row or column.
    pub fn at(&self, i: usize) -> &Value {
        if self.rows() == 1 { self.get(0, i) } else { self.get(i, 0) }
    }

    /// Length of a single row or column (`None` for a 2-D grid).
    pub fn vector_len(&self) -> Option<usize> {
        if self.rows() == 1 {
            Some(self.cols())
        } else if self.cols() == 1 {
            Some(self.rows())
        } else {
            None
        }
    }
}

impl<'a> Ctx<'a> {
    pub fn new(engine: &'a Engine, sheet: usize, at: Addr, now: f64) -> Self {
        Ctx { engine, sheet, at, now }
    }

    pub fn cell(&self, sheet: usize, addr: Addr) -> &'a Value {
        self.engine.cell_value(sheet, addr)
    }

    /// A sheet name (lowercase) to its index; `None` is the formula's own sheet.
    pub fn resolve(&self, sheet: &Option<String>) -> Option<usize> {
        match sheet {
            None => Some(self.sheet),
            Some(name) => self.engine.sheet_index_lower(name),
        }
    }

    /// Cuts a range down to the area where any sheet has data, so whole columns stay cheap.
    /// Ranges that start beyond the data keep their first row/column.
    pub fn clip(&self, range: Range) -> Range {
        let (max_row, max_col) = self.engine.extent();
        let end = Addr::new(
            range.end.row.min(max_row.max(range.start.row)),
            range.end.col.min(max_col.max(range.start.col)),
        );
        Range { start: range.start, end }
    }

    pub fn grid<'b>(&'b self, ev: &'b Ev) -> Grid<'b> {
        match ev {
            Ev::V(v) => Grid::One(v),
            Ev::A(a) => Grid::Arr(a),
            Ev::R { sheet, range } => {
                let r = self.clip(*range);
                Grid::Ref {
                    engine: self.engine,
                    sheet: *sheet,
                    start: r.start,
                    rows: r.rows() as usize,
                    cols: r.cols() as usize,
                }
            }
        }
    }

    /// Calls `f` for every non-empty cell of a range, in no particular order.
    pub fn each_in_range(&self, sheet: usize, range: Range, f: &mut dyn FnMut(Addr, &'a Value)) {
        let Some(sh) = self.engine.sheets.get(sheet) else { return };
        let Some(extent) = sh.extent() else { return };
        let Some(r) = range.intersect(&extent) else { return };
        if r.area() <= sh.cells.len() as u64 * 2 {
            for a in r.iter() {
                if let Some(c) = sh.cells.get(&a) {
                    f(a, &c.value);
                }
            }
        } else {
            for (a, c) in &sh.cells {
                if r.contains(*a) {
                    f(*a, &c.value);
                }
            }
        }
    }

    /// Calls `f` for every value of an argument: each element of an array, each non-empty cell
    /// of a reference (in no particular order), or the value itself. The flag says whether the
    /// value came from a reference or array (where text and booleans are usually skipped).
    pub fn each_value(&self, ev: &Ev, f: &mut dyn FnMut(&Value, bool)) {
        match ev {
            Ev::V(v) => f(v, false),
            Ev::A(a) => a.data.iter().for_each(|v| f(v, true)),
            Ev::R { sheet, range } => self.each_in_range(*sheet, *range, &mut |_, v| f(v, true)),
        }
    }

    /// A single value from a result: the value itself, the top-left of an array, or for a
    /// reference the cell itself or the one in the formula's row or column.
    pub fn scalar(&self, ev: &Ev) -> Value {
        match ev {
            Ev::V(v) => v.clone(),
            Ev::A(a) => a.data.first().cloned().unwrap_or(Value::Error(ErrorKind::Value)),
            Ev::R { sheet, range } => {
                if range.start == range.end {
                    return self.cell(*sheet, range.start).clone();
                }
                if range.cols() == 1 && range.start.row <= self.at.row && self.at.row <= range.end.row {
                    return self.cell(*sheet, Addr::new(self.at.row, range.start.col)).clone();
                }
                if range.rows() == 1 && range.start.col <= self.at.col && self.at.col <= range.end.col {
                    return self.cell(*sheet, Addr::new(range.start.row, self.at.col)).clone();
                }
                Value::Error(ErrorKind::Value)
            }
        }
    }

    /// The final value of a formula: a single value, with empty shown as 0.
    pub fn finish(&self, ev: &Ev) -> Value {
        let v = match ev {
            Ev::A(a) => a.data.first().cloned().unwrap_or(Value::Error(ErrorKind::Value)),
            _ => self.scalar(ev),
        };
        if v.is_empty() { Value::Number(0.0) } else { v }
    }

    /// A result as an array (references clipped to the used area).
    pub fn to_array(&self, ev: Ev) -> Array {
        match ev {
            Ev::V(v) => Array::new(1, 1, vec![v]),
            Ev::A(a) => a,
            Ev::R { sheet, range } => {
                let r = self.clip(range);
                let data = r.iter().map(|a| self.cell(sheet, a).clone()).collect();
                Array::new(r.rows() as usize, r.cols() as usize, data)
            }
        }
    }

    /// Applies `f` to each element (or to the single value).
    pub fn map(&self, ev: Ev, f: &dyn Fn(&Value) -> Value) -> Ev {
        if ev.is_multi() {
            let mut a = self.to_array(ev);
            for v in a.data.iter_mut() {
                *v = f(v);
            }
            Ev::A(a)
        } else {
            Ev::V(f(&self.scalar(&ev)))
        }
    }

    /// Applies `f` across several arguments, element by element when any of them is an array
    /// or a multi-cell reference (single rows and columns repeat to fill the larger shape).
    pub fn lift(&self, args: Vec<Ev>, f: &dyn Fn(&[Value]) -> Value) -> Ev {
        if !args.iter().any(Ev::is_multi) {
            let values: Vec<Value> = args.iter().map(|a| self.scalar(a)).collect();
            return Ev::V(f(&values));
        }
        let arrays: Vec<Array> = args.into_iter().map(|a| self.to_array(a)).collect();
        let rows = arrays.iter().map(|a| a.rows).max().unwrap_or(1);
        let cols = arrays.iter().map(|a| a.cols).max().unwrap_or(1);
        let mut data = Vec::with_capacity(rows * cols);
        let mut values = Vec::with_capacity(arrays.len());
        for r in 0..rows {
            for c in 0..cols {
                values.clear();
                values.extend(arrays.iter().map(|a| a.broadcast(r, c)));
                data.push(f(&values));
            }
        }
        Ev::A(Array::new(rows, cols, data))
    }

    pub fn eval(&self, e: &Expr) -> Ev {
        match e {
            Expr::Number(n) => Ev::V(Value::Number(*n)),
            Expr::Text(s) => Ev::V(Value::Text(s.clone())),
            Expr::Bool(b) => Ev::V(Value::Bool(*b)),
            Expr::Error(k) => Ev::err(*k),
            Expr::Missing => Ev::V(Value::Empty),
            Expr::Name(_) => Ev::err(ErrorKind::Name),
            Expr::Ref { sheet, range } => match self.resolve(sheet) {
                Some(s) => Ev::R { sheet: s, range: *range },
                None => Ev::err(ErrorKind::Ref),
            },
            Expr::Array(a) => Ev::A(a.clone()),
            Expr::Neg(a) => self.map(self.eval(a), &|v| to_num(v).map_or_else(Value::Error, |n| num_value(-n))),
            Expr::Percent(a) => {
                self.map(self.eval(a), &|v| to_num(v).map_or_else(Value::Error, |n| num_value(n / 100.0)))
            }
            Expr::Bin(op, a, b) => {
                let (l, r) = (self.eval(a), self.eval(b));
                if !l.is_multi() && !r.is_multi() {
                    Ev::V(apply_op(*op, &self.scalar(&l), &self.scalar(&r)))
                } else {
                    let op = *op;
                    self.lift(vec![l, r], &|v| apply_op(op, &v[0], &v[1]))
                }
            }
            Expr::Range(a, b) => match (self.eval(a), self.eval(b)) {
                (Ev::R { sheet: s1, range: r1 }, Ev::R { sheet: s2, range: r2 }) if s1 == s2 => {
                    let start = Addr::new(r1.start.row.min(r2.start.row), r1.start.col.min(r2.start.col));
                    let end = Addr::new(r1.end.row.max(r2.end.row), r1.end.col.max(r2.end.col));
                    Ev::R { sheet: s1, range: Range { start, end } }
                }
                (Ev::V(Value::Error(e)), _) | (_, Ev::V(Value::Error(e))) => Ev::err(e),
                _ => Ev::err(ErrorKind::Value),
            },
            Expr::Call(func, args) => self.call(func, args),
        }
    }

    fn call(&self, func: &Func, args: &[Expr]) -> Ev {
        match func {
            Func::Builtin(b) => {
                if args.len() < b.min || args.len() > b.max {
                    return Ev::err(ErrorKind::Value);
                }
                let result = match b.imp {
                    Imp::Lazy(f) => f(self, args),
                    Imp::Eager(f) => f(self, args.iter().map(|a| self.eval(a)).collect()),
                    Imp::Scalar(f) => {
                        let evs: Vec<Ev> = args.iter().map(|a| self.eval(a)).collect();
                        Ok(self.lift(evs, &|v| f(self, v).unwrap_or_else(Value::Error)))
                    }
                };
                result.unwrap_or_else(Ev::err)
            }
            Func::Custom(name) => match self.engine.custom.get(name) {
                Some(custom) => {
                    let values: Vec<Arg> = args.iter().map(|a| self.to_arg(self.eval(a))).collect();
                    Ev::V((custom.f)(&values))
                }
                None => Ev::err(ErrorKind::Name),
            },
        }
    }

    fn to_arg(&self, ev: Ev) -> Arg {
        match ev {
            Ev::V(v) => Arg::Value(v),
            Ev::R { range, .. } if range.start == range.end => Arg::Value(self.scalar(&ev)),
            other => {
                let a = self.to_array(other);
                Arg::Range { rows: a.rows as u32, cols: a.cols as u32, values: a.data }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coercions() {
        assert_eq!(to_num(&Value::Text("3".into())), Ok(3.0));
        assert_eq!(to_num(&Value::Text(" 1,000 ".into())), Ok(1000.0));
        assert_eq!(to_num(&Value::Text("50%".into())), Ok(0.5));
        assert_eq!(to_num(&Value::Text("abc".into())), Err(ErrorKind::Value));
        assert_eq!(to_num(&Value::Text("".into())), Err(ErrorKind::Value));
        assert_eq!(to_num(&Value::Bool(true)), Ok(1.0));
        assert_eq!(to_num(&Value::Empty), Ok(0.0));
        assert_eq!(to_text(&Value::Number(1.0 / 3.0)), Ok("0.333333333333333".into()));
        assert_eq!(to_bool(&Value::Text("true".into())), Ok(true));
        assert_eq!(to_bool(&Value::Text("yes".into())), Err(ErrorKind::Value));
    }

    #[test]
    fn comparisons() {
        let n = |x: f64| Value::Number(x);
        let t = |s: &str| Value::Text(s.into());
        assert_eq!(compare(&n(1.0), &n(2.0)), Ok(Ordering::Less));
        assert_eq!(compare(&n(0.1 + 0.2), &n(0.3)), Ok(Ordering::Equal));
        assert_eq!(compare(&t("abc"), &t("ABC")), Ok(Ordering::Equal));
        assert_eq!(compare(&n(1e9), &t("a")), Ok(Ordering::Less));
        assert_eq!(compare(&t("z"), &Value::Bool(false)), Ok(Ordering::Less));
        assert_eq!(compare(&Value::Empty, &n(0.0)), Ok(Ordering::Equal));
        assert_eq!(compare(&Value::Empty, &t("")), Ok(Ordering::Equal));
        assert_eq!(compare(&Value::Empty, &Value::Bool(false)), Ok(Ordering::Equal));
        assert_eq!(compare(&Value::Error(ErrorKind::NA), &n(1.0)), Err(ErrorKind::NA));
    }

    #[test]
    fn operators() {
        let n = |x: f64| Value::Number(x);
        assert_eq!(apply_op(BinOp::Add, &Value::Text("3".into()), &n(1.0)), n(4.0));
        assert_eq!(apply_op(BinOp::Add, &Value::Bool(true), &n(1.0)), n(2.0));
        assert_eq!(apply_op(BinOp::Div, &n(1.0), &n(0.0)), Value::Error(ErrorKind::Div0));
        assert_eq!(apply_op(BinOp::Pow, &n(-8.0), &n(1.0 / 3.0)), Value::Error(ErrorKind::Num));
        assert_eq!(apply_op(BinOp::Pow, &n(0.0), &n(0.0)), Value::Error(ErrorKind::Num));
        assert_eq!(apply_op(BinOp::Concat, &n(1.5), &Value::Bool(true)), Value::Text("1.5TRUE".into()));
        assert_eq!(
            apply_op(BinOp::Add, &Value::Error(ErrorKind::Ref), &Value::Error(ErrorKind::NA)),
            Value::Error(ErrorKind::Ref)
        );
        assert_eq!(apply_op(BinOp::Mul, &Value::Text("x".into()), &n(2.0)), Value::Error(ErrorKind::Value));
    }
}
