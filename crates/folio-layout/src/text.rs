//! One paragraph laid out at a width.
//!
//! The paragraph's runs are shaped with cosmic-text (one `ShapeLine` per hard line, each run
//! its own span: family, weight, slant, size) and broken at word boundaries; alignment,
//! justification, line heights (the tallest run on each line), superscripts, list markers,
//! footnote numbers, decorations and caret positions are worked out here. Results are cached
//! in [`Fonts`] by content, so laying a long document out again after one keystroke only
//! reshapes the paragraph that changed.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use cosmic_text::{Attrs, AttrsList, Hinting, Metrics, Shaping, ShapeLine, Style, Weight, Wrap};
use folio_core::Paragraph;
use folio_core::text::{Align, Family, ListKind, ParaStyle, RunStyle};

use crate::fonts::{Face, Fonts};
use crate::{Rgba, parse_hex};

/// How a paragraph is set besides its own style.
#[derive(Clone, Debug, PartialEq)]
pub struct ParaCtx {
    /// Multiplies every size: 1 on document pages; on slides, the shape's text size over 11 pt.
    pub scale: f32,
    /// Text colour when a run has none (the theme's on slides; black on paper).
    pub color: Rgba,
    /// The number shown before a numbered list item (1-based), counted by the caller.
    pub number: Option<usize>,
    /// Families replacing the style's on slides: (headings and titles, everything else).
    pub families: Option<(Family, Family)>,
    /// Footnote numbers for notes in this paragraph, in order (the caller counts them across the page).
    pub first_note: u32,
}

impl Default for ParaCtx {
    fn default() -> Self {
        ParaCtx { scale: 1.0, color: [10, 10, 10, 255], number: None, families: None, first_note: 1 }
    }
}

