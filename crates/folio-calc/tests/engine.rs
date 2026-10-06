//! The engine: dependencies, recalculation order, cycles, volatile functions, sheets, custom
//! functions and speed.

use std::sync::Arc;
use std::time::Instant;

use folio_calc::{Addr, Arg, Engine, ErrorKind, FunctionInfo, Range, Value, rename_sheet};

fn a(s: &str) -> Addr {
    Addr::parse(s).unwrap()
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn engine(sheets: &[&str]) -> Engine {
    let mut e = Engine::new();
    e.set_sheets(&sheets.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    e
}

fn set(e: &mut Engine, cells: &[(&str, &str)]) {
    for (addr, input) in cells {
        e.set_cell(0, a(addr), input);
    }
}

fn v(e: &Engine, addr: &str) -> Value {
    e.value(0, a(addr))
}

#[test]
fn values_and_inputs() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("A1", "12%"), ("A2", "'007"), ("A3", "true"), ("A4", "hello"), ("A5", "=A1*100")]);
    e.recalc();
    assert_eq!(v(&e, "A1"), n(0.12));
    assert_eq!(v(&e, "A2"), Value::Text("007".into()));
    assert_eq!(v(&e, "A3"), Value::Bool(true));
    assert_eq!(v(&e, "A5"), n(12.0));
    assert_eq!(e.input(0, a("A2")), Some("'007"));
    assert_eq!(e.input(0, a("A5")), Some("=A1*100"));
    assert_eq!(e.input(0, a("Z9")), None);
    assert_eq!(v(&e, "Z9"), Value::Empty);
    e.set_cell(0, a("A1"), "");
    e.recalc();
    assert_eq!(v(&e, "A1"), Value::Empty);
    assert_eq!(v(&e, "A5"), n(0.0));
    assert_eq!(e.input(0, a("A1")), None);
}

#[test]
fn chains_update() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("A1", "1"), ("A2", "=A1+1"), ("A3", "=A2*2"), ("A4", "=A3+A2"), ("B1", "=A4&\"!\"")]);
    e.recalc();
    assert_eq!(v(&e, "A4"), n(6.0));
    assert_eq!(v(&e, "B1"), Value::Text("6!".into()));
    e.set_cell(0, a("A1"), "10");
    let changed = e.recalc();
    assert_eq!(v(&e, "A4"), n(33.0));
    assert_eq!(v(&e, "B1"), Value::Text("33!".into()));
    let names: Vec<String> = changed.iter().map(|(_, a)| a.a1()).collect();
    assert_eq!(names, ["A1", "B1", "A2", "A3", "A4"]);
    assert!(e.recalc().is_empty(), "nothing left to do");
    // Setting the same value again changes nothing downstream.
    e.set_cell(0, a("A1"), "10");
    assert!(e.recalc().is_empty());
}

#[test]
fn ranges_update() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("AA1", "=SUM(A1:A100)"), ("AA2", "=SUM(A:A)"), ("AA3", "=COUNT(1:1)"), ("AA4", "=SUM(A1:Z1000)")]);
    e.recalc();
    e.set_cell(0, a("A5"), "7");
    let changed = e.recalc();
    assert_eq!(v(&e, "AA1"), n(7.0));
    assert_eq!(v(&e, "AA2"), n(7.0));
    assert_eq!(v(&e, "AA4"), n(7.0));
    assert!(changed.contains(&(0, a("AA1"))));
    e.set_cell(0, a("A500000"), "3");
    e.recalc();
    assert_eq!(v(&e, "AA1"), n(7.0));
    assert_eq!(v(&e, "AA2"), n(10.0));
    assert_eq!(v(&e, "AA4"), n(7.0));
    e.set_cell(0, a("Q1"), "5");
    e.recalc();
    // AA3 counts row 1: AA1 (a number) and Q1.
    assert_eq!(v(&e, "AA3"), n(2.0));
    assert_eq!(v(&e, "AA4"), n(12.0));
    // A formula reading a range that holds formulas waits for them.
    set(&mut e, &[("C1", "=SUM(D1:D3)"), ("D1", "=A5*2"), ("D2", "=D1+1"), ("D3", "1")]);
    e.recalc();
    assert_eq!(v(&e, "C1"), n(30.0));
    e.set_cell(0, a("A5"), "1");
    e.recalc();
    assert_eq!(v(&e, "C1"), n(6.0));
    assert_eq!(v(&e, "AA3"), n(4.0));
}

