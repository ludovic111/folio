//! Criteria for SUMIF, COUNTIFS and friends: `">10"`, `"<>done"`, `"=*draft*"`, `"apple"`.

use std::cmp::Ordering;

use crate::eval::{num_cmp, text_cmp};
use crate::input::text_to_number;
use crate::value::{ErrorKind, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

impl Op {
    fn test(self, ord: Ordering) -> bool {
        match self {
            Op::Eq => ord == Ordering::Equal,
            Op::Ne => ord != Ordering::Equal,
            Op::Lt => ord == Ordering::Less,
            Op::Gt => ord == Ordering::Greater,
            Op::Le => ord != Ordering::Greater,
            Op::Ge => ord != Ordering::Less,
        }
    }
}

#[derive(Clone, Debug)]
enum Target {
    Number(f64),
    /// Lowercase text, with its wildcard pattern when it has `*` or `?`.
    Text(String, Option<Vec<Pat>>),
    Bool(bool),
    Error(ErrorKind),
    /// `"="` (empty cells) or `"<>"` (non-empty cells).
    Blank,
    /// `""` or an empty criteria cell: empty cells and empty text.
    EmptyText,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pat {
    Any,
    One,
    Char(char),
}

/// One parsed criterion.
#[derive(Clone, Debug)]
pub(crate) struct Criterion {
    op: Op,
    target: Target,
}

impl Criterion {
    /// A criterion from a value: numbers, booleans and errors match equal values; text is
    /// parsed for an operator.
    pub fn new(v: &Value) -> Self {
        let eq = |target| Criterion { op: Op::Eq, target };
        match v {
            Value::Number(n) => eq(Target::Number(*n)),
            Value::Bool(b) => eq(Target::Bool(*b)),
            Value::Error(e) => eq(Target::Error(*e)),
            Value::Empty => eq(Target::EmptyText),
            Value::Text(s) => Self::parse(s),
        }
    }

    /// Reads a criteria string: an optional operator (`= <> < > <= >=`) then a number, a date,
    /// TRUE/FALSE, an error code or text (with `*`, `?` and `~` wildcards for `=` and `<>`).
    pub fn parse(s: &str) -> Self {
        let (op, rest) = [("<=", Op::Le), (">=", Op::Ge), ("<>", Op::Ne), ("<", Op::Lt), (">", Op::Gt), ("=", Op::Eq)]
            .iter()
            .find_map(|(p, op)| s.strip_prefix(p).map(|rest| (Some(*op), rest)))
            .unwrap_or((None, s));
        if rest.is_empty() {
            return match op {
                None => Criterion { op: Op::Eq, target: Target::EmptyText },
                Some(Op::Eq) => Criterion { op: Op::Eq, target: Target::Blank },
                Some(Op::Ne) => Criterion { op: Op::Ne, target: Target::Blank },
                Some(op) => Criterion { op, target: Target::Text(String::new(), None) },
            };
        }
        let op = op.unwrap_or(Op::Eq);
        let target = if let Some(n) = text_to_number(rest) {
            Target::Number(n)
        } else if rest.eq_ignore_ascii_case("TRUE") {
            Target::Bool(true)
        } else if rest.eq_ignore_ascii_case("FALSE") {
            Target::Bool(false)
        } else if let Some(e) = ErrorKind::parse(rest) {
            Target::Error(e)
        } else {
            let lower = rest.to_lowercase();
            let pattern = matches!(op, Op::Eq | Op::Ne).then(|| compile(&lower)).flatten();
            Target::Text(lower, pattern)
        };
        Criterion { op, target }
    }

    pub fn matches(&self, v: &Value) -> bool {
        let op = self.op;
        match &self.target {
            Target::Blank => (op == Op::Eq) == v.is_empty(),
            Target::EmptyText => matches!(v, Value::Empty) || matches!(v, Value::Text(s) if s.is_empty()),
            Target::Number(n) => match v {
                Value::Number(x) => op.test(num_cmp(*x, *n)),
                _ => op == Op::Ne,
            },
            Target::Bool(b) => match v {
                Value::Bool(x) => op.test(x.cmp(b)),
                _ => op == Op::Ne,
            },
            Target::Error(e) => match v {
                Value::Error(x) => op.test(if x == e { Ordering::Equal } else { Ordering::Less }),
                _ => op == Op::Ne,
            },
            Target::Text(t, pattern) => match v {
                Value::Text(s) => match (op, pattern) {
                    (Op::Eq | Op::Ne, Some(p)) => {
                        let chars: Vec<char> = s.to_lowercase().chars().collect();
                        (op == Op::Eq) == glob(p, &chars)
                    }
                    _ => op.test(text_cmp(s, t)),
                },
                Value::Empty => op == Op::Ne || (t.is_empty() && op.test(Ordering::Equal)),
                _ => op == Op::Ne,
            },
        }
    }
}

/// True when `v` meets a criteria string, with the same rules as COUNTIF (`">100"`, `"<>done"`,
/// `"=*draft*"`, `"apple"`; wildcards `*` and `?`, `~` escapes them; text compares without case;
/// `"="` matches empty cells and `"<>"` non-empty ones).
pub fn matches_criteria(v: &Value, criteria: &str) -> bool {
    Criterion::parse(criteria).matches(v)
}

/// Compiles a wildcard pattern; `None` when it has no wildcards (plain comparison is faster).
fn compile(s: &str) -> Option<Vec<Pat>> {
    if !s.contains(['*', '?', '~']) {
        return None;
    }
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '*' => out.push(Pat::Any),
            '?' => out.push(Pat::One),
            '~' => out.push(Pat::Char(chars.next().unwrap_or('~'))),
            c => out.push(Pat::Char(c)),
        }
    }
    Some(out)
}

