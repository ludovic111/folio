//! OpenDocument presentations (`.odp`, LibreOffice Impress): the common subset in and out.
//!
//! Slides with text frames (titles, subtitles and outlines become folio's placeholders),
//! rectangles, ellipses, triangles, arrows and lines, pictures, tables, speaker notes, slide
//! backgrounds and hidden slides. Text keeps sizes, bold, italic, underline, strike, colours,
//! fonts (as families), alignment and bullet or numbered lists. Charts go out as pictures when
//! folio can draw them and come in as nothing (a warning says so); animations and custom
//! shapes beyond the basic ones are left out.

use std::collections::HashMap;
use std::fmt::Write as _;

use folio_core::deck::VAlign;
use folio_core::text::{Flow, Table, TableCell};
use folio_core::{Align, Block, Deck, DeckTheme, Document, ListKind, PageKind, ParaStyle, Paragraph, Run, RunStyle, Shape, ShapeKind, Slide, SlideLayout};

use crate::ods::{length_pt, length_px};
use crate::xlsx::package::{El, Node, Package, ZipOut, esc, family_of, font_name, hex_color};
use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "odp",
    name: "OpenDocument presentation",
    extensions: &["odp"],
    kinds: &["deck"],
    import: true,
    export: true,
    apps: &["LibreOffice Impress", "Collabora", "Google Slides (File › Download › .odp)", "Microsoft PowerPoint"],
    notes: "Slides with text (sizes, styles, colours, alignment, lists), placeholders, rectangles, ellipses, triangles, arrows, lines, pictures, tables, speaker notes, backgrounds and hidden slides. Charts go out as pictures and don't come in; animations, transitions and other custom shapes are left out. PPTX keeps more.",
};

// ---------------------------------------------------------------------------------------------
// Reading

/// Styles by family and name, with their parents.
struct Styles<'a> {
    by_name: HashMap<(String, String), &'a El>,
    /// The default style per family.
    defaults: HashMap<String, &'a El>,
}

