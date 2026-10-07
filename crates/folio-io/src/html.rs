//! Web page: the whole file (or the chosen pages) as one self-contained HTML page.
//!
//! Documents become semantic HTML (headings, paragraphs, nested lists, tables, figures with
//! pictures as `data:` URIs, footnotes at the end of each document, comments as `<aside>`),
//! sheets become tables of their formatted values, decks become 16:9 slides with their shapes
//! placed in percentages, and charts become inline SVG drawn from `folio_layout::chart_prims`.
//! One `<style>` block, light and dark from the reader's system, IBM Plex with system fallbacks.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use folio_core::deck::VAlign;
use folio_core::text::{Comment, Flow, Table};
use folio_core::{Align, Block, Chart, Document, Id, ListKind, PageBody, ParaStyle, Run, Shape, ShapeKind};
use folio_layout::chart::{Anchor, ChartStyle, Prim};

use crate::docx::xml::{base64, esc, hex_color};
use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "html",
    name: "Web page",
    extensions: &["html", "htm"],
    kinds: &["doc", "sheet", "deck"],
    import: false,
    export: true,
    apps: &["any browser"],
    notes: "One self-contained page: documents with their headings, lists, tables, pictures, footnotes and comments; sheets as tables of their values; decks as slides; charts as drawings. Page size, headers and footers, formulas and slide transitions are left out, and nothing stays live.",
};

pub fn import(_bytes: &[u8], _title: &str) -> Result<Imported, String> {
    Err("folio writes web pages but can't open them: copy the text into a document, or save the page as Markdown first.".into())
}

const SANS: &str = "\"IBM Plex Sans\", system-ui, -apple-system, \"Segoe UI\", Roboto, \"Helvetica Neue\", Arial, sans-serif";
const SERIF: &str = "\"IBM Plex Serif\", Georgia, \"Times New Roman\", serif";
const MONO: &str = "\"IBM Plex Mono\", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";
const DISPLAY: &str = "\"Chakra Petch\", \"IBM Plex Sans\", system-ui, sans-serif";

fn font_stack(f: &str) -> String {
    match f {
        "sans" => SANS.into(),
        "serif" => SERIF.into(),
        "mono" => MONO.into(),
        "display" => DISPLAY.into(),
        other => match crate::docx::xml::family_of_font(other) {
            "serif" => format!("\"{}\", {SERIF}", esc(other)),
            "mono" => format!("\"{}\", {MONO}", esc(other)),
            _ => format!("\"{}\", {SANS}", esc(other)),
        },
    }
}

/// Only links that can't run code.
fn safe_href(url: &str) -> Option<String> {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    let scheme = lower.split_once(':').map(|(s, _)| s).filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'));
    match scheme {
        None => Some(u.to_string()),
        Some("http" | "https" | "mailto" | "tel" | "ftp") => Some(u.to_string()),
        _ => None,
    }
}

