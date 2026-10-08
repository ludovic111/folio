//! Writing PPTX: one slide master with six layouts carrying the deck's theme, slides with text
//! boxes and placeholders, shapes, pictures, tables, native charts (with their numbers in an
//! embedded workbook), backgrounds, speaker notes and hidden slides.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use folio_core::deck::VAlign;
use folio_core::text::{Flow, Table};
use folio_core::{Align, Block, ChartKind, Deck, DeckTheme, Document, ListKind, Media, PageKind, ParaStyle, Paragraph, Shape, ShapeKind, Slide, SlideLayout};

use crate::xlsx::package::{ZipOut, esc, font_name, mix};

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT: &str = "application/vnd.openxmlformats-officedocument.presentationml";
const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
/// Inner margin of folio's text boxes (8 pt), so text wraps where folio wraps it.
const INSET: i64 = 101_600;

fn emu(pt: f32) -> i64 {
    (pt as f64 * 12_700.0).round() as i64
}

fn hex(c: &str) -> String {
    c.trim().trim_start_matches('#').to_ascii_uppercase()
}

fn srgb(c: &str) -> String {
    format!(r#"<a:srgbClr val="{}"/>"#, hex(c))
}

fn solid(c: &str) -> String {
    format!("<a:solidFill>{}</a:solidFill>", srgb(c))
}

/// A part's relationships, numbered as they are added.
#[derive(Default)]
struct Rels(Vec<(String, String, bool)>);

impl Rels {
    fn add(&mut self, kind: &str, target: &str) -> String {
        if let Some(i) = self.0.iter().position(|(k, t, _)| k == kind && t == target) {
            return format!("rId{}", i + 1);
        }
        self.0.push((kind.to_string(), target.to_string(), false));
        format!("rId{}", self.0.len())
    }

    fn external(&mut self, kind: &str, target: &str) -> String {
        self.0.push((kind.to_string(), target.to_string(), true));
        format!("rId{}", self.0.len())
    }

    fn xml(&self) -> String {
        let mut s = format!("{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
        for (i, (k, t, ext)) in self.0.iter().enumerate() {
            let ty = if k.starts_with("http") { k.clone() } else { format!("{REL}/{k}") };
            let _ = write!(s, r#"<Relationship Id="rId{}" Type="{}" Target="{}"{}/>"#, i + 1, ty, esc(t), if *ext { r#" TargetMode="External""# } else { "" });
        }
        s.push_str("</Relationships>");
        s
    }
}

/// The parts written so far and their content types.
struct Pkg {
    zip: ZipOut,
    overrides: Vec<(String, String)>,
    defaults: BTreeSet<(String, String)>,
}

impl Pkg {
    fn part(&mut self, path: &str, content_type: Option<&str>, body: &str) -> Result<(), String> {
        if let Some(ct) = content_type {
            self.overrides.push((format!("/{path}"), ct.to_string()));
        }
        self.zip.add(path, body)
    }

    fn rels(&mut self, part: &str, rels: &Rels) -> Result<(), String> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let path = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
        self.zip.add(&path, rels.xml())
    }
}

/// Theme colours: the deck's background, text and accent, and a ramp of accents for charts.
fn theme_xml(t: &DeckTheme) -> String {
    let (bg, tx, ac) = (t.background.as_str(), t.text.as_str(), t.accent.as_str());
    let accents = [ac.to_string(), mix(ac, bg, 0.45), mix(tx, bg, 0.6), mix(ac, bg, 0.7), mix(tx, bg, 0.3), mix(ac, tx, 0.4)];
    let mut s = format!(r#"{XML_HEAD}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="{}"><a:themeElements><a:clrScheme name="{}">"#, esc(&t.name), esc(&t.name));
    let _ = write!(
        s,
        "<a:dk1>{}</a:dk1><a:lt1>{}</a:lt1><a:dk2>{}</a:dk2><a:lt2>{}</a:lt2>",
        srgb(tx),
        srgb(bg),
        srgb(&mix(tx, bg, 0.25)),
        srgb(&mix(bg, tx, 0.08))
    );
    for (i, a) in accents.iter().enumerate() {
        let _ = write!(s, "<a:accent{n}>{}</a:accent{n}>", srgb(a), n = i + 1);
    }
    let _ = write!(s, "<a:hlink>{}</a:hlink><a:folHlink>{}</a:folHlink></a:clrScheme>", srgb("#0563c1"), srgb("#954f72"));
    let (major, minor) = (font_name(&t.heading_font), font_name(&t.body_font));
    let _ = write!(
        s,
        r#"<a:fontScheme name="folio"><a:majorFont><a:latin typeface="{}"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="{}"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>"#,
        esc(&major),
        esc(&minor)
    );
    let ph_fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    let _ = write!(
        s,
        r#"<a:fmtScheme name="folio"><a:fillStyleLst>{f}{f}{f}</a:fillStyleLst><a:lnStyleLst><a:ln w="6350">{f}</a:ln><a:ln w="12700">{f}</a:ln><a:ln w="19050">{f}</a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{f}{f}{f}</a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#,
        f = ph_fill
    );
    s
}

const CLR_MAP: &str = r#"bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink""#;

const GROUP: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;

fn xfrm(s: &Shape, tag: &str) -> String {
    let rot = if s.rotation != 0.0 { format!(r#" rot="{}""#, (s.rotation as f64 * 60_000.0).round() as i64) } else { String::new() };
    format!(r#"<{tag}{rot}><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></{tag}>"#, emu(s.x), emu(s.y), emu(s.w.max(0.0)), emu(s.h.max(0.0)))
}

/// What a slide's placeholder is called in the layout: `(type, idx)`.
fn ph_attrs(layout: SlideLayout, kind: &str, body_index: usize) -> Option<(Option<&'static str>, Option<u32>)> {
    use SlideLayout::*;
    Some(match (layout, kind) {
        (Title, "title") => (Some("ctrTitle"), None),
        (Title, "subtitle") => (Some("subTitle"), Some(1)),
        (Section, "title") | (TitleContent | TwoContent | TitleOnly, "title") => (Some("title"), None),
        (Section, "subtitle") => (Some("body"), Some(1)),
        (TitleContent, "body") if body_index == 0 => (None, Some(1)),
        (TwoContent, "body") if body_index < 2 => (None, Some(body_index as u32 + 1)),
        _ => return None,
    })
}

fn ph_xml(ph: Option<(Option<&str>, Option<u32>)>) -> String {
    match ph {
        None => "<p:nvPr/>".into(),
        Some((ty, idx)) => {
            let mut s = String::from("<p:nvPr><p:ph");
            if let Some(t) = ty {
                let _ = write!(s, r#" type="{t}""#);
            }
            if let Some(i) = idx {
                let _ = write!(s, r#" idx="{i}""#);
            }
            s.push_str("/></p:nvPr>");
            s
        }
    }
}

struct TextCx<'a> {
    theme: &'a DeckTheme,
    /// Points per folio point (`text_size / 11`).
    scale: f32,
    color: Option<&'a str>,
    title: bool,
}

/// A paragraph's runs; links get relationships on the slide.
fn para_xml(p: &Paragraph, cx: &TextCx, rels: &mut Rels) -> String {
    let spec = p.style.spec();
    let mut s = String::from("<a:p>");
    // Paragraph properties.
    let mut ppr = String::new();
    let algn = match p.align {
        Align::Left => "l",
        Align::Center => "ctr",
        Align::Right => "r",
        Align::Justify => "just",
    };
    let level = p.level.min(8);
    let base = spec.size * cx.scale;
    let (mar, ind) = match p.list {
        Some(_) => {
            let step = (base * 1.6).max(12.0);
            (emu(step * (level as f32 + 1.0)), -emu(step))
        }
        None => (0, 0),
    };
    let _ = write!(ppr, r#"<a:pPr marL="{mar}" indent="{ind}" algn="{algn}"{}>"#, if level > 0 { format!(r#" lvl="{level}""#) } else { String::new() });
    let _ = write!(ppr, r#"<a:lnSpc><a:spcPct val="{}"/></a:lnSpc>"#, (spec.line / 1.2 * 100_000.0).round() as i64);
    let _ = write!(ppr, r#"<a:spcBef><a:spcPts val="{}"/></a:spcBef><a:spcAft><a:spcPts val="{}"/></a:spcAft>"#, (spec.space_before * cx.scale * 100.0).round() as i64, (spec.space_after * cx.scale * 100.0).round() as i64);
    match p.list {
        Some(ListKind::Bullet) => ppr.push_str(r#"<a:buFont typeface="Arial"/><a:buChar char="•"/>"#),
        Some(ListKind::Number) => ppr.push_str(r#"<a:buFont typeface="+mj-lt"/><a:buAutoNum type="arabicPeriod"/>"#),
        Some(ListKind::Check) => {
            let _ = write!(ppr, r#"<a:buFont typeface="Segoe UI Symbol"/><a:buChar char="{}"/>"#, if p.checked { "☑" } else { "☐" });
        }
        None => ppr.push_str("<a:buNone/>"),
    }
    ppr.push_str("</a:pPr>");
    s.push_str(&ppr);
    let family = |f: Option<&str>, code: bool| -> String {
        if code {
            return font_name("mono");
        }
        match f {
            Some(f) => font_name(f),
            None if cx.title => font_name(&cx.theme.heading_font),
            None => match spec.family {
                folio_core::text::Family::Sans => font_name(&cx.theme.body_font),
                folio_core::text::Family::Serif => font_name("serif"),
                folio_core::text::Family::Mono => font_name("mono"),
                folio_core::text::Family::Display => font_name("display"),
            },
        }
    };
    let rpr = |st: &folio_core::RunStyle, rels: &mut Rels| -> String {
        let size = st.size.unwrap_or(spec.size) * cx.scale;
        let mut a = format!(r#"<a:rPr lang="en-US" sz="{}""#, ((size * 100.0).round() as i64).clamp(100, 400_000));
        if st.bold || spec.bold {
            a.push_str(r#" b="1""#);
        }
        if st.italic || spec.italic {
            a.push_str(r#" i="1""#);
        }
        if st.underline {
            a.push_str(r#" u="sng""#);
        }
        if st.strike {
            a.push_str(r#" strike="sngStrike""#);
        }
        if st.superscript {
            a.push_str(r#" baseline="30000""#);
        } else if st.subscript {
            a.push_str(r#" baseline="-25000""#);
        }
        a.push_str(" dirty=\"0\">");
        match st.color.as_deref().or(cx.color) {
            Some(c) => a.push_str(&solid(c)),
            None if spec.muted => {
                let _ = write!(a, r#"<a:solidFill><a:srgbClr val="{}"><a:alpha val="62000"/></a:srgbClr></a:solidFill>"#, hex(&cx.theme.text));
            }
            None => {}
        }
        if let Some(h) = &st.highlight {
            let _ = write!(a, "<a:highlight>{}</a:highlight>", srgb(h));
        }
        let face = family(st.font.as_deref(), st.code);
        let _ = write!(a, r#"<a:latin typeface="{f}"/><a:ea typeface="{f}"/><a:cs typeface="{f}"/>"#, f = esc(&face));
        if let Some(l) = &st.link {
            let id = rels.external("hyperlink", l);
            let _ = write!(a, r#"<a:hlinkClick r:id="{id}"/>"#);
        }
        a.push_str("</a:rPr>");
        a
    };
    for r in &p.runs {
        if r.style.deleted.is_some() {
            continue;
        }
        let props = rpr(&r.style, rels);
        for (i, piece) in r.text.split('\n').enumerate() {
            if i > 0 {
                let _ = write!(s, "<a:br>{props}</a:br>");
            }
            if !piece.is_empty() {
                let _ = write!(s, "<a:r>{props}<a:t>{}</a:t></a:r>", esc(piece));
            }
        }
    }
    let end = rpr(&p.runs.last().map(|r| r.style.clone()).unwrap_or_default(), rels).replacen("<a:rPr", "<a:endParaRPr", 1).replace("</a:rPr>", "</a:endParaRPr>");
    // Links don't belong on the end mark.
    let end = match end.find("<a:hlinkClick") {
        Some(i) => format!("{}</a:endParaRPr>", &end[..i]),
        None => end,
    };
    s.push_str(&end);
    s.push_str("</a:p>");
    s
}

fn flow_paras(flow: &Flow) -> Vec<Paragraph> {
    flow.iter()
        .map(|b| match b {
            Block::Paragraph(p) => p.clone(),
            other => Paragraph::new(ParaStyle::Normal, other.plain()),
        })
        .collect()
}

fn tx_body(tag: &str, s: &Shape, cx: &TextCx, rels: &mut Rels) -> String {
    let anchor = match s.valign {
        VAlign::Top => "t",
        VAlign::Middle => "ctr",
        VAlign::Bottom => "b",
    };
    let mut out = format!(r#"<{tag}><a:bodyPr wrap="square" lIns="{INSET}" tIns="{INSET}" rIns="{INSET}" bIns="{INSET}" rtlCol="0" anchor="{anchor}"><a:noAutofit/></a:bodyPr><a:lstStyle/>"#);
    let paras = flow_paras(&s.text);
    if paras.is_empty() {
        let _ = write!(out, r#"<a:p><a:endParaRPr lang="en-US" sz="{}" dirty="0"/></a:p>"#, ((11.0 * cx.scale * 100.0).round() as i64).clamp(100, 400_000));
    }
    for p in &paras {
        out.push_str(&para_xml(p, cx, rels));
    }
    let _ = write!(out, "</{tag}>");
    out
}

fn table_xml(t: &Table, rows: &[Vec<String>], s: &Shape, theme: &DeckTheme, rels: &mut Rels, id: usize) -> String {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let fr = if t.widths.len() == cols { t.fractions() } else { vec![1.0 / cols as f32; cols] };
    let width = emu(s.w);
    let row_h = emu(s.h / rows.len().max(1) as f32);
    let rule = mix(&theme.text, &theme.background, 0.7);
    let line = |tag: &str| format!(r#"<a:{tag} w="9525" cap="flat" cmpd="sng" algn="ctr">{}<a:prstDash val="solid"/></a:{tag}>"#, solid(&rule));
    let mut x = format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="{}"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>{}<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="{}" bandRow="{}"/><a:tblGrid>"#,
        esc(if s.name.is_empty() { "Table" } else { &s.name }),
        xfrm(s, "p:xfrm"),
        u8::from(t.header),
        u8::from(t.banded)
    );
    for f in &fr {
        let _ = write!(x, r#"<a:gridCol w="{}"/>"#, (width as f64 * *f as f64).round() as i64);
    }
    x.push_str("</a:tblGrid>");
    let cx = TextCx { theme, scale: s.text_size / 11.0, color: s.color.as_deref(), title: false };
    for (ri, row) in rows.iter().enumerate() {
        let _ = write!(x, r#"<a:tr h="{row_h}">"#);
        for c in 0..cols {
            let cell = t.cell(ri, c);
            let mut p = match cell {
                Some(tc) if t.link.is_none() => tc.as_para(),
                _ => Paragraph::new(ParaStyle::Normal, row.get(c).cloned().unwrap_or_default()),
            };
            p.align = cell.map(|c| c.align).unwrap_or(Align::Left);
            if t.header && ri == 0 {
                for r in &mut p.runs {
                    r.style.bold = true;
                }
            }
            let _ = write!(x, r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr marL="91440" marR="91440" marT="45720" marB="45720">{}{}{}{}"#, para_xml(&p, &cx, rels), line("lnL"), line("lnR"), line("lnT"), line("lnB"));
            let band = t.banded && ri % 2 == 1 && !(t.header && ri == 0);
            match cell.and_then(|c| c.fill.as_deref()) {
                Some(f) => x.push_str(&solid(f)),
                None if band => x.push_str(&solid(&mix(&theme.background, &theme.text, 0.05))),
                None => x.push_str("<a:noFill/>"),
            }
            x.push_str("</a:tcPr></a:tc>");
        }
        x.push_str("</a:tr>");
    }
    x.push_str("</a:tbl></a:graphicData></a:graphic></p:graphicFrame>");
    x
}

/// A native chart part with its numbers cached and in an embedded workbook.
fn chart_part(chart: &folio_core::Chart, data: &folio_core::ChartData, theme: &DeckTheme) -> Result<(String, Vec<u8>), String> {
    let n = data.categories.len();
    let col = |i: usize| folio_calc::col_name(i as u32);
    let str_cache = |vals: &[String]| -> String {
        let mut s = format!(r#"<c:strCache><c:ptCount val="{}"/>"#, vals.len());
        for (i, v) in vals.iter().enumerate() {
            let _ = write!(s, r#"<c:pt idx="{i}"><c:v>{}</c:v></c:pt>"#, esc(v));
        }
        s.push_str("</c:strCache>");
        s
    };
    let num_cache = |vals: &[Option<f64>]| -> String {
        let mut s = format!(r#"<c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>"#, vals.len());
        for (i, v) in vals.iter().enumerate() {
            if let Some(v) = v {
                let _ = write!(s, r#"<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>"#);
            }
        }
        s.push_str("</c:numCache>");
        s
    };
    let cats_ref = format!("Sheet1!$A$2:$A${}", n + 1);
    let scatter = chart.kind == ChartKind::Scatter;
    let xs: Vec<Option<f64>> = data.categories.iter().enumerate().map(|(i, c)| Some(c.trim().parse::<f64>().unwrap_or(i as f64 + 1.0))).collect();
    let mut sers = String::new();
    for (i, s) in data.series.iter().enumerate() {
        let c = col(i + 1);
        let tx = format!(r#"<c:tx><c:strRef><c:f>Sheet1!${c}$1</c:f>{}</c:strRef></c:tx>"#, str_cache(std::slice::from_ref(&s.name)));
        let vals = format!(r#"<c:numRef><c:f>Sheet1!${c}$2:${c}${}</c:f>{}</c:numRef>"#, n + 1, num_cache(&s.values));
        let _ = write!(sers, r#"<c:ser><c:idx val="{i}"/><c:order val="{i}"/>{tx}"#);
        match chart.kind {
            ChartKind::Column | ChartKind::Bar => {
                let _ = write!(sers, r#"<c:invertIfNegative val="0"/><c:cat><c:strRef><c:f>{cats_ref}</c:f>{}</c:strRef></c:cat><c:val>{vals}</c:val>"#, str_cache(&data.categories));
            }
            ChartKind::Line => {
                let _ = write!(sers, r#"<c:marker><c:symbol val="none"/></c:marker><c:cat><c:strRef><c:f>{cats_ref}</c:f>{}</c:strRef></c:cat><c:val>{vals}</c:val><c:smooth val="0"/>"#, str_cache(&data.categories));
            }
            ChartKind::Area | ChartKind::Pie => {
                let _ = write!(sers, r#"<c:cat><c:strRef><c:f>{cats_ref}</c:f>{}</c:strRef></c:cat><c:val>{vals}</c:val>"#, str_cache(&data.categories));
            }
            ChartKind::Scatter => {
                let _ = write!(
                    sers,
                    r#"<c:spPr><a:ln w="19050"><a:noFill/></a:ln></c:spPr><c:marker><c:symbol val="circle"/><c:size val="6"/></c:marker><c:xVal><c:numRef><c:f>{cats_ref}</c:f>{}</c:numRef></c:xVal><c:yVal>{vals}</c:yVal><c:smooth val="0"/>"#,
                    num_cache(&xs)
                );
            }
        }
        sers.push_str("</c:ser>");
    }
    let grouping = if chart.stacked { "stacked" } else { "clustered" };
    let axes_ids = r#"<c:axId val="500000001"/><c:axId val="500000002"/>"#;
    let plot = match chart.kind {
        ChartKind::Column | ChartKind::Bar => format!(
            r#"<c:barChart><c:barDir val="{}"/><c:grouping val="{grouping}"/><c:varyColors val="0"/>{sers}<c:gapWidth val="80"/>{}{axes_ids}</c:barChart>"#,
            if chart.kind == ChartKind::Bar { "bar" } else { "col" },
            if chart.stacked { r#"<c:overlap val="100"/>"# } else { "" }
        ),
        ChartKind::Line => format!(r#"<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>{sers}<c:marker val="1"/>{axes_ids}</c:lineChart>"#),
        ChartKind::Area => format!(r#"<c:areaChart><c:grouping val="{}"/><c:varyColors val="0"/>{sers}{axes_ids}</c:areaChart>"#, if chart.stacked { "stacked" } else { "standard" }),
        ChartKind::Pie => format!(r#"<c:pieChart><c:varyColors val="1"/>{sers}<c:firstSliceAng val="0"/></c:pieChart>"#),
        ChartKind::Scatter => format!(r#"<c:scatterChart><c:scatterStyle val="lineMarker"/><c:varyColors val="0"/>{sers}{axes_ids}</c:scatterChart>"#),
    };
    let grid = format!(r#"<c:majorGridlines><c:spPr><a:ln w="6350">{}</a:ln></c:spPr></c:majorGridlines>"#, solid(&mix(&theme.text, &theme.background, 0.85)));
    let (cat_pos, val_pos) = if chart.kind == ChartKind::Bar { ("l", "b") } else { ("b", "l") };
    let axes = match chart.kind {
        ChartKind::Pie => String::new(),
        ChartKind::Scatter => format!(
            r#"<c:valAx><c:axId val="500000001"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="500000002"/><c:crosses val="autoZero"/><c:crossBetween val="midCat"/></c:valAx><c:valAx><c:axId val="500000002"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="l"/>{grid}<c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="500000001"/><c:crosses val="autoZero"/><c:crossBetween val="midCat"/></c:valAx>"#
        ),
        _ => format!(
            r#"<c:catAx><c:axId val="500000001"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="{cat_pos}"/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="500000002"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/><c:noMultiLvlLbl val="0"/></c:catAx><c:valAx><c:axId val="500000002"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="{val_pos}"/>{grid}<c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="500000001"/><c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx>"#
        ),
    };
    let title = if chart.title.is_empty() {
        r#"<c:autoTitleDeleted val="1"/>"#.to_string()
    } else {
        format!(
            r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="1400" b="0"/></a:pPr><a:r><a:rPr lang="en-US" sz="1400" b="0"/><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>"#,
            esc(&chart.title)
        )
    };
    let legend = if chart.legend { r#"<c:legend><c:legendPos val="b"/><c:overlay val="0"/></c:legend>"# } else { "" };
    let xml = format!(
        r#"{XML_HEAD}<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="{REL}"><c:date1904 val="0"/><c:roundedCorners val="0"/><c:chart>{title}<c:plotArea><c:layout/>{plot}{axes}</c:plotArea>{legend}<c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart><c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr><c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="1100">{}<a:latin typeface="{}"/></a:defRPr></a:pPr><a:endParaRPr lang="en-US"/></a:p></c:txPr><c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData></c:chartSpace>"#,
        solid(&theme.text),
        esc(&font_name(&theme.body_font))
    );
    // The numbers, for "Edit data" in PowerPoint.
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    let e = |e: rust_xlsxwriter::XlsxError| e.to_string();
    for (i, c) in data.categories.iter().enumerate() {
        if scatter && let Some(Some(x)) = xs.get(i) {
            ws.write_number(i as u32 + 1, 0, *x).map_err(e)?;
        } else {
            ws.write_string(i as u32 + 1, 0, c).map_err(e)?;
        }
    }
    for (j, s) in data.series.iter().enumerate() {
        ws.write_string(0, j as u16 + 1, &s.name).map_err(e)?;
        for (i, v) in s.values.iter().enumerate() {
            if let Some(v) = v {
                ws.write_number(i as u32 + 1, j as u16 + 1, *v).map_err(e)?;
            }
        }
    }
    let book = wb.save_to_buffer().map_err(e)?;
    Ok((xml, book))
}

/// Media as a format every presentation app reads (PNG, JPEG, GIF; others become PNG).
fn media_bytes(m: &Media) -> Option<(Vec<u8>, &'static str)> {
    match m.mime.as_str() {
        "image/png" => Some((m.bytes.to_vec(), "png")),
        "image/jpeg" => Some((m.bytes.to_vec(), "jpeg")),
        "image/gif" => Some((m.bytes.to_vec(), "gif")),
        _ => {
            let img = image::load_from_memory(&m.bytes).ok()?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).ok()?;
            Some((out.into_inner(), "png"))
        }
    }
}

struct Writer<'a> {
    doc: &'a Document,
    pkg: Pkg,
    media_done: std::collections::HashMap<String, String>,
    images: usize,
    charts: usize,
    warnings: Vec<String>,
    svg: usize,
    broken_charts: usize,
}

impl Writer<'_> {
    fn image(&mut self, id: &folio_core::Id) -> Option<String> {
        if let Some(p) = self.media_done.get(id.as_str()) {
            return Some(p.clone());
        }
        let m = self.doc.media.get(id)?;
        if m.mime == "image/svg+xml" {
            self.svg += 1;
            return None;
        }
        let (bytes, ext) = media_bytes(m)?;
        self.images += 1;
        let path = format!("ppt/media/image{}.{ext}", self.images);
        self.pkg.defaults.insert((ext.to_string(), format!("image/{ext}")));
        self.pkg.zip.add(&path, bytes).ok()?;
        let target = format!("../media/image{}.{ext}", self.images);
        self.media_done.insert(id.to_string(), target.clone());
        Some(target)
    }

    fn shape_xml(&mut self, deck: &Deck, slide: &Slide, s: &Shape, id: usize, rels: &mut Rels, bodies: &mut usize) -> String {
        let name = esc(if s.name.is_empty() { s.kind.id() } else { &s.name });
        let theme = &deck.theme;
        let title = s.placeholder.as_deref() == Some("title");
        let cx = TextCx { theme, scale: s.text_size / 11.0, color: s.color.as_deref(), title };
        let fill = |f: &Option<String>| match f {
            Some(c) => solid(c),
            None => "<a:noFill/>".into(),
        };
        let ln = |s: &Shape, head: bool| match &s.line {
            Some(c) => format!(r#"<a:ln w="{}">{}{}</a:ln>"#, emu(s.line_width.max(0.25)), solid(c), if head { r#"<a:tailEnd type="triangle"/>"# } else { "" }),
            None => "<a:ln><a:noFill/></a:ln>".into(),
        };
        match &s.kind {
            ShapeKind::Text | ShapeKind::Rect | ShapeKind::Ellipse | ShapeKind::Triangle => {
                let ph = s.placeholder.as_deref().and_then(|p| {
                    let a = ph_attrs(slide.layout, p, *bodies);
                    if p == "body" && a.is_some() {
                        *bodies += 1;
                    }
                    a
                });
                let prst = match s.kind {
                    ShapeKind::Ellipse => "ellipse",
                    ShapeKind::Triangle => "triangle",
                    _ => "rect",
                };
                let tx_box = if matches!(s.kind, ShapeKind::Text) && ph.is_none() { r#" txBox="1""# } else { "" };
                let locks = if ph.is_some() { r#"<a:spLocks noGrp="1"/>"# } else { "" };
                format!(
                    r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr{tx_box}>{locks}</p:cNvSpPr>{}</p:nvSpPr><p:spPr>{}<a:prstGeom prst="{prst}"><a:avLst/></a:prstGeom>{}{}</p:spPr>{}</p:sp>"#,
                    ph_xml(ph),
                    xfrm(s, "a:xfrm"),
                    fill(&s.fill),
                    ln(s, false),
                    tx_body("p:txBody", s, &cx, rels)
                )
            }
            ShapeKind::Line | ShapeKind::Arrow if s.kind == ShapeKind::Line || s.fill.is_none() => {
                // A line runs corner to corner of its box; an arrow without a fill is a line with a head.
                let mut l = s.clone();
                if l.line.is_none() {
                    l.line = Some(theme.text.clone());
                    l.line_width = l.line_width.max(1.5);
                }
                format!(
                    r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr>{}<a:prstGeom prst="line"><a:avLst/></a:prstGeom>{}</p:spPr></p:cxnSp>"#,
                    xfrm(s, "a:xfrm"),
                    ln(&l, s.kind == ShapeKind::Arrow)
                )
            }
            ShapeKind::Line | ShapeKind::Arrow => format!(
                r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rightArrow"><a:avLst/></a:prstGeom>{}{}</p:spPr></p:sp>"#,
                xfrm(s, "a:xfrm"),
                fill(&s.fill),
                ln(s, false)
            ),
            ShapeKind::Image { media } => {
                let Some(target) = self.image(media) else { return String::new() };
                let rid = rels.add("image", &target);
                let descr = self.doc.media.get(media).map(|m| esc(&m.name)).unwrap_or_default();
                format!(
                    r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="{name}" descr="{descr}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{}</p:spPr></p:pic>"#,
                    xfrm(s, "a:xfrm"),
                    if s.line.is_some() { ln(s, false) } else { String::new() }
                )
            }
            ShapeKind::Table { table } => {
                let rows: Vec<Vec<String>> = match &table.link {
                    Some(l) => folio_core::links::table_text(self.doc, l).unwrap_or_else(|_| table.rows.iter().map(|r| r.iter().map(|c| c.plain()).collect()).collect()),
                    None => table.rows.iter().map(|r| r.iter().map(|c| c.plain()).collect()).collect(),
                };
                let rows = if rows.is_empty() { vec![vec![String::new()]] } else { rows };
                table_xml(table, &rows, s, theme, rels, id)
            }
            ShapeKind::Chart { chart } => {
                let data = match folio_core::links::chart_data(self.doc, chart) {
                    Ok(d) if !d.series.is_empty() => d,
                    _ => {
                        self.broken_charts += 1;
                        return String::new();
                    }
                };
                let (xml, book) = match chart_part(chart, &data, theme) {
                    Ok(p) => p,
                    Err(_) => {
                        self.broken_charts += 1;
                        return String::new();
                    }
                };
                self.charts += 1;
                let n = self.charts;
                let path = format!("ppt/charts/chart{n}.xml");
                let mut crels = Rels::default();
                crels.add("package", &format!("../embeddings/Microsoft_Excel_Worksheet{n}.xlsx"));
                if self.pkg.part(&path, Some("application/vnd.openxmlformats-officedocument.drawingml.chart+xml"), &xml).is_err()
                    || self.pkg.rels(&path, &crels).is_err()
                    || self.pkg.zip.add(&format!("ppt/embeddings/Microsoft_Excel_Worksheet{n}.xlsx"), book).is_err()
                {
                    return String::new();
                }
                self.pkg.defaults.insert(("xlsx".into(), "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into()));
                let rid = rels.add("chart", &format!("../charts/chart{n}.xml"));
                format!(
                    r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="{name}"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>{}<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="{rid}"/></a:graphicData></a:graphic></p:graphicFrame>"#,
                    xfrm(s, "p:xfrm")
                )
            }
        }
    }
}

/// Placeholder shapes for layouts and the master, at folio's layout positions.
fn layout_shapes(layout: SlideLayout, size: [f32; 2], theme: &DeckTheme) -> String {
    let proto = Slide::with_layout(layout, size, "", "");
    let mut out = String::new();
    let mut bodies = 0;
    for (i, s) in proto.shapes.iter().enumerate() {
        let kind = s.placeholder.as_deref().unwrap_or("body");
        let Some(ph) = ph_attrs(layout, kind, bodies) else { continue };
        if kind == "body" {
            bodies += 1;
        }
        let title = kind == "title";
        let size_pt = s.text_size / 11.0 * if title { 28.0 } else if kind == "subtitle" { 15.0 } else { 11.0 };
        let anchor = match s.valign {
            VAlign::Top => "t",
            VAlign::Middle => "ctr",
            VAlign::Bottom => "b",
        };
        let face = font_name(if title { &theme.heading_font } else { &theme.body_font });
        let color = if kind == "subtitle" { r#"<a:solidFill><a:schemeClr val="tx1"><a:alpha val="62000"/></a:schemeClr></a:solidFill>"#.to_string() } else { r#"<a:solidFill><a:schemeClr val="tx1"/></a:solidFill>"#.to_string() };
        let _ = write!(
            out,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name="{}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>{}</p:nvSpPr><p:spPr>{}</p:spPr><p:txBody><a:bodyPr lIns="{INSET}" tIns="{INSET}" rIns="{INSET}" bIns="{INSET}" anchor="{anchor}"><a:normAutofit/></a:bodyPr><a:lstStyle><a:lvl1pPr><a:defRPr sz="{}"{}>{color}<a:latin typeface="{}"/></a:defRPr></a:lvl1pPr></a:lstStyle><a:p><a:r><a:rPr lang="en-US"/><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>"#,
            i + 2,
            esc(&s.name),
            ph_xml(Some(ph)),
            xfrm(s, "a:xfrm"),
            (size_pt * 100.0).round() as i64,
            if title { r#" b="1""# } else { "" },
            esc(&face),
            if title { "Click to add a title" } else { "Click to add text" }
        );
    }
    out
}

fn layout_meta(l: SlideLayout) -> (&'static str, &'static str) {
    match l {
        SlideLayout::Title => ("title", "Title Slide"),
        SlideLayout::TitleContent => ("obj", "Title and Content"),
        SlideLayout::Section => ("secHead", "Section Header"),
        SlideLayout::TwoContent => ("twoObj", "Two Content"),
        SlideLayout::TitleOnly => ("titleOnly", "Title Only"),
        SlideLayout::Blank => ("blank", "Blank"),
    }
}

fn notes_master_xml(size: [f32; 2]) -> String {
    let (w, h) = (6_858_000i64, 9_144_000i64);
    let img_w = w - 2 * 685_800;
    let img_h = (img_w as f64 * size[1] as f64 / size[0].max(1.0) as f64) as i64;
    format!(
        r#"{XML_HEAD}<p:notesMaster {NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>{GROUP}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg" idx="2"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="685800"/><a:ext cx="{img_w}" cy="{img_h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="12700"><a:solidFill><a:prstClr val="black"/></a:solidFill></a:ln></p:spPr></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" sz="quarter" idx="3"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="{}"/><a:ext cx="{img_w}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Notes</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap {CLR_MAP}/><p:notesStyle><a:lvl1pPr marL="0" algn="l"><a:defRPr sz="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr></p:notesStyle></p:notesMaster>"#,
        685_800 + img_h + 457_200,
        h - (685_800 + img_h + 457_200) - 685_800
    )
}

fn notes_xml(text: &str) -> String {
    let mut paras = String::new();
    for line in text.split('\n') {
        if line.is_empty() {
            paras.push_str(r#"<a:p><a:endParaRPr lang="en-US"/></a:p>"#);
        } else {
            let _ = write!(paras, r#"<a:p><a:r><a:rPr lang="en-US" dirty="0"/><a:t>{}</a:t></a:r></a:p>"#, esc(line));
        }
    }
    format!(
        r#"{XML_HEAD}<p:notes {NS}><p:cSld><p:spTree>{GROUP}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{paras}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"#
    )
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let (keep, mut warnings) = crate::pages_of_kind(doc, pages, PageKind::Deck, "PPTX");
    if keep.is_empty() {
        return Err("Nothing to write: a PowerPoint file holds slides, and none of the chosen pages is a deck.".into());
    }
    let decks: Vec<&Deck> = keep.iter().map(|i| doc.pages[*i].deck().unwrap()).collect();
    let first = decks[0];
    if decks.iter().any(|d| d.size != first.size) {
        warnings.push(format!("The decks have different slide sizes; the presentation uses \"{}\"'s.", doc.pages[keep[0]].name));
    }
    if decks.iter().any(|d| d.theme != first.theme) {
        warnings.push(format!("The decks have different themes; the presentation uses \"{}\"'s (slides keep their own colours where they set them).", doc.pages[keep[0]].name));
    }
    let size = first.size;
    let theme = &first.theme;
    let mut w = Writer {
        doc,
        pkg: Pkg { zip: ZipOut::new(), overrides: vec![], defaults: BTreeSet::new() },
        media_done: Default::default(),
        images: 0,
        charts: 0,
        warnings: vec![],
        svg: 0,
        broken_charts: 0,
    };
    w.pkg.defaults.insert(("rels".into(), "application/vnd.openxmlformats-package.relationships+xml".into()));
    w.pkg.defaults.insert(("xml".into(), "application/xml".into()));

    // Theme, master and layouts.
    w.pkg.part("ppt/theme/theme1.xml", Some("application/vnd.openxmlformats-officedocument.theme+xml"), &theme_xml(theme))?;
    let mut master_rels = Rels::default();
    let mut layout_ids = String::new();
    for (i, l) in SlideLayout::ALL.iter().enumerate() {
        let (ty, name) = layout_meta(*l);
        let path = format!("ppt/slideLayouts/slideLayout{}.xml", i + 1);
        let xml = format!(
            r#"{XML_HEAD}<p:sldLayout {NS} type="{ty}" preserve="1"><p:cSld name="{name}"><p:spTree>{GROUP}{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#,
            layout_shapes(*l, size, theme)
        );
        w.pkg.part(&path, Some(&format!("{CT}.slideLayout+xml")), &xml)?;
        let mut lr = Rels::default();
        lr.add("slideMaster", "../slideMasters/slideMaster1.xml");
        w.pkg.rels(&path, &lr)?;
        let rid = master_rels.add("slideLayout", &format!("../slideLayouts/slideLayout{}.xml", i + 1));
        let _ = write!(layout_ids, r#"<p:sldLayoutId id="{}" r:id="{rid}"/>"#, 2_147_483_649u64 + i as u64);
    }
    master_rels.add("theme", "../theme/theme1.xml");
    let title_sz = (16.0 / 11.0 * 28.0 * 100.0f32).round() as i64;
    let body_sz = 2200;
    let master = format!(
        r#"{XML_HEAD}<p:sldMaster {NS}><p:cSld><p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg><p:spTree>{GROUP}{}</p:spTree></p:cSld><p:clrMap {CLR_MAP}/><p:sldLayoutIdLst>{layout_ids}</p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr algn="l"><a:lnSpc><a:spcPct val="96000"/></a:lnSpc><a:buNone/><a:defRPr sz="{title_sz}" b="1"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mj-lt"/><a:ea typeface="+mj-ea"/><a:cs typeface="+mj-cs"/></a:defRPr></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr marL="0" indent="0" algn="l"><a:lnSpc><a:spcPct val="117000"/></a:lnSpc><a:buNone/><a:defRPr sz="{body_sz}"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr><a:lvl2pPr marL="457200" indent="0" algn="l"><a:buNone/><a:defRPr sz="2000"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl2pPr></p:bodyStyle><p:otherStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr><a:lvl1pPr marL="0" algn="l"><a:defRPr sz="1800"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>"#,
        solid(&theme.background),
        layout_shapes(SlideLayout::TitleContent, size, theme)
    );
    w.pkg.part("ppt/slideMasters/slideMaster1.xml", Some(&format!("{CT}.slideMaster+xml")), &master)?;
    w.pkg.rels("ppt/slideMasters/slideMaster1.xml", &master_rels)?;

    // Slides.
    let any_notes = decks.iter().any(|d| d.slides.iter().any(|s| !s.notes.trim().is_empty()));
    let mut pres_rels = Rels::default();
    pres_rels.add("slideMaster", "slideMasters/slideMaster1.xml");
    let notes_master_rid = if any_notes {
        w.pkg.part("ppt/theme/theme2.xml", Some("application/vnd.openxmlformats-officedocument.theme+xml"), &theme_xml(&DeckTheme::named("paper").unwrap()))?;
        w.pkg.part("ppt/notesMasters/notesMaster1.xml", Some(&format!("{CT}.notesMaster+xml")), &notes_master_xml(size))?;
        let mut r = Rels::default();
        r.add("theme", "../theme/theme2.xml");
        w.pkg.rels("ppt/notesMasters/notesMaster1.xml", &r)?;
        Some(pres_rels.add("notesMaster", "notesMasters/notesMaster1.xml"))
    } else {
        None
    };
    let mut slide_ids = String::new();
    let mut n = 0usize;
    let mut notes_n = 0usize;
    for deck in &decks {
        for slide in &deck.slides {
            n += 1;
            let path = format!("ppt/slides/slide{n}.xml");
            let mut rels = Rels::default();
            let li = SlideLayout::ALL.iter().position(|l| *l == slide.layout).unwrap_or(1);
            rels.add("slideLayout", &format!("../slideLayouts/slideLayout{}.xml", li + 1));
            let mut tree = String::new();
            let mut bodies = 0;
            for (k, s) in slide.shapes.iter().enumerate() {
                tree.push_str(&w.shape_xml(deck, slide, s, k + 2, &mut rels, &mut bodies));
            }
            let bg = match slide.background.as_deref().or((deck.theme.background != theme.background).then_some(deck.theme.background.as_str())) {
                Some(c) => format!("<p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg>", solid(c)),
                None => String::new(),
            };
            let xml = format!(
                r#"{XML_HEAD}<p:sld {NS}{}><p:cSld>{bg}<p:spTree>{GROUP}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
                if slide.hidden { r#" show="0""# } else { "" }
            );
            if !slide.notes.trim().is_empty() {
                notes_n += 1;
                let np = format!("ppt/notesSlides/notesSlide{notes_n}.xml");
                w.pkg.part(&np, Some(&format!("{CT}.notesSlide+xml")), &notes_xml(&slide.notes))?;
                let mut nr = Rels::default();
                nr.add("notesMaster", "../notesMasters/notesMaster1.xml");
                nr.add("slide", &format!("../slides/slide{n}.xml"));
                w.pkg.rels(&np, &nr)?;
                rels.add("notesSlide", &format!("../notesSlides/notesSlide{notes_n}.xml"));
            }
            w.pkg.part(&path, Some(&format!("{CT}.slide+xml")), &xml)?;
            w.pkg.rels(&path, &rels)?;
            let rid = pres_rels.add("slide", &format!("slides/slide{n}.xml"));
            let _ = write!(slide_ids, r#"<p:sldId id="{}" r:id="{rid}"/>"#, 255 + n);
        }
    }
    if n == 0 {
        return Err("Nothing to write: the chosen decks have no slides.".into());
    }
    pres_rels.add("presProps", "presProps.xml");
    pres_rels.add("viewProps", "viewProps.xml");
    pres_rels.add("theme", "theme/theme1.xml");
    pres_rels.add("tableStyles", "tableStyles.xml");
    let (cx, cy) = (emu(size[0]).clamp(914_400, 51_206_400), emu(size[1]).clamp(914_400, 51_206_400));
    let pres = format!(
        r#"{XML_HEAD}<p:presentation {NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>{}<p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/><p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr><a:lvl1pPr marL="0" algn="l"><a:defRPr sz="1800"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle></p:presentation>"#,
        notes_master_rid.map(|r| format!(r#"<p:notesMasterIdLst><p:notesMasterId r:id="{r}"/></p:notesMasterIdLst>"#)).unwrap_or_default()
    );
    w.pkg.part("ppt/presentation.xml", Some(&format!("{CT}.presentation.main+xml")), &pres)?;
    w.pkg.rels("ppt/presentation.xml", &pres_rels)?;
    w.pkg.part("ppt/presProps.xml", Some(&format!("{CT}.presProps+xml")), &format!("{XML_HEAD}<p:presentationPr {NS}/>"))?;
    w.pkg.part(
        "ppt/viewProps.xml",
        Some(&format!("{CT}.viewProps+xml")),
        &format!(r#"{XML_HEAD}<p:viewPr {NS}><p:normalViewPr><p:restoredLeft sz="15620"/><p:restoredTop sz="94660"/></p:normalViewPr><p:gridSpacing cx="76200" cy="76200"/></p:viewPr>"#),
    )?;
    w.pkg.part(
        "ppt/tableStyles.xml",
        Some(&format!("{CT}.tableStyles+xml")),
        &format!(r#"{XML_HEAD}<a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" def="{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}"/>"#),
    )?;
    // Document properties.
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let created = doc.meta.created.map(|c| c.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_else(|| now.clone());
    w.pkg.part(
        "docProps/core.xml",
        Some("application/vnd.openxmlformats-package.core-properties+xml"),
        &format!(
            r#"{XML_HEAD}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dcterms:created xsi:type="dcterms:W3CDTF">{created}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">{now}</dcterms:modified></cp:coreProperties>"#,
            esc(&doc.title),
            esc(&doc.meta.author)
        ),
    )?;
    w.pkg.part(
        "docProps/app.xml",
        Some("application/vnd.openxmlformats-officedocument.extended-properties+xml"),
        &format!(r#"{XML_HEAD}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>folio</Application><Slides>{n}</Slides><Notes>{notes_n}</Notes></Properties>"#),
    )?;
    let mut root = Rels::default();
    root.add("officeDocument", "ppt/presentation.xml");
    root.add("http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml");
    root.add("extended-properties", "docProps/app.xml");
    w.pkg.zip.add("_rels/.rels", root.xml())?;
    // Content types last: every part is known now.
    let mut ct = format!(r#"{XML_HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#);
    for (ext, ty) in &w.pkg.defaults {
        let _ = write!(ct, r#"<Default Extension="{ext}" ContentType="{ty}"/>"#);
    }
    for (part, ty) in &w.pkg.overrides {
        let _ = write!(ct, r#"<Override PartName="{part}" ContentType="{ty}"/>"#);
    }
    ct.push_str("</Types>");
    w.pkg.zip.add("[Content_Types].xml", ct)?;

    warnings.append(&mut w.warnings);
    if w.charts > 0 {
        warnings.push(format!(
            "{} {} written as PowerPoint charts with today's numbers: they no longer follow the sheet.",
            w.charts,
            if w.charts == 1 { "chart was" } else { "charts were" }
        ));
    }
    if w.broken_charts > 0 {
        warnings.push(format!("{} charts were left out: their sheet range is missing or empty.", w.broken_charts));
    }
    if w.svg > 0 {
        warnings.push(format!("{} SVG pictures were left out (save them as PNG to keep them).", w.svg));
    }
    let bytes = w.pkg.zip.finish()?;
    Ok((bytes, warnings))
}