/// One glyph to paint: its face (index into [`ParaLayout::faces`]), id, position (x from the
/// paragraph's left, y the baseline from the paragraph's top), size in points and colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub face: u16,
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: Rgba,
    /// The text this glyph shows: a byte range in [`ParaLayout::text`] (in [`Marker::text`]
    /// for marker glyphs), for exports that keep text copyable.
    pub cluster: (u32, u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DecoKind {
    Underline,
    Strike,
    /// A background behind the text (highlight colour).
    Highlight,
    /// Text with a comment on it (a tinted background); the comment's id.
    Comment(String),
    /// A tracked insertion (underlined in the author's colour) or deletion (struck).
    Inserted,
    Deleted,
    /// Text that is a link (also underlined).
    Link(String),
}

/// A line or box drawn with the text: the rectangle `x0..x1` × `y..y + h` in paragraph
/// coordinates, in a colour. Underlines, strikes, tracked changes and links are thin strokes;
/// highlights and comments cover the line's height (paint them before the glyphs).
#[derive(Clone, Debug, PartialEq)]
pub struct Deco {
    pub kind: DecoKind,
    pub x0: f32,
    pub x1: f32,
    pub color: Rgba,
    /// Top of the rectangle from the paragraph's top.
    pub y: f32,
    /// Its height (a stroke's thickness).
    pub h: f32,
}

impl Deco {
    /// Backgrounds (highlight, comment) go under the glyphs; the rest over them.
    pub fn is_background(&self) -> bool {
        matches!(self.kind, DecoKind::Highlight | DecoKind::Comment(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Top of the line from the paragraph's top.
    pub y: f32,
    pub height: f32,
    /// Baseline from the paragraph's top.
    pub baseline: f32,
    /// Characters (offsets in the paragraph) this line holds: `start..end`.
    pub start: usize,
    pub end: usize,
    pub glyphs: Vec<Glyph>,
    pub decos: Vec<Deco>,
    /// The x of every caret position on the line, `(char offset, x)`, offsets ascending from
    /// `start` to `end` (inclusive).
    pub carets: Vec<(usize, f32)>,
}

/// A list marker (bullet, number, checkbox) drawn in the indent before the first line.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub glyphs: Vec<Glyph>,
    /// A checkbox to draw instead of glyphs: (x, size, checked). It is a square of side `size`
    /// whose bottom sits on the first line's baseline (`y = baseline - size`).
    pub checkbox: Option<(f32, f32, bool)>,
    /// The marker's text ("•", "3."), which its glyphs' clusters index.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParaLayout {
    pub lines: Vec<Line>,
    /// Height of the lines (spacing before and after not included).
    pub height: f32,
    pub space_before: f32,
    pub space_after: f32,
    /// The paragraph's left indent (lists), where its lines start.
    pub indent: f32,
    pub marker: Option<Marker>,
    /// Faces the glyphs use.
    pub faces: Vec<Face>,
    /// Footnotes in this paragraph: (char offset after the anchor, number, note text).
    pub notes: Vec<(usize, u32, String)>,
    /// The width it was laid out at.
    pub width: f32,
    /// The text that was shaped (the paragraph's, with footnote numbers after their anchors):
    /// glyph clusters index it.
    pub text: String,
}

impl ParaLayout {
    /// The line holding a caret offset (the end of a line belongs to the next one, except at
    /// the paragraph's end) and its x.
    pub fn caret(&self, offset: usize) -> (usize, f32) {
        if self.lines.is_empty() {
            return (0, self.indent);
        }
        let last = self.lines.len() - 1;
        let offset = offset.min(self.lines[last].end);
        for (i, l) in self.lines.iter().enumerate() {
            if offset < l.end || i == last {
                return (i, x_at(l, offset));
            }
        }
        (last, x_at(&self.lines[last], offset))
    }

    /// The caret offset nearest to a point (paragraph coordinates).
    pub fn hit(&self, x: f32, y: f32) -> usize {
        let Some(i) = self.lines.iter().position(|l| y < l.y + l.height).or(self.lines.len().checked_sub(1)) else { return 0 };
        let line = &self.lines[i];
        let last = i + 1 == self.lines.len();
        let mut best = (f32::MAX, line.start);
        for (o, cx) in &line.carets {
            // The end of a line that isn't the last belongs to the next line.
            if !last && *o == line.end && line.end > line.start {
                continue;
            }
            let d = (cx - x).abs();
            if d < best.0 {
                best = (d, *o);
            }
        }
        best.1
    }

    /// Rectangles `(x, y, w, h)` covering offsets `from..to` (selection). A selection running
    /// past a line's end shows a little extra width there (the line break is selected).
    pub fn rects(&self, from: usize, to: usize) -> Vec<(f32, f32, f32, f32)> {
        let (from, to) = (from.min(to), from.max(to));
        let mut out = vec![];
        if from == to {
            let (i, x) = self.caret(from);
            if let Some(l) = self.lines.get(i) {
                out.push((x, l.y, 0.0, l.height));
            }
            return out;
        }
        let n = self.lines.len();
        for (i, l) in self.lines.iter().enumerate() {
            let a = from.max(l.start);
            let b = to.min(l.end);
            if a >= b && !(a == b && to > l.end && a == l.end && l.start == l.end) {
                continue;
            }
            let xa = x_at(l, a);
            let xb = x_at(l, b);
            let (x0, x1) = (xa.min(xb), xa.max(xb));
            let breaks = i + 1 < n && to > l.end;
            let extra = if breaks || (i + 1 == n && to > l.end) { 4.0 } else { 0.0 };
            out.push((x0, l.y, x1 - x0 + extra, l.height));
        }
        out
    }

    /// Offsets of the start and end of the line holding `offset` (Home / End).
    pub fn line_bounds(&self, offset: usize) -> (usize, usize) {
        let (i, _) = self.caret(offset);
        match self.lines.get(i) {
            Some(l) => {
                let last = i + 1 == self.lines.len();
                (l.start, if last { l.end } else { l.end.saturating_sub(1).max(l.start) })
            }
            None => (0, 0),
        }
    }

    /// The offset on the line above (`-1`) or below (`1`) nearest to `x`, or `None` past the
    /// first or last line.
    pub fn vertical(&self, offset: usize, dir: i32, x: f32) -> Option<usize> {
        let (i, _) = self.caret(offset);
        let j = i as i64 + dir as i64;
        if j < 0 || j as usize >= self.lines.len() {
            return None;
        }
        let l = &self.lines[j as usize];
        Some(self.hit(x, l.y + l.height / 2.0))
    }

    /// Height with the spacing before and after.
    pub fn outer_height(&self) -> f32 {
        self.space_before + self.height + self.space_after
    }
}

fn x_at(l: &Line, offset: usize) -> f32 {
    if l.carets.is_empty() {
        return 0.0;
    }
    let i = offset.saturating_sub(l.start).min(l.carets.len() - 1);
    match l.carets.get(i) {
        Some((o, x)) if *o == offset => *x,
        _ => l.carets.iter().find(|(o, _)| *o >= offset).or(l.carets.last()).map(|c| c.1).unwrap_or(0.0),
    }
}

/// Lays a paragraph out at `width` points.
pub fn layout_paragraph(fonts: &mut Fonts, p: &Paragraph, width: f32, ctx: &ParaCtx) -> ParaLayout {
    (*layout_paragraph_cached(fonts, p, width, ctx)).clone()
}

/// [`layout_paragraph`], shared: the same paragraph at the same width and context comes from
/// the cache without being shaped again.
pub fn layout_paragraph_cached(fonts: &mut Fonts, p: &Paragraph, width: f32, ctx: &ParaCtx) -> Arc<ParaLayout> {
    let key = para_key(p, width, ctx);
    if let Some(l) = fonts.cache.get(key) {
        return l;
    }
    let l = Arc::new(layout_uncached(fonts, p, width, ctx));
    fonts.cache.put(key, l.clone());
    l
}

// ---- cache --------------------------------------------------------------------------------

/// Paragraph layouts by content hash. Two generations: entries not used since the previous
/// sweep go when the current generation fills up.
#[derive(Default)]
pub(crate) struct LayoutCache {
    cur: HashMap<u64, Arc<ParaLayout>>,
    old: HashMap<u64, Arc<ParaLayout>>,
}

const CACHE_GEN: usize = 6000;

impl LayoutCache {
    fn get(&mut self, key: u64) -> Option<Arc<ParaLayout>> {
        if let Some(l) = self.cur.get(&key) {
            return Some(l.clone());
        }
        let l = self.old.remove(&key)?;
        self.put(key, l.clone());
        Some(l)
    }

    fn put(&mut self, key: u64, l: Arc<ParaLayout>) {
        if self.cur.len() >= CACHE_GEN {
            self.old = std::mem::take(&mut self.cur);
        }
        self.cur.insert(key, l);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.cur.len() + self.old.len()
    }
}

fn hash_f32<H: Hasher>(h: &mut H, v: f32) {
    v.to_bits().hash(h);
}

fn hash_run_style<H: Hasher>(h: &mut H, s: &RunStyle) {
    (s.bold, s.italic, s.underline, s.strike, s.code, s.superscript, s.subscript).hash(h);
    s.color.hash(h);
    s.highlight.hash(h);
    s.link.hash(h);
    s.size.map(f32::to_bits).hash(h);
    s.font.hash(h);
    s.note.hash(h);
    s.comment.as_ref().map(|c| c.0.as_str()).hash(h);
    s.inserted.hash(h);
    s.deleted.hash(h);
}

fn para_key(p: &Paragraph, width: f32, ctx: &ParaCtx) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    p.style.hash(&mut h);
    p.align.hash(&mut h);
    p.list.hash(&mut h);
    p.level.hash(&mut h);
    p.checked.hash(&mut h);
    p.runs.len().hash(&mut h);
    for r in &p.runs {
        r.text.hash(&mut h);
        hash_run_style(&mut h, &r.style);
    }
    hash_f32(&mut h, width);
    hash_f32(&mut h, ctx.scale);
    ctx.color.hash(&mut h);
    ctx.number.hash(&mut h);
    ctx.families.hash(&mut h);
    ctx.first_note.hash(&mut h);
    h.finish()
}

// ---- layout -------------------------------------------------------------------------------

/// What one span of shaped text looks like.
#[derive(Clone, Debug)]
struct Span {
    /// Footnote numbers: drawn but not part of the paragraph's text.
    virt: bool,
    /// The run's size before super/subscript shrinking (line heights use it).
    nominal: f32,
    size: f32,
    /// Baseline shift upwards.
    shift: f32,
    color: Rgba,
    family: String,
    weight: u16,
    italic: bool,
    underline: bool,
    strike: bool,
    highlight: Option<Rgba>,
    comment: Option<String>,
    inserted: Option<Rgba>,
    deleted: Option<Rgba>,
    link: Option<String>,
    code: bool,
}

/// Tracked-change colours by author.
fn author_color(name: &str) -> Rgba {
    const P: [Rgba; 6] = [[29, 78, 216, 255], [185, 28, 28, 255], [21, 128, 61, 255], [126, 34, 206, 255], [194, 65, 12, 255], [14, 116, 144, 255]];
    let h = name.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    P[(h % P.len() as u32) as usize]
}

pub(crate) const LINK_COLOR: Rgba = [29, 78, 216, 255];
pub(crate) const COMMENT_TINT: Rgba = [250, 204, 21, 90];
const CODE_TINT: Rgba = [0, 0, 0, 18];

/// The family a paragraph style uses in a context.
pub(crate) fn style_family(style: ParaStyle, ctx: &ParaCtx) -> Family {
    let spec = style.spec();
    match ctx.families {
        Some((head, body)) => match style {
            ParaStyle::Title | ParaStyle::Subtitle | ParaStyle::Heading1 | ParaStyle::Heading2 | ParaStyle::Heading3 => head,
            ParaStyle::Code => Family::Mono,
            _ => body,
        },
        None => spec.family,
    }
}

fn muted(c: Rgba) -> Rgba {
    [c[0], c[1], c[2], (c[3] as f32 * 0.62) as u8]
}

fn make_span(fonts: &mut Fonts, p: &Paragraph, s: &RunStyle, ctx: &ParaCtx, virt: bool) -> Span {
    let spec = p.style.spec();
    let base_family = style_family(p.style, ctx).font_name().to_string();
    let family = if s.code {
        Family::Mono.font_name().to_string()
    } else {
        s.font.as_deref().and_then(|f| fonts.family_name(f)).unwrap_or(base_family)
    };
    let nominal = s.size.filter(|v| *v > 0.0).unwrap_or(spec.size) * ctx.scale;
    let (size, shift) = if s.superscript || virt {
        (nominal * 0.62, nominal * 0.36)
    } else if s.subscript {
        (nominal * 0.62, -nominal * 0.14)
    } else {
        (nominal, 0.0)
    };
    let weight = if s.bold { 700 } else if spec.bold { 600 } else { 400 };
    let mut color = s.color.as_deref().map(|c| parse_hex(c, ctx.color)).unwrap_or(ctx.color);
    if s.link.is_some() && s.color.is_none() {
        color = LINK_COLOR;
    }
    if spec.muted && s.color.is_none() {
        color = muted(color);
    }
    let inserted = s.inserted.as_deref().map(author_color);
    let deleted = s.deleted.as_deref().map(author_color);
    if let Some(c) = inserted.or(deleted) {
        color = c;
    }
    if p.list == Some(ListKind::Check) && p.checked {
        color = muted(color);
    }
    Span {
        virt,
        nominal,
        size,
        shift,
        color,
        family,
        weight,
        italic: s.italic || spec.italic,
        underline: s.underline,
        strike: s.strike || (p.list == Some(ListKind::Check) && p.checked),
        highlight: s.highlight.as_deref().map(|h| parse_hex(h, [255, 235, 59, 255])),
        comment: s.comment.as_ref().map(|c| c.0.clone()),
        inserted,
        deleted,
        link: s.link.clone(),
        code: s.code,
    }
}

fn attrs_for<'a>(sp: &'a Span, meta: usize) -> Attrs<'a> {
    Attrs::new()
        .family(cosmic_text::Family::Name(&sp.family))
        .weight(Weight(sp.weight))
        .style(if sp.italic { Style::Italic } else { Style::Normal })
        .metrics(Metrics::new(sp.size, sp.size * 1.2))
        .metadata(meta)
}

