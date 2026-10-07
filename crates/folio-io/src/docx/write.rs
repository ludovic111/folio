//! DOCX out: folio pages to a WordprocessingML package that Word, LibreOffice, Google Docs and
//! Pages open cleanly.
//!
//! Every document page is a section (its own page size, margins, header and footer); sheets go in
//! as tables of their values; decks are left out. Child elements are written in the order the
//! schema gives (Word refuses files where they aren't).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use folio_core::text::{Comment, PageSetup, Table};
use folio_core::{Align, Block, Document, Id, ListKind, PageBody, ParaStyle, Run, RunStyle};

use super::xml::{ZipOut, esc};

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const NS_W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const NS_W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";
const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Word's highlight names for the colours it has.
const HIGHLIGHTS: &[(&str, &str)] = &[
    ("#ffff00", "yellow"),
    ("#00ff00", "green"),
    ("#00ffff", "cyan"),
    ("#ff00ff", "magenta"),
    ("#0000ff", "blue"),
    ("#ff0000", "red"),
    ("#000080", "darkBlue"),
    ("#008080", "darkCyan"),
    ("#008000", "darkGreen"),
    ("#800080", "darkMagenta"),
    ("#800000", "darkRed"),
    ("#808000", "darkYellow"),
    ("#808080", "darkGray"),
    ("#c0c0c0", "lightGray"),
    ("#000000", "black"),
];

pub(crate) fn face(family: &str) -> String {
    match family {
        "sans" => "IBM Plex Sans".into(),
        "serif" => "IBM Plex Serif".into(),
        "mono" => "IBM Plex Mono".into(),
        "display" => "Chakra Petch".into(),
        other => other.to_string(),
    }
}

fn family_face(f: folio_core::text::Family) -> &'static str {
    f.font_name()
}

fn hex6(c: &str) -> Option<String> {
    super::xml::hex_color(c).map(|h| h[1..].to_ascii_uppercase())
}

fn tw(pt: f32) -> i64 {
    (pt * 20.0).round() as i64
}

fn emu(pt: f32) -> i64 {
    (pt as f64 * 12700.0).round() as i64
}

/// A piece of a run, numbered so comments know where they start and end.
#[derive(Clone)]
struct Piece {
    idx: usize,
    /// The last piece of its run (runs in table cells split at line breaks).
    last: bool,
    text: String,
    style: RunStyle,
}

#[derive(Clone)]
struct CellOut {
    lines: Vec<Vec<Piece>>,
    fill: Option<String>,
    align: Align,
}

/// A block ready to write.
enum Out {
    Para { style: ParaStyle, align: Align, list: Option<ListKind>, level: u8, pieces: Vec<Piece> },
    Table { rows: Vec<Vec<CellOut>>, header: bool, widths: Vec<f32>, banded: bool },
    Image { media: Id, width: f32, alt: String, caption: String, align: Align },
    /// A chart as a PNG.
    Picture { png: Vec<u8>, width: f32, height: f32, caption: String },
    PageBreak,
}

struct Section {
    setup: PageSetup,
    blocks: Vec<Out>,
}

struct CommentOut {
    wid: usize,
    author: String,
    date: String,
    text: String,
    para_id: String,
    parent: Option<String>,
    done: bool,
}

struct Writer<'a> {
    doc: &'a Document,
    warnings: Vec<String>,
    warned: HashSet<&'static str>,
    /// document.xml.rels: (id, type, target, external).
    rels: Vec<(String, String, String, bool)>,
    links: HashMap<String, String>,
    media: HashMap<Id, Option<String>>,
    files: Vec<(String, Vec<u8>)>,
    exts: BTreeSet<&'static str>,
    pictures: usize,
    drawings: usize,
    footnotes: Vec<String>,
    comments: Vec<CommentOut>,
    /// A folio comment → its Word comment ids (the comment, then its replies).
    comment_wids: HashMap<Id, Vec<usize>>,
    comment_span: HashMap<Id, (usize, usize)>,
    started: HashSet<Id>,
    counter: usize,
    revision: usize,
    para_ids: u32,
    /// numId → abstractNumId, and whether it restarts numbering.
    nums: Vec<(usize, usize)>,
    hf_parts: Vec<(String, String, bool)>,
    hf_cache: HashMap<(bool, String), String>,
    now: String,
}

impl<'a> Writer<'a> {
    fn warn(&mut self, key: &'static str, msg: impl Into<String>) {
        if self.warned.insert(key) {
            self.warnings.push(msg.into());
        }
    }

    fn rel(&mut self, kind: &str, target: &str, external: bool) -> String {
        let id = format!("rId{}", self.rels.len() + 1);
        self.rels.push((id.clone(), format!("{REL}/{kind}"), target.to_string(), external));
        id
    }

    fn link_rel(&mut self, url: &str) -> String {
        if let Some(r) = self.links.get(url) {
            return r.clone();
        }
        let r = self.rel("hyperlink", url, true);
        self.links.insert(url.to_string(), r.clone());
        r
    }

    /// A picture part for some media; `None` when Word can't show its format.
    fn media_rel(&mut self, id: &Id) -> Option<String> {
        if let Some(r) = self.media.get(id) {
            return r.clone();
        }
        let got = self.doc.media.get(id).and_then(|m| {
            let (bytes, ext): (Vec<u8>, &'static str) = match m.mime.as_str() {
                "image/png" => (m.bytes.to_vec(), "png"),
                "image/jpeg" => (m.bytes.to_vec(), "jpeg"),
                "image/gif" => (m.bytes.to_vec(), "gif"),
                "image/bmp" => (m.bytes.to_vec(), "bmp"),
                "image/svg+xml" => return None,
                _ => {
                    let img = image::load_from_memory(&m.bytes).ok()?;
                    let mut out = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
                    (out.into_inner(), "png")
                }
            };
            Some((bytes, ext))
        });
        let r = match got {
            Some((bytes, ext)) => Some(self.add_picture(bytes, ext)),
            None => {
                self.warn("svg", "SVG pictures and other formats Word can't show are left out (export as PDF or HTML to keep them).");
                None
            }
        };
        self.media.insert(id.clone(), r.clone());
        r
    }

