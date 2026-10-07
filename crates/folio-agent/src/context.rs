//! What the person is looking at when they ask, sent with each request.
//!
//! "Make this bold", "sum these", "add a slide after this one": the words point at the window.
//! Each request to the agent starts with a short `<context>` block (the file, the page shown,
//! the caret or the selected cells or the slide), so the agent knows what "this" is without a
//! round trip, and the panel shows the same thing in one line above the composer. It is part of
//! the person's message, so the thread is only ever appended to (prompt caches and thinking
//! blocks stay valid).

use folio_control::Session;
use folio_control::session::UiState;
use folio_core::{Document, Page, PageBody};

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