const CSS: &str = r#"
:root {
  color-scheme: light dark;
  --bg: #fbfbfb; --fg: #0a0a0a; --muted: #5c5c5c; --line: #d6d6d6; --soft: #f0f0f0; --link: #1d4ed8;
  --chart-text: #3a3a3a; --chart-grid: rgba(0, 0, 0, .14);
  --chart-0: #0a0a0a; --chart-1: #6e6e6e; --chart-2: #b0b0b0; --chart-3: #3d3d3d; --chart-4: #8f8f8f; --chart-5: #cfcfcf;
  --sans: "IBM Plex Sans", system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  --serif: "IBM Plex Serif", Georgia, "Times New Roman", serif;
  --mono: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  --display: "Chakra Petch", "IBM Plex Sans", system-ui, sans-serif;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0e0e0e; --fg: #ededed; --muted: #a3a3a3; --line: #333; --soft: #1b1b1b; --link: #8ab4ff;
    --chart-text: #c8c8c8; --chart-grid: rgba(255, 255, 255, .16);
    --chart-0: #f2f2f2; --chart-1: #9a9a9a; --chart-2: #5e5e5e; --chart-3: #d0d0d0; --chart-4: #7a7a7a; --chart-5: #444;
  }
  .c { color: color-mix(in srgb, var(--c) 55%, #fff); }
  .hl { background: color-mix(in srgb, var(--h) 40%, var(--bg)) !important; }
  .fill { background: color-mix(in srgb, var(--f) 30%, var(--bg)) !important; }
  .page.deck img, .page.doc figure img { background: #fff; }
}
* { box-sizing: border-box; }
html { -webkit-text-size-adjust: 100%; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.6 var(--sans); }
main { padding: 3rem 1.5rem 6rem; }
a { color: var(--link); text-underline-offset: .15em; }
.c { color: var(--c); }
.hl { background: var(--h); border-radius: 2px; }
.fill { background: var(--f); }
.file { max-width: 46rem; margin: 0 auto 2rem; padding: 0 1.5rem; }
.file h1 { font: 600 2rem/1.2 var(--display); letter-spacing: .01em; margin: 3rem 0 .5rem; }
.toc { max-width: 46rem; margin: 0 auto; padding: 0 1.5rem; }
.toc ol { list-style: none; padding: 0; margin: 0; border-top: 1px solid var(--line); }
.toc li { display: flex; justify-content: space-between; gap: 1rem; padding: .5rem 0; border-bottom: 1px solid var(--line); }
.toc .kind { color: var(--muted); font-size: .85rem; text-transform: uppercase; letter-spacing: .06em; }
.page { margin: 0 auto 5rem; }
.page.doc { max-width: 46rem; }
.page.sheet, .page.deck { max-width: 72rem; }
.page-label { font: 500 .8rem/1 var(--display); text-transform: uppercase; letter-spacing: .08em; color: var(--muted); margin: 0 0 1.5rem; padding-top: 1rem; border-top: 1px solid var(--line); }
.page.sheet h2.page-label, .page.deck h2.page-label { font-size: .8rem; }
h1, h2, h3 { line-height: 1.2; margin: 2rem 0 .6rem; }
h1.title { font-size: 2.6rem; margin-top: 0; }
h1 { font-size: 1.85rem; }
h2 { font-size: 1.4rem; }
h3 { font-size: 1.15rem; }
.subtitle { font-size: 1.35rem; color: var(--muted); margin-top: 0; }
.caption, figcaption { font-size: .85rem; color: var(--muted); }
p { margin: 0 0 .75rem; }
.al-c { text-align: center; }
.al-r { text-align: right; }
.al-j { text-align: justify; }
blockquote { margin: 1rem 0; padding: .1rem 0 .1rem 1rem; border-left: 3px solid var(--line); font-family: var(--serif); font-style: italic; color: var(--muted); font-size: 1.08rem; }
code, pre { font-family: var(--mono); font-size: .88em; }
code { background: var(--soft); padding: .1em .3em; border-radius: 3px; }
pre { background: var(--soft); padding: .9rem 1rem; overflow-x: auto; line-height: 1.5; border-radius: 4px; }
pre code { background: none; padding: 0; }
ul, ol { padding-left: 1.6rem; margin: 0 0 .75rem; }
li > ul, li > ol { margin-bottom: 0; }
ul.checklist { list-style: none; padding-left: .2rem; }
ul.checklist li { display: flex; gap: .5rem; align-items: baseline; }
ul.checklist li.done > span { text-decoration: line-through; color: var(--muted); }
ul.checklist input { margin: 0; }
li.skip { list-style: none; }
ins { text-decoration: underline; text-decoration-color: #2e7d32; background: rgba(46, 125, 50, .08); }
del { color: var(--muted); }
.commented { background: rgba(250, 204, 21, .28); border-bottom: 1px solid rgba(202, 138, 4, .7); }
aside.comment { font-size: .85rem; color: var(--muted); border-left: 2px solid rgba(202, 138, 4, .8); padding: .2rem 0 .2rem .8rem; margin: .25rem 0 1rem; }
aside.comment p { margin: 0 0 .2rem; }
aside.comment .meta { color: var(--fg); }
aside.comment .reply { margin-left: .8rem; }
@media (min-width: 78rem) {
  .page.doc { position: relative; }
  aside.comment { float: right; clear: right; width: 14rem; margin-right: -16.5rem; margin-top: -1.6rem; }
}
sup.fn a { text-decoration: none; padding: 0 .1em; }
.footnotes { margin-top: 3rem; padding-top: 1rem; border-top: 1px solid var(--line); font-size: .88rem; color: var(--muted); }
.footnotes ol { padding-left: 1.4rem; }
figure { margin: 1.5rem 0; }
figure.al-c { margin-left: auto; margin-right: auto; }
figure.al-r { margin-left: auto; }
figure img { display: block; width: 100%; height: auto; }
figure svg { display: block; width: 100%; height: auto; }
.table-wrap { overflow-x: auto; margin: 1rem 0 1.25rem; }
table { border-collapse: collapse; width: 100%; font-size: .94rem; }
th, td { border: 1px solid var(--line); padding: .4rem .6rem; vertical-align: top; text-align: left; }
th { font-weight: 600; background: var(--soft); }
table.banded tbody tr:nth-child(even) td:not(.fill) { background: var(--soft); }
hr.page-break { border: 0; border-top: 1px dashed var(--line); margin: 2.5rem 0; }
.sheet-wrap { overflow: auto; max-height: 80vh; border: 1px solid var(--line); }
table.sheet { width: auto; font-size: .88rem; table-layout: fixed; }
table.sheet td, table.sheet th { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; padding: .25rem .45rem; }
table.sheet td.wrap { white-space: normal; }
table.sheet td.n { text-align: right; font-variant-numeric: tabular-nums; }
table.sheet thead th { position: sticky; top: 0; }
.deck-slides { display: grid; gap: 2rem; }
.slide { position: relative; width: 100%; overflow: hidden; container-type: inline-size; border: 1px solid var(--line); box-shadow: 0 1px 3px rgba(0, 0, 0, .08); }
.slide .shape { position: absolute; display: flex; flex-direction: column; overflow: hidden; }
.slide .shape p { margin: 0; line-height: 1.2; }
.slide .shape svg, .slide .shape img { width: 100%; height: 100%; display: block; }
.slide .shape table { font-size: inherit; height: 100%; }
.slide .shape td, .slide .shape th { border-color: currentColor; padding: .2em .4em; }
.slide-no { font-size: .8rem; color: var(--muted); margin: .4rem 0 0; }
details.notes { font-size: .9rem; color: var(--muted); margin-top: .3rem; }
@media print {
  body { background: #fff; color: #000; }
  .toc { display: none; }
  .page { break-before: page; }
  .page:first-of-type { break-before: auto; }
  hr.page-break { break-after: page; border: 0; margin: 0; }
  .slide { break-inside: avoid; box-shadow: none; }
  .sheet-wrap { max-height: none; overflow: visible; }
  aside.comment { float: none; width: auto; margin: .25rem 0 1rem; }
}
"#;

// Colours the chart style uses as markers, replaced by CSS variables in the SVG.
const MARK_TEXT: [u8; 4] = [1, 2, 3, 255];
const MARK_GRID: [u8; 4] = [4, 5, 6, 255];

fn chart_style() -> ChartStyle {
    ChartStyle { text: MARK_TEXT, grid: MARK_GRID, series: (0..6u8).map(|i| [7 + i, 8, 9, 255]).collect(), size: 9.0, background: None }
}

fn svg_color(c: [u8; 4]) -> (String, Option<f32>) {
    let rgb = [c[0], c[1], c[2]];
    let named = if rgb == [MARK_TEXT[0], MARK_TEXT[1], MARK_TEXT[2]] {
        Some("var(--chart-text)".to_string())
    } else if rgb == [MARK_GRID[0], MARK_GRID[1], MARK_GRID[2]] {
        Some("var(--chart-grid)".to_string())
    } else if (7..13).contains(&c[0]) && c[1] == 8 && c[2] == 9 {
        Some(format!("var(--chart-{})", c[0] - 7))
    } else {
        None
    };
    let alpha = if c[3] == 255 { None } else { Some(c[3] as f32 / 255.0) };
    (named.unwrap_or_else(|| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])), alpha)
}

fn fmt(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() { format!("{}", r as i64) } else { format!("{r}") }
}

/// A chart as inline SVG, or `None` when its data can't be read.
fn chart_svg(doc: &Document, chart: &Chart, w: f32, h: f32, theme: Option<&folio_core::DeckTheme>) -> Result<String, String> {
    let data = folio_core::links::chart_data(doc, chart).map_err(|e| e.0)?;
    let mut style = chart_style();
    // On a slide the deck's colours are the chart's.
    if let Some(t) = theme {
        let parse = |s: &str| folio_layout::parse_hex(s, [0, 0, 0, 255]);
        style.text = parse(&t.text);
        let accent = parse(&t.accent);
        style.series = vec![accent, mix(accent, parse(&t.background), 0.45), mix(accent, parse(&t.background), 0.7)];
        style.grid = { let mut g = parse(&t.text); g[3] = 40; g };
    }
    let prims = folio_layout::chart_prims(chart, &data, w, h, &style);
    let label = if chart.title.is_empty() { format!("{} chart", chart.kind.label()) } else { chart.title.clone() };
    let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" role=\"img\" aria-label=\"{}\" font-family=\"IBM Plex Sans, system-ui, sans-serif\">", fmt(w), fmt(h), esc(&label));
    let paint = |attr: &str, c: [u8; 4]| -> String {
        let (col, a) = svg_color(c);
        match a {
            Some(a) => format!(" {attr}=\"{col}\" {attr}-opacity=\"{}\"", fmt(a)),
            None => format!(" {attr}=\"{col}\""),
        }
    };
    for p in prims {
        match p {
            Prim::Rect { x, y, w, h, fill } => {
                let _ = write!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>", fmt(x), fmt(y), fmt(w.max(0.0)), fmt(h.max(0.0)), paint("fill", fill));
            }
            Prim::Line { points, width, color } => {
                let pts: Vec<String> = points.iter().map(|(x, y)| format!("{},{}", fmt(*x), fmt(*y))).collect();
                let _ = write!(s, "<polyline points=\"{}\" fill=\"none\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"{}/>", pts.join(" "), fmt(width), paint("stroke", color));
            }
            Prim::Poly { points, fill } => {
                let pts: Vec<String> = points.iter().map(|(x, y)| format!("{},{}", fmt(*x), fmt(*y))).collect();
                let _ = write!(s, "<polygon points=\"{}\"{}/>", pts.join(" "), paint("fill", fill));
            }
            Prim::Text { x, y, text, size, color, anchor, bold } => {
                let a = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let _ = write!(s, "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"{a}\"{}{}>{}</text>", fmt(x), fmt(y), fmt(size), if bold { " font-weight=\"600\"" } else { "" }, paint("fill", color), esc(&text));
            }
        }
    }
    s.push_str("</svg>");
    Ok(s)
}

fn mix(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let m = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), 255]
}

