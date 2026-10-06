//! Markdown in and out: what agents write (`doc.write`) and the `.md` format.
//!
//! In: headings (`#` title… `####`), paragraphs, **bold**, *italic*, ~~strike~~, `code`, links,
//! bullet, numbered and task lists (nested), quotes, fenced code, tables, `---` (a page break),
//! images as their alt text. Out: the same, from documents (linked tables as their values).

use folio_core::text::{Flow, Table, TableCell};
use folio_core::{Block, Document, ListKind, ParaStyle, Paragraph, Run, RunStyle};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Markdown to blocks.
pub fn to_blocks(md: &str) -> Vec<Block> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut out: Vec<Block> = vec![];
    // Formatting in effect.
    let mut style = RunStyle::default();
    // The paragraph being built.
    let mut para: Option<Paragraph> = None;
    // Lists: (ordered, level).
    let mut lists: Vec<bool> = vec![];
    let mut quote = 0usize;
    let mut code_block = false;
    let mut heading: Option<ParaStyle> = None;
    // Tables.
    let mut table: Option<Vec<Vec<TableCell>>> = None;
    let mut cell: Option<Vec<Run>> = None;
    let mut pending_check: Option<bool> = None;
    // The first # heading of a document is its title.
    let mut first_heading = true;

    let start_para = |lists: &Vec<bool>, quote: usize, heading: Option<ParaStyle>, code: bool| -> Paragraph {
        let mut p = Paragraph::default();
        if let Some(h) = heading {
            p.style = h;
        } else if code {
            p.style = ParaStyle::Code;
        } else if quote > 0 {
            p.style = ParaStyle::Quote;
        }
        if let Some(ordered) = lists.last() {
            p.list = Some(if *ordered { ListKind::Number } else { ListKind::Bullet });
            p.level = (lists.len() - 1).min(5) as u8;
        }
        p
    };
    let flush = |para: &mut Option<Paragraph>, out: &mut Vec<Block>| {
        if let Some(mut p) = para.take() {
            p.normalize();
            out.push(Block::Paragraph(p));
        }
    };

    for ev in Parser::new_ext(md, opts) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    // A list item's first paragraph is already open (empty).
                    let open_item = para.as_ref().is_some_and(|p| p.list.is_some() && p.is_empty());
                    if cell.is_none() && !open_item {
                        flush(&mut para, &mut out);
                        para = Some(start_para(&lists, quote, None, false));
                    }
                }
                Tag::Heading { level, .. } => {
                    flush(&mut para, &mut out);
                    let h = match level {
                        HeadingLevel::H1 if first_heading => ParaStyle::Title,
                        HeadingLevel::H1 => ParaStyle::Heading1,
                        HeadingLevel::H2 => ParaStyle::Heading1,
                        HeadingLevel::H3 => ParaStyle::Heading2,
                        _ => ParaStyle::Heading3,
                    };
                    first_heading = false;
                    heading = Some(h);
                    para = Some(start_para(&lists, quote, heading, false));
                }
                Tag::BlockQuote(_) => {
                    flush(&mut para, &mut out);
                    quote += 1;
                }
                Tag::CodeBlock(kind) => {
                    flush(&mut para, &mut out);
                    code_block = true;
                    let _ = matches!(kind, CodeBlockKind::Fenced(_));
                }
                Tag::List(start) => {
                    flush(&mut para, &mut out);
                    lists.push(start.is_some());
                }
                Tag::Item => {
                    flush(&mut para, &mut out);
                    para = Some(start_para(&lists, quote, None, false));
                }
                Tag::Emphasis => style.italic = true,
                Tag::Strong => style.bold = true,
                Tag::Strikethrough => style.strike = true,
                Tag::Link { dest_url, .. } => style.link = Some(dest_url.to_string()),
                Tag::Table(_) => {
                    flush(&mut para, &mut out);
                    table = Some(vec![]);
                }
                Tag::TableHead | Tag::TableRow => {
                    if let Some(t) = &mut table {
                        t.push(vec![]);
                    }
                }
                Tag::TableCell => cell = Some(vec![]),
                Tag::Image { .. } => style.italic = true,
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    if cell.is_none() && lists.is_empty() {
                        flush(&mut para, &mut out);
                    }
                }
                TagEnd::Heading(_) => {
                    flush(&mut para, &mut out);
                    heading = None;
                }
                TagEnd::BlockQuote(_) => {
                    flush(&mut para, &mut out);
                    quote = quote.saturating_sub(1);
                }
                TagEnd::CodeBlock => {
                    code_block = false;
                }
                TagEnd::List(_) => {
                    flush(&mut para, &mut out);
                    lists.pop();
                }
                TagEnd::Item => flush(&mut para, &mut out),
                TagEnd::Emphasis => style.italic = false,
                TagEnd::Strong => style.bold = false,
                TagEnd::Strikethrough => style.strike = false,
                TagEnd::Link => style.link = None,
                TagEnd::Image => style.italic = false,
                TagEnd::TableCell => {
                    if let (Some(t), Some(runs)) = (&mut table, cell.take()) {
                        let mut p = Paragraph { runs, ..Default::default() };
                        p.normalize();
                        if let Some(row) = t.last_mut() {
                            row.push(TableCell { runs: p.runs, ..Default::default() });
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(rows) = table.take() {
                        let cols = rows.iter().map(Vec::len).max().unwrap_or(1);
                        let mut t = Table::new(rows.len().max(1), cols);
                        for (r, row) in rows.into_iter().enumerate() {
                            for (c, cl) in row.into_iter().enumerate() {
                                t.rows[r][c] = cl;
                            }
                        }
                        out.push(Block::Table(t));
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if let Some(c) = &mut cell {
                    c.push(Run { text: t.to_string(), style: style.clone() });
                    continue;
                }
                if code_block {
                    // One paragraph per line of code.
                    for line in t.trim_end_matches('\n').split('\n') {
                        let mut p = start_para(&lists, quote, None, true);
                        if !line.is_empty() {
                            p.runs.push(Run::plain(line));
                        }
                        out.push(Block::Paragraph(p));
                    }
                    continue;
                }
                let p = para.get_or_insert_with(|| start_para(&lists, quote, heading, false));
                if let Some(c) = pending_check.take() {
                    p.list = Some(ListKind::Check);
                    p.checked = c;
                }
                p.runs.push(Run { text: t.to_string(), style: style.clone() });
            }
            Event::Code(t) => {
                let mut st = style.clone();
                st.code = true;
                let run = Run { text: t.to_string(), style: st };
                match &mut cell {
                    Some(c) => c.push(run),
                    None => para.get_or_insert_with(|| start_para(&lists, quote, heading, false)).runs.push(run),
                }
            }
            Event::SoftBreak => {
                let run = Run { text: " ".into(), style: style.clone() };
                match &mut cell {
                    Some(c) => c.push(run),
                    None => {
                        if let Some(p) = &mut para {
                            p.runs.push(run);
                        }
                    }
                }
            }
            Event::HardBreak => {
                if cell.is_none() {
                    let keep = para.as_ref().map(|p| (p.style, p.list, p.level));
                    flush(&mut para, &mut out);
                    let mut p = Paragraph::default();
                    if let Some((s, l, lv)) = keep {
                        (p.style, p.list, p.level) = (s, l, lv);
                    }
                    para = Some(p);
                }
            }
            Event::Rule => {
                flush(&mut para, &mut out);
                out.push(Block::PageBreak { id: folio_core::Id::new() });
            }
            Event::TaskListMarker(done) => {
                pending_check = Some(done);
                if let Some(p) = &mut para {
                    p.list = Some(ListKind::Check);
                    p.checked = done;
                    pending_check = None;
                }
            }
            _ => {}
        }
    }
    flush(&mut para, &mut out);
    out
}

