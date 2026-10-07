//! DOCX in: WordprocessingML to folio blocks.

use std::collections::{HashMap, HashSet};

use folio_core::text::{Comment, ImageBlock, PageSetup, Reply, Table, TableCell};
use folio_core::{Align, Block, Document, Id, ListKind, PageKind, ParaStyle, Paragraph, Run, RunStyle};

use super::xml::{El, Node, Package, family_of_font, hex_color};
use crate::Imported;

const REL_IMAGE: &str = "/image";
const REL_HYPERLINK: &str = "/hyperlink";

struct Rel {
    kind: String,
    target: String,
    external: bool,
}

fn read_rels(pkg: &mut Package, part: &str) -> HashMap<String, Rel> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_path = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
    let mut out = HashMap::new();
    if let Some(root) = pkg.xml(&rels_path) {
        for r in root.elements().filter(|e| e.local() == "Relationship") {
            let (Some(id), Some(target)) = (r.attr("Id"), r.attr("Target")) else { continue };
            let external = r.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("external"));
            let target = if external { target.to_string() } else { resolve_path(dir, target) };
            out.insert(id.to_string(), Rel { kind: r.attr("Type").unwrap_or("").to_string(), target, external });
        }
    }
    out
}

/// A part's target relative to the folder of the part that names it.
fn resolve_path(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = if dir.is_empty() { vec![] } else { dir.split('/').collect() };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn on(el: &El) -> bool {
    !matches!(el.attr("w:val"), Some("0" | "false" | "off" | "none"))
}

/// Character formatting read from `w:rPr` elements, layered.
#[derive(Clone, Debug, Default, PartialEq)]
struct RProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    /// `#rrggbb`, or empty for automatic.
    color: Option<String>,
    highlight: Option<String>,
    size: Option<f32>,
    /// A family: sans, serif, mono, display.
    font: Option<&'static str>,
    /// 1 superscript, -1 subscript, 0 baseline.
    vert: Option<i8>,
    hidden: Option<bool>,
    code: Option<bool>,
}

impl RProps {
    fn read(rpr: &El, theme: &Theme) -> RProps {
        let mut p = RProps::default();
        for e in rpr.elements() {
            match e.name.as_str() {
                "w:b" => p.bold = Some(on(e)),
                "w:i" => p.italic = Some(on(e)),
                "w:u" => p.underline = Some(on(e)),
                "w:strike" | "w:dstrike" => p.strike = Some(on(e)),
                "w:color" => p.color = Some(e.attr("w:val").and_then(hex_color).unwrap_or_default()),
                "w:highlight" => p.highlight = Some(e.attr("w:val").and_then(highlight_hex).unwrap_or_default()),
                "w:shd" => {
                    if p.highlight.is_none()
                        && let Some(f) = e.attr("w:fill").and_then(hex_color)
                    {
                        // White and automatic shading is no highlight.
                        if f != "#ffffff" {
                            p.highlight = Some(f);
                        }
                    }
                }
                "w:sz" => p.size = e.attr("w:val").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 2.0),
                "w:rFonts" => {
                    let name = e.attr("w:ascii").or(e.attr("w:hAnsi")).map(str::to_string).or_else(|| e.attr("w:asciiTheme").or(e.attr("w:hAnsiTheme")).map(|t| theme.font(t)));
                    if let Some(n) = name {
                        p.font = Some(family_of_font(&n));
                    }
                }
                "w:vertAlign" => {
                    p.vert = Some(match e.attr("w:val") {
                        Some("superscript") => 1,
                        Some("subscript") => -1,
                        _ => 0,
                    })
                }
                "w:vanish" => p.hidden = Some(on(e)),
                _ => {}
            }
        }
        p
    }

    /// `other` on top of `self`.
    fn over(&self, other: &RProps) -> RProps {
        RProps {
            bold: other.bold.or(self.bold),
            italic: other.italic.or(self.italic),
            underline: other.underline.or(self.underline),
            strike: other.strike.or(self.strike),
            color: other.color.clone().or(self.color.clone()),
            highlight: other.highlight.clone().or(self.highlight.clone()),
            size: other.size.or(self.size),
            font: other.font.or(self.font),
            vert: other.vert.or(self.vert),
            hidden: other.hidden.or(self.hidden),
            code: other.code.or(self.code),
        }
    }
}

/// Word's named highlight colours.
fn highlight_hex(name: &str) -> Option<String> {
    Some(
        match name {
            "yellow" => "#ffff00",
            "green" => "#00ff00",
            "cyan" => "#00ffff",
            "magenta" => "#ff00ff",
            "blue" => "#0000ff",
            "red" => "#ff0000",
            "darkBlue" => "#000080",
            "darkCyan" => "#008080",
            "darkGreen" => "#008000",
            "darkMagenta" => "#800080",
            "darkRed" => "#800000",
            "darkYellow" => "#808000",
            "darkGray" => "#808080",
            "lightGray" => "#c0c0c0",
            "black" => "#000000",
            "white" => "#ffffff",
            _ => return None,
        }
        .to_string(),
    )
}

#[derive(Default)]
struct Theme {
    major: String,
    minor: String,
}

impl Theme {
    fn font(&self, which: &str) -> String {
        if which.starts_with("major") { self.major.clone() } else { self.minor.clone() }
    }
}

#[derive(Clone, Default)]
struct StyleDef {
    name: String,
    kind: String,
    based_on: Option<String>,
    outline: Option<u8>,
    num: Option<(String, Option<u8>)>,
    jc: Option<Align>,
    rpr: Option<El>,
}

/// What a numbering level draws.
#[derive(Clone, Default)]
struct Level {
    fmt: String,
    text: String,
}

#[derive(Default)]
struct Numbering {
    /// numId → (abstractNumId, level overrides).
    nums: HashMap<String, (String, HashMap<u8, Level>)>,
    abstracts: HashMap<String, HashMap<u8, Level>>,
    /// abstractNumId → the numbering style it points at (`w:numStyleLink`).
    style_links: HashMap<String, String>,
}

fn levels_of(el: &El) -> HashMap<u8, Level> {
    let mut out = HashMap::new();
    for l in el.children("w:lvl") {
        let i = l.attr("w:ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
        out.insert(i, Level { fmt: l.child_attr("w:numFmt", "w:val").unwrap_or("decimal").to_string(), text: l.child_attr("w:lvlText", "w:val").unwrap_or("").to_string() });
    }
    out
}

/// A list kind, and whether its glyph says "done", from a numbering level.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ListOf {
    Kind(ListKind, bool),
    /// No glyph (a checklist written by folio, or an unnumbered list).
    Plain,
}

pub(crate) fn is_check_glyph(t: &str) -> Option<bool> {
    match t.trim() {
        "☐" | "□" | "❏" | "❑" | "\u{F0A8}" | "\u{F06F}" | "\u{F071}" => Some(false),
        "☒" | "☑" | "✓" | "✔" | "■" | "\u{F0FE}" | "\u{F078}" => Some(true),
        _ => None,
    }
}

/// Paragraph settings resolved from a style and direct `w:pPr`.
#[derive(Clone, Default)]
struct PProps {
    style: ParaStyle,
    align: Option<Align>,
    num: Option<(String, u8)>,
    outline: Option<u8>,
    /// Character formatting of the paragraph style (layered on document defaults).
    rprops: RProps,
    /// What counts as "no direct formatting" for runs of this paragraph.
    reference: RProps,
}

