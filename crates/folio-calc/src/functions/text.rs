//! Text functions. Positions and lengths count characters (Unicode scalar values).

use super::criteria::wildcard_eq;
use super::{Builtin, Imp, MANY, def, opt_num};
use crate::eval::{Ctx, Ev, R, to_bool, to_num, to_text};
use crate::format::format_value;
use crate::input::text_to_number;
use crate::value::{ErrorKind, Value};

const T: &str = "Text";

pub(crate) const FUNCTIONS: &[Builtin] = &[
    def(
        "TEXT",
        T,
        "TEXT(value, format)",
        "A number shown with a number format, such as \"0.00\" or \"yyyy-mm-dd\".",
        2,
        2,
        Imp::Scalar(text),
    ),
    def("CONCAT", T, "CONCAT(text1, [text2], …)", "Joins texts and the cells of ranges.", 1, MANY, Imp::Eager(concat)),
    def("CONCATENATE", T, "CONCATENATE(text1, [text2], …)", "Joins texts.", 1, MANY, Imp::Scalar(concatenate)),
    def(
        "TEXTJOIN",
        T,
        "TEXTJOIN(delimiter, ignore_empty, text1, [text2], …)",
        "Joins texts and ranges with a delimiter.",
        3,
        MANY,
        Imp::Eager(textjoin),
    ),
    def("LEFT", T, "LEFT(text, [count])", "The first characters of a text.", 1, 2, Imp::Scalar(left)),
    def("RIGHT", T, "RIGHT(text, [count])", "The last characters of a text.", 1, 2, Imp::Scalar(right)),
    def("MID", T, "MID(text, start, count)", "Characters from the middle of a text.", 3, 3, Imp::Scalar(mid)),
    def("LEN", T, "LEN(text)", "The number of characters.", 1, 1, Imp::Scalar(len)),
    def("LOWER", T, "LOWER(text)", "The text in lowercase.", 1, 1, Imp::Scalar(lower)),
    def("UPPER", T, "UPPER(text)", "The text in uppercase.", 1, 1, Imp::Scalar(upper)),
    def("PROPER", T, "PROPER(text)", "The text with each word capitalized.", 1, 1, Imp::Scalar(proper)),
    def("TRIM", T, "TRIM(text)", "The text without leading, trailing and repeated spaces.", 1, 1, Imp::Scalar(trim)),
    def(
        "SUBSTITUTE",
        T,
        "SUBSTITUTE(text, old_text, new_text, [instance])",
        "Replaces occurrences of a text (all, or only the given one).",
        3,
        4,
        Imp::Scalar(substitute),
    ),
    def(
        "REPLACE",
        T,
        "REPLACE(text, start, count, new_text)",
        "Replaces characters at a position.",
        4,
        4,
        Imp::Scalar(replace),
    ),
    def(
        "FIND",
        T,
        "FIND(find_text, within_text, [start])",
        "The position of a text inside another (case-sensitive).",
        2,
        3,
        Imp::Scalar(find),
    ),
    def(
        "SEARCH",
        T,
        "SEARCH(find_text, within_text, [start])",
        "The position of a text inside another (any case, wildcards).",
        2,
        3,
        Imp::Scalar(search),
    ),
    def(
        "VALUE",
        T,
        "VALUE(text)",
        "The number a text stands for (numbers, percents, dates, times).",
        1,
        1,
        Imp::Scalar(value),
    ),
    def("REPT", T, "REPT(text, times)", "A text repeated.", 2, 2, Imp::Scalar(rept)),
    def(
        "EXACT",
        T,
        "EXACT(text1, text2)",
        "TRUE when two texts are identical, case included.",
        2,
        2,
        Imp::Scalar(exact),
    ),
    def(
        "CHAR",
        T,
        "CHAR(number)",
        "The character with a code (1 to 255, or any Unicode code point).",
        1,
        1,
        Imp::Scalar(char_),
    ),
    def("CODE", T, "CODE(text)", "The code of the first character.", 1, 1, Imp::Scalar(code)),
];

