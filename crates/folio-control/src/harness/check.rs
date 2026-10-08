//! Objective checks on the open file (`harness.check`): what an expert would catch at a glance
//! and an agent should fix before it says it is done.
//!
//! * Sheets: cells showing formula errors (with the cell that causes them), totals that leave out
//!   the rows just above them, empty cells inside a table or inside a range a formula adds up,
//!   numbers too wide for their column (`###`).
//! * Documents: skipped heading levels, empty headings, bold Normal lines that look like
//!   headings, placeholder text left behind.
//! * Decks: text that runs out of its box, shapes off the slide, empty titles and placeholders,
//!   slides with too many bullets.
//! * Everywhere: live links that no longer resolve.
//!
//! Errors are wrong (a formula error, text cut off); warnings are probably wrong.

use std::collections::BTreeSet;

use folio_calc::{Addr, Range, Value};
use folio_core::sheet::Sheet;
use folio_core::text::{Block, ParaStyle};
use folio_core::{Document, PageBody, ShapeKind};
use folio_layout::Fonts;
use serde::Serialize;

/// Most problems listed per page (the counts stay exact).
const PER_PAGE: usize = 40;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// The page's name.
    pub page: String,
    /// `error` or `warning`.
    pub severity: &'static str,
    /// What kind: `formulaError`, `totalMissesRows`, `blankInRange`, `holeInTable`,
    /// `narrowColumn`, `brokenLink`, `headingSkip`, `emptyHeading`, `fakeHeading`, `placeholder`,
    /// `overflow`, `offSlide`, `emptyTitle`, `emptyPlaceholder`, `tooManyBullets`, `emptyPage`.
    pub kind: &'static str,
    /// Where: a cell (`B7`), a block (`block 4`), a slide (`slide 3`)…
    #[serde(rename = "where")]
    pub at: String,
    pub message: String,
}

/// What `harness.check` found.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub ok: bool,
    pub errors: usize,
    pub warnings: usize,
    pub problems: Vec<Problem>,
}