struct Field {
    instr: String,
    in_result: bool,
    link: Option<String>,
}

/// One paragraph being turned into blocks.
struct Builder {
    out: Vec<Block>,
    cur: Paragraph,
    /// Something other than a paragraph came out (page break, picture): drop empty pieces.
    made_other: bool,
    fields: Vec<Field>,
    pending_note: Option<String>,
    /// Text boxes found inside: their paragraphs go after this one.
    deferred: Vec<Block>,
    props: PProps,
}

impl Builder {
    fn new(props: PProps, para: Paragraph) -> Self {
        Builder { out: vec![], cur: para, made_other: false, fields: vec![], pending_note: None, deferred: vec![], props }
    }

    fn continuation(&self) -> Paragraph {
        Paragraph { style: self.cur.style, align: self.cur.align, ..Default::default() }
    }

    fn flush(&mut self) {
        let next = self.continuation();
        let p = std::mem::replace(&mut self.cur, next);
        self.out.push(Block::Paragraph(p));
    }

    fn push_block(&mut self, b: Block) {
        let next = self.continuation();
        let p = std::mem::replace(&mut self.cur, next);
        self.out.push(Block::Paragraph(p));
        self.out.push(b);
        self.made_other = true;
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

    /// Inside a field's instructions (its text isn't shown).
    fn in_instr(&self) -> bool {
        self.fields.iter().any(|f| !f.in_result)
    }

    fn field_link(&self) -> Option<String> {
        self.fields.iter().rev().find_map(|f| f.link.clone())
    }
}

/// What surrounds a run: a link, a tracked change.
#[derive(Clone, Default)]
struct Ctx {
    link: Option<String>,
    inserted: Option<String>,
    deleted: Option<String>,
}

struct Conv {
    pkg: Package,
    styles: HashMap<String, StyleDef>,
    default_para: Option<String>,
    defaults: RProps,
    numbering: Numbering,
    theme: Theme,
    doc: Document,
    media: HashMap<String, Option<(Id, u32, u32)>>,
    warnings: Vec<String>,
    warned: HashSet<&'static str>,
    comments: HashMap<String, Id>,
    open_comments: Vec<Id>,
    commented: HashSet<Id>,
    footnotes: HashMap<String, String>,
    endnotes: HashMap<String, String>,
    sections: Vec<El>,
    text_width: f32,
}

impl Conv {
    fn warn(&mut self, key: &'static str, msg: impl Into<String>) {
        if self.warned.insert(key) {
            self.warnings.push(msg.into());
        }
    }

    // ---- styles -------------------------------------------------------------------------------

    fn style_chain(&self, id: &str) -> Vec<&StyleDef> {
        let mut out = vec![];
        let mut cur = Some(id.to_string());
        let mut seen = HashSet::new();
        while let Some(id) = cur {
            if !seen.insert(id.clone()) {
                break;
            }
            let Some(s) = self.styles.get(&id) else { break };
            out.push(s);
            cur = s.based_on.clone();
        }
        out
    }

    /// The folio style a Word paragraph style stands for.
    fn para_style_of(&self, id: &str) -> ParaStyle {
        for s in self.style_chain(id) {
            if let Some(p) = classify(&s.name).or_else(|| classify(id)) {
                return p;
            }
            if let Some(l) = s.outline {
                return heading_for_level(l);
            }
        }
        ParaStyle::Normal
    }

    /// Character formatting a style chain gives (base first).
    fn chain_rprops(&self, id: &str) -> RProps {
        let mut p = RProps::default();
        for s in self.style_chain(id).into_iter().rev() {
            if let Some(r) = &s.rpr {
                p = p.over(&RProps::read(r, &self.theme));
            }
            if s.kind == "character" && is_code_name(&s.name) {
                p.code = Some(true);
            }
        }
        p
    }

    fn pprops(&self, ppr: Option<&El>) -> PProps {
        let style_id = ppr.and_then(|p| p.child_attr("w:pStyle", "w:val")).map(str::to_string).or_else(|| self.default_para.clone());
        let mut out = PProps::default();
        if let Some(id) = &style_id {
            out.style = self.para_style_of(id);
            for s in self.style_chain(id) {
                if out.num.is_none()
                    && let Some((n, l)) = &s.num
                {
                    out.num = Some((n.clone(), l.unwrap_or(0)));
                }
                if out.align.is_none() {
                    out.align = s.jc;
                }
            }
            out.rprops = self.defaults.over(&self.chain_rprops(id));
        } else {
            out.rprops = self.defaults.clone();
        }
        if let Some(ppr) = ppr {
            if let Some(n) = ppr.child("w:numPr") {
                let id = n.child_attr("w:numId", "w:val").map(str::to_string);
                let lvl = n.child_attr("w:ilvl", "w:val").and_then(|v| v.parse().ok());
                match (id, &mut out.num) {
                    (Some(id), _) => out.num = Some((id, lvl.unwrap_or(0))),
                    (None, Some((_, l))) => *l = lvl.unwrap_or(*l),
                    _ => {}
                }
            }
            if let Some(a) = ppr.child_attr("w:jc", "w:val") {
                out.align = Some(align_of(a));
            }
            if let Some(o) = ppr.child_attr("w:outlineLvl", "w:val").and_then(|v| v.parse::<u8>().ok())
                && o < 9
                && out.style == ParaStyle::Normal
            {
                out.style = heading_for_level(o);
            }
        }
        // Runs are compared with the paragraph style when folio has its own look for it, with
        // Normal otherwise (so a custom bold style stays bold).
        out.reference = if out.style == ParaStyle::Normal {
            match &self.default_para {
                Some(n) => self.defaults.over(&self.chain_rprops(n)),
                None => self.defaults.clone(),
            }
        } else {
            out.rprops.clone()
        };
        out
    }

    fn list_of(&self, num: &str, lvl: u8) -> Option<ListOf> {
        if num == "0" {
            return None;
        }
        let (abs, overrides) = self.numbering.nums.get(num)?;
        let level = overrides.get(&lvl).cloned().or_else(|| {
            let mut abs = abs.clone();
            // A list that points at a numbering style: follow it to the real definition.
            if let Some(style) = self.numbering.style_links.get(&abs)
                && let Some((n, _)) = self.styles.get(style).and_then(|s| s.num.clone())
                && let Some((a, _)) = self.numbering.nums.get(&n)
            {
                abs = a.clone();
            }
            self.numbering.abstracts.get(&abs).and_then(|l| l.get(&lvl).cloned())
        })?;
        Some(match level.fmt.as_str() {
            "none" => ListOf::Plain,
            "bullet" => match is_check_glyph(&level.text) {
                Some(done) => ListOf::Kind(ListKind::Check, done),
                None => ListOf::Kind(ListKind::Bullet, false),
            },
            _ => ListOf::Kind(ListKind::Number, false),
        })
    }

    // ---- media --------------------------------------------------------------------------------

    fn image_from_rel(&mut self, rels: &HashMap<String, Rel>, rid: &str) -> Option<(Id, u32, u32)> {
        let rel = rels.get(rid)?;
        if rel.external {
            self.warn("linked-picture", "Pictures linked to files outside the document are left out.");
            return None;
        }
        let target = rel.target.clone();
        if let Some(m) = self.media.get(&target) {
            return m.clone();
        }
        let got = self.load_image(&target);
        self.media.insert(target, got.clone());
        got
    }