fn text(_: &Ctx, args: &[Value]) -> R<Value> {
    let format = to_text(&args[1])?;
    let v = match &args[0] {
        Value::Error(e) => return Err(*e),
        Value::Text(s) => match text_to_number(s) {
            Some(n) => Value::Number(n),
            None => Value::Text(s.clone()),
        },
        Value::Empty => Value::Number(0.0),
        other => other.clone(),
    };
    Ok(Value::Text(format_value(&v, Some(&format))))
}

fn concat(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let mut out = String::new();
    for arg in args {
        for v in ctx.to_array(arg).data {
            out.push_str(&to_text(&v)?);
        }
    }
    Ok(Ev::V(Value::Text(out)))
}

fn concatenate(_: &Ctx, args: &[Value]) -> R<Value> {
    let mut out = String::new();
    for v in args {
        out.push_str(&to_text(v)?);
    }
    Ok(Value::Text(out))
}

fn textjoin(ctx: &Ctx, args: Vec<Ev>) -> R<Ev> {
    let delimiter = to_text(&ctx.scalar(&args[0]))?;
    let ignore_empty = to_bool(&ctx.scalar(&args[1]))?;
    let mut parts = Vec::new();
    for arg in args.into_iter().skip(2) {
        for v in ctx.to_array(arg).data {
            let s = to_text(&v)?;
            if !(ignore_empty && s.is_empty()) {
                parts.push(s);
            }
        }
    }
    Ok(Ev::V(Value::Text(parts.join(&delimiter))))
}

fn count_arg(args: &[Value], i: usize, default: f64) -> R<usize> {
    let n = opt_num(args, i, default)?.trunc();
    if n < 0.0 {
        return Err(ErrorKind::Value);
    }
    Ok(n as usize)
}

fn left(_: &Ctx, args: &[Value]) -> R<Value> {
    let s = to_text(&args[0])?;
    let n = count_arg(args, 1, 1.0)?;
    Ok(Value::Text(s.chars().take(n).collect()))
}

fn right(_: &Ctx, args: &[Value]) -> R<Value> {
    let s = to_text(&args[0])?;
    let n = count_arg(args, 1, 1.0)?;
    let len = s.chars().count();
    Ok(Value::Text(s.chars().skip(len.saturating_sub(n)).collect()))
}

fn mid(_: &Ctx, args: &[Value]) -> R<Value> {
    let s = to_text(&args[0])?;
    let start = to_num(&args[1])?.trunc();
    let n = count_arg(args, 2, 0.0)?;
    if start < 1.0 {
        return Err(ErrorKind::Value);
    }
    Ok(Value::Text(s.chars().skip(start as usize - 1).take(n).collect()))
}

fn len(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Number(to_text(&args[0])?.chars().count() as f64))
}

fn lower(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Text(to_text(&args[0])?.to_lowercase()))
}

fn upper(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Text(to_text(&args[0])?.to_uppercase()))
}

