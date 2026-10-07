//! What the zip-of-XML formats share (XLSX, PPTX, ODS, ODP): reading parts and their
//! relationships, a small XML tree, writing zips, escaping, and colours.
//!
//! The tree keeps local names (`c:chart` is `chart`): writers don't all use the usual prefixes,
//! and inside one part the local names are unambiguous enough for what folio reads.

use std::borrow::Cow;
use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

// ---------------------------------------------------------------------------------------------
// XML tree

#[derive(Clone, Debug, Default)]
pub struct El {
    /// Local name (no prefix).
    pub name: String,
    /// Attributes with their qualified names (`r:id`, `val`).
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Node>,
}

#[derive(Clone, Debug)]
pub enum Node {
    El(El),
    Text(String),
}

impl El {
    /// An attribute by its exact (qualified) name.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// A prefixed attribute by local name, whatever its prefix (`r:id`, `r:embed`).
    pub fn attr_ns(&self, local: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k.split_once(':').is_some_and(|(p, l)| l == local && p != "xmlns")).map(|(_, v)| v.as_str())
    }

    /// An attribute by local name, prefixed or not (ODF's `table:name`, `office:value`).
    pub fn attr_any(&self, local: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == local || k.rsplit_once(':').is_some_and(|(p, l)| l == local && p != "xmlns")).map(|(_, v)| v.as_str())
    }

    pub fn attr_f64(&self, key: &str) -> Option<f64> {
        self.attr(key).and_then(|v| v.trim().parse().ok())
    }

    /// `val="1"`, `val="true"` or the attribute absent with `default`.
    pub fn attr_bool(&self, key: &str, default: bool) -> bool {
        match self.attr(key) {
            Some(v) => matches!(v.trim(), "1" | "true" | "on"),
            None => default,
        }
    }

    pub fn elements(&self) -> impl Iterator<Item = &El> {
        self.kids.iter().filter_map(|k| match k {
            Node::El(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&El> {
        self.elements().find(|e| e.name == name)
    }

    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// Follows child names down.
    pub fn path(&self, names: &[&str]) -> Option<&El> {
        let mut at = self;
        for n in names {
            at = at.child(n)?;
        }
        Some(at)
    }

    /// The first descendant with this name (depth first).
    pub fn find(&self, name: &str) -> Option<&El> {
        for e in self.elements() {
            if e.name == name {
                return Some(e);
            }
            if let Some(f) = e.find(name) {
                return Some(f);
            }
        }
        None
    }

    /// Every descendant with this name, in document order (not looking inside matches).
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a El>) {
        for e in self.elements() {
            if e.name == name {
                out.push(e);
            } else {
                e.find_all(name, out);
            }
        }
    }

    pub fn all(&self, name: &str) -> Vec<&El> {
        let mut out = vec![];
        self.find_all(name, &mut out);
        out
    }

    /// All the text inside, in order.
    pub fn text(&self) -> String {
        let mut s = String::new();
        self.collect_text(&mut s);
        s
    }

    fn collect_text(&self, s: &mut String) {
        for k in &self.kids {
            match k {
                Node::Text(t) => s.push_str(t),
                Node::El(e) => e.collect_text(s),
            }
        }
    }

    /// `val` of a child (`<c:barDir val="col"/>`).
    pub fn child_val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.attr("val"))
    }
}

fn start_el(e: &BytesStart) -> El {
    let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
    let attrs = e
        .attributes()
        .with_checks(false)
        .flatten()
        .map(|a| {
            let k = String::from_utf8_lossy(a.key.as_ref()).into_owned();
            let v = a.unescape_value().map(Cow::into_owned).unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned());
            (k, v)
        })
        .collect();
    El { name, attrs, kids: vec![] }
}

fn push_text(parent: &mut El, t: &str) {
    if t.is_empty() {
        return;
    }
    if let Some(Node::Text(prev)) = parent.kids.last_mut() {
        prev.push_str(t);
    } else {
        parent.kids.push(Node::Text(t.to_string()));
    }
}