impl<'a> Styles<'a> {
    fn load(roots: &[&'a El]) -> Self {
        let mut by_name = HashMap::new();
        let mut defaults = HashMap::new();
        for root in roots {
            for holder in ["styles", "automatic-styles", "master-styles"] {
                let Some(h) = root.child(holder) else { continue };
                for st in h.elements() {
                    match st.name.as_str() {
                        "style" => {
                            if let (Some(f), Some(n)) = (st.attr_any("family"), st.attr_any("name")) {
                                by_name.insert((f.to_string(), n.to_string()), st);
                            }
                        }
                        "default-style" => {
                            if let Some(f) = st.attr_any("family") {
                                defaults.insert(f.to_string(), st);
                            }
                        }
                        "list-style" | "page-layout" | "master-page" => {
                            if let Some(n) = st.attr_any("name") {
                                by_name.insert((st.name.clone(), n.to_string()), st);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Styles { by_name, defaults }
    }

    /// A style and its parents, most specific first, then the family's default.
    fn chain(&self, family: &str, name: Option<&str>) -> Vec<&'a El> {
        let mut out = vec![];
        let mut at = name.map(str::to_string);
        while let Some(n) = at {
            let Some(st) = self.by_name.get(&(family.to_string(), n)) else { break };
            if out.len() > 10 {
                break;
            }
            out.push(*st);
            at = st.attr_any("parent-style-name").map(str::to_string);
        }
        if let Some(d) = self.defaults.get(family) {
            out.push(*d);
        }
        out
    }

    /// The first value of a property among chains (`text-properties`, `fo:font-size`).
    fn prop(chains: &[&[&'a El]], group: &str, key: &str) -> Option<&'a str> {
        chains.iter().flat_map(|c| c.iter()).find_map(|st| st.child(group).and_then(|g| g.attr(key)))
    }
}

/// Text style props in effect for a span.
#[derive(Clone, Debug, Default)]
struct TextProps {
    size: Option<f32>,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: Option<String>,
    font: Option<String>,
}

fn text_props(chains: &[&[&El]]) -> TextProps {
    let get = |k: &str| Styles::prop(chains, "text-properties", k);
    TextProps {
        size: get("fo:font-size").and_then(length_pt),
        bold: get("fo:font-weight").is_some_and(|w| w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600)),
        italic: get("fo:font-style").is_some_and(|s| s == "italic" || s == "oblique"),
        underline: get("style:text-underline-style").is_some_and(|u| u != "none"),
        strike: get("style:text-line-through-style").is_some_and(|u| u != "none"),
        color: get("fo:color").and_then(hex_color),
        font: get("fo:font-family").or(get("style:font-name")).map(|f| f.trim_matches('\'').to_string()),
    }
}

/// Position from `svg:x/y/width/height` or `draw:transform` (rotation in degrees, clockwise).
fn geometry(e: &El) -> Option<(f32, f32, f32, f32, f32)> {
    let g = |k: &str| e.attr(k).and_then(length_pt);
    let (w, h) = (g("svg:width").unwrap_or(0.0), g("svg:height").unwrap_or(0.0));
    if let Some(t) = e.attr("draw:transform") {
        let mut angle = 0.0f64;
        let mut tx = 0.0f64;
        let mut ty = 0.0f64;
        let mut rest = t;
        while let Some(open) = rest.find('(') {
            let name = rest[..open].trim().to_string();
            let close = rest[open..].find(')').map(|c| open + c)?;
            let args: Vec<f64> = rest[open + 1..close].split_whitespace().filter_map(|a| length_pt(a).map(|v| v as f64).or_else(|| a.parse().ok())).collect();
            match name.as_str() {
                "rotate" => angle = *args.first()?,
                "translate" => {
                    tx = *args.first()?;
                    ty = *args.get(1).unwrap_or(&0.0);
                }
                _ => {}
            }
            rest = &rest[close + 1..];
        }
        // The shape is rotated about its top-left corner then moved; folio keeps the unrotated box.
        let (hw, hh) = (w as f64 / 2.0, h as f64 / 2.0);
        let (s, c) = angle.sin_cos();
        let cx = hw * c + hh * s + tx;
        let cy = -hw * s + hh * c + ty;
        let deg = (-angle.to_degrees()).rem_euclid(360.0);
        return Some(((cx - hw) as f32, (cy - hh) as f32, w, h, deg as f32));
    }
    Some((g("svg:x").unwrap_or(0.0), g("svg:y").unwrap_or(0.0), w, h, 0.0))
}

struct Cx<'a, 'b> {
    styles: &'b Styles<'a>,
    text: String,
}

/// Paragraphs of a text container (`draw:text-box`, a custom shape, a notes frame).
fn read_paras(cx: &Cx, holder: &El, base_chain: &[&El], out: &mut Vec<(Paragraph, Vec<(String, TextProps)>)>, level: u8, list: Option<ListKind>) {
    for e in holder.elements() {
        match e.name.as_str() {
            "p" | "h" => {
                let pchain = cx.styles.chain("paragraph", e.attr_any("style-name"));
                let align = match Styles::prop(&[pchain.as_slice(), base_chain], "paragraph-properties", "fo:text-align") {
                    Some("center") => Align::Center,
                    Some("end" | "right") => Align::Right,
                    Some("justify") => Align::Justify,
                    _ => Align::Left,
                };
                let mut runs: Vec<(String, TextProps)> = vec![];
                let base = text_props(&[pchain.as_slice(), base_chain]);
                fn walk(cx: &Cx, e: &El, chains: &[&[&El]], runs: &mut Vec<(String, TextProps)>) {
                    let props = text_props(chains);
                    for k in &e.kids {
                        match k {
                            Node::Text(t) => runs.push((t.clone(), props.clone())),
                            Node::El(c) => match c.name.as_str() {
                                "s" => runs.push((" ".repeat(c.attr_any("c").and_then(|v| v.parse().ok()).unwrap_or(1)), props.clone())),
                                "tab" => runs.push(("\t".into(), props.clone())),
                                "line-break" => runs.push(("\n".into(), props.clone())),
                                "span" | "a" => {
                                    let sc = cx.styles.chain("text", c.attr_any("style-name"));
                                    let mut all: Vec<&[&El]> = vec![sc.as_slice()];
                                    all.extend_from_slice(chains);
                                    walk(cx, c, &all, runs);
                                }
                                "page-number" | "date" | "time" => runs.push((c.text(), props.clone())),
                                _ => {}
                            },
                        }
                    }
                }
                walk(cx, e, &[pchain.as_slice(), base_chain], &mut runs);
                if runs.is_empty() {
                    runs.push((String::new(), base));
                }
                let para = Paragraph { align, list, level: level.min(5), ..Default::default() };
                out.push((para, runs));
            }
            "list" => {
                // Numbered when the list style's first level numbers.
                let kind = e
                    .attr_any("style-name")
                    .and_then(|n| cx.styles.by_name.get(&("list-style".to_string(), n.to_string())))
                    .and_then(|ls| ls.elements().next())
                    .map(|l| if l.name == "list-level-style-number" { ListKind::Number } else { ListKind::Bullet })
                    .or(list)
                    .unwrap_or(ListKind::Bullet);
                // A list inside a list item goes one level down.
                let lvl = if list.is_some() { level + 1 } else { level };
                for item in e.elements().filter(|i| i.name == "list-item" || i.name == "list-header") {
                    read_paras(cx, item, base_chain, out, lvl, Some(kind));
                }
            }
            _ => {}
        }
    }
}

/// Paragraphs as a folio flow and the shape's text size and colour.
fn flow_of(cx: &Cx, paras: Vec<(Paragraph, Vec<(String, TextProps)>)>) -> (Flow, f32, Option<String>) {
    let first = paras.iter().flat_map(|(_, r)| r.iter()).find(|(t, _)| !t.trim().is_empty()).or_else(|| paras.iter().flat_map(|(_, r)| r.iter()).next());
    let size = first.and_then(|(_, p)| p.size).unwrap_or(18.0).max(1.0);
    let color = first.and_then(|(_, p)| p.color.clone());
    let mut flow = Flow::new();
    for (mut para, runs) in paras {
        para.runs = runs
            .into_iter()
            .filter(|(t, _)| !t.is_empty())
            .map(|(t, p)| {
                let fam = p.font.as_deref().map(family_of).unwrap_or("sans");
                let s = p.size.unwrap_or(size);
                Run::styled(
                    t,
                    RunStyle {
                        bold: p.bold,
                        italic: p.italic,
                        underline: p.underline,
                        strike: p.strike,
                        color: p.color.filter(|c| Some(c) != color.as_ref()),
                        size: ((s - size).abs() > 0.05).then_some(s * 11.0 / size),
                        font: (fam != "sans").then(|| fam.to_string()),
                        ..Default::default()
                    },
                )
            })
            .collect();
        para.normalize();
        flow.push_back(Block::Paragraph(para));
    }
    if flow.iter().all(|b| b.plain().is_empty()) {
        flow = Flow::new();
    }
    (flow, size, color.filter(|c| *c != cx.text))
}

struct Reader<'p, 'b> {
    pkg: &'p mut Package<'b>,
    doc: Document,
    media: HashMap<String, folio_core::Id>,
    charts: usize,
    other: usize,
}

impl Reader<'_, '_> {
    fn shapes(&mut self, cx: &Cx, holder: &El, out: &mut Vec<Shape>) {
        for e in holder.elements() {
            if let Some(s) = self.shape(cx, e, out) {
                out.push(s);
            }
        }
    }

    fn shape(&mut self, cx: &Cx, e: &El, out: &mut Vec<Shape>) -> Option<Shape> {
        let gchain = cx.styles.chain("graphic", e.attr("draw:style-name"));
        let pchain = cx.styles.chain("presentation", e.attr("presentation:style-name"));
        let chains: [&[&El]; 2] = [pchain.as_slice(), gchain.as_slice()];
        let gp = |k: &str| Styles::prop(&chains, "graphic-properties", k);
        let fill = match gp("draw:fill") {
            Some("solid") => gp("draw:fill-color").and_then(hex_color),
            Some("none") => None,
            Some("gradient") => gp("draw:fill-color").and_then(hex_color),
            _ => None,
        };
        let line = match gp("draw:stroke") {
            Some("none") => None,
            Some(_) => gp("svg:stroke-color").and_then(hex_color).or(Some("#000000".into())),
            None => None,
        };
        let line_width = gp("svg:stroke-width").and_then(length_pt).unwrap_or(0.75).max(0.25);
        let valign = match gp("draw:textarea-vertical-align") {
            Some("middle") => VAlign::Middle,
            Some("bottom") => VAlign::Bottom,
            _ => VAlign::Top,
        };
        let name = e.attr("draw:name").unwrap_or("").to_string();
        let text_chain: Vec<&El> = pchain.iter().chain(gchain.iter()).copied().collect();
        let text_of = |holder: &El| {
            let mut paras = vec![];
            read_paras(cx, holder, &text_chain, &mut paras, 0, None);
            flow_of(cx, paras)
        };
        let finish = |mut s: Shape, holder: Option<&El>, rot: f32| {
            s.name = name.clone();
            s.rotation = rot;
            if let Some(h) = holder {
                let (flow, size, color) = text_of(h);
                s.text = flow;
                s.text_size = size;
                s.color = color;
            }
            s.valign = valign;
            s
        };
        match e.name.as_str() {
            "g" => {
                self.shapes(cx, e, out);
                None
            }
            "frame" => {
                let (x, y, w, h, rot) = geometry(e)?;
                if let Some(tb) = e.child("text-box") {
                    let mut s = finish(Shape::new(ShapeKind::Text, x, y, w, h), Some(tb), rot);
                    s.placeholder = match e.attr("presentation:class") {
                        Some("title") => Some("title".into()),
                        Some("subtitle") => Some("subtitle".into()),
                        Some("outline") => Some("body".into()),
                        _ => None,
                    };
                    if s.placeholder.as_deref() == Some("body") {
                        // Outlines are bulleted unless they say otherwise.
                        for b in s.text.iter_mut() {
                            if let Block::Paragraph(p) = b
                                && p.list.is_none()
                                && !p.is_empty()
                                && e.find("list").is_some()
                            {
                                p.list = Some(ListKind::Bullet);
                            }
                        }
                    }
                    s.fill = fill;
                    s.line = line;
                    s.line_width = if s.line.is_some() { line_width } else { 0.0 };
                    return Some(s);
                }
                // A native table takes precedence over LibreOffice's preview image.
                if let Some(t) = e.child("table") {
                    let mut widths = vec![];
                    for c in t.children("table-column") {
                        let wpx = cx.styles.chain("table-column", c.attr_any("style-name")).iter().find_map(|s| s.child("table-column-properties").and_then(|p| p.attr("style:column-width")).and_then(length_px)).unwrap_or(100.0);
                        for _ in 0..c.attr_any("number-columns-repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(64) {
                            widths.push(wpx);
                        }
                    }
                    let mut size = None;
                    let rows: Vec<Vec<TableCell>> = t
                        .all("table-row")
                        .iter()
                        .map(|r| {
                            r.children("table-cell")
                                .map(|c| {
                                    let mut paras = vec![];
                                    read_paras(cx, c, &[], &mut paras, 0, None);
                                    let mut runs = vec![];
                                    let mut align = Align::Left;
                                    for (i, (p, rs)) in paras.into_iter().enumerate() {
                                        if i > 0 {
                                            runs.push(Run::plain("\n"));
                                        }
                                        align = p.align;
                                        for (t, pr) in rs {
                                            if size.is_none() {
                                                size = pr.size;
                                            }
                                            runs.push(Run::styled(t, RunStyle { bold: pr.bold, italic: pr.italic, color: pr.color.filter(|c| *c != cx.text), ..Default::default() }));
                                        }
                                    }
                                    let fill = cx.styles.chain("table-cell", c.attr_any("style-name")).iter().find_map(|s| s.child("graphic-properties").or(s.child("table-cell-properties")).and_then(|p| p.attr("draw:fill-color").or(p.attr("fo:background-color"))).and_then(hex_color));
                                    let p = Paragraph::with_runs(ParaStyle::Normal, runs);
                                    TableCell { runs: p.runs, align, fill }
                                })
                                .collect()
                        })
                        .collect();
                    let mut table = Table::from_text(vec![], t.attr_any("use-first-row-styles") == Some("true"));
                    table.rows = if rows.is_empty() { vec![vec![TableCell::default()]] } else { rows };
                    table.widths = widths;
                    table.banded = t.attr_any("use-banding-rows-styles") == Some("true");
                    let mut s = finish(Shape::new(ShapeKind::Table { table }, x, y, w, h), None, rot);
                    s.text_size = size.unwrap_or(18.0);
                    return Some(s);
                }
                if let Some(img) = e.child("image")
                    && let Some(href) = img.attr("xlink:href")
                {
                    let path = href.trim_start_matches("./").to_string();
                    let media = match self.media.get(&path) {
                        Some(m) => m.clone(),
                        None => {
                            let bytes = self.pkg.read(&path)?;
                            if folio_core::Media::sniff(&bytes) == "application/octet-stream" {
                                self.other += 1;
                                return None;
                            }
                            let id = self.doc.add_media(path.rsplit('/').next().unwrap_or("picture"), bytes);
                            self.media.insert(path, id.clone());
                            id
                        }
                    };
                    if e.child("object").is_some() {
                        self.charts += 1;
                    }
                    return Some(finish(Shape::new(ShapeKind::Image { media }, x, y, w, h), None, rot));
                }
                if e.child("object").is_some() {
                    self.charts += 1;
                } else {
                    self.other += 1;
                }
                None
            }
            "custom-shape" | "rect" | "ellipse" | "circle" => {
                let (x, y, w, h, rot) = geometry(e)?;
                let ty = e.child("enhanced-geometry").and_then(|g| g.attr("draw:type")).unwrap_or(match e.name.as_str() {
                    "ellipse" | "circle" => "ellipse",
                    _ => "rectangle",
                });
                let kind = match ty.trim_start_matches("ooxml-") {
                    "rectangle" | "rect" | "round-rectangle" | "roundRect" | "flowchart-process" | "square" | "round-square" => ShapeKind::Rect,
                    "ellipse" | "circle" | "flowchart-connector" => ShapeKind::Ellipse,
                    "isosceles-triangle" | "triangle" | "right-triangle" | "rtTriangle" => ShapeKind::Triangle,
                    "right-arrow" | "rightArrow" | "left-arrow" | "leftArrow" | "up-arrow" | "down-arrow" | "chevron" | "pentagon-right" => ShapeKind::Arrow,
                    _ => {
                        self.other += 1;
                        ShapeKind::Rect
                    }
                };
                let mut s = finish(Shape::new(kind, x, y, w, h), Some(e), rot);
                s.fill = fill;
                s.line = line;
                s.line_width = if s.line.is_some() { line_width } else { 0.0 };
                Some(s)
            }
            "line" | "connector" => {
                let g = |k: &str| e.attr(k).and_then(length_pt).unwrap_or(0.0);
                let (x1, y1, x2, y2) = (g("svg:x1"), g("svg:y1"), g("svg:x2"), g("svg:y2"));
                let head = gp("draw:marker-end").or(gp("draw:marker-start")).is_some_and(|m| !m.is_empty());
                let mut s = finish(Shape::new(if head { ShapeKind::Arrow } else { ShapeKind::Line }, x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs()), None, 0.0);
                s.line = line.or(Some(cx.text.clone()));
                s.line_width = line_width;
                Some(s)
            }
            _ => None,
        }
    }
}

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    let mut pkg = Package::open(bytes).map_err(|_| "This file isn't an OpenDocument presentation (it isn't a zip package).".to_string())?;
    if let Some(m) = pkg.read("mimetype")
        && !String::from_utf8_lossy(&m).contains("presentation")
    {
        return Err(format!("This OpenDocument file isn't a presentation ({}).", String::from_utf8_lossy(&m).trim()));
    }
    let content = pkg.xml("content.xml").ok_or("This file isn't an OpenDocument presentation (no content.xml).")?;
    let styles_x = pkg.xml("styles.xml").unwrap_or_default();
    let styles = Styles::load(&[&content, &styles_x]);
    let pres = content.path(&["body", "presentation"]).ok_or("This file has no presentation body.")?;
    // Page size and the master's background.
    let masters = styles_x.child("master-styles");
    let master = masters.and_then(|m| m.child("master-page"));
    let size = master
        .and_then(|m| m.attr_any("page-layout-name"))
        .and_then(|n| styles.by_name.get(&("page-layout".to_string(), n.to_string())))
        .and_then(|pl| pl.child("page-layout-properties"))
        .map(|p| [p.attr("fo:page-width").and_then(length_pt).unwrap_or(960.0), p.attr("fo:page-height").and_then(length_pt).unwrap_or(540.0)])
        .unwrap_or([960.0, 540.0]);
    let page_fill = |name: Option<&str>| -> Option<String> {
        let chain = styles.chain("drawing-page", name);
        let get = |k: &str| Styles::prop(&[chain.as_slice()], "drawing-page-properties", k);
        (get("draw:fill") == Some("solid")).then(|| get("draw:fill-color").and_then(hex_color)).flatten()
    };
    let master_bg = page_fill(master.and_then(|m| m.attr("draw:style-name"))).unwrap_or_else(|| "#ffffff".into());
    let text = styles.defaults.get("graphic").and_then(|d| d.child("text-properties")).and_then(|t| t.attr("fo:color")).and_then(hex_color).unwrap_or_else(|| "#000000".into());
    let title_font = styles.by_name.iter().find(|((f, n), _)| f == "presentation" && n.ends_with("-title")).and_then(|(_, st)| st.child("text-properties")).and_then(|t| t.attr("fo:font-family")).map(|f| family_of(f.trim_matches('\'')).to_string());
    let body_font = styles.by_name.iter().find(|((f, n), _)| f == "presentation" && n.ends_with("-outline1")).and_then(|(_, st)| st.child("text-properties")).and_then(|t| t.attr("fo:font-family")).map(|f| family_of(f.trim_matches('\'')).to_string());
    let theme = DeckTheme {
        name: "Imported".into(),
        background: master_bg.clone(),
        text: text.clone(),
        accent: DeckTheme::default().accent,
        heading_font: title_font.unwrap_or_else(|| "sans".into()),
        body_font: body_font.unwrap_or_else(|| "sans".into()),
    };
    let cx = Cx { styles: &styles, text: text.clone() };
    let mut reader = Reader { pkg: &mut pkg, doc: Document::empty(title), media: HashMap::new(), charts: 0, other: 0 };
    let mut slides = vec![];
    let mut animations = 0;
    for page in pres.children("page") {
        let mut shapes = vec![];
        for e in page.elements() {
            if e.name == "notes" || e.name == "forms" {
                continue;
            }
            if e.name == "animations" {
                animations += 1;
                continue;
            }
            if let Some(s) = reader.shape(&cx, e, &mut shapes) {
                shapes.push(s);
            }
        }
        let notes = page
            .child("notes")
            .and_then(|n| n.elements().find(|f| f.attr("presentation:class") == Some("notes")))
            .and_then(|f| f.child("text-box"))
            .map(|tb| {
                let mut paras = vec![];
                read_paras(&cx, tb, &[], &mut paras, 0, None);
                paras.iter().map(|(_, r)| r.iter().map(|(t, _)| t.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n").trim().to_string()
            })
            .unwrap_or_default();
        let dp = styles.chain("drawing-page", page.attr("draw:style-name"));
        let hidden = Styles::prop(&[dp.as_slice()], "drawing-page-properties", "presentation:visibility") == Some("hidden");
        let bg = page_fill(page.attr("draw:style-name")).filter(|c| *c != master_bg);
        let count = |n: &str| shapes.iter().filter(|s: &&Shape| s.placeholder.as_deref() == Some(n)).count();
        let layout = match (count("title"), count("subtitle"), count("body")) {
            (_, s, _) if s > 0 => SlideLayout::Title,
            (_, _, b) if b >= 2 => SlideLayout::TwoContent,
            (_, _, 1) => SlideLayout::TitleContent,
            (t, _, _) if t > 0 => SlideLayout::TitleOnly,
            _ => SlideLayout::Blank,
        };
        slides.push(Slide { id: folio_core::Id::new(), layout, shapes, notes, background: bg, hidden });
    }
    if slides.is_empty() {
        return Err("This presentation has no slides.".into());
    }
    let Reader { mut doc, charts, other, .. } = reader;
    let i = doc.add_page(PageKind::Deck, Some(&crate::xlsx::safe_sheet_name(title, &[])), Some(0)).map_err(|e| e.0)?;
    *doc.page_mut(i).deck_mut().unwrap() = Deck { size, theme, slides };
    let mut warnings = vec![];
    if charts > 0 {
        warnings.push(format!("{charts} charts became their pictures (folio reads ODP charts' numbers only from PPTX for now)."));
    }
    if other > 0 {
        warnings.push(format!("{other} shapes or objects folio doesn't draw were left out or became rectangles."));
    }
    if animations > 0 {
        warnings.push("Animations were left out.".into());
    }
    Ok(Imported { doc, warnings, format: "odp" })
}

// ---------------------------------------------------------------------------------------------
// Writing

const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:smil="urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0" office:version="1.3""#;

fn pt(v: f32) -> String {
    format!("{:.2}pt", v)
}

/// Automatic styles, deduplicated by their XML.
#[derive(Default)]
struct Auto {
    list: Vec<(String, String)>,
    xml: String,
}

impl Auto {
    fn get(&mut self, prefix: &str, family: &str, body: &str) -> String {
        let key = format!("{family}|{body}");
        if let Some((_, n)) = self.list.iter().find(|(k, _)| *k == key) {
            return n.clone();
        }
        let n = format!("{prefix}{}", self.list.len() + 1);
        let _ = write!(self.xml, r#"<style:style style:name="{n}" style:family="{family}">{body}</style:style>"#);
        self.list.push((key, n.clone()));
        n
    }
}

struct W<'a> {
    doc: &'a Document,
    auto: Auto,
    pictures: Vec<(String, String)>,
    media_done: HashMap<String, String>,
    charts_out: usize,
}

impl W<'_> {
    fn paras(&mut self, flow: &Flow, theme: &DeckTheme, scale: f32, color: Option<&str>, title: bool) -> String {
        let mut out = String::new();
        let mut open_lists: Vec<u8> = vec![];
        let paras: Vec<Paragraph> = flow.iter().map(|b| b.para().cloned().unwrap_or_else(|| Paragraph::new(ParaStyle::Normal, b.plain()))).collect();
        for p in &paras {
            let spec = p.style.spec();
            let align = match p.align {
                Align::Left => "start",
                Align::Center => "center",
                Align::Right => "end",
                Align::Justify => "justify",
            };
            let pstyle = self.auto.get(
                "P",
                "paragraph",
                &format!(
                    r#"<style:paragraph-properties fo:text-align="{align}" fo:margin-top="{}" fo:margin-bottom="{}" fo:line-height="{}%"/>"#,
                    pt(spec.space_before * scale),
                    pt(spec.space_after * scale),
                    (spec.line / 1.2 * 100.0).round()
                ),
            );
            // Lists: open or close to this paragraph's level.
            let want = p.list.map(|_| p.level as usize + 1).unwrap_or(0);
            while open_lists.len() > want {
                open_lists.pop();
                out.push_str("</text:list-item></text:list>");
            }
            if want > 0 && open_lists.len() == want {
                out.push_str("</text:list-item><text:list-item>");
            }
            while open_lists.len() < want {
                let ls = if p.list == Some(ListKind::Number) { "LN" } else { "LB" };
                let _ = write!(out, r#"<text:list text:style-name="{ls}"><text:list-item>"#);
                open_lists.push(p.level);
            }
            let _ = write!(out, r#"<text:p text:style-name="{pstyle}">"#);
            for r in &p.runs {
                let st = &r.style;
                let size = st.size.unwrap_or(spec.size) * scale;
                let fam = if st.code { "mono".to_string() } else { st.font.clone().unwrap_or_else(|| if title { theme.heading_font.clone() } else { theme.body_font.clone() }) };
                let mut tp = format!(r#" fo:font-size="{}" fo:font-family="'{}'""#, pt(size), esc(&font_name(&fam)));
                if st.bold || spec.bold {
                    tp.push_str(r#" fo:font-weight="bold""#);
                }
                if st.italic || spec.italic {
                    tp.push_str(r#" fo:font-style="italic""#);
                }
                if st.underline {
                    tp.push_str(r#" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color""#);
                }
                if st.strike {
                    tp.push_str(r#" style:text-line-through-style="solid""#);
                }
                if let Some(c) = st.color.as_deref().or(color) {
                    let _ = write!(tp, r#" fo:color="{c}""#);
                } else if spec.muted {
                    let _ = write!(tp, r#" fo:color="{}""#, crate::xlsx::package::mix(&theme.text, &theme.background, 0.38));
                }
                let tstyle = self.auto.get("T", "text", &format!("<style:text-properties{tp}/>"));
                for (i, piece) in r.text.split('\n').enumerate() {
                    if i > 0 {
                        out.push_str("<text:line-break/>");
                    }
                    if !piece.is_empty() {
                        let _ = write!(out, r#"<text:span text:style-name="{tstyle}">{}</text:span>"#, esc(piece));
                    }
                }
            }
            out.push_str("</text:p>");
        }
        while open_lists.pop().is_some() {
            out.push_str("</text:list-item></text:list>");
        }
        out
    }

    fn picture(&mut self, id: &folio_core::Id) -> Option<String> {
        if let Some(p) = self.media_done.get(id.as_str()) {
            return Some(p.clone());
        }
        let m = self.doc.media.get(id)?;
        let path = format!("Pictures/{}.{}", m.id, m.ext());
        self.pictures.push((path.clone(), m.mime.clone()));
        self.media_done.insert(id.to_string(), path.clone());
        Some(path)
    }

    fn shape(&mut self, s: &Shape, theme: &DeckTheme, out: &mut String, zip: &mut ZipOut) {
        let name = esc(&s.name);
        let pos = if s.rotation != 0.0 {
            // Rotated about the top-left corner, then moved so the centre stays put.
            let a = -(s.rotation as f64).to_radians();
            let (hw, hh) = (s.w as f64 / 2.0, s.h as f64 / 2.0);
            let (sn, c) = a.sin_cos();
            let tx = s.x as f64 + hw - (hw * c + hh * sn);
            let ty = s.y as f64 + hh - (-hw * sn + hh * c);
            format!(r#" svg:width="{}" svg:height="{}" draw:transform="rotate ({a}) translate ({} {})""#, pt(s.w), pt(s.h), pt(tx as f32), pt(ty as f32))
        } else {
            format!(r#" svg:x="{}" svg:y="{}" svg:width="{}" svg:height="{}""#, pt(s.x), pt(s.y), pt(s.w), pt(s.h))
        };
        let valign = match s.valign {
            VAlign::Top => "top",
            VAlign::Middle => "middle",
            VAlign::Bottom => "bottom",
        };
        let fill = match &s.fill {
            Some(c) => format!(r#"draw:fill="solid" draw:fill-color="{c}""#),
            None => r#"draw:fill="none""#.to_string(),
        };
        let stroke = |head: bool| match &s.line {
            Some(c) => format!(r#"draw:stroke="solid" svg:stroke-color="{c}" svg:stroke-width="{}"{}"#, pt(s.line_width.max(0.25)), if head { r#" draw:marker-end="Arrow" draw:marker-end-width="0.3cm""# } else { "" }),
            None => r#"draw:stroke="none""#.to_string(),
        };
        let gstyle = |auto: &mut Auto, head: bool| {
            auto.get(
                "gr",
                "graphic",
                &format!(
                    r#"<style:graphic-properties {fill} {} draw:textarea-vertical-align="{valign}" draw:auto-grow-height="false" fo:padding-top="8pt" fo:padding-bottom="8pt" fo:padding-left="8pt" fo:padding-right="8pt" fo:wrap-option="wrap"/>"#,
                    stroke(head)
                ),
            )
        };
        let scale = s.text_size / 11.0;
        let title = s.placeholder.as_deref() == Some("title");
        match &s.kind {
            ShapeKind::Text => {
                let g = gstyle(&mut self.auto, false);
                let class = match s.placeholder.as_deref() {
                    Some("title") => r#" presentation:class="title" presentation:user-transformed="true""#,
                    Some("subtitle") => r#" presentation:class="subtitle" presentation:user-transformed="true""#,
                    Some("body") => r#" presentation:class="outline" presentation:user-transformed="true""#,
                    _ => "",
                };
                let body = self.paras(&s.text, theme, scale, s.color.as_deref(), title);
                let _ = write!(out, r#"<draw:frame draw:name="{name}" draw:style-name="{g}"{pos}{class}><draw:text-box>{body}</draw:text-box></draw:frame>"#);
            }
            ShapeKind::Rect | ShapeKind::Ellipse | ShapeKind::Triangle | ShapeKind::Arrow if s.kind != ShapeKind::Arrow || s.fill.is_some() => {
                let g = gstyle(&mut self.auto, false);
                let ty = match s.kind {
                    ShapeKind::Ellipse => "ellipse",
                    ShapeKind::Triangle => "isosceles-triangle",
                    ShapeKind::Arrow => "right-arrow",
                    _ => "rectangle",
                };
                let body = self.paras(&s.text, theme, scale, s.color.as_deref(), false);
                let _ = write!(out, r#"<draw:custom-shape draw:name="{name}" draw:style-name="{g}"{pos}>{body}<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="{ty}"/></draw:custom-shape>"#);
            }
            ShapeKind::Rect | ShapeKind::Ellipse | ShapeKind::Triangle | ShapeKind::Line | ShapeKind::Arrow => {
                let mut l = s.clone();
                if l.line.is_none() {
                    l.line = Some(theme.text.clone());
                    l.line_width = 1.5;
                }
                let stroke_l = match &l.line {
                    Some(c) => format!(r#"draw:stroke="solid" svg:stroke-color="{c}" svg:stroke-width="{}"{}"#, pt(l.line_width.max(0.25)), if s.kind == ShapeKind::Arrow { r#" draw:marker-end="Arrow" draw:marker-end-width="0.3cm""# } else { "" }),
                    None => String::new(),
                };
                let g = self.auto.get("gr", "graphic", &format!(r#"<style:graphic-properties draw:fill="none" {stroke_l}/>"#));
                let _ = write!(out, r#"<draw:line draw:name="{name}" draw:style-name="{g}" svg:x1="{}" svg:y1="{}" svg:x2="{}" svg:y2="{}"/>"#, pt(s.x), pt(s.y), pt(s.x + s.w), pt(s.y + s.h));
            }
            ShapeKind::Image { media } => {
                let Some(path) = self.picture(media) else { return };
                let g = self.auto.get("gr", "graphic", r#"<style:graphic-properties draw:fill="none" draw:stroke="none"/>"#);
                let _ = write!(out, r#"<draw:frame draw:name="{name}" draw:style-name="{g}"{pos}><draw:image xlink:href="{path}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#);
            }
            ShapeKind::Table { table } => {
                let rows: Vec<Vec<String>> = match &table.link {
                    Some(l) => folio_core::links::table_text(self.doc, l).unwrap_or_default(),
                    None => table.rows.iter().map(|r| r.iter().map(|c| c.plain()).collect()).collect(),
                };
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                let fr = if table.widths.len() == cols { table.fractions() } else { vec![1.0 / cols as f32; cols] };
                let _ = write!(out, r#"<draw:frame draw:name="{name}"{pos}><table:table table:use-first-row-styles="{}" table:use-banding-rows-styles="{}">"#, table.header, table.banded);
                for f in &fr {
                    let cs = self.auto.get("co", "table-column", &format!(r#"<style:table-column-properties style:column-width="{}"/>"#, pt(s.w * f)));
                    let _ = write!(out, r#"<table:table-column table:style-name="{cs}"/>"#);
                }
                let rule = crate::xlsx::package::mix(&theme.text, &theme.background, 0.7);
                for (ri, row) in rows.iter().enumerate() {
                    out.push_str("<table:table-row>");
                    for c in 0..cols {
                        let cell = table.cell(ri, c);
                        let mut p = match cell {
                            Some(tc) if table.link.is_none() => tc.as_para(),
                            _ => Paragraph::new(ParaStyle::Normal, row.get(c).cloned().unwrap_or_default()),
                        };
                        p.align = cell.map(|c| c.align).unwrap_or(Align::Left);
                        if table.header && ri == 0 {
                            for r in &mut p.runs {
                                r.style.bold = true;
                            }
                        }
                        let fill = match cell.and_then(|c| c.fill.as_deref()) {
                            Some(f) => format!(r#"draw:fill="solid" draw:fill-color="{f}""#),
                            None => r#"draw:fill="none""#.into(),
                        };
                        let cs = self.auto.get("ce", "table-cell", &format!(r#"<style:graphic-properties {fill}/><style:paragraph-properties fo:border="0.5pt solid {rule}"/>"#));
                        let flow: Flow = std::iter::once(Block::Paragraph(p)).collect();
                        let body = self.paras(&flow, theme, scale, s.color.as_deref(), false);
                        let _ = write!(out, r#"<table:table-cell table:style-name="{cs}">{body}</table:table-cell>"#);
                    }
                    out.push_str("</table:table-row>");
                }
                out.push_str("</table:table></draw:frame>");
            }
            ShapeKind::Chart { chart } => {
                let png = folio_core::links::chart_data(self.doc, chart).map(|d| folio_layout::raster::chart_png(chart, &d, (s.w * 2.0) as u32, (s.h * 2.0) as u32, &folio_layout::chart::ChartStyle::default())).unwrap_or_default();
                if png.is_empty() {
                    self.charts_out += 1;
                    return;
                }
                self.charts_out += 1;
                let path = format!("Pictures/chart{}.png", self.pictures.len() + 1);
                if zip.add(&path, &png).is_err() {
                    return;
                }
                self.pictures.push((path.clone(), "image/png".into()));
                let g = self.auto.get("gr", "graphic", r#"<style:graphic-properties draw:fill="none" draw:stroke="none"/>"#);
                let _ = write!(out, r#"<draw:frame draw:name="{name}" draw:style-name="{g}"{pos}><draw:image xlink:href="{path}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#);
            }
        }
    }
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let (keep, mut warnings) = crate::pages_of_kind(doc, pages, PageKind::Deck, "ODP");
    if keep.is_empty() {
        return Err("Nothing to write: an OpenDocument presentation holds slides, and none of the chosen pages is a deck.".into());
    }
    let decks: Vec<&Deck> = keep.iter().map(|i| doc.pages[*i].deck().unwrap()).collect();
    let first = decks[0];
    let theme = &first.theme;
    let mut zip = ZipOut::new();
    zip.stored("mimetype", "application/vnd.oasis.opendocument.presentation")?;
    let mut w = W { doc, auto: Auto::default(), pictures: vec![], media_done: HashMap::new(), charts_out: 0 };
    let mut body = String::new();
    let mut n = 0;
    for deck in &decks {
        for slide in &deck.slides {
            n += 1;
            let mut dp = String::from("<style:drawing-page-properties");
            match slide.background.as_deref().or((deck.theme.background != theme.background).then_some(deck.theme.background.as_str())) {
                Some(c) => {
                    let _ = write!(dp, r#" draw:fill="solid" draw:fill-color="{c}" draw:background-size="full""#);
                }
                None => dp.push_str(r#" draw:background-size="full""#),
            }
            if slide.hidden {
                dp.push_str(r#" presentation:visibility="hidden""#);
            }
            dp.push_str("/>");
            let dps = w.auto.get("dp", "drawing-page", &dp);
            let _ = write!(body, r#"<draw:page draw:name="page{n}" draw:style-name="{dps}" draw:master-page-name="Default">"#);
            let mut shapes = String::new();
            for s in &slide.shapes {
                w.shape(s, &deck.theme, &mut shapes, &mut zip);
            }
            body.push_str(&shapes);
            if !slide.notes.trim().is_empty() {
                let paras: String = slide.notes.split('\n').map(|l| format!("<text:p>{}</text:p>", esc(l))).collect();
                let _ = write!(
                    body,
                    r#"<presentation:notes><draw:page-thumbnail svg:x="2cm" svg:y="2cm" svg:width="17cm" svg:height="9.6cm" draw:page-number="{n}" presentation:class="page"/><draw:frame svg:x="2cm" svg:y="12.5cm" svg:width="17cm" svg:height="13cm" presentation:class="notes"><draw:text-box>{paras}</draw:text-box></draw:frame></presentation:notes>"#
                );
            }
            body.push_str("</draw:page>");
        }
    }
    // Pictures.
    let mut manifest_pics = String::new();
    for (path, mime) in w.pictures.clone() {
        if path.contains("/chart") {
            // Already written.
        } else if let Some(m) = doc.media.values().find(|m| format!("Pictures/{}.{}", m.id, m.ext()) == path) {
            zip.add(&path, m.bytes.as_slice())?;
        }
        let _ = write!(manifest_pics, r#"<manifest:file-entry manifest:full-path="{path}" manifest:media-type="{mime}"/>"#);
    }
    let lists = r#"<text:list-style style:name="LB"><text:list-level-style-bullet text:level="1" text:bullet-char="•"><style:list-level-properties text:space-before="0cm" text:min-label-width="0.6cm"/></text:list-level-style-bullet><text:list-level-style-bullet text:level="2" text:bullet-char="–"><style:list-level-properties text:space-before="0.8cm" text:min-label-width="0.6cm"/></text:list-level-style-bullet><text:list-level-style-bullet text:level="3" text:bullet-char="•"><style:list-level-properties text:space-before="1.6cm" text:min-label-width="0.6cm"/></text:list-level-style-bullet></text:list-style><text:list-style style:name="LN"><text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix="."><style:list-level-properties text:space-before="0cm" text:min-label-width="0.7cm"/></text:list-level-style-number><text:list-level-style-number text:level="2" style:num-format="a" style:num-suffix="."><style:list-level-properties text:space-before="0.8cm" text:min-label-width="0.7cm"/></text:list-level-style-number></text:list-style>"#;
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS}><office:automatic-styles>{}{lists}</office:automatic-styles><office:body><office:presentation>{body}</office:presentation></office:body></office:document-content>"#,
        w.auto.xml
    );
    let [pw, ph] = first.size;
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS}><office:styles><draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/><style:default-style style:family="graphic"><style:graphic-properties draw:fill="none" draw:stroke="none"/><style:text-properties fo:color="{}" fo:font-family="'{}'" fo:font-size="18pt"/></style:default-style></office:styles><office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="0cm" fo:margin-bottom="0cm" fo:margin-left="0cm" fo:margin-right="0cm" fo:page-width="{}" fo:page-height="{}" style:print-orientation="landscape"/></style:page-layout><style:style style:name="Mdp1" style:family="drawing-page"><style:drawing-page-properties draw:background-size="full" draw:fill="solid" draw:fill-color="{}"/></style:style></office:automatic-styles><office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1" draw:style-name="Mdp1"/></office:master-styles></office:document-styles>"#,
        theme.text,
        esc(&font_name(&theme.body_font)),
        pt(pw),
        pt(ph),
        theme.background
    );
    let meta = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta {NS}><office:meta><meta:generator>folio</meta:generator><dc:title>{}</dc:title></office:meta></office:document-meta>"#,
        esc(&doc.title)
    );
    let manifest = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="application/vnd.oasis.opendocument.presentation"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>{manifest_pics}</manifest:manifest>"#
    );
    zip.add("META-INF/manifest.xml", manifest)?;
    zip.add("content.xml", content)?;
    zip.add("styles.xml", styles)?;
    zip.add("meta.xml", meta)?;
    if w.charts_out > 0 {
        warnings.push(format!("{} charts were written as pictures (or left out where folio can't draw them yet); they no longer follow the sheet.", w.charts_out));
    }
    Ok((zip.finish()?, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECK_ODP: &[u8] = include_bytes!("../tests/fixtures/deck.odp");

    #[test]
    fn libreoffice_deck() {
        let im = import(DECK_ODP, "deck").unwrap();
        let deck = im.doc.pages[0].deck().unwrap();
        assert_eq!(deck.slides.len(), 4);
        assert!((deck.size[0] - 960.0).abs() < 1.0, "{:?}", deck.size);
        let s1 = &deck.slides[0];
        assert_eq!(s1.title(), "Quarterly review");
        assert_eq!(s1.notes, "Welcome everyone.");
        assert_eq!(s1.layout, SlideLayout::Title);
        let s2 = &deck.slides[1];
        let body = s2.shapes.iter().find(|s| s.placeholder.as_deref() == Some("body")).unwrap();
        let paras: Vec<&Paragraph> = body.text.iter().filter_map(Block::para).collect();
        assert_eq!(paras[0].text(), "Revenue up 12%");
        assert_eq!(paras[0].list, Some(ListKind::Bullet));
        assert_eq!(paras[1].level, 1);
        let red = paras[2].runs.iter().find(|r| r.text == "and red").unwrap();
        assert_eq!(red.style.color.as_deref(), Some("#c00000"));
        assert!(red.style.italic);
        let s3 = &deck.slides[2];
        assert_eq!(s3.background.as_deref(), Some("#f4f1ea"));
        let rect = s3.shapes.iter().find(|s| s.name == "Rectangle 1").unwrap();
        assert_eq!(rect.kind, ShapeKind::Rect);
        assert_eq!(rect.fill.as_deref(), Some("#1f4e78"));
        assert_eq!(rect.plain(), "Boxed");
        let oval = s3.shapes.iter().find(|s| s.name == "Oval 2").unwrap();
        assert!((oval.rotation - 30.0).abs() < 0.1, "{}", oval.rotation);
        assert!((oval.x - 288.0).abs() < 1.0 && (oval.y - 36.0).abs() < 1.0, "{oval:?}");
        assert!(s3.shapes.iter().any(|s| matches!(s.kind, ShapeKind::Image { .. })));
        assert!(s3.shapes.iter().any(|s| matches!(s.kind, ShapeKind::Table { .. })));
        assert!(deck.slides[3].hidden);
    }

    #[test]
    fn round_trip() {
        let doc = crate::pptx::tests::sample_doc();
        let (bytes, _) = export(&doc, &[1]).unwrap();
        let back = import(&bytes, "Pitch").unwrap();
        let deck = back.doc.pages[0].deck().unwrap();
        let orig = doc.pages[1].deck().unwrap();
        assert_eq!(deck.slides.len(), orig.slides.len());
        assert_eq!(deck.theme.background, orig.theme.background);
        assert_eq!(deck.slides[0].title(), "Quarterly review");
        assert_eq!(deck.slides[0].notes, "Welcome everyone.\nThen the numbers.");
        assert!(deck.slides[3].hidden);
        let s3 = &deck.slides[2];
        assert_eq!(s3.background.as_deref(), Some("#f4f1ea"));
        let rect = &s3.shapes[0];
        assert_eq!(rect.kind, ShapeKind::Rect);
        assert_eq!(rect.fill.as_deref(), Some("#1f4e78"));
        assert_eq!(rect.plain(), "Boxed");
        assert!((rect.x - 40.0).abs() < 0.1 && (rect.w - 200.0).abs() < 0.1);
        let oval = &s3.shapes[1];
        assert!((oval.rotation - 30.0).abs() < 0.01 && (oval.x - 300.0).abs() < 0.1 && (oval.y - 40.0).abs() < 0.1, "{oval:?}");
        let kinds: Vec<&str> = s3.shapes.iter().map(|s| s.kind.id()).collect();
        assert!(kinds.starts_with(&["rect", "ellipse", "line", "arrow", "image", "table", "table"]), "{kinds:?}");
        let body = &deck.slides[1].shapes[1];
        let paras: Vec<&Paragraph> = body.text.iter().filter_map(Block::para).collect();
        assert_eq!(paras[0].list, Some(ListKind::Bullet));
        assert_eq!(paras[3].list, Some(ListKind::Number));
        assert_eq!(paras[3].level, 1);
        let red = &paras[2].runs[1];
        assert_eq!(red.style.color.as_deref(), Some("#c00000"));
        assert_eq!(red.style.size, Some(14.0));
    }
}
