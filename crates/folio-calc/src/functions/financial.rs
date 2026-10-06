//! Financial functions. Money paid out is negative, money received positive; `type` 1 means
//! payments at the start of each period, 0 (the default) at the end.

use super::{Builtin, Imp, MANY, collect_numbers, def, num_arg, opt_num};
use crate::eval::{Ctx, Ev, R, num_value, to_num};
use crate::value::{ErrorKind, Value};

const F: &str = "Financial";

pub(crate) const FUNCTIONS: &[Builtin] = &[
    def(
        "PMT",
        F,
        "PMT(rate, nper, pv, [fv], [type])",
        "The payment per period of a loan or investment.",
        3,
        5,
        Imp::Scalar(pmt),
    ),
    def("FV", F, "FV(rate, nper, pmt, [pv], [type])", "The future value of an investment.", 3, 5, Imp::Scalar(fv)),
    def("PV", F, "PV(rate, nper, pmt, [fv], [type])", "The present value of an investment.", 3, 5, Imp::Scalar(pv)),
    def(
        "NPV",
        F,
        "NPV(rate, value1, [value2], …)",
        "The net present value of future cash flows at a discount rate.",
        2,
        MANY,
        Imp::Eager(npv),
    ),
    def("IRR", F, "IRR(values, [guess])", "The internal rate of return of cash flows.", 1, 2, Imp::Eager(irr)),
    def(
        "RATE",
        F,
        "RATE(nper, pmt, pv, [fv], [type], [guess])",
        "The interest rate per period of an annuity.",
        3,
        6,
        Imp::Scalar(rate),
    ),
    def(
        "NPER",
        F,
        "NPER(rate, pmt, pv, [fv], [type])",
        "The number of periods of an investment.",
        3,
        5,
        Imp::Scalar(nper),
    ),
];

fn kind(args: &[Value], i: usize) -> R<f64> {
    Ok(if opt_num(args, i, 0.0)? != 0.0 { 1.0 } else { 0.0 })
}

fn pmt_of(r: f64, n: f64, pv: f64, fv: f64, t: f64) -> f64 {
    if r == 0.0 {
        return -(pv + fv) / n;
    }
    let g = (1.0 + r).powf(n);
    -(r * (fv + pv * g)) / ((1.0 + r * t) * (g - 1.0))
}

fn fv_of(r: f64, n: f64, pmt: f64, pv: f64, t: f64) -> f64 {
    if r == 0.0 {
        return -(pv + pmt * n);
    }
    let g = (1.0 + r).powf(n);
    -(pv * g + pmt * (1.0 + r * t) * (g - 1.0) / r)
}

fn pmt(_: &Ctx, args: &[Value]) -> R<Value> {
    let (r, n, pv) = (to_num(&args[0])?, to_num(&args[1])?, to_num(&args[2])?);
    let (fv, t) = (opt_num(args, 3, 0.0)?, kind(args, 4)?);
    if n == 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(num_value(pmt_of(r, n, pv, fv, t)))
}

fn fv(_: &Ctx, args: &[Value]) -> R<Value> {
    let (r, n, p) = (to_num(&args[0])?, to_num(&args[1])?, to_num(&args[2])?);
    let (pv, t) = (opt_num(args, 3, 0.0)?, kind(args, 4)?);
    Ok(num_value(fv_of(r, n, p, pv, t)))
}

fn pv(_: &Ctx, args: &[Value]) -> R<Value> {
    let (r, n, p) = (to_num(&args[0])?, to_num(&args[1])?, to_num(&args[2])?);
    let (fv, t) = (opt_num(args, 3, 0.0)?, kind(args, 4)?);
    if r == 0.0 {
        return Ok(num_value(-(fv + p * n)));
    }
    let g = (1.0 + r).powf(n);
    Ok(num_value(-(fv + p * (1.0 + r * t) * (g - 1.0) / r) / g))
}

fn npv(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let r = num_arg(ctx, &args, 0, 0.0)?;
    if r == -1.0 {
        return Err(ErrorKind::Div0);
    }
    let flows = collect_numbers(ctx, &args[1..])?;
    let total: f64 = flows.iter().enumerate().map(|(i, v)| v / (1.0 + r).powi(i as i32 + 1)).sum();
    Ok(Ev::num(total))
}

/// Newton's method with a numerical derivative; `None` when it does not converge.
fn solve(f: &dyn Fn(f64) -> f64, guess: f64) -> Option<f64> {
    let mut x = guess;
    for _ in 0..100 {
        let y = f(x);
        if !y.is_finite() {
            return None;
        }
        if y.abs() < 1e-10 {
            return Some(x);
        }
        let h = 1e-7 * x.abs().max(1e-3);
        let d = (f(x + h) - f(x - h)) / (2.0 * h);
        if d == 0.0 || !d.is_finite() {
            return None;
        }
        let next = x - y / d;
        if (next - x).abs() < 1e-12 {
            return Some(next);
        }
        x = next;
        if x <= -1.0 {
            x = -0.999_999;
        }
    }
    (f(x).abs() < 1e-6).then_some(x)
}

fn irr(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let flows = collect_numbers(ctx, &args[..1])?;
    let guess = num_arg(ctx, &args, 1, 0.1)?;
    if !flows.iter().any(|v| *v > 0.0) || !flows.iter().any(|v| *v < 0.0) {
        return Err(ErrorKind::Num);
    }
    let f = |r: f64| flows.iter().enumerate().map(|(i, v)| v / (1.0 + r).powi(i as i32)).sum::<f64>();
    solve(&f, guess).map(Ev::num).ok_or(ErrorKind::Num)
}

fn rate(_: &Ctx, args: &[Value]) -> R<Value> {
    let (n, p, pv) = (to_num(&args[0])?, to_num(&args[1])?, to_num(&args[2])?);
    let (fv, t) = (opt_num(args, 3, 0.0)?, kind(args, 4)?);
    let guess = opt_num(args, 5, 0.1)?;
    if n <= 0.0 {
        return Err(ErrorKind::Num);
    }
    let f = |r: f64| {
        if r.abs() < 1e-12 {
            pv + p * n + fv
        } else {
            let g = (1.0 + r).powf(n);
            pv * g + p * (1.0 + r * t) * (g - 1.0) / r + fv
        }
    };
    solve(&f, guess).map(num_value).ok_or(ErrorKind::Num)
}

fn nper(_: &Ctx, args: &[Value]) -> R<Value> {
    let (r, p, pv) = (to_num(&args[0])?, to_num(&args[1])?, to_num(&args[2])?);
    let (fv, t) = (opt_num(args, 3, 0.0)?, kind(args, 4)?);
    if r == 0.0 {
        if p == 0.0 {
            return Err(ErrorKind::Num);
        }
        return Ok(num_value(-(pv + fv) / p));
    }
    let a = p * (1.0 + r * t) - fv * r;
    let b = p * (1.0 + r * t) + pv * r;
    if a / b <= 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(num_value((a / b).ln() / (1.0 + r).ln()))
}
