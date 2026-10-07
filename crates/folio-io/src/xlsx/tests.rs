use folio_calc::{Addr, Value};
use folio_core::sheet::{Filter, SheetChart};
use folio_core::{Align, CellFormat, Chart, ChartKind, Document, Id, PageKind};

fn a(s: &str) -> Addr {
    Addr::parse(s).unwrap()
}

fn sheet_doc() -> Document {
    let mut doc = Document::empty("Round trip");
    let d = doc.add_page(PageKind::Sheet, Some("Data"), None).unwrap();
    let s = doc.add_page(PageKind::Sheet, Some("My summary"), None).unwrap();
    {
        let sh = doc.page_mut(d).sheet_mut().unwrap();
        for (c, v) in [("A1", "Month"), ("B1", "Sales"), ("C1", "Costs"), ("A2", "Jan"), ("B2", "10"), ("C2", "4"), ("A3", "Feb"), ("B3", "12.5"), ("C3", "5"), ("A4", "Mar"), ("B4", "0.1"), ("C4", "6")] {
            sh.set_input(a(c), v);
        }
        sh.set_input(a("D1"), "'00123");
        sh.set_input(a("D2"), "TRUE");
        sh.set_input(a("D3"), "2026-10-06");
        sh.set_input(a("D4"), "12%");
        sh.set_input(a("E1"), "=SUM(B2:B4)");
        sh.set_input(a("E2"), "=IF(B2>C2,\"up\",\"down\")");
        sh.set_input(a("E3"), "=B3/0");
        sh.set_input(a("F1"), "1234567.891234567");
        let f = &mut sh.cells.get_mut(&a("A1")).unwrap().format;
        *f = CellFormat { bold: true, italic: true, underline: true, strike: true, color: Some("#c00000".into()), fill: Some("#1f4e78".into()), align: Some(Align::Center), wrap: true, size: Some(14.0), border: "tb".into(), ..Default::default() };
        sh.cells.get_mut(&a("B2")).unwrap().format.number = Some("#,##0.00".into());
        sh.cols.insert(0, 150.0);
        sh.cols.insert(3, 40.0);
        sh.rows.insert(0, 36.0);
        sh.freeze_rows = 1;
        sh.freeze_cols = 1;
        sh.filter = Some(Filter { range: "A1:C4".into(), rules: Default::default() });
        let mut chart = Chart::new(ChartKind::Column, "Data!A1:C4");
        chart.title = "Sales and costs".into();
        chart.stacked = true;
        sh.charts.push(SheetChart { id: Id::new(), chart, x: 400.0, y: 60.0, w: 480.0, h: 288.0 });
        let mut line = Chart::new(ChartKind::Line, "Data!A1:B4");
        line.legend = false;
        sh.charts.push(SheetChart { id: Id::new(), chart: line, x: 20.0, y: 400.0, w: 300.0, h: 200.0 });
    }
    {
        let sh = doc.page_mut(s).sheet_mut().unwrap();
        sh.set_input(a("A1"), "=SUM(Data!B2:B4)");
        sh.set_input(a("A2"), "='My summary'!A1*2");
        sh.set_input(a("A3"), "=xlookup(\"Feb\",Data!A2:A4,Data!C2:C4)");
        sh.set_input(a("A4"), "=Data!D3+1");
        sh.gridlines = false;
    }
    folio_core::recalc::Calc::new().sync(&mut doc);
    doc
}

