//! Parses a formula into an expression tree, once per cell.
//!
//! Precedence, from tightest to loosest, as in Excel: `:` (range), unary `-`/`+`, `%` (postfix),
//! `^` (left to right, so `2^3^2` is 64, and `-2^2` is 4), `*` `/`, `+` `-`, `&`, then the
//! comparisons `= <> < > <= >=`. Function arguments are separated by `,`; array constants use
//! `,` between columns and `;` between rows. Spaces are ignored (there is no intersection
//! operator).

use std::fmt;

use crate::addr::Range;
use crate::eval::Array;
use crate::functions::{self, Builtin};
use crate::lexer::{Op, Tok, Token, lex};
use crate::value::{ErrorKind, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Clone, Debug)]
pub(crate) enum Func {
    Builtin(&'static Builtin),
    /// Not built in: a plugin function looked up when evaluated (uppercase name).
    Custom(String),
}

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
    /// A reference; the sheet name is lowercased for lookup.
    Ref {
        sheet: Option<String>,
        range: Range,
    },
    /// An argument left empty, as in `IF(A1,,1)`.
    Missing,
    /// An unknown name (defined names are not supported): `#NAME?`.
    Name(#[allow(dead_code)] String),
    Neg(Box<Expr>),
    Percent(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    /// The `:` operator between two expressions that are not plain references (`A1:INDEX(…)`).
    Range(Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
    Array(Array),
}

/// Why a formula could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Byte offset in the formula (without `=`).
    pub position: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at {})", self.message, self.position)
    }
}

impl std::error::Error for ParseError {}

/// Checks that a formula (without `=`) parses; the error says where it went wrong.
pub fn validate_formula(formula: &str) -> Result<(), ParseError> {
    parse(formula).map(|_| ())
}

/// Removes the prefixes XLSX files put on newer functions.
pub(crate) fn strip_prefixes(name: &str) -> &str {
    let mut name = name;
    for prefix in ["_xlfn.", "_xlws.", "_xludf."] {
        if name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix) {
            name = &name[prefix.len()..];
        }
    }
    name
}