#[test]
fn formulas_replaced_and_cleared() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("A1", "1"), ("B1", "2"), ("C1", "=A1")]);
    e.recalc();
    e.set_cell(0, a("C1"), "=B1*10");
    e.recalc();
    assert_eq!(v(&e, "C1"), n(20.0));
    // The old dependency is gone: changing A1 does not touch C1.
    e.set_cell(0, a("A1"), "5");
    let changed = e.recalc();
    assert_eq!(changed, vec![(0, a("A1"))]);
    e.set_cell(0, a("B1"), "3");
    e.recalc();
    assert_eq!(v(&e, "C1"), n(30.0));
    e.set_cell(0, a("C1"), "plain");
    e.set_cell(0, a("B1"), "4");
    e.recalc();
    assert_eq!(v(&e, "C1"), Value::Text("plain".into()));
    e.clear_sheet(0);
    let changed = e.recalc();
    assert_eq!(changed.len(), 3);
    assert_eq!(e.used_range(0), None);
}

#[test]
fn errors_propagate() {
    let mut e = engine(&["Sheet1"]);
    set(
        &mut e,
        &[
            ("A1", "=1/0"),
            ("A2", "=A1+1"),
            ("A3", "=SUM(A1:A2)"),
            ("A4", "=IFERROR(A3, -1)"),
            ("A5", "=SUM(("),
            ("A6", "=A5"),
        ],
    );
    e.recalc();
    assert_eq!(v(&e, "A2"), Value::Error(ErrorKind::Div0));
    assert_eq!(v(&e, "A3"), Value::Error(ErrorKind::Div0));
    assert_eq!(v(&e, "A4"), n(-1.0));
    assert_eq!(v(&e, "A5"), Value::Error(ErrorKind::Name));
    assert_eq!(v(&e, "A6"), Value::Error(ErrorKind::Name));
}

#[test]
fn cycles() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("A1", "=B1+1"), ("B1", "=C1+1"), ("C1", "=A1+1"), ("D1", "=A1*2"), ("E1", "=E1"), ("F1", "5")]);
    e.recalc();
    let circ = Value::Error(ErrorKind::Circular);
    for cell in ["A1", "B1", "C1", "E1"] {
        assert_eq!(v(&e, cell), circ, "{cell}");
    }
    assert_eq!(v(&e, "D1"), circ, "downstream of the cycle");
    assert_eq!(v(&e, "F1"), n(5.0));
    // Breaking the cycle recalculates everyone.
    e.set_cell(0, a("C1"), "1");
    e.recalc();
    assert_eq!(v(&e, "A1"), n(3.0));
    assert_eq!(v(&e, "B1"), n(2.0));
    assert_eq!(v(&e, "D1"), n(6.0));
    // A cycle through a range.
    set(&mut e, &[("G1", "=SUM(G2:G3)"), ("G2", "=G1")]);
    e.recalc();
    assert_eq!(v(&e, "G1"), circ);
    assert_eq!(v(&e, "G2"), circ);
}

#[test]
fn volatile_functions() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("A1", "=RAND()"), ("B1", "=A1*0+1"), ("C1", "=NOW()>40000")]);
    e.recalc();
    let first = v(&e, "A1");
    let mut moved = false;
    for _ in 0..5 {
        e.recalc();
        if v(&e, "A1") != first {
            moved = true;
        }
    }
    assert!(moved, "RAND changes on each recalculation");
    assert_eq!(v(&e, "B1"), n(1.0));
    assert_eq!(v(&e, "C1"), Value::Bool(true));
}

