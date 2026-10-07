use folio_calc::Addr;
use folio_core::deck::VAlign;
use folio_core::text::Table;
use folio_core::{Align, Block, Chart, ChartKind, Deck, Document, ListKind, PageKind, ParaStyle, Paragraph, Run, RunStyle, Shape, ShapeKind, Slide, SlideLayout};

const DOT: &[u8] = include_bytes!("../../tests/fixtures/dot.png");
const DECK: &[u8] = include_bytes!("../../tests/fixtures/deck.pptx");
const DECK_LO: &[u8] = include_bytes!("../../tests/fixtures/deck-lo.pptx");

pub(crate) fn sample_doc() -> Document {
    let mut doc = Document::empty("Pitch");
    let s = doc.add_page(PageKind::Sheet, Some("Numbers"), None).unwrap();
    {
        let sh = doc.page_mut(s).sheet_mut().unwrap();
        for (c, v) in [("A1", "Quarter"), ("B1", "Sales"), ("C1", "Costs"), ("A2", "Q1"), ("B2", "10"), ("C2", "7"), ("A3", "Q2"), ("B3", "14"), ("C3", "8")] {
            sh.set_input(Addr::parse(c).unwrap(), v);
        }
    }
    folio_core::recalc::Calc::new().sync(&mut doc);
    let media = doc.add_media("dot.png", DOT.to_vec());
    let d = doc.add_page(PageKind::Deck, Some("Slides"), None).unwrap();
    let mut deck = Deck { theme: folio_core::DeckTheme::named("grain").unwrap(), ..Default::default() };
    let size = deck.size;
    let mut s1 = Slide::with_layout(SlideLayout::Title, size, "Quarterly review", "October 2026");
    s1.notes = "Welcome everyone.\nThen the numbers.".into();
    let mut s2 = Slide::with_layout(SlideLayout::TitleContent, size, "Highlights", "Revenue up\nNew office");
    if let ShapeKind::Text = s2.shapes[1].kind {
        let mut p = Paragraph::with_runs(ParaStyle::Normal, vec![Run::bold("Bold "), Run::styled("red", RunStyle { color: Some("#c00000".into()), italic: true, size: Some(14.0), ..Default::default() })]);
        p.align = Align::Center;
        s2.shapes[1].text.push_back(Block::Paragraph(p));
        s2.shapes[1].text.push_back(Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Nested").list(ListKind::Number, 1)));
    }
    let mut s3 = Slide::with_layout(SlideLayout::Blank, size, "", "");
    let mut rect = Shape::new(ShapeKind::Rect, 40.0, 40.0, 200.0, 100.0);
    rect.fill = Some("#1f4e78".into());
    rect.line = Some("#000000".into());
    rect.line_width = 2.0;
    rect.text = std::iter::once(Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Boxed").align(Align::Center))).collect();
    rect.valign = VAlign::Middle;
    rect.color = Some("#ffffff".into());
    rect.text_size = 18.0;
    rect.name = "Box".into();
    s3.shapes.push(rect);
    let mut oval = Shape::new(ShapeKind::Ellipse, 300.0, 40.0, 120.0, 120.0);
    oval.fill = Some("#c8291c".into());
    oval.rotation = 30.0;
    s3.shapes.push(oval);
    let mut line = Shape::new(ShapeKind::Line, 40.0, 200.0, 300.0, 0.0);
    line.line = Some("#141414".into());
    line.line_width = 1.5;
    s3.shapes.push(line);
    let mut arrow = Shape::new(ShapeKind::Arrow, 40.0, 240.0, 300.0, 40.0);
    arrow.line = Some("#141414".into());
    arrow.line_width = 2.0;
    s3.shapes.push(arrow);
    s3.shapes.push(Shape::new(ShapeKind::Image { media }, 500.0, 40.0, 64.0, 64.0));
    let mut t = Table::from_text(vec![vec!["Name".into(), "Score".into()], vec!["Ann".into(), "3".into()]], true);
    t.rows[1][1].fill = Some("#ffeeaa".into());
    s3.shapes.push(Shape::new(ShapeKind::Table { table: t }, 40.0, 300.0, 300.0, 80.0));
    let mut linked = Table::from_text(vec![], true);
    linked.link = Some("Numbers!A1:C3".into());
    s3.shapes.push(Shape::new(ShapeKind::Table { table: linked }, 400.0, 300.0, 300.0, 80.0));
    let mut chart = Chart::new(ChartKind::Column, "Numbers!A1:C3");
    chart.title = "Sales".into();
    s3.shapes.push(Shape::new(ShapeKind::Chart { chart }, 600.0, 120.0, 320.0, 200.0));
    s3.shapes.push(Shape::text_box(40.0, 420.0, 400.0, 40.0, "A mono line", 14.0));
    if let Block::Paragraph(p) = &mut s3.shapes.last_mut().unwrap().text[0] {
        p.runs[0].style.font = Some("mono".into());
        p.runs[0].style.link = Some("https://lsuite.xyz".into());
    }
    s3.background = Some("#f4f1ea".into());
    let mut s4 = Slide::with_layout(SlideLayout::TitleOnly, size, "Backup", "");
    s4.hidden = true;
    deck.slides = vec![s1, s2, s3, s4];
    *doc.page_mut(d).deck_mut().unwrap() = deck;
    doc
}

#[test]
fn round_trip_keeps_slides() {
    let doc = sample_doc();
    let (bytes, warnings) = super::export(&doc, &[0, 1]).unwrap();
    assert!(warnings.iter().any(|w| w.contains("\"Numbers\"")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("PowerPoint charts")), "{warnings:?}");
    let back = super::import(&bytes, "Pitch").unwrap();
    let deck = back.doc.pages.iter().find_map(|p| p.deck()).unwrap();
    let orig = doc.pages[1].deck().unwrap();
    assert_eq!(deck.size, orig.size);
    assert_eq!(deck.theme.background, orig.theme.background);
    assert_eq!(deck.theme.text, orig.theme.text);
    assert_eq!(deck.theme.accent, orig.theme.accent);
    assert_eq!(deck.theme.heading_font, "display");
    assert_eq!(deck.slides.len(), 4);
    let layouts: Vec<SlideLayout> = deck.slides.iter().map(|s| s.layout).collect();
    assert_eq!(layouts, [SlideLayout::Title, SlideLayout::TitleContent, SlideLayout::Blank, SlideLayout::TitleOnly]);
    let s1 = &deck.slides[0];
    assert_eq!(s1.title(), "Quarterly review");
    assert_eq!(s1.notes, "Welcome everyone.\nThen the numbers.");
    let sub = s1.shapes.iter().find(|s| s.placeholder.as_deref() == Some("subtitle")).unwrap();
    assert_eq!(sub.plain(), "October 2026");
    let o = &orig.slides[0].shapes[0];
    let t = &s1.shapes[0];
    assert!((t.x - o.x).abs() < 0.1 && (t.y - o.y).abs() < 0.1 && (t.w - o.w).abs() < 0.1 && (t.h - o.h).abs() < 0.1, "{t:?}");
    // Title text keeps its size: Title style (28 pt) at text size 22 → 56 pt.
    assert!((t.text_size - 28.0 * 22.0 / 11.0).abs() < 0.1, "{}", t.text_size);
    let s2 = &deck.slides[1];
    let body = &s2.shapes[1];
    assert_eq!(body.placeholder.as_deref(), Some("body"));
    let paras: Vec<&Paragraph> = body.text.iter().filter_map(Block::para).collect();
    assert_eq!(paras[0].text(), "Revenue up");
    assert_eq!(paras[0].list, Some(ListKind::Bullet));
    assert_eq!(paras[2].align, Align::Center);
    assert!(paras[2].runs[0].style.bold);
    let red = &paras[2].runs[1];
    assert_eq!(red.style.color.as_deref(), Some("#c00000"));
    assert!(red.style.italic);
    assert_eq!(red.style.size, Some(14.0));
    assert_eq!(paras[3].list, Some(ListKind::Number));
    assert_eq!(paras[3].level, 1);
    let s3 = &deck.slides[2];
    assert_eq!(s3.background.as_deref(), Some("#f4f1ea"));
    let kinds: Vec<&str> = s3.shapes.iter().map(|s| s.kind.id()).collect();
    assert_eq!(kinds, ["rect", "ellipse", "line", "arrow", "image", "table", "table", "chart", "text"]);
    let rect = &s3.shapes[0];
    assert_eq!(rect.name, "Box");
    assert_eq!(rect.fill.as_deref(), Some("#1f4e78"));
    assert_eq!(rect.line.as_deref(), Some("#000000"));
    assert!((rect.line_width - 2.0).abs() < 0.01);
    assert_eq!(rect.valign, VAlign::Middle);
    assert_eq!(rect.color.as_deref(), Some("#ffffff"));
    assert_eq!(rect.plain(), "Boxed");
    assert!((rect.text_size - 18.0).abs() < 0.01);
    assert!((s3.shapes[1].rotation - 30.0).abs() < 0.01);
    assert!(matches!(&s3.shapes[4].kind, ShapeKind::Image { media } if back.doc.media[media].bytes.as_slice() == DOT));
    let ShapeKind::Table { table } = &s3.shapes[5].kind else { panic!() };
    assert_eq!(table.rows[1][0].plain(), "Ann");
    assert_eq!(table.rows[1][1].fill.as_deref(), Some("#ffeeaa"));
    assert!(table.header);
    let ShapeKind::Table { table: linked } = &s3.shapes[6].kind else { panic!() };
    assert_eq!(linked.rows[2][1].plain(), "14");
    let ShapeKind::Chart { chart } = &s3.shapes[7].kind else { panic!() };
    assert_eq!(chart.kind, ChartKind::Column);
    assert_eq!(chart.title, "Sales");
    let data = folio_core::links::chart_data(&back.doc, chart).unwrap();
    assert_eq!(data.categories, ["Q1", "Q2"]);
    assert_eq!(data.series[1].name, "Costs");
    assert_eq!(data.series[0].values, [Some(10.0), Some(14.0)]);
    let mono = &s3.shapes[8];
    let run = &mono.text[0].para().unwrap().runs[0];
    assert_eq!(run.style.font.as_deref(), Some("mono"));
    assert_eq!(run.style.link.as_deref(), Some("https://lsuite.xyz"));
    assert!(deck.slides[3].hidden);
    assert!(!deck.slides[2].hidden);
}

#[test]
fn python_pptx_deck() {
    let im = super::import(DECK, "deck").unwrap();
    let deck = im.doc.pages[0].deck().unwrap();
    assert_eq!(deck.slides.len(), 4);
    assert!((deck.size[0] - 960.0).abs() < 0.1 && (deck.size[1] - 540.0).abs() < 0.1, "{:?}", deck.size);
    let s1 = &deck.slides[0];
    assert_eq!(s1.layout, SlideLayout::Title);
    assert_eq!(s1.title(), "Quarterly review");
    assert_eq!(s1.notes, "Welcome everyone.");
    // The title's position comes from the layout (python-pptx's default 4:3 template).
    let t = s1.shapes.iter().find(|s| s.placeholder.as_deref() == Some("title")).unwrap();
    assert!((t.x - 54.0).abs() < 0.5 && (t.y - 167.75).abs() < 0.5, "{t:?}");
    assert!((t.text_size - 44.0).abs() < 0.1, "{}", t.text_size);
    assert_eq!(t.text[0].para().unwrap().align, Align::Center);
    let s2 = &deck.slides[1];
    assert_eq!(s2.layout, SlideLayout::TitleContent);
    let body = s2.shapes.iter().find(|s| s.placeholder.as_deref() == Some("body")).unwrap();
    let paras: Vec<&Paragraph> = body.text.iter().filter_map(Block::para).collect();
    assert_eq!(paras[0].list, Some(ListKind::Bullet));
    assert_eq!(paras[1].level, 1);
    assert!((body.text_size - 32.0).abs() < 0.1);
    let red = &paras[2].runs[1];
    assert_eq!(red.style.color.as_deref(), Some("#c00000"));
    assert!(red.style.italic);
    assert!((red.style.size.unwrap() - 28.0 * 11.0 / 32.0).abs() < 0.01);
    let s3 = &deck.slides[2];
    assert_eq!(s3.background.as_deref(), Some("#f4f1ea"));
    let kinds: Vec<&str> = s3.shapes.iter().map(|s| s.kind.id()).collect();
    assert_eq!(kinds, ["rect", "ellipse", "arrow", "text", "image", "table", "chart"]);
    assert_eq!(s3.shapes[0].fill.as_deref(), Some("#1f4e78"));
    assert_eq!(s3.shapes[0].color.as_deref(), Some("#ffffff"));
    // The oval's fill comes from its style (accent 1 of the theme).
    assert_eq!(s3.shapes[1].fill.as_deref(), Some("#4f81bd"));
    assert!((s3.shapes[1].rotation - 30.0).abs() < 0.01);
    assert_eq!(s3.shapes[3].text[0].para().unwrap().runs[0].style.font.as_deref(), Some("mono"));
    let ShapeKind::Chart { chart } = &s3.shapes[6].kind else { panic!() };
    let data = folio_core::links::chart_data(&im.doc, chart).unwrap();
    assert_eq!(data.categories, ["Q1", "Q2", "Q3"]);
    assert_eq!(data.series[1].values, [Some(7.0), Some(8.0), Some(9.0)]);
    assert!(deck.slides[3].hidden);
    assert!(im.doc.pages.iter().any(|p| p.name == "Charts data"));
}

#[test]
fn libreoffice_deck() {
    let im = super::import(DECK_LO, "deck").unwrap();
    let deck = im.doc.pages[0].deck().unwrap();
    assert_eq!(deck.slides.len(), 4);
    assert_eq!(deck.slides[0].title(), "Quarterly review");
    assert!(deck.slides[1].shapes.iter().any(|s| s.plain().contains("Revenue up 12%")));
    assert!(deck.slides[2].shapes.iter().any(|s| matches!(s.kind, ShapeKind::Image { .. })));
}

#[test]
fn every_part_parses_and_is_declared() {
    let doc = sample_doc();
    let (bytes, _) = super::export(&doc, &[1]).unwrap();
    let mut pkg = crate::xlsx::package::Package::open(&bytes).unwrap();
    let names: Vec<String> = pkg.names().to_vec();
    let ct = pkg.xml("[Content_Types].xml").unwrap();
    for n in &names {
        if n.ends_with(".xml") || n.ends_with(".rels") {
            let b = pkg.read(n).unwrap();
            crate::xlsx::package::parse_xml(&b).unwrap_or_else(|e| panic!("{n}: {e}"));
        }
        let ext = n.rsplit('.').next().unwrap();
        let declared = ct.elements().any(|e| e.attr("PartName") == Some(&format!("/{n}")) || e.attr("Extension") == Some(ext));
        assert!(declared, "{n} has no content type");
        // Every relationship points at a part that exists.
        if n.ends_with(".rels") {
            let part = n.replace("_rels/", "").trim_end_matches(".rels").to_string();
            for r in pkg.rels(&part) {
                assert!(r.external || names.contains(&r.target), "{n}: {} is missing", r.target);
            }
        }
    }
    // Shape ids are unique on each slide.
    for n in names.iter().filter(|n| n.starts_with("ppt/slides/slide")) {
        let x = pkg.xml(n).unwrap();
        let ids: Vec<String> = x.all("cNvPr").iter().filter_map(|c| c.attr("id").map(str::to_string)).collect();
        let set: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(set.len(), ids.len(), "{n}");
    }
}

/// Writes sample files to look at in other apps: `FOLIO_IO_OUT=/tmp/x cargo test -p folio-io
/// write_samples -- --ignored`.
#[test]
#[ignore]
fn write_samples() {
    let Ok(dir) = std::env::var("FOLIO_IO_OUT") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let doc = sample_doc();
    let (bytes, _) = super::export(&doc, &[1]).unwrap();
    std::fs::write(format!("{dir}/sample.pptx"), bytes).unwrap();
    let im = super::import(DECK, "deck").unwrap();
    let (bytes, _) = super::export(&im.doc, &[0]).unwrap();
    std::fs::write(format!("{dir}/deck-again.pptx"), bytes).unwrap();
}
