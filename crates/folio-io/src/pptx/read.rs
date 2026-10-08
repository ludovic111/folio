//! Reading PPTX: slides in order with their shapes, text (inherited from layouts and the
//! master as PowerPoint does), pictures, tables, charts (their saved numbers become a sheet the
//! chart reads), backgrounds, notes and the theme.

use std::collections::HashMap;

use folio_calc::{Addr, SheetRange, Range};
use folio_core::deck::VAlign;
use folio_core::text::{Flow, Table, TableCell};
use folio_core::{Align, Block, Chart, Deck, DeckTheme, Document, ListKind, PageKind, ParaStyle, Paragraph, Run, RunStyle, Shape, ShapeKind, Slide, SlideLayout};

use crate::Imported;
use crate::xlsx::package::{El, Package, Rel, apply_mods, chart_plot, chart_title, family_of, hex_color, theme_colors, theme_fonts};

/// English Metric Units per point.
const EMU: f64 = 12_700.0;

/// The colours and fonts in effect: the theme's scheme through the master's colour map.
#[derive(Clone, Default)]
struct Scheme {
    colors: HashMap<String, String>,
    map: HashMap<String, String>,
    major: Option<String>,
    minor: Option<String>,
}

impl Scheme {
    fn load(theme: Option<&El>, master: &El) -> Scheme {
        let mut s = Scheme::default();
        if let Some(t) = theme {
            s.colors = theme_colors(t).into_iter().collect();
            let (major, minor) = theme_fonts(t);
            s.major = major;
            s.minor = minor;
        }
        if let Some(m) = master.child("clrMap") {
            for (k, v) in &m.attrs {
                s.map.insert(k.clone(), v.clone());
            }
        }
        s
    }

    fn scheme(&self, name: &str) -> Option<String> {
        let n = self.map.get(name).map(String::as_str).unwrap_or(match name {
            "bg1" => "lt1",
            "tx1" => "dk1",
            "bg2" => "lt2",
            "tx2" => "dk2",
            other => other,
        });
        self.colors.get(n).cloned()
    }

    /// The colour held by an element (`srgbClr`, `schemeClr`… with their transforms), `ph`
    /// standing for `phClr` in style references.
    fn color_in(&self, holder: &El, ph: Option<&str>) -> Option<String> {
        for c in holder.elements() {
            let base = match c.name.as_str() {
                "srgbClr" => hex_color(c.attr("val")?),
                "schemeClr" => {
                    let v = c.attr("val")?;
                    if v == "phClr" { ph.map(str::to_string) } else { self.scheme(v) }
                }
                "sysClr" => hex_color(c.attr("lastClr").unwrap_or(if c.attr("val") == Some("window") { "FFFFFF" } else { "000000" })),
                "prstClr" => Some(
                    match c.attr("val")? {
                        "white" => "#ffffff",
                        "red" => "#ff0000",
                        "green" => "#008000",
                        "blue" => "#0000ff",
                        "yellow" => "#ffff00",
                        "gray" | "grey" => "#808080",
                        _ => "#000000",
                    }
                    .to_string(),
                ),
                "scrgbClr" => {
                    let p = |k: &str| c.attr_f64(k).unwrap_or(0.0) / 100_000.0 * 255.0;
                    Some(crate::xlsx::package::hex_of([p("r"), p("g"), p("b")]))
                }
                _ => continue,
            };
            return base.map(|b| apply_mods(&b, c));
        }
        None
    }

    /// A fill among an element's children: `Some(None)` for no fill, `Some(Some(c))` for a
    /// colour (a gradient's first stop), `None` when it says nothing.
    fn fill_in(&self, holder: &El) -> Option<Option<String>> {
        for c in holder.elements() {
            match c.name.as_str() {
                "noFill" => return Some(None),
                "solidFill" => return Some(self.color_in(c, None)),
                "gradFill" => return Some(c.find("gs").and_then(|gs| self.color_in(gs, None))),
                "pattFill" => return Some(c.child("fgClr").and_then(|f| self.color_in(f, None))),
                _ => {}
            }
        }
        None
    }

    fn font(&self, typeface: &str) -> String {
        match typeface {
            "+mj-lt" | "+mj-ea" | "+mj-cs" => self.major.clone().unwrap_or_else(|| "Calibri".into()),
            "+mn-lt" | "+mn-ea" | "+mn-cs" => self.minor.clone().unwrap_or_else(|| "Calibri".into()),
            t => t.to_string(),
        }
    }
}

/// A placeholder's identity: its type (`title`, `body`…, `obj` when absent) and index.
fn ph_of(sp: &El) -> Option<(String, Option<String>)> {
    let ph = sp.elements().find(|e| e.name.starts_with("nv")).and_then(|nv| nv.child("nvPr")).and_then(|n| n.child("ph"))?;
    Some((ph.attr("type").unwrap_or("obj").to_string(), ph.attr("idx").map(str::to_string)))
}

fn same_type(a: &str, b: &str) -> bool {
    fn norm(t: &str) -> &str {
        match t {
            "ctrTitle" | "title" => "title",
            "subTitle" | "body" | "obj" => "body",
            t => t,
        }
    }
    norm(a) == norm(b)
}

/// The shape on a layout or master standing for a slide's placeholder.
fn find_ph<'a>(tree: Option<&'a El>, ty: &str, idx: Option<&str>, by_idx: bool) -> Option<&'a El> {
    let tree = tree?;
    let shapes: Vec<&'a El> = tree.elements().filter(|e| e.name == "sp").collect();
    if by_idx
        && let Some(i) = idx
        && let Some(s) = shapes.iter().copied().find(|s| ph_of(s).is_some_and(|(_, j)| j.as_deref() == Some(i)))
    {
        return Some(s);
    }
    let exact = shapes.iter().copied().find(|s| ph_of(s).is_some_and(|(t, _)| t == ty));
    exact.or_else(|| shapes.iter().copied().find(|s| ph_of(s).is_some_and(|(t, _)| same_type(&t, ty))))
}

