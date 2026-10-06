//! Statistical functions, and the criteria family (SUMIF, COUNTIFS, MAXIFS…).

use super::criteria::Criterion;
use super::{Builtin, Imp, MANY, collect_numbers, def, num_arg, numbers};
use crate::eval::{Ctx, Ev, Grid, R, num_cmp, to_num};
use crate::value::{ErrorKind, Value};

const S: &str = "Statistical";
const M: &str = "Math";

pub(crate) const FUNCTIONS: &[Builtin] = &[
    def("AVERAGE", S, "AVERAGE(number1, [number2], …)", "The arithmetic mean of numbers.", 1, MANY, Imp::Eager(average)),
    def("COUNT", S, "COUNT(value1, [value2], …)", "Counts the numbers.", 1, MANY, Imp::Eager(count)),
    def("COUNTA", S, "COUNTA(value1, [value2], …)", "Counts the cells that are not empty.", 1, MANY, Imp::Eager(counta)),
    def("COUNTBLANK", S, "COUNTBLANK(range)", "Counts empty cells (and empty text) in a range.", 1, 1, Imp::Eager(countblank)),
    def("MIN", S, "MIN(number1, [number2], …)", "The smallest number (0 when there is none).", 1, MANY, Imp::Eager(min)),
    def("MAX", S, "MAX(number1, [number2], …)", "The largest number (0 when there is none).", 1, MANY, Imp::Eager(max)),
    def("MEDIAN", S, "MEDIAN(number1, [number2], …)", "The middle number.", 1, MANY, Imp::Eager(median)),
    def("MODE", S, "MODE(number1, [number2], …)", "The most frequent number.", 1, MANY, Imp::Eager(mode)),
    def("MODE.SNGL", S, "MODE.SNGL(number1, [number2], …)", "The most frequent number.", 1, MANY, Imp::Eager(mode)),
    def("STDEV", S, "STDEV(number1, [number2], …)", "Standard deviation of a sample.", 1, MANY, Imp::Eager(stdev_s)),
    def("STDEV.S", S, "STDEV.S(number1, [number2], …)", "Standard deviation of a sample.", 1, MANY, Imp::Eager(stdev_s)),
    def("STDEV.P", S, "STDEV.P(number1, [number2], …)", "Standard deviation of a whole population.", 1, MANY, Imp::Eager(stdev_p)),
    def("STDEVP", S, "STDEVP(number1, [number2], …)", "Standard deviation of a whole population.", 1, MANY, Imp::Eager(stdev_p)),
    def("VAR", S, "VAR(number1, [number2], …)", "Variance of a sample.", 1, MANY, Imp::Eager(var_s)),
    def("VAR.S", S, "VAR.S(number1, [number2], …)", "Variance of a sample.", 1, MANY, Imp::Eager(var_s)),
    def("VAR.P", S, "VAR.P(number1, [number2], …)", "Variance of a whole population.", 1, MANY, Imp::Eager(var_p)),
    def("VARP", S, "VARP(number1, [number2], …)", "Variance of a whole population.", 1, MANY, Imp::Eager(var_p)),
    def("LARGE", S, "LARGE(array, k)", "The k-th largest number.", 2, 2, Imp::Eager(large)),
    def("SMALL", S, "SMALL(array, k)", "The k-th smallest number.", 2, 2, Imp::Eager(small)),
    def("RANK", S, "RANK(number, ref, [order])", "The rank of a number in a list (0: largest first, 1: smallest first).", 2, 3, Imp::Eager(rank)),
    def("RANK.EQ", S, "RANK.EQ(number, ref, [order])", "The rank of a number in a list (0: largest first, 1: smallest first).", 2, 3, Imp::Eager(rank)),
    def("COUNTIF", S, "COUNTIF(range, criteria)", "Counts the cells that meet a criterion such as \">10\".", 2, 2, Imp::Eager(countif)),
    def("COUNTIFS", S, "COUNTIFS(criteria_range1, criteria1, …)", "Counts the cells that meet every criterion.", 2, MANY, Imp::Eager(countifs)),
    def("SUMIF", M, "SUMIF(range, criteria, [sum_range])", "Adds the cells that meet a criterion.", 2, 3, Imp::Eager(sumif)),
    def("SUMIFS", M, "SUMIFS(sum_range, criteria_range1, criteria1, …)", "Adds the cells that meet every criterion.", 3, MANY, Imp::Eager(sumifs)),
    def("AVERAGEIF", S, "AVERAGEIF(range, criteria, [average_range])", "The mean of the cells that meet a criterion.", 2, 3, Imp::Eager(averageif)),
    def("AVERAGEIFS", S, "AVERAGEIFS(average_range, criteria_range1, criteria1, …)", "The mean of the cells that meet every criterion.", 3, MANY, Imp::Eager(averageifs)),
    def("MINIFS", S, "MINIFS(min_range, criteria_range1, criteria1, …)", "The smallest of the cells that meet every criterion.", 3, MANY, Imp::Eager(minifs)),
    def("MAXIFS", S, "MAXIFS(max_range, criteria_range1, criteria1, …)", "The largest of the cells that meet every criterion.", 3, MANY, Imp::Eager(maxifs)),
];