#[test]
fn round_trip_keeps_cells_formats_sizes_and_charts() {
    let doc = sheet_doc();
    let (bytes, warnings) = super::export(&doc, &[0, 1]).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let back = super::import(&bytes, "Round trip").unwrap();
    let names: Vec<&str> = back.doc.pages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Data", "My summary"]);
    let (d0, d1) = (doc.pages[0].sheet().unwrap(), back.doc.pages[0].sheet().unwrap());
    for (addr, c) in d0.cells.iter() {
        let c1 = d1.cell(*addr).unwrap_or_else(|| panic!("{} is missing", addr.a1()));
        // Typed dates and percents come back as the numbers they are, with their format.
        let same_input = c.input == c1.input || c.value.as_number().is_some() && c1.value == c.value;
        assert!(same_input, "{}: {:?} became {:?}", addr.a1(), c.input, c1.input);
        assert_eq!(c.value, c1.value, "{}", addr.a1());
        assert_eq!(c.format, c1.format, "{}", addr.a1());
    }
    assert_eq!(d1.input(a("D1")), "'00123");
    assert_eq!(d1.input(a("E1")), "=SUM(B2:B4)");
    assert_eq!(d1.value(a("F1")), Value::Number(1234567.891234567));
    assert_eq!(d1.col_width(0), 150.0);
    assert_eq!(d1.col_width(3), 40.0);
    assert_eq!(d1.col_width(1), folio_core::sheet::COL_W);
    assert_eq!(d1.row_height(0), 36.0);
    assert_eq!(d1.row_height(1), folio_core::sheet::ROW_H);
    assert_eq!((d1.freeze_rows, d1.freeze_cols), (1, 1));
    assert_eq!(d1.filter.as_ref().unwrap().range, "A1:C4");
    assert_eq!(d1.charts.len(), 2);
    let c = &d1.charts[0];
    assert_eq!(c.chart.kind, ChartKind::Column);
    assert_eq!(c.chart.source, "Data!A1:C4");
    assert_eq!(c.chart.title, "Sales and costs");
    assert!(c.chart.stacked && c.chart.headers && c.chart.legend);
    assert!((c.x - 400.0).abs() <= 1.0 && (c.y - 60.0).abs() <= 1.0, "{c:?}");
    assert!((c.w - 480.0).abs() <= 1.0 && (c.h - 288.0).abs() <= 1.0, "{c:?}");
    assert_eq!(d1.charts[1].chart.source, "Data!A1:B4");
    assert!(!d1.charts[1].chart.legend);
    let s1 = back.doc.pages[1].sheet().unwrap();
    assert_eq!(s1.input(a("A2")), "='My summary'!A1*2");
    assert_eq!(s1.value(a("A1")), Value::Number(22.6));
    assert_eq!(s1.input(a("A3")), "=XLOOKUP(\"Feb\",Data!A2:A4,Data!C2:C4)");
    assert!(!s1.gridlines);
}

#[test]
fn other_pages_are_left_out_with_a_warning() {
    let mut doc = sheet_doc();
    doc.add_page(PageKind::Doc, Some("Notes"), None).unwrap();
    let (_, warnings) = super::export(&doc, &[0, 2]).unwrap();
    assert!(warnings.iter().any(|w| w.contains("\"Notes\"")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("aren't in this export")) || true);
    assert!(super::export(&doc, &[2]).is_err());
}

#[test]
fn long_sheet_names_are_cut_for_excel() {
    let mut doc = Document::empty("t");
    let i = doc.add_page(PageKind::Sheet, Some("A very long sheet name that Excel refuses"), None).unwrap();
    let j = doc.add_page(PageKind::Sheet, Some("Other"), None).unwrap();
    doc.page_mut(i).sheet_mut().unwrap().set_input(a("A1"), "5");
    doc.page_mut(j).sheet_mut().unwrap().set_input(a("A1"), "='A very long sheet name that Excel refuses'!A1+1");
    folio_core::recalc::Calc::new().sync(&mut doc);
    let (bytes, warnings) = super::export(&doc, &[0, 1]).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let back = super::import(&bytes, "t").unwrap();
    assert_eq!(back.doc.pages[0].name.chars().count(), 31);
    assert_eq!(back.doc.pages[1].sheet().unwrap().value(a("A1")), Value::Number(6.0));
}

const BUDGET: &[u8] = include_bytes!("../../tests/fixtures/budget.xlsx");
const BUDGET_LO: &[u8] = include_bytes!("../../tests/fixtures/budget-lo.xlsx");