/// Position and rotation from an `xfrm` (EMU).
#[derive(Clone, Copy, Debug)]
struct Xfrm {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    rot: f64,
}

fn xfrm_of(el: &El) -> Option<Xfrm> {
    let x = el.path(&["spPr", "xfrm"]).or_else(|| el.child("xfrm")).or_else(|| el.path(&["grpSpPr", "xfrm"]))?;
    let off = x.child("off");
    let ext = x.child("ext");
    Some(Xfrm {
        x: off.and_then(|o| o.attr_f64("x")).unwrap_or(0.0),
        y: off.and_then(|o| o.attr_f64("y")).unwrap_or(0.0),
        w: ext.and_then(|o| o.attr_f64("cx")).unwrap_or(0.0),
        h: ext.and_then(|o| o.attr_f64("cy")).unwrap_or(0.0),
        rot: x.attr_f64("rot").unwrap_or(0.0) / 60_000.0,
    })
}

/// Maps a group's child coordinates to the slide.
#[derive(Clone, Copy)]
struct Map {
    ox: f64,
    oy: f64,
    cx: f64,
    cy: f64,
    sx: f64,
    sy: f64,
}

impl Map {
    const ID: Map = Map { ox: 0.0, oy: 0.0, cx: 0.0, cy: 0.0, sx: 1.0, sy: 1.0 };

    fn apply(&self, x: Xfrm) -> Xfrm {
        Xfrm { x: self.ox + (x.x - self.cx) * self.sx, y: self.oy + (x.y - self.cy) * self.sy, w: x.w * self.sx, h: x.h * self.sy, rot: x.rot }
    }

    fn group(&self, grp: &El) -> Map {
        let Some(x) = grp.path(&["grpSpPr", "xfrm"]) else { return *self };
        let g = |n: &str, a: &str| x.child(n).and_then(|e| e.attr_f64(a)).unwrap_or(0.0);
        let (chw, chh) = (g("chExt", "cx"), g("chExt", "cy"));
        let inner = Map {
            ox: g("off", "x"),
            oy: g("off", "y"),
            cx: g("chOff", "x"),
            cy: g("chOff", "y"),
            sx: if chw > 0.0 { g("ext", "cx") / chw } else { 1.0 },
            sy: if chh > 0.0 { g("ext", "cy") / chh } else { 1.0 },
        };
        // Compose: the group's own box is in the parent's child space.
        Map { ox: self.ox + (inner.ox - self.cx) * self.sx, oy: self.oy + (inner.oy - self.cy) * self.sy, cx: inner.cx, cy: inner.cy, sx: inner.sx * self.sx, sy: inner.sy * self.sy }
    }
}

#[derive(Default)]
struct Counts {
    geometry: usize,
    smartart: usize,
    ole: usize,
    media: usize,
    pictures: usize,
    charts: usize,
    master_shapes: usize,
    image_bg: usize,
    groups: usize,
}

/// One slide's context: the parts it inherits from.
struct SlideCx<'a> {
    scheme: &'a Scheme,
    layout: Option<&'a El>,
    master: Option<&'a El>,
    default_style: Option<&'a El>,
    rels: &'a [Rel],
    /// The deck's text colour, to leave shape colours unset when they match it.
    text: &'a str,
}

impl SlideCx<'_> {
    fn layout_tree(&self) -> Option<&El> {
        self.layout.and_then(|l| l.path(&["cSld", "spTree"]))
    }

    fn master_tree(&self) -> Option<&El> {
        self.master.and_then(|m| m.path(&["cSld", "spTree"]))
    }

    /// The shapes a placeholder inherits from (layout, then master).
    fn ph_chain(&self, sp: &El) -> Vec<&El> {
        let Some((ty, idx)) = ph_of(sp) else { return vec![] };
        let mut out = vec![];
        if let Some(l) = find_ph(self.layout_tree(), &ty, idx.as_deref(), true) {
            out.push(l);
            // The master's is the one the layout's placeholder points at, by type.
            let lty = ph_of(l).map(|(t, _)| t).unwrap_or(ty.clone());
            if let Some(m) = find_ph(self.master_tree(), &lty, None, false) {
                out.push(m);
            }
        } else if let Some(m) = find_ph(self.master_tree(), &ty, None, false) {
            out.push(m);
        }
        out
    }

    /// List styles from the most specific to the least, for a shape's text.
    fn styles<'b>(&'b self, sp: &'b El, chain: &[&'b El]) -> Vec<&'b El> {
        let mut out: Vec<&El> = vec![];
        if let Some(l) = sp.path(&["txBody", "lstStyle"]) {
            out.push(l);
        }
        for c in chain {
            if let Some(l) = c.path(&["txBody", "lstStyle"]) {
                out.push(l);
            }
        }
        let tx = self.master.and_then(|m| m.child("txStyles"));
        match ph_of(sp) {
            Some((t, _)) if matches!(t.as_str(), "title" | "ctrTitle") => out.extend(tx.and_then(|t| t.child("titleStyle"))),
            Some((t, _)) if !matches!(t.as_str(), "dt" | "ftr" | "sldNum") => out.extend(tx.and_then(|t| t.child("bodyStyle"))),
            _ => {
                out.extend(self.default_style);
                out.extend(tx.and_then(|t| t.child("otherStyle")));
            }
        }
        out
    }
}

/// A paragraph-level property from the paragraph or its level in the list styles.
fn level_props<'a>(styles: &[&'a El], level: u8) -> Vec<&'a El> {
    let name = format!("lvl{}pPr", level + 1);
    styles.iter().filter_map(|s| s.child(&name)).collect()
}

struct TextOut {
    flow: Flow,
    text_size: f32,
    color: Option<String>,
    valign: VAlign,
}

