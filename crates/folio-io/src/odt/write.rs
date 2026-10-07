//! ODT out: folio pages to an OpenDocument text package (LibreOffice Writer's own format).
//!
//! Each document page starts on its own master page (page size, margins, header and footer);
//! sheets go in as tables of their values; decks are left out.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use folio_core::text::{Comment, PageSetup, Table};
use folio_core::{Align, Block, Document, Id, ListKind, PageBody, ParaStyle, Run, RunStyle};

use crate::docx::xml::{ZipOut, esc, hex_color};
use crate::docx::{face, sheet_table};

const NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:loext=\"urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0\"";
const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";

/// The ODF name of a folio paragraph style.
fn style_name(s: ParaStyle) -> &'static str {
    match s {
        ParaStyle::Normal => "Standard",
        ParaStyle::Title => "Title",
        ParaStyle::Subtitle => "Subtitle",
        ParaStyle::Heading1 => "Heading_20_1",
        ParaStyle::Heading2 => "Heading_20_2",
        ParaStyle::Heading3 => "Heading_20_3",
        ParaStyle::Quote => "Quotations",
        ParaStyle::Code => "Preformatted_20_Text",
        ParaStyle::Caption => "Caption",
    }
}

fn pt(v: f32) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    format!("{r}pt")
}

/// Text that keeps its spaces, tabs and line breaks.
fn text_xml(t: &str) -> String {
    let mut out = String::new();
    let mut spaces = 0usize;
    let flush = |out: &mut String, spaces: &mut usize, at_start: bool| {
        if *spaces == 0 {
            return;
        }
        if at_start {
            let _ = write!(out, "<text:s text:c=\"{}\"/>", *spaces);
        } else {
            out.push(' ');
            if *spaces > 1 {
                let _ = write!(out, "<text:s text:c=\"{}\"/>", *spaces - 1);
            }
        }
        *spaces = 0;
    };
    let mut at_start = true;
    for c in t.chars() {
        match c {
            ' ' => spaces += 1,
            '\t' => {
                flush(&mut out, &mut spaces, at_start);
                out.push_str("<text:tab/>");
                at_start = false;
            }
            '\n' => {
                flush(&mut out, &mut spaces, at_start);
                out.push_str("<text:line-break/>");
                at_start = true;
            }
            c => {
                flush(&mut out, &mut spaces, at_start);
                out.push_str(&esc(&c.to_string()));
                at_start = false;
            }
        }
    }
    // Trailing spaces: ODF would drop a single one, so write them all as `text:s`.
    if spaces > 0 {
        let _ = write!(out, "<text:s text:c=\"{spaces}\"/>");
    }
    out
}

struct ListRegion {
    name: String,
    levels: BTreeMap<u8, ListKind>,
}

struct Writer<'a> {
    doc: &'a Document,
    warnings: Vec<String>,
    warned: HashSet<&'static str>,
    /// Automatic text styles by their properties.
    text_styles: Vec<(String, String)>,
    para_styles: Vec<(String, String)>,
    table_styles: String,
    list_styles: Vec<ListRegion>,
    tables: usize,
    frames: usize,
    pictures: Vec<(String, String, Vec<u8>)>,
    media: HashMap<Id, Option<(String, String)>>,
    notes: usize,
    comment_span: HashMap<Id, (usize, usize)>,
    comments: HashMap<Id, Comment>,
    counter: usize,
    changes: Vec<String>,
    masters: Vec<(String, PageSetup)>,
    now: String,
}

impl<'a> Writer<'a> {
    fn warn(&mut self, key: &'static str, msg: impl Into<String>) {
        if self.warned.insert(key) {
            self.warnings.push(msg.into());
        }
    }

