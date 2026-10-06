//! Rich text: the blocks of a document page and the text inside slide shapes.
//!
//! A text flow is a list of [`Block`]s: paragraphs (runs of styled text), tables, images, live
//! charts and page breaks. Positions are [`Pos`]: a block index, a table cell when the caret is
//! inside a table, and an offset in **characters** (Unicode scalar values) inside the paragraph
//! or cell. Atomic blocks (image, chart, page break) have two positions: offset 0 before them
//! and 1 after them.
//!
//! Every edit is a function on a flow (`imbl::Vector<Block>`), so documents and text boxes on
//! slides share one implementation.

use serde::{Deserialize, Serialize};

use crate::Id;

pub type Flow = imbl::Vector<Block>;

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

/// Character formatting. Everything is optional; the paragraph style gives the rest.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunStyle {
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Inline code (the mono face).
    #[serde(skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub superscript: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub subscript: bool,
    /// Text colour `#rrggbb`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Highlight `#rrggbb`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Size in points (the paragraph style's otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// `sans`, `serif`, `mono`, `display`, or a font name (the style's otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// A footnote attached to this text: its text. The number is drawn after the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// A comment on this text (an id in the page's comments).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<Id>,
    /// Tracked change: inserted by this author (shown underlined until accepted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inserted: Option<String>,
    /// Tracked change: deleted by this author (shown struck through until accepted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<String>,
}

/// A change to character formatting: `Some` sets, `None` leaves alone. For the optional
/// strings, `Some("")` clears.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StylePatch {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub code: Option<bool>,
    pub superscript: Option<bool>,
    pub subscript: Option<bool>,
    pub color: Option<String>,
    pub highlight: Option<String>,
    pub link: Option<String>,
    /// 0 clears.
    pub size: Option<f32>,
    pub font: Option<String>,
    pub note: Option<String>,
    pub comment: Option<String>,
}

fn opt_str(target: &mut Option<String>, v: &Option<String>) {
    if let Some(v) = v {
        *target = if v.is_empty() { None } else { Some(v.clone()) };
    }
}

impl StylePatch {
    pub fn apply(&self, s: &mut RunStyle) {
        if let Some(v) = self.bold {
            s.bold = v;
        }
        if let Some(v) = self.italic {
            s.italic = v;
        }
        if let Some(v) = self.underline {
            s.underline = v;
        }
        if let Some(v) = self.strike {
            s.strike = v;
        }
        if let Some(v) = self.code {
            s.code = v;
        }
        if let Some(v) = self.superscript {
            s.superscript = v;
            if v {
                s.subscript = false;
            }
        }
        if let Some(v) = self.subscript {
            s.subscript = v;
            if v {
                s.superscript = false;
            }
        }
        opt_str(&mut s.color, &self.color);
        opt_str(&mut s.highlight, &self.highlight);
        opt_str(&mut s.link, &self.link);
        opt_str(&mut s.font, &self.font);
        opt_str(&mut s.note, &self.note);
        if let Some(c) = &self.comment {
            s.comment = if c.is_empty() { None } else { Some(Id(c.clone())) };
        }
        if let Some(v) = self.size {
            s.size = if v > 0.0 { Some(v) } else { None };
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == StylePatch::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    #[serde(flatten)]
    pub style: RunStyle,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Self {
        Run { text: text.into(), style: RunStyle::default() }
    }

    pub fn styled(text: impl Into<String>, style: RunStyle) -> Self {
        Run { text: text.into(), style }
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Run { text: text.into(), style: RunStyle { bold: true, ..Default::default() } }
    }

    pub fn italic(text: impl Into<String>) -> Self {
        Run { text: text.into(), style: RunStyle { italic: true, ..Default::default() } }
    }
}

/// Paragraph styles, like Word's and Google Docs' list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParaStyle {
    #[default]
    Normal,
    Title,
    Subtitle,
    Heading1,
    Heading2,
    Heading3,
    Quote,
    Code,
    Caption,
}

/// The font family a style or run asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Sans,
    Serif,
    Mono,
    Display,
}

impl Family {
    /// The bundled face (folio-layout registers them).
    pub fn font_name(self) -> &'static str {
        match self {
            Family::Sans => "IBM Plex Sans",
            Family::Serif => "IBM Plex Serif",
            Family::Mono => "IBM Plex Mono",
            Family::Display => "Chakra Petch",
        }
    }

    /// `sans`, `serif`, `mono`, `display` or a font's own name.
    pub fn parse(s: &str) -> Option<Family> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sans" | "ibm plex sans" | "sans-serif" | "arial" | "helvetica" | "calibri" | "aptos" | "inter" => Some(Family::Sans),
            "serif" | "ibm plex serif" | "times" | "times new roman" | "georgia" | "cambria" => Some(Family::Serif),
            "mono" | "ibm plex mono" | "monospace" | "courier" | "courier new" | "consolas" | "menlo" => Some(Family::Mono),
            "display" | "chakra petch" => Some(Family::Display),
            _ => None,
        }
    }
}

/// How a paragraph style looks: sizes in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleSpec {
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub family: Family,
    pub space_before: f32,
    pub space_after: f32,
    /// Line height as a multiple of the size.
    pub line: f32,
    /// Grey text (subtitle, caption, quote): the text colour at this opacity.
    pub muted: bool,
}

impl ParaStyle {
    pub const ALL: [ParaStyle; 9] = [
        ParaStyle::Normal,
        ParaStyle::Title,
        ParaStyle::Subtitle,
        ParaStyle::Heading1,
        ParaStyle::Heading2,
        ParaStyle::Heading3,
        ParaStyle::Quote,
        ParaStyle::Code,
        ParaStyle::Caption,
    ];

    pub fn spec(self) -> StyleSpec {
        let base = StyleSpec { size: 11.0, bold: false, italic: false, family: Family::Sans, space_before: 0.0, space_after: 8.0, line: 1.4, muted: false };
        match self {
            ParaStyle::Normal => base,
            ParaStyle::Title => StyleSpec { size: 28.0, bold: true, space_after: 6.0, line: 1.15, ..base },
            ParaStyle::Subtitle => StyleSpec { size: 15.0, space_after: 14.0, line: 1.3, muted: true, ..base },
            ParaStyle::Heading1 => StyleSpec { size: 20.0, bold: true, space_before: 18.0, space_after: 6.0, line: 1.2, ..base },
            ParaStyle::Heading2 => StyleSpec { size: 15.0, bold: true, space_before: 14.0, space_after: 4.0, line: 1.25, ..base },
            ParaStyle::Heading3 => StyleSpec { size: 12.5, bold: true, space_before: 10.0, space_after: 4.0, line: 1.3, ..base },
            ParaStyle::Quote => StyleSpec { size: 12.0, italic: true, family: Family::Serif, space_before: 4.0, space_after: 10.0, muted: true, ..base },
            ParaStyle::Code => StyleSpec { size: 9.5, family: Family::Mono, space_after: 0.0, line: 1.45, ..base },
            ParaStyle::Caption => StyleSpec { size: 9.0, space_after: 10.0, muted: true, ..base },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ParaStyle::Normal => "Normal text",
            ParaStyle::Title => "Title",
            ParaStyle::Subtitle => "Subtitle",
            ParaStyle::Heading1 => "Heading 1",
            ParaStyle::Heading2 => "Heading 2",
            ParaStyle::Heading3 => "Heading 3",
            ParaStyle::Quote => "Quote",
            ParaStyle::Code => "Code",
            ParaStyle::Caption => "Caption",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            ParaStyle::Normal => "normal",
            ParaStyle::Title => "title",
            ParaStyle::Subtitle => "subtitle",
            ParaStyle::Heading1 => "heading1",
            ParaStyle::Heading2 => "heading2",
            ParaStyle::Heading3 => "heading3",
            ParaStyle::Quote => "quote",
            ParaStyle::Code => "code",
            ParaStyle::Caption => "caption",
        }
    }