#[test]
fn indirect_and_offset_see_fresh_values() {
    let mut e = engine(&["Sheet1"]);
    set(
        &mut e,
        &[
            ("A1", "1"),
            ("A2", "=A1*10"),
            ("B1", "=INDIRECT(\"A2\")+1"),
            ("B2", "=SUM(OFFSET(A1, 0, 0, 2, 1))"),
            ("C1", "=B1*2"),
        ],
    );
    e.recalc();
    assert_eq!(v(&e, "B1"), n(11.0));
    assert_eq!(v(&e, "B2"), n(11.0));
    assert_eq!(v(&e, "C1"), n(22.0));
    e.set_cell(0, a("A1"), "2");
    e.recalc();
    assert_eq!(v(&e, "B1"), n(21.0));
    assert_eq!(v(&e, "B2"), n(22.0));
    assert_eq!(v(&e, "C1"), n(42.0));
}

#[test]
fn cross_sheet_references() {
    let mut e = engine(&["Sales", "My sheet", "Summary"]);
    e.set_cell(0, a("A1"), "100");
    e.set_cell(1, a("B2"), "5");
    e.set_cell(2, a("A1"), "=Sales!A1+'My sheet'!B2");
    e.set_cell(2, a("A2"), "=SUM(sales!A1:A3)");
    e.set_cell(2, a("A3"), "=Missing!A1");
    e.recalc();
    assert_eq!(e.value(2, a("A1")), n(105.0));
    assert_eq!(e.value(2, a("A2")), n(100.0));
    assert_eq!(e.value(2, a("A3")), Value::Error(ErrorKind::Ref));
    e.set_cell(0, a("A2"), "1");
    e.recalc();
    assert_eq!(e.value(2, a("A2")), n(101.0));
    assert_eq!(e.precedents(2, a("A1")), vec![(0, Range::parse("A1").unwrap()), (1, Range::parse("B2").unwrap())]);
    assert_eq!(e.dependents(0, a("A1")), vec![(2, a("A1")), (2, a("A2"))]);
    assert!(e.precedents(0, a("A1")).is_empty());

    // Adding the missing sheet makes the reference work.
    e.set_sheets(&["Sales".into(), "My sheet".into(), "Summary".into(), "Missing".into()]);
    e.set_cell(3, a("A1"), "7");
    e.recalc();
    assert_eq!(e.value(2, a("A3")), n(7.0));
}

#[test]
fn sheets_renamed_moved_and_removed() {
    let mut e = engine(&["Sales", "Costs"]);
    e.set_cell(0, a("A1"), "10");
    e.set_cell(1, a("A1"), "3");
    e.set_cell(1, a("B1"), "=Sales!A1-A1");
    e.recalc();
    assert_eq!(e.value(1, a("B1")), n(7.0));

    // Rename Sales to Revenue: cells stay; the app rewrites formulas with rename_sheet.
    e.set_sheets(&["Revenue".into(), "Costs".into()]);
    let formula = e.input(1, a("B1")).unwrap().to_string();
    e.set_cell(1, a("B1"), &rename_sheet(&formula, "Sales", "Revenue"));
    e.recalc();
    assert_eq!(e.input(1, a("B1")), Some("=Revenue!A1-A1"));
    assert_eq!(e.value(1, a("B1")), n(7.0));
    assert_eq!(e.value(0, a("A1")), n(10.0));

    // Reorder: cells follow their sheet's name.
    e.set_sheets(&["Costs".into(), "Revenue".into()]);
    e.recalc();
    assert_eq!(e.value(0, a("B1")), n(7.0));
    assert_eq!(e.value(1, a("A1")), n(10.0));
    assert_eq!(e.sheet_names(), ["Costs", "Revenue"]);

    // Remove Revenue: its cells go, and the reference breaks.
    e.set_sheets(&["Costs".into()]);
    e.recalc();
    assert_eq!(e.value(0, a("B1")), Value::Error(ErrorKind::Ref));
    assert_eq!(e.value(1, a("A1")), Value::Empty);
}