    fn add_picture(&mut self, bytes: Vec<u8>, ext: &'static str) -> String {
        self.pictures += 1;
        let name = format!("media/image{}.{ext}", self.pictures);
        self.exts.insert(ext);
        self.files.push((format!("word/{name}"), bytes));
        self.rel("image", &name, false)
    }

    // ---- preparing blocks ---------------------------------------------------------------------

    fn pieces(&mut self, runs: &[Run]) -> Vec<Piece> {
        let base = self.counter;
        self.counter += runs.len();
        let out: Vec<Piece> = runs.iter().enumerate().map(|(i, r)| Piece { idx: base + i, last: true, text: r.text.clone(), style: r.style.clone() }).collect();
        for p in &out {
            self.note_comment(p);
        }
        out
    }

    fn note_comment(&mut self, p: &Piece) {
        if let Some(c) = &p.style.comment {
            let e = self.comment_span.entry(c.clone()).or_insert((p.idx, p.idx));
            e.0 = e.0.min(p.idx);
            e.1 = e.1.max(p.idx);
        }
    }

    /// Cell runs as lines of pieces (a `\n` starts a new paragraph in the cell).
    fn cell_lines(&mut self, runs: &[Run]) -> Vec<Vec<Piece>> {
        let base = self.counter;
        self.counter += runs.len();
        let mut lines: Vec<Vec<Piece>> = vec![vec![]];
        for (i, r) in runs.iter().enumerate() {
            let parts: Vec<&str> = r.text.split('\n').collect();
            let n = parts.len();
            for (j, part) in parts.into_iter().enumerate() {
                if j > 0 {
                    lines.push(vec![]);
                }
                let p = Piece { idx: base + i, last: j + 1 == n, text: part.to_string(), style: r.style.clone() };
                if !part.is_empty() || j + 1 == n {
                    lines.last_mut().unwrap().push(p);
                }
            }
        }
        for l in lines.clone() {
            for p in &l {
                self.note_comment(p);
            }
        }
        lines
    }

    fn table_out(&mut self, t: &Table) -> Out {
        let rows: Vec<Vec<CellOut>> = match &t.link {
            Some(l) => match folio_core::links::table_text(self.doc, l) {
                Ok(rows) => rows.into_iter().map(|r| r.into_iter().map(|c| CellOut { lines: c.split('\n').map(|l| vec![Piece { idx: usize::MAX, last: false, text: l.to_string(), style: RunStyle::default() }]).collect(), fill: None, align: Align::Left }).collect()).collect(),
                Err(e) => {
                    self.warnings.push(format!("A linked table couldn't read its sheet ({e}): its last values are written."));
                    self.cells_of(t)
                }
            },
            None => self.cells_of(t),
        };
        if t.link.is_some() {
            self.warn("linked-table", "Tables linked to a sheet are written with their current values (they no longer follow the sheet).");
        }
        Out::Table { rows, header: t.header, widths: t.fractions(), banded: t.banded }
    }

    fn cells_of(&mut self, t: &Table) -> Vec<Vec<CellOut>> {
        t.rows.iter().map(|r| r.iter().map(|c| CellOut { lines: self.cell_lines(&c.runs), fill: c.fill.clone(), align: c.align }).collect()).collect()
    }

    fn flow_out(&mut self, blocks: &folio_core::text::Flow, setup: &PageSetup) -> Vec<Out> {
        let mut out = vec![];
        for b in blocks.iter() {
            match b {
                Block::Paragraph(p) => {
                    let mut pieces = vec![];
                    if p.list == Some(ListKind::Check) {
                        pieces.push(Piece { idx: usize::MAX, last: false, text: if p.checked { "☒ ".into() } else { "☐ ".into() }, style: RunStyle::default() });
                    }
                    pieces.extend(self.pieces(&p.runs));
                    out.push(Out::Para { style: p.style, align: p.align, list: p.list, level: p.level.min(5), pieces });
                }
                Block::Table(t) => out.push(self.table_out(t)),
                Block::Image(im) => out.push(Out::Image { media: im.media.clone(), width: im.width, alt: im.alt.clone(), caption: im.caption.clone(), align: im.align }),
                Block::Chart(c) => {
                    let width = setup.text_width();
                    let height = c.height.max(36.0);
                    let png = match folio_core::links::chart_data(self.doc, &c.chart) {
                        Ok(data) => folio_layout::raster::chart_png(&c.chart, &data, (width * 2.0) as u32, (height * 2.0) as u32, &folio_layout::chart::ChartStyle::default()),
                        Err(_) => vec![],
                    };
                    if png.is_empty() {
                        self.warn("chart-missing", "Charts couldn't be drawn as pictures and are left out (export as PDF to keep them).");
                    } else {
                        self.warn("chart", "Charts became pictures: they no longer follow their sheet.");
                        out.push(Out::Picture { png, width, height, caption: c.chart.title.clone() });
                    }
                }
                Block::PageBreak { .. } => out.push(Out::PageBreak),
            }
        }
        out
    }

    fn register_comments(&mut self, comments: &[Comment]) {
        for c in comments {
            if !self.comment_span.contains_key(&c.id) || self.comment_wids.contains_key(&c.id) {
                continue;
            }
            let mut wids = vec![];
            let wid = self.comments.len();
            let pid = self.next_para_id();
            wids.push(wid);
            self.comments.push(CommentOut { wid, author: c.author.clone(), date: c.at.format("%Y-%m-%dT%H:%M:%SZ").to_string(), text: c.text.clone(), para_id: pid.clone(), parent: None, done: c.resolved });
            for r in &c.replies {
                let rw = self.comments.len();
                let rp = self.next_para_id();
                wids.push(rw);
                self.comments.push(CommentOut { wid: rw, author: r.author.clone(), date: r.at.format("%Y-%m-%dT%H:%M:%SZ").to_string(), text: r.text.clone(), para_id: rp, parent: Some(pid.clone()), done: c.resolved });
            }
            self.comment_wids.insert(c.id.clone(), wids);
        }
        let orphans = comments.iter().filter(|c| !self.comment_span.contains_key(&c.id)).count();
        if orphans > 0 {
            self.warnings.push(format!("{orphans} comment{} no longer attached to any text {} left out.", if orphans == 1 { "" } else { "s" }, if orphans == 1 { "is" } else { "are" }));
        }
    }