    /// From an id or a label ("heading1", "Heading 1", "h1").
    pub fn parse(s: &str) -> Option<ParaStyle> {
        let k: String = s.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '_').collect::<String>().to_ascii_lowercase();
        Some(match k.as_str() {
            "normal" | "normaltext" | "body" | "p" | "paragraph" => ParaStyle::Normal,
            "title" => ParaStyle::Title,
            "subtitle" => ParaStyle::Subtitle,
            "heading1" | "h1" => ParaStyle::Heading1,
            "heading2" | "h2" => ParaStyle::Heading2,
            "heading3" | "h3" | "heading4" | "h4" | "heading5" | "h5" | "heading6" | "h6" => ParaStyle::Heading3,
            "quote" | "blockquote" => ParaStyle::Quote,
            "code" | "pre" => ParaStyle::Code,
            "caption" => ParaStyle::Caption,
            _ => return None,
        })
    }

    pub fn is_heading(self) -> bool {
        matches!(self, ParaStyle::Title | ParaStyle::Heading1 | ParaStyle::Heading2 | ParaStyle::Heading3)
    }

    /// The heading level for outlines (1 for titles and Heading 1).
    pub fn level(self) -> Option<u8> {
        match self {
            ParaStyle::Title | ParaStyle::Heading1 => Some(1),
            ParaStyle::Heading2 => Some(2),
            ParaStyle::Heading3 => Some(3),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub fn parse(s: &str) -> Option<Align> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "left" | "start" => Align::Left,
            "center" | "centre" | "middle" => Align::Center,
            "right" | "end" => Align::Right,
            "justify" | "justified" | "both" => Align::Justify,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListKind {
    Bullet,
    Number,
    Check,
}

impl ListKind {
    pub fn parse(s: &str) -> Option<Option<ListKind>> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "bullet" | "bullets" | "ul" | "disc" => Some(ListKind::Bullet),
            "number" | "numbered" | "ol" | "decimal" => Some(ListKind::Number),
            "check" | "checklist" | "todo" | "task" => Some(ListKind::Check),
            "none" | "" | "off" => None,
            _ => return None,
        })
    }
}

fn is_default_style(s: &ParaStyle) -> bool {
    *s == ParaStyle::Normal
}

fn is_left(a: &Align) -> bool {
    *a == Align::Left
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paragraph {
    pub id: Id,
    #[serde(default, skip_serializing_if = "is_default_style")]
    pub style: ParaStyle,
    #[serde(default, skip_serializing_if = "is_left")]
    pub align: Align,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<ListKind>,
    /// List nesting, 0 to 5.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub level: u8,
    /// A checklist item is done.
    #[serde(default, skip_serializing_if = "is_false")]
    pub checked: bool,
    #[serde(default)]
    pub runs: Vec<Run>,
}

impl Default for Paragraph {
    fn default() -> Self {
        Paragraph { id: Id::new(), style: ParaStyle::Normal, align: Align::Left, list: None, level: 0, checked: false, runs: vec![] }
    }
}

/// Character count of a string.
pub fn clen(s: &str) -> usize {
    s.chars().count()
}

/// Byte index of the `n`th character (the end when past it).
pub fn byte_at(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(b, _)| b).unwrap_or(s.len())
}

impl Paragraph {
    pub fn new(style: ParaStyle, text: impl Into<String>) -> Self {
        let text = text.into();
        Paragraph { style, runs: if text.is_empty() { vec![] } else { vec![Run::plain(text)] }, ..Default::default() }
    }

    pub fn with_runs(style: ParaStyle, runs: Vec<Run>) -> Self {
        let mut p = Paragraph { style, runs, ..Default::default() };
        p.normalize();
        p
    }