#[test]
fn sheets_created_on_demand() {
    let mut e = Engine::new();
    e.set_cell(1, a("A1"), "4");
    e.set_cell(0, a("A1"), "=Sheet2!A1*2");
    e.recalc();
    assert_eq!(e.value(0, a("A1")), n(8.0));
    assert_eq!(e.sheet_names(), ["Sheet1", "Sheet2"]);
}

#[test]
fn ranges_and_used_area() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("B2", "1"), ("C4", "x"), ("D3", "=B2+1")]);
    e.recalc();
    assert_eq!(e.used_range(0), Range::parse("B2:D4"));
    let rows = e.range_values(0, Range::parse("B2:C3").unwrap());
    assert_eq!(rows, vec![vec![n(1.0), Value::Empty], vec![Value::Empty, Value::Empty]]);
    let col = e.range_values(0, Range::parse("C:C").unwrap());
    assert_eq!(col.len(), 4);
    assert_eq!(col[3], vec![Value::Text("x".into())]);
    let row = e.range_values(0, Range::parse("3:3").unwrap());
    assert_eq!(row, vec![vec![Value::Empty, Value::Empty, Value::Empty, n(2.0)]]);
    assert!(e.range_values(1, Range::parse("A:A").unwrap()).is_empty());
}

#[test]
fn evaluate_without_storing() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("A1", "2"), ("A2", "3")]);
    e.recalc();
    assert_eq!(e.evaluate(0, a("B1"), "SUM(A1:A2)*2"), n(10.0));
    assert_eq!(e.evaluate(0, a("B1"), "=A1^3"), n(8.0));
    assert_eq!(e.evaluate(0, a("B2"), "A1:A2"), n(3.0), "implicit intersection");
    assert_eq!(e.evaluate(0, a("B9"), "A1:A2"), Value::Error(ErrorKind::Value));
    assert_eq!(e.evaluate(0, a("B1"), "{1,2;3,4}"), n(1.0));
    assert_eq!(e.evaluate(0, a("B1"), "1+"), Value::Error(ErrorKind::Name));
    assert_eq!(e.input(0, a("B1")), None);
}

#[test]
fn custom_functions() {
    let mut e = engine(&["Sheet1"]);
    set(&mut e, &[("A1", "2"), ("A2", "3"), ("B1", "=DOUBLE(A1)"), ("B2", "=TOTAL(A1:A2, 10)")]);
    e.recalc();
    assert_eq!(v(&e, "B1"), Value::Error(ErrorKind::Name));
    let info = |name: &str| FunctionInfo {
        name: name.into(),
        syntax: format!("{name}(x)"),
        summary: "test".into(),
        category: "Custom".into(),
    };
    let double = Arc::new(|args: &[Arg]| match args.first() {
        Some(Arg::Value(Value::Number(x))) => Value::Number(x * 2.0),
        _ => Value::Error(ErrorKind::Value),
    });
    assert!(e.register_function(info("double"), double));
    let total = Arc::new(|args: &[Arg]| {
        let mut sum = 0.0;
        for arg in args {
            match arg {
                Arg::Value(v) => sum += v.as_number().unwrap_or(0.0),
                Arg::Range { rows, cols, values } => {
                    assert_eq!((*rows, *cols, values.len()), (2, 1, 2));
                    sum += values.iter().filter_map(Value::as_number).sum::<f64>();
                }
            }
        }
        Value::Number(sum)
    });
    assert!(e.register_function(info("Total"), total));
    assert!(!e.register_function(info("SUM"), Arc::new(|_: &[Arg]| Value::Empty)), "built-ins win");
    e.recalc();
    assert_eq!(v(&e, "B1"), n(4.0));
    assert_eq!(v(&e, "B2"), n(15.0));
    assert_eq!(e.evaluate(0, a("C1"), "SUM(1, 2)"), n(3.0));
    let names: Vec<String> = e.functions().into_iter().filter(|f| f.category == "Custom").map(|f| f.name).collect();
    assert_eq!(names, ["DOUBLE", "TOTAL"]);
    assert!(e.functions().iter().any(|f| f.name == "VLOOKUP" && f.category == "Lookup"));
    e.unregister_function("double");
    e.recalc();
    assert_eq!(v(&e, "B1"), Value::Error(ErrorKind::Name));
}

