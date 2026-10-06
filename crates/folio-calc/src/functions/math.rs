//! Math functions.

use std::sync::atomic::{AtomicU64, Ordering};

use super::{Builtin, Imp, MANY, def, numbers, opt_num};
use crate::eval::{Ctx, Ev, R, num_value, power, to_num};
use crate::format::{round_half_away, round_sig};
use crate::value::{ErrorKind, Value};

const M: &str = "Math";

pub(crate) const FUNCTIONS: &[Builtin] = &[
    def("SUM", M, "SUM(number1, [number2], …)", "Adds numbers and the numbers in ranges.", 1, MANY, Imp::Eager(sum)),
    def("PRODUCT", M, "PRODUCT(number1, [number2], …)", "Multiplies numbers together.", 1, MANY, Imp::Eager(product)),
    def(
        "SUMPRODUCT",
        M,
        "SUMPRODUCT(array1, [array2], …)",
        "Multiplies matching elements of same-sized arrays and adds the products.",
        1,
        MANY,
        Imp::Eager(sumproduct),
    ),
    def("ROUND", M, "ROUND(number, digits)", "Rounds to a number of digits, halves away from zero.", 1, 2, Imp::Scalar(round)),
    def("ROUNDUP", M, "ROUNDUP(number, digits)", "Rounds away from zero.", 1, 2, Imp::Scalar(roundup)),
    def("ROUNDDOWN", M, "ROUNDDOWN(number, digits)", "Rounds toward zero.", 1, 2, Imp::Scalar(rounddown)),
    def("INT", M, "INT(number)", "Rounds down to the nearest integer.", 1, 1, Imp::Scalar(int)),
    def("TRUNC", M, "TRUNC(number, [digits])", "Cuts off the decimals.", 1, 2, Imp::Scalar(rounddown)),
    def("ABS", M, "ABS(number)", "The absolute value.", 1, 1, Imp::Scalar(abs)),
    def("MOD", M, "MOD(number, divisor)", "The remainder after division, with the divisor's sign.", 2, 2, Imp::Scalar(modulo)),
    def("POWER", M, "POWER(number, power)", "A number raised to a power.", 2, 2, Imp::Scalar(pow)),
    def("SQRT", M, "SQRT(number)", "The square root.", 1, 1, Imp::Scalar(sqrt)),
    def("EXP", M, "EXP(number)", "e raised to a power.", 1, 1, Imp::Scalar(exp)),
    def("LN", M, "LN(number)", "The natural logarithm.", 1, 1, Imp::Scalar(ln)),
    def("LOG", M, "LOG(number, [base])", "The logarithm in a base (10 by default).", 1, 2, Imp::Scalar(log)),
    def("LOG10", M, "LOG10(number)", "The base-10 logarithm.", 1, 1, Imp::Scalar(log10)),
    def("PI", M, "PI()", "The number π.", 0, 0, Imp::Scalar(pi)),
    def("RAND", M, "RAND()", "A random number from 0 up to 1, new on every recalculation.", 0, 0, Imp::Scalar(rand)),
    def(
        "RANDBETWEEN",
        M,
        "RANDBETWEEN(bottom, top)",
        "A random integer between two numbers, new on every recalculation.",
        2,
        2,
        Imp::Scalar(randbetween),
    ),
    def("CEILING", M, "CEILING(number, [significance])", "Rounds up to a multiple of significance.", 1, 2, Imp::Scalar(ceiling)),
    def("FLOOR", M, "FLOOR(number, [significance])", "Rounds down to a multiple of significance.", 1, 2, Imp::Scalar(floor)),
    def("SIGN", M, "SIGN(number)", "1 for positive numbers, -1 for negative ones, 0 for zero.", 1, 1, Imp::Scalar(sign)),
];

fn sum(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut total = 0.0;
    numbers(ctx, &args, &mut |n| total += n)?;
    Ok(Ev::num(total))
}

fn product(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut total = 1.0;
    let mut seen = false;
    numbers(ctx, &args, &mut |n| {
        total *= n;
        seen = true;
    })?;
    Ok(Ev::num(if seen { total } else { 0.0 }))
}