    pub fn list(mut self, kind: ListKind, level: u8) -> Self {
        self.list = Some(kind);
        self.level = level;
        self
    }

    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Length in characters.
    pub fn len(&self) -> usize {
        self.runs.iter().map(|r| clen(&r.text)).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.is_empty())
    }

    /// The formatting new text typed at `offset` takes: the character before it, or the one
    /// after at the start of the paragraph. Links, notes and comments don't extend.
    pub fn style_at(&self, offset: usize) -> RunStyle {
        let mut at = 0;
        let mut found: Option<&RunStyle> = None;
        for r in &self.runs {
            let n = clen(&r.text);
            if offset > at && offset <= at + n {
                found = Some(&r.style);
                break;
            }
            if offset == 0 && found.is_none() {
                found = Some(&r.style);
                break;
            }
            at += n;
        }
        let mut s = found.or_else(|| self.runs.last().map(|r| &r.style)).cloned().unwrap_or_default();
        s.link = None;
        s.note = None;
        s.comment = None;
        s.inserted = None;
        s.deleted = None;
        s
    }

    /// Makes a run boundary at `offset` and returns the index of the run that starts there.
    fn split_at(&mut self, offset: usize) -> usize {
        let mut at = 0;
        for i in 0..self.runs.len() {
            let n = clen(&self.runs[i].text);
            if offset == at {
                return i;
            }
            if offset < at + n {
                let b = byte_at(&self.runs[i].text, offset - at);
                let tail = self.runs[i].text.split_off(b);
                let style = self.runs[i].style.clone();
                self.runs.insert(i + 1, Run { text: tail, style });
                return i + 1;
            }
            at += n;
        }
        self.runs.len()
    }

    /// Merges neighbours with the same formatting and drops empty runs (an empty paragraph
    /// keeps one empty run when it carries formatting, so typing continues in it).
    pub fn normalize(&mut self) {
        let mut out: Vec<Run> = Vec::with_capacity(self.runs.len());
        let keep = if self.is_empty() { self.runs.first().filter(|r| r.style != RunStyle::default()).cloned() } else { None };
        for r in self.runs.drain(..) {
            if r.text.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.style == r.style => last.text.push_str(&r.text),
                _ => out.push(r),
            }
        }
        if out.is_empty()
            && let Some(k) = keep
        {
            out.push(Run { text: String::new(), style: k.style });
        }
        self.runs = out;
    }

    pub fn insert(&mut self, offset: usize, text: &str, style: RunStyle) {
        if text.is_empty() {
            return;
        }
        let offset = offset.min(self.len());
        let i = self.split_at(offset);
        self.runs.insert(i, Run { text: text.to_string(), style });
        self.normalize();
    }

    pub fn delete(&mut self, from: usize, to: usize) {
        let (from, to) = (from.min(to), to.max(from).min(self.len()));
        if from >= to {
            return;
        }
        let a = self.split_at(from);
        let b = self.split_at(to);
        // Keep the deleted text's formatting when the paragraph becomes empty.
        let style = self.runs.get(a).map(|r| r.style.clone());
        self.runs.drain(a..b);
        if self.runs.is_empty()
            && let Some(s) = style
        {
            self.runs.push(Run { text: String::new(), style: s });
        }
        self.normalize();
    }

    /// The runs from `from` to `to`.
    pub fn slice(&self, from: usize, to: usize) -> Vec<Run> {
        let mut p = self.clone();
        let to = to.min(p.len());
        let from = from.min(to);
        let b = p.split_at(to);
        p.runs.truncate(b);
        let a = p.split_at(from);
        p.runs.drain(..a);
        p.runs
    }

    /// Cuts the paragraph at `offset`; returns the runs after it.
    pub fn split_off(&mut self, offset: usize) -> Vec<Run> {
        let i = self.split_at(offset.min(self.len()));
        let tail = self.runs.split_off(i);
        let style = tail.first().map(|r| r.style.clone()).or_else(|| self.runs.last().map(|r| r.style.clone()));
        if self.runs.iter().all(|r| r.text.is_empty())
            && let Some(s) = style.clone()
        {
            self.runs = vec![Run { text: String::new(), style: s }];
        }
        self.normalize();
        let mut t = Paragraph { runs: tail, ..Default::default() };
        if t.runs.is_empty()
            && let Some(s) = style
        {
            t.runs.push(Run { text: String::new(), style: s });
        }
        t.normalize();
        t.runs
    }

    pub fn append(&mut self, runs: Vec<Run>) {
        if self.is_empty() && runs.iter().any(|r| !r.text.is_empty()) {
            self.runs.clear();
        }
        self.runs.extend(runs);
        self.normalize();
    }

    pub fn format(&mut self, from: usize, to: usize, patch: &StylePatch) {
        let (from, to) = (from.min(to), to.max(from).min(self.len()));
        if from == to {
            // An empty paragraph: format its typing style.
            if self.is_empty() {
                let mut s = self.runs.first().map(|r| r.style.clone()).unwrap_or_default();
                patch.apply(&mut s);
                self.runs = vec![Run { text: String::new(), style: s }];
                self.normalize();
            }
            return;
        }
        let a = self.split_at(from);
        let b = self.split_at(to);
        for r in &mut self.runs[a..b] {
            patch.apply(&mut r.style);
        }
        self.normalize();
    }

    /// Whether every character from `from` to `to` has `test` (toolbar state).
    pub fn all(&self, from: usize, to: usize, test: impl Fn(&RunStyle) -> bool) -> bool {
        if from >= to {
            return test(&self.style_at(from));
        }
        let mut at = 0;
        for r in &self.runs {
            let n = clen(&r.text);
            if at + n > from && at < to && !test(&r.style) {
                return false;
            }
            at += n;
        }
        true
    }

    /// Replaces every occurrence of `find` (case-sensitive or not); returns how many.
    pub fn replace_all(&mut self, find: &str, with: &str, case: bool) -> usize {
        if find.is_empty() {
            return 0;
        }
        let mut count = 0;
        let mut start = 0;
        loop {
            let text = self.text();
            let hay: Vec<char> = text.chars().collect();
            let Some(i) = find_chars(&hay, find, start, case) else { break };
            let n = clen(find);
            let style = self.style_at(i + 1);
            self.delete(i, i + n);
            self.insert(i, with, style);
            start = i + clen(with);
            count += 1;
        }
        count
    }
}

/// Character index of `needle` in `hay` from `start`.
pub fn find_chars(hay: &[char], needle: &str, start: usize, case: bool) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() || n.len() > hay.len() {
        return None;
    }
    let eq = |a: char, b: char| if case { a == b } else { a.to_lowercase().eq(b.to_lowercase()) };
    (start..=hay.len() - n.len()).find(|&i| n.iter().enumerate().all(|(j, c)| eq(hay[i + j], *c)))
}

/// One cell of a table: a paragraph of runs (a line break is `\n` in the text).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    #[serde(default)]
    pub runs: Vec<Run>,
    #[serde(default, skip_serializing_if = "is_left")]
    pub align: Align,
    /// Fill `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
}

impl TableCell {
    pub fn text(t: impl Into<String>) -> Self {
        let t = t.into();
        TableCell { runs: if t.is_empty() { vec![] } else { vec![Run::plain(t)] }, ..Default::default() }
    }