/// One glyph out of cosmic-text, in line coordinates (before alignment).
struct Raw {
    font: cosmic_text::fontdb::ID,
    id: u16,
    x: f32,
    w: f32,
    yoff: f32,
    size: f32,
    span: usize,
    /// Byte range in the paragraph's shaped text.
    b0: usize,
    b1: usize,
    /// Character range in the paragraph (empty for footnote numbers).
    c0: usize,
    c1: usize,
    blank: bool,
    rtl: bool,
}

struct RawLine {
    glyphs: Vec<Raw>,
    start: usize,
    end: usize,
    /// Ends a hard line (no justification).
    last: bool,
    ascent: f32,
    descent: f32,
}

fn layout_uncached(fonts: &mut Fonts, p: &Paragraph, width: f32, ctx: &ParaCtx) -> ParaLayout {
    let l = layout_once(fonts, p, width, ctx);
    // Characters the bundled faces lack: bring in the system's fonts and set it again.
    let missing = l.lines.iter().flat_map(|ln| ln.glyphs.iter()).any(|g| g.id == 0 && l.text.get(g.cluster.0 as usize..g.cluster.1 as usize).is_some_and(|t| !t.trim().is_empty()));
    if missing && fonts.load_system_fonts() {
        return layout_once(fonts, p, width, ctx);
    }
    l
}