fn average(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let (mut total, mut n) = (0.0, 0usize);
    numbers(ctx, &args, &mut |x| {
        total += x;
        n += 1;
    })?;
    if n == 0 {
        return Err(ErrorKind::Div0);
    }
    Ok(Ev::num(total / n as f64))
}

fn count(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut n = 0;
    for arg in &args {
        ctx.each_value(arg, &mut |v, in_range| match v {
            Value::Number(_) => n += 1,
            Value::Bool(_) if !in_range => n += 1,
            Value::Text(_) if !in_range && to_num(v).is_ok() => n += 1,
            _ => {}
        });
    }
    Ok(Ev::num(n as f64))
}

fn counta(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut n = 0;
    for arg in &args {
        ctx.each_value(arg, &mut |v, _| {
            if !v.is_empty() {
                n += 1;
            }
        });
    }
    Ok(Ev::num(n as f64))
}

fn countblank(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    match &args[0] {
        Ev::R { sheet, range } => {
            let mut filled = 0u64;
            ctx.each_in_range(*sheet, *range, &mut |_, v| {
                if !v.is_empty() && !matches!(v, Value::Text(s) if s.is_empty()) {
                    filled += 1;
                }
            });
            Ok(Ev::num((range.area() - filled) as f64))
        }
        other => {
            let mut n = 0;
            ctx.each_value(other, &mut |v, _| {
                if v.is_empty() || matches!(v, Value::Text(s) if s.is_empty()) {
                    n += 1;
                }
            });
            Ok(Ev::num(n as f64))
        }
    }
}

fn min(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut best: Option<f64> = None;
    numbers(ctx, &args, &mut |x| best = Some(best.map_or(x, |b| b.min(x))))?;
    Ok(Ev::num(best.unwrap_or(0.0)))
}

fn max(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut best: Option<f64> = None;
    numbers(ctx, &args, &mut |x| best = Some(best.map_or(x, |b| b.max(x))))?;
    Ok(Ev::num(best.unwrap_or(0.0)))
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.total_cmp(b));
    v
}

fn median(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let v = sorted(collect_numbers(ctx, &args)?);
    if v.is_empty() {
        return Err(ErrorKind::Num);
    }
    let mid = v.len() / 2;
    Ok(Ev::num(if v.len() % 2 == 1 { v[mid] } else { (v[mid - 1] + v[mid]) / 2.0 }))
}

/// Numbers in reading order (MODE returns the first of equally frequent numbers).
fn ordered_numbers(ctx: &Ctx, args: Vec<Ev>) -> R<Vec<f64>> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Ev::R { .. } => {
                for v in ctx.to_array(arg).data {
                    match v {
                        Value::Number(n) => out.push(n),
                        Value::Error(e) => return Err(e),
                        _ => {}
                    }
                }
            }
            other => out.extend(collect_numbers(ctx, &[other])?),
        }
    }
    Ok(out)
}