/// A text body as folio paragraphs, sizes relative to the shape's text size.
fn read_text(cx: &SlideCx, sp: &El, chain: &[&El], style_color: Option<String>, links: &[Rel]) -> TextOut {
    let body = sp.child("txBody");
    let styles = cx.styles(sp, chain);
    // Anchor and autofit scale, inherited for placeholders.
    let mut body_prs: Vec<&El> = body.and_then(|b| b.child("bodyPr")).into_iter().collect();
    body_prs.extend(chain.iter().filter_map(|c| c.path(&["txBody", "bodyPr"])));
    let valign = match body_prs.iter().find_map(|b| b.attr("anchor")) {
        Some("ctr") => VAlign::Middle,
        Some("b") => VAlign::Bottom,
        _ => VAlign::Top,
    };
    let scale = body_prs.first().and_then(|b| b.child("normAutofit")).and_then(|n| n.attr_f64("fontScale")).map(|v| v / 100_000.0).unwrap_or(1.0);
    let is_title = ph_of(sp).is_some_and(|(t, _)| matches!(t.as_str(), "title" | "ctrTitle"));

    struct RawRun {
        text: String,
        size: f64,
        bold: bool,
        italic: bool,
        underline: bool,
        strike: bool,
        color: Option<String>,
        font: String,
        link: Option<String>,
    }
    let mut paras: Vec<(Paragraph, Vec<RawRun>)> = vec![];
    for p in body.map(|b| b.children("p").collect::<Vec<_>>()).unwrap_or_default() {
        let ppr = p.child("pPr");
        let level = ppr.and_then(|p| p.attr_f64("lvl")).unwrap_or(0.0).clamp(0.0, 8.0) as u8;
        let levels = level_props(&styles, level);
        let mut chain_p: Vec<&El> = ppr.into_iter().collect();
        chain_p.extend(levels.iter().copied());
        let align = match chain_p.iter().find_map(|p| p.attr("algn")) {
            Some("ctr") => Align::Center,
            Some("r") => Align::Right,
            Some("just") | Some("dist") | Some("justLow") => Align::Justify,
            _ => Align::Left,
        };
        let list = chain_p.iter().find_map(|p| {
            p.elements().find_map(|e| match e.name.as_str() {
                "buNone" => Some(None),
                "buChar" => Some(Some(ListKind::Bullet)),
                "buAutoNum" => Some(Some(ListKind::Number)),
                _ => None,
            })
        });
        let list = list.flatten();
        // Run defaults for this level.
        let defs: Vec<&El> = chain_p.iter().filter_map(|p| p.child("defRPr")).collect();
        let mut runs = vec![];
        for r in p.elements() {
            let (text, rpr) = match r.name.as_str() {
                "r" | "fld" => (r.child("t").map(|t| t.text()).unwrap_or_default(), r.child("rPr")),
                "br" => ("\n".to_string(), r.child("rPr")),
                _ => continue,
            };
            let mut props: Vec<&El> = rpr.into_iter().collect();
            props.extend(defs.iter().copied());
            let flag = |k: &str| props.iter().find_map(|p| p.attr(k)).is_some_and(|v| v == "1" || v == "true");
            let size = props.iter().find_map(|p| p.attr_f64("sz")).map(|v| v / 100.0).unwrap_or(18.0) * scale;
            // A shape's fontRef overrides the presentation-wide default text colour, but
            // explicit run/paragraph and placeholder formatting still wins.
            let local_styles: Vec<&El> = sp.path(&["txBody", "lstStyle"]).into_iter()
                .chain(chain.iter().filter_map(|c| c.path(&["txBody", "lstStyle"]))).collect();
            let local_levels = level_props(&local_styles, level);
            let local_color = rpr.into_iter().chain(ppr.and_then(|p| p.child("defRPr")))
                .chain(local_levels.iter().filter_map(|p| p.child("defRPr")))
                .find_map(|p| p.child("solidFill").and_then(|f| cx.scheme.color_in(f, None)));
            let color = local_color.or_else(|| style_color.clone())
                .or_else(|| props.iter().find_map(|p| p.child("solidFill").and_then(|f| cx.scheme.color_in(f, None))))
                .or_else(|| cx.scheme.scheme("tx1"));
            let font = props.iter().find_map(|p| p.child("latin").and_then(|l| l.attr("typeface"))).map(|t| cx.scheme.font(t)).unwrap_or_else(|| cx.scheme.font(if is_title { "+mj-lt" } else { "+mn-lt" }));
            let link = rpr.and_then(|r| r.child("hlinkClick")).and_then(|h| h.attr_ns("id")).and_then(|id| links.iter().find(|l| l.id == id)).filter(|l| l.external).map(|l| l.target.clone());
            runs.push(RawRun {
                text,
                size,
                bold: flag("b"),
                italic: flag("i"),
                underline: props.iter().find_map(|p| p.attr("u")).is_some_and(|u| u != "none"),
                strike: props.iter().find_map(|p| p.attr("strike")).is_some_and(|s| s != "noStrike"),
                color,
                font,
                link,
            });
        }
        // An empty paragraph still has a size (its end mark's).
        if runs.is_empty() {
            let mut props: Vec<&El> = p.child("endParaRPr").into_iter().collect();
            props.extend(defs.iter().copied());
            let size = props.iter().find_map(|p| p.attr_f64("sz")).map(|v| v / 100.0).unwrap_or(18.0) * scale;
            runs.push(RawRun { text: String::new(), size, bold: false, italic: false, underline: false, strike: false, color: None, font: String::new(), link: None });
        }
        let mut para = Paragraph { align, list, level: level.min(5), ..Default::default() };
        para.style = ParaStyle::Normal;
        paras.push((para, runs));
    }
    // The shape's text size: its first real run's; its colour: the most common.
    let first = paras.iter().flat_map(|(_, r)| r.iter()).find(|r| !r.text.trim().is_empty()).or_else(|| paras.iter().flat_map(|(_, r)| r.iter()).next());
    let text_size = first.map(|r| r.size).unwrap_or(18.0).max(1.0);
    let shape_color = first.and_then(|r| r.color.clone());
    let mut flow = Flow::new();
    for (mut para, raw) in paras {
        let runs: Vec<Run> = raw
            .into_iter()
            .filter(|r| !r.text.is_empty())
            .map(|r| {
                let family = family_of(&r.font);
                Run::styled(
                    r.text,
                    RunStyle {
                        bold: r.bold,
                        italic: r.italic,
                        underline: r.underline,
                        strike: r.strike,
                        color: r.color.filter(|c| Some(c) != shape_color.as_ref()),
                        size: ((r.size - text_size).abs() > 0.05).then(|| (r.size * 11.0 / text_size) as f32),
                        font: (family != "sans").then(|| family.to_string()),
                        link: r.link,
                        ..Default::default()
                    },
                )
            })
            .collect();
        para.runs = runs;
        para.normalize();
        flow.push_back(Block::Paragraph(para));
    }
    // A body with only an empty paragraph holds no text.
    if flow.iter().all(|b| b.plain().is_empty()) {
        flow = Flow::new();
    }
    TextOut { flow, text_size: text_size as f32, color: shape_color.filter(|c| c != cx.text), valign }
}

