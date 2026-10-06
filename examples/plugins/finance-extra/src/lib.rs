//! finance-extra: an example folio plugin with financial functions folio doesn't ship:
//! XNPV and XIRR (cash flows on irregular dates), CAGR (compound annual growth) and GEOMEAN.
//!
//! Build it with `plugin.build` (or `cargo build --release` here) and install it with
//! `plugin.publishLocal`; see the SDK's GUIDE.md.

use folio_plugin::prelude::*;

/// Every value of a range (or a single value) as a number; blanks and text are `#VALUE!`.
fn series(arg: &Arg) -> Result<Vec<f64>, ErrorKind> {
    arg.values()
        .iter()
        .map(|v| match v {
            Value::Number(n) => Ok(*n),
            Value::Error(e) => Err(*e),
            Value::Text(_) if matches!(arg, Arg::Value(_)) => v.number(),
            _ => Err(ErrorKind::Value),
        })
        .collect()
}

/// Cash flows and their dates (day serials), checked: same count, at least one of each sign
/// when `needs_both_signs`, no date before the first.
fn flows(values: &Arg, dates: &Arg) -> Result<(Vec<f64>, Vec<f64>), ErrorKind> {
    let v = series(values)?;
    let d: Vec<f64> = series(dates)?.into_iter().map(f64::trunc).collect();
    if v.is_empty() || v.len() != d.len() {
        return Err(ErrorKind::Num);
    }
    if d.iter().any(|x| *x < d[0]) {
        return Err(ErrorKind::Num);
    }
    // Years from the first date.
    let t = d.iter().map(|x| (x - d[0]) / 365.0).collect();
    Ok((v, t))
}

fn npv_at(rate: f64, v: &[f64], t: &[f64]) -> f64 {
    v.iter().zip(t).map(|(v, t)| v / (1.0 + rate).powf(*t)).sum()
}

/// XNPV(rate, values, dates)
fn xnpv(args: &[Arg]) -> Result<f64, ErrorKind> {
    let rate = args[0].number()?;
    if rate <= -1.0 {
        return Err(ErrorKind::Num);
    }
    let (v, t) = flows(&args[1], &args[2])?;
    Ok(npv_at(rate, &v, &t))
}

/// XIRR(values, dates, [guess]): Newton's method from the guess, then bisection if it wanders.
fn xirr(args: &[Arg]) -> Result<f64, ErrorKind> {
    let (v, t) = flows(&args[0], &args[1])?;
    if !(v.iter().any(|x| *x > 0.0) && v.iter().any(|x| *x < 0.0)) {
        return Err(ErrorKind::Num);
    }
    let guess = match args.get(2) {
        Some(Arg::Value(Value::Empty)) | None => 0.1,
        Some(a) => a.number()?,
    };
    let f = |r: f64| npv_at(r, &v, &t);
    let df = |r: f64| v.iter().zip(&t).map(|(v, t)| -t * v / (1.0 + r).powf(t + 1.0)).sum::<f64>();
    let mut r = guess;
    for _ in 0..100 {
        let (y, dy) = (f(r), df(r));
        if !y.is_finite() || !dy.is_finite() || dy == 0.0 {
            break;
        }
        let next = r - y / dy;
        if next <= -1.0 || !next.is_finite() {
            break;
        }
        if (next - r).abs() < 1e-10 {
            return Ok(next);
        }
        r = next;
    }
    // Bisection on a bracket where the value changes sign.
    let (mut lo, mut hi) = (-0.999_999, 1.0);
    while f(lo).signum() == f(hi).signum() {
        hi *= 2.0;
        if hi > 1e6 {
            return Err(ErrorKind::Num);
        }
    }
    for _ in 0..300 {
        let mid = (lo + hi) / 2.0;
        if f(mid).signum() == f(lo).signum() {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-12 {
            break;
        }
    }
    Ok((lo + hi) / 2.0)
}

/// CAGR(beginning value, ending value, years)
fn cagr(args: &[Arg]) -> Result<f64, ErrorKind> {
    let (begin, end, years) = (args[0].number()?, args[1].number()?, args[2].number()?);
    if begin <= 0.0 || end < 0.0 || years <= 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok((end / begin).powf(1.0 / years) - 1.0)
}

/// GEOMEAN(number1, [number2], …)
fn geomean(args: &[Arg]) -> Result<f64, ErrorKind> {
    let xs = numbers(args)?;
    if xs.is_empty() || xs.iter().any(|x| *x <= 0.0) {
        return Err(ErrorKind::Num);
    }
    Ok((xs.iter().map(|x| x.ln()).sum::<f64>() / xs.len() as f64).exp())
}

export! {
    id: "xyz.lsuite.folio.finance-extra",
    name: "Finance extra",
    version: env!("CARGO_PKG_VERSION"),
    functions: [
        fn_def!("XNPV", "XNPV(rate, values, dates)", "Net present value of cash flows on irregular dates.", 3, 3, xnpv),
        fn_def!("XIRR", "XIRR(values, dates, [guess])", "Internal rate of return of cash flows on irregular dates.", 2, 3, xirr),
        fn_def!("CAGR", "CAGR(begin, end, years)", "Compound annual growth rate from a beginning to an ending value.", 3, 3, cagr),
        fn_def!("GEOMEAN", "GEOMEAN(number1, [number2], …)", "Geometric mean of positive numbers.", 1, 255, geomean),
    ],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(x: f64) -> Arg {
        Arg::Value(Value::Number(x))
    }

    fn col(xs: &[f64]) -> Arg {
        Arg::Range { rows: xs.len() as u32, cols: 1, values: xs.iter().map(|x| Value::Number(*x)).collect() }
    }

    #[test]
    fn the_functions_match_excel() {
        assert!((geomean(&[n(2.0), n(8.0)]).unwrap() - 4.0).abs() < 1e-12);
        assert!((cagr(&[n(100.0), n(121.0), n(2.0)]).unwrap() - 0.1).abs() < 1e-12);
        // Excel's XIRR example: -10000 on 2008-01-01, then four returns: 0.373362535.
        let values = col(&[-10000.0, 2750.0, 4250.0, 3250.0, 2750.0]);
        let dates = col(&[39448.0, 39508.0, 39751.0, 39859.0, 39904.0]);
        let r = xirr(&[values.clone(), dates.clone()]).unwrap();
        assert!((r - 0.373362535).abs() < 1e-6, "{r}");
        let v = xnpv(&[n(0.09), values, dates]).unwrap();
        assert!((v - 2086.647602).abs() < 1e-3, "{v}");
        assert_eq!(cagr(&[n(0.0), n(1.0), n(1.0)]), Err(ErrorKind::Num));
    }
}