fn escape(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    for c in t.chars() {
        if matches!(c, '*' | '_' | '`' | '[' | ']' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s
}

fn runs_md(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        let st = &r.style;
        let mut t = if st.code { format!("`{}`", r.text) } else { escape(&r.text) };
        // Keep spaces outside the markers (`**bold** text`, not `**bold **text`).
        let lead: String = t.chars().take_while(|c| c.is_whitespace()).collect();
        let trail: String = t.chars().rev().take_while(|c| c.is_whitespace()).collect::<String>().chars().rev().collect();
        if lead.len() < t.len() {
            t = t[lead.len()..t.len() - trail.len()].to_string();
        }
        if st.strike {
            t = format!("~~{t}~~");
        }
        if st.italic {
            t = format!("*{t}*");
        }
        if st.bold {
            t = format!("**{t}**");
        }
        if let Some(l) = &st.link {
            t = format!("[{t}]({l})");
        }
        s.push_str(&lead);
        s.push_str(&t);
        s.push_str(&trail);
    }
    s
}

/// Blocks to Markdown (linked tables are written with the sheet's values now).
pub fn from_blocks(doc: &Document, flow: &Flow) -> String {
    let mut out = String::new();
    let mut prev_list = false;
    let mut numbers: Vec<usize> = vec![0; 6];
    for b in flow.iter() {
        match b {
            Block::Paragraph(p) => {
                let text = runs_md(&p.runs);
                let is_list = p.list.is_some();
                if !out.is_empty() && !(is_list && prev_list) {
                    out.push('\n');
                }
                match (p.list, p.style) {
                    (Some(kind), _) => {
                        let indent = "  ".repeat(p.level as usize);
                        let lvl = p.level as usize;
                        for n in numbers.iter_mut().skip(lvl + 1) {
                            *n = 0;
                        }
                        let marker = match kind {
                            ListKind::Bullet => "-".to_string(),
                            ListKind::Number => {
                                numbers[lvl.min(5)] += 1;
                                format!("{}.", numbers[lvl.min(5)])
                            }
                            ListKind::Check => if p.checked { "- [x]".into() } else { "- [ ]".into() },
                        };
                        out.push_str(&format!("{indent}{marker} {text}\n"));
                    }
                    (None, ParaStyle::Title) => out.push_str(&format!("# {text}\n")),
                    (None, ParaStyle::Heading1) => out.push_str(&format!("## {text}\n")),
                    (None, ParaStyle::Heading2) => out.push_str(&format!("### {text}\n")),
                    (None, ParaStyle::Heading3) => out.push_str(&format!("#### {text}\n")),
                    (None, ParaStyle::Quote) => out.push_str(&format!("> {text}\n")),
                    (None, ParaStyle::Code) => out.push_str(&format!("    {}\n", p.text())),
                    (None, ParaStyle::Subtitle | ParaStyle::Caption) => out.push_str(&format!("*{text}*\n")),
                    (None, ParaStyle::Normal) => {
                        if !text.is_empty() {
                            out.push_str(&format!("{text}\n"));
                        }
                    }
                }
                if !is_list {
                    numbers.iter_mut().for_each(|n| *n = 0);
                }
                prev_list = is_list;
            }
            Block::Table(t) => {
                let rows: Vec<Vec<String>> = match &t.link {
                    Some(l) => folio_core::links::table_text(doc, l).unwrap_or_default().into_iter().map(|r| r.into_iter().map(|c| escape(&c)).collect()).collect(),
                    None => t.rows.iter().map(|r| r.iter().map(|c| runs_md(&c.runs).replace('\n', " ")).collect()).collect(),
                };
                if !out.is_empty() {
                    out.push('\n');
                }
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                for (i, r) in rows.iter().enumerate() {
                    let cells: Vec<String> = (0..cols).map(|c| r.get(c).cloned().unwrap_or_default().replace('|', "\\|")).collect();
                    out.push_str(&format!("| {} |\n", cells.join(" | ")));
                    if i == 0 {
                        out.push_str(&format!("|{}\n", " --- |".repeat(cols)));
                    }
                }
                prev_list = false;
            }
            Block::Image(im) => {
                let name = doc.media.get(&im.media).map(|m| m.name.clone()).unwrap_or_default();
                out.push_str(&format!("\n![{}]({})\n", if im.alt.is_empty() { &im.caption } else { &im.alt }, name));
                if !im.caption.is_empty() {
                    out.push_str(&format!("*{}*\n", escape(&im.caption)));
                }
                prev_list = false;
            }
            Block::Chart(c) => {
                out.push_str(&format!("\n*Chart: {} ({})*\n", if c.chart.title.is_empty() { "untitled" } else { &c.chart.title }, c.chart.source));
                prev_list = false;
            }
            Block::PageBreak { .. } => {
                out.push_str("\n---\n");
                prev_list = false;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_in() {
        let b = to_blocks("# Title\n\nHello **bold** and *it*.\n\n## Part\n\n- one\n- two\n  - nested\n\n1. first\n\n- [x] done\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n---\n\n> quote");
        let kinds: Vec<&str> = b.iter().map(|x| x.kind()).collect();
        assert_eq!(kinds, ["paragraph", "paragraph", "paragraph", "paragraph", "paragraph", "paragraph", "paragraph", "paragraph", "table", "pageBreak", "paragraph"]);
        assert_eq!(b[0].para().unwrap().style, ParaStyle::Title);
        assert!(b[1].para().unwrap().runs.iter().any(|r| r.style.bold && r.text == "bold"));
        assert_eq!(b[2].para().unwrap().style, ParaStyle::Heading1);
        assert_eq!(b[5].para().unwrap().level, 1);
        assert_eq!(b[6].para().unwrap().list, Some(ListKind::Number));
        assert_eq!(b[7].para().unwrap().list, Some(ListKind::Check));
        assert!(b[7].para().unwrap().checked);
        assert_eq!(b[10].para().unwrap().style, ParaStyle::Quote);
    }

    #[test]
    fn markdown_round_trip() {
        let md = "# Title\n\nHello **bold** world.\n\n- one\n- two\n";
        let doc = Document::new("t");
        let flow: Flow = to_blocks(md).into_iter().collect();
        assert_eq!(from_blocks(&doc, &flow), md);
    }
}