/// A chart's saved numbers: categories and named series.
struct ChartCache {
    categories: Vec<String>,
    series: Vec<(String, Vec<Option<f64>>)>,
    format: Option<String>,
}

fn cache_points(el: Option<&El>) -> (Vec<(usize, String)>, Option<String>) {
    let Some(el) = el else { return (vec![], None) };
    let cache = el.find("strCache").or_else(|| el.find("numCache")).or_else(|| el.find("strLit")).or_else(|| el.find("numLit"));
    let Some(cache) = cache else {
        // A plain value (`<c:v>` straight under `tx`).
        return (el.child("v").map(|v| vec![(0, v.text())]).unwrap_or_default(), None);
    };
    let format = cache.child("formatCode").map(|f| f.text()).filter(|f| !f.eq_ignore_ascii_case("general"));
    (cache.children("pt").map(|p| (p.attr_f64("idx").unwrap_or(0.0) as usize, p.child("v").map(|v| v.text()).unwrap_or_default())).collect(), format)
}

fn read_chart_cache(plot: &El) -> ChartCache {
    let mut out = ChartCache { categories: vec![], series: vec![], format: None };
    for (i, ser) in plot.children("ser").enumerate() {
        let (name, _) = cache_points(ser.child("tx"));
        let name = name.into_iter().next().map(|(_, v)| v).unwrap_or_else(|| format!("Series {}", i + 1));
        let cat_el = ser.child("cat").or(ser.child("xVal"));
        if out.categories.is_empty() {
            let (cats, _) = cache_points(cat_el);
            let n = cats.iter().map(|(i, _)| i + 1).max().unwrap_or(0);
            let mut v = vec![String::new(); n];
            for (i, c) in cats {
                v[i] = c;
            }
            out.categories = v;
        }
        let (vals, fmt) = cache_points(ser.child("val").or(ser.child("yVal")));
        if out.format.is_none() {
            out.format = fmt;
        }
        let n = vals.iter().map(|(i, _)| i + 1).max().unwrap_or(0).max(out.categories.len());
        let mut v = vec![None; n];
        for (i, s) in vals {
            v[i] = s.trim().parse::<f64>().ok();
        }
        out.series.push((name, v));
    }
    let n = out.series.iter().map(|s| s.1.len()).max().unwrap_or(0);
    if out.categories.len() < n {
        for i in out.categories.len()..n {
            out.categories.push((i + 1).to_string());
        }
    }
    out
}

/// Where chart numbers go: one block per chart on a "Charts data" sheet.
struct ChartSheet {
    page: Option<usize>,
    name: String,
    next_row: u32,
}

impl ChartSheet {
    /// Writes a chart's numbers and returns the range holding them.
    fn add(&mut self, doc: &mut Document, cache: &ChartCache, title: &str) -> Result<String, String> {
        let i = match self.page {
            Some(i) => i,
            None => {
                let name = doc.unique_name("Charts data");
                let i = doc.add_page(PageKind::Sheet, Some(&name), None).map_err(|e| e.0)?;
                self.name = name;
                self.page = Some(i);
                i
            }
        };
        let sheet = doc.page_mut(i).sheet_mut().unwrap();
        let mut r = self.next_row;
        if !title.is_empty() {
            sheet.set_input(Addr::new(r, 0), &crate::xlsx::text_input(title));
            if let Some(c) = sheet.cells.get_mut(&Addr::new(r, 0)) {
                c.format.bold = true;
            }
            r += 1;
        }
        let top = r;
        for (j, (name, _)) in cache.series.iter().enumerate() {
            sheet.set_input(Addr::new(r, j as u32 + 1), &crate::xlsx::text_input(name));
        }
        for (k, cat) in cache.categories.iter().enumerate() {
            let row = r + 1 + k as u32;
            // Labels stay text (years too), so the chart reads them as categories.
            sheet.set_input(Addr::new(row, 0), &crate::xlsx::text_input(cat));
            for (j, (_, vals)) in cache.series.iter().enumerate() {
                if let Some(Some(v)) = vals.get(k) {
                    let a = Addr::new(row, j as u32 + 1);
                    sheet.set_input(a, &crate::xlsx::num_text(*v));
                    if let (Some(f), Some(c)) = (&cache.format, sheet.cells.get_mut(&a)) {
                        c.format.number = Some(f.clone());
                    }
                }
            }
        }
        let bottom = r + cache.categories.len() as u32;
        self.next_row = bottom + 2;
        let range = Range::new(Addr::new(top, 0), Addr::new(bottom, cache.series.len() as u32));
        Ok(SheetRange { sheet: Some(self.name.clone()), range }.to_string())
    }
}