#[test]
fn whole_column_operations_stay_cheap() {
    let mut e = engine(&["Sheet1"]);
    for r in 0..100 {
        e.set_cell(0, Addr::new(r, 0), &(r + 1).to_string());
        e.set_cell(0, Addr::new(r, 1), "2");
    }
    e.set_cell(0, a("D1"), "=SUMPRODUCT(A:A, B:B)");
    e.set_cell(0, a("D2"), "=SUM(A:A*B:B)");
    e.set_cell(0, a("D3"), "=VLOOKUP(50, A:B, 2, FALSE)");
    e.set_cell(0, a("D4"), "=COUNTIF(A:A, \">90\")");
    let start = Instant::now();
    e.recalc();
    assert!(start.elapsed().as_millis() < 500);
    assert_eq!(v(&e, "D1"), n(10_100.0));
    assert_eq!(v(&e, "D2"), n(10_100.0));
    assert_eq!(v(&e, "D3"), n(2.0));
    assert_eq!(v(&e, "D4"), n(10.0));
}

/// 100 000 cells: 90 000 numbers and 10 000 formulas (each reads its row and a 7-cell range,
/// plus a running total), then single-cell edits.
fn big_sheet() -> Engine {
    let mut e = engine(&["Sheet1"]);
    for r in 0..10_000u32 {
        for c in 0..9u32 {
            e.set_cell(0, Addr::new(r, c), &((r * 9 + c) % 97).to_string());
        }
        let row = r + 1;
        let formula = if r == 0 {
            format!("=A{row}*B{row}+SUM(C{row}:I{row})")
        } else {
            format!("=A{row}*B{row}+SUM(C{row}:I{row})+J{}", row - 1)
        };
        e.set_cell(0, Addr::new(r, 9), &formula);
    }
    e
}

#[test]
fn speed_in_any_build() {
    let mut e = big_sheet();
    let start = Instant::now();
    e.recalc_all();
    let full = start.elapsed();
    // Generous bound so debug builds on a busy machine pass; see the release test below.
    assert!(full.as_millis() < 3_000, "full recalculation took {full:?}");
    let start = Instant::now();
    e.set_cell(0, a("C9000"), "1000");
    let changed = e.recalc();
    let edit = start.elapsed();
    assert!(changed.len() > 1000, "the running total ripples down");
    assert!(edit.as_millis() < 1_000, "edit took {edit:?}");
    let start = Instant::now();
    e.set_cell(0, a("K1"), "=SUM(A1:A10)");
    e.recalc();
    let small = start.elapsed();
    assert!(small.as_millis() < 50, "small edit took {small:?}");
}

/// Run with `cargo test --release -p folio-calc -- --ignored --nocapture` to check the budget:
/// a full recalculation of 100k cells with 10k formulas under 100 ms, and an edit with few
/// dependents under 1 ms.
#[test]
#[ignore]
fn speed_release_budget() {
    let mut e = big_sheet();
    let start = Instant::now();
    e.recalc_all();
    let full = start.elapsed();
    assert!(full.as_millis() < 100, "full recalculation took {full:?}");

    // An edit whose only dependent is one formula (the last row has no running total after it).
    let start = Instant::now();
    e.set_cell(0, a("C10000"), "5");
    e.recalc();
    let edit = start.elapsed();
    assert!(edit.as_micros() < 1_000, "edit took {edit:?}");

    // Typing a constant nobody reads.
    let start = Instant::now();
    e.set_cell(0, a("Z1"), "1");
    e.recalc();
    let lone = start.elapsed();
    assert!(lone.as_micros() < 1_000, "lone edit took {lone:?}");
    eprintln!("full {full:?}, edit {edit:?}, lone {lone:?}");
}
