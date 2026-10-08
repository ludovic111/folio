//! Splits a formula into tokens, keeping byte spans so formulas can be rewritten in place.
//!
//! The lexer never fails: characters it cannot read become [`Tok::Bad`], and the tokens always
//! cover the whole input (spaces included), so joining the token texts gives the formula back.

use crate::addr::{RefBody, scan_body};
use crate::value::ErrorKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Amp,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Percent,
}

/// A reference as written: an optional sheet name (unquoted) and the body with its anchors.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RefTok {
    pub sheet: Option<String>,
    /// Bytes taken by `Sheet!` or `'My sheet'!` at the start of the token (0 without a sheet).
    pub prefix_len: usize,
    pub body: RefBody,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    Number(f64),
    Text(String),
    Ident(String),
    Ref(RefTok),
    /// An error literal such as `#N/A`, possibly after a sheet (`Sheet2!#REF!`).
    Error(ErrorKind),
    Op(Op),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semicolon,
    Colon,
    Space,
    Bad,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '\\'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '\\'
}

/// Reads an error literal (`#DIV/0!`, `#N/A`…) at the start of `s`.
fn error_literal(s: &str) -> Option<(ErrorKind, usize)> {
    ErrorKind::ALL.into_iter().find_map(|kind| {
        let code = kind.code();
        (s.len() >= code.len() && s.is_char_boundary(code.len()) && s[..code.len()].eq_ignore_ascii_case(code))
            .then_some((kind, code.len()))
    })
}

/// Reads what follows a sheet prefix ending at `i`: a reference body or `#REF!`.
fn after_prefix(s: &str, i: usize, sheet: String, start: usize) -> Option<(Tok, usize)> {
    if let Some((body, end)) = scan_body(s.as_bytes(), i) {
        return Some((Tok::Ref(RefTok { sheet: Some(sheet), prefix_len: i - start, body }), end));
    }
    if let Some((kind, len)) = error_literal(&s[i..]) {
        return Some((Tok::Error(kind), i + len));
    }
    None
}

/// Tokenizes a formula (without its `=`).
pub(crate) fn lex(s: &str) -> Vec<Token> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap_or(' ');
        let start = i;
        let (tok, end) = lex_one(s, bytes, i, c);
        i = end.max(start + c.len_utf8());
        out.push(Token { tok, start, end: i });
    }
    out
}

fn lex_one(s: &str, bytes: &[u8], i: usize, c: char) -> (Tok, usize) {
    if c.is_whitespace() {
        let mut j = i;
        while let Some(ch) = s[j..].chars().next() {
            if !ch.is_whitespace() {
                break;
            }
            j += ch.len_utf8();
        }
        return (Tok::Space, j);
    }
    match c {
        '"' => {
            let mut j = i + 1;
            let mut text = String::new();
            loop {
                match s[j..].chars().next() {
                    None => return (Tok::Bad, s.len()),
                    Some('"') => {
                        if s[j + 1..].starts_with('"') {
                            text.push('"');
                            j += 2;
                        } else {
                            return (Tok::Text(text), j + 1);
                        }
                    }
                    Some(ch) => {
                        text.push(ch);
                        j += ch.len_utf8();
                    }
                }
            }
        }
        '#' => match error_literal(&s[i..]) {
            Some((kind, len)) => (Tok::Error(kind), i + len),
            None => (Tok::Bad, i + 1),
        },
        '\'' => {
            let mut j = i + 1;
            let mut name = String::new();
            loop {
                match s[j..].chars().next() {
                    None => return (Tok::Bad, s.len()),
                    Some('\'') => {
                        if s[j + 1..].starts_with('\'') {
                            name.push('\'');
                            j += 2;
                        } else {
                            j += 1;
                            break;
                        }
                    }
                    Some(ch) => {
                        name.push(ch);
                        j += ch.len_utf8();
                    }
                }
            }
            if bytes.get(j) == Some(&b'!')
                && let Some(found) = after_prefix(s, j + 1, name, i)
            {
                return found;
            }
            (Tok::Bad, j)
        }
        '+' => (Tok::Op(Op::Plus), i + 1),
        '-' => (Tok::Op(Op::Minus), i + 1),
        '*' => (Tok::Op(Op::Star), i + 1),
        '/' => (Tok::Op(Op::Slash), i + 1),
        '^' => (Tok::Op(Op::Caret), i + 1),
        '&' => (Tok::Op(Op::Amp), i + 1),
        '%' => (Tok::Op(Op::Percent), i + 1),
        '=' => (Tok::Op(Op::Eq), i + 1),
        '<' => match bytes.get(i + 1) {
            Some(b'=') => (Tok::Op(Op::Le), i + 2),
            Some(b'>') => (Tok::Op(Op::Ne), i + 2),
            _ => (Tok::Op(Op::Lt), i + 1),
        },
        '>' => match bytes.get(i + 1) {
            Some(b'=') => (Tok::Op(Op::Ge), i + 2),
            _ => (Tok::Op(Op::Gt), i + 1),
        },
        '(' => (Tok::LParen, i + 1),
        ')' => (Tok::RParen, i + 1),
        '{' => (Tok::LBrace, i + 1),
        '}' => (Tok::RBrace, i + 1),
        ',' => (Tok::Comma, i + 1),
        ';' => (Tok::Semicolon, i + 1),
        ':' => (Tok::Colon, i + 1),
        _ => {
            if (c == '$' || c.is_ascii_alphanumeric())
                && let Some((body, end)) = scan_body(bytes, i) {
                    return (Tok::Ref(RefTok { sheet: None, prefix_len: 0, body }), end);
                }
            if c.is_ascii_digit() || (c == '.' && bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit())) {
                return lex_number(s, i);
            }
            if is_ident_start(c) || c.is_ascii_digit() {
                let mut j = i;
                while let Some(ch) = s[j..].chars().next() {
                    if !is_ident_char(ch) {
                        break;
                    }
                    j += ch.len_utf8();
                }
                let name = s[i..j].to_string();
                if bytes.get(j) == Some(&b'!')
                    && let Some(found) = after_prefix(s, j + 1, name.clone(), i)
                {
                    return found;
                }
                return (Tok::Ident(name), j);
            }
            (Tok::Bad, i + c.len_utf8())
        }
    }
}