    fn text_style(&mut self, s: &RunStyle, bold: bool) -> Option<String> {
        let mut p = String::new();
        if s.bold || bold {
            p.push_str(" fo:font-weight=\"bold\" style:font-weight-asian=\"bold\" style:font-weight-complex=\"bold\"");
        }
        if s.italic {
            p.push_str(" fo:font-style=\"italic\" style:font-style-asian=\"italic\" style:font-style-complex=\"italic\"");
        }
        if s.underline {
            p.push_str(" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"");
        }
        if s.strike {
            p.push_str(" style:text-line-through-style=\"solid\" style:text-line-through-type=\"single\"");
        }
        if let Some(c) = s.color.as_deref().and_then(hex_color) {
            let _ = write!(p, " fo:color=\"{c}\"");
        }
        if let Some(h) = s.highlight.as_deref().and_then(hex_color) {
            let _ = write!(p, " fo:background-color=\"{h}\"");
        }
        if let Some(sz) = s.size.filter(|v| *v > 0.0) {
            let _ = write!(p, " fo:font-size=\"{}\" style:font-size-asian=\"{0}\" style:font-size-complex=\"{0}\"", pt(sz));
        }
        let font = if s.code { Some("IBM Plex Mono".to_string()) } else { s.font.as_deref().map(face) };
        if let Some(f) = font {
            let _ = write!(p, " style:font-name=\"{}\"", esc(&f));
        }
        if s.superscript {
            p.push_str(" style:text-position=\"super 58%\"");
        } else if s.subscript {
            p.push_str(" style:text-position=\"sub 58%\"");
        }
        if p.is_empty() {
            return None;
        }
        if let Some((n, _)) = self.text_styles.iter().find(|(_, x)| *x == p) {
            return Some(n.clone());
        }
        let n = format!("T{}", self.text_styles.len() + 1);
        self.text_styles.push((n.clone(), p));
        Some(n)
    }

    /// An automatic paragraph style on top of a named one (alignment, page break, master page).
    fn para_style(&mut self, parent: &str, align: Align, break_before: bool, master: Option<&str>, list: Option<&str>) -> String {
        if align == Align::Left && !break_before && master.is_none() && list.is_none() {
            return parent.to_string();
        }
        let mut x = format!(" style:family=\"paragraph\" style:parent-style-name=\"{parent}\"");
        if let Some(m) = master {
            let _ = write!(x, " style:master-page-name=\"{m}\"");
        }
        if let Some(l) = list {
            let _ = write!(x, " style:list-style-name=\"{l}\"");
        }
        let mut props = String::new();
        match align {
            Align::Left => {}
            Align::Center => props.push_str(" fo:text-align=\"center\""),
            Align::Right => props.push_str(" fo:text-align=\"end\""),
            Align::Justify => props.push_str(" fo:text-align=\"justify\""),
        }
        if break_before && master.is_none() {
            props.push_str(" fo:break-before=\"page\"");
        }
        let body = format!("{x}>{}", if props.is_empty() { String::new() } else { format!("<style:paragraph-properties{props}/>") });
        if let Some((n, _)) = self.para_styles.iter().find(|(_, b)| *b == body) {
            return n.clone();
        }
        let n = format!("P{}", self.para_styles.len() + 1);
        self.para_styles.push((n.clone(), body));
        n
    }

    fn picture(&mut self, id: &Id) -> Option<(String, String)> {
        if let Some(p) = self.media.get(id) {
            return p.clone();
        }
        let got = self.doc.media.get(id).and_then(|m| {
            let (bytes, ext, mime): (Vec<u8>, &str, &str) = match m.mime.as_str() {
                "image/png" | "image/jpeg" | "image/gif" | "image/svg+xml" | "image/bmp" => (m.bytes.to_vec(), m.ext(), m.mime.as_str()),
                _ => {
                    let img = image::load_from_memory(&m.bytes).ok()?;
                    let mut out = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
                    (out.into_inner(), "png", "image/png")
                }
            };
            let path = format!("Pictures/{}.{ext}", m.id);
            self.pictures.push((path.clone(), mime.to_string(), bytes));
            Some((path, mime.to_string()))
        });
        if got.is_none() {
            self.warn("picture", "Some pictures are in formats LibreOffice can't show and are left out.");
        }
        self.media.insert(id.clone(), got.clone());
        got
    }

    fn frame(&mut self, path: &str, mime: &str, w: f32, h: f32, alt: &str) -> String {
        self.frames += 1;
        let desc = if alt.is_empty() { String::new() } else { format!("<svg:desc>{}</svg:desc>", esc(alt)) };
        format!(
            "<draw:frame draw:style-name=\"fr1\" draw:name=\"Picture {}\" text:anchor-type=\"as-char\" svg:width=\"{}\" svg:height=\"{}\" draw:z-index=\"0\"><draw:image xlink:href=\"{path}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\" draw:mime-type=\"{mime}\"/>{desc}</draw:frame>",
            self.frames,
            pt(w),
            pt(h)
        )
    }

    fn scan(&mut self, runs: &[Run]) -> usize {
        let base = self.counter;
        for (i, r) in runs.iter().enumerate() {
            if let Some(c) = &r.style.comment {
                let e = self.comment_span.entry(c.clone()).or_insert((base + i, base + i));
                e.1 = base + i;
            }
        }
        self.counter += runs.len();
        base
    }

    fn scan_flow(&mut self, blocks: &folio_core::text::Flow) {
        for b in blocks.iter() {
            match b {
                Block::Paragraph(p) => {
                    self.scan(&p.runs);
                }
                Block::Table(t) if t.link.is_none() => {
                    for c in t.rows.iter().flatten() {
                        self.scan(&c.runs);
                    }
                }
                _ => {}
            }
        }
    }

    fn annotation(&self, c: &Comment, name: &str) -> String {
        let mut x = format!("<office:annotation office:name=\"{name}\"{}>", if c.resolved { " loext:resolved=\"true\"" } else { "" });
        let _ = write!(x, "<dc:creator>{}</dc:creator><dc:date>{}</dc:date>", esc(&c.author), c.at.format("%Y-%m-%dT%H:%M:%S"));
        for line in c.text.split('\n') {
            let _ = write!(x, "<text:p>{}</text:p>", esc(line));
        }
        for r in &c.replies {
            let _ = write!(x, "<text:p>{}: {}</text:p>", esc(&r.author), esc(&r.text));
        }
        x.push_str("</office:annotation>");
        x
    }

    /// The runs of a paragraph or cell line. `idxs` numbers them for comment ranges
    /// (`usize::MAX` for text without comments).
    fn runs(&mut self, out: &mut String, runs: &[Run], idxs: &[usize], bold: bool, code_para: bool) {
        let mut i = 0;
        while i < runs.len() {
            let link = runs[i].style.link.clone().filter(|l| !l.is_empty());
            let start = i;
            i += 1;
            while i < runs.len() && runs[i].style.link.clone().filter(|l| !l.is_empty()) == link {
                i += 1;
            }
            if let Some(url) = &link {
                let _ = write!(out, "<text:a xlink:type=\"simple\" xlink:href=\"{}\" text:style-name=\"Internet_20_link\" text:visited-style-name=\"Visited_20_Internet_20_Link\">", esc(url));
            }
            for (k, r) in runs.iter().enumerate().take(i).skip(start) {
                let idx = idxs.get(k).copied().unwrap_or(usize::MAX);
                let mut style = r.style.clone();
                if code_para {
                    style.code = false;
                }
                if let Some(c) = &r.style.comment
                    && self.comment_span.get(c).is_some_and(|s| s.0 == idx)
                    && let Some(cm) = self.comments.get(c).cloned()
                {
                    out.push_str(&self.annotation(&cm, &format!("__Annotation__{c}")));
                }
                if let Some(a) = &r.style.deleted {
                    self.changes.push(format!("<text:changed-region text:id=\"ct{}\"><text:deletion><office:change-info><dc:creator>{}</dc:creator><dc:date>{}</dc:date></office:change-info><text:p>{}</text:p></text:deletion></text:changed-region>", self.changes.len() + 1, esc(a), self.now, esc(&r.text)));
                    let _ = write!(out, "<text:change text:change-id=\"ct{}\"/>", self.changes.len());
                } else if !r.text.is_empty() {
                    let ins = r.style.inserted.as_ref().map(|a| {
                        self.changes.push(format!("<text:changed-region text:id=\"ct{}\"><text:insertion><office:change-info><dc:creator>{}</dc:creator><dc:date>{}</dc:date></office:change-info></text:insertion></text:changed-region>", self.changes.len() + 1, esc(a), self.now));
                        self.changes.len()
                    });
                    if let Some(n) = ins {
                        let _ = write!(out, "<text:change-start text:change-id=\"ct{n}\"/>");
                    }
                    match self.text_style(&style, bold) {
                        Some(t) => {
                            let _ = write!(out, "<text:span text:style-name=\"{t}\">{}</text:span>", text_xml(&r.text));
                        }
                        None => out.push_str(&text_xml(&r.text)),
                    }
                    if let Some(n) = ins {
                        let _ = write!(out, "<text:change-end text:change-id=\"ct{n}\"/>");
                    }
                }
                if let Some(n) = &r.style.note
                    && runs.get(k + 1).is_none_or(|q| q.style.note.as_ref() != Some(n))
                {
                    self.notes += 1;
                    let _ = write!(out, "<text:note text:id=\"ftn{0}\" text:note-class=\"footnote\"><text:note-citation>{0}</text:note-citation><text:note-body><text:p text:style-name=\"Footnote\">{1}</text:p></text:note-body></text:note>", self.notes, text_xml(n));
                }
                if let Some(c) = &r.style.comment
                    && self.comment_span.get(c).is_some_and(|s| s.1 == idx)
                    && self.comments.contains_key(c)
                {
                    let _ = write!(out, "<office:annotation-end office:name=\"__Annotation__{c}\"/>");
                }
            }
            if link.is_some() {
                out.push_str("</text:a>");
            }
        }
    }

    fn list_style_for(&mut self, blocks: &[Block], from: usize) -> (String, usize) {
        let mut levels: BTreeMap<u8, ListKind> = BTreeMap::new();
        let mut end = from;
        for b in &blocks[from..] {
            let Block::Paragraph(p) = b else { break };
            let Some(k) = p.list else { break };
            // ODF has one marker kind per level in a list style. Start a new list when the
            // writer switches from bullets to numbers (or checkboxes) at the same level.
            if levels.get(&p.level).is_some_and(|old| *old != k) {
                break;
            }
            levels.entry(p.level).or_insert(k);
            end += 1;
        }
        if let Some(r) = self.list_styles.iter().find(|r| r.levels == levels) {
            return (r.name.clone(), end);
        }
        let name = format!("L{}", self.list_styles.len() + 1);
        self.list_styles.push(ListRegion { name: name.clone(), levels });
        (name, end)
    }

    fn table_xml(&mut self, out: &mut String, t: &Table, text_w: f32, break_before: bool, master: Option<&str>) {
        self.tables += 1;
        let tn = format!("Table{}", self.tables);
        let rows: Vec<Vec<(Vec<Run>, Option<String>, Align, usize)>> = match &t.link {
            Some(l) => match folio_core::links::table_text(self.doc, l) {
                Ok(rows) => rows.into_iter().map(|r| r.into_iter().map(|c| (vec![Run::plain(c)], None, Align::Left, usize::MAX)).collect()).collect(),
                Err(e) => {
                    self.warnings.push(format!("A linked table couldn't read its sheet ({e}): its last values are written."));
                    t.rows.iter().map(|r| r.iter().map(|c| (c.runs.clone(), c.fill.clone(), c.align, usize::MAX)).collect()).collect()
                }
            },
            None => t
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| {
                            let base = self.counter;
                            self.counter += c.runs.len();
                            (c.runs.clone(), c.fill.clone(), c.align, base)
                        })
                        .collect()
                })
                .collect(),
        };
        if t.link.is_some() {
            self.warn("linked-table", "Tables linked to a sheet are written with their current values (they no longer follow the sheet).");
        }
        let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let fr = if t.widths.len() == cols { t.fractions() } else { vec![1.0 / cols as f32; cols] };
        let mut tprops = format!("style:width=\"{}\" table:align=\"margins\"", pt(text_w));
        if break_before && master.is_none() {
            tprops.push_str(" fo:break-before=\"page\"");
        }
        let master_attr = master.map(|m| format!(" style:master-page-name=\"{m}\"")).unwrap_or_default();
        let _ = write!(self.table_styles, "<style:style style:name=\"{tn}\" style:family=\"table\"{master_attr}><style:table-properties {tprops}/></style:style>");
        for (i, f) in fr.iter().enumerate() {
            let _ = write!(self.table_styles, "<style:style style:name=\"{tn}.C{i}\" style:family=\"table-column\"><style:table-column-properties style:column-width=\"{}\" style:rel-column-width=\"{}*\"/></style:style>", pt(f * text_w), (f * 65535.0).round() as u32);
        }
        let _ = write!(out, "<table:table table:name=\"{tn}\" table:style-name=\"{tn}\">");
        for i in 0..cols {
            let _ = write!(out, "<table:table-column table:style-name=\"{tn}.C{i}\"/>");
        }
        let mut fills: Vec<Option<String>> = vec![];
        for (ri, row) in rows.iter().enumerate() {
            let header = t.header && ri == 0;
            if header {
                out.push_str("<table:table-header-rows>");
            }
            out.push_str("<table:table-row>");
            for ci in 0..cols {
                let empty = (vec![], None, Align::Left, usize::MAX);
                let (runs, fill, align, base) = row.get(ci).unwrap_or(&empty);
                let band = t.banded && !header && (ri + usize::from(!t.header)) % 2 == 0;
                let fill = fill.clone().or(if band { Some("#f2f2f2".into()) } else { None });
                let k = match fills.iter().position(|f| *f == fill) {
                    Some(k) => k,
                    None => {
                        fills.push(fill.clone());
                        let bg = fill.as_deref().and_then(hex_color).map(|c| format!(" fo:background-color=\"{c}\"")).unwrap_or_default();
                        let _ = write!(self.table_styles, "<style:style style:name=\"{tn}.K{}\" style:family=\"table-cell\"><style:table-cell-properties fo:padding=\"3pt\" fo:border=\"0.5pt solid #bfbfbf\"{bg}/></style:style>", fills.len() - 1);
                        fills.len() - 1
                    }
                };
                let _ = write!(out, "<table:table-cell table:style-name=\"{tn}.K{k}\" office:value-type=\"string\">");
                // A line break in a cell is a new paragraph.
                let mut lines: Vec<(Vec<Run>, Vec<usize>)> = vec![(vec![], vec![])];
                for (j, r) in runs.iter().enumerate() {
                    let idx = if *base == usize::MAX { usize::MAX } else { base + j };
                    for (n, part) in r.text.split('\n').enumerate() {
                        if n > 0 {
                            lines.push((vec![], vec![]));
                        }
                        let l = lines.last_mut().unwrap();
                        l.0.push(Run { text: part.to_string(), style: r.style.clone() });
                        l.1.push(idx);
                    }
                }
                let pstyle = self.para_style("Table_20_Contents", *align, false, None, None);
                for (l, idxs) in lines {
                    let _ = write!(out, "<text:p text:style-name=\"{pstyle}\">");
                    self.runs(out, &l, &idxs, header, false);
                    out.push_str("</text:p>");
                }
                out.push_str("</table:table-cell>");
            }
            out.push_str("</table:table-row>");
            if header {
                out.push_str("</table:table-header-rows>");
            }
        }
        out.push_str("</table:table>");
    }

    fn flow(&mut self, out: &mut String, blocks: &[Block], setup: &PageSetup, master: &str) {
        let text_w = setup.text_width();
        let mut pending_break = false;
        let mut master_pending = Some(master.to_string());
        let mut i = 0;
        while i < blocks.len() {
            match &blocks[i] {
                Block::Paragraph(p) if p.list.is_some() => {
                    let (ls, end) = self.list_style_for(blocks, i);
                    let _ = write!(out, "<text:list text:style-name=\"{ls}\">");
                    let mut depth = 0u8; // open lists inside the outer one
                    let mut first = true;
                    while i < end {
                        let Block::Paragraph(p) = &blocks[i] else { break };
                        if p.list.is_none() {
                            break;
                        }
                        let target = p.level;
                        if first {
                            out.push_str("<text:list-item>");
                            for _ in 0..target {
                                out.push_str("<text:list><text:list-item>");
                            }
                            depth = target;
                            first = false;
                        } else {
                            while depth > target {
                                out.push_str("</text:list-item></text:list>");
                                depth -= 1;
                            }
                            if depth == target {
                                out.push_str("</text:list-item><text:list-item>");
                            }
                            while depth < target {
                                out.push_str("<text:list><text:list-item>");
                                depth += 1;
                            }
                        }
                        let m = master_pending.take();
                        self.paragraph(out, p, pending_break, m.as_deref());
                        pending_break = false;
                        i += 1;
                    }
                    while depth > 0 {
                        out.push_str("</text:list-item></text:list>");
                        depth -= 1;
                    }
                    out.push_str("</text:list-item></text:list>");
                    continue;
                }
                Block::Paragraph(p) => {
                    let m = master_pending.take();
                    self.paragraph(out, p, pending_break, m.as_deref());
                    pending_break = false;
                }
                Block::Table(t) => {
                    let m = master_pending.take();
                    self.table_xml(out, t, text_w, pending_break, m.as_deref());
                    pending_break = false;
                }
                Block::Image(im) => {
                    let m = master_pending.take();
                    let ps = self.para_style("Standard", im.align, pending_break, m.as_deref(), None);
                    pending_break = false;
                    if let Some((path, mime)) = self.picture(&im.media) {
                        let me = &self.doc.media[&im.media];
                        let w = if im.width > 0.0 { im.width.min(text_w) } else if me.width > 0 { (me.width as f32 * 0.75).min(text_w) } else { text_w };
                        let h = if me.width > 0 && me.height > 0 { w * me.height as f32 / me.width as f32 } else { w * 0.75 };
                        let f = self.frame(&path, &mime, w, h, &im.alt);
                        let _ = write!(out, "<text:p text:style-name=\"{ps}\">{f}</text:p>");
                        if !im.caption.is_empty() {
                            let cs = self.para_style("Caption", im.align, false, None, None);
                            let _ = write!(out, "<text:p text:style-name=\"{cs}\">{}</text:p>", text_xml(&im.caption));
                        }
                    } else {
                        let _ = write!(out, "<text:p text:style-name=\"{ps}\"/>");
                    }
                }
                Block::Chart(c) => {
                    let width = text_w;
                    let height = c.height.max(36.0);
                    let png = match folio_core::links::chart_data(self.doc, &c.chart) {
                        Ok(data) => folio_layout::raster::chart_png(&c.chart, &data, (width * 2.0) as u32, (height * 2.0) as u32, &folio_layout::chart::ChartStyle::default()),
                        Err(_) => vec![],
                    };
                    if png.is_empty() {
                        self.warn("chart-missing", "Charts couldn't be drawn as pictures and are left out (export as PDF to keep them).");
                        i += 1;
                        continue;
                    }
                    self.warn("chart", "Charts became pictures: they no longer follow their sheet.");
                    let path = format!("Pictures/chart{}.png", self.pictures.len() + 1);
                    self.pictures.push((path.clone(), "image/png".into(), png));
                    let m = master_pending.take();
                    let ps = self.para_style("Standard", Align::Left, pending_break, m.as_deref(), None);
                    pending_break = false;
                    let f = self.frame(&path, "image/png", width, height, &c.chart.title);
                    let _ = write!(out, "<text:p text:style-name=\"{ps}\">{f}</text:p>");
                }
                Block::PageBreak { .. } => pending_break = true,
            }
            i += 1;
        }
        if pending_break || master_pending.is_some() {
            let m = master_pending.take();
            let ps = self.para_style("Standard", Align::Left, pending_break, m.as_deref(), None);
            let _ = write!(out, "<text:p text:style-name=\"{ps}\"/>");
        }
    }

    fn paragraph(&mut self, out: &mut String, p: &folio_core::Paragraph, break_before: bool, master: Option<&str>) {
        let parent = style_name(p.style);
        let ps = self.para_style(parent, p.align, break_before, master, None);
        let base = self.counter;
        self.counter += p.runs.len();
        let (open, close) = match p.style.level().filter(|_| p.style != ParaStyle::Title) {
            Some(l) => (format!("<text:h text:style-name=\"{ps}\" text:outline-level=\"{l}\">"), "</text:h>"),
            None => (format!("<text:p text:style-name=\"{ps}\">"), "</text:p>"),
        };
        out.push_str(&open);
        if p.list == Some(ListKind::Check) {
            out.push_str(if p.checked { "☒ " } else { "☐ " });
        }
        let idxs: Vec<usize> = (base..base + p.runs.len()).collect();
        self.runs(out, &p.runs, &idxs, false, p.style == ParaStyle::Code);
        out.push_str(close);
    }

    fn master_for(&mut self, setup: &PageSetup) -> String {
        if let Some((n, _)) = self.masters.iter().find(|(_, s)| s == setup) {
            return n.clone();
        }
        let n = if self.masters.is_empty() { "Standard".to_string() } else { format!("folio_{}", self.masters.len() + 1) };
        self.masters.push((n.clone(), setup.clone()));
        n
    }
}