    pub fn plain(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// The cell's text as a paragraph, to reuse paragraph editing.
    pub fn as_para(&self) -> Paragraph {
        Paragraph { id: Id(String::new()), align: self.align, runs: self.runs.clone(), ..Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub id: Id,
    pub rows: Vec<Vec<TableCell>>,
    /// The first row is a header (bold, repeated on each page).
    #[serde(default)]
    pub header: bool,
    /// Column widths as fractions of the text width (equal when empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widths: Vec<f32>,
    /// Live link to a sheet range (`'Budget'!A1:D8`): the rows are the range's values,
    /// formatted, refreshed whenever the sheet changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Banded rows.
    #[serde(default, skip_serializing_if = "is_false")]
    pub banded: bool,
}

impl Table {
    pub fn new(rows: usize, cols: usize) -> Self {
        Table {
            id: Id::new(),
            rows: (0..rows.max(1)).map(|_| (0..cols.max(1)).map(|_| TableCell::default()).collect()).collect(),
            header: true,
            widths: vec![],
            link: None,
            banded: false,
        }
    }

    pub fn from_text(rows: Vec<Vec<String>>, header: bool) -> Self {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        Table {
            id: Id::new(),
            rows: rows.into_iter().map(|r| (0..cols).map(|c| TableCell::text(r.get(c).cloned().unwrap_or_default())).collect()).collect(),
            header,
            widths: vec![],
            link: None,
            banded: false,
        }
    }

    pub fn cols(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    pub fn cell(&self, r: usize, c: usize) -> Option<&TableCell> {
        self.rows.get(r).and_then(|row| row.get(c))
    }

    pub fn cell_mut(&mut self, r: usize, c: usize) -> Option<&mut TableCell> {
        self.rows.get_mut(r).and_then(|row| row.get_mut(c))
    }

    /// Column widths as fractions summing to 1.
    pub fn fractions(&self) -> Vec<f32> {
        let n = self.cols().max(1);
        if self.widths.len() == n {
            let sum: f32 = self.widths.iter().sum::<f32>().max(0.0001);
            self.widths.iter().map(|w| w / sum).collect()
        } else {
            vec![1.0 / n as f32; n]
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageBlock {
    pub id: Id,
    /// Media id in the document.
    pub media: Id,
    /// Width in points (the text width when 0).
    #[serde(default)]
    pub width: f32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt: String,
    #[serde(default)]
    pub align: Align,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartBlock {
    pub id: Id,
    pub chart: crate::Chart,
    /// Height in points.
    #[serde(default = "default_chart_h")]
    pub height: f32,
}

fn default_chart_h() -> f32 {
    240.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Image(ImageBlock),
    Chart(ChartBlock),
    PageBreak { id: Id },
}

impl Block {
    pub fn id(&self) -> &Id {
        match self {
            Block::Paragraph(p) => &p.id,
            Block::Table(t) => &t.id,
            Block::Image(i) => &i.id,
            Block::Chart(c) => &c.id,
            Block::PageBreak { id } => id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Block::Paragraph(_) => "paragraph",
            Block::Table(_) => "table",
            Block::Image(_) => "image",
            Block::Chart(_) => "chart",
            Block::PageBreak { .. } => "pageBreak",
        }
    }

    pub fn para(&self) -> Option<&Paragraph> {
        match self {
            Block::Paragraph(p) => Some(p),
            _ => None,
        }
    }

    pub fn para_mut(&mut self) -> Option<&mut Paragraph> {
        match self {
            Block::Paragraph(p) => Some(p),
            _ => None,
        }
    }

    /// Image, chart and page break: selected whole, offsets 0 (before) and 1 (after).
    pub fn is_atomic(&self) -> bool {
        matches!(self, Block::Image(_) | Block::Chart(_) | Block::PageBreak { .. })
    }

    /// Length in caret positions: characters for paragraphs, 1 for atomic blocks, 0 for tables
    /// (whose positions are inside cells).
    pub fn len(&self) -> usize {
        match self {
            Block::Paragraph(p) => p.len(),
            Block::Table(_) => 0,
            _ => 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Plain text (tables as tab-separated rows).
    pub fn plain(&self) -> String {
        match self {
            Block::Paragraph(p) => p.text(),
            Block::Table(t) => t.rows.iter().map(|r| r.iter().map(TableCell::plain).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n"),
            Block::Image(i) => if i.caption.is_empty() { "[image]".into() } else { format!("[image: {}]", i.caption) },
            Block::Chart(c) => format!("[chart: {}]", c.chart.title),
            Block::PageBreak { .. } => String::new(),
        }
    }

    pub fn para_block(p: Paragraph) -> Block {
        Block::Paragraph(p)
    }
}

/// A caret position in a flow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pos {
    pub block: usize,
    /// `(row, column)` when inside a table cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<(usize, usize)>,
    pub offset: usize,
}

impl Pos {
    pub fn new(block: usize, offset: usize) -> Self {
        Pos { block, cell: None, offset }
    }

    pub fn in_cell(block: usize, row: usize, col: usize, offset: usize) -> Self {
        Pos { block, cell: Some((row, col)), offset }
    }
}

impl PartialOrd for Pos {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pos {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.block, self.cell, self.offset).cmp(&(other.block, other.cell, other.offset))
    }
}

/// Page size and margins in points; header and footer text (`{page}`, `{pages}` and `{title}`
/// are filled in).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub header: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub footer: String,
    /// Header and footer skip the first page.
    #[serde(skip_serializing_if = "is_false")]
    pub different_first: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self::a4()
    }
}

impl PageSetup {
    pub fn a4() -> Self {
        PageSetup { width: 595.0, height: 842.0, margin_top: 72.0, margin_bottom: 72.0, margin_left: 72.0, margin_right: 72.0, header: String::new(), footer: "{page}".into(), different_first: false }
    }

    pub fn letter() -> Self {
        PageSetup { width: 612.0, height: 792.0, ..Self::a4() }
    }

    /// Named sizes: a4, letter, legal, a5, a3.
    pub fn size_named(name: &str) -> Option<(f32, f32)> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "a4" => (595.0, 842.0),
            "letter" | "us letter" => (612.0, 792.0),
            "legal" => (612.0, 1008.0),
            "a5" => (420.0, 595.0),
            "a3" => (842.0, 1191.0),
            _ => return None,
        })
    }

    pub fn size_name(&self) -> &'static str {
        let (w, h) = (self.width.min(self.height), self.width.max(self.height));
        for name in ["a4", "letter", "legal", "a5", "a3"] {
            let (a, b) = Self::size_named(name).unwrap();
            if (a - w).abs() < 2.0 && (b - h).abs() < 2.0 {
                return name;
            }
        }
        "custom"
    }

    pub fn landscape(&self) -> bool {
        self.width > self.height
    }

    pub fn text_width(&self) -> f32 {
        (self.width - self.margin_left - self.margin_right).max(36.0)
    }

    pub fn text_height(&self) -> f32 {
        (self.height - self.margin_top - self.margin_bottom).max(36.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: Id,
    pub author: String,
    pub text: String,
    pub at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Reply>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub author: String,
    pub text: String,
    pub at: chrono::DateTime<chrono::Utc>,
}

/// A document page: rich text on paper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDoc {
    pub blocks: Flow,
    #[serde(default)]
    pub setup: PageSetup,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
    /// Edits are recorded as tracked changes (`inserted` / `deleted` on runs).
    #[serde(default, skip_serializing_if = "is_false")]
    pub track_changes: bool,
}

impl Default for TextDoc {
    fn default() -> Self {
        TextDoc { blocks: imbl::vector![Block::Paragraph(Paragraph::default())], setup: PageSetup::default(), comments: vec![], track_changes: false }
    }
}

impl TextDoc {
    pub fn plain(&self) -> String {
        self.blocks.iter().map(Block::plain).collect::<Vec<_>>().join("\n")
    }

    pub fn word_count(&self) -> usize {
        self.blocks.iter().map(|b| b.plain().split_whitespace().count()).sum()
    }

    /// Headings in order: (block index, level, text).
    pub fn outline(&self) -> Vec<(usize, u8, String)> {
        self.blocks.iter().enumerate().filter_map(|(i, b)| b.para().and_then(|p| p.style.level().map(|l| (i, l, p.text())))).collect()
    }

    pub fn find_block(&self, id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| b.id() == id)
    }
}

// ---- flow editing -------------------------------------------------------------------------

/// Makes sure a flow is never empty (an empty document still has a paragraph to type in).
pub fn ensure_nonempty(flow: &mut Flow) {
    if flow.is_empty() {
        flow.push_back(Block::Paragraph(Paragraph::default()));
    }
}

/// Clamps a position to the flow.
pub fn clamp(flow: &Flow, pos: Pos) -> Pos {
    if flow.is_empty() {
        return Pos::default();
    }
    let block = pos.block.min(flow.len() - 1);
    match &flow[block] {
        Block::Table(t) => {
            let (r, c) = pos.cell.unwrap_or((0, 0));
            let r = r.min(t.rows.len().saturating_sub(1));
            let c = c.min(t.rows.get(r).map(|x| x.len()).unwrap_or(1).saturating_sub(1));
            let len = t.cell(r, c).map(|x| x.as_para().len()).unwrap_or(0);
            Pos { block, cell: Some((r, c)), offset: pos.offset.min(len) }
        }
        b => Pos { block, cell: None, offset: pos.offset.min(b.len()) },
    }
}

/// The end of the flow.
pub fn end(flow: &Flow) -> Pos {
    match flow.last() {
        None => Pos::default(),
        Some(Block::Table(t)) => {
            let r = t.rows.len().saturating_sub(1);
            let c = t.rows.get(r).map(|x| x.len()).unwrap_or(1).saturating_sub(1);
            Pos::in_cell(flow.len() - 1, r, c, t.cell(r, c).map(|x| x.as_para().len()).unwrap_or(0))
        }
        Some(b) => Pos::new(flow.len() - 1, b.len()),
    }
}

/// Runs `f` on the paragraph (or table cell, as a paragraph) at `pos`.
fn with_para<R>(flow: &mut Flow, pos: Pos, f: impl FnOnce(&mut Paragraph) -> R) -> Option<R> {
    let block = flow.get_mut(pos.block)?;
    match (block, pos.cell) {
        (Block::Paragraph(p), None) => Some(f(p)),
        (Block::Table(t), Some((r, c))) => {
            let cell = t.cell_mut(r, c)?;
            let mut p = cell.as_para();
            let out = f(&mut p);
            cell.runs = p.runs;
            Some(out)
        }
        _ => None,
    }
}

fn para_at(flow: &Flow, pos: Pos) -> Option<Paragraph> {
    match (flow.get(pos.block)?, pos.cell) {
        (Block::Paragraph(p), None) => Some(p.clone()),
        (Block::Table(t), Some((r, c))) => t.cell(r, c).map(TableCell::as_para),
        _ => None,
    }
}

/// The formatting text typed at `pos` takes.
pub fn style_at(flow: &Flow, pos: Pos) -> RunStyle {
    para_at(flow, pos).map(|p| p.style_at(pos.offset)).unwrap_or_default()
}

/// Inserts text at `pos` (a `\n` starts a new paragraph, or a line inside a table cell) and
/// returns the position after it. On an image or chart, the text goes in a new paragraph
/// beside it.
pub fn insert_text(flow: &mut Flow, pos: Pos, text: &str, style: Option<RunStyle>) -> Pos {
    ensure_nonempty(flow);
    let mut pos = clamp(flow, pos);
    if flow[pos.block].is_atomic() {
        let at = if pos.offset == 0 { pos.block } else { pos.block + 1 };
        flow.insert(at, Block::Paragraph(Paragraph::default()));
        pos = Pos::new(at, 0);
    }
    let style = style.unwrap_or_else(|| style_at(flow, pos));
    let in_cell = pos.cell.is_some();
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            if in_cell {
                with_para(flow, pos, |p| p.insert(pos.offset, "\n", style.clone()));
                pos.offset += 1;
            } else {
                pos = split(flow, pos);
            }
        }
        first = false;
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !line.is_empty() {
            with_para(flow, pos, |p| p.insert(pos.offset, line, style.clone()));
            pos.offset += clen(line);
        }
    }
    pos
}

/// Enter: splits the paragraph at `pos` and returns the start of the new one. An empty list
/// item leaves the list instead; in a table cell it adds a line.
pub fn split(flow: &mut Flow, pos: Pos) -> Pos {
    ensure_nonempty(flow);
    let pos = clamp(flow, pos);
    if pos.cell.is_some() {
        let style = style_at(flow, pos);
        with_para(flow, pos, |p| p.insert(pos.offset, "\n", style));
        return Pos { offset: pos.offset + 1, ..pos };
    }
    match &mut flow[pos.block] {
        Block::Paragraph(p) => {
            if p.is_empty() && p.list.is_some() {
                if p.level > 0 {
                    p.level -= 1;
                } else {
                    p.list = None;
                    p.checked = false;
                }
                return pos;
            }
            let at_end = pos.offset >= p.len();
            let tail = p.split_off(pos.offset);
            let style = if at_end && (p.style.is_heading() || p.style == ParaStyle::Subtitle || p.style == ParaStyle::Caption) { ParaStyle::Normal } else { p.style };
            let next = Paragraph { id: Id::new(), style, align: p.align, list: p.list, level: p.level, checked: false, runs: tail };
            flow.insert(pos.block + 1, Block::Paragraph(next));
            Pos::new(pos.block + 1, 0)
        }
        _ => {
            let at = if pos.offset == 0 { pos.block } else { pos.block + 1 };
            flow.insert(at, Block::Paragraph(Paragraph::default()));
            Pos::new(if pos.offset == 0 { pos.block + 1 } else { at }, 0)
        }
    }
}

/// Deletes from `a` to `b` (in either order) and returns where the caret lands. Across
/// blocks: the first paragraph keeps its start and takes the last one's end; whole blocks in
/// between go; a table at either end stays (only its cell text in the range goes).
pub fn delete(flow: &mut Flow, a: Pos, b: Pos) -> Pos {
    ensure_nonempty(flow);
    let (a, b) = (clamp(flow, a.min(b)), clamp(flow, a.max(b)));
    if a == b {
        return a;
    }
    if a.block == b.block {
        if a.cell.is_some() && a.cell == b.cell || a.cell.is_none() && b.cell.is_none() && !flow[a.block].is_atomic() {
            with_para(flow, a, |p| p.delete(a.offset, b.offset));
            return a;
        }
        if flow[a.block].is_atomic() {
            flow.remove(a.block);
            ensure_nonempty(flow);
            return clamp(flow, Pos::new(a.block, 0));
        }
        // Across cells of one table: clear the text of the cells in between.
        if let Block::Table(t) = &mut flow[a.block] {
            let (ra, ca) = a.cell.unwrap_or((0, 0));
            let (rb, cb) = b.cell.unwrap_or((t.rows.len(), 0));
            for (r, row) in t.rows.iter_mut().enumerate() {
                for (c, cell) in row.iter_mut().enumerate() {
                    if (r, c) > (ra, ca) && (r, c) < (rb, cb) {
                        cell.runs.clear();
                    }
                }
            }
            let len_a = t.cell(ra, ca).map(|c| c.as_para().len()).unwrap_or(0);
            with_para(flow, a, |p| p.delete(a.offset, len_a));
            with_para(flow, b, |p| p.delete(0, b.offset));
        }
        return a;
    }
    // Head.
    let mut remove_from = a.block + 1;
    let mut caret = a;
    match &flow[a.block] {
        Block::Paragraph(_) => {
            let len = flow[a.block].len();
            with_para(flow, a, |p| p.delete(a.offset, len));
        }
        Block::Table(_) => {
            let len = para_at(flow, a).map(|p| p.len()).unwrap_or(0);
            with_para(flow, a, |p| p.delete(a.offset, len));
        }
        _ => {
            if a.offset == 0 {
                remove_from = a.block;
                caret = Pos::new(a.block, 0);
            }
        }
    }
    // Tail.
    let mut remove_to = b.block; // exclusive
    let mut merge_tail = false;
    match &flow[b.block] {
        Block::Paragraph(_) => {
            with_para(flow, b, |p| p.delete(0, b.offset));
            merge_tail = matches!(flow[a.block], Block::Paragraph(_)) && remove_from == a.block + 1;
            if merge_tail {
                remove_to = b.block + 1;
            }
        }
        Block::Table(_) => {
            with_para(flow, b, |p| p.delete(0, b.offset));
        }
        _ => {
            if b.offset == 1 {
                remove_to = b.block + 1;
            }
        }
    }
    let tail_runs = if merge_tail { flow[b.block].para().map(|p| p.runs.clone()) } else { None };
    for i in (remove_from..remove_to).rev() {
        flow.remove(i);
    }
    if let Some(runs) = tail_runs
        && let Some(p) = flow[a.block].para_mut()
    {
        p.append(runs);
    }
    ensure_nonempty(flow);
    if caret.block >= flow.len() {
        caret = end(flow);
    }
    clamp(flow, caret)
}

/// Applies character formatting from `a` to `b`.
pub fn format(flow: &mut Flow, a: Pos, b: Pos, patch: &StylePatch) {
    if flow.is_empty() {
        return;
    }
    let (a, b) = (a.min(b), a.max(b));
    for i in a.block..=b.block.min(flow.len().saturating_sub(1)) {
        match &mut flow[i] {
            Block::Paragraph(p) => {
                let from = if i == a.block { a.offset } else { 0 };
                let to = if i == b.block { b.offset } else { p.len() };
                p.format(from, to, patch);
            }
            Block::Table(t) => {
                let first = if i == a.block { a.cell } else { None };
                let last = if i == b.block { b.cell } else { None };
                for (r, row) in t.rows.iter_mut().enumerate() {
                    for (c, cell) in row.iter_mut().enumerate() {
                        let inside = first.is_none_or(|f| (r, c) >= f) && last.is_none_or(|l| (r, c) <= l);
                        if !inside {
                            continue;
                        }
                        let mut p = cell.as_para();
                        let from = if first == Some((r, c)) { a.offset } else { 0 };
                        let to = if last == Some((r, c)) { b.offset } else { p.len() };
                        p.format(from, to, patch);
                        cell.runs = p.runs;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Whether all text from `a` to `b` passes `test` (bold button state…).
pub fn all_styled(flow: &Flow, a: Pos, b: Pos, test: impl Fn(&RunStyle) -> bool) -> bool {
    if flow.is_empty() {
        return false;
    }
    let (a, b) = (a.min(b), a.max(b));
    if a == b {
        return test(&style_at(flow, a));
    }
    for i in a.block..=b.block.min(flow.len().saturating_sub(1)) {
        let ok = match (&flow[i], a.cell.filter(|_| i == a.block)) {
            (Block::Paragraph(p), _) => {
                let from = if i == a.block { a.offset } else { 0 };
                let to = if i == b.block { b.offset } else { p.len() };
                p.all(from, to, &test)
            }
            (Block::Table(t), Some((r, c))) if a.cell == b.cell && i == b.block => t.cell(r, c).is_none_or(|cell| cell.as_para().all(a.offset, b.offset, &test)),
            _ => true,
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Paragraph settings for a set of paragraphs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ParaPatch {
    pub style: Option<ParaStyle>,
    pub align: Option<Align>,
    /// `Some(None)` ends the list.
    pub list: Option<Option<ListKind>>,
    pub level: Option<u8>,
    pub checked: Option<bool>,
}

impl ParaPatch {
    pub fn apply(&self, p: &mut Paragraph) {
        if let Some(s) = self.style {
            p.style = s;
        }
        if let Some(a) = self.align {
            p.align = a;
        }
        if let Some(l) = self.list {
            p.list = l;
            if l.is_none() {
                p.level = 0;
                p.checked = false;
            }
        }
        if let Some(l) = self.level {
            p.level = l.min(5);
        }
        if let Some(c) = self.checked {
            p.checked = c;
        }
    }
}

/// Applies paragraph settings to every paragraph from block `a` to block `b`.
pub fn set_paragraphs(flow: &mut Flow, a: usize, b: usize, patch: &ParaPatch) {
    if flow.is_empty() {
        return;
    }
    let (a, b) = (a.min(b), a.max(b));
    for i in a..=b.min(flow.len().saturating_sub(1)) {
        if let Block::Paragraph(p) = &mut flow[i] {
            patch.apply(p);
        } else if let (Block::Table(t), Some(align)) = (&mut flow[i], patch.align) {
            for cell in t.rows.iter_mut().flatten() {
                cell.align = align;
            }
        }
    }
}

/// The blocks from `a` to `b`, cut to the range (copy).
pub fn slice(flow: &Flow, a: Pos, b: Pos) -> Vec<Block> {
    let (a, b) = (clamp(flow, a.min(b)), clamp(flow, a.max(b)));
    let mut out = vec![];
    for i in a.block..=b.block {
        match &flow[i] {
            Block::Paragraph(p) => {
                let from = if i == a.block { a.offset } else { 0 };
                let to = if i == b.block { b.offset } else { p.len() };
                let mut q = p.clone();
                q.id = Id::new();
                q.runs = p.slice(from, to);
                out.push(Block::Paragraph(q));
            }
            Block::Table(t) if i == a.block && i == b.block && a.cell == b.cell && a.cell.is_some() => {
                let (r, c) = a.cell.unwrap();
                if let Some(cell) = t.cell(r, c) {
                    let runs = cell.as_para().slice(a.offset, b.offset);
                    out.push(Block::Paragraph(Paragraph { runs, ..Default::default() }));
                }
            }
            b2 => {
                let atomic_skipped = b2.is_atomic() && ((i == a.block && a.offset == 1) || (i == b.block && b.offset == 0));
                if !atomic_skipped {
                    let mut c = b2.clone();
                    renew_ids(&mut c);
                    out.push(c);
                }
            }
        }
    }
    out
}

fn renew_ids(b: &mut Block) {
    match b {
        Block::Paragraph(p) => p.id = Id::new(),
        Block::Table(t) => t.id = Id::new(),
        Block::Image(i) => i.id = Id::new(),
        Block::Chart(c) => c.id = Id::new(),
        Block::PageBreak { id } => *id = Id::new(),
    }
}

/// Plain text from `a` to `b`.
pub fn plain(flow: &Flow, a: Pos, b: Pos) -> String {
    slice(flow, a, b).iter().map(Block::plain).collect::<Vec<_>>().join("\n")
}

/// Pastes blocks at `pos`: the first paragraph joins the one at the caret, the last one takes
/// the rest of it, anything else goes in between. Returns the position after the paste.
pub fn insert_blocks(flow: &mut Flow, pos: Pos, blocks: Vec<Block>) -> Pos {
    ensure_nonempty(flow);
    let mut blocks = blocks;
    if blocks.is_empty() {
        return pos;
    }
    for b in &mut blocks {
        renew_ids(b);
    }
    let pos = clamp(flow, pos);
    // Inside a table cell, only text goes in.
    if pos.cell.is_some() {
        let text = blocks.iter().map(Block::plain).collect::<Vec<_>>().join("\n");
        return insert_text(flow, pos, &text, None);
    }
    if blocks.len() == 1
        && let Block::Paragraph(p) = &blocks[0]
        && matches!(flow[pos.block], Block::Paragraph(_))
    {
        let n = p.len();
        let runs = p.runs.clone();
        if let Some(target) = flow[pos.block].para_mut() {
            let tail = target.split_off(pos.offset);
            target.append(runs);
            target.append(tail);
        }
        return Pos::new(pos.block, pos.offset + n);
    }
    // Split at the caret, then put the blocks between the halves.
    let after = split_keep(flow, pos);
    let mut at = after;
    let first_is_para = matches!(blocks.first(), Some(Block::Paragraph(_)));
    let last_is_para = matches!(blocks.last(), Some(Block::Paragraph(_)));
    let count = blocks.len();
    let mut caret = Pos::new(after, 0);
    for (i, b) in blocks.into_iter().enumerate() {
        if i == 0 && first_is_para && after > 0 && matches!(flow[after - 1], Block::Paragraph(_)) {
            if let (Block::Paragraph(src), Some(dst)) = (&b, flow[after - 1].para_mut()) {
                dst.append(src.runs.clone());
                caret = Pos::new(after - 1, dst.len());
            }
            continue;
        }
        if i == count - 1 && last_is_para && i > 0 && at < flow.len() && matches!(flow[at], Block::Paragraph(_)) {
            if let Block::Paragraph(src) = &b {
                let n = src.len();
                if let Some(dst) = flow[at].para_mut() {
                    let tail = std::mem::take(&mut dst.runs);
                    dst.runs = src.runs.clone();
                    dst.style = src.style;
                    dst.list = src.list;
                    dst.append(tail);
                }
                caret = Pos::new(at, n);
            }
            continue;
        }
        flow.insert(at, b);
        caret = Pos::new(at, flow[at].len());
        at += 1;
    }
    caret
}

/// Splits at `pos` without Enter's list rules; returns the index of the second half.
fn split_keep(flow: &mut Flow, pos: Pos) -> usize {
    match &mut flow[pos.block] {
        Block::Paragraph(p) => {
            let tail = p.split_off(pos.offset);
            let next = Paragraph { id: Id::new(), style: p.style, align: p.align, list: p.list, level: p.level, checked: false, runs: tail };
            flow.insert(pos.block + 1, Block::Paragraph(next));
            pos.block + 1
        }
        _ => {
            if pos.offset == 0 {
                pos.block
            } else {
                pos.block + 1
            }
        }
    }
}

/// Every match of `find` in the flow's paragraphs and table cells: (start, end) positions.
pub fn find_all(flow: &Flow, find: &str, case: bool) -> Vec<(Pos, Pos)> {
    let mut out = vec![];
    let n = clen(find);
    if n == 0 {
        return out;
    }
    let mut search = |p: &Paragraph, mk: &dyn Fn(usize) -> Pos| {
        let hay: Vec<char> = p.text().chars().collect();
        let mut start = 0;
        while let Some(i) = find_chars(&hay, find, start, case) {
            out.push((mk(i), mk(i + n)));
            start = i + n.max(1);
        }
    };
    for (bi, b) in flow.iter().enumerate() {
        match b {
            Block::Paragraph(p) => search(p, &|o| Pos::new(bi, o)),
            Block::Table(t) => {
                for (r, row) in t.rows.iter().enumerate() {
                    for (c, cell) in row.iter().enumerate() {
                        search(&cell.as_para(), &|o| Pos::in_cell(bi, r, c, o));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Replaces every match; returns how many.
pub fn replace_all(flow: &mut Flow, find: &str, with: &str, case: bool) -> usize {
    let mut count = 0;
    for i in 0..flow.len() {
        match &mut flow[i] {
            Block::Paragraph(p) => count += p.replace_all(find, with, case),
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flatten() {
                    let mut p = cell.as_para();
                    let n = p.replace_all(find, with, case);
                    if n > 0 {
                        cell.runs = p.runs;
                        count += n;
                    }
                }
            }
            _ => {}
        }
    }
    count
}

/// Moves a position one character left or right (across block boundaries, into and out of
/// table cells in reading order).
pub fn step(flow: &Flow, pos: Pos, forward: bool) -> Pos {
    let pos = clamp(flow, pos);
    let len = |p: Pos| -> usize {
        match (&flow[p.block], p.cell) {
            (Block::Table(t), Some((r, c))) => t.cell(r, c).map(|x| x.as_para().len()).unwrap_or(0),
            (b, _) => b.len(),
        }
    };
    if forward {
        if pos.offset < len(pos) {
            return Pos { offset: pos.offset + 1, ..pos };
        }
        if let (Block::Table(t), Some((r, c))) = (&flow[pos.block], pos.cell) {
            if c + 1 < t.rows[r].len() {
                return Pos::in_cell(pos.block, r, c + 1, 0);
            }
            if r + 1 < t.rows.len() {
                return Pos::in_cell(pos.block, r + 1, 0, 0);
            }
        }
        if pos.block + 1 < flow.len() {
            return first_in(flow, pos.block + 1);
        }
        pos
    } else {
        if pos.offset > 0 {
            return Pos { offset: pos.offset - 1, ..pos };
        }
        if let (Block::Table(t), Some((r, c))) = (&flow[pos.block], pos.cell) {
            if c > 0 {
                let p = Pos::in_cell(pos.block, r, c - 1, 0);
                return Pos { offset: len(p), ..p };
            }
            if r > 0 {
                let c = t.rows[r - 1].len().saturating_sub(1);
                let p = Pos::in_cell(pos.block, r - 1, c, 0);
                return Pos { offset: len(p), ..p };
            }
        }
        if pos.block > 0 {
            return last_in(flow, pos.block - 1);
        }
        pos
    }
}

/// The first caret position in a block.
pub fn first_in(flow: &Flow, block: usize) -> Pos {
    match flow.get(block) {
        Some(Block::Table(_)) => Pos::in_cell(block, 0, 0, 0),
        _ => Pos::new(block, 0),
    }
}

/// The last caret position in a block.
pub fn last_in(flow: &Flow, block: usize) -> Pos {
    match flow.get(block) {
        Some(Block::Table(t)) => {
            let r = t.rows.len().saturating_sub(1);
            let c = t.rows.get(r).map(|x| x.len()).unwrap_or(1).saturating_sub(1);
            Pos::in_cell(block, r, c, t.cell(r, c).map(|x| x.as_para().len()).unwrap_or(0))
        }
        Some(b) => Pos::new(block, b.len()),
        None => Pos::default(),
    }
}

/// Word boundaries around a character offset in a paragraph (double-click).
pub fn word_at(p: &Paragraph, offset: usize) -> (usize, usize) {
    let chars: Vec<char> = p.text().chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    let mut a = offset.min(chars.len());
    let mut b = a;
    if a < chars.len() && !is_word(chars[a]) && (a == 0 || !is_word(chars[a - 1])) {
        return (a, (a + 1).min(chars.len()));
    }
    while a > 0 && is_word(chars[a - 1]) {
        a -= 1;
    }
    while b < chars.len() && is_word(chars[b]) {
        b += 1;
    }
    (a, b)
}

/// Accepts or rejects every tracked change: inserted text stays (accept) or goes (reject),
/// deleted text goes (accept) or stays (reject). Returns how many runs changed.
pub fn resolve_changes(flow: &mut Flow, accept: bool) -> usize {
    let mut n = 0;
    let fix = |runs: &mut Vec<Run>, n: &mut usize| {
        runs.retain_mut(|r| {
            let (ins, del) = (r.style.inserted.take(), r.style.deleted.take());
            if ins.is_some() || del.is_some() {
                *n += 1;
            }
            !((accept && del.is_some()) || (!accept && ins.is_some()))
        });
    };
    for i in 0..flow.len() {
        match &mut flow[i] {
            Block::Paragraph(p) => {
                fix(&mut p.runs, &mut n);
                p.normalize();
            }
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flatten() {
                    fix(&mut cell.runs, &mut n);
                }
            }
            _ => {}
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(paras: &[&str]) -> Flow {
        paras.iter().map(|t| Block::Paragraph(Paragraph::new(ParaStyle::Normal, *t))).collect()
    }

    fn texts(f: &Flow) -> Vec<String> {
        f.iter().map(Block::plain).collect()
    }

    #[test]
    fn insert_and_split() {
        let mut f = flow(&["Hello world"]);
        let p = insert_text(&mut f, Pos::new(0, 5), ",", None);
        assert_eq!(p, Pos::new(0, 6));
        assert_eq!(texts(&f), ["Hello, world"]);
        let p = insert_text(&mut f, Pos::new(0, 6), "\nnew\n", None);
        assert_eq!(texts(&f), ["Hello,", "new", " world"]);
        assert_eq!(p, Pos::new(2, 0));
    }

    #[test]
    fn unicode_offsets_are_characters() {
        let mut f = flow(&["héllo wörld"]);
        insert_text(&mut f, Pos::new(0, 2), "X", None);
        assert_eq!(texts(&f), ["héXllo wörld"]);
        delete(&mut f, Pos::new(0, 7), Pos::new(0, 9));
        assert_eq!(texts(&f), ["héXllo ld"]);
    }

    #[test]
    fn delete_across_paragraphs_merges() {
        let mut f = flow(&["one two", "three", "four five"]);
        let p = delete(&mut f, Pos::new(0, 4), Pos::new(2, 5));
        assert_eq!(texts(&f), ["one five"]);
        assert_eq!(p, Pos::new(0, 4));
    }

    #[test]
    fn backspace_at_start_merges() {
        let mut f = flow(&["ab", "cd"]);
        let p = delete(&mut f, Pos::new(0, 2), Pos::new(1, 0));
        assert_eq!(texts(&f), ["abcd"]);
        assert_eq!(p, Pos::new(0, 2));
    }

    #[test]
    fn format_and_query() {
        let mut f = flow(&["make this bold"]);
        format(&mut f, Pos::new(0, 5), Pos::new(0, 9), &StylePatch { bold: Some(true), ..Default::default() });
        let p = f[0].para().unwrap();
        assert_eq!(p.runs.len(), 3);
        assert!(p.runs[1].style.bold);
        assert_eq!(p.runs[1].text, "this");
        assert!(all_styled(&f, Pos::new(0, 5), Pos::new(0, 9), |s| s.bold));
        assert!(!all_styled(&f, Pos::new(0, 0), Pos::new(0, 9), |s| s.bold));
        // Typing after bold text continues bold.
        insert_text(&mut f, Pos::new(0, 9), "!", None);
        assert_eq!(f[0].para().unwrap().runs[1].text, "this!");
        // Unbolding merges the runs back.
        format(&mut f, Pos::new(0, 0), Pos::new(0, 15), &StylePatch { bold: Some(false), ..Default::default() });
        assert_eq!(f[0].para().unwrap().runs.len(), 1);
    }

    #[test]
    fn enter_on_empty_list_item_ends_list() {
        let mut f: Flow = imbl::vector![Block::Paragraph(Paragraph::new(ParaStyle::Normal, "item").list(ListKind::Bullet, 0))];
        let p = split(&mut f, Pos::new(0, 4));
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].para().unwrap().list, Some(ListKind::Bullet));
        let p2 = split(&mut f, p);
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].para().unwrap().list, None);
        assert_eq!(p2, p);
    }

    #[test]
    fn enter_after_heading_gives_normal() {
        let mut f: Flow = imbl::vector![Block::Paragraph(Paragraph::new(ParaStyle::Heading1, "Title"))];
        split(&mut f, Pos::new(0, 5));
        assert_eq!(f[1].para().unwrap().style, ParaStyle::Normal);
    }

    #[test]
    fn table_cells_edit_like_paragraphs() {
        let mut f: Flow = imbl::vector![Block::Table(Table::new(2, 2))];
        let p = insert_text(&mut f, Pos::in_cell(0, 1, 1, 0), "cell", None);
        assert_eq!(p, Pos::in_cell(0, 1, 1, 4));
        let Block::Table(t) = &f[0] else { panic!() };
        assert_eq!(t.cell(1, 1).unwrap().plain(), "cell");
        assert_eq!(step(&f, Pos::in_cell(0, 0, 0, 0), true), Pos::in_cell(0, 0, 1, 0));
    }

    #[test]
    fn atomic_blocks_delete_whole() {
        let mut f = flow(&["a", "b"]);
        f.insert(1, Block::PageBreak { id: Id::new() });
        let p = delete(&mut f, Pos::new(1, 0), Pos::new(1, 1));
        assert_eq!(f.len(), 2);
        assert_eq!(p, Pos::new(1, 0));
    }

    #[test]
    fn paste_blocks_in_the_middle() {
        let mut f = flow(&["start end"]);
        let pasted = vec![Block::Paragraph(Paragraph::new(ParaStyle::Normal, "A")), Block::Paragraph(Paragraph::new(ParaStyle::Normal, "B"))];
        let p = insert_blocks(&mut f, Pos::new(0, 6), pasted);
        assert_eq!(texts(&f), ["start A", "Bend"]);
        assert_eq!(p, Pos::new(1, 1));
    }

    #[test]
    fn find_and_replace() {
        let mut f = flow(&["the cat and the hat", "The end"]);
        assert_eq!(find_all(&f, "the", false).len(), 3);
        assert_eq!(find_all(&f, "the", true).len(), 2);
        assert_eq!(replace_all(&mut f, "the", "a", false), 3);
        assert_eq!(texts(&f), ["a cat and a hat", "a end"]);
    }

    #[test]
    fn slice_keeps_formatting() {
        let mut f = flow(&["one two three"]);
        format(&mut f, Pos::new(0, 4), Pos::new(0, 7), &StylePatch { italic: Some(true), ..Default::default() });
        let s = slice(&f, Pos::new(0, 2), Pos::new(0, 9));
        let p = s[0].para().unwrap();
        assert_eq!(p.text(), "e two t");
        assert!(p.runs.iter().any(|r| r.style.italic && r.text == "two"));
    }

    #[test]
    fn word_boundaries() {
        let p = Paragraph::new(ParaStyle::Normal, "hello big world");
        assert_eq!(word_at(&p, 7), (6, 9));
        assert_eq!(word_at(&p, 0), (0, 5));
    }

    #[test]
    fn tracked_changes_resolve() {
        let mut f = flow(&["keep"]);
        if let Some(p) = f[0].para_mut() {
            p.runs.push(Run::styled(" new", RunStyle { inserted: Some("me".into()), ..Default::default() }));
            p.runs.push(Run::styled(" old", RunStyle { deleted: Some("me".into()), ..Default::default() }));
        }
        let mut accepted = f.clone();
        assert_eq!(resolve_changes(&mut accepted, true), 2);
        assert_eq!(texts(&accepted), ["keep new"]);
        resolve_changes(&mut f, false);
        assert_eq!(texts(&f), ["keep old"]);
    }

    #[test]
    fn json_is_compact() {
        let p = Paragraph::new(ParaStyle::Normal, "hi");
        let v = serde_json::to_value(Block::Paragraph(p)).unwrap();
        assert_eq!(v["type"], "paragraph");
        assert_eq!(v["runs"][0], serde_json::json!({"text": "hi"}));
        assert!(v.get("style").is_none());
    }
}