    fn next_para_id(&mut self) -> String {
        self.para_ids += 1;
        format!("{:08X}", 0x1000_0000 + self.para_ids)
    }

    // ---- writing ------------------------------------------------------------------------------

    fn rpr(&self, s: &RunStyle, extra_bold: bool) -> String {
        let mut x = String::new();
        if s.link.is_some() {
            x.push_str("<w:rStyle w:val=\"Hyperlink\"/>");
        }
        let font = if s.code { Some("IBM Plex Mono".to_string()) } else { s.font.as_deref().map(face) };
        if let Some(f) = font {
            let f = esc(&f);
            let _ = write!(x, "<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:eastAsia=\"{f}\" w:cs=\"{f}\"/>");
        }
        if s.bold || extra_bold {
            x.push_str("<w:b/>");
        }
        if s.italic {
            x.push_str("<w:i/>");
        }
        if s.strike {
            x.push_str("<w:strike/>");
        }
        if let Some(c) = s.color.as_deref().and_then(hex6) {
            let _ = write!(x, "<w:color w:val=\"{c}\"/>");
        }
        if let Some(sz) = s.size.filter(|v| *v > 0.0) {
            let hp = (sz * 2.0).round().clamp(2.0, 3276.0) as i64;
            let _ = write!(x, "<w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/>");
        }
        let hl = s.highlight.as_deref().and_then(super::xml::hex_color);
        if let Some(h) = &hl
            && let Some((_, name)) = HIGHLIGHTS.iter().find(|(hex, _)| hex == h)
        {
            let _ = write!(x, "<w:highlight w:val=\"{name}\"/>");
        }
        if s.underline {
            x.push_str("<w:u w:val=\"single\"/>");
        }
        if let Some(h) = &hl
            && !HIGHLIGHTS.iter().any(|(hex, _)| hex == h)
        {
            let _ = write!(x, "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", h[1..].to_ascii_uppercase());
        }
        if s.superscript {
            x.push_str("<w:vertAlign w:val=\"superscript\"/>");
        } else if s.subscript {
            x.push_str("<w:vertAlign w:val=\"subscript\"/>");
        }
        if x.is_empty() { x } else { format!("<w:rPr>{x}</w:rPr>") }
    }

    fn text_xml(text: &str, deleted: bool) -> String {
        let tag = if deleted { "w:delText" } else { "w:t" };
        let mut x = String::new();
        let mut first = true;
        for line in text.split('\n') {
            if !first {
                x.push_str("<w:br/>");
            }
            first = false;
            let mut tfirst = true;
            for part in line.split('\t') {
                if !tfirst {
                    x.push_str("<w:tab/>");
                }
                tfirst = false;
                if !part.is_empty() {
                    let _ = write!(x, "<{tag} xml:space=\"preserve\">{}</{tag}>", esc(part));
                }
            }
        }
        x
    }

    fn piece(&mut self, out: &mut String, pieces: &[Piece], i: usize, bold: bool) {
        let p = &pieces[i];
        if let Some(c) = &p.style.comment
            && self.comment_span.get(c).is_some_and(|s| s.0 == p.idx)
            && !self.started.contains(c)
            && let Some(wids) = self.comment_wids.get(c)
        {
            for w in wids {
                let _ = write!(out, "<w:commentRangeStart w:id=\"{w}\"/>");
            }
            self.started.insert(c.clone());
        }
        if !p.text.is_empty() {
            let deleted = p.style.deleted.is_some();
            let run = format!("<w:r>{}{}</w:r>", self.rpr(&p.style, bold), Self::text_xml(&p.text, deleted));
            if let Some(a) = &p.style.deleted {
                self.revision += 1;
                let _ = write!(out, "<w:del w:id=\"{}\" w:author=\"{}\" w:date=\"{}\">{run}</w:del>", self.revision, esc(a), self.now);
            } else if let Some(a) = &p.style.inserted {
                self.revision += 1;
                let _ = write!(out, "<w:ins w:id=\"{}\" w:author=\"{}\" w:date=\"{}\">{run}</w:ins>", self.revision, esc(a), self.now);
            } else {
                out.push_str(&run);
            }
        }
        if let Some(n) = &p.style.note
            && (p.last || i + 1 == pieces.len())
            && pieces.get(i + 1).is_none_or(|q| q.style.note.as_ref() != Some(n))
        {
            self.footnotes.push(n.clone());
            let _ = write!(out, "<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteReference w:id=\"{}\"/></w:r>", self.footnotes.len());
        }
        if let Some(c) = &p.style.comment
            && p.last
            && self.comment_span.get(c).is_some_and(|s| s.1 == p.idx)
            && let Some(wids) = self.comment_wids.get(c)
        {
            for w in wids {
                let _ = write!(out, "<w:commentRangeEnd w:id=\"{w}\"/><w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:commentReference w:id=\"{w}\"/></w:r>");
            }
        }
    }

    fn runs(&mut self, out: &mut String, pieces: &[Piece], bold: bool, code_para: bool) {
        let pieces: Vec<Piece> = if code_para {
            pieces.iter().cloned().map(|mut p| {
                p.style.code = false;
                p
            }).collect()
        } else {
            pieces.to_vec()
        };
        let mut i = 0;
        while i < pieces.len() {
            match pieces[i].style.link.clone().filter(|l| !l.is_empty()) {
                Some(url) => {
                    let rid = self.link_rel(&url);
                    let _ = write!(out, "<w:hyperlink r:id=\"{rid}\" w:history=\"1\">");
                    while i < pieces.len() && pieces[i].style.link.as_deref() == Some(url.as_str()) {
                        self.piece(out, &pieces, i, bold);
                        i += 1;
                    }
                    out.push_str("</w:hyperlink>");
                }
                None => {
                    self.piece(out, &pieces, i, bold);
                    i += 1;
                }
            }
        }
    }