/// Matches text against a compiled pattern (both lowercase).
fn glob(p: &[Pat], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        match p.get(pi) {
            Some(Pat::Any) => {
                star = Some((pi, si));
                pi += 1;
            }
            Some(Pat::One) => {
                pi += 1;
                si += 1;
            }
            Some(Pat::Char(c)) if *c == s[si] => {
                pi += 1;
                si += 1;
            }
            _ => match star {
                Some((sp, ss)) => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|x| *x == Pat::Any)
}

/// Wildcard text equality for lookups (MATCH, VLOOKUP and XLOOKUP in wildcard mode).
pub(crate) fn wildcard_eq(pattern: &str, text: &str) -> bool {
    let p = pattern.to_lowercase();
    match compile(&p) {
        Some(pat) => glob(&pat, &text.to_lowercase().chars().collect::<Vec<_>>()),
        None => text_cmp(pattern, text) == Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: Value, c: &str) -> bool {
        matches_criteria(&v, c)
    }

    #[test]
    fn criteria() {
        let n = Value::Number;
        let t = |s: &str| Value::Text(s.into());
        assert!(m(n(11.0), ">10"));
        assert!(!m(n(10.0), ">10"));
        assert!(m(n(10.0), ">=10"));
        assert!(m(n(10.0), "10"));
        assert!(m(n(10.0), "=10"));
        assert!(!m(t("10"), ">5"));
        assert!(m(n(3.0), "<>4"));
        assert!(m(t("x"), "<>4"));
        assert!(m(t("Apple"), "apple"));
        assert!(!m(t("Apples"), "apple"));
        assert!(m(t("my draft v2"), "=*draft*"));
        assert!(m(t("cat"), "c?t"));
        assert!(!m(t("coat"), "c?t"));
        assert!(m(t("a*b"), "a~*b"));
        assert!(!m(t("axb"), "a~*b"));
        assert!(!m(t("done"), "<>Done"));
        assert!(m(t("todo"), "<>done"));
        assert!(m(Value::Empty, "<>done"));
        assert!(m(Value::Empty, "="));
        assert!(!m(t(""), "="));
        assert!(m(t("x"), "<>"));
        assert!(!m(Value::Empty, "<>"));
        assert!(m(Value::Empty, ""));
        assert!(m(t(""), ""));
        assert!(m(t("b"), ">a"));
        assert!(!m(n(5.0), ">a"));
        assert!(m(Value::Bool(true), "TRUE"));
        assert!(!m(n(1.0), "TRUE"));
        assert!(m(Value::Error(ErrorKind::NA), "#N/A"));
        assert!(m(n(crate::format::date_to_serial(2026, 3, 1)), ">2026-01-01"));
        assert!(m(n(0.5), "50%"));
        assert!(wildcard_eq("a*", "ABC"));
        assert!(wildcard_eq("abc", "ABC"));
        assert!(!wildcard_eq("a?", "abc"));
    }
}