fn hf_xml(text: &str, title: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let next = ["{page}", "{pages}", "{title}"].iter().filter_map(|k| rest.find(k).map(|i| (i, *k))).min();
        match next {
            Some((i, k)) => {
                out.push_str(&text_xml(&rest[..i]));
                match k {
                    "{page}" => out.push_str("<text:page-number text:select-page=\"current\">1</text:page-number>"),
                    "{pages}" => out.push_str("<text:page-count>1</text:page-count>"),
                    _ => out.push_str(&esc(title)),
                }
                rest = &rest[i + k.len()..];
            }
            None => {
                out.push_str(&text_xml(rest));
                rest = "";
            }
        }
    }
    out
}

fn font_decls() -> &'static str {
    "<office:font-face-decls><style:font-face style:name=\"IBM Plex Sans\" svg:font-family=\"&apos;IBM Plex Sans&apos;\" style:font-family-generic=\"swiss\" style:font-pitch=\"variable\"/><style:font-face style:name=\"IBM Plex Serif\" svg:font-family=\"&apos;IBM Plex Serif&apos;\" style:font-family-generic=\"roman\" style:font-pitch=\"variable\"/><style:font-face style:name=\"IBM Plex Mono\" svg:font-family=\"&apos;IBM Plex Mono&apos;\" style:font-family-generic=\"modern\" style:font-pitch=\"fixed\"/><style:font-face style:name=\"Chakra Petch\" svg:font-family=\"&apos;Chakra Petch&apos;\" style:font-family-generic=\"swiss\" style:font-pitch=\"variable\"/></office:font-face-decls>"
}

