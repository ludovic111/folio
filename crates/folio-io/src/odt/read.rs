//! ODT in: OpenDocument text to folio blocks.

use std::collections::{HashMap, HashSet};

use folio_core::text::{Comment, ImageBlock, PageSetup, Reply, Table, TableCell};
use folio_core::{Align, Block, Document, Id, ListKind, PageKind, ParaStyle, Paragraph, Run, RunStyle};

use crate::Imported;
use crate::docx::xml::{El, Node, Package, family_of_font, hex_color};
use crate::docx::{classify, heading_for_level, is_check_glyph, is_code_name, page_name, parse_date};

/// A length (`2.54cm`, `1in`, `12pt`) in points.
pub(crate) fn length(v: &str) -> Option<f32> {
    let v = v.trim();
    let num: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+').collect();
    let n: f32 = num.parse().ok()?;
    Some(match v[num.len()..].trim() {
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "in" | "inch" => n * 72.0,
        "pc" => n * 12.0,
        "px" => n * 0.75,
        _ => n,
    })
}

/// ODF encodes characters in style names: `Heading_20_1` is "Heading 1".
fn decode_name(n: &str) -> String {
    let mut out = String::new();
    let mut rest = n;
    while let Some(i) = rest.find('_') {
        let after = &rest[i + 1..];
        if let Some(j) = after.find('_')
            && j > 0
            && j <= 4
            && after[..j].chars().all(|c| c.is_ascii_hexdigit())
            && let Some(c) = u32::from_str_radix(&after[..j], 16).ok().and_then(char::from_u32)
        {
            out.push_str(&rest[..i]);
            out.push(c);
            rest = &after[j + 1..];
            continue;
        }
        out.push_str(&rest[..=i]);
        rest = after;
    }
    out.push_str(rest);
    out
}

#[derive(Clone, Debug, Default)]
struct TProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<String>,
    highlight: Option<String>,
    /// Points, or a percentage of the parent's size (negative).
    size: Option<f32>,
    font: Option<&'static str>,
    vert: Option<i8>,
    hidden: Option<bool>,
}

impl TProps {
    fn over(&self, o: &TProps) -> TProps {
        TProps {
            bold: o.bold.or(self.bold),
            italic: o.italic.or(self.italic),
            underline: o.underline.or(self.underline),
            strike: o.strike.or(self.strike),
            color: o.color.clone().or(self.color.clone()),
            highlight: o.highlight.clone().or(self.highlight.clone()),
            size: match (o.size, self.size) {
                (Some(p), Some(base)) if p < 0.0 && base > 0.0 => Some(base * -p),
                (Some(p), _) => Some(p),
                (None, s) => s,
            },
            font: o.font.or(self.font),
            vert: o.vert.or(self.vert),
            hidden: o.hidden.or(self.hidden),
        }
    }
}

#[derive(Clone, Default)]
struct StyleDef {
    parent: Option<String>,
    display: String,
    outline: Option<u8>,
    align: Option<Align>,
    break_before: bool,
    break_after: bool,
    master: Option<String>,
    list_style: Option<String>,
    text: TProps,
    /// Table cells and columns.
    fill: Option<String>,
    width: Option<f32>,
}

struct Conv {
    pkg: Package,
    styles: HashMap<String, StyleDef>,
    fonts: HashMap<String, &'static str>,
    /// list style → level (1-based) → (kind, check glyph "done", no label).
    lists: HashMap<String, HashMap<u8, (ListKind, Option<bool>, bool)>>,
    doc: Document,
    warnings: Vec<String>,
    warned: HashSet<&'static str>,
    comments: Vec<Comment>,
    open_comments: Vec<(String, Id)>,
    commented: HashSet<Id>,
    changes: HashMap<String, Change>,
    open_insert: Option<String>,
    media: HashMap<String, Option<(Id, u32)>>,
    first_block: bool,
    text_width: f32,
    defaults: TProps,
}

#[derive(Clone)]
struct Change {
    insertion: bool,
    author: String,
    deleted: Vec<String>,
}

/// One paragraph being built.
struct Builder {
    out: Vec<Block>,
    cur: Paragraph,
    made_other: bool,
    deferred: Vec<Block>,
    base: TProps,
    reference: TProps,
    style: ParaStyle,
}

impl Builder {
    fn continuation(&self) -> Paragraph {
        Paragraph { style: self.cur.style, align: self.cur.align, ..Default::default() }
    }

    fn push_block(&mut self, b: Block) {
        let next = self.continuation();
        let p = std::mem::replace(&mut self.cur, next);
        self.out.push(Block::Paragraph(p));
        self.out.push(b);
        self.made_other = true;
    }

    fn line_break(&mut self) {
        let next = self.continuation();
        let p = std::mem::replace(&mut self.cur, next);
        self.out.push(Block::Paragraph(p));
    }

    fn finish(mut self) -> Vec<Block> {
        let p = std::mem::take(&mut self.cur);
        self.out.push(Block::Paragraph(p));
        let made_other = self.made_other;
        let mut out: Vec<Block> = self
            .out
            .into_iter()
            .map(|mut b| {
                if let Block::Paragraph(p) = &mut b {
                    p.normalize();
                }
                b
            })
            .filter(|b| !made_other || !matches!(b, Block::Paragraph(p) if p.is_empty()))
            .collect();
        out.extend(self.deferred);
        out
    }
}