struct Reader<'p, 'b> {
    pkg: &'p mut Package<'b>,
    doc: Document,
    counts: Counts,
    charts: ChartSheet,
    media: HashMap<String, folio_core::Id>,
    warnings: Vec<String>,
}

impl Reader<'_, '_> {
    fn emu(v: f64) -> f32 {
        (v / EMU) as f32
    }

    fn picture(&mut self, target: &str) -> Option<folio_core::Id> {
        if let Some(id) = self.media.get(target) {
            return Some(id.clone());
        }
        let bytes = self.pkg.read(target)?;
        let mime = folio_core::Media::sniff(&bytes);
        let bytes = if mime == "application/octet-stream" {
            // TIFF and others the image crate reads become PNG; EMF/WMF can't.
            let img = image::load_from_memory(&bytes).ok()?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).ok()?;
            out.into_inner()
        } else {
            bytes
        };
        let name = target.rsplit('/').next().unwrap_or("picture").to_string();
        let id = self.doc.add_media(&name, bytes);
        self.media.insert(target.to_string(), id.clone());
        Some(id)
    }

    /// The shapes of a tree (a slide's, a group's) as folio shapes.
    fn shapes(&mut self, cx: &SlideCx, tree: &El, map: Map, out: &mut Vec<Shape>) {
        for el in tree.elements() {
            let el = if el.name == "AlternateContent" {
                match el.child("Fallback").or_else(|| el.child("Choice")) {
                    Some(f) => f,
                    None => continue,
                }
            } else {
                el
            };
            for item in if el.name == "Fallback" || el.name == "Choice" { el.elements().collect::<Vec<_>>() } else { vec![el] } {
                self.shape(cx, item, map, out);
            }
        }
    }

    fn shape(&mut self, cx: &SlideCx, el: &El, map: Map, out: &mut Vec<Shape>) {
        let name = el.elements().find(|e| e.name.starts_with("nv")).and_then(|nv| nv.child("cNvPr")).and_then(|c| c.attr("name")).unwrap_or("").to_string();
        match el.name.as_str() {
            "grpSp" => {
                self.counts.groups += 1;
                let inner = map.group(el);
                self.shapes(cx, el, inner, out);
            }
            "sp" | "cxnSp" => {
                let chain = cx.ph_chain(el);
                let Some(xf) = xfrm_of(el).or_else(|| chain.iter().find_map(|c| xfrm_of(c))) else { return };
                let xf = map.apply(xf);
                let sppr = el.child("spPr");
                let geom = sppr.and_then(|s| s.child("prstGeom")).and_then(|g| g.attr("prst"));
                let ph = ph_of(el);
                let style = el.child("style");
                let style_color = |n: &str| style.and_then(|s| s.child(n)).filter(|r| r.attr("idx") != Some("0")).and_then(|r| cx.scheme.color_in(r, None));
                // Fill: the shape's, its placeholder's, or its style's.
                let fill = sppr.and_then(|s| cx.scheme.fill_in(s)).or_else(|| chain.iter().find_map(|c| c.child("spPr").and_then(|s| cx.scheme.fill_in(s)))).unwrap_or_else(|| style_color("fillRef"));
                let ln = sppr.and_then(|s| s.child("ln"));
                let (line, line_width) = match ln {
                    Some(l) if l.child("noFill").is_some() => (None, 0.0),
                    Some(l) => (
                        l.child("solidFill").and_then(|f| cx.scheme.color_in(f, None)).or_else(|| style_color("lnRef")),
                        l.attr_f64("w").map(|w| (w / EMU) as f32).unwrap_or(0.75),
                    ),
                    None => match style_color("lnRef") {
                        Some(c) => (Some(c), style.and_then(|s| s.child("lnRef")).and_then(|r| r.attr_f64("idx")).map(|i| (i as f32 * 0.5).clamp(0.5, 2.0)).unwrap_or(0.75)),
                        None => (None, 0.0),
                    },
                };
                let arrow_head = ln.is_some_and(|l| l.elements().any(|e| (e.name == "tailEnd" || e.name == "headEnd") && e.attr("type").is_some_and(|t| t != "none")));
                let text_box = el.path(&["nvSpPr", "cNvSpPr"]).is_some_and(|c| c.attr_bool("txBox", false));
                let kind = if el.name == "cxnSp" {
                    if arrow_head { ShapeKind::Arrow } else { ShapeKind::Line }
                } else if ph.is_some() || text_box {
                    ShapeKind::Text
                } else {
                    match geom {
                        Some("rect") | None if fill.is_none() && line.is_none() => ShapeKind::Text,
                        Some("rect" | "roundRect" | "snip1Rect" | "snip2SameRect" | "round1Rect" | "round2SameRect" | "flowChartProcess" | "flowChartAlternateProcess" | "plaque" | "frame") => ShapeKind::Rect,
                        Some("ellipse" | "flowChartConnector" | "donut") => ShapeKind::Ellipse,
                        Some("triangle" | "rtTriangle" | "flowChartExtract" | "flowChartMerge") => ShapeKind::Triangle,
                        Some("line" | "straightConnector1" | "bentConnector2" | "bentConnector3" | "curvedConnector3") => {
                            if arrow_head {
                                ShapeKind::Arrow
                            } else {
                                ShapeKind::Line
                            }
                        }
                        Some("rightArrow" | "leftArrow" | "upArrow" | "downArrow" | "leftRightArrow" | "notchedRightArrow" | "stripedRightArrow" | "chevron" | "homePlate") => ShapeKind::Arrow,
                        None => {
                            self.counts.geometry += 1;
                            ShapeKind::Rect
                        }
                        Some(_) => {
                            self.counts.geometry += 1;
                            ShapeKind::Rect
                        }
                    }
                };
                let mut s = Shape::new(kind, Self::emu(xf.x), Self::emu(xf.y), Self::emu(xf.w), Self::emu(xf.h));
                s.name = name;
                s.rotation = xf.rot as f32;
                s.fill = fill;
                s.line = line;
                s.line_width = if s.line.is_some() { line_width } else { 0.0 };
                s.placeholder = ph.as_ref().and_then(|(t, _)| match t.as_str() {
                    "title" | "ctrTitle" => Some("title".to_string()),
                    "subTitle" => Some("subtitle".to_string()),
                    "body" | "obj" => Some("body".to_string()),
                    _ => None,
                });
                let font_color = style.and_then(|s| s.child("fontRef")).and_then(|r| cx.scheme.color_in(r, None));
                let t = read_text(cx, el, &chain, font_color, cx.rels);
                s.text = t.flow;
                s.text_size = t.text_size;
                s.color = t.color;
                s.valign = t.valign;
                out.push(s);
            }
            "pic" => {
                if el.find("videoFile").is_some() || el.find("audioFile").is_some() || el.find("wavAudioFile").is_some() {
                    self.counts.media += 1;
                }
                let Some(xf) = xfrm_of(el).or_else(|| cx.ph_chain(el).iter().find_map(|c| xfrm_of(c))) else { return };
                let xf = map.apply(xf);
                let Some(embed) = el.find("blip").and_then(|b| b.attr_ns("embed")) else { return };
                let Some(target) = cx.rels.iter().find(|r| r.id == embed).map(|r| r.target.clone()) else { return };
                match self.picture(&target) {
                    Some(media) => {
                        let mut s = Shape::new(ShapeKind::Image { media }, Self::emu(xf.x), Self::emu(xf.y), Self::emu(xf.w), Self::emu(xf.h));
                        s.name = name;
                        s.rotation = xf.rot as f32;
                        out.push(s);
                    }
                    None => self.counts.pictures += 1,
                }
            }
            "graphicFrame" => {
                let Some(xf) = xfrm_of(el) else { return };
                let xf = map.apply(xf);
                let Some(data) = el.path(&["graphic", "graphicData"]) else { return };
                let uri = data.attr("uri").unwrap_or("");
                let (x, y, w, h) = (Self::emu(xf.x), Self::emu(xf.y), Self::emu(xf.w), Self::emu(xf.h));
                if let Some(tbl) = data.child("tbl") {
                    let table = self.table(cx, tbl);
                    let mut s = Shape::new(ShapeKind::Table { table: table.0 }, x, y, w, h);
                    s.name = name;
                    s.text_size = table.1;
                    out.push(s);
                } else if uri.ends_with("/chart") {
                    let Some(id) = data.child("chart").and_then(|c| c.attr_ns("id")) else { return };
                    let Some(target) = cx.rels.iter().find(|r| r.id == id).map(|r| r.target.clone()) else { return };
                    match self.chart(&target) {
                        Ok(chart) => {
                            let mut s = Shape::new(ShapeKind::Chart { chart }, x, y, w, h);
                            s.name = name;
                            self.counts.charts += 1;
                            out.push(s);
                        }
                        Err(why) => self.warnings.push(format!("A chart was left out: {why}.")),
                    }
                } else if uri.contains("diagram") {
                    self.counts.smartart += 1;
                } else {
                    self.counts.ole += 1;
                }
            }
            _ => {}
        }
    }

    fn table(&mut self, cx: &SlideCx, tbl: &El) -> (Table, f32) {
        let widths: Vec<f32> = tbl.child("tblGrid").map(|g| g.children("gridCol").map(|c| c.attr_f64("w").unwrap_or(0.0) as f32).collect()).unwrap_or_default();
        let pr = tbl.child("tblPr");
        let mut size = None;
        let rows: Vec<Vec<TableCell>> = tbl
            .children("tr")
            .map(|tr| {
                tr.children("tc")
                    .map(|tc| {
                        let mut runs: Vec<Run> = vec![];
                        let mut align = Align::Left;
                        for (pi, p) in tc.path(&["txBody"]).map(|b| b.children("p").collect::<Vec<_>>()).unwrap_or_default().into_iter().enumerate() {
                            if pi > 0 {
                                runs.push(Run::plain("\n"));
                            }
                            if let Some(a) = p.child("pPr").and_then(|p| p.attr("algn")) {
                                align = match a {
                                    "ctr" => Align::Center,
                                    "r" => Align::Right,
                                    _ => Align::Left,
                                };
                            }
                            for r in p.children("r") {
                                let rpr = r.child("rPr");
                                if size.is_none() {
                                    size = rpr.and_then(|r| r.attr_f64("sz")).map(|v| (v / 100.0) as f32);
                                }
                                runs.push(Run::styled(
                                    r.child("t").map(|t| t.text()).unwrap_or_default(),
                                    RunStyle {
                                        bold: rpr.is_some_and(|r| r.attr_bool("b", false)),
                                        italic: rpr.is_some_and(|r| r.attr_bool("i", false)),
                                        color: rpr.and_then(|r| r.child("solidFill")).and_then(|f| cx.scheme.color_in(f, None)).filter(|c| c != cx.text),
                                        ..Default::default()
                                    },
                                ));
                            }
                        }
                        let mut p = Paragraph::with_runs(ParaStyle::Normal, runs);
                        p.normalize();
                        TableCell { runs: p.runs, align, fill: tc.child("tcPr").and_then(|t| cx.scheme.fill_in(t)).flatten() }
                    })
                    .collect()
            })
            .collect();
        let mut t = Table::from_text(vec![], false);
        t.rows = if rows.is_empty() { vec![vec![TableCell::default()]] } else { rows };
        t.header = pr.is_some_and(|p| p.attr_bool("firstRow", false));
        t.banded = pr.is_some_and(|p| p.attr_bool("bandRow", false));
        t.widths = widths;
        (t, size.unwrap_or(18.0))
    }

    fn chart(&mut self, path: &str) -> Result<Chart, String> {
        let x = self.pkg.xml(path).ok_or("its part is missing")?;
        let chart = x.child("chart").ok_or("it has no chart")?;
        let (kind, stacked, plot, _) = chart_plot(chart)?;
        let cache = read_chart_cache(plot);
        if cache.series.is_empty() {
            return Err("it has no saved numbers".into());
        }
        let title = chart_title(chart);
        let source = self.charts.add(&mut self.doc, &cache, &title)?;
        let mut c = Chart::new(kind, source);
        c.title = title;
        c.stacked = stacked;
        c.legend = chart.child("legend").is_some();
        Ok(c)
    }
}