fn align_class(a: Align) -> &'static str {
    match a {
        Align::Left => "",
        Align::Center => "al-c",
        Align::Right => "al-r",
        Align::Justify => "al-j",
    }
}

fn class_attr(classes: &[&str]) -> String {
    let c: Vec<&str> = classes.iter().copied().filter(|c| !c.is_empty()).collect();
    if c.is_empty() { String::new() } else { format!(" class=\"{}\"", c.join(" ")) }
}

struct Html<'a> {
    doc: &'a Document,
    warnings: Vec<String>,
    warned: HashSet<&'static str>,
    media: HashMap<Id, String>,
}

/// State while writing one document page.
#[derive(Default)]
struct DocCtx {
    page: usize,
    notes: Vec<String>,
    comments: HashMap<Id, Comment>,
    /// Where each comment's text ends: (block, run) — its aside goes after that block.
    comment_end: HashMap<Id, usize>,
    pending: Vec<Id>,
    shown: HashSet<Id>,
}

impl<'a> Html<'a> {
    fn warn(&mut self, key: &'static str, msg: impl Into<String>) {
        if self.warned.insert(key) {
            self.warnings.push(msg.into());
        }
    }

    fn data_uri(&mut self, id: &Id) -> Option<String> {
        if let Some(u) = self.media.get(id) {
            return Some(u.clone());
        }
        let m = self.doc.media.get(id)?;
        let u = format!("data:{};base64,{}", m.mime, base64(&m.bytes));
        self.media.insert(id.clone(), u.clone());
        Some(u)
    }

    /// One run's HTML (without its link, which groups runs).
    fn run(&mut self, r: &Run, ctx: &mut DocCtx, in_code: bool) -> String {
        let s = &r.style;
        let mut t = esc(&r.text);
        if t.contains('\n') {
            t = t.replace('\n', "<br>");
        }
        if s.code && !in_code {
            t = format!("<code>{t}</code>");
        }
        if s.superscript {
            t = format!("<sup>{t}</sup>");
        } else if s.subscript {
            t = format!("<sub>{t}</sub>");
        }
        if s.strike {
            t = format!("<s>{t}</s>");
        }
        if s.underline {
            t = format!("<u>{t}</u>");
        }
        if s.italic {
            t = format!("<em>{t}</em>");
        }
        if s.bold {
            t = format!("<strong>{t}</strong>");
        }
        let mut classes = vec![];
        let mut style = String::new();
        if let Some(c) = s.color.as_deref().and_then(hex_color) {
            classes.push("c");
            let _ = write!(style, "--c:{c};");
        }
        if let Some(h) = s.highlight.as_deref().and_then(hex_color) {
            classes.push("hl");
            let _ = write!(style, "--h:{h};");
        }
        if let Some(sz) = s.size.filter(|v| *v > 0.0) {
            let _ = write!(style, "font-size:{}em;", fmt(sz / 11.0));
        }
        if let Some(f) = s.font.as_deref() {
            let _ = write!(style, "font-family:{};", font_stack(f).replace('"', "'"));
        }
        if !classes.is_empty() || !style.is_empty() {
            let st = if style.is_empty() { String::new() } else { format!(" style=\"{style}\"") };
            t = format!("<span{}{st}>{t}</span>", class_attr(&classes));
        }
        if let Some(c) = &s.comment
            && ctx.comments.contains_key(c)
        {
            t = format!("<span class=\"commented\" data-comment=\"{c}\">{t}</span>");
            if !ctx.pending.contains(c) && !ctx.shown.contains(c) {
                ctx.pending.push(c.clone());
            }
        }
        if let Some(a) = &s.inserted {
            t = format!("<ins title=\"Inserted by {}\">{t}</ins>", esc(a));
        }
        if let Some(a) = &s.deleted {
            t = format!("<del title=\"Deleted by {}\">{t}</del>", esc(a));
        }
        t
    }