pub(crate) fn parse(formula: &str) -> Result<Expr, ParseError> {
    let tokens: Vec<Token> = lex(formula).into_iter().filter(|t| t.tok != Tok::Space).collect();
    let mut p = Parser { tokens, pos: 0, len: formula.len() };
    if p.tokens.is_empty() {
        return Err(p.error("empty formula"));
    }
    let expr = p.comparison()?;
    if p.pos < p.tokens.len() {
        return Err(p.error("unexpected text"));
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    len: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|t| &t.tok)
    }

    fn error(&self, message: &str) -> ParseError {
        let position = self.tokens.get(self.pos).map_or(self.len, |t| t.start);
        ParseError { message: message.to_string(), position }
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn comparison(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.concat()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::Eq)) => BinOp::Eq,
                Some(Tok::Op(Op::Ne)) => BinOp::Ne,
                Some(Tok::Op(Op::Lt)) => BinOp::Lt,
                Some(Tok::Op(Op::Gt)) => BinOp::Gt,
                Some(Tok::Op(Op::Le)) => BinOp::Le,
                Some(Tok::Op(Op::Ge)) => BinOp::Ge,
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.concat()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn concat(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.additive()?;
        while self.eat(&Tok::Op(Op::Amp)) {
            let rhs = self.additive()?;
            lhs = Expr::Bin(BinOp::Concat, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn additive(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::Plus)) => BinOp::Add,
                Some(Tok::Op(Op::Minus)) => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.multiplicative()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.power()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::Star)) => BinOp::Mul,
                Some(Tok::Op(Op::Slash)) => BinOp::Div,
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.power()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn power(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.postfix()?;
        while self.eat(&Tok::Op(Op::Caret)) {
            let rhs = self.postfix()?;
            lhs = Expr::Bin(BinOp::Pow, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.unary()?;
        while self.eat(&Tok::Op(Op::Percent)) {
            e = Expr::Percent(Box::new(e));
        }
        Ok(e)
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        if self.eat(&Tok::Op(Op::Minus)) {
            let inner = self.unary()?;
            return Ok(match inner {
                Expr::Number(n) => Expr::Number(-n),
                other => Expr::Neg(Box::new(other)),
            });
        }
        if self.eat(&Tok::Op(Op::Plus)) {
            return self.unary();
        }
        self.range()
    }

    fn range(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.primary()?;
        while self.eat(&Tok::Colon) {
            let rhs = self.primary()?;
            lhs = Expr::Range(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let Some(token) = self.tokens.get(self.pos).cloned() else {
            return Err(self.error("formula ends too early"));
        };
        self.pos += 1;
        match token.tok {
            Tok::Number(n) => Ok(Expr::Number(n)),
            Tok::Text(s) => Ok(Expr::Text(s)),
            Tok::Error(e) => Ok(Expr::Error(e)),
            Tok::Ref(r) => Ok(Expr::Ref { sheet: r.sheet.map(|s| s.to_lowercase()), range: r.body.range() }),
            Tok::Ident(name) => {
                if self.eat(&Tok::LParen) {
                    let args = self.arguments()?;
                    let name = strip_prefixes(&name).to_ascii_uppercase();
                    let func = match functions::lookup(&name) {
                        Some(b) => Func::Builtin(b),
                        None => Func::Custom(name),
                    };
                    return Ok(Expr::Call(func, args));
                }
                if name.eq_ignore_ascii_case("TRUE") {
                    Ok(Expr::Bool(true))
                } else if name.eq_ignore_ascii_case("FALSE") {
                    Ok(Expr::Bool(false))
                } else {
                    Ok(Expr::Name(name))
                }
            }
            Tok::LParen => {
                let e = self.comparison()?;
                if !self.eat(&Tok::RParen) {
                    return Err(self.error("missing )"));
                }
                Ok(e)
            }
            Tok::LBrace => self.array(),
            _ => {
                self.pos -= 1;
                Err(self.error("unexpected symbol"))
            }
        }
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.eat(&Tok::RParen) {
            return Ok(args);
        }
        loop {
            match self.peek() {
                Some(Tok::Comma) | Some(Tok::RParen) => args.push(Expr::Missing),
                _ => args.push(self.comparison()?),
            }
            if self.eat(&Tok::Comma) {
                continue;
            }
            if self.eat(&Tok::RParen) {
                return Ok(args);
            }
            return Err(self.error("expected , or )"));
        }
    }

    fn array(&mut self) -> Result<Expr, ParseError> {
        let mut rows: Vec<Vec<Value>> = vec![Vec::new()];
        loop {
            let negative = self.eat(&Tok::Op(Op::Minus));
            let Some(token) = self.tokens.get(self.pos).cloned() else {
                return Err(self.error("missing }"));
            };
            self.pos += 1;
            let value = match token.tok {
                Tok::Number(n) => Value::Number(if negative { -n } else { n }),
                Tok::Text(s) if !negative => Value::Text(s),
                Tok::Error(e) if !negative => Value::Error(e),
                Tok::Ident(ref s) if !negative && s.eq_ignore_ascii_case("TRUE") => Value::Bool(true),
                Tok::Ident(ref s) if !negative && s.eq_ignore_ascii_case("FALSE") => Value::Bool(false),
                _ => {
                    self.pos -= 1;
                    return Err(self.error("arrays hold only constants"));
                }
            };
            if let Some(row) = rows.last_mut() {
                row.push(value);
            }
            if self.eat(&Tok::Comma) {
                continue;
            }
            if self.eat(&Tok::Semicolon) {
                rows.push(Vec::new());
                continue;
            }
            if self.eat(&Tok::RBrace) {
                break;
            }
            return Err(self.error("expected , ; or }"));
        }
        let cols = rows[0].len();
        if rows.iter().any(|r| r.len() != cols) {
            return Err(self.error("array rows differ in length"));
        }
        let n_rows = rows.len();
        Ok(Expr::Array(Array { rows: n_rows, cols, data: rows.into_iter().flatten().collect() }))
    }
}

/// Visits every reference in an expression.
pub(crate) fn visit_refs<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Option<String>, &'a Range)) {
    match e {
        Expr::Ref { sheet, range } => f(sheet, range),
        Expr::Neg(a) | Expr::Percent(a) => visit_refs(a, f),
        Expr::Bin(_, a, b) | Expr::Range(a, b) => {
            visit_refs(a, f);
            visit_refs(b, f);
        }
        Expr::Call(_, args) => args.iter().for_each(|a| visit_refs(a, f)),
        _ => {}
    }
}

/// True when an expression calls a volatile function (NOW, RAND, OFFSET, INDIRECT…).
pub(crate) fn is_volatile(e: &Expr) -> bool {
    match e {
        Expr::Neg(a) | Expr::Percent(a) => is_volatile(a),
        Expr::Bin(_, a, b) | Expr::Range(a, b) => is_volatile(a) || is_volatile(b),
        Expr::Call(func, args) => matches!(func, Func::Builtin(b) if b.volatile) || args.iter().any(is_volatile),
        _ => false,
    }
}

/// True when an expression calls the custom function `name` (uppercase).
pub(crate) fn calls_custom(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Neg(a) | Expr::Percent(a) => calls_custom(a, name),
        Expr::Bin(_, a, b) | Expr::Range(a, b) => calls_custom(a, name) || calls_custom(b, name),
        Expr::Call(func, args) => {
            matches!(func, Func::Custom(n) if n == name) || args.iter().any(|a| calls_custom(a, name))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes an expression back in a fully parenthesized form to check the tree's shape.
    fn show(e: &Expr) -> String {
        match e {
            Expr::Number(n) => n.to_string(),
            Expr::Text(s) => format!("{s:?}"),
            Expr::Bool(b) => b.to_string().to_uppercase(),
            Expr::Error(k) => k.code().to_string(),
            Expr::Ref { sheet, range } => match sheet {
                Some(s) => format!("{s}!{}", range.a1()),
                None => range.a1(),
            },
            Expr::Missing => "_".into(),
            Expr::Name(n) => format!("?{n}"),
            Expr::Neg(a) => format!("(-{})", show(a)),
            Expr::Percent(a) => format!("({}%)", show(a)),
            Expr::Bin(op, a, b) => {
                let sym = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Pow => "^",
                    BinOp::Concat => "&",
                    BinOp::Eq => "=",
                    BinOp::Ne => "<>",
                    BinOp::Lt => "<",
                    BinOp::Gt => ">",
                    BinOp::Le => "<=",
                    BinOp::Ge => ">=",
                };
                format!("({}{sym}{})", show(a), show(b))
            }
            Expr::Range(a, b) => format!("({}:{})", show(a), show(b)),
            Expr::Call(f, args) => {
                let name = match f {
                    Func::Builtin(b) => b.name.to_string(),
                    Func::Custom(n) => n.clone(),
                };
                format!("{name}({})", args.iter().map(show).collect::<Vec<_>>().join(","))
            }
            Expr::Array(a) => format!("{{{}x{}}}", a.rows, a.cols),
        }
    }

    fn p(s: &str) -> String {
        show(&parse(s).unwrap_or_else(|e| panic!("{s}: {e}")))
    }

    #[test]
    fn precedence() {
        assert_eq!(p("1+2*3"), "(1+(2*3))");
        assert_eq!(p("(1+2)*3"), "((1+2)*3)");
        assert_eq!(p("2^3^2"), "((2^3)^2)");
        assert_eq!(p("-2^2"), "(-2^2)");
        assert_eq!(p("-A1^2"), "((-A1)^2)");
        assert_eq!(p("50%*2"), "((50%)*2)");
        assert_eq!(p("1&2+3"), "(1&(2+3))");
        assert_eq!(p("1+2=3"), "((1+2)=3)");
        assert_eq!(p("A1<>B1&\"x\""), "(A1<>(B1&\"x\"))");
        assert_eq!(p("1-2-3"), "((1-2)-3)");
        assert_eq!(p("8/4/2"), "((8/4)/2)");
        assert_eq!(p("+A1"), "A1");
        assert_eq!(p("--A1"), "(-(-A1))");
    }

    #[test]
    fn references_and_calls() {
        assert_eq!(p("SUM(A1:B3, $C$4)"), "SUM(A1:B3,C4)");
        assert_eq!(p("sum(a1)"), "SUM(A1)");
        assert_eq!(p("_xlfn.STDEV.S(A:A)"), "STDEV.S(A:A)");
        assert_eq!(p("'My sheet'!A1+Sheet2!B2"), "(my sheet!A1+sheet2!B2)");
        assert_eq!(p("SUM(1:1)"), "SUM(1:1)");
        assert_eq!(p("IF(A1,,1)"), "IF(A1,_,1)");
        assert_eq!(p("NOW()"), "NOW()");
        assert_eq!(p("A1:INDEX(B1:B3,2)"), "(A1:INDEX(B1:B3,2))");
        assert_eq!(p("MYFUNC(1)"), "MYFUNC(1)");
        assert_eq!(p("foo"), "?foo");
        assert_eq!(p("true"), "TRUE");
        assert_eq!(p("#DIV/0!"), "#DIV/0!");
        assert_eq!(p("{1,2;3,-4}"), "{2x2}");
        assert_eq!(p(" 1 + 2 "), "(1+2)");
        assert_eq!(p("\"a\"\"b\""), "\"a\\\"b\"");
    }

    #[test]
    fn errors() {
        for bad in ["", "1+", "SUM(1", "(1", "1)", "{1,2;3}", "{A1}", "\"open", "1 2", "*2", "A1!"] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
        assert_eq!(validate_formula("SUM(1,").unwrap_err().position, 6);
    }
}
