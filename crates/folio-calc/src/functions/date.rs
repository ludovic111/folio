//! Date and time functions, on Excel serials (see [`crate::format`]).

use super::{Builtin, Imp, collect_numbers, def, num_arg, opt_num};
use crate::eval::{Ctx, Ev, R, num_value, to_num, to_text};
use crate::format::{date_to_serial, days_in_month, serial_to_date, weekday0};
use crate::input::text_to_number;
use crate::value::{ErrorKind, Value};

const D: &str = "Date";

pub(crate) const FUNCTIONS: &[Builtin] = &[
    def("DATE", D, "DATE(year, month, day)", "The date serial of a year, month and day (months and days past the end carry over).", 3, 3, Imp::Scalar(date)),
    def("TODAY", D, "TODAY()", "Today's date, updated on every recalculation.", 0, 0, Imp::Scalar(today)),
    def("NOW", D, "NOW()", "The current date and time, updated on every recalculation.", 0, 0, Imp::Scalar(now)),
    def("YEAR", D, "YEAR(date)", "The year of a date.", 1, 1, Imp::Scalar(year)),
    def("MONTH", D, "MONTH(date)", "The month of a date, 1 to 12.", 1, 1, Imp::Scalar(month)),
    def("DAY", D, "DAY(date)", "The day of the month, 1 to 31.", 1, 1, Imp::Scalar(day)),
    def("WEEKDAY", D, "WEEKDAY(date, [type])", "The day of the week (type 1: Sunday is 1; 2: Monday is 1; 3: Monday is 0).", 1, 2, Imp::Scalar(weekday)),
    def("WEEKNUM", D, "WEEKNUM(date, [type])", "The week of the year (type 1: weeks start on Sunday; 2: Monday; 21: ISO weeks).", 1, 2, Imp::Scalar(weeknum)),
    def("EDATE", D, "EDATE(start_date, months)", "The same day a number of months later or earlier.", 2, 2, Imp::Scalar(edate)),
    def("EOMONTH", D, "EOMONTH(start_date, months)", "The last day of the month a number of months later or earlier.", 2, 2, Imp::Scalar(eomonth)),
    def("DATEDIF", D, "DATEDIF(start_date, end_date, unit)", "The time between two dates in \"Y\", \"M\", \"D\", \"MD\", \"YM\" or \"YD\".", 3, 3, Imp::Scalar(datedif)),
    def("DAYS", D, "DAYS(end_date, start_date)", "The number of days between two dates.", 2, 2, Imp::Scalar(days)),
    def("HOUR", D, "HOUR(time)", "The hour, 0 to 23.", 1, 1, Imp::Scalar(hour)),
    def("MINUTE", D, "MINUTE(time)", "The minute, 0 to 59.", 1, 1, Imp::Scalar(minute)),
    def("SECOND", D, "SECOND(time)", "The second, 0 to 59.", 1, 1, Imp::Scalar(second)),
    def("TIME", D, "TIME(hour, minute, second)", "The fraction of a day for a time.", 3, 3, Imp::Scalar(time)),
    def("NETWORKDAYS", D, "NETWORKDAYS(start_date, end_date, [holidays])", "The number of working days (Monday to Friday) between two dates, both included.", 2, 3, Imp::Eager(networkdays)),
    def("WORKDAY", D, "WORKDAY(start_date, days, [holidays])", "The date a number of working days after (or before) a date.", 2, 3, Imp::Eager(workday)),
    def("DATEVALUE", D, "DATEVALUE(date_text)", "The date serial of a date written as text.", 1, 1, Imp::Scalar(datevalue)),
];

/// A date argument: a non-negative serial (text dates are read too).
fn serial(v: &Value) -> R<f64> {
    let n = to_num(v)?;
    if !(0.0..2_958_466.0).contains(&n) {
        return Err(ErrorKind::Num);
    }
    Ok(n)
}

fn ymd(v: &Value) -> R<(i64, u32, u32)> {
    let (y, m, d) = serial_to_date(serial(v)?);
    Ok((y as i64, m, d))
}