fn layout_once(fonts: &mut Fonts, p: &Paragraph, width: f32, ctx: &ParaCtx) -> ParaLayout {
    let spec = p.style.spec();
    let scale = ctx.scale;
    let indent = if p.list.is_some() { (p.level as f32 + 1.0) * 18.0 * scale } else { 0.0 };
    let avail = (width - indent).max(1.0);

    // The shaped text, its spans, and where each byte sits in the paragraph.
    let mut text = String::new();
    let mut spans: Vec<Span> = vec![];
    let mut span_ranges: Vec<(usize, usize, usize)> = vec![]; // (byte start, byte end, span)
    let mut byte_char: Vec<u32> = vec![];
    let mut notes = vec![];
    let mut chars = 0usize;
    let mut note_no = ctx.first_note;
    for r in &p.runs {
        if !r.text.is_empty() {
            let si = spans.len();
            spans.push(make_span(fonts, p, &r.style, ctx, false));
            let b = text.len();
            for ch in r.text.chars() {
                for _ in 0..ch.len_utf8() {
                    byte_char.push(chars as u32);
                }
                chars += 1;
            }
            text.push_str(&r.text);
            span_ranges.push((b, text.len(), si));
        }
        if let Some(note) = &r.style.note {
            let si = spans.len();
            let mut s = r.style.clone();
            s.underline = false;
            s.strike = false;
            s.link = None;
            spans.push(make_span(fonts, p, &s, ctx, true));
            let num = note_no.to_string();
            let b = text.len();
            text.push_str(&num);
            byte_char.extend(std::iter::repeat_n(chars as u32, num.len()));
            span_ranges.push((b, text.len(), si));
            notes.push((chars, note_no, note.clone()));
            note_no += 1;
        }
    }
    // The typing style of an empty paragraph sets its line height.
    let empty_span = {
        let s = p.runs.first().map(|r| r.style.clone()).unwrap_or_default();
        make_span(fonts, p, &s, ctx, false)
    };
    let char_at = |b: usize| -> usize { byte_char.get(b).map(|c| *c as usize).unwrap_or(chars) };

    // Shape each hard line and break it.
    let mut raw_lines: Vec<RawLine> = vec![];
    let mut seg_start = 0usize;
    loop {
        let seg_end = text[seg_start..].find('\n').map(|i| seg_start + i).unwrap_or(text.len());
        let hard = seg_end < text.len();
        let seg = &text[seg_start..seg_end];
        let c_start = char_at(seg_start);
        let c_end = if hard { char_at(seg_end) } else { chars };
        if seg.is_empty() {
            let vm = fonts.vmetrics(&empty_span.family, empty_span.weight, empty_span.italic);
            raw_lines.push(RawLine { glyphs: vec![], start: c_start, end: c_end + hard as usize, last: true, ascent: vm.ascent * empty_span.size, descent: vm.descent * empty_span.size });
        } else {
            let defaults = attrs_for(&empty_span, usize::MAX);
            let mut list = AttrsList::new(&defaults);
            for (b0, b1, si) in &span_ranges {
                let (a, b) = ((*b0).max(seg_start), (*b1).min(seg_end));
                if a < b {
                    list.add_span(a - seg_start..b - seg_start, &attrs_for(&spans[*si], *si));
                }
            }
            let shaped = ShapeLine::new(&mut fonts.sys, seg, &list, Shaping::Advanced, 4);
            let base = empty_span.size.max(1.0);
            let lines = shaped.layout(base, Some(avail), Wrap::WordOrGlyph, Some(cosmic_text::Align::Left), None, Hinting::Disabled);
            let first_new = raw_lines.len();
            for ll in &lines {
                let mut glyphs = Vec::with_capacity(ll.glyphs.len());
                for g in &ll.glyphs {
                    let span = if g.metadata < spans.len() { g.metadata } else { usize::MAX };
                    let b0 = seg_start + g.start;
                    let b1 = seg_start + g.end;
                    let virt = span != usize::MAX && spans[span].virt;
                    let (c0, c1) = if virt { (char_at(b0), char_at(b0)) } else { (char_at(b0), if b1 >= text.len() { chars } else { char_at(b1) }) };
                    let blank = text.get(b0..b1).is_some_and(|t| t.chars().all(char::is_whitespace));
                    glyphs.push(Raw {
                        font: g.font_id,
                        id: g.glyph_id,
                        x: g.x + g.font_size * g.x_offset,
                        w: g.w,
                        yoff: g.font_size * g.y_offset,
                        size: g.font_size,
                        span,
                        b0,
                        b1,
                        c0,
                        c1,
                        blank,
                        rtl: g.level.is_rtl(),
                    });
                }
                let vm = fonts.vmetrics(&empty_span.family, empty_span.weight, empty_span.italic);
                let (asc, desc) = if glyphs.is_empty() { (vm.ascent * empty_span.size, vm.descent * empty_span.size) } else { (ll.max_ascent, ll.max_descent) };
                raw_lines.push(RawLine { glyphs, start: 0, end: 0, last: false, ascent: asc, descent: desc });
            }
            if raw_lines.len() == first_new {
                raw_lines.push(RawLine { glyphs: vec![], start: c_start, end: c_start, last: true, ascent: 0.0, descent: 0.0 });
            }
            // Character ranges: each wrapped line runs to where the next one starts.
            let n = raw_lines.len();
            for i in first_new..n {
                let s = if i == first_new { c_start } else { raw_lines[i].glyphs.iter().filter(|g| g.c1 > g.c0).map(|g| g.c0).min().unwrap_or(c_start) };
                raw_lines[i].start = s;
            }
            for i in first_new..n {
                raw_lines[i].end = if i + 1 < n { raw_lines[i + 1].start.max(raw_lines[i].start) } else { c_end + hard as usize };
                raw_lines[i].last = i + 1 == n;
            }
        }
        if !hard {
            break;
        }
        seg_start = seg_end + 1;
    }

    // Faces used.
    let mut faces: Vec<Face> = vec![];
    let mut face_idx: HashMap<cosmic_text::fontdb::ID, u16> = HashMap::new();
    let mut face_of = |fonts: &mut Fonts, id: cosmic_text::fontdb::ID| -> u16 {
        *face_idx.entry(id).or_insert_with(|| {
            faces.push(fonts.face_of(id));
            (faces.len() - 1) as u16
        })
    };

    // Place lines: alignment, vertical metrics, glyphs, decorations, carets.
    let mut lines = Vec::with_capacity(raw_lines.len());
    let mut y = 0.0f32;
    for rl in raw_lines.iter_mut() {
        // Visible extent (trailing spaces hang past the edge).
        let vis_right = rl.glyphs.iter().filter(|g| !g.blank).map(|g| g.x + g.w).fold(0.0f32, f32::max);
        let vis_left = rl.glyphs.iter().filter(|g| !g.blank).map(|g| g.x).fold(f32::MAX, f32::min).min(vis_right);
        let free = avail - vis_right;
        let mut extra_per_space = 0.0;
        let mut shift = match p.align {
            Align::Left | Align::Justify => 0.0,
            Align::Center => free / 2.0,
            Align::Right => free,
        };
        if rl.glyphs.is_empty() {
            shift = match p.align {
                Align::Center => avail / 2.0,
                Align::Right => avail,
                _ => 0.0,
            };
        }
        if p.align == Align::Justify && !rl.last && free > 0.0 {
            let spaces = rl.glyphs.iter().filter(|g| g.blank && g.x + g.w <= vis_right + 0.01 && g.x >= vis_left).count();
            if spaces > 0 {
                extra_per_space = free / spaces as f32;
            }
        }
        if extra_per_space > 0.0 {
            // Glyphs are in visual order: spread spaces left to right.
            let mut add = 0.0;
            for g in rl.glyphs.iter_mut() {
                g.x += add;
                if g.blank && g.x - add + g.w <= vis_right + 0.01 && g.x - add >= vis_left {
                    g.w += extra_per_space;
                    add += extra_per_space;
                }
            }
        }
        let shift = shift.max(0.0) + indent;
        for g in rl.glyphs.iter_mut() {
            g.x += shift;
        }

        // Height: the tallest run's line height.
        let nominal = rl.glyphs.iter().filter(|g| g.span < spans.len()).map(|g| spans[g.span].nominal).fold(0.0f32, f32::max);
        let nominal = if nominal > 0.0 { nominal } else if rl.glyphs.is_empty() { empty_span.nominal } else { rl.glyphs.iter().map(|g| g.size).fold(0.0, f32::max) };
        let lh = nominal * spec.line;
        let baseline = y + (lh - (rl.ascent + rl.descent)) / 2.0 + rl.ascent;

        let mut glyphs = Vec::with_capacity(rl.glyphs.len());
        for g in &rl.glyphs {
            let sp = spans.get(g.span).unwrap_or(&empty_span);
            if g.blank && g.id == 0 {
                continue;
            }
            let face = face_of(fonts, g.font);
            glyphs.push(Glyph { face, id: g.id, x: g.x, y: baseline - g.yoff - sp.shift, size: g.size, color: sp.color, cluster: (g.b0 as u32, g.b1 as u32) });
        }

        // Decorations, merged over neighbouring glyphs.
        let mut decos: Vec<Deco> = vec![];
        let vis_right_shifted = vis_right + shift + if extra_per_space > 0.0 { free } else { 0.0 };
        for g in &rl.glyphs {
            if g.span >= spans.len() || (g.blank && g.x >= vis_right_shifted - 0.01) {
                continue;
            }
            let sp = &spans[g.span];
            let size = sp.nominal;
            let thick = (size * 0.06).max(0.5);
            let under_y = baseline + size * 0.12;
            let strike_y = baseline - size * 0.27;
            let mut add = |kind: DecoKind, color: Rgba, dy: f32, h: f32| {
                let (x0, x1) = (g.x, g.x + g.w);
                if let Some(d) = decos.iter_mut().rev().find(|d| d.kind == kind && d.color == color && (d.x1 - x0).abs() < 0.5 && d.y == dy) {
                    d.x1 = d.x1.max(x1);
                    d.h = d.h.max(h);
                    return;
                }
                decos.push(Deco { kind, x0, x1, color, y: dy, h });
            };
            if let Some(c) = sp.highlight {
                add(DecoKind::Highlight, c, y, lh);
            }
            if sp.code {
                add(DecoKind::Highlight, CODE_TINT, y, lh);
            }
            if let Some(id) = &sp.comment {
                add(DecoKind::Comment(id.clone()), COMMENT_TINT, y, lh);
            }
            if sp.underline {
                add(DecoKind::Underline, sp.color, under_y, thick);
            }
            if let Some(url) = &sp.link {
                add(DecoKind::Link(url.clone()), sp.color, under_y, thick);
            }
            if sp.strike {
                add(DecoKind::Strike, sp.color, strike_y, thick);
            }
            if let Some(c) = sp.inserted {
                add(DecoKind::Inserted, c, under_y, thick);
            }
            if let Some(c) = sp.deleted {
                add(DecoKind::Deleted, c, strike_y, thick);
            }
        }

        // Carets: every offset from start to end.
        let n = rl.end.saturating_sub(rl.start) + 1;
        let mut xs = vec![f32::NAN; n];
        let mut clusters: Vec<(usize, usize, f32, f32, bool)> = vec![];
        for g in &rl.glyphs {
            if g.c1 <= g.c0 {
                continue;
            }
            match clusters.last_mut() {
                Some(c) if c.0 == g.c0 && c.1 == g.c1 => {
                    c.2 = c.2.min(g.x);
                    c.3 = c.3.max(g.x + g.w);
                }
                _ => clusters.push((g.c0, g.c1, g.x, g.x + g.w, g.rtl)),
            }
        }
        let mut right_of_last = (0usize, f32::NAN);
        for (c0, c1, x0, x1, rtl) in &clusters {
            let k = (c1 - c0) as f32;
            for c in *c0..*c1 {
                if c >= rl.start && c < rl.end {
                    let t = (c - c0) as f32 / k;
                    xs[c - rl.start] = if *rtl { x1 - (x1 - x0) * t } else { x0 + (x1 - x0) * t };
                }
            }
            if *c1 >= right_of_last.0 {
                right_of_last = (*c1, if *rtl { *x0 } else { *x1 });
            }
        }
        if right_of_last.0 >= rl.start && right_of_last.0 < rl.start + n && xs[right_of_last.0 - rl.start].is_nan() {
            xs[right_of_last.0 - rl.start] = right_of_last.1;
        }
        let first_x = if rl.glyphs.is_empty() { shift } else { xs.iter().copied().find(|x| !x.is_nan()).unwrap_or(shift) };
        let mut prev = first_x;
        for x in xs.iter_mut() {
            if x.is_nan() {
                *x = prev;
            }
            prev = *x;
        }
        let carets = xs.into_iter().enumerate().map(|(i, x)| (rl.start + i, x)).collect();

        lines.push(Line { y, height: lh, baseline, start: rl.start, end: rl.end, glyphs, decos, carets });
        y += lh;
    }

    // List marker in the indent, on the first line's baseline.
    let marker = match (p.list, lines.first()) {
        (Some(kind), Some(first)) => {
            let size = empty_span.nominal;
            match kind {
                ListKind::Check => {
                    let s = size * 0.72;
                    Some(Marker { glyphs: vec![], checkbox: Some((indent - s - 6.0 * scale, s, p.checked)), text: if p.checked { "☑".into() } else { "☐".into() } })
                }
                ListKind::Bullet | ListKind::Number => {
                    let mtext = match kind {
                        ListKind::Number => format!("{}.", ctx.number.unwrap_or(1)),
                        _ => ["•", "–", "◦"][p.level as usize % 3].to_string(),
                    };
                    let mut sp = empty_span.clone();
                    sp.size = size;
                    sp.shift = 0.0;
                    sp.weight = if spec.bold { 600 } else { 400 };
                    sp.italic = false;
                    let (gl, w) = shape_simple(fonts, &mtext, &sp);
                    let x0 = match kind {
                        ListKind::Number => indent - 5.0 * scale - w,
                        _ => indent - 13.0 * scale,
                    };
                    let mut glyphs = vec![];
                    for (font, id, x, b0, b1) in gl {
                        let face = face_of(fonts, font);
                        glyphs.push(Glyph { face, id, x: x0 + x, y: first.baseline, size, color: sp.color, cluster: (b0 as u32, b1 as u32) });
                    }
                    Some(Marker { glyphs, checkbox: None, text: mtext })
                }
            }
        }
        _ => None,
    };

    ParaLayout {
        height: y,
        lines,
        space_before: spec.space_before * scale,
        space_after: spec.space_after * scale,
        indent,
        marker,
        faces,
        notes,
        width,
        text,
    }
}