    fn load_image(&mut self, target: &str) -> Option<(Id, u32, u32)> {
        let bytes = self.pkg.read(target)?;
        let name = target.rsplit('/').next().unwrap_or(target).to_string();
        let mime = folio_core::Media::sniff(&bytes);
        let bytes = if mime == "application/octet-stream" || mime == "image/bmp" {
            // TIFF and the like: turn into PNG when the image crate can read them.
            match image::load_from_memory(&bytes) {
                Ok(img) => {
                    let mut out = std::io::Cursor::new(Vec::new());
                    if img.write_to(&mut out, image::ImageFormat::Png).is_err() {
                        return None;
                    }
                    out.into_inner()
                }
                Err(_) => {
                    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_uppercase();
                    self.warn("picture-format", format!("Pictures in formats folio can't show ({ext}, like Windows metafiles) are left out."));
                    return None;
                }
            }
        } else {
            bytes
        };
        let id = self.doc.add_media(&name, bytes);
        let m = &self.doc.media[&id];
        Some((id, m.width, m.height))
    }

    // ---- paragraphs ---------------------------------------------------------------------------

    fn paragraph(&mut self, p: &El, rels: &HashMap<String, Rel>) -> Vec<Block> {
        let ppr = p.child("w:pPr");
        let props = self.pprops(ppr);
        let mut para = Paragraph { style: props.style, align: props.align.unwrap_or_default(), ..Default::default() };
        let mut checked_by_glyph = None;
        let mut plain_list = false;
        if let Some((num, lvl)) = &props.num {
            match self.list_of(num, *lvl) {
                Some(ListOf::Kind(k, done)) => {
                    para.list = Some(k);
                    para.level = (*lvl).min(5);
                    if k == ListKind::Check {
                        checked_by_glyph = Some(done);
                    }
                }
                Some(ListOf::Plain) => {
                    plain_list = true;
                    para.level = (*lvl).min(5);
                }
                None => {}
            }
        }
        let mut b = Builder::new(props, para);
        if ppr.and_then(|p| p.child("w:pageBreakBefore")).is_some_and(on) {
            b.out.push(Block::PageBreak { id: Id::new() });
            b.made_other = true;
        }
        let ctx = Ctx::default();
        self.inline_children(p, &ctx, &mut b, rels);
        // A section ends with this paragraph.
        if let Some(sect) = ppr.and_then(|p| p.child("w:sectPr")) {
            self.sections.push(sect.clone());
            b.made_other = true;
            let continuous = sect.child_attr("w:type", "w:val") == Some("continuous");
            let mut blocks = b.finish();
            if !continuous {
                blocks.push(Block::PageBreak { id: Id::new() });
            }
            return fix_checks(blocks, checked_by_glyph, plain_list);
        }
        fix_checks(b.finish(), checked_by_glyph, plain_list)
    }

    fn inline_children(&mut self, el: &El, ctx: &Ctx, b: &mut Builder, rels: &HashMap<String, Rel>) {
        for k in el.elements() {
            self.inline(k, ctx, b, rels);
        }
    }

    fn inline(&mut self, e: &El, ctx: &Ctx, b: &mut Builder, rels: &HashMap<String, Rel>) {
        match e.name.as_str() {
            "w:r" => self.run(e, ctx, b, rels),
            "w:hyperlink" => {
                let mut c = ctx.clone();
                if let Some(rid) = e.attr("r:id")
                    && let Some(rel) = rels.get(rid)
                    && rel.kind.ends_with(REL_HYPERLINK)
                {
                    c.link = Some(rel.target.clone());
                }
                self.inline_children(e, &c, b, rels);
            }
            "w:ins" | "w:moveTo" => {
                let mut c = ctx.clone();
                c.inserted = Some(e.attr("w:author").unwrap_or("Unknown").to_string());
                self.inline_children(e, &c, b, rels);
            }
            "w:del" | "w:moveFrom" => {
                let mut c = ctx.clone();
                c.deleted = Some(e.attr("w:author").unwrap_or("Unknown").to_string());
                self.inline_children(e, &c, b, rels);
            }
            "w:smartTag" | "w:customXml" | "w:dir" | "w:bdo" => self.inline_children(e, ctx, b, rels),
            "w:sdt" => {
                if let Some(c) = e.child("w:sdtContent") {
                    self.inline_children(c, ctx, b, rels);
                }
            }
            "w:fldSimple" => {
                let instr = e.attr("w:instr").unwrap_or("");
                let mut c = ctx.clone();
                if let Some(url) = hyperlink_target(instr) {
                    c.link = Some(url);
                }
                self.inline_children(e, &c, b, rels);
            }
            "w:commentRangeStart" => {
                if let Some(id) = e.attr("w:id").and_then(|i| self.comments.get(i)).cloned() {
                    self.open_comments.push(id);
                }
            }
            "w:commentRangeEnd" => {
                if let Some(id) = e.attr("w:id").and_then(|i| self.comments.get(i)).cloned() {
                    self.open_comments.retain(|c| *c != id);
                }
            }
            "m:oMath" | "m:oMathPara" => {
                let mut ts = vec![];
                e.find_all("m:t", &mut ts);
                let text: String = ts.iter().map(|t| t.own_text()).collect();
                if !text.is_empty() {
                    let style = RunStyle { italic: true, ..Default::default() };
                    self.emit(b, &text, style, ctx);
                }
                self.warn("math", "Equations became plain text.");
            }
            "mc:AlternateContent" => {
                if let Some(c) = e.child("mc:Choice").or(e.child("mc:Fallback")) {
                    self.inline_children(c, ctx, b, rels);
                }
            }
            "w:pPr" | "w:proofErr" | "w:bookmarkStart" | "w:bookmarkEnd" | "w:permStart" | "w:permEnd" | "w:moveFromRangeStart" | "w:moveFromRangeEnd" | "w:moveToRangeStart" | "w:moveToRangeEnd" | "w:customXmlInsRangeStart" | "w:customXmlInsRangeEnd" | "w:customXmlDelRangeStart" | "w:customXmlDelRangeEnd" | "w:sdtPr" | "w:sdtEndPr" => {}
            _ => {}
        }
    }