fn sumproduct(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let dims = args[0].dims();
    if args.iter().any(|a| a.dims() != dims) {
        return Err(ErrorKind::Value);
    }
    let arrays: Vec<_> = args.into_iter().map(|a| ctx.to_array(a)).collect();
    let len = arrays.iter().map(|a| a.data.len()).min().unwrap_or(0);
    let mut total = 0.0;
    for i in 0..len {
        let mut p = 1.0;
        for a in &arrays {
            match &a.data[i] {
                Value::Number(n) => p *= n,
                Value::Error(e) => return Err(*e),
                _ => p = 0.0,
            }
        }
        total += p;
    }
    Ok(Ev::num(total))
}

fn digits(args: &[Value]) -> R<i32> {
    Ok(opt_num(args, 1, 0.0)?.trunc() as i32)
}

fn round(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(num_value(round_half_away(to_num(&args[0])?, digits(args)?)))
}

fn scaled(x: f64, d: i32, f: fn(f64) -> f64) -> f64 {
    let d = d.clamp(-308, 308);
    let scale = 10f64.powi(d.abs());
    let y = if d >= 0 { x * scale } else { x / scale };
    if !y.is_finite() || y.abs() >= 1e17 {
        return x;
    }
    let y = f(round_sig(y, 15));
    if d >= 0 { y / scale } else { y * scale }
}

fn roundup(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    Ok(num_value(scaled(x, digits(args)?, |y| y.abs().ceil().copysign(y))))
}

fn rounddown(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    Ok(num_value(scaled(x, digits(args)?, f64::trunc)))
}

fn int(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(num_value(to_num(&args[0])?.floor()))
}

fn abs(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(num_value(to_num(&args[0])?.abs()))
}

fn modulo(_: &Ctx, args: &[Value]) -> R<Value> {
    let (n, d) = (to_num(&args[0])?, to_num(&args[1])?);
    if d == 0.0 {
        return Err(ErrorKind::Div0);
    }
    let r = n - d * (n / d).floor();
    // Remove noise such as MOD(1.1, 1) = 0.10000000000000009.
    Ok(num_value(round_sig(r, 15)))
}

fn pow(_: &Ctx, args: &[Value]) -> R<Value> {
    power(to_num(&args[0])?, to_num(&args[1])?)
}

fn sqrt(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    if x < 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(num_value(x.sqrt()))
}

fn exp(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(num_value(to_num(&args[0])?.exp()))
}

fn ln(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    if x <= 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(num_value(x.ln()))
}

fn log(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    let base = opt_num(args, 1, 10.0)?;
    if x <= 0.0 || base <= 0.0 {
        return Err(ErrorKind::Num);
    }
    if base == 1.0 {
        return Err(ErrorKind::Div0);
    }
    let r = if base == 10.0 { x.log10() } else { x.ln() / base.ln() };
    Ok(num_value(round_sig(r, 15)))
}

fn log10(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    if x <= 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(num_value(x.log10()))
}

fn pi(_: &Ctx, _: &[Value]) -> R<Value> {
    Ok(Value::Number(std::f64::consts::PI))
}

/// A random number in [0, 1) from a SplitMix64 sequence seeded by the clock.
pub(crate) fn random() -> f64 {
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut seed = STATE.load(Ordering::Relaxed);
    if seed == 0 {
        seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x1234_5678, |d| d.as_nanos() as u64)
            | 1;
    }
    let next = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    STATE.store(next, Ordering::Relaxed);
    let mut z = next;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

fn rand(_: &Ctx, _: &[Value]) -> R<Value> {
    Ok(Value::Number(random()))
}

fn randbetween(_: &Ctx, args: &[Value]) -> R<Value> {
    let lo = to_num(&args[0])?.ceil();
    let hi = to_num(&args[1])?.floor();
    if lo > hi {
        return Err(ErrorKind::Num);
    }
    Ok(Value::Number((lo + (random() * (hi - lo + 1.0)).floor()).min(hi)))
}

fn ceiling(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    let sig = opt_num(args, 1, 1.0)?;
    if sig == 0.0 {
        return Ok(Value::Number(0.0));
    }
    if x > 0.0 && sig < 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(num_value(round_sig(round_sig(x / sig, 15).ceil() * sig, 15)))
}

fn floor(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    let sig = opt_num(args, 1, 1.0)?;
    if sig == 0.0 {
        return if x == 0.0 { Ok(Value::Number(0.0)) } else { Err(ErrorKind::Div0) };
    }
    if x > 0.0 && sig < 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(num_value(round_sig(round_sig(x / sig, 15).floor() * sig, 15)))
}

fn sign(_: &Ctx, args: &[Value]) -> R<Value> {
    let x = to_num(&args[0])?;
    Ok(Value::Number(if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }))
}