impl Report {
    /// One line: "2 errors, 1 warning on Budget and Slides" or "No problems".
    pub fn summary(&self) -> String {
        if self.problems.is_empty() {
            return "No problems found".into();
        }
        let pages: BTreeSet<&str> = self.problems.iter().map(|p| p.page.as_str()).collect();
        let pages: Vec<String> = pages.into_iter().map(|p| format!("\"{p}\"")).collect();
        format!("{} error{}, {} warning{} on {}", self.errors, plural(self.errors), self.warnings, plural(self.warnings), pages.join(", "))
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Checks every page, or only `page` (an index).
pub fn check(doc: &Document, page: Option<usize>) -> Report {
    let mut fonts = Fonts::shared();
    let mut problems = vec![];
    for (i, p) in doc.pages.iter().enumerate() {
        if page.is_some_and(|only| only != i) {
            continue;
        }
        let mut found = match &p.body {
            PageBody::Sheet(s) => sheet(&mut fonts, &p.name, s),
            PageBody::Doc(d) => document(&p.name, d),
            PageBody::Deck(d) => deck(&mut fonts, &p.name, d),
        };
        found.truncate(PER_PAGE);
        problems.extend(found);
    }
    drop(fonts);
    let only = page.map(|i| doc.pages[i].name.as_str());
    for (on, what, link) in folio_core::links::all(doc) {
        if only.is_some_and(|n| n != on) {
            continue;
        }
        if let Err(e) = folio_core::links::resolve(doc, &link) {
            problems.push(Problem { page: on, severity: "error", kind: "brokenLink", at: what, message: format!("The live link {link} doesn't resolve: {}", e.0) });
        }
    }
    // Errors first, each page's order kept.
    problems.sort_by_key(|p| p.severity != "error");
    let errors = problems.iter().filter(|p| p.severity == "error").count();
    Report { ok: errors == 0, errors, warnings: problems.len() - errors, problems }
}

// ---- sheets ------------------------------------------------------------------------------

fn sheet(fonts: &mut Fonts, name: &str, s: &Sheet) -> Vec<Problem> {
    let mut out = vec![];
    let problem = |severity, kind, at: String, message: String| Problem { page: name.to_string(), severity, kind, at, message };
    // Errors, the cells that cause them first.
    let errors: Vec<(Addr, &folio_core::sheet::Cell)> = s.cells.iter().filter(|(_, c)| matches!(c.value, Value::Error(_))).map(|(a, c)| (*a, c)).collect();
    let error_cells: BTreeSet<Addr> = errors.iter().map(|(a, _)| *a).collect();
    let mut causes = vec![];
    let mut follow = vec![];
    for (a, c) in &errors {
        let refs = folio_calc::references(&c.input);
        let from: Vec<String> = refs
            .iter()
            .filter(|r| r.sheet.as_deref().is_none_or(|n| n.eq_ignore_ascii_case(name)))
            .flat_map(|r| error_cells.iter().filter(|e| r.range.contains(**e) && *e != a).map(|e| e.a1()))
            .take(3)
            .collect();
        if from.is_empty() {
            causes.push(problem("error", "formulaError", a.a1(), format!("{} shows {} ({}): fix this cell first.", a.a1(), c.display(), shown_input(&c.input))));
        } else {
            follow.push(problem("error", "formulaError", a.a1(), format!("{} shows {} because {} does.", a.a1(), c.display(), from.join(", "))));
        }
    }
    out.extend(causes);
    out.extend(follow);
    // Formulas: totals that stop short, ranges with empty cells.
    let mut seen_ranges = BTreeSet::new();
    for (a, c) in s.cells.iter().filter(|(_, c)| c.is_formula()) {
        let upper = c.input.to_ascii_uppercase();
        let adds_up = ["SUM(", "AVERAGE(", "COUNT(", "SUMIF", "AVERAGEIF", "COUNTIF", "MIN(", "MAX(", "PRODUCT(", "SUMPRODUCT(", "MEDIAN("].iter().any(|f| upper.contains(f));
        if !adds_up {
            continue;
        }
        for r in folio_calc::references(&c.input) {
            if r.sheet.as_deref().is_some_and(|n| !n.eq_ignore_ascii_case(name)) || r.range.is_whole_cols() || r.range.is_whole_rows() || r.range.area() > 5000 || r.range.area() < 2 {
                continue;
            }
            let range = r.range;
            // A one-column range ending above the formula, with numbers in between: they are left out.
            if range.cols() == 1 && range.start.col == a.col && range.end.row + 1 < a.row {
                let missed: Vec<Addr> = (range.end.row + 1..a.row).map(|row| Addr::new(row, a.col)).filter(|m| s.cell(*m).is_some_and(|mc| !mc.is_formula() && matches!(mc.value, Value::Number(_)))).collect();
                if !missed.is_empty() && missed.len() == (a.row - range.end.row - 1) as usize {
                    out.push(problem(
                        "warning",
                        "totalMissesRows",
                        a.a1(),
                        format!("{} ({}) adds up {} but leaves out {} just above it.", a.a1(), shown_input(&c.input), range.a1(), list(&missed)),
                    ));
                }
            }
            // The same for a one-row range ending left of the formula.
            if range.rows() == 1 && range.start.row == a.row && range.end.col + 1 < a.col {
                let missed: Vec<Addr> = (range.end.col + 1..a.col).map(|col| Addr::new(a.row, col)).filter(|m| s.cell(*m).is_some_and(|mc| !mc.is_formula() && matches!(mc.value, Value::Number(_)))).collect();
                if !missed.is_empty() && missed.len() == (a.col - range.end.col - 1) as usize {
                    out.push(problem("warning", "totalMissesRows", a.a1(), format!("{} ({}) adds up {} but leaves out {} just left of it.", a.a1(), shown_input(&c.input), range.a1(), list(&missed))));
                }
            }
            if !seen_ranges.insert(range.a1()) {
                continue;
            }
            let blanks: Vec<Addr> = range.iter().filter(|x| s.cell(*x).is_none_or(|cc| cc.input.trim().is_empty())).collect();
            let filled = range.area() as usize - blanks.len();
            if !blanks.is_empty() && filled >= 2 && blanks.len() * 2 < range.area() as usize {
                out.push(problem("warning", "blankInRange", a.a1(), format!("{} adds up {}, which has {} empty cell{}: {}. Fill them or say why they are empty.", a.a1(), range.a1(), blanks.len(), plural(blanks.len()), list(&blanks))));
            }
        }
    }
    out.extend(holes(s).into_iter().map(|(a, header)| problem("warning", "holeInTable", a.a1(), format!("{} is empty inside the table (column \"{header}\", filled above and below).", a.a1()))));
    if let Some(used) = s.used_range() {
        let narrow = folio_layout::sheet_narrow_cells(fonts, s, used);
        let cols: BTreeSet<u32> = narrow.iter().map(|a| a.col).collect();
        for col in cols {
            let cells: Vec<Addr> = narrow.iter().filter(|a| a.col == col).copied().collect();
            out.push(problem(
                "warning",
                "narrowColumn",
                folio_calc::col_name(col),
                format!("Column {} is too narrow for its numbers ({} show ###): sheet.resize fit={}:{}.", folio_calc::col_name(col), list(&cells), folio_calc::col_name(col), folio_calc::col_name(col)),
            ));
        }
    }
    out
}

fn shown_input(input: &str) -> String {
    let one: String = input.chars().take(60).collect();
    if one.len() < input.len() { format!("{one}…") } else { one }
}

/// Up to five cells, then a count.
fn list(cells: &[Addr]) -> String {
    let mut s: Vec<String> = cells.iter().take(5).map(Addr::a1).collect();
    if cells.len() > 5 {
        s.push(format!("and {} more", cells.len() - 5));
    }
    s.join(", ")
}

/// Empty cells inside a table: blocks of rows separated by empty rows, columns with a header in
/// the block's first row, a cell empty while cells above and below it in its column are not.
fn holes(s: &Sheet) -> Vec<(Addr, String)> {
    let Some(used) = s.used_range() else { return vec![] };
    if used.area() > 200_000 {
        return vec![];
    }
    let filled = |a: Addr| s.cell(a).is_some_and(|c| !c.input.trim().is_empty());
    let row_empty = |row: u32| (used.start.col..=used.end.col).all(|col| !filled(Addr::new(row, col)));
    let mut out = vec![];
    let mut row = used.start.row;
    while row <= used.end.row {
        if row_empty(row) {
            row += 1;
            continue;
        }
        let start = row;
        while row <= used.end.row && !row_empty(row) {
            row += 1;
        }
        let end = row - 1;
        if end < start + 2 {
            continue;
        }
        for col in used.start.col..=used.end.col {
            let head = Addr::new(start, col);
            if !filled(head) {
                continue;
            }
            let header = s.display(head);
            let rows: Vec<u32> = (start + 1..=end).collect();
            let last = rows.iter().rposition(|r| filled(Addr::new(*r, col)));
            let Some(last) = last else { continue };
            for r in &rows[..last] {
                let a = Addr::new(*r, col);
                if !filled(a) && (start + 1..*r).any(|above| filled(Addr::new(above, col))) {
                    out.push((a, header.clone()));
                }
            }
        }
    }
    out
}

// ---- documents ---------------------------------------------------------------------------

/// Text a writer leaves to fill in later.
const PLACEHOLDERS: &[&str] = &["lorem ipsum", "[todo", "todo:", "tbd:", "xxx", "click to add", "insert text here", "[your ", "[insert"];

fn placeholder_in(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    PLACEHOLDERS.iter().find(|p| lower.contains(*p)).copied()
}

fn rank(style: ParaStyle) -> Option<u8> {
    match style {
        ParaStyle::Title => Some(0),
        ParaStyle::Heading1 => Some(1),
        ParaStyle::Heading2 => Some(2),
        ParaStyle::Heading3 => Some(3),
        _ => None,
    }
}

fn document(name: &str, d: &folio_core::text::TextDoc) -> Vec<Problem> {
    let mut out = vec![];
    let problem = |severity, kind, at: String, message: String| Problem { page: name.to_string(), severity, kind, at, message };
    if d.blocks.iter().all(|b| b.plain().trim().is_empty() && matches!(b, Block::Paragraph(_))) {
        out.push(problem("warning", "emptyPage", "page".into(), format!("The document \"{name}\" is empty.")));
        return out;
    }
    let mut last: Option<u8> = None;
    let blocks: Vec<&Block> = d.blocks.iter().collect();
    for (i, b) in blocks.iter().enumerate() {
        let Block::Paragraph(p) = b else { continue };
        let text = p.text();
        if let Some(r) = rank(p.style) {
            if text.trim().is_empty() {
                out.push(problem("warning", "emptyHeading", format!("block {i}"), format!("Block {i} is an empty {} paragraph.", style_name(p.style))));
                continue;
            }
            if let Some(prev) = last
                && r > prev + 1
                && r > 1
            {
                out.push(problem("warning", "headingSkip", format!("block {i}"), format!("\"{}\" (block {i}) is a {} right after a {}: a level is skipped.", short(&text), style_name(p.style), level_name(prev))));
            }
            last = Some(r);
        } else if p.style == ParaStyle::Normal && p.list.is_none() {
            let words = text.split_whitespace().count();
            let all_bold = !p.runs.is_empty() && p.runs.iter().filter(|r| !r.text.trim().is_empty()).all(|r| r.style.bold);
            let next_is_body = blocks.get(i + 1).and_then(|n| n.para()).is_some_and(|n| n.style == ParaStyle::Normal && !n.text().trim().is_empty() && !n.runs.iter().all(|r| r.style.bold));
            if all_bold && (1..=10).contains(&words) && !text.trim_end().ends_with(['.', ':', '!', '?']) && next_is_body {
                out.push(problem("warning", "fakeHeading", format!("block {i}"), format!("\"{}\" (block {i}) is bold Normal text that reads like a heading: give it a heading style.", short(&text))));
            }
        }
        if let Some(p) = placeholder_in(&text) {
            out.push(problem("warning", "placeholder", format!("block {i}"), format!("Block {i} still holds placeholder text (\"{p}\"): \"{}\".", short(&text))));
        }
    }
    for (i, b) in blocks.iter().enumerate() {
        if let Block::Table(t) = b
            && t.link.is_none()
            && let Some(p) = t.rows.iter().flatten().find_map(|c| placeholder_in(&c.plain()))
        {
            out.push(problem("warning", "placeholder", format!("block {i}"), format!("The table at block {i} still holds placeholder text (\"{p}\").")));
        }
    }
    out
}

fn style_name(s: ParaStyle) -> &'static str {
    match s {
        ParaStyle::Title => "Title",
        ParaStyle::Heading1 => "Heading 1",
        ParaStyle::Heading2 => "Heading 2",
        ParaStyle::Heading3 => "Heading 3",
        _ => "paragraph",
    }
}

fn level_name(rank: u8) -> &'static str {
    match rank {
        0 => "Title",
        1 => "Heading 1",
        2 => "Heading 2",
        _ => "Heading 3",
    }
}

