//! The live context (HARNESS.md part 3): what the person is looking at, what the file holds,
//! what is wrong in it and what the person changed, given to an agent before every model step.
//!
//! "Make this bold", "sum these", "add a slide after this one": the words point at the window.
//! [`glance`] is the cheap part (the file, the page shown, the caret or the selected cells or the
//! slide), which the Agent panel also shows in one line above the composer. [`context`] adds
//! each page's size, the open problems ([`super::check`]) and the person's own changes since the
//! agent's last step, so an agent never works from a stale picture of the file. The full state
//! stays one command away (`file.overview`).

use folio_core::{Document, Page, PageBody};

use crate::session::{Session, UiState};

/// Most pages and shapes listed by name; the rest are counted.
const LISTED: usize = 8;

/// The person's view of folio at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glance {
    /// The `<context>` block's lines.
    pub lines: Vec<String>,
    /// One short line for the panel ("Budget · B2:D9").
    pub short: String,
}

impl Glance {
    /// The context block, then the request.
    pub fn frame(&self, prompt: &str) -> String {
        if self.lines.is_empty() {
            return prompt.to_string();
        }
        format!("<context>\n{}\n</context>\n\n{prompt}", self.lines.join("\n"))
    }
}

/// `text` without the context blocks [`Glance::frame`] put in front of it.
pub fn unframed(text: &str) -> &str {
    match text.strip_prefix("<context>\n").and_then(|rest| rest.split_once("\n</context>\n\n")) {
        Some((_, prompt)) => prompt,
        None => text,
    }
}

/// What the person sees now.
pub fn glance(session: &Session) -> Glance {
    let ui = session.ui_state();
    // Under the lock, without copying the file: the panel asks on every redraw.
    session
        .read(|ed| of(ed.doc(), &ui))
        .unwrap_or_else(|_| Glance { lines: vec!["No file is open (file_new or file_open makes one).".into()], short: "No file open".into() })
}

fn kind_name(p: &Page) -> &'static str {
    match p.body {
        PageBody::Doc(_) => "document",
        PageBody::Sheet(_) => "sheet",
        PageBody::Deck(_) => "deck",
    }
}

fn of(doc: &Document, ui: &UiState) -> Glance {
    let mut lines = vec!["What the person sees in folio as they ask (\"this\", \"here\" and \"the selection\" mean these; file_overview has the rest):".to_string()];
    let listed: Vec<String> = doc.pages.iter().take(LISTED).map(|p| format!("{} ({})", quoted(&p.name), kind_name(p))).collect();
    let more = doc.pages.len().saturating_sub(LISTED);
    lines.push(format!(
        "File {}: {} page{}: {}{}.",
        quoted(&doc.title),
        doc.pages.len(),
        plural(doc.pages.len()),
        listed.join(", "),
        if more > 0 { format!(" and {more} more") } else { String::new() }
    ));
    let page = ui.page.as_deref().and_then(|key| doc.page(key).ok());
    let short = match page {
        None => {
            if ui.screen == "home" {
                lines.push("The window shows the home screen.".into());
            }
            quoted(&doc.title)
        }
        Some(p) => {
            let name = quoted(&p.name);
            match &p.body {
                PageBody::Doc(d) => {
                    let blocks = d.blocks.len();
                    match &ui.text {
                        Some(sel) if sel.anchor != sel.focus => {
                            let (a, b) = if (sel.anchor.block, sel.anchor.offset) <= (sel.focus.block, sel.focus.offset) { (sel.anchor, sel.focus) } else { (sel.focus, sel.anchor) };
                            let text = folio_core::text::plain(&d.blocks, a, b);
                            lines.push(format!(
                                "Showing document {name} ({blocks} block{}). Selected: block {} offset {} to block {} offset {}: {}.",
                                plural(blocks),
                                a.block,
                                a.offset,
                                b.block,
                                b.offset,
                                quoted(&text)
                            ));
                            format!("{name} · {} selected", count_words(&text))
                        }
                        Some(sel) => {
                            lines.push(format!("Showing document {name} ({blocks} block{}). The caret is in block {} at offset {}.", plural(blocks), sel.focus.block, sel.focus.offset));
                            format!("{name} · block {}", sel.focus.block)
                        }
                        None => {
                            lines.push(format!("Showing document {name} ({blocks} block{}).", plural(blocks)));
                            name
                        }
                    }
                }
                PageBody::Sheet(_) => {
                    let range = ui.range.as_deref().filter(|r| r.contains(':'));
                    match (range, ui.cell.as_deref()) {
                        (Some(r), _) => {
                            lines.push(format!("Showing sheet {name}. Selected range: {r}{}.", ui.cell.as_deref().map(|c| format!(" (active cell {c})")).unwrap_or_default()));
                            format!("{name} · {r}")
                        }
                        (None, Some(c)) => {
                            lines.push(format!("Showing sheet {name}. Active cell: {c}."));
                            format!("{name} · {c}")
                        }
                        (None, None) => {
                            lines.push(format!("Showing sheet {name}."));
                            name
                        }
                    }
                }
                PageBody::Deck(d) => {
                    let n = d.slides.len();
                    match ui.slide.filter(|s| *s < n) {
                        Some(i) => {
                            let shapes: Vec<&str> = ui.shapes.iter().map(String::as_str).take(LISTED).collect();
                            lines.push(format!(
                                "Showing deck {name}, slide {} of {n} (id {}){}{}.",
                                i + 1,
                                d.slides[i].id,
                                if shapes.is_empty() { String::new() } else { format!(", selected shapes: {}", shapes.join(", ")) },
                                if ui.presenting { ", presenting" } else { "" }
                            ));
                            format!("{name} · slide {}{}", i + 1, if shapes.is_empty() { String::new() } else { format!(" · {} shape{}", ui.shapes.len(), plural(ui.shapes.len())) })
                        }
                        None => {
                            lines.push(format!("Showing deck {name} ({n} slide{}).", plural(n)));
                            name
                        }
                    }
                }
            }
        }
    };
    let panels: Vec<&str> = ui.open.iter().map(String::as_str).filter(|o| *o != "agent").collect();
    if !panels.is_empty() {
        lines.push(format!("Also open: {}.", panels.join(", ")));
    }
    Glance { lines, short }
}

