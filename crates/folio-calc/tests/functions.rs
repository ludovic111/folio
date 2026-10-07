//! Built-in functions, evaluated through the engine on a small fixture sheet.

use folio_calc::{Addr, Engine, ErrorKind, Value, date_to_serial};

fn a(s: &str) -> Addr {
    Addr::parse(s).unwrap()
}

/// Sheet1:
///   A1:A5  1 2 3 4 5
///   B1:B5  10 20 "x" 40 TRUE
///   C1:C5  apple Banana cherry "apple pie" date
///   D1:D4  10 20 30 40 (sorted, for approximate lookups)
///   E1:E4  "ten" "twenty" "thirty" "forty"
///   F1     #DIV/0! (a formula)
///   G1:G3  "3" (text) 4 ""(formula giving empty text)
/// Sheet2: A1 100, B2 "two"
fn fixture() -> Engine {
    let mut e = Engine::new();
    e.set_sheets(&["Sheet1".into(), "Sheet2".into()]);
    let cells = [
        ("A1", "1"),
        ("A2", "2"),
        ("A3", "3"),
        ("A4", "4"),
        ("A5", "5"),
        ("B1", "10"),
        ("B2", "20"),
        ("B3", "x"),
        ("B4", "40"),
        ("B5", "TRUE"),
        ("C1", "apple"),
        ("C2", "Banana"),
        ("C3", "cherry"),
        ("C4", "apple pie"),
        ("C5", "date"),
        ("D1", "10"),
        ("D2", "20"),
        ("D3", "30"),
        ("D4", "40"),
        ("E1", "ten"),
        ("E2", "twenty"),
        ("E3", "thirty"),
        ("E4", "forty"),
        ("F1", "=1/0"),
        ("G1", "'3"),
        ("G2", "4"),
        ("G3", "=\"\""),
    ];
    for (addr, input) in cells {
        e.set_cell(0, a(addr), input);
    }
    e.set_cell(1, a("A1"), "100");
    e.set_cell(1, a("B2"), "two");
    e.recalc();
    e
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

/// Evaluates formulas in Sheet1!K1 and compares the results.
fn check(cases: &[(&str, Value)]) {
    let e = fixture();
    for (formula, want) in cases {
        let got = e.evaluate(0, a("K1"), formula);
        match (&got, want) {
            (Value::Number(x), Value::Number(y)) => {
                assert!((x - y).abs() <= 1e-9 * y.abs().max(1.0), "{formula}: got {x}, want {y}")
            }
            _ => assert_eq!(&got, want, "{formula}"),
        }
    }
}

// Rounding examples deliberately use truncated decimal values, not pi.
#[allow(clippy::approx_constant)]
#[test]
fn math() {
    use ErrorKind::*;
    check(&[
        ("SUM(A1:A5)", n(15.0)),
        ("SUM(B1:B5)", n(70.0)),
        ("SUM(A1:A5, 10, \"5\", TRUE)", n(31.0)),
        ("SUM(A:A)", n(15.0)),
        ("SUM(\"x\")", err(Value)),
        ("SUM(A1, F1)", err(Div0)),
        ("SUM(G1:G3)", n(4.0)),
        ("PRODUCT(A1:A5)", n(120.0)),
        ("PRODUCT(B3)", n(0.0)),
        ("SUMPRODUCT(A1:A4, D1:D4)", n(300.0)),
        ("SUMPRODUCT(A1:A4*D1:D4)", n(300.0)),
        ("SUMPRODUCT((A1:A5>2)*A1:A5)", n(12.0)),
        ("SUMPRODUCT(A1:A4, D1:D3)", err(Value)),
        ("ROUND(2.675, 2)", n(2.68)),
        ("ROUND(-2.5, 0)", n(-3.0)),
        ("ROUND(1234.5678, -2)", n(1200.0)),
        ("ROUND(1.5)", n(2.0)),
        ("ROUNDUP(3.2, 0)", n(4.0)),
        ("ROUNDUP(-3.2, 0)", n(-4.0)),
        ("ROUNDUP(3.14159, 3)", n(3.142)),
        ("ROUNDDOWN(3.99, 0)", n(3.0)),
        ("ROUNDDOWN(-3.99, 1)", n(-3.9)),
        ("INT(-3.5)", n(-4.0)),
        ("INT(3.9)", n(3.0)),
        ("TRUNC(-3.9)", n(-3.0)),
        ("TRUNC(3.14159, 2)", n(3.14)),
        ("ABS(-4)", n(4.0)),
        ("MOD(10, 3)", n(1.0)),
        ("MOD(-3, 2)", n(1.0)),
        ("MOD(3, -2)", n(-1.0)),
        ("MOD(1, 0)", err(Div0)),
        ("POWER(2, 10)", n(1024.0)),
        ("POWER(-8, 1/3)", err(Num)),
        ("SQRT(16)", n(4.0)),
        ("SQRT(-1)", err(Num)),
        ("EXP(0)", n(1.0)),
        ("LN(EXP(2))", n(2.0)),
        ("LN(0)", err(Num)),
        ("LOG(1000)", n(3.0)),
        ("LOG(8, 2)", n(3.0)),
        ("LOG(8, 1)", err(Div0)),
        ("LOG10(0.01)", n(-2.0)),
        ("PI()", n(std::f64::consts::PI)),
        ("CEILING(2.5, 1)", n(3.0)),
        ("CEILING(4.42, 0.05)", n(4.45)),
        ("CEILING(-2.5, 2)", n(-2.0)),
        ("CEILING(-2.5, -2)", n(-4.0)),
        ("CEILING(2.5, -2)", err(Num)),
        ("FLOOR(2.5, 1)", n(2.0)),
        ("FLOOR(-2.5, 2)", n(-4.0)),
        ("FLOOR(-2.5, -2)", n(-2.0)),
        ("FLOOR(5, 0)", err(Div0)),
        ("SIGN(-0.5)", n(-1.0)),
        ("SIGN(0)", n(0.0)),
        ("AND(RAND()>=0, RAND()<1)", b(true)),
        ("RANDBETWEEN(3, 3)", n(3.0)),
        ("RANDBETWEEN(5, 1)", err(Num)),
        ("ABS()", err(Value)),
    ]);
    let e = fixture();
    for _ in 0..50 {
        let v = e.evaluate(0, a("K1"), "RANDBETWEEN(1, 6)").as_number().unwrap();
        assert!((1.0..=6.0).contains(&v) && v.fract() == 0.0);
    }
}

#[test]
fn statistics() {
    use ErrorKind::*;
    check(&[
        ("AVERAGE(A1:A5)", n(3.0)),
        ("AVERAGE(B1:B5)", n(70.0 / 3.0)),
        ("AVERAGE(B3)", err(Div0)),
        ("COUNT(A1:C5)", n(8.0)),
        ("COUNT(1, \"2\", \"x\", TRUE)", n(3.0)),
        ("COUNT(F1, A1)", n(1.0)),
        ("COUNTA(A1:C5)", n(15.0)),
        ("COUNTA(G1:G3)", n(3.0)),
        ("COUNTBLANK(G1:G5)", n(3.0)),
        ("COUNTBLANK(H1:H10)", n(10.0)),
        ("MIN(A1:A5, -2)", n(-2.0)),
        ("MAX(B1:B5)", n(40.0)),
        ("MAX(H1:H3)", n(0.0)),
        ("MEDIAN(A1:A5)", n(3.0)),
        ("MEDIAN(A1:A4)", n(2.5)),
        ("MEDIAN(H1)", err(Num)),
        ("MODE(1, 2, 2, 3, 3)", n(2.0)),
        ("MODE(1, 2, 3)", err(NA)),
        ("STDEV(A1:A5)", n(2.5f64.sqrt())),
        ("STDEV.S(A1:A5)", n(2.5f64.sqrt())),
        ("STDEV.P(A1:A5)", n(2.0f64.sqrt())),
        ("VAR(A1:A5)", n(2.5)),
        ("VAR.S(A1:A5)", n(2.5)),
        ("VAR.P(A1:A5)", n(2.0)),
        ("STDEV(1)", err(Div0)),
        ("LARGE(D1:D4, 1)", n(40.0)),
        ("LARGE(D1:D4, 3)", n(20.0)),
        ("SMALL(D1:D4, 2)", n(20.0)),
        ("SMALL(D1:D4, 5)", err(Num)),
        ("RANK(30, D1:D4)", n(2.0)),
        ("RANK(30, D1:D4, 1)", n(3.0)),
        ("RANK(35, D1:D4)", err(NA)),
    ]);
}

#[test]
fn criteria_functions() {
    use ErrorKind::*;
    check(&[
        ("COUNTIF(A1:A5, \">2\")", n(3.0)),
        ("COUNTIF(A1:A5, 3)", n(1.0)),
        ("COUNTIF(C1:C5, \"apple\")", n(1.0)),
        ("COUNTIF(C1:C5, \"APPLE*\")", n(2.0)),
        ("COUNTIF(C1:C5, \"?????\")", n(1.0)),
        ("COUNTIF(C1:C5, \"<>apple\")", n(4.0)),
        ("COUNTIF(B1:B5, \"<>\")", n(5.0)),
        ("COUNTIF(H1:H4, \"\")", n(4.0)),
        ("COUNTIF(A:A, \">=4\")", n(2.0)),
        ("SUMIF(A1:A5, \">2\")", n(12.0)),
        ("SUMIF(C1:C5, \"apple*\", A1:A5)", n(5.0)),
        ("SUMIF(C1:C5, \"apple*\", A1)", n(5.0)),
        ("SUMIF(A1:A5, \"<3\", D1:D5)", n(30.0)),
        ("SUMIFS(D1:D4, A1:A4, \">1\", A1:A4, \"<4\")", n(50.0)),
        ("SUMIFS(D1:D4, A1:A3, \">1\")", err(Value)),
        ("COUNTIFS(A1:A5, \">1\", C1:C5, \"*a*\")", n(3.0)),
        ("AVERAGEIF(A1:A5, \">2\")", n(4.0)),
        ("AVERAGEIF(A1:A5, \">9\")", err(Div0)),
        ("AVERAGEIF(C1:C4, \"apple*\", D1:D4)", n(25.0)),
        ("AVERAGEIFS(D1:D4, A1:A4, \"<=2\")", n(15.0)),
        ("MINIFS(D1:D4, A1:A4, \">1\")", n(20.0)),
        ("MAXIFS(D1:D4, A1:A4, \"<4\")", n(30.0)),
        ("MAXIFS(D1:D4, A1:A4, \">40\")", n(0.0)),
        ("SUMIF(A1:A5, \">2\", F1:F5)", n(0.0)),
        ("SUMIF(A1:A5, 1, F1:F5)", err(Div0)),
    ]);
}

#[test]
fn logical() {
    use ErrorKind::*;
    check(&[
        ("IF(A1=1, \"yes\", \"no\")", t("yes")),
        ("IF(A1=2, \"yes\", \"no\")", t("no")),
        ("IF(FALSE, 1)", b(false)),
        ("IF(TRUE, )", n(0.0)),
        ("IF(TRUE, 1, 1/0)", n(1.0)),
        ("IF(\"x\", 1, 2)", err(Value)),
        ("IF(\"true\", 1, 2)", n(1.0)),
        ("IF(F1, 1, 2)", err(Div0)),
        ("SUM(IF(A1:A5>2, A1:A5, 0))", n(12.0)),
        ("SUM(IF(A1:A5>2, 1))", n(3.0)),
        ("IFS(A1>5, \"big\", A1>0, \"small\")", t("small")),
        ("IFS(A1>5, \"big\")", err(NA)),
        ("IFERROR(1/0, \"oops\")", t("oops")),
        ("IFERROR(5, \"oops\")", n(5.0)),
        ("SUM(IFERROR(1/(A1:A3-2), 0))", n(0.0)),
        ("IFNA(NA(), 0)", n(0.0)),
        ("IFNA(1/0, 0)", err(Div0)),
        ("AND(TRUE, 1, A1:A5)", b(true)),
        ("AND(TRUE, 0)", b(false)),
        ("AND(C1:C2)", err(Value)),
        ("AND(\"x\")", err(Value)),
        ("OR(FALSE, 0, A1)", b(true)),
        ("OR(F1, TRUE)", err(Div0)),
        ("XOR(TRUE, TRUE, TRUE)", b(true)),
        ("XOR(TRUE, TRUE)", b(false)),
        ("NOT(TRUE)", b(false)),
        ("NOT(0)", b(true)),
        ("TRUE()", b(true)),
        ("FALSE()", b(false)),
        ("SWITCH(A2, 1, \"one\", 2, \"two\", \"other\")", t("two")),
        ("SWITCH(A5, 1, \"one\", 2, \"two\", \"other\")", t("other")),
        ("SWITCH(A5, 1, \"one\")", err(NA)),
        ("SWITCH(\"B\", \"a\", 1, \"b\", 2)", n(2.0)),
        ("CHOOSE(2, \"a\", \"b\", \"c\")", t("b")),
        ("CHOOSE(4, \"a\", \"b\", \"c\")", err(Value)),
        ("SUM(CHOOSE(1, A1:A5, B1:B5))", n(15.0)),
    ]);
}

#[test]
fn lookups() {
    use ErrorKind::*;
    check(&[
        ("VLOOKUP(30, D1:E4, 2, FALSE)", t("thirty")),
        ("VLOOKUP(35, D1:E4, 2)", t("thirty")),
        ("VLOOKUP(35, D1:E4, 2, TRUE)", t("thirty")),
        ("VLOOKUP(5, D1:E4, 2)", err(NA)),
        ("VLOOKUP(99, D1:E4, 2)", t("forty")),
        ("VLOOKUP(35, D1:E4, 2, FALSE)", err(NA)),
        ("VLOOKUP(30, D1:E4, 3, FALSE)", err(Ref)),
        ("VLOOKUP(30, D1:E4, 0, FALSE)", err(Value)),
        ("VLOOKUP(\"BANANA\", C1:D5, 2, FALSE)", n(20.0)),
        ("VLOOKUP(\"ch*\", C1:D5, 2, FALSE)", n(30.0)),
        ("VLOOKUP(30, D:E, 2, FALSE)", t("thirty")),
        ("HLOOKUP(3, A1:A5, 1, FALSE)", err(NA)),
        ("HLOOKUP(\"twenty\", {\"ten\",\"twenty\";1,2}, 2, FALSE)", n(2.0)),
        ("HLOOKUP(15, {10,20,30;\"a\",\"b\",\"c\"}, 2)", t("a")),
        ("MATCH(30, D1:D4, 0)", n(3.0)),
        ("MATCH(35, D1:D4)", n(3.0)),
        ("MATCH(35, D1:D4, 1)", n(3.0)),
        ("MATCH(5, D1:D4, 1)", err(NA)),
        ("MATCH(25, {40,30,20,10}, -1)", n(2.0)),
        ("MATCH(\"b*\", C1:C5, 0)", n(2.0)),
        ("MATCH(1, A1:B2, 0)", err(NA)),
        ("XLOOKUP(20, D1:D4, E1:E4)", t("twenty")),
        ("XLOOKUP(25, D1:D4, E1:E4)", err(NA)),
        ("XLOOKUP(25, D1:D4, E1:E4, \"none\")", t("none")),
        ("XLOOKUP(25, D1:D4, E1:E4, , -1)", t("twenty")),
        ("XLOOKUP(25, D1:D4, E1:E4, , 1)", t("thirty")),
        ("XLOOKUP(\"*ana*\", C1:C5, A1:A5, , 2)", n(2.0)),
        ("XLOOKUP(\"*ana*\", C1:C5, A1:A5)", err(NA)),
        ("XLOOKUP(\"apple*\", C1:C5, A1:A5, , 2, -1)", n(4.0)),
        ("XLOOKUP(\"apple*\", C1:C5, A1:A5, , 2, 1)", n(1.0)),
        ("SUM(XLOOKUP(3, A1:A5, A1:B5))", n(3.0)),
        ("XLOOKUP(20, D1:D4, E1:E3)", err(Value)),
        ("XMATCH(30, D1:D4)", n(3.0)),
        ("XMATCH(31, D1:D4, 1)", n(4.0)),
        ("XMATCH(31, D1:D4, -1)", n(3.0)),
        ("XMATCH(31, D1:D4)", err(NA)),
        ("LOOKUP(25, D1:D4, E1:E4)", t("twenty")),
        ("LOOKUP(25, D1:E4)", t("twenty")),
        ("LOOKUP(1, D1:D4)", err(NA)),
        ("INDEX(D1:E4, 2, 2)", t("twenty")),
        ("INDEX(D1:D4, 3)", n(30.0)),
        ("INDEX(A1:E1, 4)", n(10.0)),
        ("SUM(INDEX(D1:E4, 0, 1))", n(100.0)),
        ("SUM(INDEX(A1:B5, 2, 0))", n(22.0)),
        ("INDEX(D1:D4, 5)", err(Ref)),
        ("INDEX({1,2;3,4}, 2, 1)", n(3.0)),
        ("SUM(A1:INDEX(A1:A5, 3))", n(6.0)),
        ("INDEX(D1:E4, MATCH(40, D1:D4, 0), 2)", t("forty")),
        ("ROW()", n(1.0)),
        ("ROW(C7)", n(7.0)),
        ("SUM(ROW(A1:A3))", n(6.0)),
        ("COLUMN()", n(11.0)),
        ("COLUMN(D2)", n(4.0)),
        ("ROWS(A1:C5)", n(5.0)),
        ("COLUMNS(A1:C5)", n(3.0)),
        ("ROWS({1,2;3,4;5,6})", n(3.0)),
        ("ROWS(A:A)", n(1_048_576.0)),
        ("SUM(OFFSET(A1, 1, 0, 3, 1))", n(9.0)),
        ("OFFSET(A1, 2, 3)", n(30.0)),
        ("OFFSET(A1, -1, 0)", err(Ref)),
        ("OFFSET(1, 1, 1)", err(Value)),
        ("INDIRECT(\"D3\")", n(30.0)),
        ("INDIRECT(\"Sheet2!A1\")", n(100.0)),
        ("INDIRECT(\"'sheet2'!B2\")", t("two")),
        ("SUM(INDIRECT(\"A1:A\" & 3))", n(6.0)),
        ("INDIRECT(\"Nope!A1\")", err(Ref)),
        ("INDIRECT(\"hello\")", err(Ref)),
    ]);
}

#[test]
fn text() {
    use ErrorKind::*;
    check(&[
        ("TEXT(1234.5, \"#,##0.00\")", t("1,234.50")),
        ("TEXT(0.25, \"0%\")", t("25%")),
        ("TEXT(\"3\", \"0.0\")", t("3.0")),
        ("TEXT(45936, \"yyyy-mm-dd\")", t("2025-10-06")),
        ("TEXT(\"abc\", \"0\")", t("abc")),
        ("CONCAT(\"a\", 1, TRUE)", t("a1TRUE")),
        ("CONCAT(A1:A3)", t("123")),
        ("CONCATENATE(\"x\", \"-\", 2.5)", t("x-2.5")),
        ("\"a\" & 1/3", t("a0.333333333333333")),
        ("TEXTJOIN(\", \", TRUE, \"a\", \"\", \"b\")", t("a, b")),
        ("TEXTJOIN(\"-\", FALSE, \"a\", \"\", \"b\")", t("a--b")),
        ("TEXTJOIN(\"/\", TRUE, A1:A3, H1)", t("1/2/3")),
        ("LEFT(\"hello\", 2)", t("he")),
        ("LEFT(\"hello\")", t("h")),
        ("LEFT(\"hi\", 10)", t("hi")),
        ("LEFT(\"hi\", -1)", err(Value)),
        ("RIGHT(\"hello\", 3)", t("llo")),
        ("MID(\"hello\", 2, 3)", t("ell")),
        ("MID(\"hello\", 9, 3)", t("")),
        ("MID(\"hello\", 0, 3)", err(Value)),
        ("LEN(\"héllo\")", n(5.0)),
        ("LEN(12.5)", n(4.0)),
        ("LOWER(\"ABC\")", t("abc")),
        ("UPPER(\"abc\")", t("ABC")),
        ("PROPER(\"hello wORLD o'neil\")", t("Hello World O'Neil")),
        ("TRIM(\"  a   b  \")", t("a b")),
        ("SUBSTITUTE(\"a-b-c\", \"-\", \"+\")", t("a+b+c")),
        ("SUBSTITUTE(\"a-b-c\", \"-\", \"+\", 2)", t("a-b+c")),
        ("SUBSTITUTE(\"a-b-c\", \"-\", \"+\", 5)", t("a-b-c")),
        ("REPLACE(\"abcdef\", 2, 3, \"X\")", t("aXef")),
        ("FIND(\"b\", \"abcb\")", n(2.0)),
        ("FIND(\"b\", \"abcb\", 3)", n(4.0)),
        ("FIND(\"B\", \"abc\")", err(Value)),
        ("SEARCH(\"B\", \"abc\")", n(2.0)),
        ("SEARCH(\"c?e\", \"abcdef\")", n(3.0)),
        ("SEARCH(\"z\", \"abc\")", err(Value)),
        ("VALUE(\"1,234.5\")", n(1234.5)),
        ("VALUE(\"50%\")", n(0.5)),
        ("VALUE(\"2025-10-06\")", n(45936.0)),
        ("VALUE(\"abc\")", err(Value)),
        ("REPT(\"ab\", 3)", t("ababab")),
        ("REPT(\"ab\", -1)", err(Value)),
        ("EXACT(\"a\", \"A\")", b(false)),
        ("EXACT(\"a\", \"a\")", b(true)),
        ("CHAR(65)", t("A")),
        ("CODE(\"A\")", n(65.0)),
        ("CODE(\"\")", err(Value)),
        ("LEN(F1)", err(Div0)),
    ]);
}

#[test]
fn dates() {
    use ErrorKind::*;
    let d = |y, m, day| n(date_to_serial(y, m, day));
    check(&[
        ("DATE(2025, 10, 6)", n(45936.0)),
        ("DATE(2026, 13, 1)", d(2027, 1, 1)),
        ("DATE(2026, 1, 0)", d(2025, 12, 31)),
        ("DATE(2026, 0, 1)", d(2025, 12, 1)),
        ("DATE(26, 1, 1)", d(1926, 1, 1)),
        ("DATE(1900, 2, 29)", n(60.0)),
        ("DATE(1900, 3, 1)", n(61.0)),
        ("DATE(-1, 1, 1)", err(Num)),
        ("YEAR(45936)", n(2025.0)),
        ("MONTH(45936)", n(10.0)),
        ("DAY(45936)", n(6.0)),
        ("DAY(\"2026-10-06\")", n(6.0)),
        ("YEAR(-1)", err(Num)),
        ("WEEKDAY(45936)", n(2.0)),
        ("WEEKDAY(45936, 2)", n(1.0)),
        ("WEEKDAY(45936, 3)", n(0.0)),
        ("WEEKDAY(45936, 12)", n(7.0)),
        ("WEEKDAY(1)", n(1.0)),
        ("WEEKDAY(45936, 9)", err(Num)),
        ("WEEKNUM(DATE(2026, 1, 1))", n(1.0)),
        ("WEEKNUM(DATE(2026, 1, 4))", n(2.0)),
        ("WEEKNUM(DATE(2026, 1, 4), 2)", n(1.0)),
        ("WEEKNUM(DATE(2026, 1, 1), 21)", n(1.0)),
        ("WEEKNUM(DATE(2027, 1, 1), 21)", n(53.0)),
        ("WEEKNUM(DATE(2026, 10, 6), 21)", n(41.0)),
        ("EDATE(DATE(2026, 1, 31), 1)", d(2026, 2, 28)),
        ("EDATE(DATE(2024, 1, 31), 1)", d(2024, 2, 29)),
        ("EDATE(DATE(2026, 3, 15), -3)", d(2025, 12, 15)),
        ("EOMONTH(DATE(2026, 1, 15), 0)", d(2026, 1, 31)),
        ("EOMONTH(DATE(2026, 1, 15), 1)", d(2026, 2, 28)),
        ("EOMONTH(DATE(2026, 1, 15), -2)", d(2025, 11, 30)),
        ("DATEDIF(DATE(2020, 5, 10), DATE(2026, 3, 9), \"Y\")", n(5.0)),
        ("DATEDIF(DATE(2020, 5, 10), DATE(2026, 3, 9), \"M\")", n(69.0)),
        ("DATEDIF(DATE(2020, 5, 10), DATE(2020, 6, 9), \"D\")", n(30.0)),
        ("DATEDIF(DATE(2020, 5, 10), DATE(2026, 3, 9), \"YM\")", n(9.0)),
        ("DATEDIF(DATE(2020, 5, 10), DATE(2026, 3, 9), \"MD\")", n(27.0)),
        ("DATEDIF(DATE(2020, 5, 10), DATE(2026, 3, 9), \"YD\")", n(303.0)),
        ("DATEDIF(DATE(2026, 1, 1), DATE(2020, 1, 1), \"D\")", err(Num)),
        ("DATEDIF(1, 2, \"Q\")", err(Num)),
        ("DAYS(DATE(2026, 3, 1), DATE(2026, 2, 1))", n(28.0)),
        ("HOUR(0.75)", n(18.0)),
        ("MINUTE(TIME(14, 35, 50))", n(35.0)),
        ("SECOND(TIME(14, 35, 50))", n(50.0)),
        ("HOUR(\"14:30\")", n(14.0)),
        ("TIME(12, 0, 0)", n(0.5)),
        ("TIME(25, 0, 0)", n(1.0 / 24.0)),
        ("TIME(0, 90, 0)", n(1.5 / 24.0)),
        ("TIME(-1, 0, 0)", err(Num)),
        ("NETWORKDAYS(DATE(2026, 10, 5), DATE(2026, 10, 16))", n(10.0)),
        ("NETWORKDAYS(DATE(2026, 10, 3), DATE(2026, 10, 4))", n(0.0)),
        ("NETWORKDAYS(DATE(2026, 10, 5), DATE(2026, 10, 16), DATE(2026, 10, 7))", n(9.0)),
        ("NETWORKDAYS(DATE(2026, 10, 16), DATE(2026, 10, 5))", n(-10.0)),
        ("WORKDAY(DATE(2026, 10, 9), 1)", d(2026, 10, 12)),
        ("WORKDAY(DATE(2026, 10, 5), 10)", d(2026, 10, 19)),
        ("WORKDAY(DATE(2026, 10, 12), -1)", d(2026, 10, 9)),
        ("WORKDAY(DATE(2026, 10, 9), 1, DATE(2026, 10, 12))", d(2026, 10, 13)),
        ("DATEVALUE(\"2025-10-06\")", n(45936.0)),
        ("DATEVALUE(\"6 Oct 2025 14:00\")", n(45936.0)),
        ("DATEVALUE(\"nope\")", err(Value)),
        ("DATEVALUE(5)", err(Value)),
    ]);
    let e = fixture();
    let today = e.evaluate(0, a("K1"), "TODAY()").as_number().unwrap();
    let now = e.evaluate(0, a("K1"), "NOW()").as_number().unwrap();
    assert!(today > 46_000.0 && today.fract() == 0.0);
    assert!(now >= today && now < today + 1.0);
}

#[test]
fn information() {
    use ErrorKind::*;
    check(&[
        ("ISBLANK(H1)", b(true)),
        ("ISBLANK(G3)", b(false)),
        ("ISBLANK(A1)", b(false)),
        ("ISNUMBER(A1)", b(true)),
        ("ISNUMBER(G1)", b(false)),
        ("ISTEXT(G1)", b(true)),
        ("ISTEXT(A1)", b(false)),
        ("ISERROR(F1)", b(true)),
        ("ISERROR(A1)", b(false)),
        ("ISERR(NA())", b(false)),
        ("ISERR(F1)", b(true)),
        ("ISNA(NA())", b(true)),
        ("ISNA(F1)", b(false)),
        ("ISLOGICAL(B5)", b(true)),
        ("ISLOGICAL(1)", b(false)),
        ("NA()", err(NA)),
        ("SUM(ISNUMBER(B1:B5)*1)", n(3.0)),
    ]);
}

#[test]
fn financial() {
    use ErrorKind::*;
    check(&[
        ("PMT(0.05/12, 360, 200000)", n(-1073.643_246_024_277_9)),
        ("PMT(0, 10, 1000)", n(-100.0)),
        ("PMT(0.1, 0, 1000)", err(Num)),
        ("PMT(0.08/12, 10, 10000, 0, 1)", n(-1030.164_327_177_788_5)),
        ("FV(0.06/12, 10, -200, -500, 1)", n(2581.403_374_000_686)),
        ("FV(0, 12, -100)", n(1200.0)),
        ("PV(0.08/12, 12*20, 500)", n(-59_777.145_851_187_83)),
        ("NPV(0.1, -10000, 3000, 4200, 6800)", n(1188.443_412_335_769_7)),
        ("IRR({-70000, 12000, 15000, 18000, 21000, 26000})", n(0.086_630_948_036_531_3)),
        ("IRR({1, 2, 3})", err(Num)),
        ("RATE(48, -200, 8000)", n(0.007_701_472_488_246_008)),
        ("NPER(0.01, -100, 1000)", n(10.588_644_459_354_75)),
        ("NPER(0, -100, 1000)", n(10.0)),
    ]);
}

#[test]
fn unknown_and_arity() {
    use ErrorKind::*;
    check(&[
        ("NOSUCHFUNCTION(1)", err(Name)),
        ("foo + 1", err(Name)),
        ("SUM(", err(Name)),
        ("IF()", err(Value)),
        ("_xlfn.XLOOKUP(20, D1:D4, E1:E4)", t("twenty")),
        ("sum(a1:a2)", n(3.0)),
    ]);
}