/// Parses a whole XML part into its root element.
pub fn parse_xml(bytes: &[u8]) -> Result<El, String> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut r = Reader::from_reader(bytes);
    r.config_mut().trim_text(false);
    r.config_mut().check_end_names = false;
    let mut stack: Vec<El> = vec![El { name: "#document".into(), ..Default::default() }];
    let mut buf = Vec::new();
    loop {
        let ev = r.read_event_into(&mut buf).map_err(|e| format!("Broken XML at byte {}: {e}", r.buffer_position()))?;
        match ev {
            Event::Start(e) => stack.push(start_el(&e)),
            Event::Empty(e) => {
                let el = start_el(&e);
                stack.last_mut().unwrap().kids.push(Node::El(el));
            }
            Event::End(_) => {
                if stack.len() > 1 {
                    let el = stack.pop().unwrap();
                    stack.last_mut().unwrap().kids.push(Node::El(el));
                }
            }
            Event::Text(t) => {
                let s = t.xml_content().map(Cow::into_owned).unwrap_or_else(|_| String::from_utf8_lossy(&t).into_owned());
                push_text(stack.last_mut().unwrap(), &s);
            }
            Event::CData(t) => {
                let s = String::from_utf8_lossy(&t).into_owned();
                push_text(stack.last_mut().unwrap(), &s);
            }
            Event::GeneralRef(g) => {
                let s: String = if g.is_char_ref() {
                    g.resolve_char_ref().ok().flatten().map(String::from).unwrap_or_default()
                } else {
                    match g.decode().as_deref().unwrap_or("") {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        _ => String::new(),
                    }
                };
                push_text(stack.last_mut().unwrap(), &s);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    while stack.len() > 1 {
        let el = stack.pop().unwrap();
        stack.last_mut().unwrap().kids.push(Node::El(el));
    }
    let doc = stack.pop().unwrap();
    doc.kids.into_iter().find_map(|k| if let Node::El(e) = k { Some(e) } else { None }).ok_or_else(|| "Empty XML part.".to_string())
}

// ---------------------------------------------------------------------------------------------
// Reading packages

/// A relationship from one part to another (`_rels/*.rels`).
#[derive(Clone, Debug)]
pub struct Rel {
    pub id: String,
    /// The last segment of the type URI (`worksheet`, `image`, `slideLayout`…).
    pub kind: String,
    /// The target part's path inside the zip (resolved), or the URL when external.
    pub target: String,
    pub external: bool,
}

pub struct Package<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    names: Vec<String>,
}

impl<'a> Package<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Package<'a>, String> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("Not a zip package: {e}"))?;
        let names = zip.file_names().map(str::to_string).collect();
        Ok(Package { zip, names })
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    fn real_name(&self, path: &str) -> Option<String> {
        let path = path.trim_start_matches('/');
        if self.names.iter().any(|n| n == path) {
            return Some(path.to_string());
        }
        self.names.iter().find(|n| n.eq_ignore_ascii_case(path)).cloned()
    }

    pub fn has(&self, path: &str) -> bool {
        self.real_name(path).is_some()
    }

    /// A part's bytes (paths compare without case as a last resort).
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let name = self.real_name(path)?;
        let mut f = self.zip.by_name(&name).ok()?;
        let mut out = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut out).ok()?;
        Some(out)
    }

    pub fn xml(&mut self, path: &str) -> Option<El> {
        let b = self.read(path)?;
        parse_xml(&b).ok()
    }

    /// A part's relationships, targets resolved to part paths.
    pub fn rels(&mut self, part: &str) -> Vec<Rel> {
        let part = part.trim_start_matches('/');
        let (dir, file) = match part.rsplit_once('/') {
            Some((d, f)) => (d.to_string(), f.to_string()),
            None => (String::new(), part.to_string()),
        };
        let rels_path = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
        let Some(x) = self.xml(&rels_path) else { return vec![] };
        x.children("Relationship")
            .map(|r| {
                let external = r.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("external"));
                let target = r.attr("Target").unwrap_or("");
                Rel {
                    id: r.attr("Id").unwrap_or("").to_string(),
                    kind: r.attr("Type").unwrap_or("").rsplit('/').next().unwrap_or("").to_string(),
                    target: if external { target.to_string() } else { resolve_target(part, target) },
                    external,
                }
            })
            .collect()
    }

    /// The main part (`officeDocument` in `_rels/.rels`).
    pub fn main_part(&mut self) -> Option<String> {
        let rels = self.rels("");
        rels.into_iter().find(|r| r.kind == "officeDocument").map(|r| r.target)
    }
}