    fn run_style(&self, b: &Builder, rpr: Option<&El>, ctx: &Ctx) -> (RunStyle, bool) {
        let mut fin = b.props.rprops.clone();
        if let Some(rpr) = rpr {
            if let Some(cs) = rpr.child_attr("w:rStyle", "w:val")
                && !self.styles.get(cs).is_some_and(|d| is_link_style(&d.name))
                && !is_link_style(cs)
            {
                fin = fin.over(&self.chain_rprops(cs));
            }
            fin = fin.over(&RProps::read(rpr, &self.theme));
        }
        let r = &b.props.reference;
        let spec = b.props.style.spec();
        let set = |a: Option<bool>, base: Option<bool>| a == Some(true) && base != Some(true);
        let mut s = RunStyle {
            bold: set(fin.bold, r.bold) && !spec.bold,
            italic: set(fin.italic, r.italic) && !spec.italic,
            underline: set(fin.underline, r.underline),
            strike: set(fin.strike, r.strike),
            code: fin.code == Some(true),
            superscript: fin.vert == Some(1),
            subscript: fin.vert == Some(-1),
            ..Default::default()
        };
        if let Some(c) = &fin.color
            && !c.is_empty()
            && fin.color != r.color
        {
            s.color = Some(c.clone());
        }
        if let Some(h) = &fin.highlight
            && !h.is_empty()
            && fin.highlight != r.highlight
        {
            s.highlight = Some(h.clone());
        }
        if let Some(sz) = fin.size
            && r.size.is_none_or(|rs| (rs - sz).abs() > 0.01)
            && (sz - spec.size).abs() > 0.01
        {
            s.size = Some(sz);
        }
        if let Some(f) = fin.font
            && fin.font != r.font
        {
            if f == "mono" {
                s.code = true;
            } else {
                let spec_family = match spec.family {
                    folio_core::text::Family::Sans => "sans",
                    folio_core::text::Family::Serif => "serif",
                    folio_core::text::Family::Mono => "mono",
                    folio_core::text::Family::Display => "display",
                };
                if f != spec_family {
                    s.font = Some(f.to_string());
                }
            }
        }
        if b.props.style == ParaStyle::Code {
            s.code = false;
        }
        s.link = b.field_link().or_else(|| ctx.link.clone());
        // Links look like links in folio already: drop the blue and the underline apps add.
        if s.link.is_some() {
            s.underline = false;
            if s.color.as_deref().is_some_and(is_link_blue) {
                s.color = None;
            }
        }
        s.inserted = ctx.inserted.clone();
        s.deleted = ctx.deleted.clone();
        (s, fin.hidden == Some(true))
    }

    fn emit(&mut self, b: &mut Builder, text: &str, mut style: RunStyle, ctx: &Ctx) {
        if text.is_empty() {
            return;
        }
        if style.link.is_none() {
            style.link = ctx.link.clone();
        }
        style.comment = self.open_comments.last().cloned();
        if let Some(c) = &style.comment {
            self.commented.insert(c.clone());
        }
        if let Some(n) = b.pending_note.take() {
            style.note = Some(n);
        }
        b.cur.runs.push(Run { text: text.to_string(), style });
    }

    fn attach_note(&mut self, b: &mut Builder, note: String) {
        match b.cur.runs.iter_mut().rev().find(|r| !r.text.is_empty()) {
            Some(r) if r.style.note.is_none() => r.style.note = Some(note),
            Some(r) => {
                let n = r.style.note.take().unwrap_or_default();
                r.style.note = Some(format!("{n} {note}"));
            }
            None => b.pending_note = Some(note),
        }
    }

    fn run(&mut self, r: &El, ctx: &Ctx, b: &mut Builder, rels: &HashMap<String, Rel>) {
        let (style, hidden) = self.run_style(b, r.child("w:rPr"), ctx);
        if hidden {
            if r.elements().any(|e| e.is("w:t")) {
                self.warn("hidden", "Hidden text is left out.");
            }
            return;
        }
        for e in r.elements() {
            match e.name.as_str() {
                "w:t" | "w:delText" => {
                    if b.in_instr() {
                        continue;
                    }
                    let mut st = style.clone();
                    st.link = b.field_link().or(st.link);
                    self.emit(b, &e.own_text(), st, ctx);
                }
                "w:instrText" | "w:delInstrText" => {
                    if let Some(f) = b.fields.last_mut()
                        && !f.in_result
                    {
                        f.instr.push_str(&e.own_text());
                    }
                }
                "w:fldChar" => match e.attr("w:fldCharType") {
                    Some("begin") => b.fields.push(Field { instr: String::new(), in_result: false, link: None }),
                    Some("separate") => {
                        if let Some(f) = b.fields.last_mut() {
                            f.in_result = true;
                            f.link = hyperlink_target(&f.instr);
                        }
                    }
                    Some("end") => {
                        b.fields.pop();
                    }
                    _ => {}
                },
                "w:tab" | "w:ptab" => {
                    if !b.in_instr() {
                        self.emit(b, "\t", style.clone(), ctx);
                    }
                }
                "w:noBreakHyphen" => self.emit(b, "\u{2011}", style.clone(), ctx),
                "w:sym" => {
                    let c = e.attr("w:char").and_then(|h| u32::from_str_radix(h, 16).ok()).map(|v| if v >= 0xF000 { v - 0xF000 } else { v });
                    let ch = match c {
                        Some(0xB7) => Some('•'),
                        Some(v) if v >= 0x20 => char::from_u32(v),
                        _ => None,
                    };
                    if let Some(ch) = ch {
                        self.emit(b, &ch.to_string(), style.clone(), ctx);
                    }
                }
                "w:br" | "w:cr" => match e.attr("w:type") {
                    Some("page") => b.push_block(Block::PageBreak { id: Id::new() }),
                    Some("column") => self.emit(b, " ", style.clone(), ctx),
                    _ => b.flush(),
                },
                "w:footnoteReference" | "w:endnoteReference" => {
                    let id = e.attr("w:id").unwrap_or("");
                    let note = if e.is("w:footnoteReference") {
                        self.footnotes.get(id).cloned()
                    } else {
                        self.warn("endnotes", "Endnotes became footnotes.");
                        self.endnotes.get(id).cloned()
                    };
                    if let Some(n) = note {
                        self.attach_note(b, n);
                    }
                }
                "w:commentReference" => {
                    // A comment on a point (no range): it goes on the text just before.
                    if let Some(id) = e.attr("w:id").and_then(|i| self.comments.get(i)).cloned()
                        && !self.commented.contains(&id)
                        && let Some(r) = b.cur.runs.iter_mut().rev().find(|r| !r.text.is_empty() && r.style.comment.is_none())
                    {
                        r.style.comment = Some(id.clone());
                        self.commented.insert(id);
                    }
                }
                "w:drawing" => self.drawing(e, b, rels),
                "w:pict" => self.pict(e, b, rels),
                "w:object" => {
                    self.warn("object", "Embedded objects (spreadsheets, equations made with old editors) are left out.");
                }
                "mc:AlternateContent" => {
                    if let Some(c) = e.child("mc:Choice").or(e.child("mc:Fallback")) {
                        // The choice holds run content.
                        let fake = El { name: "w:r".into(), attrs: vec![], kids: r.kids.iter().filter(|n| matches!(n, Node::El(x) if x.is("w:rPr"))).cloned().chain(c.kids.iter().cloned()).collect() };
                        self.run(&fake, ctx, b, rels);
                    }
                }
                _ => {}
            }
        }
    }

    fn picture(&mut self, b: &mut Builder, rels: &HashMap<String, Rel>, rid: &str, width_pt: Option<f32>, alt: String, floating: bool) {
        let Some((media, pw, _)) = self.image_from_rel(rels, rid) else { return };
        if floating {
            self.warn("floating", "Pictures that floated beside the text are placed in line.");
        }
        let natural = if pw > 0 { pw as f32 * 0.75 } else { 0.0 };
        let mut width = width_pt.filter(|w| *w > 0.5).unwrap_or(natural);
        if width >= self.text_width - 0.5 {
            width = 0.0;
        }
        let align = b.cur.align;
        b.push_block(Block::Image(ImageBlock { id: Id::new(), media, width, caption: String::new(), alt, align }));
    }