/// Shapes a short single-line text in one span: (font, glyph, x, cluster start, cluster end)
/// and the advance width.
fn shape_simple(fonts: &mut Fonts, text: &str, sp: &Span) -> (Vec<(cosmic_text::fontdb::ID, u16, f32, usize, usize)>, f32) {
    let attrs = attrs_for(sp, 0);
    let list = AttrsList::new(&attrs);
    let shaped = ShapeLine::new(&mut fonts.sys, text, &list, Shaping::Advanced, 4);
    let lines = shaped.layout(sp.size, None, Wrap::None, Some(cosmic_text::Align::Left), None, Hinting::Disabled);
    let mut out = vec![];
    let mut w = 0.0f32;
    for l in &lines {
        for g in &l.glyphs {
            out.push((g.font_id, g.glyph_id, g.x + g.font_size * g.x_offset, g.start, g.end));
            w = w.max(g.x + g.w);
        }
    }
    (out, w)
}

/// Shapes one line of plain text (labels, cells, headers): glyphs from x = 0 on the baseline
/// y = 0, the faces they use, and the advance width.
pub fn shape_label(fonts: &mut Fonts, text: &str, family: &str, weight: u16, italic: bool, size: f32, color: Rgba) -> (Vec<Glyph>, Vec<Face>, f32) {
    let family = fonts.family_name(family).unwrap_or_else(|| Family::Sans.font_name().to_string());
    let sp = Span {
        virt: false,
        nominal: size,
        size,
        shift: 0.0,
        color,
        family,
        weight,
        italic,
        underline: false,
        strike: false,
        highlight: None,
        comment: None,
        inserted: None,
        deleted: None,
        link: None,
        code: false,
    };
    let (gl, w) = shape_simple(fonts, text, &sp);
    let mut faces: Vec<Face> = vec![];
    let mut glyphs = Vec::with_capacity(gl.len());
    for (font, id, x, b0, b1) in gl {
        let f = fonts.face_of(font);
        let idx = match faces.iter().position(|x| *x == f) {
            Some(i) => i,
            None => {
                faces.push(f);
                faces.len() - 1
            }
        };
        glyphs.push(Glyph { face: idx as u16, id, x, y: 0.0, size, color, cluster: (b0 as u32, b1 as u32) });
    }
    (glyphs, faces, w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use folio_core::{Run, RunStyle};

    fn fonts() -> std::sync::MutexGuard<'static, Fonts> {
        static F: std::sync::OnceLock<std::sync::Mutex<Fonts>> = std::sync::OnceLock::new();
        F.get_or_init(|| std::sync::Mutex::new(Fonts::bundled_only())).lock().unwrap_or_else(|e| e.into_inner())
    }

    fn para(text: &str) -> Paragraph {
        Paragraph::new(ParaStyle::Normal, text)
    }

    #[test]
    fn wraps_and_carets() {
        let mut f = fonts();
        let p = para("The quick brown fox jumps over the lazy dog and keeps running far away.");
        let l = layout_paragraph(&mut f, &p, 120.0, &ParaCtx::default());
        assert!(l.lines.len() > 2, "{}", l.lines.len());
        assert_eq!(l.lines[0].start, 0);
        assert_eq!(l.lines.last().unwrap().end, p.len());
        for w in l.lines.windows(2) {
            assert_eq!(w[0].end, w[1].start);
            assert!(w[1].y >= w[0].y + w[0].height - 0.01);
        }
        for line in &l.lines {
            assert_eq!(line.carets.len(), line.end - line.start + 1);
            assert!(line.carets.windows(2).all(|c| c[1].1 >= c[0].1 - 0.01), "carets ascend");
            for g in &line.glyphs {
                assert!(g.x >= -0.01 && g.x <= 120.0 + 0.01, "glyph x {}", g.x);
            }
            let vis = line.glyphs.iter().filter(|g| !l.text[g.cluster.0 as usize..g.cluster.1 as usize].trim().is_empty()).map(|g| g.x).fold(0.0, f32::max);
            assert!(vis < 120.0);
        }
        assert_eq!(l.faces[0].family, "IBM Plex Sans");
        // Lines are 11 pt × 1.4.
        assert!((l.lines[0].height - 15.4).abs() < 0.01);
        // Caret and hit agree.
        for o in 0..=p.len() {
            let (li, x) = l.caret(o);
            let line = &l.lines[li];
            let back = l.hit(x, line.y + 1.0);
            let (li2, x2) = l.caret(back);
            assert_eq!(li2, li, "offset {o}");
            assert!((x2 - x).abs() < 0.01, "offset {o}: {x} vs {x2}");
        }
    }

    #[test]
    fn empty_paragraph_has_a_line() {
        let mut f = fonts();
        let l = layout_paragraph(&mut f, &para(""), 300.0, &ParaCtx::default());
        assert_eq!(l.lines.len(), 1);
        assert!((l.height - 15.4).abs() < 0.01);
        assert_eq!(l.lines[0].carets, vec![(0, 0.0)]);
        let h = Paragraph::new(ParaStyle::Heading1, "");
        let lh = layout_paragraph(&mut f, &h, 300.0, &ParaCtx::default());
        assert!((lh.height - 24.0).abs() < 0.01);
        assert_eq!(lh.space_before, 18.0);
    }

    #[test]
    fn alignment() {
        let mut f = fonts();
        let w = 300.0;
        let right = layout_paragraph(&mut f, &para("Right").align(Align::Right), w, &ParaCtx::default());
        let end = right.lines[0].carets.last().unwrap().1;
        assert!((end - w).abs() < 0.5, "{end}");
        let center = layout_paragraph(&mut f, &para("Mid").align(Align::Center), w, &ParaCtx::default());
        let (a, b) = (center.lines[0].carets[0].1, center.lines[0].carets.last().unwrap().1);
        assert!(((a + b) / 2.0 - w / 2.0).abs() < 0.5);
        let text = "Justified text spreads its spaces so every line but the last one reaches the right edge exactly.";
        let j = layout_paragraph(&mut f, &para(text).align(Align::Justify), 200.0, &ParaCtx::default());
        assert!(j.lines.len() >= 3);
        for (i, line) in j.lines.iter().enumerate() {
            let right = line.glyphs.iter().filter(|g| !j.text[g.cluster.0 as usize..g.cluster.1 as usize].trim().is_empty()).map(|g| g.x).fold(0.0, f32::max);
            if i + 1 < j.lines.len() {
                assert!(right > 185.0 && right <= 200.0, "line {i} right {right}");
            }
        }
    }

    #[test]
    fn runs_styles_and_notes() {
        let mut f = fonts();
        let p = Paragraph::with_runs(
            ParaStyle::Normal,
            vec![
                Run::plain("Plain "),
                Run::bold("bold "),
                Run::styled("big", RunStyle { size: Some(22.0), ..Default::default() }),
                Run::styled(" link", RunStyle { link: Some("https://lsuite.xyz".into()), ..Default::default() }),
                Run::styled(" noted", RunStyle { note: Some("A footnote.".into()), ..Default::default() }),
                Run::styled("2", RunStyle { superscript: true, ..Default::default() }),
                Run::styled(" mark", RunStyle { highlight: Some("#ffee00".into()), underline: true, ..Default::default() }),
            ],
        );
        let ctx = ParaCtx { first_note: 3, ..Default::default() };
        let l = layout_paragraph(&mut f, &p, 400.0, &ctx);
        assert_eq!(l.lines.len(), 1);
        // The 22 pt run sets the line height.
        assert!((l.lines[0].height - 22.0 * 1.4).abs() < 0.01);
        assert!(l.faces.iter().any(|f| f.weight == 700));
        assert_eq!(l.notes.len(), 1);
        assert_eq!(l.notes[0].1, 3);
        assert_eq!(l.notes[0].0, "Plain bold big link noted".chars().count());
        assert!(l.text.contains("noted3"));
        // The footnote number has no caret of its own.
        assert_eq!(l.lines[0].carets.len(), p.len() + 1);
        let kinds: Vec<_> = l.lines[0].decos.iter().map(|d| d.kind.clone()).collect();
        assert!(kinds.contains(&DecoKind::Link("https://lsuite.xyz".into())));
        assert!(kinds.contains(&DecoKind::Highlight));
        assert!(kinds.contains(&DecoKind::Underline));
        // Superscript glyphs sit above the baseline and are smaller.
        let sup = l.lines[0].glyphs.iter().find(|g| &l.text[g.cluster.0 as usize..g.cluster.1 as usize] == "2" && g.size < 10.0).unwrap();
        assert!(sup.y < l.lines[0].baseline - 2.0);
    }

    #[test]
    fn lists_and_breaks() {
        let mut f = fonts();
        let p = para("Item").list(ListKind::Bullet, 1);
        let l = layout_paragraph(&mut f, &p, 300.0, &ParaCtx::default());
        assert_eq!(l.indent, 36.0);
        let m = l.marker.as_ref().unwrap();
        assert_eq!(m.text, "–");
        assert!(m.glyphs[0].x < 36.0 && m.glyphs[0].x > 0.0);
        assert!(l.lines[0].carets[0].1 >= 36.0);
        let n = layout_paragraph(&mut f, &para("Item").list(ListKind::Number, 0), 300.0, &ParaCtx { number: Some(12), ..Default::default() });
        assert_eq!(n.marker.as_ref().unwrap().text, "12.");
        let c = layout_paragraph(&mut f, &Paragraph { checked: true, ..para("Done").list(ListKind::Check, 0) }, 300.0, &ParaCtx::default());
        assert!(c.marker.as_ref().unwrap().checkbox.unwrap().2);
        // A line break inside a table cell.
        let cell = para("one\n\ntwo\tthree");
        let l = layout_paragraph(&mut f, &cell, 300.0, &ParaCtx::default());
        assert_eq!(l.lines.len(), 3);
        assert_eq!((l.lines[0].start, l.lines[0].end), (0, 4));
        assert_eq!((l.lines[1].start, l.lines[1].end), (4, 5));
        assert_eq!(l.lines[2].start, 5);
        assert_eq!(l.caret(3).0, 0);
        assert_eq!(l.caret(4).0, 1);
        assert_eq!(l.caret(5).0, 2);
        // Tabs go to the next stop (every four spaces).
        let tab = l.lines[2].carets[4].1 - l.lines[2].carets[3].1;
        assert!(tab > 0.5, "{tab}");
        let wide = layout_paragraph(&mut f, &para("\tx"), 300.0, &ParaCtx::default());
        assert!(wide.lines[0].carets[1].1 > 8.0, "{:?}", wide.lines[0].carets);
    }

    #[test]
    fn rects_cover_selection() {
        let mut f = fonts();
        let p = para("Select across a few wrapped lines of plain text here");
        let l = layout_paragraph(&mut f, &p, 100.0, &ParaCtx::default());
        let r = l.rects(2, p.len() - 2);
        assert_eq!(r.len(), l.lines.len());
        assert!(r.iter().all(|r| r.2 > 0.0));
        assert_eq!(l.rects(3, 3).len(), 1);
    }

    #[test]
    fn cache_hits() {
        let mut f = fonts();
        let p = para("Cached paragraph");
        let a = layout_paragraph_cached(&mut f, &p, 200.0, &ParaCtx::default());
        let b = layout_paragraph_cached(&mut f, &p, 200.0, &ParaCtx::default());
        assert!(Arc::ptr_eq(&a, &b));
        let c = layout_paragraph_cached(&mut f, &p, 201.0, &ParaCtx::default());
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(f.cache.len() >= 2);
    }
}