    fn jc(a: Align) -> &'static str {
        match a {
            Align::Left => "",
            Align::Center => "<w:jc w:val=\"center\"/>",
            Align::Right => "<w:jc w:val=\"right\"/>",
            Align::Justify => "<w:jc w:val=\"both\"/>",
        }
    }

    fn picture_xml(&mut self, rid: &str, cx: i64, cy: i64, name: &str, alt: &str) -> String {
        self.drawings += 1;
        let id = self.drawings;
        format!(
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\" descr=\"{alt}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"{NS_PIC}\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"{name}\" descr=\"{alt}\"/><pic:cNvPicPr><a:picLocks noChangeAspect=\"1\" noChangeArrowheads=\"1\"/></pic:cNvPicPr></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr bwMode=\"auto\"><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
            alt = esc(alt),
            name = esc(name),
        )
    }

    fn caption_xml(out: &mut String, caption: &str, align: Align) {
        if !caption.is_empty() {
            let _ = write!(out, "<w:p><w:pPr><w:pStyle w:val=\"Caption\"/>{}</w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", Self::jc(align), esc(caption));
        }
    }

    fn body(&mut self, sections: &[Section]) -> String {
        let mut out = String::new();
        let n = sections.len();
        for (si, s) in sections.iter().enumerate() {
            let text_w = s.setup.text_width();
            // Lists: one numbering per list kind for each run of list paragraphs.
            let mut region: HashMap<u8, usize> = HashMap::new();
            let mut prev_table = false;
            for b in &s.blocks {
                if !matches!(b, Out::Para { list: Some(_), .. }) {
                    region.clear();
                }
                if matches!(b, Out::Table { .. }) && prev_table {
                    // Two tables in a row would join into one.
                    out.push_str("<w:p><w:pPr><w:spacing w:after=\"0\"/></w:pPr></w:p>");
                }
                prev_table = matches!(b, Out::Table { .. });
                match b {
                    Out::Para { style, align, list, level, pieces } => {
                        let mut ppr = String::new();
                        if *style != ParaStyle::Normal {
                            let _ = write!(ppr, "<w:pStyle w:val=\"{}\"/>", style_id(*style));
                        }
                        if let Some(k) = list {
                            let key = match k {
                                ListKind::Bullet => 1u8,
                                ListKind::Number => 2,
                                ListKind::Check => 3,
                            };
                            let num = match region.get(&key) {
                                Some(n) => *n,
                                None => {
                                    self.nums.push((self.nums.len() + 1, key as usize));
                                    let n = self.nums.len();
                                    region.insert(key, n);
                                    n
                                }
                            };
                            let _ = write!(ppr, "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num}\"/></w:numPr>");
                        }
                        ppr.push_str(Self::jc(*align));
                        out.push_str("<w:p>");
                        if !ppr.is_empty() {
                            let _ = write!(out, "<w:pPr>{ppr}</w:pPr>");
                        }
                        self.runs(&mut out, pieces, false, *style == ParaStyle::Code);
                        out.push_str("</w:p>");
                    }
                    Out::PageBreak => out.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>"),
                    Out::Image { media, width, alt, caption, align } => {
                        let Some(rid) = self.media_rel(media) else { continue };
                        let m = &self.doc.media[media];
                        let w = if *width > 0.0 { width.min(text_w) } else if m.width > 0 { (m.width as f32 * 0.75).min(text_w) } else { text_w };
                        let h = if m.width > 0 && m.height > 0 { w * m.height as f32 / m.width as f32 } else { w * 0.75 };
                        let name = m.name.clone();
                        let pic = self.picture_xml(&rid, emu(w), emu(h), &name, alt);
                        let _ = write!(out, "<w:p><w:pPr><w:keepNext/>{}</w:pPr>{pic}</w:p>", Self::jc(*align));
                        Self::caption_xml(&mut out, caption, *align);
                    }
                    Out::Picture { png, width, height, caption } => {
                        let rid = self.add_picture(png.clone(), "png");
                        let pic = self.picture_xml(&rid, emu(*width), emu(*height), "Chart", caption);
                        let _ = write!(out, "<w:p><w:pPr><w:keepNext/></w:pPr>{pic}</w:p>");
                        Self::caption_xml(&mut out, caption, Align::Left);
                    }
                    Out::Table { rows, header, widths, banded } => self.table_xml(&mut out, rows, *header, widths, *banded, text_w),
                }
            }
            if matches!(s.blocks.last(), Some(Out::Table { .. })) || s.blocks.is_empty() {
                out.push_str("<w:p/>");
            }
            let sect = self.sect_pr(&s.setup, si + 1 == n);
            if si + 1 < n {
                let _ = write!(out, "<w:p><w:pPr>{sect}</w:pPr></w:p>");
            } else {
                out.push_str(&sect);
            }
        }
        out
    }

    fn table_xml(&mut self, out: &mut String, rows: &[Vec<CellOut>], header: bool, widths: &[f32], banded: bool, text_w: f32) {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let widths: Vec<f32> = if widths.len() == cols { widths.to_vec() } else { vec![1.0 / cols as f32; cols] };
        let total = tw(text_w);
        let grid: Vec<i64> = widths.iter().map(|f| ((*f as f64) * total as f64).round().max(1.0) as i64).collect();
        let border = |side: &str| format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/>");
        let look = if header { "<w:tblLook w:val=\"0420\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"0\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/>" } else { "<w:tblLook w:val=\"0400\" w:firstRow=\"0\" w:lastRow=\"0\" w:firstColumn=\"0\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/>" };
        let _ = write!(
            out,
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"{total}\" w:type=\"dxa\"/><w:tblBorders>{}{}{}{}{}{}</w:tblBorders><w:tblLayout w:type=\"fixed\"/>{look}</w:tblPr><w:tblGrid>",
            border("top"),
            border("left"),
            border("bottom"),
            border("right"),
            border("insideH"),
            border("insideV")
        );
        for g in &grid {
            let _ = write!(out, "<w:gridCol w:w=\"{g}\"/>");
        }
        out.push_str("</w:tblGrid>");
        for (ri, row) in rows.iter().enumerate() {
            out.push_str("<w:tr>");
            if ri == 0 && header {
                out.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for ci in 0..cols {
                let empty = CellOut { lines: vec![vec![]], fill: None, align: Align::Left };
                let cell = row.get(ci).unwrap_or(&empty);
                let band = banded && !(header && ri == 0) && (ri + usize::from(!header)) % 2 == 0;
                let fill = cell.fill.as_deref().and_then(hex6).or(if band { Some("F2F2F2".into()) } else { None });
                let _ = write!(out, "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>", grid[ci]);
                if let Some(f) = fill {
                    let _ = write!(out, "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{f}\"/>");
                }
                out.push_str("</w:tcPr>");
                let lines = if cell.lines.is_empty() { vec![vec![]] } else { cell.lines.clone() };
                for line in &lines {
                    let _ = write!(out, "<w:p><w:pPr><w:spacing w:before=\"40\" w:after=\"40\"/>{}</w:pPr>", Self::jc(cell.align));
                    self.runs(out, line, header && ri == 0, false);
                    out.push_str("</w:p>");
                }
                out.push_str("</w:tc>");
            }
            out.push_str("</w:tr>");
        }
        out.push_str("</w:tbl>");
    }

    fn hf_part(&mut self, header: bool, text: &str) -> String {
        if let Some(r) = self.hf_cache.get(&(header, text.to_string())) {
            return r.clone();
        }
        let n = self.hf_parts.iter().filter(|p| p.2 == header).count() + 1;
        let kind = if header { "header" } else { "footer" };
        let part = format!("{kind}{n}.xml");
        let mut runs = String::new();
        let mut rest = text;
        let title = self.doc.title.clone();
        let field = |instr: &str, shown: &str| format!("<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> {instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>{shown}</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>");
        while !rest.is_empty() {
            let next = ["{page}", "{pages}", "{title}"].iter().filter_map(|k| rest.find(k).map(|i| (i, *k))).min();
            match next {
                Some((i, k)) => {
                    if i > 0 {
                        let _ = write!(runs, "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(&rest[..i]));
                    }
                    match k {
                        "{page}" => runs.push_str(&field("PAGE", "1")),
                        "{pages}" => runs.push_str(&field("NUMPAGES", "1")),
                        _ => {
                            let _ = write!(runs, "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(&title));
                        }
                    }
                    rest = &rest[i + k.len()..];
                }
                None => {
                    let _ = write!(runs, "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(rest));
                    rest = "";
                }
            }
        }
        let tag = if header { "w:hdr" } else { "w:ftr" };
        let style = if header { "Header" } else { "Footer" };
        let xml = format!("{XML_HEAD}<{tag} xmlns:w=\"{NS_W}\" xmlns:r=\"{NS_R}\"><w:p><w:pPr><w:pStyle w:val=\"{style}\"/><w:jc w:val=\"center\"/></w:pPr>{runs}</w:p></{tag}>");
        self.files.push((format!("word/{part}"), xml.into_bytes()));
        self.hf_parts.push((part.clone(), kind.to_string(), header));
        let rid = self.rel(kind, &part, false);
        self.hf_cache.insert((header, text.to_string()), rid.clone());
        rid
    }

    fn sect_pr(&mut self, s: &PageSetup, _last: bool) -> String {
        let mut x = String::from("<w:sectPr>");
        if !s.header.trim().is_empty() {
            let rid = self.hf_part(true, s.header.trim());
            let _ = write!(x, "<w:headerReference w:type=\"default\" r:id=\"{rid}\"/>");
        }
        if !s.footer.trim().is_empty() {
            let rid = self.hf_part(false, s.footer.trim());
            let _ = write!(x, "<w:footerReference w:type=\"default\" r:id=\"{rid}\"/>");
        }
        let orient = if s.width > s.height { " w:orient=\"landscape\"" } else { "" };
        let hdist = tw((s.margin_top / 2.0).min(36.0));
        let fdist = tw((s.margin_bottom / 2.0).min(36.0));
        let _ = write!(
            x,
            "<w:type w:val=\"nextPage\"/><w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{hdist}\" w:footer=\"{fdist}\" w:gutter=\"0\"/><w:cols w:space=\"720\"/>",
            tw(s.width),
            tw(s.height),
            tw(s.margin_top),
            tw(s.margin_right),
            tw(s.margin_bottom),
            tw(s.margin_left)
        );
        if s.different_first {
            x.push_str("<w:titlePg/>");
        }
        x.push_str("<w:docGrid w:linePitch=\"360\"/></w:sectPr>");
        x
    }
}

pub(crate) fn style_id(s: ParaStyle) -> &'static str {
    match s {
        ParaStyle::Normal => "Normal",
        ParaStyle::Title => "Title",
        ParaStyle::Subtitle => "Subtitle",
        ParaStyle::Heading1 => "Heading1",
        ParaStyle::Heading2 => "Heading2",
        ParaStyle::Heading3 => "Heading3",
        ParaStyle::Quote => "Quote",
        ParaStyle::Code => "Code",
        ParaStyle::Caption => "Caption",
    }
}

fn styles_xml() -> String {
    let mut x = format!("{XML_HEAD}<w:styles xmlns:w=\"{NS_W}\">");
    let sans = "IBM Plex Sans";
    let _ = write!(
        x,
        "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{sans}\" w:hAnsi=\"{sans}\" w:eastAsia=\"{sans}\" w:cs=\"{sans}\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"en-US\" w:eastAsia=\"en-US\" w:bidi=\"ar-SA\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"336\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>"
    );
    for s in ParaStyle::ALL {
        let spec = s.spec();
        let (name, outline, ui) = match s {
            ParaStyle::Normal => ("Normal", None, 0),
            ParaStyle::Title => ("Title", None, 10),
            ParaStyle::Subtitle => ("Subtitle", None, 11),
            ParaStyle::Heading1 => ("heading 1", Some(0), 9),
            ParaStyle::Heading2 => ("heading 2", Some(1), 9),
            ParaStyle::Heading3 => ("heading 3", Some(2), 9),
            ParaStyle::Quote => ("Quote", None, 29),
            ParaStyle::Code => ("Code", None, 30),
            ParaStyle::Caption => ("caption", None, 35),
        };
        let id = style_id(s);
        let default = if s == ParaStyle::Normal { " w:default=\"1\"" } else { "" };
        let _ = write!(x, "<w:style w:type=\"paragraph\"{default} w:styleId=\"{id}\"><w:name w:val=\"{name}\"/>");
        if s != ParaStyle::Normal {
            x.push_str("<w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>");
        }
        let _ = write!(x, "<w:uiPriority w:val=\"{ui}\"/><w:qFormat/><w:pPr>");
        if s.is_heading() {
            x.push_str("<w:keepNext/><w:keepLines/>");
        }
        if s == ParaStyle::Quote {
            x.push_str("<w:pBdr><w:left w:val=\"single\" w:sz=\"12\" w:space=\"8\" w:color=\"BFBFBF\"/></w:pBdr>");
        }
        if s == ParaStyle::Code {
            x.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F4F4F4\"/>");
        }
        let _ = write!(x, "<w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{}\" w:lineRule=\"auto\"/>", tw(spec.space_before), tw(spec.space_after), (spec.line * 240.0).round() as i64);
        if s == ParaStyle::Quote {
            x.push_str("<w:ind w:left=\"284\" w:right=\"284\"/>");
        }
        if let Some(o) = outline {
            let _ = write!(x, "<w:outlineLvl w:val=\"{o}\"/>");
        }
        x.push_str("</w:pPr><w:rPr>");
        if spec.family != folio_core::text::Family::Sans {
            let f = family_face(spec.family);
            let _ = write!(x, "<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:eastAsia=\"{f}\" w:cs=\"{f}\"/>");
        }
        if spec.bold {
            x.push_str("<w:b/><w:bCs/>");
        }
        if spec.italic {
            x.push_str("<w:i/><w:iCs/>");
        }
        if spec.muted {
            x.push_str("<w:color w:val=\"595959\"/>");
        }
        let hp = (spec.size * 2.0).round() as i64;
        let _ = write!(x, "<w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/></w:rPr></w:style>");
    }
    // Character styles, notes, comments, header and footer, tables.
    x.push_str("<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/><w:uiPriority w:val=\"1\"/><w:semiHidden/><w:unhideWhenUsed/></w:style>");
    x.push_str("<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:uiPriority w:val=\"99\"/><w:unhideWhenUsed/><w:rPr><w:color w:val=\"1D4ED8\"/><w:u w:val=\"single\"/></w:rPr></w:style>");
    x.push_str("<w:style w:type=\"character\" w:styleId=\"FootnoteReference\"><w:name w:val=\"footnote reference\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr></w:style>");
    x.push_str("<w:style w:type=\"paragraph\" w:styleId=\"FootnoteText\"><w:name w:val=\"footnote text\"/><w:basedOn w:val=\"Normal\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr></w:style>");
    x.push_str("<w:style w:type=\"character\" w:styleId=\"CommentReference\"><w:name w:val=\"annotation reference\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:sz w:val=\"16\"/><w:szCs w:val=\"16\"/></w:rPr></w:style>");
    x.push_str("<w:style w:type=\"paragraph\" w:styleId=\"CommentText\"><w:name w:val=\"annotation text\"/><w:basedOn w:val=\"Normal\"/><w:uiPriority w:val=\"99\"/><w:unhideWhenUsed/><w:pPr><w:spacing w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style>");
    for (id, name) in [("Header", "header"), ("Footer", "footer")] {
        let _ = write!(x, "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{name}\"/><w:basedOn w:val=\"Normal\"/><w:uiPriority w:val=\"99\"/><w:unhideWhenUsed/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:color w:val=\"595959\"/><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr></w:style>");
    }
    x.push_str("<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>");
    x.push_str("<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:basedOn w:val=\"TableNormal\"/><w:uiPriority w:val=\"39\"/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/></w:tblBorders></w:tblPr></w:style>");
    x.push_str("</w:styles>");
    x
}

fn numbering_xml(nums: &[(usize, usize)]) -> String {
    let mut x = format!("{XML_HEAD}<w:numbering xmlns:w=\"{NS_W}\">");
    let bullets = ["•", "◦", "▪"];
    let formats = ["decimal", "lowerLetter", "lowerRoman"];
    for abs in 1..=3 {
        let _ = write!(x, "<w:abstractNum w:abstractNumId=\"{abs}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>");
        for lvl in 0..9 {
            let left = 720 * (lvl + 1);
            let (fmt, text) = match abs {
                1 => ("bullet".to_string(), bullets[lvl % 3].to_string()),
                2 => (formats[lvl % 3].to_string(), format!("%{}.", lvl + 1)),
                _ => ("none".to_string(), String::new()),
            };
            let _ = write!(x, "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{left}\" w:hanging=\"360\"/></w:pPr>", esc(&text));
            if abs == 1 {
                x.push_str("<w:rPr><w:rFonts w:ascii=\"IBM Plex Sans\" w:hAnsi=\"IBM Plex Sans\" w:hint=\"default\"/></w:rPr>");
            }
            x.push_str("</w:lvl>");
        }
        x.push_str("</w:abstractNum>");
    }
    for (num, abs) in nums {
        let _ = write!(x, "<w:num w:numId=\"{num}\"><w:abstractNumId w:val=\"{abs}\"/>");
        if *abs == 2 {
            // Each numbered list starts again at 1.
            for l in 0..9 {
                let _ = write!(x, "<w:lvlOverride w:ilvl=\"{l}\"><w:startOverride w:val=\"1\"/></w:lvlOverride>");
            }
        }
        x.push_str("</w:num>");
    }
    x.push_str("</w:numbering>");
    x
}

fn font_table_xml() -> String {
    let mut x = format!("{XML_HEAD}<w:fonts xmlns:w=\"{NS_W}\">");
    for (name, alt, family, pitch) in [
        ("IBM Plex Sans", "Arial", "swiss", "variable"),
        ("IBM Plex Serif", "Georgia", "roman", "variable"),
        ("IBM Plex Mono", "Courier New", "modern", "fixed"),
        ("Chakra Petch", "Arial", "swiss", "variable"),
    ] {
        let _ = write!(x, "<w:font w:name=\"{name}\"><w:altName w:val=\"{alt}\"/><w:charset w:val=\"00\"/><w:family w:val=\"{family}\"/><w:pitch w:val=\"{pitch}\"/></w:font>");
    }
    x.push_str("</w:fonts>");
    x
}

fn settings_xml(track: bool, notes: bool) -> String {
    let mut x = format!("{XML_HEAD}<w:settings xmlns:w=\"{NS_W}\"><w:zoom w:percent=\"100\"/>");
    if track {
        x.push_str("<w:trackRevisions/>");
    }
    x.push_str("<w:defaultTabStop w:val=\"720\"/><w:characterSpacingControl w:val=\"doNotCompress\"/>");
    if notes {
        x.push_str("<w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr>");
    }
    x.push_str("<w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>");
    x
}

fn footnotes_xml(notes: &[String]) -> String {
    let mut x = format!("{XML_HEAD}<w:footnotes xmlns:w=\"{NS_W}\">");
    x.push_str("<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote>");
    x.push_str("<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>");
    for (i, n) in notes.iter().enumerate() {
        let _ = write!(x, "<w:footnote w:id=\"{}\">", i + 1);
        for (j, line) in n.split('\n').enumerate() {
            x.push_str("<w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr>");
            if j == 0 {
                x.push_str("<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> </w:t></w:r>");
            }
            if !line.is_empty() {
                let _ = write!(x, "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(line));
            }
            x.push_str("</w:p>");
        }
        x.push_str("</w:footnote>");
    }
    x.push_str("</w:footnotes>");
    x
}

fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).flat_map(char::to_uppercase).take(3).collect()
}