fn count_words(text: &str) -> String {
    let n = text.split_whitespace().count();
    format!("{n} word{}", plural(n))
}

/// A name on one line, quoted, at most 60 characters.
fn quoted(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = one.chars().take(60).collect();
    format!("\"{short}{}\"", if short.len() < one.len() { "…" } else { "" })
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}


/// The live context: [`glance`], then each page's size, open problems and the person's changes.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Context {
    /// The `<context>` block's lines.
    pub lines: Vec<String>,
    /// One short line ("Budget · B2:D9").
    pub short: String,
    /// The newest undo step's number: pass it back as `since` next time.
    pub seq: u64,
}

impl Context {
    /// The block, as it goes in front of a request or after a step's results.
    pub fn block(&self) -> String {
        format!("<context>\n{}\n</context>", self.lines.join("\n"))
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// The block, then the request (as [`Glance::frame`]; [`unframed`] takes it off again).
    pub fn frame(&self, prompt: &str) -> String {
        if self.lines.is_empty() { prompt.to_string() } else { format!("{}\n\n{prompt}", self.block()) }
    }
}

/// Most of the person's changes listed by name.
const CHANGES: usize = 6;

/// The live context now. `since`: the `seq` of the last context the agent got; the person's
/// changes after it are listed.
pub fn context(session: &Session, since: Option<u64>) -> Context {
    let g = glance(session);
    let mut lines = g.lines;
    let detail = session.read(|ed| {
        let doc = ed.doc();
        let sizes: Vec<String> = doc.pages.iter().take(LISTED).map(|p| format!("{} {}", quoted(&p.name), size_of(p))).collect();
        let steps = ed.history().undo_list();
        let seq = steps.first().map(|s| s.seq).unwrap_or(0);
        let mine: Vec<String> = match since {
            Some(since) => steps.iter().take_while(|s| s.seq > since).filter(|s| s.source == "window").map(|s| s.label.clone()).collect(),
            None => vec![],
        };
        (sizes, seq, mine, doc.clone())
    });
    let Ok((sizes, seq, mine, doc)) = detail else {
        return Context { lines, short: g.short, seq: 0 };
    };
    if !sizes.is_empty() {
        lines.push(format!("Pages: {}.", sizes.join("; ")));
    }
    let report = super::check::check(&doc, None);
    if !report.problems.is_empty() {
        lines.push(format!("Open problems: {} (harness.check lists them).", report.summary()));
    }
    if !mine.is_empty() {
        let mut named: Vec<String> = mine.iter().take(CHANGES).cloned().collect();
        if mine.len() > CHANGES {
            named.push(format!("{} more", mine.len() - CHANGES));
        }
        lines.push(format!("The person changed the file since your last step ({}): read what you rely on again.", named.join(", ")));
    }
    Context { lines, short: g.short, seq }
}

/// A page's size in a few words.
fn size_of(p: &Page) -> String {
    match &p.body {
        PageBody::Doc(d) => {
            let headings = d.outline().len();
            format!("(document, {} words, {} heading{}, {} block{})", d.word_count(), headings, plural(headings), d.blocks.len(), plural(d.blocks.len()))
        }
        PageBody::Sheet(s) => {
            let formulas = s.cells.values().filter(|c| c.is_formula()).count();
            let charts = s.charts.len();
            format!(
                "(sheet, {}, {} formula{}{})",
                s.used_range().map(|r| r.a1()).unwrap_or_else(|| "empty".into()),
                formulas,
                plural(formulas),
                if charts > 0 { format!(", {charts} chart{}", plural(charts)) } else { String::new() }
            )
        }
        PageBody::Deck(d) => format!("(deck, {} slide{})", d.slides.len(), plural(d.slides.len())),
    }
}