    fn runs(&mut self, runs: &[Run], ctx: &mut DocCtx, in_code: bool) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < runs.len() {
            let link = runs[i].style.link.clone().filter(|l| !l.is_empty());
            let start = i;
            i += 1;
            while i < runs.len() && runs[i].style.link.clone().filter(|l| !l.is_empty()) == link {
                i += 1;
            }
            let mut inner = String::new();
            for k in start..i {
                let r = &runs[k];
                if !r.text.is_empty() {
                    inner.push_str(&self.run(r, ctx, in_code));
                }
                if let Some(n) = &r.style.note
                    && runs.get(k + 1).is_none_or(|q| q.style.note.as_ref() != Some(n))
                {
                    ctx.notes.push(n.clone());
                    let k = ctx.notes.len();
                    let p = ctx.page;
                    let _ = write!(inner, "<sup class=\"fn\"><a href=\"#fn-{p}-{k}\" id=\"fnref-{p}-{k}\" aria-describedby=\"fn-label-{p}\">{k}</a></sup>");
                }
            }
            match link.as_deref().and_then(safe_href) {
                Some(href) => {
                    let _ = write!(out, "<a href=\"{}\">{inner}</a>", esc(&href));
                }
                None => out.push_str(&inner),
            }
        }
        out
    }

    fn asides(&mut self, ctx: &mut DocCtx, block: usize) -> String {
        let mut out = String::new();
        let due: Vec<Id> = ctx.pending.iter().filter(|c| ctx.comment_end.get(*c).is_none_or(|b| *b <= block)).cloned().collect();
        for id in due {
            ctx.pending.retain(|c| *c != id);
            ctx.shown.insert(id.clone());
            let Some(c) = ctx.comments.get(&id) else { continue };
            let _ = write!(
                out,
                "<aside class=\"comment\" id=\"comment-{id}\" aria-label=\"Comment by {a}\"><p class=\"meta\"><strong>{a}</strong> · <time datetime=\"{dt}\">{d}</time>{r}</p>",
                a = esc(&c.author),
                dt = c.at.to_rfc3339(),
                d = c.at.format("%-d %b %Y"),
                r = if c.resolved { " · resolved" } else { "" }
            );
            for line in c.text.split('\n') {
                let _ = write!(out, "<p>{}</p>", esc(line));
            }
            for r in &c.replies {
                let _ = write!(out, "<div class=\"reply\"><p class=\"meta\"><strong>{}</strong> · <time datetime=\"{}\">{}</time></p><p>{}</p></div>", esc(&r.author), r.at.to_rfc3339(), r.at.format("%-d %b %Y"), esc(&r.text));
            }
            out.push_str("</aside>");
        }
        out
    }

    fn table(&mut self, t: &Table, ctx: &mut DocCtx) -> String {
        let rows: Vec<Vec<(Vec<Run>, Option<String>, Align)>> = match &t.link {
            Some(l) => match folio_core::links::table_text(self.doc, l) {
                Ok(rows) => rows.into_iter().map(|r| r.into_iter().map(|c| (vec![Run::plain(c)], None, Align::Left)).collect()).collect(),
                Err(e) => {
                    self.warnings.push(format!("A linked table couldn't read its sheet ({e}): its last values are written."));
                    t.rows.iter().map(|r| r.iter().map(|c| (c.runs.clone(), c.fill.clone(), c.align)).collect()).collect()
                }
            },
            None => t.rows.iter().map(|r| r.iter().map(|c| (c.runs.clone(), c.fill.clone(), c.align)).collect()).collect(),
        };
        let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let mut out = format!("<div class=\"table-wrap\"><table{}>", class_attr(&[if t.banded { "banded" } else { "" }]));
        if t.widths.len() == cols {
            out.push_str("<colgroup>");
            for f in t.fractions() {
                let _ = write!(out, "<col style=\"width:{}%\">", fmt(f * 100.0));
            }
            out.push_str("</colgroup>");
        }
        for (ri, row) in rows.iter().enumerate() {
            let head = t.header && ri == 0;
            if head {
                out.push_str("<thead>");
            } else if ri == usize::from(t.header) {
                out.push_str("<tbody>");
            }
            out.push_str("<tr>");
            for ci in 0..cols {
                let (runs, fill, align) = row.get(ci).cloned().unwrap_or((vec![], None, Align::Left));
                let tag = if head { "th" } else { "td" };
                let mut classes = vec![align_class(align)];
                let mut style = String::new();
                if let Some(f) = fill.as_deref().and_then(hex_color) {
                    classes.push("fill");
                    let _ = write!(style, " style=\"--f:{f}\"");
                }
                let scope = if head { " scope=\"col\"" } else { "" };
                let _ = write!(out, "<{tag}{scope}{}{style}>{}</{tag}>", class_attr(&classes), self.runs(&runs, ctx, false));
            }
            out.push_str("</tr>");
            if head {
                out.push_str("</thead>");
            }
        }
        if rows.len() > usize::from(t.header) {
            out.push_str("</tbody>");
        }
        out.push_str("</table></div>");
        out
    }

    fn doc_page(&mut self, t: &folio_core::TextDoc, page: usize) -> String {
        let mut ctx = DocCtx { page, ..Default::default() };
        for c in &t.comments {
            ctx.comments.insert(c.id.clone(), c.clone());
        }
        for (i, b) in t.blocks.iter().enumerate() {
            let runs: Vec<&Run> = match b {
                Block::Paragraph(p) => p.runs.iter().collect(),
                Block::Table(t) if t.link.is_none() => t.rows.iter().flatten().flat_map(|c| c.runs.iter()).collect(),
                _ => vec![],
            };
            for r in runs {
                if let Some(c) = &r.style.comment {
                    ctx.comment_end.insert(c.clone(), i);
                }
            }
        }
        let orphans = t.comments.iter().filter(|c| !ctx.comment_end.contains_key(&c.id)).count();
        if orphans > 0 {
            self.warnings.push(format!("{orphans} comment{} no longer attached to any text {} left out.", if orphans == 1 { "" } else { "s" }, if orphans == 1 { "is" } else { "are" }));
        }
        let mut out = String::new();
        let blocks: Vec<&Block> = t.blocks.iter().collect();
        let text_w = t.setup.text_width();
        let mut i = 0;
        while i < blocks.len() {
            match blocks[i] {
                Block::Paragraph(p) if p.list.is_some() => {
                    let end = (i..blocks.len()).find(|&j| !matches!(blocks[j], Block::Paragraph(q) if q.list.is_some())).unwrap_or(blocks.len());
                    out.push_str(&self.list(&blocks[i..end], i, &mut ctx));
                    i = end;
                    continue;
                }
                Block::Paragraph(p) if p.style == ParaStyle::Code => {
                    let end = (i..blocks.len()).find(|&j| !matches!(blocks[j], Block::Paragraph(q) if q.style == ParaStyle::Code && q.list.is_none())).unwrap_or(blocks.len());
                    let lines: Vec<String> = blocks[i..end].iter().filter_map(|b| b.para()).map(|q| self.runs(&q.runs, &mut ctx, true)).collect();
                    let _ = write!(out, "<pre><code>{}</code></pre>", lines.join("\n"));
                    out.push_str(&self.asides(&mut ctx, end - 1));
                    i = end;
                    continue;
                }
                Block::Paragraph(p) if p.style == ParaStyle::Quote => {
                    let end = (i..blocks.len()).find(|&j| !matches!(blocks[j], Block::Paragraph(q) if q.style == ParaStyle::Quote && q.list.is_none())).unwrap_or(blocks.len());
                    out.push_str("<blockquote>");
                    for b in &blocks[i..end] {
                        if let Some(q) = b.para() {
                            let _ = write!(out, "<p{}>{}</p>", class_attr(&[align_class(q.align)]), self.runs(&q.runs, &mut ctx, false));
                        }
                    }
                    out.push_str("</blockquote>");
                    out.push_str(&self.asides(&mut ctx, end - 1));
                    i = end;
                    continue;
                }
                Block::Paragraph(p) => {
                    let (tag, class) = match p.style {
                        ParaStyle::Title => ("h1", "title"),
                        ParaStyle::Heading1 => ("h1", ""),
                        ParaStyle::Heading2 => ("h2", ""),
                        ParaStyle::Heading3 => ("h3", ""),
                        ParaStyle::Subtitle => ("p", "subtitle"),
                        ParaStyle::Caption => ("p", "caption"),
                        _ => ("p", ""),
                    };
                    let body = self.runs(&p.runs, &mut ctx, false);
                    if body.is_empty() && tag == "p" {
                        // An empty paragraph is a blank line.
                        out.push_str("<p><br></p>");
                    } else {
                        let _ = write!(out, "<{tag}{}>{body}</{tag}>", class_attr(&[class, align_class(p.align)]));
                    }
                }
                Block::Table(t) => out.push_str(&self.table(t, &mut ctx)),
                Block::Image(im) => {
                    let Some(uri) = self.data_uri(&im.media) else {
                        i += 1;
                        continue;
                    };
                    let m = &self.doc.media[&im.media];
                    let pct = if im.width > 0.0 { (im.width / text_w * 100.0).min(100.0) } else { 100.0 };
                    let dims = if m.width > 0 && m.height > 0 { format!(" width=\"{}\" height=\"{}\"", m.width, m.height) } else { String::new() };
                    let alt = if im.alt.is_empty() { &im.caption } else { &im.alt };
                    let _ = write!(out, "<figure{} style=\"width:{}%\"><img src=\"{uri}\" alt=\"{}\"{dims} loading=\"lazy\" decoding=\"async\">", class_attr(&[align_class(im.align)]), fmt(pct), esc(alt));
                    if !im.caption.is_empty() {
                        let _ = write!(out, "<figcaption>{}</figcaption>", esc(&im.caption));
                    }
                    out.push_str("</figure>");
                }
                Block::Chart(c) => match chart_svg(self.doc, &c.chart, text_w, c.height.max(36.0), None) {
                    Ok(svg) => {
                        let _ = write!(out, "<figure class=\"chart\">{svg}");
                        if !c.chart.title.is_empty() {
                            let _ = write!(out, "<figcaption>{}</figcaption>", esc(&c.chart.title));
                        }
                        out.push_str("</figure>");
                        self.warn("chart", "Charts are drawings of their values now: they no longer follow their sheet.");
                    }
                    Err(e) => self.warnings.push(format!("A chart couldn't read its data ({e}) and is left out.")),
                },
                Block::PageBreak { .. } => out.push_str("<hr class=\"page-break\">"),
            }
            out.push_str(&self.asides(&mut ctx, i));
            i += 1;
        }
        let rest: Vec<Id> = ctx.pending.clone();
        if !rest.is_empty() {
            out.push_str(&self.asides(&mut ctx, usize::MAX));
        }
        if !ctx.notes.is_empty() {
            let _ = write!(out, "<section class=\"footnotes\" aria-labelledby=\"fn-label-{page}\"><h2 id=\"fn-label-{page}\" class=\"page-label\">Notes</h2><ol>");
            for (k, n) in ctx.notes.iter().enumerate() {
                let k = k + 1;
                let _ = write!(out, "<li id=\"fn-{page}-{k}\">{} <a href=\"#fnref-{page}-{k}\" aria-label=\"Back to the text\">↩</a></li>", esc(n));
            }
            out.push_str("</ol></section>");
        }
        out
    }

    /// Nested lists from list paragraphs (`blocks` are all list items; `first` is the index of
    /// the first in the page).
    fn list(&mut self, blocks: &[&Block], first: usize, ctx: &mut DocCtx) -> String {
        let mut out = String::new();
        // Open lists: (level, closing tag).
        let mut stack: Vec<(u8, &'static str, ListKind)> = vec![];
        for (k, b) in blocks.iter().enumerate() {
            let Some(p) = b.para() else { continue };
            let kind = p.list.unwrap_or(ListKind::Bullet);
            let level = p.level.min(5);
            while stack.last().is_some_and(|(l, _, _)| *l > level) {
                let (_, tag, _) = stack.pop().unwrap();
                let _ = write!(out, "</li></{tag}>");
            }
            if let Some((l, tag, kd)) = stack.last().copied()
                && l == level
            {
                if kd == kind {
                    out.push_str("</li>");
                } else {
                    stack.pop();
                    let _ = write!(out, "</li></{tag}>");
                }
            }
            // Open lists down to this level (levels are always contiguous from 0).
            while stack.last().is_none_or(|(l, _, _)| *l < level) {
                let lvl = stack.last().map(|(l, _, _)| l + 1).unwrap_or(0);
                if lvl == level {
                    let (open, tag) = match kind {
                        ListKind::Bullet => ("<ul>", "ul"),
                        ListKind::Number => ("<ol>", "ol"),
                        ListKind::Check => ("<ul class=\"checklist\">", "ul"),
                    };
                    out.push_str(open);
                    stack.push((lvl, tag, kind));
                } else {
                    out.push_str("<ul><li class=\"skip\">");
                    stack.push((lvl, "ul", ListKind::Bullet));
                }
            }
            let body = self.runs(&p.runs, ctx, false);
            let cls = class_attr(&[align_class(p.align), if kind == ListKind::Check && p.checked { "done" } else { "" }]);
            if kind == ListKind::Check {
                let _ = write!(out, "<li{cls}><input type=\"checkbox\" disabled{} aria-label=\"{}\"><span>{body}</span>", if p.checked { " checked" } else { "" }, if p.checked { "Done" } else { "To do" });
            } else {
                let _ = write!(out, "<li{cls}>{body}");
            }
            out.push_str(&self.asides(ctx, first + k));
        }
        while let Some((_, tag, _)) = stack.pop() {
            let _ = write!(out, "</li></{tag}>");
        }
        out
    }

    fn sheet_page(&mut self, page: &folio_core::Page, s: &folio_core::Sheet) -> String {
        let mut out = String::new();
        let Some(used) = s.used_range() else {
            return "<p class=\"caption\">This sheet is empty.</p>".into();
        };
        let max_rows = 5000u32;
        let max_cols = 200u32;
        let rows = (used.end.row - used.start.row + 1).min(max_rows);
        let cols = (used.end.col - used.start.col + 1).min(max_cols);
        if rows < used.end.row - used.start.row + 1 || cols < used.end.col - used.start.col + 1 {
            self.warnings.push(format!("\"{}\" is big: only its first {max_rows} rows and {max_cols} columns are on the page.", page.name));
        }
        let hidden = s.hidden_rows();
        let header_rows = s.freeze_rows.saturating_sub(used.start.row).min(rows);
        out.push_str("<div class=\"sheet-wrap\"><table class=\"sheet\"><colgroup>");
        for c in 0..cols {
            let _ = write!(out, "<col style=\"width:{}px\">", fmt(s.col_width(used.start.col + c)));
        }
        out.push_str("</colgroup>");
        for r in 0..rows {
            let row = used.start.row + r;
            if hidden.contains(&row) {
                continue;
            }
            let head = r < header_rows;
            if head && r == 0 {
                out.push_str("<thead>");
            }
            if !head && r == header_rows {
                out.push_str("<tbody>");
            }
            out.push_str("<tr>");
            for c in 0..cols {
                let a = folio_calc::Addr::new(row, used.start.col + c);
                let tag = if head { "th" } else { "td" };
                let Some(cell) = s.cell(a) else {
                    let _ = write!(out, "<{tag}></{tag}>");
                    continue;
                };
                let f = &cell.format;
                let mut classes: Vec<&str> = vec![];
                let mut style = String::new();
                let numeric = matches!(cell.value, folio_calc::Value::Number(_));
                match f.align {
                    Some(Align::Center) => classes.push("al-c"),
                    Some(Align::Right) => classes.push("al-r"),
                    Some(Align::Justify) => classes.push("al-j"),
                    Some(Align::Left) => {}
                    None if numeric => classes.push("n"),
                    None => {}
                }
                if f.wrap {
                    classes.push("wrap");
                }
                if let Some(c) = f.color.as_deref().and_then(hex_color) {
                    classes.push("c");
                    let _ = write!(style, "--c:{c};");
                }
                if let Some(c) = f.fill.as_deref().and_then(hex_color) {
                    classes.push("fill");
                    let _ = write!(style, "--f:{c};");
                }
                if f.bold {
                    style.push_str("font-weight:600;");
                }
                if f.italic {
                    style.push_str("font-style:italic;");
                }
                let deco: Vec<&str> = [(f.underline, "underline"), (f.strike, "line-through")].iter().filter(|(on, _)| *on).map(|(_, d)| *d).collect();
                if !deco.is_empty() {
                    let _ = write!(style, "text-decoration:{};", deco.join(" "));
                }
                if let Some(sz) = f.size.filter(|v| (*v - 10.0).abs() > 0.01) {
                    let _ = write!(style, "font-size:{}em;", fmt(sz / 10.0));
                }
                for (side, name) in [('t', "top"), ('r', "right"), ('b', "bottom"), ('l', "left")] {
                    if f.border.contains(side) {
                        let _ = write!(style, "border-{name}:1.5px solid currentColor;");
                    }
                }
                let st = if style.is_empty() { String::new() } else { format!(" style=\"{style}\"") };
                let _ = write!(out, "<{tag}{}{st}>{}</{tag}>", class_attr(&classes), esc(&cell.display()));
            }
            out.push_str("</tr>");
            if head && r + 1 == header_rows {
                out.push_str("</thead>");
            }
        }
        if rows > header_rows {
            out.push_str("</tbody>");
        }
        out.push_str("</table></div>");
        for ch in &s.charts {
            match chart_svg(self.doc, &ch.chart, ch.w.max(120.0), ch.h.max(80.0), None) {
                Ok(svg) => {
                    let _ = write!(out, "<figure class=\"chart\" style=\"max-width:{}px\">{svg}", fmt(ch.w.max(120.0)));
                    if !ch.chart.title.is_empty() {
                        let _ = write!(out, "<figcaption>{}</figcaption>", esc(&ch.chart.title));
                    }
                    out.push_str("</figure>");
                    self.warn("chart", "Charts are drawings of their values now: they no longer follow their sheet.");
                }
                Err(e) => self.warnings.push(format!("A chart on \"{}\" couldn't read its data ({e}) and is left out.", page.name)),
            }
        }
        out
    }

    fn shape_text(&mut self, flow: &Flow, shape: &Shape, deck: &folio_core::Deck, color: &str) -> String {
        let t = &deck.theme;
        let mut out = String::new();
        let mut numbers = [0usize; 6];
        for b in flow.iter() {
            let Block::Paragraph(p) = b else { continue };
            let spec = p.style.spec();
            let size = shape.text_size * spec.size / 11.0;
            let family = if p.style.is_heading() || p.style == ParaStyle::Subtitle { &t.heading_font } else { &t.body_font };
            let family = if p.style == ParaStyle::Code { "mono" } else { family.as_str() };
            let mut style = format!("font-size:calc(var(--u) * {});font-family:{};", fmt(size), font_stack(family).replace('"', "'"));
            if spec.bold {
                style.push_str("font-weight:600;");
            }
            if spec.italic {
                style.push_str("font-style:italic;");
            }
            if spec.muted {
                style.push_str("opacity:.72;");
            }
            let marker = match p.list {
                Some(k) => {
                    let lvl = p.level.min(5) as usize;
                    for n in numbers.iter_mut().skip(lvl + 1) {
                        *n = 0;
                    }
                    let _ = write!(style, "padding-left:{}em;text-indent:-1.1em;", fmt(1.1 + lvl as f32 * 1.2));
                    match k {
                        ListKind::Bullet => "•\u{2002}".to_string(),
                        ListKind::Number => {
                            numbers[lvl] += 1;
                            format!("{}.\u{2002}", numbers[lvl])
                        }
                        ListKind::Check => if p.checked { "☒\u{2002}".into() } else { "☐\u{2002}".into() },
                    }
                }
                None => {
                    numbers = [0; 6];
                    String::new()
                }
            };
            let mut ctx = DocCtx::default();
            let body = self.runs(&p.runs, &mut ctx, p.style == ParaStyle::Code);
            let _ = color;
            let _ = write!(out, "<p{} style=\"{style}\">{marker}{}</p>", class_attr(&[align_class(p.align)]), if body.is_empty() { "<br>".to_string() } else { body });
        }
        out
    }

    fn deck_page(&mut self, page: &folio_core::Page, d: &folio_core::Deck) -> String {
        let [sw, sh] = d.size;
        let (sw, sh) = (sw.max(1.0), sh.max(1.0));
        let t = &d.theme;
        let mut out = String::from("<div class=\"deck-slides\">");
        let mut hidden = 0;
        let mut n = 0;
        for slide in &d.slides {
            if slide.hidden {
                hidden += 1;
                continue;
            }
            n += 1;
            let bg = slide.background.as_deref().and_then(hex_color).or_else(|| hex_color(&t.background)).unwrap_or_else(|| "#ffffff".into());
            let fg = hex_color(&t.text).unwrap_or_else(|| "#0a0a0a".into());
            let title = slide.title();
            let label = if title.is_empty() { format!("Slide {n}") } else { format!("Slide {n}: {title}") };
            let _ = write!(
                out,
                "<div><section class=\"slide\" aria-label=\"{}\" style=\"aspect-ratio:{} / {};background:{bg};color:{fg};--u:calc(100cqw / {});font-family:{}\">",
                esc(&label),
                fmt(sw),
                fmt(sh),
                fmt(sw),
                font_stack(&t.body_font).replace('"', "'")
            );
            for s in &slide.shapes {
                out.push_str(&self.shape(s, d, sw, sh, &fg));
            }
            let _ = write!(out, "</section><p class=\"slide-no\">{n}</p>");
            if !slide.notes.trim().is_empty() {
                out.push_str("<details class=\"notes\"><summary>Speaker notes</summary>");
                for line in slide.notes.split('\n') {
                    let _ = write!(out, "<p>{}</p>", esc(line));
                }
                out.push_str("</details>");
            }
            out.push_str("</div>");
        }
        out.push_str("</div>");
        if hidden > 0 {
            self.warnings.push(format!("{hidden} hidden slide{} in \"{}\" {} left out.", if hidden == 1 { "" } else { "s" }, page.name, if hidden == 1 { "is" } else { "are" }));
        }
        out
    }

    fn shape(&mut self, s: &Shape, d: &folio_core::Deck, sw: f32, sh: f32, fg: &str) -> String {
        let (x, w) = if s.w < 0.0 { (s.x + s.w, -s.w) } else { (s.x, s.w) };
        let (y, h) = if s.h < 0.0 { (s.y + s.h, -s.h) } else { (s.y, s.h) };
        let mut style = format!("left:{}%;top:{}%;width:{}%;height:{}%;", fmt(x / sw * 100.0), fmt(y / sh * 100.0), fmt(w.max(0.5) / sw * 100.0), fmt(h.max(0.5) / sh * 100.0));
        if s.rotation != 0.0 {
            let _ = write!(style, "transform:rotate({}deg);", fmt(s.rotation));
        }
        let color = s.color.as_deref().and_then(hex_color).unwrap_or_else(|| fg.to_string());
        let _ = write!(style, "color:{color};");
        let stroke = s.line.as_deref().and_then(hex_color);
        let lw = if s.line_width > 0.0 { s.line_width } else { 1.0 };
        let fill = s.fill.as_deref().and_then(hex_color);
        let mut inner = String::new();
        let mut class = "shape";
        match &s.kind {
            ShapeKind::Text | ShapeKind::Rect | ShapeKind::Ellipse => {
                if let Some(f) = &fill {
                    let _ = write!(style, "background:{f};");
                }
                if let Some(l) = &stroke {
                    let _ = write!(style, "border:calc(var(--u) * {}) solid {l};", fmt(lw));
                }
                if matches!(s.kind, ShapeKind::Ellipse) {
                    style.push_str("border-radius:50%;");
                }
            }
            ShapeKind::Triangle => {
                let _ = write!(
                    inner,
                    "<svg viewBox=\"0 0 100 100\" preserveAspectRatio=\"none\" aria-hidden=\"true\" style=\"position:absolute;inset:0\"><polygon points=\"50,0 100,100 0,100\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" vector-effect=\"non-scaling-stroke\"/></svg>",
                    fill.clone().unwrap_or_else(|| "none".into()),
                    stroke.clone().unwrap_or_else(|| "none".into()),
                    fmt(lw)
                );
            }
            ShapeKind::Line | ShapeKind::Arrow => {
                let (x1, x2) = if s.w < 0.0 { (w, 0.0) } else { (0.0, w) };
                let (y1, y2) = if s.h < 0.0 { (h, 0.0) } else { (0.0, h) };
                let col = stroke.clone().unwrap_or_else(|| color.clone());
                let marker = if matches!(s.kind, ShapeKind::Arrow) {
                    let mid = format!("ah-{}", s.id);
                    let _ = write!(inner, "<svg viewBox=\"0 0 {} {}\" preserveAspectRatio=\"none\" overflow=\"visible\" aria-hidden=\"true\" style=\"overflow:visible\"><defs><marker id=\"{mid}\" viewBox=\"0 0 10 10\" refX=\"8\" refY=\"5\" markerWidth=\"6\" markerHeight=\"6\" orient=\"auto-start-reverse\"><path d=\"M0,0 L10,5 L0,10 z\" fill=\"{col}\"/></marker></defs>", fmt(w.max(0.5)), fmt(h.max(0.5)));
                    format!(" marker-end=\"url(#{mid})\"")
                } else {
                    let _ = write!(inner, "<svg viewBox=\"0 0 {} {}\" preserveAspectRatio=\"none\" aria-hidden=\"true\" style=\"overflow:visible\">", fmt(w.max(0.5)), fmt(h.max(0.5)));
                    String::new()
                };
                let _ = write!(inner, "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{col}\" stroke-width=\"{}\" vector-effect=\"non-scaling-stroke\"{marker}/></svg>", fmt(x1), fmt(y1), fmt(x2), fmt(y2), fmt(lw * 1.5));
                style.push_str("overflow:visible;");
            }
            ShapeKind::Image { media } => match self.data_uri(media) {
                Some(uri) => {
                    let _ = write!(inner, "<img src=\"{uri}\" alt=\"{}\" style=\"object-fit:fill\">", esc(&s.name));
                }
                None => return String::new(),
            },
            ShapeKind::Chart { chart } => match chart_svg(self.doc, chart, w.max(10.0), h.max(10.0), Some(&d.theme)) {
                Ok(svg) => {
                    inner.push_str(&svg);
                    self.warn("chart", "Charts are drawings of their values now: they no longer follow their sheet.");
                }
                Err(e) => {
                    self.warnings.push(format!("A chart on a slide couldn't read its data ({e}) and is left out."));
                    return String::new();
                }
            },
            ShapeKind::Table { table } => {
                let mut ctx = DocCtx::default();
                let _ = write!(style, "font-size:calc(var(--u) * {});", fmt(s.text_size * 0.8));
                inner.push_str(&self.table(table, &mut ctx));
                class = "shape table";
            }
        }
        if s.takes_text() && !s.text.is_empty() {
            let justify = match s.valign {
                VAlign::Top => "flex-start",
                VAlign::Middle => "center",
                VAlign::Bottom => "flex-end",
            };
            let _ = write!(style, "justify-content:{justify};padding:calc(var(--u) * 6) calc(var(--u) * 8);");
            let text = self.shape_text(&s.text, s, d, &color);
            if matches!(s.kind, ShapeKind::Triangle) {
                let _ = write!(inner, "<div style=\"position:relative\">{text}</div>");
            } else {
                inner.push_str(&text);
            }
        }
        format!("<div class=\"{class}\" style=\"{style}\">{inner}</div>")
    }
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut h = Html { doc, warnings: vec![], warned: HashSet::new(), media: HashMap::new() };
    let pages: Vec<usize> = pages.iter().copied().filter(|&i| i < doc.pages.len()).collect();
    if pages.is_empty() {
        return Err("Nothing to write: no pages were chosen.".into());
    }
    let several = pages.len() > 1;
    let mut body = String::new();
    for (n, &i) in pages.iter().enumerate() {
        let p = &doc.pages[i];
        let id = format!("page-{}", n + 1);
        let label = if several { format!("<h2 class=\"page-label\">{}</h2>", esc(&p.name)) } else { String::new() };
        match &p.body {
            PageBody::Doc(t) => {
                let _ = write!(body, "<article class=\"page doc\" id=\"{id}\" aria-label=\"{}\">{label}{}</article>", esc(&p.name), h.doc_page(t, n + 1));
            }
            PageBody::Sheet(s) => {
                let label = format!("<h2 class=\"page-label\">{}</h2>", esc(&p.name));
                let _ = write!(body, "<section class=\"page sheet\" id=\"{id}\" aria-label=\"{}\">{label}{}</section>", esc(&p.name), h.sheet_page(p, s));
            }
            PageBody::Deck(d) => {
                let label = format!("<h2 class=\"page-label\">{}</h2>", esc(&p.name));
                let _ = write!(body, "<section class=\"page deck\" id=\"{id}\" aria-label=\"{}\">{label}{}</section>", esc(&p.name), h.deck_page(p, d));
            }
        }
    }
    let mut head = String::new();
    if several {
        let _ = write!(head, "<header class=\"file\"><h1>{}</h1></header><nav class=\"toc\" aria-label=\"Pages\"><ol>", esc(&doc.title));
        for (n, &i) in pages.iter().enumerate() {
            let p = &doc.pages[i];
            let kind = match p.kind() {
                folio_core::PageKind::Doc => "Document",
                folio_core::PageKind::Sheet => "Sheet",
                folio_core::PageKind::Deck => "Deck",
            };
            let _ = write!(head, "<li><a href=\"#page-{}\">{}</a><span class=\"kind\">{kind}</span></li>", n + 1, esc(&p.name));
        }
        head.push_str("</ol></nav>");
    }
    let lang = "en";
    let author = if doc.meta.author.is_empty() { String::new() } else { format!("<meta name=\"author\" content=\"{}\">", esc(&doc.meta.author)) };
    let html = format!(
        "<!doctype html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"folio\">\n{author}<title>{}</title>\n<style>{CSS}</style>\n</head>\n<body>\n{head}<main>{body}</main>\n</body>\n</html>\n",
        esc(&doc.title)
    );
    Ok((html.into_bytes(), h.warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::{PageKind, Paragraph};

    fn sample() -> Document {
        let mut doc = Document::new("Report & plan");
        let t = doc.page_mut(0).doc_mut().unwrap();
        let mut link = Run::plain("site");
        link.style.link = Some("https://example.com/?a=1&b=2".into());
        let mut bad = Run::plain("bad");
        bad.style.link = Some("javascript:alert(1)".into());
        let mut noted = Run::bold("noted");
        noted.style.note = Some("A note.".into());
        t.blocks = imbl::vector![
            Block::Paragraph(Paragraph::new(ParaStyle::Title, "Report")),
            Block::Paragraph(Paragraph::with_runs(ParaStyle::Normal, vec![Run::plain("See "), link, Run::plain(" and "), bad, Run::plain(" "), noted])),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "one").list(ListKind::Bullet, 0)),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "nested").list(ListKind::Number, 1)),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "two").list(ListKind::Bullet, 0)),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "done").list(ListKind::Check, 0)),
            Block::Paragraph(Paragraph::new(ParaStyle::Code, "let a = 1;")),
            Block::Paragraph(Paragraph::new(ParaStyle::Code, "let b = <2>;")),
            Block::Table(Table::from_text(vec![vec!["A".into(), "B".into()], vec!["1".into(), "2".into()]], true)),
            Block::PageBreak { id: Id::new() },
        ];
        doc.add_page(PageKind::Sheet, Some("Budget"), None).unwrap();
        let s = doc.page_mut(1).sheet_mut().unwrap();
        s.set_input(folio_calc::Addr::new(0, 0), "Item");
        s.set_input(folio_calc::Addr::new(0, 1), "12.5");
        doc.add_page(PageKind::Deck, Some("Pitch"), None).unwrap();
        doc
    }

    #[test]
    fn whole_file() {
        let doc = sample();
        let (bytes, _w) = export(&doc, &[0, 1, 2]).unwrap();
        let html = String::from_utf8(bytes).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<title>Report &amp; plan</title>"));
        assert!(html.contains("<nav class=\"toc\""));
        assert!(html.contains("<h1 class=\"title\">Report</h1>"));
        assert!(html.contains("href=\"https://example.com/?a=1&amp;b=2\""));
        assert!(!html.contains("javascript:"));
        assert!(html.contains("<ul><li>one<ol><li>nested</li></ol></li><li>two</li></ul>"));
        assert!(html.contains("class=\"checklist\""));
        assert!(html.contains("<pre><code>let a = 1;\nlet b = &lt;2&gt;;</code></pre>"));
        assert!(html.contains("<thead><tr><th scope=\"col\">A</th>"));
        assert!(html.contains("id=\"fn-1-1\""));
        assert!(html.contains("prefers-color-scheme: dark"));
        assert!(html.contains("class=\"slide\""));
        assert!(html.contains(">Item</td>") || html.contains(">Item</th>"));
        assert_eq!(html.matches("<ul").count(), html.matches("</ul>").count());
        assert_eq!(html.matches("<ol").count(), html.matches("</ol>").count());
        assert_eq!(html.matches("<li").count(), html.matches("</li>").count());
    }

    #[test]
    fn deep_list_levels_balance() {
        let mut doc = Document::new("t");
        let t = doc.page_mut(0).doc_mut().unwrap();
        t.blocks = imbl::vector![
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "deep").list(ListKind::Bullet, 2)),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "top").list(ListKind::Number, 0)),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "mid").list(ListKind::Bullet, 1)),
            Block::Paragraph(Paragraph::new(ParaStyle::Normal, "mid2").list(ListKind::Number, 1)),
        ];
        let (bytes, _) = export(&doc, &[0]).unwrap();
        let html = String::from_utf8(bytes).unwrap();
        let open = html.matches("<ul").count() + html.matches("<ol").count();
        let close = html.matches("</ul>").count() + html.matches("</ol>").count();
        assert_eq!(open, close, "{html}");
        assert_eq!(html.matches("<li").count(), html.matches("</li>").count());
        assert!(html.contains("deep") && html.contains("mid2"));
    }
}