/// The serial of year/month/day where the month may be outside 1–12 and the day may overflow.
fn make_date(y: i64, m: i64, d: i64) -> R<f64> {
    let total = y * 12 + (m - 1);
    let (yy, mm) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    if !(0..=9999).contains(&yy) {
        return Err(ErrorKind::Num);
    }
    let s = date_to_serial(yy as i32, mm, 1) + (d - 1) as f64;
    if !(0.0..2_958_466.0).contains(&s) {
        return Err(ErrorKind::Num);
    }
    Ok(s)
}

fn date(_: &Ctx, args: &[Value]) -> R<Value> {
    let mut y = to_num(&args[0])?.trunc() as i64;
    let m = to_num(&args[1])?.trunc() as i64;
    let d = to_num(&args[2])?.trunc() as i64;
    if !(0..10_000).contains(&y) {
        return Err(ErrorKind::Num);
    }
    if y < 1900 {
        y += 1900;
    }
    Ok(Value::Number(make_date(y, m, d)?))
}

fn today(ctx: &Ctx, _: &[Value]) -> R<Value> {
    Ok(Value::Number(ctx.now.floor()))
}

fn now(ctx: &Ctx, _: &[Value]) -> R<Value> {
    Ok(Value::Number(ctx.now))
}

fn year(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number(ymd(&args[0])?.0 as f64))
}

fn month(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number(ymd(&args[0])?.1 as f64))
}

fn day(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number(ymd(&args[0])?.2 as f64))
}

fn weekday(_: &Ctx, args: &[Value]) -> R<Value> {
    let wd = weekday0(serial(&args[0])?);
    let kind = opt_num(args, 1, 1.0)?.trunc() as i64;
    let n = match kind {
        1 | 17 => wd + 1,
        2 | 11 => (wd + 6) % 7 + 1,
        3 => (wd + 6) % 7,
        12..=16 => (wd + 7 - (kind as u32 - 10)) % 7 + 1,
        _ => return Err(ErrorKind::Num),
    };
    Ok(Value::Number(n as f64))
}

pub(crate) fn iso_week(s: f64) -> u32 {
    let s = s.floor();
    let monday_based = ((weekday0(s) + 6) % 7) as f64;
    let thursday = s - monday_based + 3.0;
    let (ty, _, _) = serial_to_date(thursday);
    let jan1 = date_to_serial(ty, 1, 1);
    ((thursday - jan1) / 7.0).floor() as u32 + 1
}

fn weeknum(_: &Ctx, args: &[Value]) -> R<Value> {
    let s = serial(&args[0])?.floor();
    let kind = opt_num(args, 1, 1.0)?.trunc() as i64;
    let start = match kind {
        1 | 17 => 0,
        2 | 11 => 1,
        12..=16 => (kind - 10) as u32,
        21 => return Ok(Value::Number(iso_week(s) as f64)),
        _ => return Err(ErrorKind::Num),
    };
    let (y, _, _) = serial_to_date(s);
    let jan1 = date_to_serial(y, 1, 1);
    let offset = ((weekday0(jan1) + 7 - start) % 7) as f64;
    Ok(Value::Number(((s - jan1 + offset) / 7.0).floor() + 1.0))
}

fn add_months(v: &Value, months: &Value, end_of_month: bool) -> R<f64> {
    let (y, m, d) = ymd(v)?;
    let months = to_num(months)?.trunc() as i64;
    let total = y * 12 + (m as i64 - 1) + months;
    let (yy, mm) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    if !(1900..=9999).contains(&yy) {
        return Err(ErrorKind::Num);
    }
    let last = days_in_month(yy, mm);
    let day = if end_of_month { last } else { d.min(last) };
    Ok(date_to_serial(yy as i32, mm, day))
}

fn edate(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number(add_months(&args[0], &args[1], false)?))
}

fn eomonth(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number(add_months(&args[0], &args[1], true)?))
}

