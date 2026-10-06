//! Templates for `file.new`: ready-made files that show what folio does, each built with the
//! same model functions the commands use.

use folio_calc::Addr;
use folio_core::deck::{Shape, ShapeKind, Slide, SlideLayout};
use folio_core::text::{ChartBlock, PageSetup, Table};
use folio_core::{Block, Chart, ChartKind, Document, Id, PageKind};
use serde_json::{Value, json};

use crate::session::CmdResult;

pub fn list() -> Value {
    json!([
        { "id": "report", "name": "Report", "kinds": ["doc"], "description": "A titled report with headings, a table and notes." },
        { "id": "letter", "name": "Letter", "kinds": ["doc"], "description": "A one-page letter." },
        { "id": "budget", "name": "Budget", "kinds": ["sheet"], "description": "Monthly budget with totals, percentages and a chart." },
        { "id": "invoice", "name": "Invoice", "kinds": ["sheet"], "description": "Invoice lines with tax and total." },
        { "id": "pitch", "name": "Pitch deck", "kinds": ["deck"], "description": "Eight slides: problem, solution, market, plan." },
        { "id": "review", "name": "Quarterly review", "kinds": ["doc", "sheet", "deck"], "description": "One file, three kinds of page: the numbers in a sheet, a report whose table and chart read them live, and slides with the same chart." },
    ])
}

fn set(d: &mut Document, page: usize, rows: &[(&str, &[&str])]) {
    let sh = d.page_mut(page).sheet_mut().unwrap();
    for (start, values) in rows {
        let a = Addr::parse(start).unwrap();
        for (i, v) in values.iter().enumerate() {
            sh.set_input(Addr::new(a.row, a.col + i as u32), v);
        }
    }
}

fn md(d: &mut Document, page: usize, markdown: &str) {
    let blocks = folio_io::markdown::to_blocks(markdown);
    let t = d.page_mut(page).doc_mut().unwrap();
    t.blocks = blocks.into_iter().collect();
    folio_core::text::ensure_nonempty(&mut t.blocks);
}

fn bold_row(d: &mut Document, page: usize, range: &str, fill: Option<&str>) {
    let r = folio_calc::Range::parse(range).unwrap();
    let sh = d.page_mut(page).sheet_mut().unwrap();
    sh.format(r, &folio_core::sheet::FormatPatch { bold: Some(true), fill: fill.map(str::to_string), border: Some("b".into()), ..Default::default() });
}

fn number(d: &mut Document, page: usize, range: &str, code: &str) {
    let r = folio_calc::Range::parse(range).unwrap();
    d.page_mut(page).sheet_mut().unwrap().format(r, &folio_core::sheet::FormatPatch { number: Some(code.into()), ..Default::default() });
}