/// Resolves a relationship target against the part that holds the relationship.
pub fn resolve_target(part: &str, target: &str) -> String {
    let target = target.replace('\\', "/");
    if let Some(abs) = target.strip_prefix('/') {
        return normalize_path(abs);
    }
    let dir = part.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    normalize_path(&if dir.is_empty() { target } else { format!("{dir}/{target}") })
}

fn normalize_path(p: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    // Targets are URIs: %20 and friends.
    if joined.contains('%') { percent_decode(&joined) } else { joined }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------------------------
// Writing packages

pub struct ZipOut {
    w: zip::ZipWriter<Cursor<Vec<u8>>>,
}

impl Default for ZipOut {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipOut {
    pub fn new() -> Self {
        ZipOut { w: zip::ZipWriter::new(Cursor::new(Vec::new())) }
    }

    fn put(&mut self, name: &str, bytes: &[u8], method: zip::CompressionMethod) -> Result<(), String> {
        let opts = zip::write::SimpleFileOptions::default().compression_method(method);
        self.w.start_file(name, opts).map_err(|e| e.to_string())?;
        self.w.write_all(bytes).map_err(|e| e.to_string())
    }

    /// A deflated entry.
    pub fn add(&mut self, name: &str, bytes: impl AsRef<[u8]>) -> Result<(), String> {
        self.put(name, bytes.as_ref(), zip::CompressionMethod::Deflated)
    }

    /// An entry stored as is (ODF's `mimetype` must be).
    pub fn stored(&mut self, name: &str, bytes: impl AsRef<[u8]>) -> Result<(), String> {
        self.put(name, bytes.as_ref(), zip::CompressionMethod::Stored)
    }

    pub fn finish(self) -> Result<Vec<u8>, String> {
        self.w.finish().map(Cursor::into_inner).map_err(|e| e.to_string())
    }
}

/// Escapes text for XML content and attribute values, dropping characters XML can't hold.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Colours

/// `FF336699`, `336699` or `#336699` to `#336699` (lowercase).
pub fn hex_color(s: &str) -> Option<String> {
    let h = s.trim().trim_start_matches('#');
    let h = if h.len() == 8 { &h[2..] } else { h };
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) { Some(format!("#{}", h.to_ascii_lowercase())) } else { None }
}

/// `#rrggbb` to bytes.
pub fn rgb_of(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn hex_of(rgb: [f64; 3]) -> String {
    let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

fn to_hsl(rgb: [u8; 3]) -> (f64, f64, f64) {
    let [r, g, b] = rgb.map(|v| v as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l * 255.0; 3];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        if t < 0.0 {
            t += 1.0
        }
        if t > 1.0 {
            t -= 1.0
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [hue(h + 1.0 / 3.0) * 255.0, hue(h) * 255.0, hue(h - 1.0 / 3.0) * 255.0]
}

/// SpreadsheetML's `tint` (-1 darker … 1 lighter), applied to the luminance.
pub fn apply_tint(hex: &str, tint: f64) -> String {
    let Some(rgb) = rgb_of(hex) else { return hex.to_string() };
    if tint == 0.0 {
        return hex.to_string();
    }
    let (h, s, l) = to_hsl(rgb);
    let l = if tint < 0.0 { l * (1.0 + tint) } else { l * (1.0 - tint) + tint };
    hex_of(from_hsl(h, s, l.clamp(0.0, 1.0)))
}

/// DrawingML colour transforms (`lumMod`, `lumOff`, `tint`, `shade`; values in 1/1000 %).
pub fn apply_mods(hex: &str, el: &El) -> String {
    let Some(rgb) = rgb_of(hex) else { return hex.to_string() };
    let mut rgbf = rgb.map(|v| v as f64);
    let (mut lum_mod, mut lum_off) = (1.0, 0.0);
    for m in el.elements() {
        let v = m.attr_f64("val").unwrap_or(100_000.0) / 100_000.0;
        match m.name.as_str() {
            "lumMod" => lum_mod = v,
            "lumOff" => lum_off = v,
            "tint" => rgbf = rgbf.map(|c| c * v + 255.0 * (1.0 - v)),
            "shade" => rgbf = rgbf.map(|c| c * v),
            _ => {}
        }
    }
    if lum_mod != 1.0 || lum_off != 0.0 {
        let (h, s, l) = to_hsl(rgbf.map(|v| v.round().clamp(0.0, 255.0) as u8));
        rgbf = from_hsl(h, s, (l * lum_mod + lum_off).clamp(0.0, 1.0));
    }
    hex_of(rgbf)
}

/// The legacy 64-colour palette (`indexed` colours in SpreadsheetML).
pub const INDEXED: [&str; 64] = [
    "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff", "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff", "#800000", "#008000", "#000080",
    "#808000", "#800080", "#008080", "#c0c0c0", "#808080", "#9999ff", "#993366", "#ffffcc", "#ccffff", "#660066", "#ff8080", "#0066cc", "#ccccff", "#000080", "#ff00ff", "#ffff00", "#00ffff", "#800080",
    "#800000", "#008080", "#0000ff", "#00ccff", "#ccffff", "#ccffcc", "#ffff99", "#99ccff", "#ff99cc", "#cc99ff", "#ffcc99", "#3366ff", "#33cccc", "#99cc00", "#ffcc00", "#ff9900", "#ff6600", "#666699",
    "#969696", "#003366", "#339966", "#003300", "#333300", "#993300", "#993366", "#333399", "#333333",
];

/// A theme's colour scheme (`a:clrScheme`): name (`dk1`, `accent1`…) to `#rrggbb`.
pub fn theme_colors(theme: &El) -> Vec<(String, String)> {
    let Some(scheme) = theme.find("clrScheme") else { return vec![] };
    scheme
        .elements()
        .filter_map(|c| {
            let inner = c.elements().next()?;
            let hex = match inner.name.as_str() {
                "srgbClr" => hex_color(inner.attr("val")?)?,
                "sysClr" => hex_color(inner.attr("lastClr").unwrap_or(if inner.attr("val") == Some("window") { "FFFFFF" } else { "000000" }))?,
                _ => return None,
            };
            Some((c.name.clone(), hex))
        })
        .collect()
}

/// A theme's major (headings) and minor (body) latin fonts.
pub fn theme_fonts(theme: &El) -> (Option<String>, Option<String>) {
    let face = |n: &str| theme.find(n).and_then(|f| f.child("latin")).and_then(|l| l.attr("typeface")).filter(|t| !t.is_empty()).map(str::to_string);
    (face("majorFont"), face("minorFont"))
}

/// A font name to folio's families: `sans`, `serif`, `mono` or `display`.
pub fn family_of(font: &str) -> &'static str {
    let f = font.to_ascii_lowercase();
    if ["mono", "courier", "consolas", "menlo", "monaco", "code", "fira code", "source code", "inconsolata", "lucida console"].iter().any(|k| f.contains(k)) {
        "mono"
    } else if ["times", "georgia", "cambria", "garamond", "serif", "palatino", "book antiqua", "baskerville", "didot", "minion", "merriweather", "playfair", "libre baskerville", "constantia"]
        .iter()
        .any(|k| f.contains(k))
        && !f.contains("sans")
    {
        "serif"
    } else if ["chakra", "impact", "bebas", "oswald", "league gothic", "display", "anton"].iter().any(|k| f.contains(k)) {
        "display"
    } else {
        "sans"
    }
}

/// The face folio draws each family with, written into files so readers that have it (or
/// fetch it, like Google Slides) show the same text; others substitute their own.
pub fn font_name(family: &str) -> String {
    match family {
        "sans" => "IBM Plex Sans".into(),
        "serif" => "IBM Plex Serif".into(),
        "mono" => "IBM Plex Mono".into(),
        "display" => "Chakra Petch".into(),
        other => other.to_string(),
    }
}

/// `a` mixed with `b` (`t` = 0 is `a`, 1 is `b`).
pub fn mix(a: &str, b: &str, t: f64) -> String {
    match (rgb_of(a), rgb_of(b)) {
        (Some(x), Some(y)) => hex_of([0, 1, 2].map(|i| x[i] as f64 * (1.0 - t) + y[i] as f64 * t)),
        _ => a.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// DrawingML charts (`c:chartSpace`), shared by XLSX and PPTX

/// The kind of a chart part's first plot, whether it stacks, that plot's element, and whether
/// the chart combines several kinds.
pub fn chart_plot(chart: &El) -> Result<(folio_core::ChartKind, bool, &El, bool), String> {
    use folio_core::ChartKind;
    let plot = chart.child("plotArea").ok_or("it has no plot area")?;
    let types: Vec<&El> = plot.elements().filter(|e| e.name.ends_with("Chart")).collect();
    let first = *types.first().ok_or("it has no chart type")?;
    let kind = match first.name.as_str() {
        "barChart" | "bar3DChart" => {
            if first.child_val("barDir") == Some("bar") {
                ChartKind::Bar
            } else {
                ChartKind::Column
            }
        }
        "lineChart" | "line3DChart" | "stockChart" | "radarChart" => ChartKind::Line,
        "areaChart" | "area3DChart" => ChartKind::Area,
        "pieChart" | "pie3DChart" | "doughnutChart" | "ofPieChart" => ChartKind::Pie,
        "scatterChart" | "bubbleChart" => ChartKind::Scatter,
        "surfaceChart" | "surface3DChart" => ChartKind::Column,
        other => return Err(format!("{other} charts aren't supported")),
    };
    let stacked = matches!(first.child_val("grouping"), Some("stacked") | Some("percentStacked")) && matches!(kind, ChartKind::Column | ChartKind::Bar | ChartKind::Area);
    Ok((kind, stacked, first, types.len() > 1))
}

/// A chart's title text (rich text or a cached cell).
pub fn chart_title(chart: &El) -> String {
    let Some(t) = chart.child("title") else { return String::new() };
    let mut texts = vec![];
    if let Some(rich) = t.find("rich") {
        for p in rich.children("p") {
            texts.push(p.all("t").iter().map(|t| t.text()).collect::<String>());
        }
    } else if let Some(v) = t.find("v") {
        texts.push(v.text());
    }
    texts.join(" ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_and_entities() {
        let x = parse_xml(br#"<?xml version="1.0"?><a:root xmlns:a="x" r:id="rId1" id="5"><a:t>Tom &amp; Jerry &#233;</a:t><b/></a:root>"#).unwrap();
        assert_eq!(x.name, "root");
        assert_eq!(x.attr_ns("id"), Some("rId1"));
        assert_eq!(x.attr("id"), Some("5"));
        assert_eq!(x.child("t").unwrap().text(), "Tom & Jerry é");
        assert!(x.child("b").is_some());
    }

    #[test]
    fn targets() {
        assert_eq!(resolve_target("xl/worksheets/sheet1.xml", "../drawings/drawing1.xml"), "xl/drawings/drawing1.xml");
        assert_eq!(resolve_target("xl/workbook.xml", "/xl/worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve_target("ppt/slides/slide1.xml", "../media/my%20pic.png"), "ppt/media/my pic.png");
    }

    #[test]
    fn tints() {
        assert_eq!(apply_tint("#000000", 0.5), "#808080");
        assert_eq!(hex_color("FF336699").as_deref(), Some("#336699"));
    }
}