    fn drawing(&mut self, d: &El, b: &mut Builder, rels: &HashMap<String, Rel>) {
        let Some(holder) = d.elements().find(|e| e.is("wp:inline") || e.is("wp:anchor")) else { return };
        let floating = holder.is("wp:anchor");
        let width = holder.child("wp:extent").and_then(|x| x.attr("cx")).and_then(|v| v.parse::<f64>().ok()).map(|emu| (emu / 12700.0) as f32);
        let alt = holder.child("wp:docPr").map(|p| p.attr("descr").filter(|s| !s.is_empty()).or(p.attr("title")).unwrap_or("").to_string()).unwrap_or_default();
        let mut blips = vec![];
        holder.find_all("a:blip", &mut blips);
        let embeds: Vec<String> = blips.iter().filter_map(|bl| bl.attr("r:embed").or(bl.attr("r:link")).map(str::to_string)).collect();
        if !embeds.is_empty() {
            let single = embeds.len() == 1;
            for rid in embeds {
                self.picture(b, rels, &rid, if single { width } else { None }, alt.clone(), floating);
            }
            return;
        }
        if let Some(tb) = holder.find("w:txbxContent") {
            self.text_box(tb, b, rels);
            return;
        }
        if holder.find("c:chart").is_some() {
            self.warn("chart", "Charts in Word files are left out (folio charts read a sheet: rebuild them from a sheet).");
        } else if holder.find("dgm:relIds").is_some() {
            self.warn("smartart", "SmartArt diagrams are left out.");
        } else {
            self.warn("shape", "Drawn shapes are left out.");
        }
    }

    fn pict(&mut self, p: &El, b: &mut Builder, rels: &HashMap<String, Rel>) {
        if let Some(img) = p.find("v:imagedata")
            && let Some(rid) = img.attr("r:id").or(img.attr("o:relid"))
        {
            let width = p.find("v:shape").and_then(|s| s.attr("style")).and_then(|st| css_length(st, "width"));
            let alt = p.find("v:shape").and_then(|s| s.attr("alt")).unwrap_or("").to_string();
            self.picture(b, rels, &rid.to_string(), width, alt, false);
            return;
        }
        if let Some(tb) = p.find("w:txbxContent") {
            self.text_box(tb, b, rels);
            return;
        }
        self.warn("shape", "Drawn shapes are left out.");
    }

    fn text_box(&mut self, content: &El, b: &mut Builder, rels: &HashMap<String, Rel>) {
        self.warn("textbox", "Text boxes became ordinary paragraphs.");
        let blocks = self.blocks(content, rels);
        b.deferred.extend(blocks);
    }

    // ---- block containers ---------------------------------------------------------------------