fn short(text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = one.chars().take(60).collect();
    if s.len() < one.len() { format!("{s}…") } else { s }
}

// ---- decks -------------------------------------------------------------------------------

/// More body paragraphs than this on one slide is a wall of text.
const MAX_BULLETS: usize = 7;

fn deck(fonts: &mut Fonts, name: &str, d: &folio_core::Deck) -> Vec<Problem> {
    let mut out = vec![];
    let problem = |severity, kind, at: String, message: String| Problem { page: name.to_string(), severity, kind, at, message };
    if d.slides.is_empty() {
        out.push(problem("warning", "emptyPage", "deck".into(), format!("The deck \"{name}\" has no slides.")));
    }
    let [sw, sh] = d.size;
    for (si, slide) in d.slides.iter().enumerate() {
        let n = si + 1;
        if slide.hidden {
            continue;
        }
        for shape in &slide.shapes {
            let label = shape_label(shape);
            let (below, beside) = folio_layout::text_overflow(fonts, d, shape);
            if below > 1.0 {
                out.push(problem("error", "overflow", format!("slide {n}"), format!("On slide {n}, the text of {label} runs {below:.0} pt below its box: shorten it, split the slide, or make the box taller.")));
            }
            if beside > 0.0 {
                out.push(problem("error", "overflow", format!("slide {n}"), format!("On slide {n}, a word in {label} is {beside:.0} pt wider than its box.")));
            }
            let slack = 2.0;
            if shape.rotation == 0.0 && (shape.x < -slack || shape.y < -slack || shape.x + shape.w > sw + slack || shape.y + shape.h > sh + slack) {
                out.push(problem(
                    "error",
                    "offSlide",
                    format!("slide {n}"),
                    format!("On slide {n}, {label} goes past the edge of the slide (x {:.0}, y {:.0}, {:.0} × {:.0} on a {sw:.0} × {sh:.0} slide).", shape.x, shape.y, shape.w, shape.h),
                ));
            }
            if let Some(ph) = shape.placeholder.as_deref()
                && matches!(shape.kind, ShapeKind::Text)
                && shape.plain().trim().is_empty()
            {
                if ph == "title" {
                    out.push(problem("warning", "emptyTitle", format!("slide {n}"), format!("Slide {n} has no title: say its takeaway in the title.")));
                } else {
                    out.push(problem("warning", "emptyPlaceholder", format!("slide {n}"), format!("Slide {n} has an empty {ph} placeholder: fill it or remove it (deck.removeShape shape={}).", if shape.name.is_empty() { shape.id.to_string() } else { shape.name.clone() })));
                }
            }
            if shape.placeholder.as_deref() == Some("body") {
                let bullets = shape.text.iter().filter(|b| !b.plain().trim().is_empty()).count();
                if bullets > MAX_BULLETS {
                    out.push(problem("warning", "tooManyBullets", format!("slide {n}"), format!("Slide {n} has {bullets} lines of body text: keep it to six or fewer, or split the slide.")));
                }
            }
            if let Some(p) = placeholder_in(&shape.plain()) {
                out.push(problem("warning", "placeholder", format!("slide {n}"), format!("On slide {n}, {label} still holds placeholder text (\"{p}\").")));
            }
        }
        if slide.layout != folio_core::deck::SlideLayout::Blank && !slide.shapes.iter().any(|s| s.placeholder.as_deref() == Some("title")) && !slide.shapes.iter().any(|s| !s.plain().trim().is_empty()) {
            out.push(problem("warning", "emptyTitle", format!("slide {n}"), format!("Slide {n} has no title and no text.")));
        }
    }
    out
}