#[derive(Clone, Default)]
struct Ctx {
    link: Option<String>,
    props: TProps,
}

impl Conv {
    fn warn(&mut self, key: &'static str, msg: impl Into<String>) {
        if self.warned.insert(key) {
            self.warnings.push(msg.into());
        }
    }

    fn read_styles(&mut self, container: &El) {
        for s in container.elements() {
            match s.name.as_str() {
                "style:style" => {
                    let Some(name) = s.attr("style:name") else { continue };
                    let mut d = StyleDef {
                        parent: s.attr("style:parent-style-name").map(str::to_string),
                        display: s.attr("style:display-name").map(str::to_string).unwrap_or_else(|| decode_name(name)),
                        outline: s.attr("style:default-outline-level").and_then(|v| v.parse().ok()),
                        master: s.attr("style:master-page-name").filter(|m| !m.is_empty()).map(str::to_string),
                        list_style: s.attr("style:list-style-name").filter(|m| !m.is_empty()).map(str::to_string),
                        ..Default::default()
                    };
                    if let Some(pp) = s.child("style:paragraph-properties") {
                        d.align = pp.attr("fo:text-align").map(|a| match a {
                            "center" => Align::Center,
                            "end" | "right" => Align::Right,
                            "justify" => Align::Justify,
                            _ => Align::Left,
                        });
                        d.break_before = pp.attr("fo:break-before") == Some("page");
                        d.break_after = pp.attr("fo:break-after") == Some("page");
                    }
                    if let Some(tp) = s.child("style:text-properties") {
                        d.text = self.text_props(tp);
                    }
                    if let Some(cp) = s.child("style:table-cell-properties") {
                        d.fill = cp.attr("fo:background-color").and_then(hex_color);
                    }
                    if let Some(cp) = s.child("style:table-column-properties") {
                        d.width = cp.attr("style:column-width").and_then(length).or_else(|| cp.attr("style:rel-column-width").and_then(|v| v.trim_end_matches('*').parse().ok()));
                    }
                    if let Some(tp) = s.child("style:table-properties") {
                        d.break_before = tp.attr("fo:break-before") == Some("page");
                    }
                    self.styles.insert(name.to_string(), d);
                }
                "style:default-style" if s.attr("style:family") == Some("paragraph") => {
                    if let Some(tp) = s.child("style:text-properties") {
                        self.defaults = self.text_props(tp);
                    }
                }
                "text:list-style" => {
                    let Some(name) = s.attr("style:name") else { continue };
                    let mut levels = HashMap::new();
                    for l in s.elements() {
                        let lvl: u8 = l.attr("text:level").and_then(|v| v.parse().ok()).unwrap_or(1);
                        let entry = match l.name.as_str() {
                            "text:list-level-style-bullet" => {
                                let ch = l.attr("text:bullet-char").unwrap_or("•");
                                match is_check_glyph(ch) {
                                    Some(done) => (ListKind::Check, Some(done), false),
                                    None => (ListKind::Bullet, None, false),
                                }
                            }
                            "text:list-level-style-number" => {
                                let fmt = l.attr("style:num-format").unwrap_or("1");
                                (ListKind::Number, None, fmt.is_empty())
                            }
                            "text:list-level-style-image" => (ListKind::Bullet, None, false),
                            _ => continue,
                        };
                        levels.insert(lvl, entry);
                    }
                    self.lists.insert(name.to_string(), levels);
                }
                _ => {}
            }
        }
    }