fn datedif(_: &Ctx, args: &[Value]) -> R<Value> {
    let (s, e) = (serial(&args[0])?.floor(), serial(&args[1])?.floor());
    if s > e {
        return Err(ErrorKind::Num);
    }
    let unit = to_text(&args[2])?.to_ascii_uppercase();
    let (sy, sm, sd) = serial_to_date(s);
    let (ey, em, ed) = serial_to_date(e);
    let (sy, ey) = (sy as i64, ey as i64);
    let months = (ey - sy) * 12 + em as i64 - sm as i64 - i64::from(ed < sd);
    let n = match unit.as_str() {
        "D" => e - s,
        "M" => months as f64,
        "Y" => (months / 12) as f64,
        "YM" => (months % 12) as f64,
        "MD" => {
            if ed >= sd {
                (ed - sd) as f64
            } else {
                let (py, pm) = if em == 1 { (ey - 1, 12) } else { (ey, em - 1) };
                (days_in_month(py, pm) as i64 - sd as i64 + ed as i64).max(0) as f64
            }
        }
        "YD" => {
            let mut anniversary = make_date(ey, sm as i64, sd as i64)?;
            if anniversary > e {
                anniversary = make_date(ey - 1, sm as i64, sd as i64)?;
            }
            e - anniversary
        }
        _ => return Err(ErrorKind::Num),
    };
    Ok(Value::Number(n))
}

fn days(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number(serial(&args[0])?.trunc() - serial(&args[1])?.trunc()))
}

/// Seconds since midnight, rounded to the nearest second.
fn seconds_of_day(v: &Value) -> R<i64> {
    let n = to_num(v)?;
    if n < 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(((n - n.floor()) * 86_400.0).round() as i64 % 86_400)
}

fn hour(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number((seconds_of_day(&args[0])? / 3600) as f64))
}

fn minute(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number((seconds_of_day(&args[0])? / 60 % 60) as f64))
}

fn second(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number((seconds_of_day(&args[0])? % 60) as f64))
}

fn time(_: &Ctx, args: &[Value]) -> R<Value> {
    let h = to_num(&args[0])?.trunc();
    let m = to_num(&args[1])?.trunc();
    let s = to_num(&args[2])?.trunc();
    let total = h * 3600.0 + m * 60.0 + s;
    if total < 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(num_value(total.rem_euclid(86_400.0) / 86_400.0))
}

fn holidays(ctx: &Ctx, args: &[Ev]) -> R<Vec<f64>> {
    match args.get(2) {
        None | Some(Ev::V(Value::Empty)) => Ok(Vec::new()),
        Some(h) => {
            let mut v: Vec<f64> = collect_numbers(ctx, std::slice::from_ref(h))?.into_iter().map(f64::floor).collect();
            v.sort_by(f64::total_cmp);
            v.dedup();
            Ok(v)
        }
    }
}

fn is_workday(s: f64, holidays: &[f64]) -> bool {
    let wd = weekday0(s);
    wd != 0 && wd != 6 && holidays.binary_search_by(|h| h.total_cmp(&s)).is_err()
}

fn networkdays(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let a = serial(&ctx.scalar(&args[0]))?.floor();
    let b = serial(&ctx.scalar(&args[1]))?.floor();
    let hol = holidays(ctx, &args)?;
    let (lo, hi, sign) = if a <= b { (a, b, 1.0) } else { (b, a, -1.0) };
    let total = (hi - lo) as i64 + 1;
    let full_weeks = total / 7;
    let mut n = full_weeks * 5;
    let mut s = lo + (full_weeks * 7) as f64;
    while s <= hi {
        if is_workday(s, &[]) {
            n += 1;
        }
        s += 1.0;
    }
    let off = hol.iter().filter(|&&h| h >= lo && h <= hi && is_workday(h, &[])).count() as i64;
    Ok(Ev::num(sign * (n - off) as f64))
}

fn workday(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut s = serial(&ctx.scalar(&args[0]))?.floor();
    let mut days = num_arg(ctx, &args, 1, 0.0)?.trunc();
    let hol = holidays(ctx, &args)?;
    let step = if days < 0.0 { -1.0 } else { 1.0 };
    while days != 0.0 {
        s += step;
        if !(1.0..2_958_466.0).contains(&s) {
            return Err(ErrorKind::Num);
        }
        if is_workday(s, &hol) {
            days -= step;
        }
    }
    Ok(Ev::num(s))
}

fn datevalue(_: &Ctx, args: &[Value]) -> R<Value> {
    match &args[0] {
        Value::Text(s) => text_to_number(s).map(|n| Value::Number(n.floor())).ok_or(ErrorKind::Value),
        Value::Error(e) => Err(*e),
        _ => Err(ErrorKind::Value),
    }
}
