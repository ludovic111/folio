//! The formula language end to end: operators, precedence, coercion, arrays, errors and
//! references across sheets.

use folio_calc::{Addr, Engine, ErrorKind, Value};

fn a(s: &str) -> Addr {
    Addr::parse(s).unwrap()
}

fn fixture() -> Engine {
    let mut e = Engine::new();
    e.set_sheets(&["Sheet1".into(), "It's data".into(), "2024".into()]);
    for (addr, input) in [("A1", "2"), ("A2", "3"), ("A3", "abc"), ("A4", "TRUE"), ("A5", "'5")] {
        e.set_cell(0, a(addr), input);
    }
    e.set_cell(1, a("A1"), "40");
    e.set_cell(1, a("A2"), "2");
    e.set_cell(2, a("B2"), "7");
    e.recalc();
    e
}

fn check(cases: &[(&str, Value)]) {
    let e = fixture();
    for (formula, want) in cases {
        let got = e.evaluate(0, a("D1"), formula);
        match (&got, want) {
            (Value::Number(x), Value::Number(y)) => assert!((x - y).abs() < 1e-9, "{formula}: got {x}, want {y}"),
            _ => assert_eq!(&got, want, "{formula}"),
        }
    }
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn t(s: &str) -> Value {
    Value::Text(s.into())
}

fn b(x: bool) -> Value {
    Value::Bool(x)
}

fn err(k: ErrorKind) -> Value {
    Value::Error(k)
}

#[test]
fn arithmetic_and_precedence() {
    check(&[
        ("1+2*3", n(7.0)),
        ("(1+2)*3", n(9.0)),
        ("2^3^2", n(64.0)),
        ("-2^2", n(4.0)),
        ("0-2^2", n(-4.0)),
        ("2*-3", n(-6.0)),
        ("10/4", n(2.5)),
        ("50%", n(0.5)),
        ("200*10%", n(20.0)),
        ("5%%", n(0.0005)),
        ("1-2-3", n(-4.0)),
        ("2^-1", n(0.5)),
        ("1e3+1", n(1001.0)),
        (".5*4", n(2.0)),
        ("+A1", n(2.0)),
        ("--A1", n(2.0)),
        ("A1*A2+A1^A2", n(14.0)),
        ("1/0", err(ErrorKind::Div0)),
        ("(1/0)*0", err(ErrorKind::Div0)),
    ]);
}

#[test]
fn text_and_comparisons() {
    check(&[
        ("\"a\"&\"b\"&1", t("ab1")),
        ("\"say \"\"hi\"\"\"", t("say \"hi\"")),
        ("1+2&3", t("33")),
        ("\"x\"&TRUE", t("xTRUE")),
        ("A1&A9", t("2")),
        ("1=1", b(true)),
        ("\"abc\"=\"ABC\"", b(true)),
        ("\"a\"<\"b\"", b(true)),
        ("\"b\">\"A\"", b(true)),
        ("1<\"a\"", b(true)),
        ("\"z\"<TRUE", b(true)),
        ("A9=0", b(true)),
        ("A9=\"\"", b(true)),
        ("0.1+0.2=0.3", b(true)),
        ("1<>1", b(false)),
        ("2>=2", b(true)),
        ("2<=1", b(false)),
        ("1+1=2", b(true)),
        ("\"a\"&\"b\"=\"AB\"", b(true)),
    ]);
}

#[test]
fn coercion() {
    check(&[
        ("\"3\"+1", n(4.0)),
        ("\" 3 \"*2", n(6.0)),
        ("\"1,000\"+1", n(1001.0)),
        ("\"50%\"*2", n(1.0)),
        ("\"2025-10-06\"+0", n(45936.0)),
        ("TRUE+1", n(2.0)),
        ("FALSE*5", n(0.0)),
        ("A4+1", n(2.0)),
        ("A5+1", n(6.0)),
        ("A3+1", err(ErrorKind::Value)),
        ("\"\"+1", err(ErrorKind::Value)),
        ("A9+1", n(1.0)),
        ("-\"2\"", n(-2.0)),
        ("SUM(A1:A5)", n(5.0)),
        ("SUM(A5, A4)", n(0.0)),
        ("SUM(\"5\", TRUE)", n(6.0)),
    ]);
}

#[test]
fn arrays() {
    check(&[
        ("SUM({1,2;3,4})", n(10.0)),
        ("SUM({1,2,3}*{4,5,6})", n(32.0)),
        ("SUM({1;2;3}*{1,10})", n(66.0)),
        ("SUM({1,2}*{1,2,3})", err(ErrorKind::NA)),
        ("SUM({1,2}+1)", n(5.0)),
        ("SUM(-{1,2})", n(-3.0)),
        ("SUM({1,2}^2)", n(5.0)),
        ("{\"a\",TRUE,#N/A}", t("a")),
        ("INDEX({\"a\",TRUE,#N/A}, 3)", err(ErrorKind::NA)),
        ("SUMPRODUCT(LEN({\"ab\",\"cde\"}))", n(5.0)),
        ("SUM((A1:A2>2)*1)", n(1.0)),
        ("COUNT({1,\"a\",TRUE,2})", n(2.0)),
    ]);
}

#[test]
fn errors_and_names() {
    check(&[
        ("#N/A", err(ErrorKind::NA)),
        ("#REF!+1", err(ErrorKind::Ref)),
        ("NA()+1/0", err(ErrorKind::NA)),
        ("1/0+NA()", err(ErrorKind::Div0)),
        ("ISERROR(#VALUE!)", b(true)),
        ("unknown_name", err(ErrorKind::Name)),
        ("SUM(1,,2)", n(3.0)),
        ("ROUND(1.5,)", n(2.0)),
        ("1+", err(ErrorKind::Name)),
        ("(1", err(ErrorKind::Name)),
    ]);
}

#[test]
fn sheet_references() {
    check(&[
        ("'It''s data'!A1+A1", n(42.0)),
        ("SUM('it''s data'!A1:A2)", n(42.0)),
        ("'2024'!B2*2", n(14.0)),
        ("Nowhere!A1", err(ErrorKind::Ref)),
        ("SUM(Nowhere!A1:A3)", err(ErrorKind::Ref)),
        ("Sheet1!A1+sheet1!$A$2", n(5.0)),
        ("ROWS(Sheet1!A1:A5)", n(5.0)),
    ]);
}
