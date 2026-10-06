//! Decks: slides with layouts, text, shapes, images, charts and tables, speaker notes and a
//! theme. Coordinates are points on the slide (960 × 540 for 16:9, PowerPoint's default).

use serde::{Deserialize, Serialize};

use crate::text::{Align, Block, Flow, ParaStyle, Paragraph, Run, Table};
use crate::{Chart, Id};

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeckTheme {
    pub name: String,
    pub background: String,
    pub text: String,
    /// Shapes, chart series and accents.
    pub accent: String,
    /// `display`, `sans`, `serif` or `mono`.
    pub heading_font: String,
    pub body_font: String,
}

impl Default for DeckTheme {
    fn default() -> Self {
        DeckTheme::named("paper").unwrap()
    }
}

impl DeckTheme {
    pub const NAMES: [&'static str; 5] = ["paper", "ink", "grain", "serif", "mono"];

    /// Built-in themes: black and white like the suite, plus a serif and a mono one.
    pub fn named(name: &str) -> Option<DeckTheme> {
        let t = |name: &str, bg: &str, text: &str, accent: &str, h: &str, b: &str| DeckTheme {
            name: name.into(),
            background: bg.into(),
            text: text.into(),
            accent: accent.into(),
            heading_font: h.into(),
            body_font: b.into(),
        };
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "paper" | "light" | "default" => t("paper", "#fbfbfb", "#0a0a0a", "#0a0a0a", "display", "sans"),
            "ink" | "dark" => t("ink", "#0a0a0a", "#f2f2f2", "#f2f2f2", "display", "sans"),
            "grain" => t("grain", "#e9e6df", "#141414", "#c8291c", "display", "sans"),
            "serif" | "classic" => t("serif", "#ffffff", "#1a1a1a", "#1d4ed8", "serif", "serif"),
            "mono" | "terminal" => t("mono", "#111111", "#e6e6e6", "#7dd3a8", "mono", "mono"),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlideLayout {
    Title,
    #[default]
    TitleContent,
    Section,
    TwoContent,
    TitleOnly,
    Blank,
}

impl SlideLayout {
    pub const ALL: [SlideLayout; 6] = [SlideLayout::Title, SlideLayout::TitleContent, SlideLayout::Section, SlideLayout::TwoContent, SlideLayout::TitleOnly, SlideLayout::Blank];

    pub fn parse(s: &str) -> Option<SlideLayout> {
        let k: String = s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_lowercase();
        Some(match k.as_str() {
            "title" | "titleslide" | "cover" => SlideLayout::Title,
            "titlecontent" | "titleandcontent" | "content" | "bullets" => SlideLayout::TitleContent,
            "section" | "sectionheader" | "divider" => SlideLayout::Section,
            "twocontent" | "twocolumns" | "twocolumn" | "comparison" => SlideLayout::TwoContent,
            "titleonly" => SlideLayout::TitleOnly,
            "blank" | "empty" => SlideLayout::Blank,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            SlideLayout::Title => "title",
            SlideLayout::TitleContent => "titleContent",
            SlideLayout::Section => "section",
            SlideLayout::TwoContent => "twoContent",
            SlideLayout::TitleOnly => "titleOnly",
            SlideLayout::Blank => "blank",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SlideLayout::Title => "Title slide",
            SlideLayout::TitleContent => "Title and content",
            SlideLayout::Section => "Section header",
            SlideLayout::TwoContent => "Two columns",
            SlideLayout::TitleOnly => "Title only",
            SlideLayout::Blank => "Blank",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ShapeKind {
    /// A text box (no fill unless given one).
    Text,
    Rect,
    Ellipse,
    Triangle,
    Line,
    Arrow,
    Image { media: Id },
    Chart { chart: Chart },
    Table { table: Table },
}

impl ShapeKind {
    pub fn id(&self) -> &'static str {
        match self {
            ShapeKind::Text => "text",
            ShapeKind::Rect => "rect",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Triangle => "triangle",
            ShapeKind::Line => "line",
            ShapeKind::Arrow => "arrow",
            ShapeKind::Image { .. } => "image",
            ShapeKind::Chart { .. } => "chart",
            ShapeKind::Table { .. } => "table",
        }
    }

    /// Plain geometric kinds from a name.
    pub fn parse_basic(s: &str) -> Option<ShapeKind> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "text" | "textbox" => ShapeKind::Text,
            "rect" | "rectangle" | "square" | "box" => ShapeKind::Rect,
            "ellipse" | "circle" | "oval" => ShapeKind::Ellipse,
            "triangle" => ShapeKind::Triangle,
            "line" => ShapeKind::Line,
            "arrow" => ShapeKind::Arrow,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub id: Id,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: ShapeKind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Degrees, clockwise.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f32,
    /// Fill `#rrggbb` (none when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// Outline colour `#rrggbb` (none when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub line_width: f32,
    /// Text inside the shape (paragraphs only).
    #[serde(default, skip_serializing_if = "imbl::Vector::is_empty")]
    pub text: Flow,
    /// The size Normal text has in this shape, in points (paragraph styles scale from it).
    #[serde(default = "default_text_size")]
    pub text_size: f32,
    #[serde(default)]
    pub valign: VAlign,
    /// `title`, `subtitle` or `body` when the shape comes from the slide's layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Text colour override `#rrggbb` (the theme's otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

fn default_text_size() -> f32 {
    20.0
}

impl Shape {
    pub fn new(kind: ShapeKind, x: f32, y: f32, w: f32, h: f32) -> Self {
        Shape { id: Id::new(), name: String::new(), kind, x, y, w, h, rotation: 0.0, fill: None, line: None, line_width: 0.0, text: Flow::new(), text_size: 20.0, valign: VAlign::Top, placeholder: None, color: None }
    }

    pub fn text_box(x: f32, y: f32, w: f32, h: f32, text: &str, size: f32) -> Self {
        let mut s = Shape::new(ShapeKind::Text, x, y, w, h);
        s.text = text.split('\n').map(|l| Block::Paragraph(Paragraph::new(ParaStyle::Normal, l))).collect();
        s.text_size = size;
        s
    }

    pub fn plain(&self) -> String {
        self.text.iter().map(Block::plain).collect::<Vec<_>>().join("\n")
    }

    /// Whether the shape holds editable text (text boxes and filled shapes).
    pub fn takes_text(&self) -> bool {
        matches!(self.kind, ShapeKind::Text | ShapeKind::Rect | ShapeKind::Ellipse | ShapeKind::Triangle)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slide {
    pub id: Id,
    #[serde(default)]
    pub layout: SlideLayout,
    #[serde(default)]
    pub shapes: Vec<Shape>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Background `#rrggbb` (the theme's otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// Skipped when presenting and exporting.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
}

impl Slide {
    /// A slide with its layout's placeholders, filled with `title` and `body` when given.
    pub fn with_layout(layout: SlideLayout, size: [f32; 2], title: &str, body: &str) -> Slide {
        let [w, h] = size;
        let m = w * 0.075;
        let mut shapes = vec![];
        let mut ph = |name: &str, x: f32, y: f32, sw: f32, sh: f32, text: &str, size: f32, style: ParaStyle, align: Align, valign: VAlign| {
            let mut s = Shape::new(ShapeKind::Text, x, y, sw, sh);
            s.placeholder = Some(name.into());
            s.name = match name {
                "title" => "Title".into(),
                "subtitle" => "Subtitle".into(),
                _ => "Body".into(),
            };
            s.text_size = size;
            s.valign = valign;
            let bullets = name == "body" && style == ParaStyle::Normal;
            s.text = text
                .split('\n')
                .map(|l| {
                    let mut p = Paragraph::new(style, l.trim_start_matches("- ").trim_start_matches("• "));
                    p.align = align;
                    if bullets && !l.is_empty() {
                        p.list = Some(crate::ListKind::Bullet);
                    }
                    Block::Paragraph(p)
                })
                .collect();
            shapes.push(s);
        };
        match layout {
            SlideLayout::Title => {
                ph("title", m, h * 0.30, w - 2.0 * m, h * 0.22, title, 22.0, ParaStyle::Title, Align::Left, VAlign::Bottom);
                ph("subtitle", m, h * 0.55, w - 2.0 * m, h * 0.16, body, 22.0, ParaStyle::Subtitle, Align::Left, VAlign::Top);
            }
            SlideLayout::TitleContent => {
                ph("title", m, h * 0.07, w - 2.0 * m, h * 0.16, title, 16.0, ParaStyle::Title, Align::Left, VAlign::Bottom);
                ph("body", m, h * 0.27, w - 2.0 * m, h * 0.64, body, 22.0, ParaStyle::Normal, Align::Left, VAlign::Top);
            }
            SlideLayout::Section => {
                ph("title", m, h * 0.36, w - 2.0 * m, h * 0.2, title, 22.0, ParaStyle::Title, Align::Left, VAlign::Bottom);
                ph("subtitle", m, h * 0.58, w - 2.0 * m, h * 0.12, body, 20.0, ParaStyle::Subtitle, Align::Left, VAlign::Top);
            }
            SlideLayout::TwoContent => {
                ph("title", m, h * 0.07, w - 2.0 * m, h * 0.16, title, 16.0, ParaStyle::Title, Align::Left, VAlign::Bottom);
                let (left, right) = body.split_once("\n\n").unwrap_or((body, ""));
                let cw = (w - 2.0 * m - m * 0.5) / 2.0;
                ph("body", m, h * 0.27, cw, h * 0.64, left, 20.0, ParaStyle::Normal, Align::Left, VAlign::Top);
                ph("body", m + cw + m * 0.5, h * 0.27, cw, h * 0.64, right, 20.0, ParaStyle::Normal, Align::Left, VAlign::Top);
            }
            SlideLayout::TitleOnly => {
                ph("title", m, h * 0.07, w - 2.0 * m, h * 0.16, title, 16.0, ParaStyle::Title, Align::Left, VAlign::Bottom);
            }
            SlideLayout::Blank => {}
        }
        Slide { id: Id::new(), layout, shapes, notes: String::new(), background: None, hidden: false }
    }

    pub fn title(&self) -> String {
        self.shapes.iter().find(|s| s.placeholder.as_deref() == Some("title")).map(Shape::plain).unwrap_or_default()
    }

    pub fn shape(&self, id: &str) -> Option<usize> {
        self.shapes.iter().position(|s| s.id == id || (!s.name.is_empty() && s.name.eq_ignore_ascii_case(id)))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deck {
    /// Slide size in points.
    #[serde(default = "default_size")]
    pub size: [f32; 2],
    #[serde(default)]
    pub theme: DeckTheme,
    pub slides: Vec<Slide>,
}

fn default_size() -> [f32; 2] {
    [960.0, 540.0]
}

impl Default for Deck {
    fn default() -> Self {
        let size = default_size();
        Deck { size, theme: DeckTheme::default(), slides: vec![Slide::with_layout(SlideLayout::Title, size, "", "")] }
    }
}

impl Deck {
    pub fn slide(&self, id: &str) -> Option<usize> {
        self.slides.iter().position(|s| s.id == id).or_else(|| id.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= self.slides.len()).map(|n| n - 1))
    }

    pub fn plain(&self) -> String {
        self.slides
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let body: Vec<String> = s.shapes.iter().map(Shape::plain).filter(|t| !t.is_empty()).collect();
                format!("Slide {}: {}", i + 1, body.join(" / "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A run of text in a given colour, for building slides from code.
pub fn colored(text: &str, color: &str) -> Run {
    Run::styled(text, crate::RunStyle { color: Some(color.into()), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_make_placeholders() {
        let s = Slide::with_layout(SlideLayout::TitleContent, [960.0, 540.0], "Plan", "One\nTwo");
        assert_eq!(s.title(), "Plan");
        assert_eq!(s.shapes.len(), 2);
        assert_eq!(s.shapes[1].text.len(), 2);
        assert_eq!(s.shapes[1].text[0].para().unwrap().list, Some(crate::ListKind::Bullet));
    }

    #[test]
    fn deck_json_round_trip() {
        let d = Deck::default();
        let v = serde_json::to_string(&d).unwrap();
        let back: Deck = serde_json::from_str(&v).unwrap();
        assert_eq!(back, d);
    }
}