fn comments_xml(comments: &[CommentOut]) -> (String, String) {
    let mut x = format!("{XML_HEAD}<w:comments xmlns:w=\"{NS_W}\" xmlns:w14=\"{NS_W14}\" xmlns:mc=\"{NS_MC}\" mc:Ignorable=\"w14\">");
    let mut ex = format!("{XML_HEAD}<w15:commentsEx xmlns:w15=\"{NS_W15}\" xmlns:mc=\"{NS_MC}\" mc:Ignorable=\"w15\">");
    for c in comments {
        let _ = write!(x, "<w:comment w:id=\"{}\" w:author=\"{}\" w:date=\"{}\" w:initials=\"{}\">", c.wid, esc(&c.author), c.date, esc(&initials(&c.author)));
        let lines: Vec<&str> = c.text.split('\n').collect();
        let n = lines.len();
        for (j, line) in lines.into_iter().enumerate() {
            let pid = if j + 1 == n { format!(" w14:paraId=\"{}\" w14:textId=\"77777777\"", c.para_id) } else { String::new() };
            let _ = write!(x, "<w:p{pid}><w:pPr><w:pStyle w:val=\"CommentText\"/></w:pPr>");
            if j == 0 {
                x.push_str("<w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:annotationRef/></w:r>");
            }
            if !line.is_empty() {
                let _ = write!(x, "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(line));
            }
            x.push_str("</w:p>");
        }
        x.push_str("</w:comment>");
        let parent = c.parent.as_ref().map(|p| format!(" w15:paraIdParent=\"{p}\"")).unwrap_or_default();
        let _ = write!(ex, "<w15:commentEx w15:paraId=\"{}\"{parent} w15:done=\"{}\"/>", c.para_id, u8::from(c.done));
    }
    x.push_str("</w:comments>");
    ex.push_str("</w15:commentsEx>");
    (x, ex)
}

/// A sheet's used range as a document table of what its cells show.
pub(crate) fn sheet_table(sheet: &folio_core::Sheet, max_rows: u32, max_cols: u32) -> Option<(Table, bool)> {
    let used = sheet.used_range()?;
    let rows = (used.end.row - used.start.row + 1).min(max_rows);
    let cols = (used.end.col - used.start.col + 1).min(max_cols);
    let cut = rows < used.end.row - used.start.row + 1 || cols < used.end.col - used.start.col + 1;
    let mut t = Table::new(rows as usize, cols as usize);
    for r in 0..rows {
        for c in 0..cols {
            let a = folio_calc::Addr::new(used.start.row + r, used.start.col + c);
            let Some(cell) = sheet.cell(a) else { continue };
            let f = &cell.format;
            let text = cell.display();
            let style = RunStyle { bold: f.bold, italic: f.italic, underline: f.underline, strike: f.strike, color: f.color.clone(), size: f.size.filter(|s| (*s - 10.0).abs() > 0.01), ..Default::default() };
            let tc = &mut t.rows[r as usize][c as usize];
            if !text.is_empty() {
                tc.runs = vec![Run { text, style }];
            }
            tc.fill = f.fill.clone();
            tc.align = f.align.unwrap_or(if matches!(cell.value, folio_calc::Value::Number(_)) { Align::Right } else { Align::Left });
        }
    }
    let widths: Vec<f32> = (0..cols).map(|c| sheet.col_width(used.start.col + c)).collect();
    t.widths = widths;
    t.header = sheet.freeze_rows > 0 || (rows > 1 && t.rows[0].iter().all(|c| c.runs.iter().all(|r| r.style.bold)) && t.rows[0].iter().any(|c| !c.runs.is_empty()));
    if t.header {
        for c in &mut t.rows[0] {
            for r in &mut c.runs {
                r.style.bold = false;
            }
        }
    }
    Some((t, cut))
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let now = chrono::Utc::now();
    let mut w = Writer {
        doc,
        warnings: vec![],
        warned: HashSet::new(),
        rels: vec![],
        links: HashMap::new(),
        media: HashMap::new(),
        files: vec![],
        exts: BTreeSet::new(),
        pictures: 0,
        drawings: 0,
        footnotes: vec![],
        comments: vec![],
        comment_wids: HashMap::new(),
        comment_span: HashMap::new(),
        started: HashSet::new(),
        counter: 0,
        revision: 0,
        para_ids: 0,
        nums: vec![],
        hf_parts: vec![],
        hf_cache: HashMap::new(),
        now: now.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    // Fixed relationships first.
    w.rel("styles", "styles.xml", false);
    w.rel("settings", "settings.xml", false);
    w.rel("fontTable", "fontTable.xml", false);
    w.rel("numbering", "numbering.xml", false);

    let mut sections = vec![];
    let mut track = false;
    for &i in pages {
        let Some(p) = doc.pages.get(i) else { continue };
        match &p.body {
            PageBody::Doc(t) => {
                track |= t.track_changes;
                let blocks = w.flow_out(&t.blocks, &t.setup);
                w.register_comments(&t.comments);
                sections.push(Section { setup: t.setup.clone(), blocks });
            }
            PageBody::Sheet(s) => {
                let mut setup = PageSetup::a4();
                let mut blocks = vec![Out::Para { style: ParaStyle::Heading1, align: Align::Left, list: None, level: 0, pieces: vec![Piece { idx: usize::MAX, last: false, text: p.name.clone(), style: RunStyle::default() }] }];
                match sheet_table(s, 2000, 40) {
                    Some((t, cut)) => {
                        if t.cols() > 7 {
                            std::mem::swap(&mut setup.width, &mut setup.height);
                        }
                        blocks.push(w.table_out(&t));
                        w.warnings.push(format!("\"{}\" is a sheet: written as a table of its values (formulas{} are left out).", p.name, if s.charts.is_empty() { "" } else { " and charts" }));
                        if cut {
                            w.warnings.push(format!("\"{}\" is too big for a document: only its first 2000 rows and 40 columns are written.", p.name));
                        }
                    }
                    None => w.warnings.push(format!("\"{}\" is an empty sheet.", p.name)),
                }
                sections.push(Section { setup, blocks });
            }
            PageBody::Deck(_) => w.warnings.push(format!("\"{}\" is a deck and Word documents only carry documents: left out (export it as PPTX or PDF).", p.name)),
        }
    }
    if sections.is_empty() {
        return Err("Nothing to write: Word documents carry documents and sheets, and none were chosen.".into());
    }
    let body = w.body(&sections);

    let notes = !w.footnotes.is_empty();
    let has_comments = !w.comments.is_empty();
    if notes {
        w.rel("footnotes", "footnotes.xml", false);
    }
    if has_comments {
        w.rel("comments", "comments.xml", false);
        let id = format!("rId{}", w.rels.len() + 1);
        w.rels.push((id, "http://schemas.microsoft.com/office/2011/relationships/commentsExtended".into(), "commentsExtended.xml".into(), false));
    }

    let document = format!(
        "{XML_HEAD}<w:document xmlns:w=\"{NS_W}\" xmlns:r=\"{NS_R}\" xmlns:wp=\"{NS_WP}\" xmlns:a=\"{NS_A}\" xmlns:pic=\"{NS_PIC}\"><w:body>{body}</w:body></w:document>"
    );

    // Package.
    let mut z = ZipOut::new();
    let mut ct = String::from(XML_HEAD);
    ct.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    for ext in &w.exts {
        let _ = write!(ct, "<Default Extension=\"{ext}\" ContentType=\"image/{ext}\"/>");
    }
    let _ = write!(ct, "<Override PartName=\"/word/document.xml\" ContentType=\"{CT}.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"{CT}.styles+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"{CT}.settings+xml\"/><Override PartName=\"/word/fontTable.xml\" ContentType=\"{CT}.fontTable+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"{CT}.numbering+xml\"/>");
    if notes {
        let _ = write!(ct, "<Override PartName=\"/word/footnotes.xml\" ContentType=\"{CT}.footnotes+xml\"/>");
    }
    if has_comments {
        let _ = write!(ct, "<Override PartName=\"/word/comments.xml\" ContentType=\"{CT}.comments+xml\"/><Override PartName=\"/word/commentsExtended.xml\" ContentType=\"{CT}.commentsExtended+xml\"/>");
    }
    for (part, kind, _) in &w.hf_parts {
        let _ = write!(ct, "<Override PartName=\"/word/{part}\" ContentType=\"{CT}.{kind}+xml\"/>");
    }
    ct.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>");
    z.add("[Content_Types].xml", ct.as_bytes())?;
    z.add(
        "_rels/.rels",
        format!("{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"{REL}/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>").as_bytes(),
    )?;
    let created = doc.meta.created.unwrap_or(now).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let modified = doc.meta.modified.unwrap_or(now).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let author = esc(&doc.meta.author);
    z.add(
        "docProps/core.xml",
        format!(
            "{XML_HEAD}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>{author}</dc:creator><cp:lastModifiedBy>{author}</cp:lastModifiedBy><dcterms:created xsi:type=\"dcterms:W3CDTF\">{created}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{modified}</dcterms:modified></cp:coreProperties>",
            esc(&doc.title)
        )
        .as_bytes(),
    )?;
    let words: usize = pages.iter().filter_map(|&i| doc.pages.get(i)).filter_map(|p| p.doc()).map(|t| t.word_count()).sum();
    z.add(
        "docProps/app.xml",
        format!("{XML_HEAD}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>folio</Application><DocSecurity>0</DocSecurity><Words>{words}</Words></Properties>").as_bytes(),
    )?;
    z.add("word/document.xml", document.as_bytes())?;
    z.add("word/styles.xml", styles_xml().as_bytes())?;
    z.add("word/settings.xml", settings_xml(track, notes).as_bytes())?;
    z.add("word/fontTable.xml", font_table_xml().as_bytes())?;
    z.add("word/numbering.xml", numbering_xml(&w.nums).as_bytes())?;
    if notes {
        z.add("word/footnotes.xml", footnotes_xml(&w.footnotes).as_bytes())?;
    }
    if has_comments {
        let (c, ex) = comments_xml(&w.comments);
        z.add("word/comments.xml", c.as_bytes())?;
        z.add("word/commentsExtended.xml", ex.as_bytes())?;
    }
    for (name, bytes) in &w.files {
        if name.contains("/media/") {
            z.add_stored(name, bytes)?;
        } else {
            z.add(name, bytes)?;
        }
    }
    let mut rels = format!("{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for (id, kind, target, external) in &w.rels {
        let mode = if *external { " TargetMode=\"External\"" } else { "" };
        let _ = write!(rels, "<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{}\"{mode}/>", esc(target));
    }
    rels.push_str("</Relationships>");
    z.add("word/_rels/document.xml.rels", rels.as_bytes())?;
    Ok((z.finish()?, w.warnings))
}
