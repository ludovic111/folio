//! Built-in functions: the registry and helpers shared by the function modules.

use std::fmt;
use std::sync::OnceLock;

use crate::eval::{Ctx, Ev, R, to_num};
use crate::parser::Expr;
use crate::util::FxMap;
use crate::value::Value;

pub(crate) mod criteria;
mod date;
mod financial;
mod info;
mod logical;
mod lookup;
mod math;
mod stats;
mod text;

/// A function that works on single values; it is applied element by element to arrays.
pub(crate) type ScalarFn = fn(&Ctx, &[Value]) -> R<Value>;
/// A function that receives its evaluated arguments (references stay references).
pub(crate) type EagerFn = fn(&Ctx, Vec<Ev>) -> R<Ev>;
/// A function that evaluates its own arguments (IF, IFERROR, CHOOSE…).
pub(crate) type LazyFn = fn(&Ctx, &[Expr]) -> R<Ev>;

#[derive(Clone, Copy)]
pub(crate) enum Imp {
    Scalar(ScalarFn),
    Eager(EagerFn),
    Lazy(LazyFn),
}

/// A built-in function and its documentation.
#[derive(Clone, Copy)]
pub struct Builtin {
    /// Uppercase name, such as `SUM` or `STDEV.S`.
    pub name: &'static str,
    /// How to call it: `SUM(number1, [number2], …)`.
    pub syntax: &'static str,
    /// One sentence on what it does.
    pub summary: &'static str,
    /// Math, Statistical, Logical, Lookup, Text, Date, Financial or Information.
    pub category: &'static str,
    /// Recalculated on every recalculation (NOW, TODAY, RAND, RANDBETWEEN, OFFSET, INDIRECT).
    pub volatile: bool,
    pub(crate) min: usize,
    pub(crate) max: usize,
    pub(crate) imp: Imp,
}

impl fmt::Debug for Builtin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Builtin").field("name", &self.name).finish()
    }
}

/// No upper limit on arguments.
pub(crate) const MANY: usize = 255;

pub(crate) const fn def(
    name: &'static str,
    category: &'static str,
    syntax: &'static str,
    summary: &'static str,
    min: usize,
    max: usize,
    imp: Imp,
) -> Builtin {
    Builtin { name, syntax, summary, category, volatile: false, min, max, imp }
}

const VOLATILE: [&str; 6] = ["NOW", "TODAY", "RAND", "RANDBETWEEN", "OFFSET", "INDIRECT"];

fn table() -> &'static Vec<Builtin> {
    static TABLE: OnceLock<Vec<Builtin>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut all: Vec<Builtin> = [
            math::FUNCTIONS,
            stats::FUNCTIONS,
            logical::FUNCTIONS,
            lookup::FUNCTIONS,
            text::FUNCTIONS,
            date::FUNCTIONS,
            info::FUNCTIONS,
            financial::FUNCTIONS,
        ]
        .concat();
        for b in all.iter_mut() {
            b.volatile = VOLATILE.contains(&b.name);
        }
        all.sort_by_key(|b| b.name);
        all
    })
}

/// Every built-in function, sorted by name (for documentation and the function picker).
pub fn builtin_functions() -> &'static [Builtin] {
    table()
}

/// A built-in function by uppercase name.
pub(crate) fn lookup(name: &str) -> Option<&'static Builtin> {
    static INDEX: OnceLock<FxMap<&'static str, usize>> = OnceLock::new();
    let index = INDEX.get_or_init(|| table().iter().enumerate().map(|(i, b)| (b.name, i)).collect());
    index.get(name).map(|&i| &table()[i])
}

// ---------------------------------------------------------------------------------------------
// Helpers for function implementations

/// Argument `i` as a number, or `default` when it was not given.
pub(crate) fn opt_num(args: &[Value], i: usize, default: f64) -> R<f64> {
    args.get(i).map_or(Ok(default), to_num)
}

/// Feeds the numbers of the arguments to `f`, the way SUM reads them: numbers in references
/// and arrays (text, booleans and empty cells there are skipped), and typed arguments coerced
/// (`SUM("3", TRUE)` is 4). The first error found is returned.
pub(crate) fn numbers(ctx: &Ctx, args: &[Ev], f: &mut dyn FnMut(f64)) -> R<()> {
    let mut error = None;
    for arg in args {
        ctx.each_value(arg, &mut |v, in_range| {
            if error.is_some() {
                return;
            }
            match v {
                Value::Number(n) => f(*n),
                Value::Error(e) => error = Some(*e),
                _ if in_range => {}
                Value::Empty => {}
                other => match to_num(other) {
                    Ok(n) => f(n),
                    Err(e) => error = Some(e),
                },
            }
        });
        if let Some(e) = error {
            return Err(e);
        }
    }
    Ok(())
}

/// The numbers of the arguments as a list (see [`numbers`]).
pub(crate) fn collect_numbers(ctx: &Ctx, args: &[Ev]) -> R<Vec<f64>> {
    let mut out = Vec::new();
    numbers(ctx, args, &mut |n| out.push(n))?;
    Ok(out)
}

/// A single argument value (implicit intersection for ranges).
pub(crate) fn scalar_arg(ctx: &Ctx, args: &[Ev], i: usize) -> Option<Value> {
    args.get(i).map(|a| ctx.scalar(a))
}

/// A single argument as a number, or `default` when not given.
pub(crate) fn num_arg(ctx: &Ctx, args: &[Ev], i: usize, default: f64) -> R<f64> {
    match scalar_arg(ctx, args, i) {
        Some(v) => to_num(&v),
        None => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry() {
        let all = builtin_functions();
        assert!(all.len() >= 125, "{} functions", all.len());
        assert!(all.windows(2).all(|w| w[0].name < w[1].name), "sorted, no duplicates");
        for b in all {
            assert!(b.syntax.starts_with(b.name), "{}", b.name);
            assert!(!b.summary.is_empty());
            assert!(
                ["Math", "Statistical", "Logical", "Lookup", "Text", "Date", "Financial", "Information"]
                    .contains(&b.category)
            );
        }
        assert!(lookup("SUM").is_some());
        assert!(lookup("NOW").unwrap().volatile);
        assert!(!lookup("SUM").unwrap().volatile);
        assert!(lookup("NOPE").is_none());
    }
}