fn shape_label(s: &folio_core::deck::Shape) -> String {
    let what = match &s.kind {
        ShapeKind::Text => "text box",
        k => k.id(),
    };
    if s.name.is_empty() { format!("the {what} {}", s.id) } else { format!("the {what} \"{}\"", s.name) }
}

/// The range of `s` to look at by default: its used range, grown to cover its charts, at most
/// `max_cols` × `max_rows`.
pub fn default_range(s: &Sheet, max_cols: u32, max_rows: u32) -> Option<Range> {
    // The cell under a point of the grid (pixels from A1's corner).
    let cell_at = |x: f32, y: f32| {
        let (mut left, mut col) = (0.0, 0);
        while left + s.col_width(col) < x && col < 200 {
            left += s.col_width(col);
            col += 1;
        }
        let (mut top, mut row) = (0.0, 0);
        while top + s.row_height(row) < y && row < 2000 {
            top += s.row_height(row);
            row += 1;
        }
        Addr::new(row, col)
    };
    let mut r = s.used_range();
    for ch in &s.charts {
        let box_ = Range::new(cell_at(ch.x, ch.y), cell_at(ch.x + ch.w, ch.y + ch.h));
        r = Some(match r {
            Some(r) => Range::new(Addr::new(r.start.row.min(box_.start.row), r.start.col.min(box_.start.col)), Addr::new(r.end.row.max(box_.end.row), r.end.col.max(box_.end.col))),
            None => box_,
        });
    }
    r.map(|r| Range::new(r.start, Addr::new(r.end.row.min(r.start.row + max_rows - 1), r.end.col.min(r.start.col + max_cols - 1))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::PageKind;

    fn doc_with_sheet(cells: &[(&str, &str)]) -> Document {
        let mut d = Document::new("t");
        let i = d.add_page(PageKind::Sheet, Some("Budget"), None).unwrap();
        let s = d.page_mut(i).sheet_mut().unwrap();
        for (a, v) in cells {
            s.set_input(Addr::parse(a).unwrap(), v);
        }
        let mut ed = folio_core::Editor::new(d);
        ed.recalc_all();
        ed.doc().clone()
    }

    #[test]
    fn finds_errors_short_totals_and_holes() {
        let d = doc_with_sheet(&[
            ("A1", "Item"), ("B1", "Cost"),
            ("A2", "Rent"), ("B2", "1200"),
            ("A3", "Food"), ("B3", ""),
            ("A4", "Fun"), ("B4", "50"),
            ("A5", "Bus"), ("B5", "30"),
            ("A6", "Total"), ("B6", "=SUM(B2:B4)"),
            ("C2", "=B2/0"), ("C3", "=C2*2"),
        ]);
        let r = check(&d, None);
        let kinds: Vec<&str> = r.problems.iter().map(|p| p.kind).collect();
        assert!(!r.ok);
        assert_eq!(r.errors, 2, "{:#?}", r.problems);
        assert!(r.problems[0].message.contains("fix this cell first") && r.problems[0].at == "C2");
        assert!(r.problems[1].message.contains("because C2 does"));
        assert!(kinds.contains(&"totalMissesRows"), "{kinds:?}");
        assert!(kinds.contains(&"blankInRange"));
        assert!(kinds.contains(&"holeInTable"));
        assert!(r.summary().contains("2 errors"));
    }

    #[test]
    fn a_clean_sheet_passes() {
        let d = doc_with_sheet(&[("A1", "Item"), ("B1", "Cost"), ("A2", "Rent"), ("B2", "1200"), ("A3", "Food"), ("B3", "300"), ("A4", "Total"), ("B4", "=SUM(B2:B3)")]);
        let sheet = d.pages.iter().position(|p| p.name == "Budget");
        let r = check(&d, sheet);
        assert!(r.ok && r.problems.is_empty(), "{:#?}", r.problems);
        assert_eq!(r.summary(), "No problems found");
    }

    #[test]
    fn finds_document_and_deck_problems() {
        let mut d = Document::new("t");
        let i = d.add_page(PageKind::Doc, Some("Report"), None).unwrap();
        let t = d.page_mut(i).doc_mut().unwrap();
        t.blocks = vec![
            Block::Paragraph(folio_core::Paragraph::new(ParaStyle::Title, "Report")),
            Block::Paragraph(folio_core::Paragraph::new(ParaStyle::Heading3, "Too deep")),
            Block::Paragraph(folio_core::Paragraph::with_runs(ParaStyle::Normal, vec![folio_core::Run::bold("Results")])),
            Block::Paragraph(folio_core::Paragraph::new(ParaStyle::Normal, "Lorem ipsum dolor sit amet.")),
        ]
        .into();
        let k = d.add_page(PageKind::Deck, Some("Slides"), None).unwrap();
        let deck = d.page_mut(k).deck_mut().unwrap();
        let long = (0..40).map(|n| format!("A long bullet line number {n} that keeps going")).collect::<Vec<_>>().join("\n");
        deck.slides = vec![folio_core::deck::Slide::with_layout(folio_core::deck::SlideLayout::TitleContent, deck.size, "", &long)];
        let r = check(&d, None);
        let kinds: Vec<&str> = r.problems.iter().map(|p| p.kind).collect();
        for k in ["headingSkip", "fakeHeading", "placeholder", "overflow", "emptyTitle", "tooManyBullets"] {
            assert!(kinds.contains(&k), "{k} in {kinds:?}");
        }
        assert!(r.problems.iter().any(|p| p.kind == "overflow" && p.severity == "error"));
        // One page only.
        let only = check(&d, Some(k));
        assert!(only.problems.iter().all(|p| p.page == "Slides"));
    }
}