fn mode(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let v = ordered_numbers(ctx, args)?;
    let mut best: Option<(f64, usize)> = None;
    for (i, x) in v.iter().enumerate() {
        if v[..i].contains(x) {
            continue;
        }
        let n = v.iter().filter(|y| *y == x).count();
        if n > 1 && best.is_none_or(|(_, m)| n > m) {
            best = Some((*x, n));
        }
    }
    best.map(|(x, _)| Ev::num(x)).ok_or(ErrorKind::NA)
}

fn variance(ctx: &Ctx, args: &[Ev], sample: bool) -> R<f64> {
    let v = collect_numbers(ctx, args)?;
    let n = v.len() as f64;
    if v.is_empty() || (sample && v.len() < 2) {
        return Err(ErrorKind::Div0);
    }
    let mean = v.iter().sum::<f64>() / n;
    let ss: f64 = v.iter().map(|x| (x - mean) * (x - mean)).sum();
    Ok(ss / if sample { n - 1.0 } else { n })
}

fn stdev_s(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    Ok(Ev::num(variance(ctx, &args, true)?.sqrt()))
}

fn stdev_p(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    Ok(Ev::num(variance(ctx, &args, false)?.sqrt()))
}

fn var_s(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    Ok(Ev::num(variance(ctx, &args, true)?))
}

fn var_p(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    Ok(Ev::num(variance(ctx, &args, false)?))
}

fn kth(ctx: &Ctx, args: Vec<Ev>, largest: bool) -> R<Ev> {
    let k = num_arg(ctx, &args, 1, 1.0)?.ceil();
    let v = sorted(collect_numbers(ctx, &args[..1])?);
    if k < 1.0 || k as usize > v.len() {
        return Err(ErrorKind::Num);
    }
    let k = k as usize;
    Ok(Ev::num(if largest { v[v.len() - k] } else { v[k - 1] }))
}

fn large(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    kth(ctx, args, true)
}

fn small(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    kth(ctx, args, false)
}

fn rank(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let x = num_arg(ctx, &args, 0, 0.0)?;
    let ascending = num_arg(ctx, &args, 2, 0.0)? != 0.0;
    let v = collect_numbers(ctx, &args[1..2])?;
    if !v.iter().any(|y| num_cmp(*y, x).is_eq()) {
        return Err(ErrorKind::NA);
    }
    let before = v.iter().filter(|&&y| if ascending { num_cmp(y, x).is_lt() } else { num_cmp(y, x).is_gt() }).count();
    Ok(Ev::num(before as f64 + 1.0))
}

// ---------------------------------------------------------------------------------------------
// Criteria

/// Calls `f(r, c)` for every position where all `(range, criterion)` pairs match. The ranges
/// must all have the same size.
fn each_match(ctx: &Ctx, pairs: &[(&Ev, Criterion)], f: &mut dyn FnMut(usize, usize) -> R<()>) -> R<()> {
    let dims = pairs[0].0.dims();
    if pairs.iter().any(|(r, _)| r.dims() != dims) {
        return Err(ErrorKind::Value);
    }
    let grids: Vec<(Grid, &Criterion)> = pairs.iter().map(|(r, c)| (ctx.grid(r), c)).collect();
    let rows = grids.iter().map(|(g, _)| g.rows()).max().unwrap_or(0).min(dims.0);
    let cols = grids.iter().map(|(g, _)| g.cols()).max().unwrap_or(0).min(dims.1);
    for r in 0..rows {
        for c in 0..cols {
            if grids.iter().all(|(g, crit)| crit.matches(g.get(r, c))) {
                f(r, c)?;
            }
        }
    }
    Ok(())
}

/// The `(range, criterion)` pairs of a COUNTIFS-style argument list.
fn pairs<'a>(ctx: &Ctx, args: &'a [Ev]) -> R<Vec<(&'a Ev, Criterion)>> {
    if args.is_empty() || args.len() % 2 != 0 {
        return Err(ErrorKind::Value);
    }
    Ok(args.chunks(2).map(|p| (&p[0], Criterion::new(&ctx.scalar(&p[1])))).collect())
}