fn styles_xml(masters: &[(String, PageSetup)], title: &str) -> String {
    let mut x = format!("{XML_HEAD}<office:document-styles {NS} office:version=\"1.3\">{}<office:styles>", font_decls());
    x.push_str("<style:default-style style:family=\"paragraph\"><style:paragraph-properties style:writing-mode=\"page\"/><style:text-properties style:font-name=\"IBM Plex Sans\" fo:font-size=\"11pt\" style:font-size-asian=\"11pt\" style:font-size-complex=\"11pt\" fo:language=\"en\" fo:country=\"US\" fo:hyphenate=\"false\"/></style:default-style>");
    x.push_str("<style:default-style style:family=\"table\"><style:table-properties table:border-model=\"collapsing\"/></style:default-style>");
    x.push_str("<style:default-style style:family=\"graphic\"><style:graphic-properties svg:stroke-color=\"#3465a4\" draw:fill-color=\"#729fcf\" style:flow-with-text=\"false\"/></style:default-style>");
    for s in ParaStyle::ALL {
        let spec = s.spec();
        let name = style_name(s);
        let display = match s {
            ParaStyle::Normal => "Default Paragraph Style",
            ParaStyle::Heading1 => "Heading 1",
            ParaStyle::Heading2 => "Heading 2",
            ParaStyle::Heading3 => "Heading 3",
            ParaStyle::Code => "Preformatted Text",
            _ => s.label(),
        };
        let class = match s {
            ParaStyle::Normal => "text",
            ParaStyle::Title | ParaStyle::Subtitle | ParaStyle::Heading1 | ParaStyle::Heading2 | ParaStyle::Heading3 => "chapter",
            ParaStyle::Caption => "extra",
            _ => "html",
        };
        let parent = if s == ParaStyle::Normal { String::new() } else { " style:parent-style-name=\"Standard\" style:next-style-name=\"Standard\"".into() };
        let outline = s.level().filter(|_| s != ParaStyle::Title).map(|l| format!(" style:default-outline-level=\"{l}\"")).unwrap_or_default();
        let _ = write!(x, "<style:style style:name=\"{name}\" style:display-name=\"{display}\" style:family=\"paragraph\"{parent}{outline} style:class=\"{class}\">");
        let mut pp = format!(" fo:margin-top=\"{}\" fo:margin-bottom=\"{}\" fo:line-height=\"{}%\"", pt(spec.space_before), pt(spec.space_after), (spec.line * 100.0).round());
        if s.is_heading() {
            pp.push_str(" fo:keep-with-next=\"always\"");
        }
        if s == ParaStyle::Quote {
            pp.push_str(" fo:margin-left=\"14pt\" fo:margin-right=\"14pt\" fo:border-left=\"1.5pt solid #bfbfbf\" fo:padding-left=\"8pt\"");
        }
        if s == ParaStyle::Code {
            pp.push_str(" fo:background-color=\"#f4f4f4\"");
        }
        let _ = write!(x, "<style:paragraph-properties{pp}/>");
        let mut tp = format!(" fo:font-size=\"{0}\" style:font-size-asian=\"{0}\" style:font-size-complex=\"{0}\"", pt(spec.size));
        if spec.family != folio_core::text::Family::Sans {
            let _ = write!(tp, " style:font-name=\"{}\"", spec.family.font_name());
        }
        if spec.bold {
            tp.push_str(" fo:font-weight=\"bold\" style:font-weight-asian=\"bold\" style:font-weight-complex=\"bold\"");
        }
        if spec.italic {
            tp.push_str(" fo:font-style=\"italic\" style:font-style-asian=\"italic\" style:font-style-complex=\"italic\"");
        }
        if spec.muted {
            tp.push_str(" fo:color=\"#595959\"");
        }
        let _ = write!(x, "<style:text-properties{tp}/></style:style>");
    }
    x.push_str("<style:style style:name=\"Table_20_Contents\" style:display-name=\"Table Contents\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:class=\"extra\"><style:paragraph-properties fo:margin-top=\"0pt\" fo:margin-bottom=\"0pt\" fo:line-height=\"120%\"/></style:style>");
    x.push_str("<style:style style:name=\"Footnote\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:class=\"extra\"><style:paragraph-properties fo:margin-left=\"12pt\" fo:text-indent=\"-12pt\" fo:margin-top=\"0pt\" fo:margin-bottom=\"0pt\" fo:line-height=\"120%\"/><style:text-properties fo:font-size=\"9pt\" style:font-size-asian=\"9pt\" style:font-size-complex=\"9pt\"/></style:style>");
    x.push_str("<style:style style:name=\"Header\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:class=\"extra\"><style:paragraph-properties fo:text-align=\"center\" fo:margin-top=\"0pt\" fo:margin-bottom=\"0pt\"/><style:text-properties fo:color=\"#595959\" fo:font-size=\"9pt\"/></style:style>");
    x.push_str("<style:style style:name=\"Footer\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:class=\"extra\"><style:paragraph-properties fo:text-align=\"center\" fo:margin-top=\"0pt\" fo:margin-bottom=\"0pt\"/><style:text-properties fo:color=\"#595959\" fo:font-size=\"9pt\"/></style:style>");
    x.push_str("<style:style style:name=\"Internet_20_link\" style:display-name=\"Internet Link\" style:family=\"text\"><style:text-properties fo:color=\"#1d4ed8\" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"/></style:style>");
    x.push_str("<style:style style:name=\"Visited_20_Internet_20_Link\" style:display-name=\"Visited Internet Link\" style:family=\"text\"><style:text-properties fo:color=\"#5b21b6\" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"/></style:style>");
    x.push_str("<text:notes-configuration text:note-class=\"footnote\" style:num-format=\"1\" text:start-value=\"0\" text:footnotes-position=\"page\" text:start-numbering-at=\"document\"/>");
    x.push_str("</office:styles><office:automatic-styles>");
    for (i, (_, s)) in masters.iter().enumerate() {
        let orient = if s.width > s.height { "landscape" } else { "portrait" };
        // ODF headers sit inside the page margin: keep the body where folio puts it.
        let hh = if s.header.trim().is_empty() { 0.0 } else { (s.margin_top / 2.0).min(28.0) };
        let fh = if s.footer.trim().is_empty() { 0.0 } else { (s.margin_bottom / 2.0).min(28.0) };
        let _ = write!(
            x,
            "<style:page-layout style:name=\"pm{}\"><style:page-layout-properties fo:page-width=\"{}\" fo:page-height=\"{}\" style:print-orientation=\"{orient}\" fo:margin-top=\"{}\" fo:margin-bottom=\"{}\" fo:margin-left=\"{}\" fo:margin-right=\"{}\" style:writing-mode=\"lr-tb\"/>",
            i + 1,
            pt(s.width),
            pt(s.height),
            pt(s.margin_top - hh),
            pt(s.margin_bottom - fh),
            pt(s.margin_left),
            pt(s.margin_right)
        );
        let _ = write!(x, "<style:header-style>{}</style:header-style>", if hh > 0.0 { format!("<style:header-footer-properties fo:min-height=\"{0}\" fo:margin-bottom=\"{0}\"/>", pt(hh * 0.5)) } else { String::new() });
        let _ = write!(x, "<style:footer-style>{}</style:footer-style></style:page-layout>", if fh > 0.0 { format!("<style:header-footer-properties fo:min-height=\"{0}\" fo:margin-top=\"{0}\"/>", pt(fh * 0.5)) } else { String::new() });
    }
    x.push_str("</office:automatic-styles><office:master-styles>");
    for (i, (name, s)) in masters.iter().enumerate() {
        let first = if s.different_first { format!(" style:next-style-name=\"{name}\"") } else { String::new() };
        let _ = first;
        let _ = write!(x, "<style:master-page style:name=\"{name}\" style:page-layout-name=\"pm{}\">", i + 1);
        if !s.header.trim().is_empty() {
            let _ = write!(x, "<style:header><text:p text:style-name=\"Header\">{}</text:p></style:header>", hf_xml(s.header.trim(), title));
            if s.different_first {
                x.push_str("<style:header-first style:display=\"false\"/>");
            }
        }
        if !s.footer.trim().is_empty() {
            let _ = write!(x, "<style:footer><text:p text:style-name=\"Footer\">{}</text:p></style:footer>", hf_xml(s.footer.trim(), title));
            if s.different_first {
                x.push_str("<style:footer-first style:display=\"false\"/>");
            }
        }
        x.push_str("</style:master-page>");
    }
    x.push_str("</office:master-styles></office:document-styles>");
    x
}

fn list_style_xml(r: &ListRegion) -> String {
    let mut x = format!("<text:list-style style:name=\"{}\">", r.name);
    for lvl in 1..=10u8 {
        let kind = r.levels.get(&(lvl - 1)).copied().or_else(|| r.levels.values().next().copied()).unwrap_or(ListKind::Bullet);
        let indent = 18.0 * lvl as f32;
        let props = format!("<style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\"><style:list-level-label-alignment text:label-followed-by=\"listtab\" text:list-tab-stop-position=\"{0}\" fo:text-indent=\"-12pt\" fo:margin-left=\"{0}\"/></style:list-level-properties>", pt(indent));
        match kind {
            ListKind::Bullet => {
                let ch = ["•", "◦", "▪"][(lvl as usize - 1) % 3];
                let _ = write!(x, "<text:list-level-style-bullet text:level=\"{lvl}\" text:bullet-char=\"{ch}\">{props}</text:list-level-style-bullet>");
            }
            ListKind::Number => {
                let fmt = ["1", "a", "i"][(lvl as usize - 1) % 3];
                let _ = write!(x, "<text:list-level-style-number text:level=\"{lvl}\" style:num-suffix=\".\" style:num-format=\"{fmt}\">{props}</text:list-level-style-number>");
            }
            ListKind::Check => {
                let _ = write!(x, "<text:list-level-style-number text:level=\"{lvl}\" style:num-format=\"\">{props}</text:list-level-style-number>");
            }
        }
    }
    x.push_str("</text:list-style>");
    x
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let now = chrono::Utc::now();
    let mut w = Writer {
        doc,
        warnings: vec![],
        warned: HashSet::new(),
        text_styles: vec![],
        para_styles: vec![],
        table_styles: String::new(),
        list_styles: vec![],
        tables: 0,
        frames: 0,
        pictures: vec![],
        media: HashMap::new(),
        notes: 0,
        comment_span: HashMap::new(),
        comments: HashMap::new(),
        counter: 0,
        changes: vec![],
        masters: vec![],
        now: now.format("%Y-%m-%dT%H:%M:%S").to_string(),
    };
    let mut body = String::new();
    let mut any = false;
    let mut track = false;
    for &i in pages {
        let Some(p) = doc.pages.get(i) else { continue };
        match &p.body {
            PageBody::Doc(t) => {
                any = true;
                track |= t.track_changes;
                // Comment ranges: where each comment's text starts and ends.
                let saved = w.counter;
                w.scan_flow(&t.blocks);
                w.counter = saved;
                let attached: HashSet<&Id> = t.comments.iter().filter(|c| w.comment_span.contains_key(&c.id)).map(|c| &c.id).collect();
                let orphans = t.comments.len() - attached.len();
                if orphans > 0 {
                    w.warnings.push(format!("{orphans} comment{} no longer attached to any text {} left out.", if orphans == 1 { "" } else { "s" }, if orphans == 1 { "is" } else { "are" }));
                }
                for c in &t.comments {
                    w.comments.insert(c.id.clone(), c.clone());
                }
                let master = w.master_for(&t.setup);
                let blocks: Vec<Block> = t.blocks.iter().cloned().collect();
                w.flow(&mut body, &blocks, &t.setup, &master);
            }
            PageBody::Sheet(s) => {
                let mut setup = PageSetup::a4();
                match sheet_table(s, 2000, 40) {
                    Some((t, cut)) => {
                        any = true;
                        if t.cols() > 7 {
                            std::mem::swap(&mut setup.width, &mut setup.height);
                        }
                        let master = w.master_for(&setup);
                        let blocks = vec![Block::Paragraph(folio_core::Paragraph::new(ParaStyle::Heading1, p.name.clone())), Block::Table(t)];
                        w.flow(&mut body, &blocks, &setup, &master);
                        w.warnings.push(format!("\"{}\" is a sheet: written as a table of its values (formulas{} are left out).", p.name, if s.charts.is_empty() { "" } else { " and charts" }));
                        if cut {
                            w.warnings.push(format!("\"{}\" is too big for a document: only its first 2000 rows and 40 columns are written.", p.name));
                        }
                    }
                    None => w.warnings.push(format!("\"{}\" is an empty sheet.", p.name)),
                }
            }
            PageBody::Deck(_) => w.warnings.push(format!("\"{}\" is a deck and OpenDocument text only carries documents: left out (export it as ODP, PPTX or PDF).", p.name)),
        }
    }
    if !any {
        return Err("Nothing to write: OpenDocument text carries documents and sheets, and none were chosen.".into());
    }

    let mut auto = String::new();
    for (n, props) in &w.text_styles {
        let _ = write!(auto, "<style:style style:name=\"{n}\" style:family=\"text\"><style:text-properties{props}/></style:style>");
    }
    for (n, body) in &w.para_styles {
        let _ = write!(auto, "<style:style style:name=\"{n}\"{body}</style:style>");
    }
    auto.push_str(&w.table_styles);
    auto.push_str("<style:style style:name=\"fr1\" style:family=\"graphic\"><style:graphic-properties style:vertical-pos=\"top\" style:vertical-rel=\"baseline\" style:horizontal-pos=\"center\" style:horizontal-rel=\"paragraph\" fo:border=\"none\" style:mirror=\"none\" fo:clip=\"rect(0pt, 0pt, 0pt, 0pt)\" draw:luminance=\"0%\" draw:contrast=\"0%\" draw:red=\"0%\" draw:green=\"0%\" draw:blue=\"0%\" draw:gamma=\"100%\" draw:color-inversion=\"false\" draw:image-opacity=\"100%\" draw:color-mode=\"standard\"/></style:style>");
    for r in &w.list_styles {
        auto.push_str(&list_style_xml(r));
    }
    let changes = if w.changes.is_empty() {
        String::new()
    } else {
        format!("<text:tracked-changes text:track-changes=\"{}\">{}</text:tracked-changes>", track, w.changes.join(""))
    };
    let content = format!(
        "{XML_HEAD}<office:document-content {NS} office:version=\"1.3\">{}<office:automatic-styles>{auto}</office:automatic-styles><office:body><office:text>{changes}<text:sequence-decls><text:sequence-decl text:display-outline-level=\"0\" text:name=\"Illustration\"/><text:sequence-decl text:display-outline-level=\"0\" text:name=\"Table\"/><text:sequence-decl text:display-outline-level=\"0\" text:name=\"Text\"/><text:sequence-decl text:display-outline-level=\"0\" text:name=\"Drawing\"/><text:sequence-decl text:display-outline-level=\"0\" text:name=\"Figure\"/></text:sequence-decls>{body}</office:text></office:body></office:document-content>",
        font_decls()
    );
    let styles = styles_xml(&w.masters, &doc.title);
    let created = doc.meta.created.unwrap_or(now).format("%Y-%m-%dT%H:%M:%S").to_string();
    let modified = doc.meta.modified.unwrap_or(now).format("%Y-%m-%dT%H:%M:%S").to_string();
    let author = esc(&doc.meta.author);
    let meta = format!(
        "{XML_HEAD}<office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>folio</meta:generator><dc:title>{}</dc:title><meta:initial-creator>{author}</meta:initial-creator><dc:creator>{author}</dc:creator><meta:creation-date>{created}</meta:creation-date><dc:date>{modified}</dc:date></office:meta></office:document-meta>",
        esc(&doc.title)
    );
    let mut manifest = format!("{XML_HEAD}<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/>");
    for (path, mime, _) in &w.pictures {
        let _ = write!(manifest, "<manifest:file-entry manifest:full-path=\"{path}\" manifest:media-type=\"{mime}\"/>");
    }
    manifest.push_str("</manifest:manifest>");

    let mut z = ZipOut::new();
    z.add_stored("mimetype", b"application/vnd.oasis.opendocument.text")?;
    z.add("META-INF/manifest.xml", manifest.as_bytes())?;
    z.add("content.xml", content.as_bytes())?;
    z.add("styles.xml", styles.as_bytes())?;
    z.add("meta.xml", meta.as_bytes())?;
    for (path, _, bytes) in &w.pictures {
        z.add_stored(path, bytes)?;
    }
    Ok((z.finish()?, w.warnings))
}