    /// The blocks of a body, cell, text box or content control.
    fn blocks(&mut self, container: &El, rels: &HashMap<String, Rel>) -> Vec<Block> {
        let mut out = vec![];
        for e in container.elements() {
            match e.name.as_str() {
                "w:p" => out.extend(self.paragraph(e, rels)),
                "w:tbl" => out.push(self.table(e, rels)),
                "w:sdt" => {
                    if let Some(c) = e.child("w:sdtContent") {
                        out.extend(self.blocks(c, rels));
                    }
                }
                "w:customXml" | "w:ins" | "w:moveTo" => out.extend(self.blocks(e, rels)),
                "w:sectPr" => self.sections.push(e.clone()),
                "w:commentRangeStart" | "w:commentRangeEnd" => {
                    let mut b = Builder::new(PProps::default(), Paragraph::default());
                    self.inline(e, &Ctx::default(), &mut b, rels);
                }
                "w:altChunk" => self.warn("altchunk", "Content pasted in from other formats (alternative chunks) is left out."),
                "mc:AlternateContent" => {
                    if let Some(c) = e.child("mc:Choice").or(e.child("mc:Fallback")) {
                        out.extend(self.blocks(c, rels));
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn table(&mut self, tbl: &El, rels: &HashMap<String, Rel>) -> Block {
        let mut rows_el: Vec<&El> = vec![];
        collect(tbl, "w:tr", &mut rows_el);
        let mut rows: Vec<Vec<TableCell>> = vec![];
        let mut header = false;
        let mut first_row_bold = true;
        for (ri, tr) in rows_el.iter().enumerate() {
            let mut row = vec![];
            let trpr = tr.child("w:trPr");
            let before = trpr.and_then(|p| p.child_attr("w:gridBefore", "w:val")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            for _ in 0..before {
                row.push(TableCell::default());
            }
            if ri == 0 && trpr.and_then(|p| p.child("w:tblHeader")).is_some_and(on) {
                header = true;
            }
            let mut cells: Vec<&El> = vec![];
            collect(tr, "w:tc", &mut cells);
            for tc in cells {
                let tcpr = tc.child("w:tcPr");
                let span = tcpr.and_then(|p| p.child_attr("w:gridSpan", "w:val")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 64);
                if span > 1 || tcpr.and_then(|p| p.child("w:vMerge")).is_some() || tcpr.and_then(|p| p.child("w:hMerge")).is_some() {
                    self.warn("merged", "Merged table cells were split back into single cells.");
                }
                let fill = tcpr.and_then(|p| p.child("w:shd")).and_then(|s| s.attr("w:fill")).and_then(hex_color).filter(|f| f != "#ffffff");
                let mut cell = self.cell(tc, rels);
                cell.fill = fill;
                if ri == 0 && cell.runs.iter().any(|r| !r.text.trim().is_empty() && !r.style.bold) {
                    first_row_bold = false;
                }
                row.push(cell);
                for _ in 1..span {
                    row.push(TableCell::default());
                }
            }
            let after = trpr.and_then(|p| p.child_attr("w:gridAfter", "w:val")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            for _ in 0..after {
                row.push(TableCell::default());
            }
            rows.push(row);
        }
        if rows.is_empty() {
            rows.push(vec![TableCell::default()]);
        }
        let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for r in &mut rows {
            r.resize(cols, TableCell::default());
        }
        let look_first = tbl.child("w:tblPr").and_then(|p| p.child("w:tblLook")).is_some_and(|l| match l.attr("w:firstRow") {
            Some(v) => v == "1" || v == "true" || v == "on",
            None => l.attr("w:val").and_then(|v| u32::from_str_radix(v, 16).ok()).is_some_and(|v| v & 0x0020 != 0),
        });
        let has_text = rows[0].iter().any(|c| !c.plain().trim().is_empty());
        header = header || look_first || (first_row_bold && has_text && rows.len() > 1);
        if header {
            // folio draws the header row bold itself.
            for c in &mut rows[0] {
                for r in &mut c.runs {
                    r.style.bold = false;
                }
            }
        }
        let grid: Vec<f32> = tbl.child("w:tblGrid").map(|g| g.children("w:gridCol").map(|c| c.attr("w:w").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0)).collect()).unwrap_or_default();
        let widths = if grid.len() == cols && grid.iter().all(|w| *w > 0.0) {
            let sum: f32 = grid.iter().sum();
            grid.iter().map(|w| w / sum).collect()
        } else {
            vec![]
        };
        Block::Table(Table { id: Id::new(), rows, header, widths, link: None, banded: false })
    }

    fn cell(&mut self, tc: &El, rels: &HashMap<String, Rel>) -> TableCell {
        let blocks = self.blocks(tc, rels);
        let mut runs: Vec<Run> = vec![];
        let mut align = None;
        let mut first = true;
        for b in blocks {
            let piece: Vec<Run> = match b {
                Block::Paragraph(p) => {
                    if align.is_none() {
                        align = Some(p.align);
                    }
                    let mut rs = p.runs;
                    if let Some(l) = p.list {
                        let mark = match l {
                            ListKind::Bullet => "• ".to_string(),
                            ListKind::Number => "– ".to_string(),
                            ListKind::Check => if p.checked { "☒ ".into() } else { "☐ ".into() },
                        };
                        rs.insert(0, Run::plain(mark));
                    }
                    rs
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
        TableCell { runs: p.runs, align: align.unwrap_or_default(), fill: None }
    }
}

/// Elements named `name` among `el`'s children, looking through content controls.
fn collect<'a>(el: &'a El, name: &str, out: &mut Vec<&'a El>) {
    for e in el.elements() {
        if e.name == name {
            out.push(e);
        } else if e.is("w:sdt") {
            if let Some(c) = e.child("w:sdtContent") {
                collect(c, name, out);
            }
        } else if e.is("w:customXml") || e.is("w:ins") {
            collect(e, name, out);
        }
    }
}

fn fix_checks(mut blocks: Vec<Block>, glyph: Option<bool>, plain_list: bool) -> Vec<Block> {
    for b in &mut blocks {
        let Block::Paragraph(p) = b else { continue };
        if let Some(done) = glyph {
            p.checked = done;
            continue;
        }
        // folio writes checklists as unmarked list items starting with a box.
        if p.list == Some(ListKind::Bullet) || plain_list {
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
    blocks
}

fn align_of(v: &str) -> Align {
    match v {
        "center" => Align::Center,
        "right" | "end" => Align::Right,
        "both" | "distribute" | "lowKashida" | "mediumKashida" | "highKashida" | "thaiDistribute" => Align::Justify,
        _ => Align::Left,
    }
}

pub(crate) fn heading_for_level(l: u8) -> ParaStyle {
    match l {
        0 => ParaStyle::Heading1,
        1 => ParaStyle::Heading2,
        _ => ParaStyle::Heading3,
    }
}

fn is_link_style(name: &str) -> bool {
    let k: String = name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_lowercase();
    matches!(k.as_str(), "hyperlink" | "followedhyperlink" | "internetlink" | "visitedinternetlink")
}

/// The blues apps colour links with.
pub(crate) fn is_link_blue(hex: &str) -> bool {
    let h = hex.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0);
    let (r, g, b) = ((v >> 16) as i32 & 255, (v >> 8) as i32 & 255, v as i32 & 255);
    b > 120 && b > r + 60 && b > g + 20
}

pub(crate) fn is_code_name(name: &str) -> bool {
    let k: String = name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_lowercase();
    matches!(k.as_str(), "code" | "codechar" | "inlinecode" | "htmlcode" | "sourcetext" | "verbatimchar" | "htmlkeyboard" | "htmltypewriter" | "sourcecode" | "monospace")
}

/// The folio style a Word style name (or id) means, if it is one folio knows.
pub(crate) fn classify(name: &str) -> Option<ParaStyle> {
    let k: String = name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_lowercase();
    if let Some(n) = k.strip_prefix("heading") {
        return match n {
            "" | "1" => Some(ParaStyle::Heading1),
            "2" => Some(ParaStyle::Heading2),
            n if n.parse::<u8>().is_ok() => Some(ParaStyle::Heading3),
            _ => None,
        };
    }
    Some(match k.as_str() {
        "title" => ParaStyle::Title,
        "subtitle" => ParaStyle::Subtitle,
        "quote" | "intensequote" | "blockquote" | "blocktext" | "quotations" | "quotation" => ParaStyle::Quote,
        "caption" | "figure" | "illustration" | "imagecaption" | "tablecaption" => ParaStyle::Caption,
        "code" | "sourcecode" | "htmlpreformatted" | "preformattedtext" | "plaintext" | "codeblock" | "macrotext" => ParaStyle::Code,
        "tocheading" => ParaStyle::Heading1,
        _ => return None,
    })
}

/// The URL of a `HYPERLINK "url"` field (internal `\l` bookmarks give none).
fn hyperlink_target(instr: &str) -> Option<String> {
    let t = instr.trim();
    let rest = t.strip_prefix("HYPERLINK").or_else(|| t.strip_prefix("hyperlink"))?.trim();
    if rest.starts_with("\\l") {
        return None;
    }
    let url = if let Some(r) = rest.strip_prefix('"') { r.split('"').next().unwrap_or("") } else { rest.split_whitespace().next().unwrap_or("") };
    (!url.is_empty()).then(|| url.to_string())
}

/// A length in a VML style (`width:120pt;height:3in`) in points.
fn css_length(style: &str, key: &str) -> Option<f32> {
    for part in style.split(';') {
        let (k, v) = part.split_once(':')?;
        if k.trim().eq_ignore_ascii_case(key) {
            let v = v.trim();
            let num: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
            let n: f32 = num.parse().ok()?;
            let unit = &v[num.len()..];
            return Some(match unit.trim() {
                "in" => n * 72.0,
                "cm" => n * 72.0 / 2.54,
                "mm" => n * 72.0 / 25.4,
                "px" => n * 0.75,
                "emu" => n / 12700.0,
                _ => n,
            });
        }
    }
    None
}

/// Text of paragraphs: notes, comments, headers and footers (with `{page}` and `{pages}` for
/// page number fields when `hf`).
fn plain_text(container: &El, hf: bool, sep: &str) -> String {
    let mut paras = vec![];
    let mut ps = vec![];
    container.find_all("w:p", &mut ps);
    for p in ps {
        let mut s = String::new();
        let mut fields: Vec<(String, bool)> = vec![];
        walk_text(p, hf, &mut s, &mut fields);
        paras.push(s);
    }
    let joined = paras.join(sep);
    if hf {
        let t = joined.replace('\t', " ");
        t.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        joined.trim().to_string()
    }
}

fn field_placeholder(instr: &str) -> Option<&'static str> {
    let w = instr.split_whitespace().next()?.to_ascii_uppercase();
    match w.as_str() {
        "PAGE" => Some("{page}"),
        "NUMPAGES" | "SECTIONPAGES" => Some("{pages}"),
        "TITLE" => Some("{title}"),
        _ => None,
    }
}

fn walk_text(el: &El, hf: bool, s: &mut String, fields: &mut Vec<(String, bool)>) {
    for e in el.elements() {
        match e.name.as_str() {
            "w:t" => {
                // Inside a page field's result in a header: the placeholder stands for it.
                let hidden = fields.iter().any(|(instr, in_result)| !in_result || (hf && field_placeholder(instr).is_some()));
                if !hidden {
                    s.push_str(&e.own_text());
                }
            }
            "w:tab" | "w:ptab" => s.push('\t'),
            "w:br" | "w:cr" => s.push(' '),
            "w:instrText" => {
                if let Some((instr, false)) = fields.last_mut() {
                    instr.push_str(&e.own_text());
                }
            }
            "w:fldChar" => match e.attr("w:fldCharType") {
                Some("begin") => fields.push((String::new(), false)),
                Some("separate") => {
                    if let Some(f) = fields.last_mut() {
                        f.1 = true;
                        if hf && let Some(ph) = field_placeholder(&f.0) {
                            s.push_str(ph);
                        }
                    }
                }
                Some("end") => {
                    if let Some((instr, in_result)) = fields.pop()
                        && !in_result
                        && hf
                        && let Some(ph) = field_placeholder(&instr)
                    {
                        s.push_str(ph);
                    }
                }
                _ => {}
            },
            "w:fldSimple" => {
                if hf && let Some(ph) = field_placeholder(e.attr("w:instr").unwrap_or("")) {
                    s.push_str(ph);
                } else {
                    walk_text(e, hf, s, fields);
                }
            }
            "w:delText" | "w:footnoteRef" | "w:endnoteRef" | "w:annotationRef" | "w:rPr" | "w:pPr" => {}
            "w:del" => {}
            _ => walk_text(e, hf, s, fields),
        }
    }
}

fn twips(v: Option<&str>) -> Option<f32> {
    v.and_then(|x| x.parse::<f32>().ok()).map(|t| t / 20.0)
}

fn setup_of(sect: &El) -> PageSetup {
    let mut s = PageSetup::a4();
    s.footer.clear();
    if let Some(sz) = sect.child("w:pgSz") {
        if let (Some(w), Some(h)) = (twips(sz.attr("w:w")), twips(sz.attr("w:h"))) {
            s.width = w;
            s.height = h;
        }
        if sz.attr("w:orient") == Some("landscape") && s.width < s.height {
            std::mem::swap(&mut s.width, &mut s.height);
        }
    }
    if let Some(m) = sect.child("w:pgMar") {
        s.margin_top = twips(m.attr("w:top")).map(f32::abs).unwrap_or(s.margin_top);
        s.margin_bottom = twips(m.attr("w:bottom")).map(f32::abs).unwrap_or(s.margin_bottom);
        s.margin_left = twips(m.attr("w:left").or(m.attr("w:start"))).unwrap_or(s.margin_left);
        s.margin_right = twips(m.attr("w:right").or(m.attr("w:end"))).unwrap_or(s.margin_right);
    }
    s.different_first = sect.child("w:titlePg").is_some_and(on);
    s
}

pub(crate) fn parse_date(s: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = s?.trim();
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&chrono::Utc))
        .ok()
        .or_else(|| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok().map(|n| n.and_utc()))
        .or_else(|| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|n| n.and_utc()))
}

/// A page name from a file title (names can't hold formula characters).
pub(crate) fn page_name(title: &str) -> String {
    let n: String = title.chars().map(|c| if matches!(c, '!' | '\'' | '[' | ']' | ':' | '*' | '?' | '/' | '\\') { ' ' } else { c }).take(80).collect();
    let n = n.trim().to_string();
    if n.is_empty() { "Document".into() } else { n }
}

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    let mut pkg = Package::open(bytes, "Word document")?;
    // The main part: from the package relationships (usually word/document.xml).
    let root_rels = read_rels(&mut pkg, "");
    let main = root_rels.values().find(|r| r.kind.ends_with("/officeDocument")).map(|r| r.target.clone()).unwrap_or_else(|| "word/document.xml".into());
    let doc_xml = pkg.xml(&main).ok_or_else(|| if pkg.has(&main) { "The Word document's text is damaged.".to_string() } else { "This isn't a Word document (no word/document.xml).".to_string() })?;
    let body = doc_xml.child("w:body").ok_or("The Word document has no body.")?.clone();
    let rels = read_rels(&mut pkg, &main);
    let part = |kind: &str| rels.values().find(|r| r.kind.ends_with(kind) && !r.external).map(|r| r.target.clone());

    let mut conv = Conv {
        pkg,
        styles: HashMap::new(),
        default_para: None,
        defaults: RProps::default(),
        numbering: Numbering::default(),
        theme: Theme { major: "Calibri".into(), minor: "Calibri".into() },
        doc: Document::empty(title),
        media: HashMap::new(),
        warnings: vec![],
        warned: HashSet::new(),
        comments: HashMap::new(),
        open_comments: vec![],
        commented: HashSet::new(),
        footnotes: HashMap::new(),
        endnotes: HashMap::new(),
        sections: vec![],
        text_width: 451.0,
    };

    // Theme fonts.
    if let Some(t) = part("/theme").and_then(|p| conv.pkg.xml(&p)) {
        if let Some(f) = t.find("a:majorFont").and_then(|f| f.child("a:latin")).and_then(|l| l.attr("typeface")) {
            conv.theme.major = f.to_string();
        }
        if let Some(f) = t.find("a:minorFont").and_then(|f| f.child("a:latin")).and_then(|l| l.attr("typeface")) {
            conv.theme.minor = f.to_string();
        }
    }

    // Styles.
    if let Some(st) = part("/styles").and_then(|p| conv.pkg.xml(&p)) {
        if let Some(rpr) = st.child("w:docDefaults").and_then(|d| d.child("w:rPrDefault")).and_then(|r| r.child("w:rPr")) {
            conv.defaults = RProps::read(rpr, &conv.theme);
        }
        for s in st.children("w:style") {
            let Some(id) = s.attr("w:styleId") else { continue };
            let ppr = s.child("w:pPr");
            let def = StyleDef {
                name: s.child_attr("w:name", "w:val").unwrap_or(id).to_string(),
                kind: s.attr("w:type").unwrap_or("paragraph").to_string(),
                based_on: s.child_attr("w:basedOn", "w:val").map(str::to_string),
                outline: ppr.and_then(|p| p.child_attr("w:outlineLvl", "w:val")).and_then(|v| v.parse().ok()).filter(|l: &u8| *l < 9),
                num: ppr.and_then(|p| p.child("w:numPr")).and_then(|n| n.child_attr("w:numId", "w:val").map(|id| (id.to_string(), n.child_attr("w:ilvl", "w:val").and_then(|v| v.parse().ok())))),
                jc: ppr.and_then(|p| p.child_attr("w:jc", "w:val")).map(align_of),
                rpr: s.child("w:rPr").cloned(),
            };
            if def.kind == "paragraph" && s.attr("w:default").is_some_and(|v| v == "1" || v == "true") {
                conv.default_para = Some(id.to_string());
            }
            conv.styles.insert(id.to_string(), def);
        }
    }

    // Numbering.
    if let Some(n) = part("/numbering").and_then(|p| conv.pkg.xml(&p)) {
        for a in n.children("w:abstractNum") {
            let Some(id) = a.attr("w:abstractNumId") else { continue };
            conv.numbering.abstracts.insert(id.to_string(), levels_of(a));
            if let Some(link) = a.child_attr("w:numStyleLink", "w:val") {
                conv.numbering.style_links.insert(id.to_string(), link.to_string());
            }
        }
        for num in n.children("w:num") {
            let (Some(id), Some(abs)) = (num.attr("w:numId"), num.child_attr("w:abstractNumId", "w:val")) else { continue };
            let mut overrides = HashMap::new();
            for o in num.children("w:lvlOverride") {
                if let Some(l) = o.child("w:lvl") {
                    let i = o.attr("w:ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                    overrides.insert(i, Level { fmt: l.child_attr("w:numFmt", "w:val").unwrap_or("decimal").into(), text: l.child_attr("w:lvlText", "w:val").unwrap_or("").into() });
                }
            }
            conv.numbering.nums.insert(id.to_string(), (abs.to_string(), overrides));
        }
    }

    // Notes.
    for (kind, endnote) in [("/footnotes", false), ("/endnotes", true)] {
        if let Some(x) = part(kind).and_then(|p| conv.pkg.xml(&p)) {
            for n in x.elements() {
                if n.attr("w:type").is_some_and(|t| t != "normal") {
                    continue;
                }
                let Some(id) = n.attr("w:id") else { continue };
                let text = plain_text(n, false, " ");
                if endnote {
                    conv.endnotes.insert(id.to_string(), text);
                } else {
                    conv.footnotes.insert(id.to_string(), text);
                }
            }
        }
    }

    // Comments, with replies and "done" from commentsExtended.
    // (w:id, comment, its last paragraph's id, the parent's paragraph id)
    let mut comments: Vec<(i64, Comment, Option<String>, Option<String>)> = vec![];
    if let Some(x) = part("/comments").and_then(|p| conv.pkg.xml(&p)) {
        for c in x.children("w:comment") {
            let Some(wid) = c.attr("w:id") else { continue };
            let id = Id::new();
            conv.comments.insert(wid.to_string(), id.clone());
            let comment = Comment {
                id,
                author: c.attr("w:author").unwrap_or("Unknown").to_string(),
                text: plain_text(c, false, "\n"),
                at: parse_date(c.attr("w:date")).unwrap_or_else(chrono::Utc::now),
                resolved: false,
                replies: vec![],
            };
            let pid = c.children("w:p").last().and_then(|p| p.attr("w14:paraId")).map(|s| s.to_ascii_uppercase());
            comments.push((wid.parse().unwrap_or(0), comment, pid, None));
        }
    }
    if let Some(x) = part("/commentsExtended").and_then(|p| conv.pkg.xml(&p)) {
        for ex in x.elements() {
            let Some(pid) = ex.attr("w15:paraId").map(|s| s.to_ascii_uppercase()) else { continue };
            let Some(c) = comments.iter_mut().find(|c| c.2.as_deref() == Some(pid.as_str())) else { continue };
            if ex.attr("w15:done") == Some("1") {
                c.1.resolved = true;
            }
            c.3 = ex.attr("w15:paraIdParent").map(|s| s.to_ascii_uppercase());
        }
    }
    comments.sort_by_key(|c| c.0);

    // Setup from the first section.
    let mut sects = vec![];
    body.find_all("w:sectPr", &mut sects);
    if let Some(first) = sects.first() {
        conv.text_width = setup_of(first).text_width();
    }
    let rels_body = &rels;
    let mut blocks = conv.blocks(&body, rels_body);
    // The section break that ends the last section isn't a page break.
    if matches!(blocks.last(), Some(Block::PageBreak { .. })) {
        blocks.pop();
    }
    let had_sections = conv.sections.len();
    // A picture followed by a caption paragraph: the caption is the picture's.
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
    let mut setup = conv.sections.first().map(setup_of).unwrap_or_else(|| {
        let mut s = PageSetup::a4();
        s.footer.clear();
        s
    });
    if had_sections > 1 {
        let all_same = conv.sections.iter().all(|s| setup_of(s) == setup);
        if !all_same {
            conv.warn("sections", "The document had sections with different page sizes or margins: the first section's are used for the whole document.");
        }
    }
    // Header and footer: the first default ones of any section.
    let hf_ref = |which: &str| -> Option<String> {
        conv.sections.iter().find_map(|s| s.children(which).find(|r| r.attr("w:type").is_none_or(|t| t == "default")).and_then(|r| r.attr("r:id")).map(str::to_string))
    };
    let (h_id, f_id) = (hf_ref("w:headerReference"), hf_ref("w:footerReference"));
    for (rid, header) in [(h_id, true), (f_id, false)] {
        let Some(rid) = rid else { continue };
        let Some(target) = rels_body.get(&rid).map(|r| r.target.clone()) else { continue };
        if let Some(x) = conv.pkg.xml(&target) {
            let text = plain_text(&x, true, " ");
            let mut hfs = vec![];
            x.find_all("a:blip", &mut hfs);
            if !hfs.is_empty() {
                conv.warn("hf-picture", "Pictures in headers and footers are left out.");
            }
            if header {
                setup.header = text;
            } else {
                setup.footer = text;
            }
        }
    }

    let track = part("/settings").and_then(|p| conv.pkg.xml(&p)).is_some_and(|s| s.child("w:trackRevisions").is_some_and(on));

    // Replies fold into their comment.
    let mut top: Vec<Comment> = vec![];
    let mut pid_to_top: HashMap<String, usize> = HashMap::new();
    let mut replies: Vec<(String, Comment)> = vec![];
    for (_, c, pid, parent) in comments {
        match parent {
            Some(par) => replies.push((par, c)),
            None => {
                if let Some(p) = pid {
                    pid_to_top.insert(p, top.len());
                }
                top.push(c);
            }
        }
    }
    let mut remap: HashMap<Id, Id> = HashMap::new();
    for (par, c) in replies {
        match pid_to_top.get(&par) {
            Some(&i) => {
                remap.insert(c.id.clone(), top[i].id.clone());
                top[i].replies.push(Reply { author: c.author, text: c.text, at: c.at });
            }
            None => top.push(c),
        }
    }
    if !remap.is_empty() {
        for b in &mut merged {
            let fix = |runs: &mut Vec<Run>| {
                for r in runs {
                    if let Some(c) = &r.style.comment
                        && let Some(to) = remap.get(c)
                    {
                        r.style.comment = Some(to.clone());
                    }
                }
            };
            match b {
                Block::Paragraph(p) => fix(&mut p.runs),
                Block::Table(t) => t.rows.iter_mut().flatten().for_each(|c| fix(&mut c.runs)),
                _ => {}
            }
        }
        for p in merged.iter_mut().filter_map(Block::para_mut) {
            p.normalize();
        }
    }

    // Document properties.
    if let Some(core) = root_rels.values().find(|r| r.kind.ends_with("/core-properties")).map(|r| r.target.clone()).and_then(|p| conv.pkg.xml(&p)) {
        if let Some(a) = core.child("dc:creator").map(|e| e.own_text()).filter(|s| !s.trim().is_empty()) {
            conv.doc.meta.author = a.trim().to_string();
        }
        if let Some(d) = parse_date(core.child("dcterms:created").map(|e| e.own_text()).as_deref()) {
            conv.doc.meta.created = Some(d);
        }
        if let Some(d) = parse_date(core.child("dcterms:modified").map(|e| e.own_text()).as_deref()) {
            conv.doc.meta.modified = Some(d);
        }
    }

    let mut doc = conv.doc;
    let i = doc.add_page(PageKind::Doc, Some(&page_name(title)), None).map_err(|e| e.0)?;
    let t = doc.page_mut(i).doc_mut().unwrap();
    t.blocks = merged.into_iter().collect();
    folio_core::text::ensure_nonempty(&mut t.blocks);
    t.setup = setup;
    t.comments = top;
    t.track_changes = track;
    Ok(Imported { doc, warnings: conv.warnings, format: "docx" })
}