pub fn build(id: &str, title: &str, paper: &str) -> CmdResult<Document> {
    let mut d = Document::empty(title);
    let setup = if paper == "letter" { PageSetup::letter() } else { PageSetup::a4() };
    match id {
        "report" => {
            let p = d.add_page(PageKind::Doc, Some("Report"), None).unwrap();
            d.page_mut(p).doc_mut().unwrap().setup = setup;
            md(&mut d, p, &format!("# {title}\n\nA short summary of what this report says, in two or three sentences.\n\n## Background\n\nWhat led here, and what the reader needs to know first.\n\n## Findings\n\n- The first thing we found\n- The second thing, with **one number** that matters\n- What surprised us\n\n| Area | Before | After |\n| --- | --- | --- |\n| Speed | 12 s | 4 s |\n| Cost | $1,200 | $800 |\n\n## Next steps\n\n1. Decide on the plan\n2. Share it with the team\n3. Review in a month\n\n> A quote or a note that deserves to stand out.\n"));
        }
        "letter" => {
            let p = d.add_page(PageKind::Doc, Some("Letter"), None).unwrap();
            d.page_mut(p).doc_mut().unwrap().setup = PageSetup { footer: String::new(), ..setup };
            md(&mut d, p, "Your name\n\nYour address\n\n6 October 2026\n\nDear reader,\n\nThe reason for this letter, in one paragraph.\n\nThe details, and what you would like to happen next.\n\nKind regards,\n\nYour name\n");
        }
        "budget" => {
            let s = d.add_page(PageKind::Sheet, Some("Budget"), None).unwrap();
            set(&mut d, s, &[
                ("A1", &["Category", "Planned", "Actual", "Difference", "Share"]),
                ("A2", &["Rent", "1200", "1200", "=B2-C2", "=C2/$C$9"]),
                ("A3", &["Groceries", "450", "512.40", "=B3-C3", "=C3/$C$9"]),
                ("A4", &["Transport", "120", "96.50", "=B4-C4", "=C4/$C$9"]),
                ("A5", &["Utilities", "180", "171.20", "=B5-C5", "=C5/$C$9"]),
                ("A6", &["Going out", "200", "264", "=B6-C6", "=C6/$C$9"]),
                ("A7", &["Savings", "500", "500", "=B7-C7", "=C7/$C$9"]),
                ("A9", &["Total", "=SUM(B2:B7)", "=SUM(C2:C7)", "=SUM(D2:D7)", "=SUM(E2:E7)"]),
            ]);
            bold_row(&mut d, s, "A1:E1", Some("#e9e9e9"));
            bold_row(&mut d, s, "A9:E9", None);
            number(&mut d, s, "B2:D9", "#,##0.00");
            number(&mut d, s, "E2:E9", "0.0%");
            let sh = d.page_mut(s).sheet_mut().unwrap();
            sh.cols.insert(0, 130.0);
            sh.freeze_rows = 1;
            sh.charts.push(folio_core::sheet::SheetChart { id: Id::new(), chart: Chart { title: "Planned and actual".into(), ..Chart::new(ChartKind::Column, "Budget!A1:C7") }, x: 520.0, y: 0.0, w: 460.0, h: 290.0 });
        }
        "invoice" => {
            let s = d.add_page(PageKind::Sheet, Some("Invoice"), None).unwrap();
            set(&mut d, s, &[
                ("A1", &["Invoice", "", "", "No. 2026-014"]),
                ("A2", &["Date", "=TODAY()"]),
                ("A4", &["Item", "Quantity", "Price", "Amount"]),
                ("A5", &["Design work (hours)", "12", "85", "=B5*C5"]),
                ("A6", &["Illustrations", "4", "140", "=B6*C6"]),
                ("A7", &["Printing", "1", "320", "=B7*C7"]),
                ("C9", &["Subtotal", "=SUM(D5:D7)"]),
                ("C10", &["Tax 20 %", "=ROUND(D9*0.2, 2)"]),
                ("C11", &["Total", "=D9+D10"]),
            ]);
            bold_row(&mut d, s, "A4:D4", Some("#e9e9e9"));
            bold_row(&mut d, s, "C11:D11", None);
            number(&mut d, s, "C5:D11", "$#,##0.00");
            number(&mut d, s, "B2", "d mmm yyyy");
            let sh = d.page_mut(s).sheet_mut().unwrap();
            sh.format(folio_calc::Range::parse("A1").unwrap(), &folio_core::sheet::FormatPatch { bold: Some(true), size: Some(18.0), ..Default::default() });
            sh.cols.insert(0, 180.0);
        }
        "pitch" => {
            let p = d.add_page(PageKind::Deck, Some("Pitch"), None).unwrap();
            let size = d.pages[p].deck().unwrap().size;
            let slides = vec![
                Slide::with_layout(SlideLayout::Title, size, title, "What we do, in one line"),
                Slide::with_layout(SlideLayout::TitleContent, size, "The problem", "Who has it\nHow often\nWhat it costs them"),
                Slide::with_layout(SlideLayout::TitleContent, size, "Our answer", "What we built\nWhy it works\nWhat makes it different"),
                Slide::with_layout(SlideLayout::Section, size, "How it works", "A short demo"),
                Slide::with_layout(SlideLayout::TwoContent, size, "Market", "Who buys\nHow many\n\nWho else sells\nWhy we win"),
                Slide::with_layout(SlideLayout::TitleContent, size, "Plan", "Next three months\nNext year\nWhat we need"),
                Slide::with_layout(SlideLayout::TitleContent, size, "Team", "Who we are\nWhat we've done"),
                Slide::with_layout(SlideLayout::Title, size, "Thank you", "you@example.com"),
            ];
            d.page_mut(p).deck_mut().unwrap().slides = slides;
        }
        "review" => {
            let s = d.add_page(PageKind::Sheet, Some("Numbers"), None).unwrap();
            set(&mut d, s, &[
                ("A1", &["Month", "Revenue", "Costs", "Profit", "Margin"]),
                ("A2", &["July", "42000", "31500", "=B2-C2", "=D2/B2"]),
                ("A3", &["August", "45500", "32800", "=B3-C3", "=D3/B3"]),
                ("A4", &["September", "51200", "34100", "=B4-C4", "=D4/B4"]),
                ("A5", &["Quarter", "=SUM(B2:B4)", "=SUM(C2:C4)", "=SUM(D2:D4)", "=D5/B5"]),
            ]);
            bold_row(&mut d, s, "A1:E1", Some("#e9e9e9"));
            bold_row(&mut d, s, "A5:E5", None);
            number(&mut d, s, "B2:D5", "$#,##0");
            number(&mut d, s, "E2:E5", "0.0%");
            d.page_mut(s).sheet_mut().unwrap().cols.insert(0, 120.0);
            let r = d.add_page(PageKind::Doc, Some("Report"), Some(0)).unwrap();
            d.page_mut(r).doc_mut().unwrap().setup = setup;
            md(&mut d, r, &format!("# {title}\n\nThe third quarter in one page. The table and the chart below read the **Numbers** sheet live: change a figure there and they follow.\n\n## Results\n"));
            let mut t = Table::from_text(vec![], true);
            t.link = Some("Numbers!A1:E5".into());
            let flow = &mut d.page_mut(r).doc_mut().unwrap().blocks;
            flow.push_back(Block::Table(t));
            flow.push_back(Block::Chart(ChartBlock { id: Id::new(), chart: Chart { title: "Revenue and costs".into(), ..Chart::new(ChartKind::Column, "Numbers!A1:C4") }, height: 220.0 }));
            for b in folio_io::markdown::to_blocks("## What changed\n\n- Revenue grew every month\n- Costs grew more slowly\n- The margin is the best of the year\n") {
                flow.push_back(b);
            }
            let k = d.add_page(PageKind::Deck, Some("Slides"), None).unwrap();
            let size = d.pages[k].deck().unwrap().size;
            let mut chart_slide = Slide::with_layout(SlideLayout::TitleOnly, size, "Revenue and costs", "");
            chart_slide.shapes.push(Shape::new(ShapeKind::Chart { chart: Chart::new(ChartKind::Line, "Numbers!A1:C4") }, 72.0, 150.0, 816.0, 340.0));
            let mut table_slide = Slide::with_layout(SlideLayout::TitleOnly, size, "The quarter", "");
            let mut lt = Table::from_text(vec![], true);
            lt.link = Some("Numbers!A1:E5".into());
            let mut ts = Shape::new(ShapeKind::Table { table: lt }, 72.0, 160.0, 816.0, 230.0);
            ts.text_size = 18.0;
            table_slide.shapes.push(ts);
            d.page_mut(k).deck_mut().unwrap().slides = vec![
                Slide::with_layout(SlideLayout::Title, size, title, "Third quarter, 2026"),
                table_slide,
                chart_slide,
                Slide::with_layout(SlideLayout::TitleContent, size, "Next quarter", "Keep costs flat\nLaunch the new plan\nHire two people"),
            ];
        }
        other => return Err(format!("Unknown template \"{other}\" (file.templates lists them).")),
    }
    // Compute the formulas before the file is shown.
    let mut calc = folio_core::recalc::Calc::new();
    calc.sync(&mut d);
    Ok(d)
}