    fn text_props(&self, tp: &El) -> TProps {
        let mut p = TProps::default();
        if let Some(w) = tp.attr("fo:font-weight") {
            p.bold = Some(w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600));
        }
        if let Some(s) = tp.attr("fo:font-style") {
            p.italic = Some(s == "italic" || s == "oblique");
        }
        if let Some(u) = tp.attr("style:text-underline-style") {
            p.underline = Some(u != "none");
        }
        if let Some(u) = tp.attr("style:text-line-through-style") {
            p.strike = Some(u != "none");
        }
        if let Some(c) = tp.attr("fo:color") {
            p.color = Some(hex_color(c).unwrap_or_default());
        }
        if let Some(c) = tp.attr("fo:background-color") {
            p.highlight = Some(hex_color(c).unwrap_or_default());
        }
        if let Some(sz) = tp.attr("fo:font-size") {
            p.size = if let Some(pct) = sz.strip_suffix('%') { pct.trim().parse::<f32>().ok().map(|v| -(v / 100.0)) } else { length(sz) };
        }
        if let Some(f) = tp.attr("style:font-name") {
            p.font = Some(self.fonts.get(f).copied().unwrap_or_else(|| family_of_font(&decode_name(f))));
        } else if let Some(f) = tp.attr("fo:font-family") {
            p.font = Some(family_of_font(f));
        }
        if let Some(pos) = tp.attr("style:text-position") {
            let first = pos.split_whitespace().next().unwrap_or("0");
            p.vert = Some(match first {
                "super" => 1,
                "sub" => -1,
                v => {
                    let n: f32 = v.trim_end_matches('%').parse().unwrap_or(0.0);
                    if n > 0.0 {
                        1
                    } else if n < 0.0 {
                        -1
                    } else {
                        0
                    }
                }
            });
        }
        if let Some(d) = tp.attr("text:display") {
            p.hidden = Some(d == "none");
        }
        p
    }

    fn chain(&self, name: &str) -> Vec<&StyleDef> {
        let mut out = vec![];
        let mut seen = HashSet::new();
        let mut cur = Some(name.to_string());
        while let Some(n) = cur {
            if !seen.insert(n.clone()) {
                break;
            }
            let Some(s) = self.styles.get(&n) else { break };
            out.push(s);
            cur = s.parent.clone();
        }
        out
    }

    fn chain_text(&self, name: &str) -> TProps {
        let mut p = TProps::default();
        for s in self.chain(name).into_iter().rev() {
            p = p.over(&s.text);
        }
        p
    }

    fn para_style_of(&self, name: &str) -> ParaStyle {
        for s in self.chain(name) {
            if let Some(p) = classify(&s.display) {
                return p;
            }
            if let Some(l) = s.outline.filter(|l| *l > 0) {
                return heading_for_level(l - 1);
            }
        }
        ParaStyle::Normal
    }

    fn style_flag(&self, name: &str, f: impl Fn(&StyleDef) -> bool) -> bool {
        self.chain(name).into_iter().any(f)
    }

    fn run_style(&self, b: &Builder, ctx: &Ctx) -> (RunStyle, bool) {
        let fin = b.base.over(&ctx.props);
        let r = &b.reference;
        let spec = b.style.spec();
        let set = |a: Option<bool>, base: Option<bool>| a == Some(true) && base != Some(true);
        let mut s = RunStyle {
            bold: set(fin.bold, r.bold) && !spec.bold,
            italic: set(fin.italic, r.italic) && !spec.italic,
            underline: set(fin.underline, r.underline),
            strike: set(fin.strike, r.strike),
            superscript: fin.vert == Some(1),
            subscript: fin.vert == Some(-1),
            link: ctx.link.clone(),
            ..Default::default()
        };
        if let Some(c) = fin.color.as_ref().filter(|c| !c.is_empty())
            && fin.color != r.color
        {
            s.color = Some(c.clone());
        }
        if let Some(h) = fin.highlight.as_ref().filter(|c| !c.is_empty() && *c != "#ffffff")
            && fin.highlight != r.highlight
        {
            s.highlight = Some(h.clone());
        }
        if let Some(sz) = fin.size.filter(|s| *s > 0.0)
            && r.size.is_none_or(|rs| (rs - sz).abs() > 0.01)
            && (sz - spec.size).abs() > 0.01
        {
            s.size = Some(sz);
        }
        if let Some(f) = fin.font
            && fin.font != r.font
        {
            if f == "mono" {
                s.code = b.style != ParaStyle::Code;
            } else if f != family_id(spec.family) {
                s.font = Some(f.to_string());
            }
        }
        if s.link.is_some() {
            s.underline = false;
            if s.color.as_deref().is_some_and(crate::docx::is_link_blue) {
                s.color = None;
            }
        }
        (s, fin.hidden == Some(true))
    }

    fn emit(&mut self, b: &mut Builder, text: &str, ctx: &Ctx) {
        if text.is_empty() {
            return;
        }
        let (mut style, hidden) = self.run_style(b, ctx);
        if hidden {
            self.warn("hidden", "Hidden text is left out.");
            return;
        }
        style.comment = self.open_comments.last().map(|c| c.1.clone());
        if let Some(c) = &style.comment {
            self.commented.insert(c.clone());
        }
        if let Some(ch) = &self.open_insert
            && let Some(c) = self.changes.get(ch)
        {
            style.inserted = Some(c.author.clone());
        }
        b.cur.runs.push(Run { text: text.to_string(), style });
    }

    // ---- media ------------------------------------------------------------------------------------

    fn image(&mut self, href: &str) -> Option<(Id, u32)> {
        if href.contains("://") {
            self.warn("linked-picture", "Pictures linked to files outside the document are left out.");
            return None;
        }
        let path = href.trim_start_matches("./").to_string();
        if let Some(m) = self.media.get(&path) {
            return m.clone();
        }
        let got = self.pkg.read(&path).and_then(|bytes| {
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let mime = folio_core::Media::sniff(&bytes);
            let bytes = if mime == "application/octet-stream" {
                let img = image::load_from_memory(&bytes).ok()?;
                let mut out = std::io::Cursor::new(Vec::new());
                img.write_to(&mut out, image::ImageFormat::Png).ok()?;
                out.into_inner()
            } else {
                bytes
            };
            let id = self.doc.add_media(&name, bytes);
            let w = self.doc.media[&id].width;
            Some((id, w))
        });
        if got.is_none() {
            self.warn("picture-format", "Pictures in formats folio can't show (like Windows metafiles) are left out.");
        }
        self.media.insert(path, got.clone());
        got
    }

    fn frame(&mut self, f: &El, b: &mut Builder) {
        let width = f.attr("svg:width").and_then(length);
        let alt = f.child("svg:desc").or(f.child("svg:title")).map(|d| d.own_text()).unwrap_or_default();
        if let Some(img) = f.child("draw:image") {
            let href = img.attr("xlink:href").unwrap_or("");
            if href.is_empty() && img.child("office:binary-data").is_some() {
                self.warn("binary-picture", "Pictures stored inline as text are left out.");
                return;
            }
            let Some((media, pw)) = self.image(href) else { return };
            if f.attr("text:anchor-type").is_some_and(|a| a != "as-char") {
                self.warn("floating", "Pictures that floated beside the text are placed in line.");
            }
            let natural = pw as f32 * 0.75;
            let mut width = width.filter(|w| *w > 0.5).unwrap_or(natural);
            if width >= self.text_width - 0.5 {
                width = 0.0;
            }
            let align = b.cur.align;
            b.push_block(Block::Image(ImageBlock { id: Id::new(), media, width, caption: String::new(), alt, align }));
            return;
        }
        if let Some(tb) = f.child("draw:text-box") {
            let blocks = self.blocks(tb, None, 0);
            b.deferred.extend(blocks);
            return;
        }
        if f.child("draw:object").is_some() || f.child("draw:object-ole").is_some() {
            self.warn("object", "Embedded objects (charts, formulas, spreadsheets) are left out.");
        }
    }

    // ---- paragraphs -----------------------------------------------------------------------------

    fn paragraph(&mut self, p: &El, list: Option<(ListKind, u8, Option<bool>, bool)>) -> Vec<Block> {
        let sname = p.attr("text:style-name").unwrap_or("").to_string();
        let mut style = self.para_style_of(&sname);
        if p.is("text:h") {
            let lvl: u8 = p.attr("text:outline-level").and_then(|v| v.parse().ok()).unwrap_or(1);
            if !matches!(style, ParaStyle::Title | ParaStyle::Subtitle) {
                style = heading_for_level(lvl.saturating_sub(1));
            }
        }
        let align = self.chain(&sname).into_iter().find_map(|s| s.align).unwrap_or_default();
        let mut para = Paragraph { style, align, ..Default::default() };
        let mut glyph = None;
        let mut plain = false;
        if let Some((k, lvl, done, no_label)) = list {
            para.level = lvl.min(5);
            if no_label {
                plain = true;
            } else {
                para.list = Some(k);
                glyph = done;
            }
        }
        let base = self.defaults.over(&self.chain_text(&sname));
        let reference = if style == ParaStyle::Normal { self.defaults.over(&self.chain_text("Standard")) } else { base.clone() };
        let mut b = Builder { out: vec![], cur: para, made_other: false, deferred: vec![], base, reference, style };
        let first = self.first_block;
        self.first_block = false;
        if !first && (self.style_flag(&sname, |s| s.break_before) || self.chain(&sname).first().is_some_and(|s| s.master.is_some())) {
            b.out.push(Block::PageBreak { id: Id::new() });
            b.made_other = true;
        }
        self.inline_children(p, &Ctx::default(), &mut b);
        let after = self.style_flag(&sname, |s| s.break_after);
        let mut blocks = b.finish();
        for bl in &mut blocks {
            let Block::Paragraph(p) = bl else { continue };
            if let Some(done) = glyph {
                p.checked = done;
            } else if p.list == Some(ListKind::Bullet) || plain {
                let text = p.text();
                let mut chars = text.chars();
                if let Some(done) = chars.next().and_then(|c| is_check_glyph(&c.to_string())) {
                    p.list = Some(ListKind::Check);
                    p.checked = done;
                    let skip = if chars.next() == Some(' ') { 2 } else { 1 };
                    p.delete(0, skip);
                }
            }
        }
        if after {
            blocks.push(Block::PageBreak { id: Id::new() });
        }
        blocks
    }

    fn inline_children(&mut self, el: &El, ctx: &Ctx, b: &mut Builder) {
        for k in &el.kids {
            match k {
                Node::Text(t) => {
                    // Whitespace collapses like XML says ODF text does.
                    let collapsed = collapse(t);
                    self.emit(b, &collapsed, ctx);
                }
                Node::El(e) => self.inline(e, ctx, b),
            }
        }
    }

    fn inline(&mut self, e: &El, ctx: &Ctx, b: &mut Builder) {
        match e.name.as_str() {
            "text:span" => {
                let mut c = ctx.clone();
                if let Some(s) = e.attr("text:style-name") {
                    c.props = ctx.props.over(&self.chain_text(s));
                    if self.chain(s).iter().any(|d| is_code_name(&d.display)) {
                        c.props.font = Some("mono");
                    }
                }
                self.inline_children(e, &c, b);
            }
            "text:a" => {
                let mut c = ctx.clone();
                c.link = e.attr("xlink:href").filter(|h| !h.starts_with('#')).map(str::to_string);
                self.inline_children(e, &c, b);
            }
            "text:s" => {
                let n: usize = e.attr("text:c").and_then(|v| v.parse().ok()).unwrap_or(1).min(1000);
                self.emit(b, &" ".repeat(n), ctx);
            }
            "text:tab" => self.emit(b, "\t", ctx),
            "text:line-break" => b.line_break(),
            "text:note" => {
                let text = e.child("text:note-body").map(|nb| para_texts(nb).join(" ")).unwrap_or_default();
                if e.attr("text:note-class") == Some("endnote") {
                    self.warn("endnotes", "Endnotes became footnotes.");
                }
                match b.cur.runs.iter_mut().rev().find(|r| !r.text.is_empty()) {
                    Some(r) if r.style.note.is_none() => r.style.note = Some(text),
                    Some(r) => {
                        let n = r.style.note.take().unwrap_or_default();
                        r.style.note = Some(format!("{n} {text}"));
                    }
                    None => {}
                }
            }
            "office:annotation" => {
                let id = Id::new();
                let mut texts = para_texts(e);
                let author = e.child("dc:creator").map(|c| c.own_text()).unwrap_or_else(|| "Unknown".into());
                let at = parse_date(e.child("dc:date").map(|d| d.own_text()).as_deref()).unwrap_or_else(chrono::Utc::now);
                texts.retain(|t| !t.is_empty());
                let resolved = e.attr("loext:resolved") == Some("true");
                let comment = Comment { id: id.clone(), author, text: texts.join("\n"), at, resolved, replies: vec![] };
                // A reply (LibreOffice writes the parent's name).
                if let Some(parent) = e.attr("loext:parent-name").or(e.attr("office:parent-name"))
                    && let Some((_, pid)) = self.open_comments.iter().find(|(n, _)| n == parent).or(None)
                    && let Some(pc) = self.comments.iter_mut().find(|c| c.id == *pid)
                {
                    pc.replies.push(Reply { author: comment.author, text: comment.text, at: comment.at });
                    return;
                }
                self.comments.push(comment);
                match e.attr("office:name") {
                    Some(name) => self.open_comments.push((name.to_string(), id)),
                    None => {
                        if let Some(r) = b.cur.runs.iter_mut().rev().find(|r| !r.text.is_empty() && r.style.comment.is_none()) {
                            r.style.comment = Some(id.clone());
                            self.commented.insert(id);
                        }
                    }
                }
            }
            "office:annotation-end" => {
                if let Some(name) = e.attr("office:name") {
                    self.open_comments.retain(|(n, _)| n != name);
                }
            }
            "text:change-start" => self.open_insert = e.attr("text:change-id").filter(|id| self.changes.get(*id).is_some_and(|c| c.insertion)).map(str::to_string),
            "text:change-end" => self.open_insert = None,
            "text:change" => {
                if let Some(c) = e.attr("text:change-id").and_then(|id| self.changes.get(id)).cloned()
                    && !c.insertion
                {
                    let text = c.deleted.join(" ");
                    if !text.is_empty() {
                        let (mut style, _) = self.run_style(b, ctx);
                        style.deleted = Some(c.author.clone());
                        b.cur.runs.push(Run { text, style });
                    }
                }
            }
            "draw:frame" => self.frame(e, b),
            "draw:a" => self.inline_children(e, ctx, b),
            "draw:g" | "draw:custom-shape" | "draw:rect" | "draw:ellipse" | "draw:line" | "draw:polygon" | "draw:path" | "draw:connector" => {
                self.warn("shape", "Drawn shapes are left out.");
            }
            "text:bookmark" | "text:bookmark-start" | "text:bookmark-end" | "text:reference-mark" | "text:reference-mark-start" | "text:reference-mark-end" | "text:soft-page-break" | "text:alphabetical-index-mark" | "text:toc-mark" | "text:alphabetical-index-mark-start" | "text:alphabetical-index-mark-end" | "text:toc-mark-start" | "text:toc-mark-end" | "office:annotation-text" => {}
            "text:page-number" | "text:page-count" | "text:date" | "text:time" | "text:title" | "text:sequence" | "text:chapter" | "text:author-name" | "text:file-name" | "text:bookmark-ref" | "text:sequence-ref" | "text:note-ref" | "text:reference-ref" | "text:ruby" | "text:ruby-base" | "text:meta" | "text:meta-field" | "text:placeholder" | "text:variable-get" | "text:user-field-get" | "text:expression" | "text:subject" | "text:description" | "text:initial-creator" | "text:creation-date" | "text:word-count" | "text:hidden-text" => {
                if e.is("text:hidden-text") {
                    return;
                }
                self.inline_children(e, ctx, b);
            }
            "text:ruby-text" => {}
            _ => self.inline_children(e, ctx, b),
        }
    }

    // ---- block containers ---------------------------------------------------------------------

    fn list_kind(&self, list_style: Option<&str>, level: u8) -> (ListKind, Option<bool>, bool) {
        list_style
            .and_then(|s| self.lists.get(s))
            .and_then(|l| l.get(&(level + 1)).or_else(|| l.get(&1)))
            .copied()
            .unwrap_or((ListKind::Bullet, None, false))
    }

    fn blocks(&mut self, container: &El, list_style: Option<&str>, depth: u8) -> Vec<Block> {
        let mut out = vec![];
        for e in container.elements() {
            match e.name.as_str() {
                "text:p" | "text:h" => out.extend(self.paragraph(e, None)),
                "text:list" => {
                    let style = e.attr("text:style-name").map(str::to_string).or_else(|| list_style.map(str::to_string));
                    out.extend(self.list(e, style.as_deref(), depth));
                }
                "table:table" => out.push(self.table(e)),
                "text:section" | "text:index-body" | "text:table-of-content" | "text:illustration-index" | "text:alphabetical-index" | "text:bibliography" | "text:user-index" | "text:object-index" | "text:table-index" => {
                    let inner = if let Some(ib) = e.child("text:index-body") { ib } else { e };
                    out.extend(self.blocks(inner, list_style, depth));
                }
                "office:annotation" | "office:annotation-end" => {
                    let mut b = Builder { out: vec![], cur: Paragraph::default(), made_other: false, deferred: vec![], base: TProps::default(), reference: TProps::default(), style: ParaStyle::Normal };
                    self.inline(e, &Ctx::default(), &mut b);
                }
                "draw:frame" => {
                    let mut b = Builder { out: vec![], cur: Paragraph::default(), made_other: true, deferred: vec![], base: TProps::default(), reference: TProps::default(), style: ParaStyle::Normal };
                    self.frame(e, &mut b);
                    out.extend(b.finish());
                }
                _ => {}
            }
        }
        out
    }

    fn list(&mut self, list: &El, style: Option<&str>, depth: u8) -> Vec<Block> {
        let mut out = vec![];
        for item in list.elements() {
            let header = item.is("text:list-header");
            if !item.is("text:list-item") && !header {
                continue;
            }
            for e in item.elements() {
                match e.name.as_str() {
                    "text:p" | "text:h" => {
                        // A paragraph's own list style wins (LibreOffice writes it on the paragraph style too).
                        let pstyle = e.attr("text:style-name").and_then(|s| self.chain(s).into_iter().find_map(|d| d.list_style.clone()));
                        let ls = style.map(str::to_string).or(pstyle);
                        let (k, done, no_label) = self.list_kind(ls.as_deref(), depth);
                        let marker = if header { None } else { Some((k, depth, done, no_label)) };
                        let blocks = self.paragraph(e, marker);
                        out.extend(blocks);
                    }
                    "text:list" => {
                        let s = e.attr("text:style-name").or(style);
                        out.extend(self.list(e, s, depth + 1));
                    }
                    _ => {
                        let blocks = self.blocks(&El { name: "x".into(), attrs: vec![], kids: vec![Node::El(e.clone())] }, style, depth);
                        out.extend(blocks);
                    }
                }
            }
        }
        out
    }

    fn table(&mut self, t: &El) -> Block {
        let mut widths: Vec<f32> = vec![];
        let mut rows: Vec<Vec<TableCell>> = vec![];
        let mut header_rows = 0;
        let break_before = t.attr("table:style-name").is_some_and(|s| self.style_flag(s, |d| d.break_before));
        let _ = break_before;
        self.first_block = false;
        self.table_parts(t, &mut widths, &mut rows, &mut header_rows, false);
        if rows.is_empty() {
            rows.push(vec![TableCell::default()]);
        }
        let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for r in &mut rows {
            r.resize(cols, TableCell::default());
        }
        if header_rows > 0 {
            for c in &mut rows[0] {
                for r in &mut c.runs {
                    r.style.bold = false;
                }
            }
        }
        let widths = if widths.len() == cols && widths.iter().all(|w| *w > 0.0) {
            let sum: f32 = widths.iter().sum();
            widths.iter().map(|w| w / sum).collect()
        } else {
            vec![]
        };
        Block::Table(Table { id: Id::new(), rows, header: header_rows > 0, widths, link: None, banded: false })
    }

    fn table_parts(&mut self, el: &El, widths: &mut Vec<f32>, rows: &mut Vec<Vec<TableCell>>, header_rows: &mut usize, in_header: bool) {
        for e in el.elements() {
            match e.name.as_str() {
                "table:table-column" => {
                    let n: usize = e.attr("table:number-columns-repeated").and_then(|v| v.parse().ok()).unwrap_or(1).min(256);
                    let w = e.attr("table:style-name").and_then(|s| self.styles.get(s)).and_then(|s| s.width).unwrap_or(0.0);
                    for _ in 0..n {
                        widths.push(w);
                    }
                }
                "table:table-columns" | "table:table-column-group" | "table:table-row-group" | "table:table-rows" => self.table_parts(e, widths, rows, header_rows, in_header),
                "table:table-header-columns" => self.table_parts(e, widths, rows, header_rows, in_header),
                "table:table-header-rows" => {
                    if rows.is_empty() {
                        self.table_parts(e, widths, rows, header_rows, true);
                    } else {
                        self.table_parts(e, widths, rows, header_rows, false);
                    }
                }
                "table:table-row" => {
                    let n: usize = e.attr("table:number-rows-repeated").and_then(|v| v.parse().ok()).unwrap_or(1).min(1000);
                    let row = self.row(e);
                    for _ in 0..n {
                        rows.push(row.clone());
                    }
                    if in_header {
                        *header_rows += n;
                    }
                }
                _ => {}
            }
        }
    }

    fn row(&mut self, tr: &El) -> Vec<TableCell> {
        let mut out = vec![];
        for c in tr.elements() {
            let covered = c.is("table:covered-table-cell");
            if !c.is("table:table-cell") && !covered {
                continue;
            }
            let n: usize = c.attr("table:number-columns-repeated").and_then(|v| v.parse().ok()).unwrap_or(1).min(256);
            if covered || c.attr("table:number-columns-spanned").is_some_and(|v| v != "1") || c.attr("table:number-rows-spanned").is_some_and(|v| v != "1") {
                self.warn("merged", "Merged table cells were split back into single cells.");
            }
            let cell = if covered { TableCell::default() } else { self.cell(c) };
            for _ in 0..n {
                out.push(cell.clone());
            }
        }
        out
    }

    fn cell(&mut self, tc: &El) -> TableCell {
        let fill = tc.attr("table:style-name").and_then(|s| self.styles.get(s)).and_then(|s| s.fill.clone()).filter(|f| f != "#ffffff");
        let blocks = self.blocks(tc, None, 0);
        let mut runs: Vec<Run> = vec![];
        let mut align = None;
        let mut first = true;
        for b in blocks {
            let piece = match b {
                Block::Paragraph(p) => {
                    if align.is_none() {
                        align = Some(p.align);
                    }
                    p.runs
                }
                Block::Table(t) => {
                    self.warn("nested-table", "Tables inside table cells became text.");
                    vec![Run::plain(Block::Table(t).plain())]
                }
                Block::Image(_) => {
                    self.warn("cell-picture", "Pictures inside table cells are left out.");
                    continue;
                }
                _ => continue,
            };
            if !first {
                runs.push(Run::plain("\n"));
            }
            first = false;
            runs.extend(piece);
        }
        let mut p = Paragraph { runs, ..Default::default() };
        p.normalize();
        TableCell { runs: p.runs, align: align.unwrap_or_default(), fill }
    }
}

