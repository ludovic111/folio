//! One paragraph laid out at a width.

use folio_core::Paragraph;
use folio_core::text::Family;

use crate::fonts::{Face, Fonts};
use crate::Rgba;

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
}

#[derive(Clone, Debug, PartialEq)]
pub enum DecoKind {
    Underline,
    Strike,
    /// A background behind the text (highlight colour).
    Highlight,
    /// Text with a comment on it (a tinted background).
    Comment(String),
    /// A tracked insertion (underlined in the author's colour) or deletion (struck).
    Inserted,
    Deleted,
    /// Text that is a link (also underlined).
    Link(String),
}

/// A line or box drawn with the text: from x0 to x1 on the line, at the given y (baseline-relative
/// for underline and strike), in a colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Deco {
    pub kind: DecoKind,
    pub x0: f32,
    pub x1: f32,
    pub color: Rgba,
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
    /// A checkbox to draw instead of glyphs: (x, size, checked).
    pub checkbox: Option<(f32, f32, bool)>,
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
}

impl ParaLayout {
    /// The line holding a caret offset (the end of a line belongs to the next one, except at
    /// the paragraph's end) and its x.
    pub fn caret(&self, offset: usize) -> (usize, f32) {
        unimplemented_caret(self, offset)
    }

    /// The caret offset nearest to a point (paragraph coordinates).
    pub fn hit(&self, x: f32, y: f32) -> usize {
        unimplemented_hit(self, x, y)
    }

    /// Rectangles `(x, y, w, h)` covering offsets `from..to` (selection).
    pub fn rects(&self, from: usize, to: usize) -> Vec<(f32, f32, f32, f32)> {
        let mut out = vec![];
        for l in &self.lines {
            let a = from.max(l.start);
            let b = to.min(l.end);
            if a > b || (a == b && from != to) {
                continue;
            }
            let xa = x_at(l, a);
            let xb = x_at(l, b);
            let extra = if b == l.end && to > l.end { 4.0 } else { 0.0 };
            out.push((xa, l.y, (xb - xa).max(0.0) + extra, l.height));
        }
        out
    }
}

fn x_at(l: &Line, offset: usize) -> f32 {
    l.carets.iter().find(|(o, _)| *o >= offset).or(l.carets.last()).map(|c| c.1).unwrap_or(0.0)
}

fn unimplemented_caret(p: &ParaLayout, offset: usize) -> (usize, f32) {
    if p.lines.is_empty() {
        return (0, p.indent);
    }
    let last = p.lines.len() - 1;
    for (i, l) in p.lines.iter().enumerate() {
        if offset < l.end || i == last || (offset == l.end && l.end == l.start) {
            return (i, x_at(l, offset));
        }
    }
    (last, x_at(&p.lines[last], offset))
}

fn unimplemented_hit(p: &ParaLayout, x: f32, y: f32) -> usize {
    let Some(line) = p.lines.iter().find(|l| y < l.y + l.height).or(p.lines.last()) else { return 0 };
    let mut best = (f32::MAX, line.start);
    for (o, cx) in &line.carets {
        let d = (cx - x).abs();
        if d < best.0 {
            best = (d, *o);
        }
    }
    best.1
}

/// Lays a paragraph out at `width` points.
pub fn layout_paragraph(fonts: &mut Fonts, p: &Paragraph, width: f32, ctx: &ParaCtx) -> ParaLayout {
    crate::fonts::layout(fonts, p, width, ctx)
}