fn proper(_: &Ctx, args: &[Value]) -> R<Value> {
    let mut out = String::new();
    let mut after_letter = false;
    for c in to_text(&args[0])?.chars() {
        if c.is_alphabetic() {
            if after_letter {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            after_letter = true;
        } else {
            out.push(c);
            after_letter = false;
        }
    }
    Ok(Value::Text(out))
}

fn trim(_: &Ctx, args: &[Value]) -> R<Value> {
    let s = to_text(&args[0])?;
    Ok(Value::Text(s.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")))
}

fn substitute(_: &Ctx, args: &[Value]) -> R<Value> {
    let s = to_text(&args[0])?;
    let old = to_text(&args[1])?;
    let new = to_text(&args[2])?;
    if old.is_empty() {
        return Ok(Value::Text(s));
    }
    match args.get(3) {
        None => Ok(Value::Text(s.replace(&old, &new))),
        Some(v) => {
            let n = to_num(v)?.trunc();
            if n < 1.0 {
                return Err(ErrorKind::Value);
            }
            match s.match_indices(&old).nth(n as usize - 1) {
                Some((i, _)) => Ok(Value::Text(format!("{}{}{}", &s[..i], new, &s[i + old.len()..]))),
                None => Ok(Value::Text(s)),
            }
        }
    }
}

fn replace(_: &Ctx, args: &[Value]) -> R<Value> {
    let s: Vec<char> = to_text(&args[0])?.chars().collect();
    let start = to_num(&args[1])?.trunc();
    let n = count_arg(args, 2, 0.0)?;
    let new = to_text(&args[3])?;
    if start < 1.0 {
        return Err(ErrorKind::Value);
    }
    let a = (start as usize - 1).min(s.len());
    let b = (a + n).min(s.len());
    let mut out: String = s[..a].iter().collect();
    out.push_str(&new);
    out.extend(&s[b..]);
    Ok(Value::Text(out))
}

/// Shared by FIND and SEARCH: the 1-based character position of `needle` from `start`.
fn position(args: &[Value], matches: &dyn Fn(&[char], &[char]) -> Option<usize>) -> R<Value> {
    let needle: Vec<char> = to_text(&args[0])?.chars().collect();
    let hay: Vec<char> = to_text(&args[1])?.chars().collect();
    let start = opt_num(args, 2, 1.0)?.trunc();
    if start < 1.0 || start as usize > hay.len() + 1 {
        return Err(ErrorKind::Value);
    }
    let from = start as usize - 1;
    match matches(&needle, &hay[from..]) {
        Some(i) => Ok(Value::Number((from + i + 1) as f64)),
        None => Err(ErrorKind::Value),
    }
}

fn find(_: &Ctx, args: &[Value]) -> R<Value> {
    position(args, &|needle, hay| {
        if needle.is_empty() {
            return Some(0);
        }
        hay.windows(needle.len()).position(|w| w == needle)
    })
}

fn search(_: &Ctx, args: &[Value]) -> R<Value> {
    position(args, &|needle, hay| {
        if needle.is_empty() {
            return Some(0);
        }
        let pattern: String = needle.iter().collect();
        let wild = pattern.contains(['*', '?', '~']);
        let pattern = if wild { format!("{pattern}*") } else { pattern.to_lowercase() };
        (0..hay.len()).find(|&i| {
            let rest: String = hay[i..].iter().collect();
            if wild { wildcard_eq(&pattern, &rest) } else { rest.to_lowercase().starts_with(&pattern) }
        })
    })
}

fn value(_: &Ctx, args: &[Value]) -> R<Value> {
    match &args[0] {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::Empty => Ok(Value::Number(0.0)),
        Value::Text(s) => text_to_number(s).map(Value::Number).ok_or(ErrorKind::Value),
        Value::Bool(_) => Err(ErrorKind::Value),
        Value::Error(e) => Err(*e),
    }
}

fn rept(_: &Ctx, args: &[Value]) -> R<Value> {
    let s = to_text(&args[0])?;
    let n = to_num(&args[1])?.trunc();
    if n < 0.0 || s.len() as f64 * n > 32_767.0 {
        return Err(ErrorKind::Value);
    }
    Ok(Value::Text(s.repeat(n as usize)))
}

fn exact(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(to_text(&args[0])? == to_text(&args[1])?))
}

fn char_(_: &Ctx, args: &[Value]) -> R<Value> {
    let n = to_num(&args[0])?.trunc();
    if n < 1.0 {
        return Err(ErrorKind::Value);
    }
    char::from_u32(n as u32).map(|c| Value::Text(c.to_string())).ok_or(ErrorKind::Value)
}

fn code(_: &Ctx, args: &[Value]) -> R<Value> {
    let s = to_text(&args[0])?;
    s.chars().next().map(|c| Value::Number(c as u32 as f64)).ok_or(ErrorKind::Value)
}
