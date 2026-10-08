//! Logical functions. IF, IFS, IFERROR, IFNA, SWITCH and CHOOSE only evaluate the branch they
//! return (when the condition is a single value).

use super::{Builtin, Imp, MANY, def};
use crate::eval::{Ctx, Ev, R, compare, to_bool, to_num};
use crate::parser::Expr;
use crate::value::{ErrorKind, Value};

const L: &str = "Logical";

pub(crate) const FUNCTIONS: &[Builtin] = &[
    def(
        "IF",
        L,
        "IF(condition, [value_if_true], [value_if_false])",
        "One value when a condition is true, another when it is false.",
        1,
        3,
        Imp::Lazy(if_),
    ),
    def(
        "IFS",
        L,
        "IFS(condition1, value1, [condition2, value2], …)",
        "The value of the first true condition (#N/A if none).",
        2,
        MANY,
        Imp::Lazy(ifs),
    ),
    def(
        "IFERROR",
        L,
        "IFERROR(value, value_if_error)",
        "The value, or another one when it is an error.",
        2,
        2,
        Imp::Lazy(iferror),
    ),
    def("IFNA", L, "IFNA(value, value_if_na)", "The value, or another one when it is #N/A.", 2, 2, Imp::Lazy(ifna)),
    def("AND", L, "AND(logical1, [logical2], …)", "TRUE when every argument is true.", 1, MANY, Imp::Eager(and)),
    def("OR", L, "OR(logical1, [logical2], …)", "TRUE when any argument is true.", 1, MANY, Imp::Eager(or)),
    def(
        "XOR",
        L,
        "XOR(logical1, [logical2], …)",
        "TRUE when an odd number of arguments are true.",
        1,
        MANY,
        Imp::Eager(xor),
    ),
    def("NOT", L, "NOT(logical)", "Reverses TRUE and FALSE.", 1, 1, Imp::Scalar(not)),
    def("TRUE", L, "TRUE()", "The value TRUE.", 0, 0, Imp::Scalar(true_)),
    def("FALSE", L, "FALSE()", "The value FALSE.", 0, 0, Imp::Scalar(false_)),
    def(
        "SWITCH",
        L,
        "SWITCH(expression, value1, result1, [value2, result2], …, [default])",
        "The result matching the expression's value, or the default.",
        3,
        MANY,
        Imp::Lazy(switch),
    ),
    def(
        "CHOOSE",
        L,
        "CHOOSE(index, value1, [value2], …)",
        "The value at a position in the list.",
        2,
        MANY,
        Imp::Lazy(choose),
    ),
];

fn if_(ctx: &Ctx, args: &[Expr]) -> R<Ev> {
    let cond = ctx.eval(&args[0]);
    let branch = |i: usize, default: bool| args.get(i).map_or(Ev::V(Value::Bool(default)), |e| ctx.eval(e));
    if cond.is_multi() {
        // Element by element, as in SUM(IF(A1:A9>0, A1:A9)).
        let (t, f) = (branch(1, true), branch(2, false));
        return Ok(ctx.lift(vec![cond, t, f], &|v| match to_bool(&v[0]) {
            Ok(true) => {
                if v[1].is_empty() {
                    Value::Number(0.0)
                } else {
                    v[1].clone()
                }
            }
            Ok(false) => {
                if v[2].is_empty() {
                    Value::Number(0.0)
                } else {
                    v[2].clone()
                }
            }
            Err(e) => Value::Error(e),
        }));
    }
    if to_bool(&ctx.scalar(&cond))? { Ok(branch(1, true)) } else { Ok(branch(2, false)) }
}

fn ifs(ctx: &Ctx, args: &[Expr]) -> R<Ev> {
    if !args.len().is_multiple_of(2) {
        return Err(ErrorKind::Value);
    }
    for pair in args.chunks(2) {
        let cond = ctx.eval(&pair[0]);
        if to_bool(&ctx.scalar(&cond))? {
            return Ok(ctx.eval(&pair[1]));
        }
    }
    Err(ErrorKind::NA)
}

fn on_error(ctx: &Ctx, args: &[Expr], catch: fn(ErrorKind) -> bool) -> R<Ev> {
    let value = ctx.eval(&args[0]);
    if value.is_multi() {
        let fallback = ctx.eval(&args[1]);
        return Ok(ctx.lift(vec![value, fallback], &|v| match &v[0] {
            Value::Error(e) if catch(*e) => v[1].clone(),
            other => other.clone(),
        }));
    }
    match ctx.scalar(&value) {
        Value::Error(e) if catch(e) => Ok(ctx.eval(&args[1])),
        _ => Ok(value),
    }
}

fn iferror(ctx: &Ctx, args: &[Expr]) -> R<Ev> {
    on_error(ctx, args, |_| true)
}

fn ifna(ctx: &Ctx, args: &[Expr]) -> R<Ev> {
    on_error(ctx, args, |e| e == ErrorKind::NA)
}

/// The truth values of AND/OR/XOR arguments: text and empty cells in ranges are skipped.
fn truths(ctx: &Ctx, args: &[Ev]) -> R<Vec<bool>> {
    let mut out = Vec::new();
    let mut error = None;
    for arg in args {
        ctx.each_value(arg, &mut |v, in_range| {
            if error.is_some() {
                return;
            }
            match v {
                Value::Bool(b) => out.push(*b),
                Value::Number(n) => out.push(*n != 0.0),
                Value::Error(e) => error = Some(*e),
                Value::Empty => {}
                Value::Text(_) if in_range => {}
                other => match to_bool(other) {
                    Ok(b) => out.push(b),
                    Err(e) => error = Some(e),
                },
            }
        });
    }
    if let Some(e) = error {
        return Err(e);
    }
    if out.is_empty() {
        return Err(ErrorKind::Value);
    }
    Ok(out)
}

fn and(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    Ok(Ev::V(Value::Bool(truths(ctx, &args)?.iter().all(|b| *b))))
}

fn or(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    Ok(Ev::V(Value::Bool(truths(ctx, &args)?.iter().any(|b| *b))))
}

fn xor(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    Ok(Ev::V(Value::Bool(truths(ctx, &args)?.iter().filter(|b| **b).count() % 2 == 1)))
}

fn not(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(!to_bool(&args[0])?))
}

fn true_(_: &Ctx, _: &[Value]) -> R<Value> {
    Ok(Value::Bool(true))
}

fn false_(_: &Ctx, _: &[Value]) -> R<Value> {
    Ok(Value::Bool(false))
}

fn switch(ctx: &Ctx, args: &[Expr]) -> R<Ev> {
    let target = ctx.scalar(&ctx.eval(&args[0]));
    if let Value::Error(e) = target {
        return Err(e);
    }
    let rest = &args[1..];
    for pair in rest.chunks(2) {
        if pair.len() == 2 {
            let candidate = ctx.scalar(&ctx.eval(&pair[0]));
            if compare(&target, &candidate)?.is_eq()
                && std::mem::discriminant(&target) == std::mem::discriminant(&candidate)
            {
                return Ok(ctx.eval(&pair[1]));
            }
        } else {
            return Ok(ctx.eval(&pair[0]));
        }
    }
    Err(ErrorKind::NA)
}

fn choose(ctx: &Ctx, args: &[Expr]) -> R<Ev> {
    let index = to_num(&ctx.scalar(&ctx.eval(&args[0])))?.trunc();
    if index < 1.0 || index as usize >= args.len() {
        return Err(ErrorKind::Value);
    }
    Ok(ctx.eval(&args[index as usize]))
}