fn background(scheme: &Scheme, holder: Option<&El>) -> Option<Option<String>> {
    let bg = holder?.path(&["cSld", "bg"])?;
    if let Some(pr) = bg.child("bgPr") {
        if pr.child("blipFill").is_some() {
            return Some(None);
        }
        return scheme.fill_in(pr);
    }
    bg.child("bgRef").map(|r| scheme.color_in(r, None))
}

fn layout_kind(layout: Option<&El>, shapes: &[Shape]) -> SlideLayout {
    match layout.and_then(|l| l.attr("type")) {
        Some("title") => return SlideLayout::Title,
        Some("secHead") => return SlideLayout::Section,
        Some("twoObj" | "twoTxTwoObj" | "twoColTx" | "objAndTx" | "txAndObj" | "txAndChart" | "chartAndTx" | "picTx") => return SlideLayout::TwoContent,
        Some("titleOnly") => return SlideLayout::TitleOnly,
        Some("blank") => return SlideLayout::Blank,
        Some("obj" | "tx") => return SlideLayout::TitleContent,
        _ => {}
    }
    let count = |n: &str| shapes.iter().filter(|s| s.placeholder.as_deref() == Some(n)).count();
    match (count("title"), count("subtitle"), count("body")) {
        (_, s, _) if s > 0 => SlideLayout::Title,
        (_, _, b) if b >= 2 => SlideLayout::TwoContent,
        (_, _, 1) => SlideLayout::TitleContent,
        (t, _, _) if t > 0 => SlideLayout::TitleOnly,
        _ => SlideLayout::Blank,
    }
}