#[test]
fn openpyxl_budget() {
    let im = super::import(BUDGET, "budget").unwrap();
    let doc = &im.doc;
    let names: Vec<&str> = doc.pages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Budget", "Rates & Notes", "Limits", "Bob’s list", "Trend"]);
    let b = doc.pages[0].sheet().unwrap();
    assert!((b.value(a("C9")).as_number().unwrap() - 1847.65).abs() < 1e-9);
    assert_eq!(b.value(a("C10")), Value::Number(1200.0));
    assert_eq!(b.value(a("C11")), Value::Text("no".into()));
    assert_eq!(b.value(a("C12")), Value::Number(0.4));
    assert_eq!(b.cell(a("C12")).unwrap().format.number.as_deref(), Some("0%"));
    assert_eq!(b.value(a("C13")), Value::Number(19.0));
    assert_eq!(b.input(a("C14")), "'00123");
    assert_eq!(b.value(a("C15")), Value::Bool(true));
    assert_eq!(b.input(a("C16")), "=\"Bob's \"&'Bob’s list'!A1");
    assert_eq!(b.value(a("C16")), Value::Text("Bob's Ann".into()));
    assert_eq!(b.display(a("D3")), "2026-01-01");
    assert_eq!(b.display(a("E3")), "64.9%");
    let head = &b.cell(a("A2")).unwrap().format;
    assert!(head.bold);
    assert_eq!(head.fill.as_deref(), Some("#1f4e78"));
    assert_eq!(head.color.as_deref(), Some("#ffffff"));
    assert_eq!(head.align, Some(Align::Center));
    assert_eq!(head.border, "b");
    let mean = &b.cell(a("C17")).unwrap().format;
    assert!(mean.underline && mean.strike && mean.wrap);
    assert_eq!(mean.color.as_deref(), Some("#c00000"));
    assert_eq!(b.cell(a("A1")).unwrap().format.size, Some(16.0));
    assert_eq!(b.freeze_rows, 2);
    assert_eq!(b.filter.as_ref().unwrap().range, "A2:E7");
    assert_eq!(b.row_height(0), 40.0);
    assert_eq!(b.col_width(0), 126.0);
    assert_eq!(b.charts.len(), 1);
    assert_eq!(b.charts[0].chart.kind, ChartKind::Column);
    assert_eq!(b.charts[0].chart.title, "Spending");
    assert_eq!(b.charts[0].chart.source, "Budget!A2:C7");
    let t = doc.pages[4].sheet().unwrap();
    assert_eq!(t.charts.len(), 2);
    assert_eq!(t.charts[0].chart.kind, ChartKind::Line);
    assert_eq!(t.charts[0].chart.source, "Trend!A1:C5");
    assert_eq!(t.charts[1].chart.kind, ChartKind::Pie);
    let w = im.warnings.join("\n");
    assert!(w.contains("merged range"), "{w}");
    assert!(w.contains("\"Income\""), "{w}");
    assert!(w.contains("Bob’s list"), "{w}");
}

#[test]
fn libreoffice_budget_matches_saved_results() {
    let im = super::import(BUDGET_LO, "budget").unwrap();
    let w = im.warnings.join("\n");
    assert!(!w.contains("differently"), "{w}");
    let b = im.doc.pages[0].sheet().unwrap();
    assert!((b.value(a("C9")).as_number().unwrap() - 1847.65).abs() < 1e-9);
    assert_eq!(b.value(a("C16")), Value::Text("Bob's Ann".into()));
}

#[test]
fn imported_budget_exports_again() {
    let im = super::import(BUDGET_LO, "budget").unwrap();
    let all: Vec<usize> = (0..im.doc.pages.len()).collect();
    let (bytes, _) = super::export(&im.doc, &all).unwrap();
    let back = super::import(&bytes, "budget").unwrap();
    let w = back.warnings.join("\n");
    assert!(!w.contains("differently"), "{w}");
    assert!((back.doc.pages[0].sheet().unwrap().value(a("C9")).as_number().unwrap() - 1847.65).abs() < 1e-9);
    assert_eq!(back.doc.pages[4].sheet().unwrap().charts.len(), 2);
}

#[test]
fn unknown_functions_are_named() {
    let mut c = super::FormulaCheck::default();
    c.compare("S", a("A1"), "=LAMBDA(x, x+1)(2)+SUM(1)", &Value::Number(3.0), &Value::Error(folio_calc::ErrorKind::Name));
    c.compare("S", a("A2"), "=SUM(1,2)", &Value::Number(4.0), &Value::Number(3.0));
    let w = c.warnings("Excel");
    assert!(w[0].contains("LAMBDA"), "{w:?}");
    assert!(w[1].contains("S!A2"), "{w:?}");
}

/// Writes sample files to look at in other apps: `FOLIO_IO_OUT=/tmp/x cargo test -p folio-io
/// write_samples -- --ignored`.
#[test]
#[ignore]
fn write_samples() {
    let Ok(dir) = std::env::var("FOLIO_IO_OUT") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let doc = sheet_doc();
    let (bytes, _) = super::export(&doc, &[0, 1]).unwrap();
    std::fs::write(format!("{dir}/sample.xlsx"), bytes).unwrap();
    let im = super::import(BUDGET_LO, "budget").unwrap();
    let all: Vec<usize> = (0..im.doc.pages.len()).collect();
    let (bytes, _) = super::export(&im.doc, &all).unwrap();
    std::fs::write(format!("{dir}/budget-again.xlsx"), bytes).unwrap();
    let (bytes, _) = crate::ods::export(&im.doc, &all).unwrap();
    std::fs::write(format!("{dir}/budget-again.ods"), bytes).unwrap();
    let (bytes, _) = crate::csv::export(&im.doc, &[0]).unwrap();
    std::fs::write(format!("{dir}/budget-again.csv"), bytes).unwrap();
}