fn family_id(f: folio_core::text::Family) -> &'static str {
    match f {
        folio_core::text::Family::Sans => "sans",
        folio_core::text::Family::Serif => "serif",
        folio_core::text::Family::Mono => "mono",
        folio_core::text::Family::Display => "display",
    }
}

/// Runs of whitespace in ODF text count as one space (`text:s` writes the others).
fn collapse(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut prev_space = false;
    for c in t.chars() {
        if c == ' ' || c == '\n' || c == '\r' || c == '\t' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out
}

/// The text of each paragraph inside (notes, comments, deletions, headers).
fn para_texts(el: &El) -> Vec<String> {
    let mut out = vec![];
    for e in el.elements() {
        match e.name.as_str() {
            "text:p" | "text:h" => {
                let mut s = String::new();
                inline_text(e, &mut s, false);
                out.push(s.trim().to_string());
            }
            "dc:creator" | "dc:date" | "meta:date-string" => {}
            _ => out.extend(para_texts(e)),
        }
    }
    out
}

fn inline_text(el: &El, s: &mut String, hf: bool) {
    for k in &el.kids {
        match k {
            Node::Text(t) => s.push_str(&collapse(t)),
            Node::El(e) => match e.name.as_str() {
                "text:s" => {
                    let n: usize = e.attr("text:c").and_then(|v| v.parse().ok()).unwrap_or(1).min(100);
                    s.push_str(&" ".repeat(n));
                }
                "text:tab" => s.push(if hf { ' ' } else { '\t' }),
                "text:line-break" => s.push(' '),
                "text:page-number" if hf => s.push_str("{page}"),
                "text:page-count" if hf => s.push_str("{pages}"),
                "text:title" if hf => s.push_str("{title}"),
                "text:note" | "office:annotation" => {}
                _ => inline_text(e, s, hf),
            },
        }
    }
}

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    let mut pkg = Package::open(bytes, "OpenDocument text")?;
    if let Some(m) = pkg.read("mimetype") {
        let m = String::from_utf8_lossy(&m);
        if !m.trim().starts_with("application/vnd.oasis.opendocument.text") {
            return Err(format!("This OpenDocument file isn't a text document ({}).", m.trim()));
        }
    }
    let content = pkg.xml("content.xml").ok_or("This isn't an OpenDocument text file (no content.xml).")?;
    let styles_xml = pkg.xml("styles.xml");
    let mut conv = Conv {
        pkg,
        styles: HashMap::new(),
        fonts: HashMap::new(),
        lists: HashMap::new(),
        doc: Document::empty(title),
        warnings: vec![],
        warned: HashSet::new(),
        comments: vec![],
        open_comments: vec![],
        commented: HashSet::new(),
        changes: HashMap::new(),
        open_insert: None,
        media: HashMap::new(),
        first_block: true,
        text_width: 451.0,
        defaults: TProps::default(),
    };
    // Fonts, then styles (named ones, then the automatic ones of each part).
    for root in styles_xml.iter().chain(std::iter::once(&content)) {
        if let Some(ff) = root.child("office:font-face-decls") {
            for f in ff.children("style:font-face") {
                let Some(name) = f.attr("style:name") else { continue };
                let fam = match f.attr("style:font-family-generic") {
                    Some("roman") => "serif",
                    Some("modern") => "mono",
                    Some("swiss") => "sans",
                    _ => family_of_font(f.attr("svg:font-family").unwrap_or(name)),
                };
                let fam = if f.attr("style:font-pitch") == Some("fixed") { "mono" } else { fam };
                // The family name says more than the generic class when they disagree.
                let by_name = family_of_font(f.attr("svg:font-family").unwrap_or(name));
                conv.fonts.insert(name.to_string(), if by_name != "sans" { by_name } else { fam });
            }
        }
    }
    let mut page_layouts: HashMap<String, PageSetup> = HashMap::new();
    let mut master: Option<(String, Option<String>, Option<String>)> = None;
    if let Some(st) = &styles_xml {
        if let Some(s) = st.child("office:styles") {
            conv.read_styles(s);
        }
        if let Some(s) = st.child("office:automatic-styles") {
            conv.read_styles(s);
            for pl in s.children("style:page-layout") {
                let Some(name) = pl.attr("style:name") else { continue };
                let mut setup = PageSetup::a4();
                setup.footer.clear();
                if let Some(p) = pl.child("style:page-layout-properties") {
                    if let (Some(w), Some(h)) = (p.attr("fo:page-width").and_then(length), p.attr("fo:page-height").and_then(length)) {
                        setup.width = w;
                        setup.height = h;
                    }
                    let m = |k: &str, d: f32| p.attr(k).and_then(length).or_else(|| p.attr("fo:margin").and_then(length)).unwrap_or(d);
                    setup.margin_top = m("fo:margin-top", setup.margin_top);
                    setup.margin_bottom = m("fo:margin-bottom", setup.margin_bottom);
                    setup.margin_left = m("fo:margin-left", setup.margin_left);
                    setup.margin_right = m("fo:margin-right", setup.margin_right);
                }
                // Headers sit inside the margin in ODF: their height counts too.
                for (key, top) in [("style:header-style", true), ("style:footer-style", false)] {
                    if let Some(hp) = pl.child(key).and_then(|h| h.child("style:header-footer-properties")) {
                        let extra = hp.attr("fo:min-height").and_then(length).unwrap_or(0.0) + if top { hp.attr("fo:margin-bottom") } else { hp.attr("fo:margin-top") }.and_then(length).unwrap_or(0.0);
                        if top {
                            setup.margin_top += extra;
                        } else {
                            setup.margin_bottom += extra;
                        }
                    }
                }
                page_layouts.insert(name.to_string(), setup);
            }
        }
        if let Some(ms) = st.child("office:master-styles") {
            let pages: Vec<&El> = ms.children("style:master-page").collect();
            let pick = pages.iter().find(|p| p.attr("style:name") == Some("Standard")).or(pages.first());
            if let Some(mp) = pick {
                let text = |k: &str| {
                    mp.child(k).filter(|h| h.attr("style:display") != Some("false")).map(|h| {
                        let mut s = String::new();
                        for p in h.elements() {
                            if !s.is_empty() {
                                s.push(' ');
                            }
                            inline_text(p, &mut s, true);
                        }
                        s.split_whitespace().collect::<Vec<_>>().join(" ")
                    })
                };
                master = Some((mp.attr("style:page-layout-name").unwrap_or("").to_string(), text("style:header"), text("style:footer")));
            }
        }
    }
    if let Some(s) = content.child("office:automatic-styles") {
        conv.read_styles(s);
    }
    let mut setup = PageSetup::a4();
    setup.footer.clear();
    if let Some((layout, header, footer)) = master {
        if let Some(l) = page_layouts.get(&layout) {
            setup = l.clone();
        }
        setup.header = header.unwrap_or_default();
        setup.footer = footer.unwrap_or_default();
    }
    conv.text_width = setup.text_width();

    let text = content.child("office:body").and_then(|b| b.child("office:text")).ok_or("This OpenDocument file has no text.")?;
    if let Some(tc) = text.child("text:tracked-changes") {
        for r in tc.children("text:changed-region") {
            let Some(id) = r.attr("text:id").or(r.attr("xml:id")) else { continue };
            let (insertion, kind_el) = if let Some(i) = r.child("text:insertion") {
                (true, i)
            } else if let Some(d) = r.child("text:deletion") {
                (false, d)
            } else {
                conv.warn("format-change", "Tracked formatting changes are kept as their result.");
                continue;
            };
            let author = kind_el.child("office:change-info").and_then(|c| c.child("dc:creator")).map(|c| c.own_text()).unwrap_or_else(|| "Unknown".into());
            let deleted = if insertion { vec![] } else { para_texts(kind_el) };
            conv.changes.insert(id.to_string(), Change { insertion, author, deleted });
        }
    }
    let mut blocks = conv.blocks(text, None, 0);
    while matches!(blocks.last(), Some(Block::PageBreak { .. })) {
        blocks.pop();
    }
    let mut merged: Vec<Block> = Vec::with_capacity(blocks.len());
    for b in blocks {
        if let Block::Paragraph(p) = &b
            && p.style == ParaStyle::Caption
            && let Some(Block::Image(im)) = merged.last_mut()
            && im.caption.is_empty()
            && !p.is_empty()
        {
            im.caption = p.text();
            continue;
        }
        merged.push(b);
    }

    // Metadata.
    if let Some(meta) = conv.pkg.xml("meta.xml").and_then(|m| m.child("office:meta").cloned()) {
        if let Some(a) = meta.child("meta:initial-creator").or(meta.child("dc:creator")).map(|e| e.own_text()).filter(|s| !s.trim().is_empty()) {
            conv.doc.meta.author = a.trim().to_string();
        }
        if let Some(d) = parse_date(meta.child("meta:creation-date").map(|e| e.own_text()).as_deref()) {
            conv.doc.meta.created = Some(d);
        }
        if let Some(d) = parse_date(meta.child("dc:date").map(|e| e.own_text()).as_deref()) {
            conv.doc.meta.modified = Some(d);
        }
    }
    let track = content.find("text:tracked-changes").is_some_and(|t| t.attr("text:track-changes") != Some("false"));

    let mut doc = conv.doc;
    let i = doc.add_page(PageKind::Doc, Some(&page_name(title)), None).map_err(|e| e.0)?;
    let t = doc.page_mut(i).doc_mut().unwrap();
    t.blocks = merged.into_iter().collect();
    folio_core::text::ensure_nonempty(&mut t.blocks);
    t.setup = setup;
    t.comments = conv.comments;
    t.track_changes = track && !conv.changes.is_empty();
    Ok(Imported { doc, warnings: conv.warnings, format: "odt" })
}