fn notes_text(x: &El) -> String {
    let tree = x.path(&["cSld", "spTree"]);
    let body = tree.and_then(|t| t.elements().find(|s| s.name == "sp" && ph_of(s).is_some_and(|(t, _)| t == "body")));
    body.and_then(|b| b.child("txBody"))
        .map(|b| b.children("p").map(|p| p.elements().filter(|e| e.name == "r" || e.name == "fld" || e.name == "br").map(|r| if r.name == "br" { "\n".into() } else { r.text() }).collect::<String>()).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
        return Err("This is a PowerPoint 97–2003 file (.ppt) or a password-protected one; folio opens .pptx: save it as .pptx first.".into());
    }
    let mut pkg = Package::open(bytes).map_err(|_| "This file isn't a PowerPoint presentation (it isn't a zip package).".to_string())?;
    let pres_path = pkg.main_part().unwrap_or_else(|| "ppt/presentation.xml".into());
    let pres = pkg.xml(&pres_path).ok_or("This file isn't a PowerPoint presentation (no presentation part).")?;
    let pres_rels = pkg.rels(&pres_path);
    let size = pres.child("sldSz").map(|s| [(s.attr_f64("cx").unwrap_or(9_144_000.0) / EMU) as f32, (s.attr_f64("cy").unwrap_or(6_858_000.0) / EMU) as f32]).unwrap_or([960.0, 540.0]);
    let slide_paths: Vec<String> = pres.child("sldIdLst").map(|l| l.children("sldId").filter_map(|s| s.attr_ns("id")).filter_map(|id| pres_rels.iter().find(|r| r.id == id)).map(|r| r.target.clone()).collect()).unwrap_or_default();
    let default_style = pres.child("defaultTextStyle").cloned();

    // Parts shared between slides, loaded once.
    let mut parts: HashMap<String, El> = HashMap::new();
    let mut part_rels: HashMap<String, Vec<Rel>> = HashMap::new();
    let load = |pkg: &mut Package, path: &str, parts: &mut HashMap<String, El>, part_rels: &mut HashMap<String, Vec<Rel>>| {
        if !parts.contains_key(path)
            && let Some(x) = pkg.xml(path)
        {
            parts.insert(path.to_string(), x);
            part_rels.insert(path.to_string(), pkg.rels(path));
        }
    };

    let mut reader = Reader { pkg: &mut pkg, doc: Document::empty(title), counts: Counts::default(), charts: ChartSheet { page: None, name: String::new(), next_row: 0 }, media: HashMap::new(), warnings: vec![] };
    let mut deck = Deck { size, theme: DeckTheme::default(), slides: vec![] };
    let mut theme_set = false;
    let mut animations = 0;
    for path in &slide_paths {
        let Some(sx) = reader.pkg.xml(path) else { continue };
        let rels = reader.pkg.rels(path);
        let layout_path = rels.iter().find(|r| r.kind == "slideLayout").map(|r| r.target.clone());
        if let Some(l) = &layout_path {
            load(reader.pkg, l, &mut parts, &mut part_rels);
        }
        let master_path = layout_path.as_ref().and_then(|l| part_rels.get(l)).and_then(|r| r.iter().find(|r| r.kind == "slideMaster")).map(|r| r.target.clone());
        if let Some(m) = &master_path {
            load(reader.pkg, m, &mut parts, &mut part_rels);
        }
        let theme_path = master_path.as_ref().and_then(|m| part_rels.get(m)).and_then(|r| r.iter().find(|r| r.kind == "theme")).map(|r| r.target.clone());
        if let Some(t) = &theme_path {
            load(reader.pkg, t, &mut parts, &mut part_rels);
        }
        let layout = layout_path.as_ref().and_then(|p| parts.get(p));
        let master = master_path.as_ref().and_then(|p| parts.get(p));
        let theme = theme_path.as_ref().and_then(|p| parts.get(p));
        let empty = El::default();
        let scheme = Scheme::load(theme, master.unwrap_or(&empty));
        let master_bg = background(&scheme, master).flatten().or_else(|| scheme.scheme("bg1")).unwrap_or_else(|| "#ffffff".into());
        if !theme_set {
            theme_set = true;
            let text = scheme.scheme("tx1").unwrap_or_else(|| "#000000".into());
            deck.theme = DeckTheme {
                name: theme.and_then(|t| t.attr("name")).unwrap_or("Imported").to_string(),
                background: master_bg.clone(),
                text,
                accent: scheme.scheme("accent1").unwrap_or_else(|| "#4472c4".into()),
                heading_font: scheme.major.as_deref().map(family_of).unwrap_or("sans").to_string(),
                body_font: scheme.minor.as_deref().map(family_of).unwrap_or("sans").to_string(),
            };
        }
        let text_color = deck.theme.text.clone();
        let cx = SlideCx { scheme: &scheme, layout, master, default_style: default_style.as_ref(), rels: &rels, text: &text_color };
        let mut shapes = vec![];
        // Decorations from the master and the layout (logos, bars) come first, unless hidden.
        let show_master = sx.attr_bool("showMasterSp", true) && layout.is_none_or(|l| l.attr_bool("showMasterSp", true));
        for (holder, holder_path, show) in [(master, &master_path, show_master), (layout, &layout_path, sx.attr_bool("showMasterSp", true))] {
            let (Some(holder), Some(hp), true) = (holder, holder_path, show) else { continue };
            let Some(tree) = holder.path(&["cSld", "spTree"]) else { continue };
            let deco: Vec<&El> = tree.elements().filter(|e| matches!(e.name.as_str(), "sp" | "pic" | "grpSp" | "cxnSp" | "graphicFrame") && ph_of(e).is_none()).collect();
            if deco.is_empty() {
                continue;
            }
            let hrels = part_rels.get(hp).cloned().unwrap_or_default();
            let hcx = SlideCx { scheme: &scheme, layout: None, master, default_style: default_style.as_ref(), rels: &hrels, text: &text_color };
            let before = shapes.len();
            for d in deco {
                reader.shape(&hcx, d, Map::ID, &mut shapes);
            }
            reader.counts.master_shapes += shapes.len() - before;
        }
        if let Some(tree) = sx.path(&["cSld", "spTree"]) {
            reader.shapes(&cx, tree, Map::ID, &mut shapes);
        }
        let bg = background(&scheme, Some(&sx)).or_else(|| background(&scheme, layout));
        if sx.path(&["cSld", "bg", "bgPr"]).and_then(|b| b.child("blipFill")).is_some() {
            reader.counts.image_bg += 1;
        }
        let notes = rels.iter().find(|r| r.kind == "notesSlide").and_then(|r| reader.pkg.xml(&r.target)).map(|n| notes_text(&n)).unwrap_or_default();
        if sx.child("timing").is_some() || sx.child("transition").is_some() {
            animations += 1;
        }
        deck.slides.push(Slide {
            id: folio_core::Id::new(),
            layout: layout_kind(layout, &shapes),
            shapes,
            notes,
            background: bg.flatten().filter(|c| *c != deck.theme.background),
            hidden: matches!(sx.attr("show"), Some("0") | Some("false")),
        });
    }
    if deck.slides.is_empty() {
        return Err("This presentation has no slides folio can read.".into());
    }
    let Reader { mut doc, counts: c, charts, mut warnings, .. } = reader;
    let name = crate::xlsx::safe_sheet_name(title, &doc.pages.iter().map(|p| p.name.clone()).collect::<Vec<_>>());
    let at = doc.add_page(PageKind::Deck, Some(&name), Some(0)).map_err(|e| e.0)?;
    *doc.page_mut(at).deck_mut().unwrap() = deck;
    if charts.page.is_some() {
        folio_core::recalc::Calc::new().sync(&mut doc);
    }
    let mut note = |n: usize, one: &str, many: &str| {
        if n == 1 {
            warnings.push(one.to_string());
        } else if n > 1 {
            warnings.push(many.replace("{n}", &n.to_string()));
        }
    };
    note(c.charts, "The chart's numbers are now on the \"Charts data\" sheet, which it reads: change them there.", "The {n} charts' numbers are now on the \"Charts data\" sheet, which they read: change them there.");
    note(c.geometry, "A shape folio doesn't draw became a rectangle.", "{n} shapes folio doesn't draw became rectangles.");
    note(c.smartart, "A SmartArt diagram was left out.", "{n} SmartArt diagrams were left out.");
    note(c.ole, "An embedded object was left out.", "{n} embedded objects were left out.");
    note(c.media, "A video or sound was left out (its poster picture stays).", "{n} videos or sounds were left out (their poster pictures stay).");
    note(c.pictures, "A picture in a format folio can't show (EMF, WMF…) was left out.", "{n} pictures in formats folio can't show (EMF, WMF…) were left out.");
    note(c.master_shapes, "A shape from the slide master or layout (a logo, a bar) was copied onto the slides that show it.", "{n} shapes from the slide master and layouts (logos, bars) were copied onto the slides that show them.");
    note(c.image_bg, "A picture background was left out.", "{n} picture backgrounds were left out.");
    note(c.groups, "A group of shapes was ungrouped.", "{n} groups of shapes were ungrouped.");
    note(animations, "Animations and transitions were left out.", "Animations and transitions were left out ({n} slides).");
    Ok(Imported { doc, warnings, format: "pptx" })
}