/// Folds the numbers of `target` at the positions matching all criteria.
fn fold_matches(ctx: &Ctx, target: &Ev, pairs: &[(&Ev, Criterion)], f: &mut dyn FnMut(f64)) -> R<()> {
    let grid = ctx.grid(target);
    each_match(ctx, pairs, &mut |r, c| match grid.get(r, c) {
        Value::Number(n) => {
            f(*n);
            Ok(())
        }
        Value::Error(e) => Err(*e),
        _ => Ok(()),
    })
}

fn countif(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    countifs(ctx, args)
}

fn countifs(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let pairs = pairs(ctx, &args)?;
    let mut n = 0;
    each_match(ctx, &pairs, &mut |_, _| {
        n += 1;
        Ok(())
    })?;
    Ok(Ev::num(n as f64))
}

/// SUMIF-style arguments: (range, criteria, [target]) where a target range takes the size of
/// the criteria range from its own top-left cell.
fn single(ctx: &Ctx, args: &[Ev]) -> (Ev, Criterion) {
    let crit = Criterion::new(&ctx.scalar(&args[1]));
    let target = match args.get(2) {
        Some(Ev::R { sheet, range }) => {
            let (rows, cols) = args[0].dims();
            let end = crate::addr::Addr::new(
                (range.start.row as usize + rows - 1).min(crate::addr::MAX_ROWS as usize - 1) as u32,
                (range.start.col as usize + cols - 1).min(crate::addr::MAX_COLS as usize - 1) as u32,
            );
            Ev::R { sheet: *sheet, range: crate::addr::Range { start: range.start, end } }
        }
        Some(Ev::V(Value::Empty)) | None => args[0].clone(),
        Some(other) => other.clone(),
    };
    (target, crit)
}

fn sumif(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let (target, crit) = single(ctx, &args);
    let mut total = 0.0;
    let pairs = [(&args[0], crit)];
    if target.dims() != args[0].dims() {
        return Err(ErrorKind::Value);
    }
    fold_matches(ctx, &target, &pairs, &mut |x| total += x)?;
    Ok(Ev::num(total))
}

fn averageif(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let (target, crit) = single(ctx, &args);
    if target.dims() != args[0].dims() {
        return Err(ErrorKind::Value);
    }
    let (mut total, mut n) = (0.0, 0);
    fold_matches(ctx, &target, &[(&args[0], crit)], &mut |x| {
        total += x;
        n += 1;
    })?;
    if n == 0 {
        return Err(ErrorKind::Div0);
    }
    Ok(Ev::num(total / n as f64))
}

/// The target range and criteria pairs of SUMIFS-style functions.
fn multi(ctx: &Ctx, args: &[Ev], f: &mut dyn FnMut(f64)) -> R<()> {
    let pairs = pairs(ctx, &args[1..])?;
    if args[0].dims() != pairs[0].0.dims() {
        return Err(ErrorKind::Value);
    }
    fold_matches(ctx, &args[0], &pairs, f)
}

fn sumifs(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut total = 0.0;
    multi(ctx, &args, &mut |x| total += x)?;
    Ok(Ev::num(total))
}

fn averageifs(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let (mut total, mut n) = (0.0, 0);
    multi(ctx, &args, &mut |x| {
        total += x;
        n += 1;
    })?;
    if n == 0 {
        return Err(ErrorKind::Div0);
    }
    Ok(Ev::num(total / n as f64))
}

fn minifs(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut best: Option<f64> = None;
    multi(ctx, &args, &mut |x| best = Some(best.map_or(x, |b| b.min(x))))?;
    Ok(Ev::num(best.unwrap_or(0.0)))
}

fn maxifs(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut best: Option<f64> = None;
    multi(ctx, &args, &mut |x| best = Some(best.map_or(x, |b| b.max(x))))?;
    Ok(Ev::num(best.unwrap_or(0.0)))
}