fn lex_number(s: &str, i: usize) -> (Tok, usize) {
    let b = s.as_bytes();
    let mut j = i;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j < b.len() && b[j] == b'.' {
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
    }
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        let mut k = j + 1;
        if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
            k += 1;
        }
        if k < b.len() && b[k].is_ascii_digit() {
            while k < b.len() && b[k].is_ascii_digit() {
                k += 1;
            }
            j = k;
        }
    }
    match s[i..j].parse::<f64>() {
        Ok(n) => (Tok::Number(n), j),
        Err(_) => (Tok::Bad, j.max(i + 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<Tok> {
        lex(s).into_iter().map(|t| t.tok).filter(|t| *t != Tok::Space).collect()
    }

    #[test]
    fn covers_input() {
        for s in ["SUM(A1:B2, 'My sheet'!C3) & \"x\"\"y\"", "1+ 2 *  3", "€ ?", "{1,2;3,4}"] {
            let joined: String = lex(s).iter().map(|t| &s[t.start..t.end]).collect();
            assert_eq!(joined, s);
        }
    }

    #[test]
    fn tokens() {
        assert_eq!(kinds("1.5e3"), vec![Tok::Number(1500.0)]);
        assert_eq!(kinds("\"a\"\"b\""), vec![Tok::Text("a\"b".into())]);
        assert_eq!(kinds("#n/a"), vec![Tok::Error(ErrorKind::NA)]);
        assert_eq!(kinds("<>"), vec![Tok::Op(Op::Ne)]);
        assert!(matches!(kinds("LOG10(")[0], Tok::Ident(ref n) if n == "LOG10"));
        assert!(matches!(kinds("A1")[0], Tok::Ref(_)));
        assert!(matches!(kinds("A1B")[0], Tok::Ident(_)));
        assert!(matches!(kinds("_xlfn.STDEV.S(")[0], Tok::Ident(ref n) if n == "_xlfn.STDEV.S"));
        assert!(matches!(kinds("1:3")[0], Tok::Ref(RefTok { body: RefBody::Rows(..), .. })));
        assert!(matches!(kinds("B:B")[0], Tok::Ref(RefTok { body: RefBody::Cols(..), .. })));
        assert_eq!(kinds("Sheet2!#REF!"), vec![Tok::Error(ErrorKind::Ref)]);
        match &kinds("'It''s here'!$A$1:B2")[0] {
            Tok::Ref(r) => {
                assert_eq!(r.sheet.as_deref(), Some("It's here"));
                assert_eq!(r.prefix_len, 13);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(kinds("\"open"), vec![Tok::Bad]);
    }
}
